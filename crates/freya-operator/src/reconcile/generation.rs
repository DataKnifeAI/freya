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

    patch_status(
        &ctx,
        &ns,
        &name,
        StatusPatch {
            phase: "Running",
            ..Default::default()
        },
    )
    .await?;

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
        patch_status(
            &ctx,
            &ns,
            &name,
            StatusPatch {
                phase: "Failed",
                message: Some(msg.chars().take(500).collect()),
                ..Default::default()
            },
        )
        .await?;
        return Ok(Action::await_change());
    }

    let result: RunResponse = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
    patch_status(
        &ctx,
        &ns,
        &name,
        StatusPatch {
            phase: "Succeeded",
            backend: Some(result.backend),
            artifact: result.artifact_id.map(|id| format!("gallery/{id}")),
            enhanced_prompt: result.enhanced_prompt,
            message: None,
        },
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

#[derive(Default)]
struct StatusPatch {
    phase: &'static str,
    backend: Option<String>,
    artifact: Option<String>,
    enhanced_prompt: Option<String>,
    message: Option<String>,
}

async fn patch_status(ctx: &Ctx, ns: &str, name: &str, patch: StatusPatch) -> Result<(), Error> {
    let api: Api<FreyaGeneration> = Api::namespaced(ctx.client.clone(), ns);
    let status = serde_json::json!({
        "status": {
            "phase": patch.phase,
            "backend": patch.backend,
            "artifactPath": patch.artifact,
            "enhancedPrompt": patch.enhanced_prompt,
            "message": patch.message,
        }
    });
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
