//! Freya diffusion worker: OpenAI-compatible images + asset install.
//! GPU path: Diffusers engine at `FREYA_ENGINE_URL` (see `deploy/worker/`,
//! `Dockerfile.worker`). Without an engine, generate falls back to a stub PNG
//! for wiring tests. vLLM-Omni remains the CRD engine label for external swaps.

use axum::extract::{Path as PathParam, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex, RwLock, Semaphore};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Parser, Clone)]
struct Config {
    #[arg(long, env = "FREYA_WORKER_PORT", default_value = "8091")]
    port: u16,
    #[arg(long, env = "FREYA_MODELS_DIR", default_value = "./data/models")]
    models_dir: PathBuf,
    /// Host path for the models bind-mount (UI hint). Empty = unknown.
    #[arg(long, env = "FREYA_MODELS_HOST_DIR", default_value = "")]
    models_dir_host: String,
    /// Optional default / active base (HF id or relative path under models dir).
    #[arg(long, env = "FREYA_WORKER_MODEL", default_value = "")]
    model: String,
    /// Optional bearer token for `/v1/*`. Empty = open (local/dev only).
    #[arg(long, env = "FREYA_WORKER_TOKEN", default_value = "")]
    token: String,
    /// Diffusers (or compatible) engine base URL. Empty = stub PNG generate.
    #[arg(long, env = "FREYA_ENGINE_URL", default_value = "")]
    engine_url: String,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallJob {
    id: String,
    status: String,
    url: String,
    dest_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    bytes_done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    created_at: u64,
    updated_at: u64,
}

struct JobEntry {
    job: InstallJob,
    cancel: Arc<AtomicBool>,
    /// Auth token kept out of serialized job state.
    auth_token: Option<String>,
    version_id: Option<i64>,
    model_id: Option<i64>,
    thumbnail_url: Option<String>,
}

struct JobStore {
    jobs: HashMap<String, JobEntry>,
    /// Newest-first ids for list (cap recent history).
    recent: VecDeque<String>,
}

const JOB_HISTORY_CAP: usize = 100;

#[derive(Clone)]
struct App {
    models_dir: PathBuf,
    /// Optional host bind-mount path (FREYA_MODELS_HOST_DIR) for UI.
    models_dir_host: Option<String>,
    token: String,
    /// Empty = stub generate (no GPU). Set by GPU image entrypoint.
    engine_url: String,
    started: Instant,
    active: Arc<RwLock<ActiveBase>>,
    http: reqwest::Client,
    jobs: Arc<Mutex<JobStore>>,
    job_tx: mpsc::UnboundedSender<String>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
    let engine_url = cfg.engine_url.trim().trim_end_matches('/').to_string();
    let engine_label = if engine_url.is_empty() {
        "stub"
    } else {
        "diffusers"
    };
    if engine_url.is_empty() {
        warn!("FREYA_ENGINE_URL unset — generate returns stub PNGs (no GPU weights)");
    } else {
        info!(%engine_url, "FREYA_ENGINE_URL set — generate proxies to Diffusers engine");
    }
    let models_dir_host = {
        let h = cfg.models_dir_host.trim();
        if h.is_empty() {
            None
        } else {
            Some(h.to_string())
        }
    };
    let (job_tx, job_rx) = mpsc::unbounded_channel::<String>();
    let jobs = Arc::new(Mutex::new(JobStore {
        jobs: HashMap::new(),
        recent: VecDeque::new(),
    }));
    let state = Arc::new(App {
        models_dir: cfg.models_dir.clone(),
        models_dir_host,
        token: cfg.token.trim().to_string(),
        engine_url,
        started: Instant::now(),
        active: Arc::new(RwLock::new(active)),
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .expect("reqwest client"),
        jobs: jobs.clone(),
        job_tx,
    });
    spawn_job_worker(state.clone(), job_rx);
    let protected = Router::new()
        .route("/v1/loras", get(list_loras))
        .route("/v1/checkpoints", get(list_checkpoints))
        .route("/v1/embeddings", get(list_embeddings))
        .route("/v1/vae", get(list_vaes))
        .route("/v1/base-model", get(get_base_model).put(put_base_model))
        .route("/v1/stats", get(stats))
        .route("/v1/images/generations", post(generate))
        .route("/v1/videos/generations", post(generate_video))
        .route("/v1/assets/install", post(install))
        .route("/v1/assets/jobs", get(list_jobs).post(enqueue_install))
        .route("/v1/assets/jobs/clear", post(clear_jobs))
        .route("/v1/assets/jobs/{id}", get(get_job))
        .route("/v1/assets/jobs/{id}/cancel", post(cancel_job))
        .route("/v1/assets", delete(delete_asset))
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
        engine = %engine_label,
        "freya-worker listening (checkpoints + LoRAs on disk; GPU via FREYA_ENGINE_URL)"
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

/// Liveness for probes. GPU, disk, and inventory are on authenticated `GET /v1/stats`.
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
            e.get("loadedInVram")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AssetMeta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    base_model: Option<String>,
    #[serde(default)]
    version_id: Option<i64>,
    /// Civitai model id when known (distinct from on-disk file stem `id`).
    #[serde(default)]
    model_id: Option<i64>,
    #[serde(default)]
    model_type: Option<String>,
    #[serde(default)]
    source: Option<String>,
    /// Preview image URL (Civitai) for Installed cards.
    #[serde(default)]
    thumbnail_url: Option<String>,
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
        let size_bytes = ent.metadata().await.ok().map(|m| m.len());
        let id = name.trim_end_matches(".safetensors").to_string();
        data.push(json!({
            "id": id,
            "name": meta.name.unwrap_or_else(|| id.clone()),
            "kind": kind,
            "path": path,
            "baseModel": meta.base_model,
            "versionId": meta.version_id,
            "modelId": meta.model_id,
            "modelType": meta.model_type,
            "sizeBytes": size_bytes,
            "source": meta.source,
            "thumbnailUrl": meta.thumbnail_url,
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

async fn list_embeddings(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({ "data": list_dir_assets(&app, "embeddings", "embedding").await }))
}

async fn list_vaes(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({ "data": list_dir_assets(&app, "vae", "vae").await }))
}

async fn get_base_model(State(app): State<Arc<App>>) -> Json<Value> {
    let active = app.active.read().await.clone();
    Json(json!({
        "ok": true,
        "model": if active.model.is_empty() { Value::Null } else { json!(active.model) },
        "baseModel": active.base_model,
        "modelsDir": app.models_dir.display().to_string(),
        "modelsDirHost": app.models_dir_host.clone(),
        "note": "Active base for generate; Diffusers engine loads weights into VRAM on demand.",
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
    // Drop any previously loaded weights so the next generate picks up the new base.
    if !app.engine_url.is_empty() {
        let url = format!("{}/v1/unload", app.engine_url);
        let _ = app.http.post(&url).send().await;
    }
    info!(model = %next.model, base = ?next.base_model, "set active base model");
    let engine = if app.engine_url.is_empty() {
        "stub"
    } else {
        "diffusers"
    };
    Ok(Json(json!({
        "ok": true,
        "model": next.model,
        "baseModel": next.base_model,
        "loadedInVram": false,
        "engine": engine,
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
    #[serde(default)]
    steps: Option<u32>,
    #[serde(default, alias = "cfg", alias = "guidance")]
    guidance_scale: Option<f64>,
    #[serde(default)]
    sampler: Option<String>,
    #[serde(default)]
    clip_skip: Option<u32>,
    #[serde(default, alias = "n")]
    quantity: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoGenReq {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    seed: Option<i64>,
    #[serde(default)]
    negative_prompt: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    frames: Option<u32>,
    #[serde(default)]
    steps: Option<u32>,
    #[serde(default, alias = "cfg", alias = "guidance")]
    guidance_scale: Option<f64>,
    #[serde(default)]
    sampler: Option<String>,
    #[serde(default)]
    clip_skip: Option<u32>,
}

/// Local motion: multi-frame GIF via Diffusers. True T2V (Wan/LTX) is OpenRouter / SwarmUI.
async fn generate_video(
    State(app): State<Arc<App>>,
    Json(req): Json<VideoGenReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let fmt = req.format.as_deref().unwrap_or("gif").to_ascii_lowercase();
    if fmt != "gif" {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "local worker video supports format=gif only; use OpenRouter for mp4 T2V"
            })),
        ));
    }
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
    if app.engine_url.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "FREYA_ENGINE_URL unset — no Diffusers engine for local gif" })),
        ));
    }
    let Some(model) = model else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no model for local gif" })),
        ));
    };
    let url = format!("{}/v1/generate_gif", app.engine_url);
    let body = json!({
        "prompt": req.prompt,
        "model": model,
        "baseModel": active.base_model,
        "size": req.size,
        "seed": req.seed,
        "negative_prompt": req.negative_prompt,
        "frames": req.frames.unwrap_or(6),
        "format": "gif",
        "steps": req.steps,
        "guidance_scale": req.guidance_scale,
        "sampler": req.sampler,
        "clipSkip": req.clip_skip,
    });
    info!(model = %model, frames = ?req.frames, "engine gif generate");
    let resp = app.http.post(&url).json(&body).send().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("engine unreachable: {e}") })),
        )
    })?;
    let status = resp.status();
    let mut payload: Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("engine response: {e}") })),
        )
    })?;
    if !status.is_success() {
        let detail = payload
            .get("detail")
            .cloned()
            .unwrap_or_else(|| json!(payload.to_string()));
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(json!({ "error": detail })),
        ));
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("stub".into(), json!(false));
        obj.insert("mime".into(), json!("image/gif"));
    }
    Ok(Json(payload))
}

