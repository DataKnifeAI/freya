//! Minimal diffusion worker: OpenAI-compatible images + asset install for Freya.
//! Production: replace with vLLM-Omni. This stub enables LoRA install + API wiring.

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use clap::Parser;
use serde::Deserialize;
use serde_json::json;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

#[derive(Parser, Clone)]
struct Config {
    #[arg(long, env = "FREYA_WORKER_PORT", default_value = "8091")]
    port: u16,
    #[arg(long, env = "FREYA_MODELS_DIR", default_value = "./data/models")]
    models_dir: PathBuf,
    /// Optional bearer token for `/v1/*`. Empty = open (local/dev only).
    #[arg(long, env = "FREYA_WORKER_TOKEN", default_value = "")]
    token: String,
}

#[derive(Clone)]
struct App {
    models_dir: PathBuf,
    token: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let cfg = Config::parse();
    if cfg.token.trim().is_empty() {
        warn!(
            "FREYA_WORKER_TOKEN unset — /v1 routes are open (local/dev only). \
             Set FreyaDiffusionWorker authSecretRef in-cluster."
        );
    } else {
        info!("FREYA_WORKER_TOKEN set — bearer auth required on /v1 routes");
    }
    fs::create_dir_all(cfg.models_dir.join("loras")).await?;
    fs::create_dir_all(cfg.models_dir.join("checkpoints")).await?;
    let state = Arc::new(App {
        models_dir: cfg.models_dir.clone(),
        token: cfg.token.trim().to_string(),
    });
    let protected = Router::new()
        .route("/v1/loras", get(list_loras))
        .route("/v1/images/generations", post(generate))
        .route("/v1/assets/install", post(install))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_worker_token,
        ));
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let bind = format!("0.0.0.0:{}", cfg.port);
    info!(%bind, models = %cfg.models_dir.display(), "freya-worker listening (stub — swap for vLLM-Omni in prod)");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn require_worker_token(
    State(app): State<Arc<App>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if app.token.is_empty() {
        return Ok(next.run(req).await);
    }
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let ok = auth
        .strip_prefix("Bearer ")
        .map(|t| t == app.token)
        .unwrap_or(false);
    if !ok {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}

async fn list_loras(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let mut data = Vec::new();
    let lora_dir = app.models_dir.join("loras");
    if let Ok(mut rd) = fs::read_dir(&lora_dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent.file_name().to_string_lossy().to_string();
            data.push(json!({
                "id": name,
                "name": name,
                "kind": "lora",
                "path": format!("loras/{name}"),
            }));
        }
    }
    Json(json!({ "data": data }))
}

#[derive(Deserialize)]
struct GenReq {
    prompt: String,
}

async fn generate(Json(req): Json<GenReq>) -> Json<serde_json::Value> {
    // 1x1 PNG stub so Freya gallery path works without a GPU.
    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xfe, 0xd4, 0xef, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let _ = req.prompt;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    Json(json!({ "data": [{ "b64_json": b64 }] }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallReq {
    url: String,
    dest_path: String,
}

/// Resolve `dest_path` under `models_dir`, rejecting absolute paths and `..` escape.
fn resolve_dest(models_dir: &Path, dest_path: &str) -> Result<PathBuf, String> {
    let rel = Path::new(dest_path);
    if dest_path.is_empty() || rel.is_absolute() {
        return Err("destPath must be a non-empty relative path under models dir".into());
    }
    if rel
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return Err("destPath must not contain '..' or absolute components".into());
    }
    let models_root = models_dir
        .canonicalize()
        .map_err(|e| format!("models dir: {e}"))?;
    let dest = models_root.join(rel);
    // Ensure the joined path still lives under models_root (defense in depth).
    if !dest.starts_with(&models_root) {
        return Err("destPath escapes models dir".into());
    }
    Ok(dest)
}

fn install_host_ok(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "civitai.com"
        || h.ends_with(".civitai.com")
        || h == "huggingface.co"
        || h.ends_with(".huggingface.co")
        || h == "hf.co"
        || h.ends_with(".hf.co")
}

fn install_url_ok(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "https" | "http") {
        return false;
    }
    parsed.host_str().is_some_and(install_host_ok)
}

fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            match attempt.url().host_str() {
                Some(h) if install_host_ok(h) => attempt.follow(),
                _ => attempt.error("redirect to disallowed host"),
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

async fn install(
    State(app): State<Arc<App>>,
    Json(req): Json<InstallReq>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let dest = resolve_dest(&app.models_dir, &req.dest_path).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": e })),
        )
    })?;
    if !install_url_ok(&req.url) {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "url must be http(s) on civitai.com, huggingface.co, or hf.co"
            })),
        ));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await.map_err(internal)?;
    }
    let client = download_client().map_err(internal)?;
    let resp = client.get(&req.url).send().await.map_err(internal)?;
    if !resp.status().is_success() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("download failed with HTTP {}", resp.status())
            })),
        ));
    }
    let bytes = resp.bytes().await.map_err(internal)?;
    fs::write(&dest, &bytes).await.map_err(internal)?;
    info!(path = %dest.display(), bytes = bytes.len(), "installed asset");
    Ok(Json(json!({ "ok": true, "path": req.dest_path })))
}

fn internal(e: impl std::fmt::Display) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(json!({ "error": e.to_string() })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reject_path_escape() {
        let dir = tempfile_dir();
        assert!(resolve_dest(&dir, "../etc/passwd").is_err());
        assert!(resolve_dest(&dir, "/etc/passwd").is_err());
        assert!(resolve_dest(&dir, "loras/../../etc/passwd").is_err());
        assert!(resolve_dest(&dir, "loras/ok.safetensors").is_ok());
    }

    #[test]
    fn allowlist_install_hosts() {
        assert!(install_url_ok(
            "https://civitai.com/api/download/models/1?token=x"
        ));
        assert!(install_url_ok(
            "https://huggingface.co/org/model/resolve/main/file.safetensors"
        ));
        assert!(!install_url_ok("https://evil.example/steal"));
        assert!(!install_url_ok("file:///etc/passwd"));
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("freya-worker-test-{}", std::process::id()));
        let _ = fs::create_dir_all(dir.join("loras"));
        dir
    }
}
