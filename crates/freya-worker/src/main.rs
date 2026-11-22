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

async fn load_active_base(models_dir: &Path, env_model: &str) -> ActiveBase {
    let path = models_dir.join("active-base.json");
    if let Ok(bytes) = fs::read(&path).await {
        if let Ok(mut a) = serde_json::from_slice::<ActiveBase>(&bytes) {
            if a.model.is_empty() && !env_model.is_empty() {
                a.model = env_model.to_string();
            }
            return a;
        }
    }
    ActiveBase {
        model: env_model.to_string(),
        base_model: None,
    }
}

async fn persist_active_base(models_dir: &Path, active: &ActiveBase) -> Result<(), String> {
    let path = models_dir.join("active-base.json");
    let bytes = serde_json::to_vec_pretty(active).map_err(|e| e.to_string())?;
    fs::write(&path, bytes)
        .await
        .map_err(|e| format!("write active-base.json: {e}"))
}

/// Health for local ops. Includes GPU/VRAM when `nvidia-smi` is available
/// (null inside GPU-less containers — freya-api may enrich from the host).
async fn health() -> Json<Value> {
    Json(serde_json::json!({ "ok": true, "service": "freya-worker" }))
}

/// GPU, disk, and model inventory. Requires the worker token when one is set.
async fn stats(State(app): State<Arc<App>>) -> Json<Value> {
    Json(build_stats(&app).await)
}

async fn build_stats(app: &App) -> Value {
    let loras = count_weight_files(&app.models_dir.join("loras")).await;
    let checkpoints = count_weight_files(&app.models_dir.join("checkpoints")).await;
    let (vram, host) = collect_gpu_stats().await;
    let active = app.active.read().await.clone();
    let engine = engine_health(app).await;
    let (loaded_in_vram, engine_name) = match &engine {
        Some(e) => (
            e.get("loadedInVram").and_then(|v| v.as_bool()).unwrap_or(false),
            e.get("engine")
                .and_then(|v| v.as_str())
                .unwrap_or("diffusers")
                .to_string(),
        ),
        None if !app.engine_url.is_empty() => (false, "diffusers-unreachable".into()),
        None => (false, "stub".into()),
    };
    let model_loaded = if active.model.is_empty() {
        Value::Null
    } else {
        json!({
            "model": active.model,
            "baseModel": active.base_model,
            "loadedInVram": loaded_in_vram,
            "engine": engine_name,
        })
    };
    let disk = collect_path_disk(&app.models_dir);
    json!({
        "ok": true,
        "modelsDir": app.models_dir.display().to_string(),
        "modelsDirHost": app.models_dir_host.clone(),
        "loras": loras,
        "checkpoints": checkpoints,
        "uptimeSec": app.started.elapsed().as_secs(),
        "modelLoaded": model_loaded,
        "engine": engine,
        "vram": vram,
        "host": host,
        "disk": disk,
        "layout": {
            "loras": "loras/*.safetensors (+ optional *.safetensors.meta.json)",
            "checkpoints": "checkpoints/*.safetensors (+ optional *.meta.json)",
            "activeBase": "active-base.json",
        },
        "vramBehavior": "on-demand load during generate; unload after unless FREYA_ENGINE_KEEP_LOADED=1",
    })
}

/// df-style capacity for `FREYA_MODELS_DIR` via statvfs.
fn collect_path_disk(path: &Path) -> Value {
    use std::ffi::CString;
    let probe = if path.exists() {
        path.to_path_buf()
    } else if let Some(parent) = path.parent() {
        parent.to_path_buf()
    } else {
        return Value::Null;
    };
    let Some(s) = probe.to_str() else {
        return Value::Null;
    };
    let Ok(c_path) = CString::new(s) else {
        return Value::Null;
    };
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut st) != 0 {
            return Value::Null;
        }
        let bsize = st.f_frsize as u64;
        if bsize == 0 {
            return Value::Null;
        }
        let total = (st.f_blocks as u64).saturating_mul(bsize);
        let free = (st.f_bavail as u64).saturating_mul(bsize);
        let free_root = (st.f_bfree as u64).saturating_mul(bsize);
        let used = total.saturating_sub(free_root);
        let pct = if total > 0 {
            ((used as f64 / total as f64) * 100.0).round() as u64
        } else {
            0
        };
        json!({
            "path": path.display().to_string(),
            "usedBytes": used,
            "freeBytes": free,
            "totalBytes": total,
            "usedPercent": pct.min(100),
        })
    }
}