async fn generate(
    State(app): State<Arc<App>>,
    Json(req): Json<GenReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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

    if !app.engine_url.is_empty() {
        let Some(model) = model.clone() else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "no model: set active base (PUT /v1/base-model) or pass model"
                })),
            ));
        };
        let url = format!("{}/v1/generate", app.engine_url);
        let body = json!({
            "prompt": req.prompt,
            "model": model,
            "baseModel": active.base_model,
            "lora_request": req.lora_request,
            "size": req.size,
            "seed": req.seed,
            "negative_prompt": req.negative_prompt,
            "steps": req.steps,
            "guidance_scale": req.guidance_scale,
            "sampler": req.sampler,
            "clipSkip": req.clip_skip,
            "quantity": req.quantity.unwrap_or(1).clamp(1, 4),
            "n": req.quantity.unwrap_or(1).clamp(1, 4),
        });
        info!(
            model = %model,
            loras = ?lora_names,
            size = ?req.size,
            seed = ?req.seed,
            "engine generate"
        );
        let resp = app.http.post(&url).json(&body).send().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("engine unreachable: {e}") })),
            )
        })?;
        let status = resp.status();
        let mut payload: Value = resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("engine response: {e}") })),
            )
        })?;
        if !status.is_success() {
            let detail = payload
                .get("detail")
                .cloned()
                .unwrap_or_else(|| json!(payload.to_string()));
            return Err((
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                Json(json!({ "error": detail })),
            ));
        }
        if let Some(obj) = payload.as_object_mut() {
            obj.entry("baseModel")
                .or_insert_with(|| json!(active.base_model));
            obj.entry("stub").or_insert(json!(false));
        }
        return Ok(Json(payload));
    }

    info!(
        model = ?model,
        loras = ?lora_names,
        size = ?req.size,
        seed = ?req.seed,
        steps = ?req.steps,
        "stub generate (weights not applied)"
    );
    // Visible stub PNG (no GPU) so gallery / local Generate can be smoke-tested.
    let n = req.quantity.unwrap_or(1).clamp(1, 4);
    let base_seed = req
        .seed
        .unwrap_or_else(|| (Uuid::new_v4().as_u128() as u64 % 2_147_483_646) as i64);
    let plan = stub_scheduler_note(req.sampler.as_deref());
    let mut data = Vec::new();
    for i in 0..n {
        let seed = base_seed.wrapping_add(i as i64);
        let png = stub_png_from_prompt(&format!("{}#{seed}", req.prompt), 256, 256);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        data.push(json!({ "b64_json": b64, "seed": seed }));
    }
    Ok(Json(json!({
        "data": data,
        "model": model,
        "baseModel": active.base_model,
        "loras": lora_names,
        "negativePrompt": req.negative_prompt,
        "stub": true,
        "steps": req.steps,
        "guidance_scale": req.guidance_scale,
        "cfg": req.guidance_scale,
        "sampler": req.sampler,
        "scheduler": plan.0,
        "samplerNote": plan.1,
        "clipSkip": req.clip_skip,
        "clipSkipApplied": false,
        "quantity": n,
        "seed": base_seed,
    })))
}

