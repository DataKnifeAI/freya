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
use std::time::Instant;
use tokio::fs;
use tokio::process::Command;
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
    started: Instant,
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
        started: Instant::now(),
    });
    let protected = Router::new()
        .route("/v1/loras", get(list_loras))
        .route("/v1/stats", get(stats))
        .route("/v1/images/generations", post(generate))
        .route("/v1/assets/install", post(install))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_worker_token,
        ));
    let app = Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let bind = format!("0.0.0.0:{}", cfg.port);
    info!(%bind, models = %cfg.models_dir.display(), "freya-worker listening (stub — swap for vLLM-Omni in prod)");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Health for local ops. Includes GPU/VRAM when `nvidia-smi` is available
/// (null inside GPU-less containers — freya-api may enrich from the host).
async fn health(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    Json(build_stats(&app).await)
}

/// Same payload as `/health` behind optional worker auth.
async fn stats(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    Json(build_stats(&app).await)
}

async fn build_stats(app: &App) -> serde_json::Value {
    let loras = count_entries(&app.models_dir.join("loras")).await;
    let checkpoints = count_entries(&app.models_dir.join("checkpoints")).await;
    let (vram, host) = collect_gpu_stats().await;
    json!({
        "ok": true,
        "modelsDir": app.models_dir.display().to_string(),
        "loras": loras,
        "checkpoints": checkpoints,
        "uptimeSec": app.started.elapsed().as_secs(),
        "modelLoaded": serde_json::Value::Null,
        "vram": vram,
        "host": host,
    })
}

/// Query NVIDIA GPUs via `nvidia-smi`. Returns `(vram, host)` JSON values (null if unavailable).
async fn collect_gpu_stats() -> (serde_json::Value, serde_json::Value) {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.used,memory.total,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await;
    let Ok(out) = out else {
        return (serde_json::Value::Null, serde_json::Value::Null);
    };
    if !out.status.success() {
        return (serde_json::Value::Null, serde_json::Value::Null);
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
        return (serde_json::Value::Null, serde_json::Value::Null);
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

async fn count_entries(dir: &Path) -> usize {
    let Ok(mut rd) = fs::read_dir(dir).await else {
        return 0;
    };
    let mut n = 0usize;
    while let Ok(Some(_)) = rd.next_entry().await {
        n += 1;
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
    // Visible stub PNG (no GPU) so gallery / local Generate can be smoke-tested.
    let png = stub_png_from_prompt(&req.prompt, 256, 256);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Json(json!({ "data": [{ "b64_json": b64 }] }))
}

/// Build a valid RGB PNG whose colors derive from the prompt (deterministic stub).
fn stub_png_from_prompt(prompt: &str, w: u32, h: u32) -> Vec<u8> {
    let mut hash: u32 = 2166136261;
    for b in prompt.as_bytes() {
        hash ^= u32::from(*b);
        hash = hash.wrapping_mul(16777619);
    }
    let r0 = ((hash >> 16) & 0xff) as u8;
    let g0 = ((hash >> 8) & 0xff) as u8;
    let b0 = (hash & 0xff) as u8;

    let mut raw = Vec::with_capacity(((w * 3 + 1) * h) as usize);
    for y in 0..h {
        raw.push(0); // filter: none
        for x in 0..w {
            let t = (x + y) as u8;
            raw.push(r0.wrapping_add(t));
            raw.push(g0.wrapping_add(t.wrapping_mul(3)));
            raw.push(b0.wrapping_add(t.wrapping_mul(5)));
        }
    }
    encode_png_rgb(w, h, &raw)
}

fn encode_png_rgb(w: u32, h: u32, raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB
    write_png_chunk(&mut out, b"IHDR", &ihdr);
    write_png_chunk(&mut out, b"IDAT", &deflate_zlib(raw));
    write_png_chunk(&mut out, b"IEND", &[]);
    out
}

fn write_png_chunk(out: &mut Vec<u8>, ctype: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ctype);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32_ieee(ctype, data).to_be_bytes());
}

/// Stored-block zlib wrapper (no external compress crate).
fn deflate_zlib(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16 + data.len().div_ceil(65535) * 5);
    out.extend_from_slice(&[0x78, 0x01]); // zlib cmf/flg — no compression
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + 65535).min(data.len());
        let chunk = &data[offset..end];
        let last = end == data.len();
        out.push(if last { 0x01 } else { 0x00 });
        let n = chunk.len() as u16;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(chunk);
        offset = end;
    }
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32_ieee(ctype: &[u8], data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &x in ctype.iter().chain(data.iter()) {
        crc ^= u32::from(x);
        for _ in 0..8 {
            let mask = (!(crc & 1)).wrapping_add(1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
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
