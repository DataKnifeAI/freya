//! Reconcile FreyaModelAsset — install LoRA/checkpoint onto worker (in-cluster Job or remote HTTP).

use crate::reconcile::{LABEL_INSTANCE, LABEL_MANAGED, LABEL_NAME, MANAGED_BY};
use crate::Ctx;
use freya_crd::{FreyaDiffusionWorker, FreyaModelAsset, WorkerPlacement};
use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, EnvVarSource, PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec,
    SecretKeySelector, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube::api::{Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::{Api, Resource, ResourceExt};
use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

pub async fn reconcile(asset: Arc<FreyaModelAsset>, ctx: Arc<Ctx>) -> Result<Action, Error> {
    let ns = asset.namespace().ok_or(Error::MissingNamespace)?;
    let name = asset.name_any();

    match asset.status.as_ref().and_then(|s| s.phase.as_deref()) {
        Some("Succeeded") | Some("Failed") => return Ok(Action::await_change()),
        _ => {}
    }

    patch_phase(&ctx, &ns, &name, "Installing", None).await?;

    let workers: Api<FreyaDiffusionWorker> = Api::namespaced(ctx.client.clone(), &ns);
    let worker = workers
        .get(&asset.spec.worker_ref)
        .await
        .map_err(Error::Kube)?;

    let owner = asset.controller_owner_ref(&()).ok_or(Error::OwnerRef)?;

    match worker.spec.placement {
        WorkerPlacement::External => {
            let endpoint = worker
                .spec
                .external_endpoint
                .clone()
                .or_else(|| worker.status.as_ref().and_then(|s| s.endpoint.clone()))
                .ok_or(Error::MissingEndpoint)?;

            info!(%ns, asset = %name, %endpoint, "installing LoRA/asset on remote worker");
            install_remote(&ctx, &ns, &asset, &worker, &endpoint).await?;
            patch_phase(
                &ctx,
                &ns,
                &name,
                "Succeeded",
                Some("installed on remote worker".into()),
            )
            .await?;
            Ok(Action::await_change())
        }
        WorkerPlacement::InCluster => {
            info!(%ns, asset = %name, "creating download Job into worker models PVC");
            ensure_download_job(&ctx, &ns, &name, &asset, &worker, &owner).await?;
            watch_download_job(&ctx, &ns, &name).await
        }
    }
}

async fn install_remote(
    ctx: &Ctx,
    ns: &str,
    asset: &FreyaModelAsset,
    worker: &FreyaDiffusionWorker,
    endpoint: &str,
) -> Result<(), Error> {
    sanitize_dest_path(&asset.spec.dest_path)?;
    let tls_insecure = worker
        .annotations()
        .get("freya.dataknife.ai/tls-insecure")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .danger_accept_invalid_certs(tls_insecure && super::tls_skip_allowed(endpoint))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let body = serde_json::json!({
        "source": asset.spec.source,
        "url": asset.spec.url,
        "destPath": asset.spec.dest_path,
    });

    let mut req = client
        .post(format!(
            "{}/v1/assets/install",
            endpoint.trim_end_matches('/')
        ))
        .json(&body);

    if let Some(auth) = &worker.spec.auth_secret_ref {
        let token = read_secret_key(ctx, ns, &auth.name, &auth.key).await?;
        req = req.bearer_auth(token);
    }

    let resp = req.send().await.map_err(|e| Error::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let msg = resp.text().await.unwrap_or_default();
        return Err(Error::Http(format!("remote install failed: {msg}")));
    }
    Ok(())
}

async fn read_secret_key(ctx: &Ctx, ns: &str, name: &str, key: &str) -> Result<String, Error> {
    use k8s_openapi::api::core::v1::Secret;
    let api: Api<Secret> = Api::namespaced(ctx.client.clone(), ns);
    let secret = api.get(name).await.map_err(Error::Kube)?;
    let data = secret
        .data
        .ok_or_else(|| Error::Http(format!("secret {name} has no data")))?;
    let bytes = data
        .get(key)
        .ok_or_else(|| Error::Http(format!("secret {name} missing key {key}")))?;
    String::from_utf8(bytes.0.clone())
        .map_err(|e| Error::Http(format!("secret {name}/{key} not utf8: {e}")))
}

/// Same download hosts as freya-worker install — required before attaching `secretRef`.
fn asset_download_host_ok(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "civitai.com"
        || h.ends_with(".civitai.com")
        || h == "huggingface.co"
        || h.ends_with(".huggingface.co")
        || h == "hf.co"
        || h.ends_with(".hf.co")
}

fn asset_download_url_ok(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "https" | "http") {
        return false;
    }
    parsed.host_str().is_some_and(asset_download_host_ok)
}