/// Stub has no Diffusers scheduler. Record the closest name the engine would load.
fn stub_scheduler_note(sampler: Option<&str>) -> (Option<String>, String) {
    let Some(name) = sampler.map(str::trim).filter(|s| !s.is_empty()) else {
        return (None, "stub engine; no sampler requested".into());
    };
    let key = name.to_ascii_lowercase();
    let (applied, sched) = if key.contains("euler")
        && (key.contains("ancestral") || key.ends_with(" a") || key == "euler a")
    {
        ("Euler a", "EulerAncestralDiscreteScheduler")
    } else if key == "euler" {
        ("Euler", "EulerDiscreteScheduler")
    } else if key.contains("dpm")
        && key.contains("2m")
        && key.contains("karras")
        && !key.contains("sde")
    {
        ("DPM++ 2M Karras", "DPMSolverMultistepScheduler")
    } else if key.contains("dpm") || key.contains("karras") {
        ("DPM++ 2M Karras", "DPMSolverMultistepScheduler")
    } else if key.contains("euler") {
        ("Euler", "EulerDiscreteScheduler")
    } else {
        ("Euler a", "EulerAncestralDiscreteScheduler")
    };
    let exact = key == "euler a"
        || key == "euler"
        || key.replace(' ', "") == "eulera"
        || key == "dpm++ 2m karras";
    let note = if exact {
        format!("stub engine; would apply {applied} ({sched})")
    } else {
        format!("requested {name}; stub engine would apply {applied} ({sched})")
    };
    (Some(sched.into()), note)
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
    model_id: Option<i64>,
    #[serde(default)]
    model_type: Option<String>,
    #[serde(default)]
    thumbnail_url: Option<String>,
    /// When true (default for `/v1/assets/jobs`), enqueue and return immediately.
    /// `/v1/assets/install` defaults to false for operator/sync callers.
    #[serde(default)]
    r#async: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClearJobsReq {
    /// When true, also remove queued jobs that have not started. Active downloads are never cleared.
    #[serde(default)]
    include_queued: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteAssetReq {
    path: String,
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

/// Civitai now serves some files from Cloudflare R2
/// (`civitai-delivery-worker-prod.<hash>.r2.cloudflarestorage.com`). Allow only as a
/// redirect target after an allowlisted civitai.com hop — never as a direct install URL.
fn civitai_r2_delivery_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h.contains("civitai") && h.ends_with(".r2.cloudflarestorage.com")
}

fn redirect_came_from_civitai(previous: &[reqwest::Url]) -> bool {
    previous.iter().any(|u| {
        u.host_str()
            .map(|h| {
                let h = h.to_ascii_lowercase();
                h == "civitai.com" || h.ends_with(".civitai.com")
            })
            .unwrap_or(false)
    })
}

fn redirect_host_ok(host: &str, previous: &[reqwest::Url]) -> bool {
    if install_host_ok(host) {
        return true;
    }
    civitai_r2_delivery_host(host) && redirect_came_from_civitai(previous)
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
                Some(h) if redirect_host_ok(h, attempt.previous()) => attempt.follow(),
                _ => attempt.error("redirect to disallowed host"),
            }
        }))
        .build()
        .map_err(|e| e.to_string())
}

