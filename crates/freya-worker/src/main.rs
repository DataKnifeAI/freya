//! Minimal diffusion worker: OpenAI-compatible images + asset install for Freya.
//! Production: replace with vLLM-Omni. This stub enables LoRA install + API wiring.

use axum::extract::State;
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
use tracing::info;

#[derive(Parser, Clone)]
struct Config {
    #[arg(long, env = "FREYA_WORKER_PORT", default_value = "8091")]
    port: u16,
    #[arg(long, env = "FREYA_MODELS_DIR", default_value = "./data/models")]
    models_dir: PathBuf,
}

#[derive(Clone)]
struct App {
    models_dir: PathBuf,
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
    fs::create_dir_all(cfg.models_dir.join("loras")).await?;
    fs::create_dir_all(cfg.models_dir.join("checkpoints")).await?;
    let state = Arc::new(App {
        models_dir: cfg.models_dir.clone(),
    });
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/v1/loras", get(list_loras))
        .route("/v1/images/generations", post(generate))
        .route("/v1/assets/install", post(install))
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let bind = format!("0.0.0.0:{}", cfg.port);
    info!(%bind, models = %cfg.models_dir.display(), "freya-worker listening (stub — swap for vLLM-Omni in prod)");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
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
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await.map_err(internal)?;
    }
    let resp = reqwest::get(&req.url).await.map_err(internal)?;
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

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("freya-worker-test-{}", std::process::id()));
        let _ = fs::create_dir_all(dir.join("loras"));
        dir
    }
}
