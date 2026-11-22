//! Minimal diffusion worker: OpenAI-compatible images + asset install for Freya.
//! Production: replace with vLLM-Omni. This stub enables LoRA/checkpoint install +
//! base-model selection wiring (list / download / set active) without loading GPU weights.

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

#[derive(Parser, Clone)]
struct Config {
    #[arg(long, env = "FREYA_WORKER_PORT", default_value = "8091")]
    port: u16,
    #[arg(long, env = "FREYA_MODELS_DIR", default_value = "./data/models")]
    models_dir: PathBuf,
    /// Optional default / active base (HF id or relative path under models dir).
    #[arg(long, env = "FREYA_WORKER_MODEL", default_value = "")]
    model: String,
    /// Optional bearer token for `/v1/*`. Empty = open (local/dev only).
    #[arg(long, env = "FREYA_WORKER_TOKEN", default_value = "")]
    token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ActiveBase {
    /// HF id or relative path e.g. `checkpoints/foo.safetensors`.
    #[serde(default)]
    model: String,
    /// Civitai-style family label when known (SDXL 1.0, Pony, …).
    #[serde(default)]
    base_model: Option<String>,
}

#[derive(Clone)]
struct App {
    models_dir: PathBuf,
    token: String,
    started: Instant,
    active: Arc<RwLock<ActiveBase>>,
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
    let active = load_active_base(&cfg.models_dir, cfg.model.trim()).await;
    if !active.model.is_empty() {
        info!(
            model = %active.model,
            base = ?active.base_model,
            "active base model"
        );
    }
    let state = Arc::new(App {
        models_dir: cfg.models_dir.clone(),
        token: cfg.token.trim().to_string(),
        started: Instant::now(),
        active: Arc::new(RwLock::new(active)),
    });
    let protected = Router::new()
        .route("/v1/loras", get(list_loras))
        .route("/v1/checkpoints", get(list_checkpoints))
        .route(
            "/v1/base-model",
            get(get_base_model).put(put_base_model),
        )
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
    info!(
        %bind,
        models = %cfg.models_dir.display(),
        "freya-worker listening (stub — swap for vLLM-Omni in prod; hosts checkpoints + LoRAs on disk)"
    );
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
async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(build_stats(&app).await)
}

/// Same payload as `/health` behind optional worker auth.
async fn stats(State(app): State<Arc<App>>) -> Json<Value> {
    Json(build_stats(&app).await)
}

async fn build_stats(app: &App) -> Value {
    let loras = count_weight_files(&app.models_dir.join("loras")).await;
    let checkpoints = count_weight_files(&app.models_dir.join("checkpoints")).await;
    let (vram, host) = collect_gpu_stats().await;
    let active = app.active.read().await.clone();
    let model_loaded = if active.model.is_empty() {
        Value::Null
    } else {
        json!({
            "model": active.model,
            "baseModel": active.base_model,
            // Stub does not load weights into VRAM — production engine flips this.
            "loadedInVram": false,
            "engine": "stub",
        })
    };
    json!({
        "ok": true,
        "modelsDir": app.models_dir.display().to_string(),
        "loras": loras,
        "checkpoints": checkpoints,
        "uptimeSec": app.started.elapsed().as_secs(),
        "modelLoaded": model_loaded,
        "vram": vram,
        "host": host,
        "layout": {
            "loras": "loras/*.safetensors (+ optional *.safetensors.meta.json)",
            "checkpoints": "checkpoints/*.safetensors (+ optional *.meta.json)",
            "activeBase": "active-base.json",
        },
    })
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
    let Ok(mut rd) = fs::read_dir(dir).await else {
        return 0;
    };
    let mut n = 0usize;
    while let Ok(Some(ent)) = rd.next_entry().await {
        let name = ent.file_name().to_string_lossy().to_string();
        if name.ends_with(".meta.json") || name.starts_with('.') {
            continue;
        }
        if ent
            .file_type()
            .await
            .map(|t| t.is_file())
            .unwrap_or(false)
        {
            n += 1;
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AssetMeta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    base_model: Option<String>,
    #[serde(default)]
    version_id: Option<i64>,
    #[serde(default)]
    model_type: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

async fn read_meta_sidecar(weight_path: &Path) -> Option<AssetMeta> {
    let meta_path = meta_path_for(weight_path);
    let bytes = fs::read(&meta_path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn meta_path_for(weight_path: &Path) -> PathBuf {
    let mut s = weight_path.as_os_str().to_os_string();
    s.push(".meta.json");
    PathBuf::from(s)
}

async fn list_dir_assets(app: &App, subdir: &str, kind: &str) -> Vec<Value> {
    let mut data = Vec::new();
    let dir = app.models_dir.join(subdir);
    let Ok(mut rd) = fs::read_dir(&dir).await else {
        return data;
    };
    while let Ok(Some(ent)) = rd.next_entry().await {
        let name = ent.file_name().to_string_lossy().to_string();
        if name.ends_with(".meta.json") || name.starts_with('.') {
            continue;
        }
        let Ok(ft) = ent.file_type().await else {
            continue;
        };
        if !ft.is_file() {
            continue;
        }
        let path = format!("{subdir}/{name}");
        let full = app.models_dir.join(&path);
        let meta = read_meta_sidecar(&full).await.unwrap_or_default();
        let id = name.trim_end_matches(".safetensors").to_string();
        data.push(json!({
            "id": id,
            "name": meta.name.unwrap_or_else(|| id.clone()),
            "kind": kind,
            "path": path,
            "baseModel": meta.base_model,
            "versionId": meta.version_id,
            "modelType": meta.model_type,
        }));
    }
    data.sort_by(|a, b| {
        a.get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|x| x.as_str()).unwrap_or(""))
    });
    data
}

async fn list_loras(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({ "data": list_dir_assets(&app, "loras", "lora").await }))
}

async fn list_checkpoints(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({ "data": list_dir_assets(&app, "checkpoints", "checkpoint").await }))
}

async fn get_base_model(State(app): State<Arc<App>>) -> Json<Value> {
    let active = app.active.read().await.clone();
    Json(json!({
        "ok": true,
        "model": if active.model.is_empty() { Value::Null } else { json!(active.model) },
        "baseModel": active.base_model,
        "modelsDir": app.models_dir.display().to_string(),
        "note": "Stub selects the active base for generate wiring; GPU load happens in vLLM-Omni / real engine.",
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetBaseReq {
    /// HF id or relative path under models dir (e.g. checkpoints/foo.safetensors).
    model: String,
    #[serde(default)]
    base_model: Option<String>,
}

async fn put_base_model(
    State(app): State<Arc<App>>,
    Json(req): Json<SetBaseReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let model = req.model.trim().to_string();
    if model.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "model must be non-empty" })),
        ));
    }
    // If it looks like a relative checkpoint path, ensure it stays under models dir.
    if model.contains('/') || model.ends_with(".safetensors") {
        if let Err(e) = resolve_dest(&app.models_dir, &model) {
            return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": e }))));
        }
    }
    let next = ActiveBase {
        model: model.clone(),
        base_model: req.base_model.filter(|s| !s.trim().is_empty()),
    };
    persist_active_base(&app.models_dir, &next)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;
    *app.active.write().await = next.clone();
    info!(model = %next.model, base = ?next.base_model, "set active base model");
    Ok(Json(json!({
        "ok": true,
        "model": next.model,
        "baseModel": next.base_model,
        "loadedInVram": false,
        "engine": "stub",
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenReq {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    lora_request: Option<Value>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    seed: Option<i64>,
    #[serde(default)]
    negative_prompt: Option<String>,
}

async fn generate(State(app): State<Arc<App>>, Json(req): Json<GenReq>) -> Json<Value> {
    let active = app.active.read().await.clone();
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            if active.model.is_empty() {
                None
            } else {
                Some(active.model.clone())
            }
        });
    let lora_names: Vec<String> = req
        .lora_request
        .as_ref()
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| {
                    x.get("lora_name")
                        .or_else(|| x.get("loraName"))
                        .and_then(|n| n.as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    info!(
        model = ?model,
        loras = ?lora_names,
        size = ?req.size,
        seed = ?req.seed,
        "stub generate (weights not applied)"
    );
    // Visible stub PNG (no GPU) so gallery / local Generate can be smoke-tested.
    let png = stub_png_from_prompt(&req.prompt, 256, 256);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Json(json!({
        "data": [{ "b64_json": b64 }],
        "model": model,
        "baseModel": active.base_model,
        "loras": lora_names,
        "negativePrompt": req.negative_prompt,
        "stub": true,
    }))
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
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    base_model: Option<String>,
    #[serde(default)]
    version_id: Option<i64>,
    #[serde(default)]
    model_type: Option<String>,
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
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let dest = resolve_dest(&app.models_dir, &req.dest_path).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e })),
        )
    })?;
    if !install_url_ok(&req.url) {
        return Err((
            StatusCode::BAD_REQUEST,
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
            StatusCode::BAD_REQUEST,
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

    let meta = AssetMeta {
        name: req.name.clone(),
        base_model: req.base_model.clone(),
        version_id: req.version_id,
        model_type: req.model_type.clone(),
        source: req.source.clone(),
    };
    if meta.name.is_some()
        || meta.base_model.is_some()
        || meta.version_id.is_some()
        || meta.model_type.is_some()
        || meta.source.is_some()
    {
        let meta_path = meta_path_for(&dest);
        if let Ok(blob) = serde_json::to_vec_pretty(&meta) {
            let _ = fs::write(&meta_path, blob).await;
        }
    }

    // Auto-select newly installed checkpoints as active base when none set.
    let is_checkpoint = req
        .dest_path
        .starts_with("checkpoints/")
        || req
            .model_type
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("checkpoint"))
            .unwrap_or(false);
    if is_checkpoint {
        let mut active = app.active.write().await;
        if active.model.is_empty() {
            let next = ActiveBase {
                model: req.dest_path.clone(),
                base_model: req.base_model.clone(),
            };
            let _ = persist_active_base(&app.models_dir, &next).await;
            *active = next;
        }
    }

    info!(
        path = %dest.display(),
        bytes = bytes.len(),
        base = ?req.base_model,
        "installed asset"
    );
    Ok(Json(json!({
        "ok": true,
        "path": req.dest_path,
        "baseModel": req.base_model,
        "name": req.name,
        "versionId": req.version_id,
        "modelType": req.model_type,
    })))
}

/// Never return raw transport errors — reqwest Display includes the full URL (and query secrets).
fn internal(context: &str, e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    warn!(%context, error = %e, "asset install failed");
    (
        StatusCode::BAD_REQUEST,
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

    #[test]
    fn meta_sidecar_path() {
        let p = PathBuf::from("/models/checkpoints/foo.safetensors");
        assert_eq!(
            meta_path_for(&p),
            PathBuf::from("/models/checkpoints/foo.safetensors.meta.json")
        );
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("freya-worker-test-{}", std::process::id()));
        let _ = fs::create_dir_all(dir.join("loras"));
        let _ = dir.canonicalize();
        // canonicalize needs the dir to exist
        let _ = fs::create_dir_all(&dir);
        dir
    }
}