/// Client-facing and logged text for a failed download.
/// `raw_display` is used only to classify redirect-policy failures. It is never
/// returned or logged: reqwest's Display includes the request URL and query.
fn download_failure_message(raw_display: &str, status: Option<reqwest::StatusCode>) -> String {
    if raw_display.contains("redirect to disallowed host") {
        "download request failed: redirect to disallowed host".to_string()
    } else if raw_display.contains("too many redirects") {
        "download request failed: too many redirects".to_string()
    } else if let Some(status) = status {
        format!("download failed with HTTP {}", status.as_u16())
    } else {
        "download request failed".to_string()
    }
}

/// Map reqwest transport errors to safe job messages (never echo URLs / query secrets).
fn download_request_err(e: reqwest::Error) -> String {
    let message = download_failure_message(&e.to_string(), e.status());
    warn!(error = %message, "download request failed");
    message
}

fn validate_install(app: &App, req: &InstallReq) -> Result<PathBuf, (StatusCode, Json<Value>)> {
    let dest = resolve_dest(&app.models_dir, &req.dest_path)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    if !install_url_ok(&req.url) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "url must be http(s) on civitai.com, huggingface.co, or hf.co"
            })),
        ));
    }
    Ok(dest)
}

fn job_to_json(job: &InstallJob) -> Value {
    serde_json::to_value(job).unwrap_or(Value::Null)
}

