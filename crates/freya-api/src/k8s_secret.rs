//! Best-effort sync of the rotated studio API token into a Kubernetes Secret.
//!
//! When `FREYA_API_TOKEN_SECRET_NAME` is set and the pod has an in-cluster service
//! account that can patch that Secret, UI rotates stay aligned with MCP / env clients.

use anyhow::{anyhow, Context};
use base64::Engine;
use tracing::{info, warn};

/// Patch `data.<key>` on the named Secret. Returns `Ok(true)` when the Secret was updated,
/// `Ok(false)` when sync is not configured (no secret name), or `Err` on failure.
pub async fn sync_api_token_secret(token: &str) -> anyhow::Result<bool> {
    let secret_name = std::env::var("FREYA_API_TOKEN_SECRET_NAME").unwrap_or_default();
    if secret_name.trim().is_empty() {
        return Ok(false);
    }
    let secret_key =
        std::env::var("FREYA_API_TOKEN_SECRET_KEY").unwrap_or_else(|_| "token".into());
    let namespace = resolve_namespace()?;
    let sa_token = std::fs::read_to_string(
        "/var/run/secrets/kubernetes.io/serviceaccount/token",
    )
    .context("read in-cluster service account token")?;
    let host = std::env::var("KUBERNETES_SERVICE_HOST")
        .context("KUBERNETES_SERVICE_HOST (not in-cluster?)")?;
    let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".into());

    let ca_pem = std::fs::read("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt")
        .context("read service account CA")?;
    let cert = reqwest::Certificate::from_pem(&ca_pem).context("parse service account CA")?;
    let client = reqwest::Client::builder()
        .add_root_certificate(cert)
        .use_rustls_tls()
        .build()
        .context("build kubernetes API client")?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(token.as_bytes());
    let body = serde_json::json!({
        "data": { secret_key: b64 }
    });
    let url = format!(
        "https://{host}:{port}/api/v1/namespaces/{namespace}/secrets/{secret_name}"
    );
    let res = client
        .patch(&url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", sa_token.trim()),
        )
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/strategic-merge-patch+json",
        )
        .json(&body)
        .send()
        .await
        .context("patch secret")?;
    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(anyhow!(
            "patch secret {namespace}/{secret_name}: HTTP {status}: {text}"
        ));
    }
    info!(
        %namespace,
        secret = %secret_name,
        "synced rotated API token into Kubernetes Secret"
    );
    Ok(true)
}

fn resolve_namespace() -> anyhow::Result<String> {
    if let Ok(ns) = std::env::var("FREYA_NAMESPACE") {
        if !ns.trim().is_empty() {
            return Ok(ns);
        }
    }
    let ns = std::fs::read_to_string(
        "/var/run/secrets/kubernetes.io/serviceaccount/namespace",
    )
    .context("read in-cluster namespace")?;
    let ns = ns.trim().to_string();
    if ns.is_empty() {
        return Err(anyhow!("empty in-cluster namespace"));
    }
    Ok(ns)
}

/// Run sync and log failures without failing the token rotate response.
pub async fn try_sync_api_token_secret(token: &str) -> bool {
    match sync_api_token_secret(token).await {
        Ok(synced) => synced,
        Err(e) => {
            warn!(error = %e, "could not sync API token to Kubernetes Secret — use sync commands or scripts/sync-api-token-from-pvc.sh");
            false
        }
    }
}