async fn engine_health(app: &App) -> Option<Value> {
    if app.engine_url.is_empty() {
        return None;
    }
    let url = format!("{}/health", app.engine_url);
    match app.http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok(),
        _ => None,
    }
}

/// Query NVIDIA GPUs via `nvidia-smi`. Returns `(vram, host)` JSON values (null if unavailable).
async fn collect_gpu_stats() -> (Value, Value) {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.used,memory.total,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await;
    let Ok(out) = out else {
        return (Value::Null, Value::Null);
    };
    if !out.status.success() {
        return (Value::Null, Value::Null);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut gpus = Vec::new();
    let mut used_total = 0u64;
    let mut mem_total = 0u64;
    let mut util_sum = 0u64;
    for line in text.lines() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 5 {
            continue;
        }
        let index: u64 = parts[0].parse().unwrap_or(0);
        let name = parts[1].to_string();
        let used: u64 = parts[2].parse().unwrap_or(0);
        let total: u64 = parts[3].parse().unwrap_or(0);
        let util: u64 = parts[4].parse().unwrap_or(0);
        used_total += used;
        mem_total += total;
        util_sum += util;
        gpus.push(json!({
            "index": index,
            "name": name,
            "usedMiB": used,
            "totalMiB": total,
            "utilPercent": util,
        }));
    }
    if gpus.is_empty() {
        return (Value::Null, Value::Null);
    }
    let n = gpus.len() as u64;
    let hostname = hostname_string();
    (
        json!({
            "usedMiB": used_total,
            "totalMiB": mem_total,
            "utilPercent": util_sum / n.max(1),
            "gpus": gpus,
        }),
        json!({
            "hostname": hostname,
            "source": "nvidia-smi",
        }),
    )
}

fn hostname_string() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "unknown".into())
}

async fn count_weight_files(dir: &Path) -> usize {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || count_weight_files_depth(&dir, 0))
        .await
        .unwrap_or(0)
}

fn count_weight_files_depth(dir: &Path, depth: u32) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0usize;
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if name.ends_with(".meta.json") || name.starts_with('.') {
            continue;
        }
        let Ok(ft) = ent.file_type() else {
            continue;
        };
        if ft.is_file() {
            n += 1;
        } else if ft.is_dir() && depth < 2 {
            // e.g. checkpoints/swarmui/ from optional SwarmUI bind-mount
            n += count_weight_files_depth(&ent.path(), depth + 1);
        }
    }
    n
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
    /// Optional download credential (Civitai/HF). Never embed secrets in `url` — reqwest errors echo URLs.
    #[serde(default)]
    auth_token: Option<String>,
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
        fs::create_dir_all(parent)
            .await
            .map_err(|e| internal("create dest dir", e))?;
    }
    let client = download_client().map_err(|e| internal("download client", e))?;
    let mut req_builder = client.get(&req.url);
    if let Some(token) = req
        .auth_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        req_builder = req_builder.bearer_auth(token);
    }
    let resp = req_builder
        .send()
        .await
        .map_err(|e| internal("download request", e))?;
    if !resp.status().is_success() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("download failed with HTTP {}", resp.status())
            })),
        ));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| internal("download body", e))?;
    fs::write(&dest, &bytes)
        .await
        .map_err(|e| internal("write asset", e))?;
    info!(path = %dest.display(), bytes = bytes.len(), "installed asset");
    Ok(Json(json!({ "ok": true, "path": req.dest_path })))
}

/// Never return raw transport errors — reqwest Display includes the full URL (and query secrets).
fn internal(
    context: &str,
    e: impl std::fmt::Display,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    warn!(%context, error = %e, "asset install failed");
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(json!({ "error": format!("{context} failed") })),
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