async fn enqueue_job(app: &App, req: InstallReq) -> Result<InstallJob, (StatusCode, Json<Value>)> {
    let dest = validate_install(app, &req)?;
    if dest.exists() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!("already installed at {} — delete it first", req.dest_path)
            })),
        ));
    }
    {
        let store = app.jobs.lock().await;
        for e in store.jobs.values() {
            if e.job.dest_path == req.dest_path
                && matches!(e.job.status.as_str(), "queued" | "downloading")
            {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": format!(
                            "already downloading to {} — wait or cancel the existing job",
                            req.dest_path
                        )
                    })),
                ));
            }
        }
    }
    let id = Uuid::new_v4().to_string();
    let now = now_unix();
    let job = InstallJob {
        id: id.clone(),
        status: "queued".into(),
        url: req.url.clone(),
        dest_path: req.dest_path.clone(),
        name: req.name.clone(),
        base_model: req.base_model.clone(),
        model_type: req.model_type.clone(),
        source: req.source.clone(),
        bytes_done: 0,
        bytes_total: None,
        percent: None,
        error: None,
        created_at: now,
        updated_at: now,
    };
    let entry = JobEntry {
        job: job.clone(),
        cancel: Arc::new(AtomicBool::new(false)),
        auth_token: req
            .auth_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        version_id: req.version_id,
        model_id: req.model_id,
        thumbnail_url: req.thumbnail_url.clone(),
    };
    {
        let mut store = app.jobs.lock().await;
        store.jobs.insert(id.clone(), entry);
        store.recent.push_front(id.clone());
        while store.recent.len() > JOB_HISTORY_CAP {
            if let Some(old) = store.recent.pop_back() {
                if let Some(e) = store.jobs.get(&old) {
                    if matches!(e.job.status.as_str(), "succeeded" | "failed" | "cancelled") {
                        store.jobs.remove(&old);
                    } else {
                        store.recent.push_back(old);
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }
    app.job_tx.send(id).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "job queue closed" })),
        )
    })?;
    Ok(job)
}

/// Sync install (operator / legacy). Prefer `POST /v1/assets/jobs` for UI/agent.
async fn install(
    State(app): State<Arc<App>>,
    Json(req): Json<InstallReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if req.r#async.unwrap_or(false) {
        let job = enqueue_job(&app, req).await?;
        return Ok(Json(json!({
            "ok": true,
            "queued": true,
            "job": job_to_json(&job),
            "jobId": job.id,
            "status": job.status,
            "path": job.dest_path,
            "name": job.name,
            "baseModel": job.base_model,
            "modelType": job.model_type,
        })));
    }
    let dest = validate_install(&app, &req)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let result = run_download(
        &app,
        &req.url,
        &req.dest_path,
        &dest,
        req.auth_token.as_deref(),
        &AssetMeta {
            name: req.name.clone(),
            base_model: req.base_model.clone(),
            version_id: req.version_id,
            model_id: req.model_id,
            model_type: req.model_type.clone(),
            source: req.source.clone(),
            thumbnail_url: req.thumbnail_url.clone(),
        },
        cancel,
        None,
    )
    .await;
    match result {
        Ok(bytes) => {
            info!(
                path = %dest.display(),
                bytes,
                base = ?req.base_model,
                "installed asset"
            );
            Ok(Json(json!({
                "ok": true,
                "path": req.dest_path,
                "baseModel": req.base_model,
                "name": req.name,
                "versionId": req.version_id,
                "modelId": req.model_id,
                "modelType": req.model_type,
                "bytes": bytes,
            })))
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(json!({ "error": e })))),
    }
}

async fn enqueue_install(
    State(app): State<Arc<App>>,
    Json(req): Json<InstallReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let job = enqueue_job(&app, req).await?;
    Ok(Json(json!({
        "ok": true,
        "queued": true,
        "job": job_to_json(&job),
        "jobId": job.id,
        "status": job.status,
    })))
}

async fn list_jobs(State(app): State<Arc<App>>) -> Json<Value> {
    let store = app.jobs.lock().await;
    let jobs: Vec<Value> = store
        .recent
        .iter()
        .filter_map(|id| store.jobs.get(id).map(|e| job_to_json(&e.job)))
        .collect();
    Json(json!({ "data": jobs }))
}

