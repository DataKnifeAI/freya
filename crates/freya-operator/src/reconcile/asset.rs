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
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

pub async fn reconcile(asset: Arc<FreyaModelAsset>, ctx: Arc<Ctx>) -> Result<Action, Error> {
    let ns = asset.namespace().ok_or(Error::MissingNamespace)?;
    let name = asset.name_any();

    if asset.status.as_ref().and_then(|s| s.phase.as_deref()) == Some("Succeeded") {
        return Ok(Action::await_change());
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
            install_remote(&asset, &endpoint).await?;
            patch_phase(
                &ctx,
                &ns,
                &name,
                "Succeeded",
                Some("installed on remote worker".into()),
            )
            .await?;
        }
        WorkerPlacement::InCluster => {
            info!(%ns, asset = %name, "creating download Job into worker models PVC");
            ensure_download_job(&ctx, &ns, &name, &asset, &worker, &owner).await?;
            patch_phase(
                &ctx,
                &ns,
                &name,
                "Succeeded",
                Some("download Job applied".into()),
            )
            .await?;
        }
    }

    Ok(Action::await_change())
}

async fn install_remote(asset: &FreyaModelAsset, endpoint: &str) -> Result<(), Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let body = serde_json::json!({
        "source": asset.spec.source,
        "url": asset.spec.url,
        "destPath": asset.spec.dest_path,
    });

    let resp = client
        .post(format!(
            "{}/v1/assets/install",
            endpoint.trim_end_matches('/')
        ))
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let msg = resp.text().await.unwrap_or_default();
        return Err(Error::Http(format!("remote install failed: {msg}")));
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
                        args: Some(vec![r#"set -e
mkdir -p "$(dirname "$MODELS_DIR/$DEST_PATH")"
AUTH=""
if [ -n "${ASSET_TOKEN:-}" ]; then AUTH="-H Authorization:\ Bearer\ $ASSET_TOKEN"; fi
eval curl -fL $AUTH -o "$MODELS_DIR/$DEST_PATH" "$ASSET_URL"
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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing namespace")]
    MissingNamespace,
    #[error("owner reference")]
    OwnerRef,
    #[error("worker has no endpoint")]
    MissingEndpoint,
    #[error("kube: {0}")]
    Kube(#[from] kube::Error),
    #[error("http: {0}")]
    Http(String),
}
