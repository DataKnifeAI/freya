//! Reconcile FreyaGeneration — call Freya API /v1/run and update status.

use crate::Ctx;
use freya_crd::{FreyaGeneration, FreyaStudio, GenerationBackend};
use kube::api::{Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::{Api, ResourceExt};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

pub async fn reconcile(gen: Arc<FreyaGeneration>, ctx: Arc<Ctx>) -> Result<Action, Error> {
    let ns = gen.namespace().ok_or(Error::MissingNamespace)?;
    let name = gen.name_any();

    if gen.status.as_ref().and_then(|s| s.phase.as_deref()) == Some("Succeeded")
        || gen.status.as_ref().and_then(|s| s.phase.as_deref()) == Some("Failed")
    {
        return Ok(Action::await_change());
    }

    patch_phase(&ctx, &ns, &name, "Running", None, None, None).await?;

    let studio_name = gen
        .spec
        .studio_ref
        .clone()
        .unwrap_or_else(|| "freya".into());
    let studios: Api<FreyaStudio> = Api::namespaced(ctx.client.clone(), &ns);
    let studio = studios.get(&studio_name).await.map_err(Error::Kube)?;
    let api_base = studio
        .status
        .as_ref()
        .and_then(|s| s.api_endpoint.clone())
        .unwrap_or_else(|| format!("http://{studio_name}-api.{ns}.svc.cluster.local:8080"));

    let backend = match gen.spec.backend {
        GenerationBackend::Auto => "auto",
        GenerationBackend::Openrouter => "openrouter",
        GenerationBackend::Local => "local",
    };

    let body = serde_json::json!({
        "prompt": gen.spec.prompt,
        "negativePrompt": gen.spec.negative_prompt,
        "enhance": gen.spec.enhance,
        "backend": backend,
        "model": gen.spec.model,
        "styleRef": gen.spec.style_ref,
        "loras": gen.spec.loras,
        "size": gen.spec.size,
        "seed": gen.spec.seed,
    });

    info!(%ns, generation = %name, %api_base, "calling Freya API /v1/run");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let resp = client
        .post(format!("{api_base}/v1/run"))
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::Http(e.to_string()))?;

    if !resp.status().is_success() {
        let msg = resp.text().await.unwrap_or_default();
        patch_phase(
            &ctx,
            &ns,
            &name,
            "Failed",
            None,
            None,
            Some(msg.chars().take(500).collect()),
        )
        .await?;
        return Ok(Action::await_change());
    }

    let result: RunResponse = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
    patch_phase(
        &ctx,
        &ns,
        &name,
        "Succeeded",
        Some(result.backend),
        result.artifact_id.map(|id| format!("gallery/{id}")),
        result.enhanced_prompt,
    )
    .await?;

    Ok(Action::await_change())
}

#[derive(Debug, Deserialize)]
struct RunResponse {
    backend: String,
    #[serde(default, alias = "artifactId")]
    artifact_id: Option<String>,
    #[serde(default, alias = "enhancedPrompt")]
    enhanced_prompt: Option<String>,
}

async fn patch_phase(
    ctx: &Ctx,
    ns: &str,
    name: &str,
    phase: &str,
    backend: Option<String>,
    artifact: Option<String>,
    message: Option<String>,
) -> Result<(), Error> {
    let api: Api<FreyaGeneration> = Api::namespaced(ctx.client.clone(), ns);
    let status = serde_json::json!({
        "status": {
            "phase": phase,
            "backend": backend,
            "artifactPath": artifact,
            "message": message,
            "enhancedPrompt": serde_json::Value::Null,
        }
    });
    // Keep enhanced prompt if provided via message channel separately — simplify:
    let status = if let Some(ref art) = artifact {
        serde_json::json!({
            "status": {
                "phase": phase,
                "backend": backend,
                "artifactPath": art,
                "message": message,
            }
        })
    } else {
        status
    };
    api.patch_status(name, &PatchParams::default(), &Patch::Merge(&status))
        .await
        .map_err(Error::Kube)?;
    Ok(())
}

pub fn error_policy(gen: Arc<FreyaGeneration>, err: &Error, _ctx: Arc<Ctx>) -> Action {
    warn!(generation = %gen.name_any(), error = %err, "generation reconcile failed");
    Action::requeue(Duration::from_secs(20))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing namespace")]
    MissingNamespace,
    #[error("kube: {0}")]
    Kube(#[from] kube::Error),
    #[error("http: {0}")]
    Http(String),
}