async fn get_job(
    State(app): State<Arc<App>>,
    PathParam(id): PathParam<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let store = app.jobs.lock().await;
    let entry = store.jobs.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "job not found" })),
        )
    })?;
    Ok(Json(json!({ "job": job_to_json(&entry.job) })))
}

async fn cancel_job(
    State(app): State<Arc<App>>,
    PathParam(id): PathParam<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut store = app.jobs.lock().await;
    let entry = store.jobs.get_mut(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "job not found" })),
        )
    })?;
    match entry.job.status.as_str() {
        "queued" => {
            entry.cancel.store(true, Ordering::SeqCst);
            entry.job.status = "cancelled".into();
            entry.job.updated_at = now_unix();
            entry.job.error = Some("cancelled before download".into());
        }
        "downloading" => {
            entry.cancel.store(true, Ordering::SeqCst);
            // Status flips to cancelled in the worker loop when it notices.
        }
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("cannot cancel job in status {other}")
                })),
            ));
        }
    }
    Ok(Json(json!({ "ok": true, "job": job_to_json(&entry.job) })))
}

/// Drop finished (and optionally idle queued) jobs from the recent list. Never touches in-flight downloads.
async fn clear_jobs(State(app): State<Arc<App>>, body: Option<Json<ClearJobsReq>>) -> Json<Value> {
    let include_queued = body.map(|Json(b)| b.include_queued).unwrap_or(false);
    let mut store = app.jobs.lock().await;
    let mut drop_ids: Vec<String> = Vec::new();
    let mut keep = VecDeque::new();
    for id in store.recent.iter() {
        let Some(entry) = store.jobs.get(id) else {
            continue;
        };
        let st = entry.job.status.as_str();
        let finished = matches!(st, "succeeded" | "failed" | "cancelled");
        let drop_queued = include_queued && st == "queued";
        if finished || drop_queued {
            drop_ids.push(id.clone());
        } else {
            keep.push_back(id.clone());
        }
    }
    for id in &drop_ids {
        store.jobs.remove(id);
    }
    let removed = drop_ids.len();
    store.recent = keep;
    let data: Vec<Value> = store
        .recent
        .iter()
        .filter_map(|id| store.jobs.get(id).map(|e| job_to_json(&e.job)))
        .collect();
    Json(json!({ "ok": true, "removed": removed, "data": data }))
}

async fn delete_asset(
    State(app): State<Arc<App>>,
    Json(req): Json<DeleteAssetReq>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let dest = resolve_dest(&app.models_dir, &req.path)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    if !dest.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "asset not found" })),
        ));
    }
    fs::remove_file(&dest).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("delete failed: {e}") })),
        )
    })?;
    let meta = meta_path_for(&dest);
    let _ = fs::remove_file(&meta).await;
    // Clear active base if we removed it.
    {
        let mut active = app.active.write().await;
        if active.model == req.path {
            let next = ActiveBase {
                model: String::new(),
                base_model: None,
            };
            let _ = persist_active_base(&app.models_dir, &next).await;
            *active = next;
        }
    }
    info!(path = %req.path, "deleted asset");
    Ok(Json(json!({ "ok": true, "path": req.path })))
}

/// Max concurrent Civitai/asset downloads (queue still accepts more; extras stay queued).
const MAX_CONCURRENT_DOWNLOADS: usize = 3;

fn spawn_job_worker(app: Arc<App>, mut rx: mpsc::UnboundedReceiver<String>) {
    tokio::spawn(async move {
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
        while let Some(id) = rx.recv().await {
            let permit = match sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => break,
            };
            let app = app.clone();
            tokio::spawn(async move {
                process_job(app, id).await;
                drop(permit);
            });
        }
    });
}