fn sanitize_asset_url(url: &str) -> Result<(), Error> {
    if !asset_download_url_ok(url) {
        return Err(Error::BadUrl(
            "url must be http(s) on civitai.com, huggingface.co, or hf.co".into(),
        ));
    }
    Ok(())
}

/// Reject absolute / `..` / shell-metachar dest paths; must stay under `/models`.
fn sanitize_dest_path(dest: &str) -> Result<(), Error> {
    if dest.is_empty() {
        return Err(Error::BadDest("destPath is empty".into()));
    }
    let path = Path::new(dest);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return Err(Error::BadDest(
            "destPath must be relative under /models (no '..')".into(),
        ));
    }
    if dest.chars().any(|c| {
        matches!(
            c,
            ';' | '|'
                | '&'
                | '$'
                | '`'
                | '\n'
                | '\r'
                | '"'
                | '\''
                | '\\'
                | '\0'
                | '('
                | ')'
                | '<'
                | '>'
                | '*'
                | '?'
                | '['
                | ']'
                | '{'
                | '}'
                | '!'
                | '#'
        )
    }) {
        return Err(Error::BadDest(
            "destPath contains disallowed characters".into(),
        ));
    }
    Ok(())
}

async fn ensure_download_job(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    asset: &FreyaModelAsset,
    worker: &FreyaDiffusionWorker,
    owner: &OwnerReference,
) -> Result<(), Error> {
    sanitize_dest_path(&asset.spec.dest_path)?;
    // Allowlist before mounting secretRef — prevents sending namespace secrets to attacker URLs.
    sanitize_asset_url(&asset.spec.url)?;
    let job_name = format!("freya-asset-{name}");
    let pvc = format!("{}-models", asset.spec.worker_ref);
    let mut env = vec![
        EnvVar {
            name: "ASSET_URL".into(),
            value: Some(asset.spec.url.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "DEST_PATH".into(),
            value: Some(asset.spec.dest_path.clone()),
            ..Default::default()
        },
        EnvVar {
            name: "MODELS_DIR".into(),
            value: Some("/models".into()),
            ..Default::default()
        },
    ];

    // Only mount secretRef after allowlist — never present namespace secrets to arbitrary hosts.
    if let Some(secret) = &asset.spec.secret_ref {
        env.push(EnvVar {
            name: "ASSET_TOKEN".into(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: secret.name.clone(),
                    key: secret.key.clone(),
                    optional: Some(true),
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
    }

    let _ = worker; // model path layout shared via PVC

    // No `eval`: curl argv stays literal; dest constrained under MODELS_DIR in-script + Rust.
    let job = Job {
        metadata: ObjectMeta {
            name: Some(job_name.clone()),
            namespace: Some(ns.to_string()),
            labels: Some(BTreeMap::from([
                (LABEL_MANAGED.to_string(), MANAGED_BY.to_string()),
                (LABEL_NAME.to_string(), "freya-asset".to_string()),
                (LABEL_INSTANCE.to_string(), name.to_string()),
            ])),
            owner_references: Some(vec![owner.clone()]),
            ..Default::default()
        },
        spec: Some(JobSpec {
            backoff_limit: Some(2),
            ttl_seconds_after_finished: Some(3600),
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(BTreeMap::from([
                        (LABEL_MANAGED.to_string(), MANAGED_BY.to_string()),
                        (LABEL_INSTANCE.to_string(), name.to_string()),
                    ])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    restart_policy: Some("OnFailure".into()),
                    containers: vec![Container {
                        name: "download".into(),
                        image: Some("curlimages/curl:8.11.1".into()),
                        command: Some(vec!["/bin/sh".into(), "-c".into()]),
                        args: Some(vec![r#"set -eu
case "$DEST_PATH" in
  /*|*..*)
    echo "refusing unsafe DEST_PATH" >&2
    exit 1
    ;;
esac
DEST="$MODELS_DIR/$DEST_PATH"
case "$DEST" in
  "$MODELS_DIR"/*) ;;
  *)
    echo "dest escapes models dir" >&2
    exit 1
    ;;
esac
# Defense in depth: refuse token attach / download outside allowlisted hosts.
case "$ASSET_URL" in
  https://civitai.com/*|http://civitai.com/*|https://*.civitai.com/*|http://*.civitai.com/*) ;;
  https://huggingface.co/*|http://huggingface.co/*|https://*.huggingface.co/*|http://*.huggingface.co/*) ;;
  https://hf.co/*|http://hf.co/*|https://*.hf.co/*|http://*.hf.co/*) ;;
  *)
    echo "refusing non-allowlisted ASSET_URL" >&2
    exit 1
    ;;
esac
mkdir -p "$(dirname "$DEST")"
if [ -n "${ASSET_TOKEN:-}" ]; then
  curl -fL --max-redirs 5 -H "Authorization: Bearer ${ASSET_TOKEN}" -o "$DEST" -- "$ASSET_URL"
else
  curl -fL --max-redirs 5 -o "$DEST" -- "$ASSET_URL"
fi
echo "installed $DEST_PATH"
"#
                        .into()]),
                        env: Some(env),
                        volume_mounts: Some(vec![VolumeMount {
                            name: "models".into(),
                            mount_path: "/models".into(),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![Volume {
                        name: "models".into(),
                        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                            claim_name: pvc,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let api: Api<Job> = Api::namespaced(ctx.client.clone(), ns);
    api.patch(
        &job_name,
        &PatchParams::apply(MANAGED_BY).force(),
        &Patch::Apply(&job),
    )
    .await
    .map_err(Error::Kube)?;
    Ok(())
}

async fn watch_download_job(ctx: &Ctx, ns: &str, name: &str) -> Result<Action, Error> {
    let job_name = format!("freya-asset-{name}");
    let api: Api<Job> = Api::namespaced(ctx.client.clone(), ns);
    let job = api.get(&job_name).await.map_err(Error::Kube)?;
    let status = job.status.as_ref();
    let succeeded = status.and_then(|s| s.succeeded).unwrap_or(0);
    let failed = status.and_then(|s| s.failed).unwrap_or(0);

    if succeeded > 0 {
        patch_phase(
            ctx,
            ns,
            name,
            "Succeeded",
            Some("download Job completed".into()),
        )
        .await?;
        return Ok(Action::await_change());
    }
    if failed > 0 {
        patch_phase(ctx, ns, name, "Failed", Some("download Job failed".into())).await?;
        return Ok(Action::await_change());
    }

    patch_phase(
        ctx,
        ns,
        name,
        "Installing",
        Some("waiting for download Job".into()),
    )
    .await?;
    Ok(Action::requeue(Duration::from_secs(15)))
}

async fn patch_phase(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    phase: &str,
    message: Option<String>,
) -> Result<(), Error> {
    let api: Api<FreyaModelAsset> = Api::namespaced(ctx.client.clone(), ns);
    let status = serde_json::json!({
        "status": {
            "phase": phase,
            "message": message,
        }
    });
    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status))
        .await
        .map_err(Error::Kube)?;
    Ok(())
}

pub fn error_policy(asset: Arc<FreyaModelAsset>, err: &Error, _ctx: Arc<Ctx>) -> Action {
    warn!(asset = %asset.name_any(), error = %err, "asset reconcile failed");
    Action::requeue(Duration::from_secs(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_asset_download_hosts() {
        assert!(asset_download_url_ok(
            "https://civitai.com/api/download/models/1?token=x"
        ));
        assert!(asset_download_url_ok(
            "https://huggingface.co/org/model/resolve/main/file.safetensors"
        ));
        assert!(sanitize_asset_url("https://hf.co/org/model").is_ok());
        assert!(sanitize_asset_url("https://evil.example/steal").is_err());
        assert!(sanitize_asset_url("file:///etc/passwd").is_err());
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing namespace")]
    MissingNamespace,
    #[error("owner reference")]
    OwnerRef,
    #[error("worker has no endpoint")]
    MissingEndpoint,
    #[error("bad destPath: {0}")]
    BadDest(String),
    #[error("bad url: {0}")]
    BadUrl(String),
    #[error("kube: {0}")]
    Kube(#[from] kube::Error),
    #[error("http: {0}")]
    Http(String),
}