async fn process_job(app: Arc<App>, id: String) {
    let (url, dest_path, auth, meta, cancel) = {
        let mut store = app.jobs.lock().await;
        let Some(entry) = store.jobs.get_mut(&id) else {
            return;
        };
        if entry.cancel.load(Ordering::SeqCst) || entry.job.status == "cancelled" {
            entry.job.status = "cancelled".into();
            entry.job.updated_at = now_unix();
            return;
        }
        entry.job.status = "downloading".into();
        entry.job.updated_at = now_unix();
        (
            entry.job.url.clone(),
            entry.job.dest_path.clone(),
            entry.auth_token.clone(),
            AssetMeta {
                name: entry.job.name.clone(),
                base_model: entry.job.base_model.clone(),
                version_id: entry.version_id,
                model_id: entry.model_id,
                model_type: entry.job.model_type.clone(),
                source: entry.job.source.clone(),
                thumbnail_url: entry.thumbnail_url.clone(),
            },
            entry.cancel.clone(),
        )
    };
    let dest = match resolve_dest(&app.models_dir, &dest_path) {
        Ok(p) => p,
        Err(e) => {
            update_job_failed(&app, &id, e).await;
            return;
        }
    };
    let result = run_download(
        &app,
        &url,
        &dest_path,
        &dest,
        auth.as_deref(),
        &meta,
        cancel.clone(),
        Some(id.clone()),
    )
    .await;
    let mut store = app.jobs.lock().await;
    let Some(entry) = store.jobs.get_mut(&id) else {
        return;
    };
    entry.job.updated_at = now_unix();
    match result {
        Ok(bytes) => {
            if cancel.load(Ordering::SeqCst) {
                entry.job.status = "cancelled".into();
                entry.job.error = Some("cancelled".into());
                let _ = fs::remove_file(&dest).await;
            } else {
                entry.job.status = "succeeded".into();
                entry.job.bytes_done = bytes;
                if let Some(total) = entry.job.bytes_total {
                    entry.job.percent = Some(100.0);
                    let _ = total;
                } else {
                    entry.job.bytes_total = Some(bytes);
                    entry.job.percent = Some(100.0);
                }
                entry.job.error = None;
                info!(job = %id, path = %dest_path, bytes, "install job succeeded");
            }
        }
        Err(e) => {
            if cancel.load(Ordering::SeqCst) || e.contains("cancelled") {
                entry.job.status = "cancelled".into();
                entry.job.error = Some("cancelled".into());
            } else {
                entry.job.status = "failed".into();
                entry.job.error = Some(e);
            }
        }
    }
}

async fn update_job_failed(app: &App, id: &str, error: String) {
    let mut store = app.jobs.lock().await;
    if let Some(entry) = store.jobs.get_mut(id) {
        entry.job.status = "failed".into();
        entry.job.error = Some(error);
        entry.job.updated_at = now_unix();
    }
}

async fn update_job_progress(app: &App, id: &str, done: u64, total: Option<u64>) {
    let mut store = app.jobs.lock().await;
    if let Some(entry) = store.jobs.get_mut(id) {
        entry.job.bytes_done = done;
        entry.job.bytes_total = total.or(entry.job.bytes_total);
        entry.job.percent = entry.job.bytes_total.map(|t| {
            if t == 0 {
                0.0
            } else {
                (done as f64 / t as f64) * 100.0
            }
        });
        entry.job.updated_at = now_unix();
    }
}

/// Download to disk with optional progress + cancel. Returns bytes written.
#[allow(clippy::too_many_arguments)]
async fn run_download(
    app: &App,
    url: &str,
    dest_path: &str,
    dest: &Path,
    auth_token: Option<&str>,
    meta: &AssetMeta,
    cancel: Arc<AtomicBool>,
    job_id: Option<String>,
) -> Result<u64, String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|_| "create dest dir failed".to_string())?;
    }
    let client = download_client().map_err(|_| "download client failed".to_string())?;
    let mut req_builder = client.get(url);
    if let Some(token) = auth_token.map(str::trim).filter(|t| !t.is_empty()) {
        req_builder = req_builder.bearer_auth(token);
    }
    let mut resp = req_builder.send().await.map_err(download_request_err)?;
    if !resp.status().is_success() {
        return Err(format!("download failed with HTTP {}", resp.status()));
    }
    let total = resp.content_length();
    if let (Some(id), Some(t)) = (job_id.as_ref(), total) {
        update_job_progress(app, id, 0, Some(t)).await;
    }
    let tmp = {
        let mut s = dest.as_os_str().to_os_string();
        s.push(".partial");
        PathBuf::from(s)
    };
    let mut file = fs::File::create(&tmp)
        .await
        .map_err(|_| "create temp file failed".to_string())?;
    let mut done: u64 = 0;
    let mut last_report = 0u64;
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_file(&tmp).await;
            return Err("cancelled".into());
        }
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                file.write_all(&chunk)
                    .await
                    .map_err(|_| "write asset failed".to_string())?;
                done += chunk.len() as u64;
                if let Some(id) = job_id.as_ref() {
                    // Throttle progress updates (~256 KiB).
                    if done - last_report >= 256 * 1024 || total == Some(done) {
                        update_job_progress(app, id, done, total).await;
                        last_report = done;
                    }
                }
            }
            Ok(None) => break,
            Err(_) => {
                let _ = fs::remove_file(&tmp).await;
                return Err("download body failed".into());
            }
        }
    }
    file.flush()
        .await
        .map_err(|_| "flush asset failed".to_string())?;
    drop(file);
    if cancel.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&tmp).await;
        return Err("cancelled".into());
    }
    fs::rename(&tmp, dest)
        .await
        .map_err(|_| "finalize asset failed".to_string())?;

    if meta.name.is_some()
        || meta.base_model.is_some()
        || meta.version_id.is_some()
        || meta.model_id.is_some()
        || meta.model_type.is_some()
        || meta.source.is_some()
        || meta.thumbnail_url.is_some()
    {
        let meta_path = meta_path_for(dest);
        if let Ok(blob) = serde_json::to_vec_pretty(meta) {
            let _ = fs::write(&meta_path, blob).await;
        }
    }

    let is_checkpoint = dest_path.starts_with("checkpoints/")
        || meta
            .model_type
            .as_deref()
            .map(|t| t.eq_ignore_ascii_case("checkpoint"))
            .unwrap_or(false);
    if is_checkpoint {
        let mut active = app.active.write().await;
        if active.model.is_empty() {
            let next = ActiveBase {
                model: dest_path.to_string(),
                base_model: meta.base_model.clone(),
            };
            let _ = persist_active_base(&app.models_dir, &next).await;
            *active = next;
        }
    }
    if let Some(id) = job_id.as_ref() {
        update_job_progress(app, id, done, total.or(Some(done))).await;
    }
    Ok(done)
}

/// Never return or log raw transport errors — reqwest Display includes the full URL.
#[allow(dead_code)]
fn internal(context: &str, _detail: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    warn!(%context, error = "asset install failed", "asset install failed");
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
    fn download_failure_log_omits_url() {
        let leaked = "error sending request for url (https://civitai.com/api/download/models/1?token=secret): connection reset";
        let msg = download_failure_message(leaked, None);
        assert_eq!(msg, "download request failed");
        assert!(!msg.contains("token"));
        assert!(!msg.contains("http"));
        assert!(!msg.contains('?'));

        let redir = "error following redirect for url (https://evil.example/x?token=secret): redirect to disallowed host";
        let msg = download_failure_message(redir, None);
        assert_eq!(msg, "download request failed: redirect to disallowed host");
        assert!(!msg.contains("token"));
        assert!(!msg.contains("http"));
        assert!(!msg.contains('?'));

        let many = "error following redirect for url (https://huggingface.co/x?token=hf): too many redirects";
        let msg = download_failure_message(many, None);
        assert_eq!(msg, "download request failed: too many redirects");
        assert!(!msg.contains("token"));
        assert!(!msg.contains("huggingface"));

        let msg = download_failure_message(leaked, Some(reqwest::StatusCode::FORBIDDEN));
        assert_eq!(msg, "download failed with HTTP 403");
        assert!(!msg.contains("token"));
        assert!(!msg.contains("http"));
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
        // R2 delivery must not be accepted as a direct install URL (SSRF).
        assert!(!install_url_ok(
            "https://civitai-delivery-worker-prod.abc.r2.cloudflarestorage.com/file"
        ));
    }

    #[test]
    fn allowlist_civitai_r2_redirect_only() {
        let r2 = "civitai-delivery-worker-prod.5ac0637cfd0766c97916cefa3764fbdf.r2.cloudflarestorage.com";
        assert!(civitai_r2_delivery_host(r2));
        assert!(!civitai_r2_delivery_host(
            "evil-delivery.abc.r2.cloudflarestorage.com"
        ));
        assert!(!redirect_host_ok(r2, &[]));
        let from_civitai =
            vec![reqwest::Url::parse("https://civitai.com/api/download/models/1").unwrap()];
        assert!(redirect_host_ok(r2, &from_civitai));
        assert!(redirect_host_ok("b2.civitai.com", &from_civitai));
        let from_evil = vec![reqwest::Url::parse("https://evil.example/x").unwrap()];
        assert!(!redirect_host_ok(r2, &from_evil));
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
