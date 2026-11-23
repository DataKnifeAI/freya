use crate::gallery::ArtifactMeta;
use crate::prefs::{looks_nsfw_label, AlignmentPrefs, AutoFallbacksPrefs, PrivacyPrefs};
use crate::state::{AppState, RuntimeSecrets};
use crate::styles::{self, StylePack};
use crate::worker::LoraWeight;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

type ApiResult<T> = Result<T, ApiError>;

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let (open_router, civitai, worker_enabled, worker_url) = {
        let cfg = state.config.read().await;
        (
            cfg.openrouter_configured(),
            cfg.civitai_configured(),
            cfg.worker_enabled,
            cfg.diffusion_worker_url.clone(),
        )
    };
    let worker_ok = {
        let w = state.worker.read().await;
        match w.as_ref() {
            Some(c) => c.health().await,
            None => false,
        }
    };
    Json(json!({
        "ok": true,
        "openRouter": open_router,
        "civitai": civitai,
        "diffusionWorker": worker_ok,
        "workerEnabled": worker_enabled,
        "workerConfigured": worker_url.is_some(),
    }))
}

/// Proxy worker GPU/runtime stats; enrich VRAM from host `nvidia-smi` when worker returns null.
pub async fn worker_stats(State(state): State<AppState>) -> impl IntoResponse {
    let (enabled, url, container, unit) = {
        let cfg = state.config.read().await;
        (
            cfg.worker_enabled,
            cfg.diffusion_worker_url.clone(),
            cfg.worker_container.clone(),
            cfg.worker_systemd_unit.clone(),
        )
    };
    let runtime = crate::worker_ctl::detect(&container, &unit).await;
    let mut worker_payload = Value::Null;
    let mut reachable = false;
    if enabled {
        let w = state.worker.read().await;
        if let Some(c) = w.as_ref() {
            if let Some(detail) = c.stats_detail().await {
                reachable = true;
                worker_payload = detail;
            }
        }
    } else if let Some(url) = url.as_ref() {
        // Still probe reachability for the panel when disabled (URL kept, client detached).
        let probe = crate::worker::DiffusionWorkerClient::new_with_tls(
            url.clone(),
            state.config.read().await.worker_tls_insecure,
        )
        .with_token(state.config.read().await.worker_token.clone());
        if let Some(detail) = probe.stats_detail().await {
            reachable = true;
            worker_payload = detail;
        }
    }

    let mut vram = worker_payload.get("vram").cloned().unwrap_or(Value::Null);
    let mut host = worker_payload.get("host").cloned().unwrap_or(Value::Null);
    if vram.is_null() {
        let host_vram = crate::gpu::collect_host_vram().await;
        if !host_vram.is_null() {
            vram = host_vram;
            if host.is_null() {
                host = json!({
                    "hostname": crate::gpu::hostname_string(),
                    "source": "host-nvidia-smi",
                });
            }
        }
    }

    let gallery_dir = state.config.read().await.gallery_dir.clone();
    let gallery_disk = crate::disk::collect_path_disk(&gallery_dir);
    let models_disk = worker_payload.get("disk").cloned().unwrap_or(Value::Null);

    Json(json!({
        "ok": true,
        "enabled": enabled,
        "configured": url.is_some(),
        "url": url,
        "reachable": reachable,
        "runtime": runtime.to_json(),
        "vram": vram,
        "host": host,
        "loras": worker_payload.get("loras").cloned().unwrap_or(Value::Null),
        "checkpoints": worker_payload.get("checkpoints").cloned().unwrap_or(Value::Null),
        "uptimeSec": worker_payload.get("uptimeSec").cloned().unwrap_or(Value::Null),
        "modelLoaded": worker_payload.get("modelLoaded").cloned().unwrap_or(Value::Null),
        "modelsDir": worker_payload.get("modelsDir").cloned().unwrap_or(Value::Null),
        "modelsDirHost": worker_payload.get("modelsDirHost").cloned().unwrap_or(Value::Null),
        "engine": worker_payload.get("engine").cloned().unwrap_or(Value::Null),
        "vramBehavior": worker_payload
            .get("vramBehavior")
            .cloned()
            .unwrap_or(Value::Null),
        "layout": worker_payload.get("layout").cloned().unwrap_or(Value::Null),
        "storage": {
            "gallery": gallery_disk,
            "models": models_disk,
        },
        "worker": worker_payload,
    }))
}

fn local_asset_json(a: crate::worker::LocalAsset) -> Value {
    json!({
        "id": a.id,
        "name": a.name.unwrap_or_else(|| a.id.clone()),
        "kind": a.kind,
        "path": a.path,
        "baseModel": a.base_model,
        "modelType": a.model_type,
        "versionId": a.version_id,
        "modelId": a.model_id,
        "sizeBytes": a.size_bytes,
        "source": a.source,
        "thumbnailUrl": a.thumbnail_url,
        "backend": "local",
    })
}

pub async fn worker_checkpoints(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let items = client.list_checkpoints().await.map_err(ApiError::from)?;
    Ok(Json(json!({
        "data": items.into_iter().map(local_asset_json).collect::<Vec<_>>(),
    })))
}

pub async fn worker_installed(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    installed_payload(&state).await.map(Json)
}

async fn installed_payload(state: &AppState) -> ApiResult<Value> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let loras = client.list_loras().await.map_err(ApiError::from)?;
    let checkpoints = client.list_checkpoints().await.map_err(ApiError::from)?;
    let embeddings = client.list_embeddings().await.unwrap_or_default();
    let vaes = client.list_vaes().await.unwrap_or_default();
    Ok(json!({
        "loras": loras.into_iter().map(local_asset_json).collect::<Vec<_>>(),
        "checkpoints": checkpoints.into_iter().map(local_asset_json).collect::<Vec<_>>(),
        "embeddings": embeddings.into_iter().map(local_asset_json).collect::<Vec<_>>(),
        "vaes": vaes.into_iter().map(local_asset_json).collect::<Vec<_>>(),
    }))
}

pub async fn worker_loras(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let items = client.list_loras().await.map_err(ApiError::from)?;
    Ok(Json(json!({
        "data": items.into_iter().map(local_asset_json).collect::<Vec<_>>(),
    })))
}

pub async fn worker_embeddings(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let items = client.list_embeddings().await.map_err(ApiError::from)?;
    Ok(Json(json!({
        "data": items.into_iter().map(local_asset_json).collect::<Vec<_>>(),
    })))
}

pub async fn worker_vaes(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let items = client.list_vaes().await.map_err(ApiError::from)?;
    Ok(Json(json!({
        "data": items.into_iter().map(local_asset_json).collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorkerAsset {
    pub path: String,
}

pub async fn worker_asset_delete(
    State(state): State<AppState>,
    Json(req): Json<DeleteWorkerAsset>,
) -> ApiResult<Json<Value>> {
    let path = req.path.trim();
    if path.is_empty() {
        return Err(ApiError::Msg("path required".into()));
    }
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let v = client.delete_asset(path).await.map_err(ApiError::from)?;
    Ok(Json(v))
}

pub async fn civitai_jobs_list(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let v = client.list_install_jobs().await.map_err(ApiError::from)?;
    Ok(Json(v))
}

pub async fn civitai_job_get(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let v = client.get_install_job(&id).await.map_err(|e| {
        if e.to_string().contains("not found") {
            ApiError::NotFound
        } else {
            ApiError::from(e)
        }
    })?;
    Ok(Json(v))
}

pub async fn civitai_job_cancel(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let v = client
        .cancel_install_job(&id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(v))
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClearCivitaiJobsRequest {
    /// When true, also drop idle queued jobs. Active downloads are never cleared.
    #[serde(default)]
    pub include_queued: bool,
}

pub async fn civitai_jobs_clear(
    State(state): State<AppState>,
    body: Option<Json<ClearCivitaiJobsRequest>>,
) -> ApiResult<Json<Value>> {
    let include_queued = body.map(|Json(b)| b.include_queued).unwrap_or(false);
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let v = client
        .clear_install_jobs(include_queued)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(v))
}

pub async fn worker_base_model_get(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let v = client.get_active_base().await.map_err(ApiError::from)?;
    Ok(Json(v))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetWorkerBaseModel {
    pub model: String,
    #[serde(default)]
    pub base_model: Option<String>,
}

pub async fn worker_base_model_put(
    State(state): State<AppState>,
    Json(req): Json<SetWorkerBaseModel>,
) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let v = client
        .set_active_base(&req.model, req.base_model.as_deref())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(v))
}

pub async fn base_models_catalog() -> Json<Value> {
    Json(json!({
        "items": crate::base_models::catalog(),
        "note": "Local GPU worker hosts SD/SDXL/Pony/Flux checkpoints under FREYA_MODELS_DIR. Remote LLM/vLLM text endpoints typically cannot run these.",
    }))
}

pub async fn worker_start(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let (container, unit) = {
        let cfg = state.config.read().await;
        (
            cfg.worker_container.clone(),
            cfg.worker_systemd_unit.clone(),
        )
    };
    let runtime = crate::worker_ctl::start(&container, &unit)
        .await
        .map_err(ApiError::Msg)?;
    // Brief wait so /health can come up before the UI polls.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    Ok(Json(json!({ "ok": true, "runtime": runtime.to_json() })))
}

pub async fn worker_stop(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let (container, unit) = {
        let cfg = state.config.read().await;
        (
            cfg.worker_container.clone(),
            cfg.worker_systemd_unit.clone(),
        )
    };
    let runtime = crate::worker_ctl::stop(&container, &unit)
        .await
        .map_err(ApiError::Msg)?;
    Ok(Json(json!({ "ok": true, "runtime": runtime.to_json() })))
}

pub async fn worker_enable(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    {
        let mut cfg = state.config.write().await;
        cfg.worker_enabled = true;
    }
    state.rebuild_worker_client().await;
    let _ = state.persist_secrets().await;
    Ok(Json(json!({ "ok": true, "enabled": true })))
}

pub async fn worker_disable(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    {
        let mut cfg = state.config.write().await;
        cfg.worker_enabled = false;
    }
    state.rebuild_worker_client().await;
    let _ = state.persist_secrets().await;
    Ok(Json(json!({ "ok": true, "enabled": false })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistRequest {
    pub mode: String,
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Optional prior turns for agentic session context.
    #[serde(default)]
    pub session: Option<serde_json::Value>,
}

fn strip_json_fence(raw: &str) -> &str {
    raw.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

pub async fn assist(
    State(state): State<AppState>,
    Json(req): Json<AssistRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let system = load_instruction_toned(&state, &req.mode).await;
    let user_msg = if let Some(sess) = &req.session {
        format!(
            "{}\n\n--- session context ---\n{}",
            req.message,
            serde_json::to_string_pretty(sess).unwrap_or_default()
        )
    } else {
        req.message.clone()
    };
    let or = state.openrouter.read().await;
    let text = or
        .chat(&system, &user_msg, req.model.as_deref())
        .await
        .map_err(ApiError::from)?;

    let mut out = json!({ "mode": req.mode, "text": text });

    // Structured modes: parse plan JSON and optionally search Civitai for LoRAs.
    if matches!(req.mode.as_str(), "suggest" | "session" | "plan") {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(strip_json_fence(&text)) {
            out["plan"] = parsed.clone();
            if let Some(ideas) = parsed.get("ideas") {
                out["ideas"] = ideas.clone();
            }
            if let Some(batch) = parsed.get("batchPrompts") {
                out["batchPrompts"] = batch.clone();
            }
            if let Some(note) = parsed.get("sessionNote").and_then(|n| n.as_str()) {
                out["sessionNote"] = json!(note);
            }
            if let Some(goal) = parsed.get("goal") {
                out["goal"] = goal.clone();
            }
            if let Some(steps) = parsed.get("steps") {
                out["steps"] = steps.clone();
            }
            let query = parsed
                .get("civitaiQuery")
                .and_then(|q| q.as_str())
                .map(str::trim)
                .filter(|q| !q.is_empty());
            if let Some(q) = query {
                out["civitaiQuery"] = json!(q);
                let civ = state.civitai.read().await;
                if civ.configured() {
                    match civ.search(q, 12).await {
                        Ok(items) => {
                            let mut assets: Vec<_> = items
                                .into_iter()
                                .filter(|c| crate::civitai::is_freya_asset_type(&c.model_type))
                                .collect();
                            assets.sort_by_key(|c| crate::civitai::asset_type_rank(&c.model_type));
                            assets.truncate(6);
                            out["loras"] = json!(assets);
                        }
                        Err(e) => {
                            out["civitaiError"] = json!(e.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(Json(out))
}

// ——— Conversational chat with tool actions (Plan / Auto / YOLO) ———

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageIn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatActionIn {
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub messages: Vec<ChatMessageIn>,
    #[serde(default)]
    pub session: Option<serde_json::Value>,
    #[serde(default)]
    pub automation_mode: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub image_model: Option<String>,
    /// Composer Cloud | Local preference: `openrouter` | `local` | `cloud` (optional).
    #[serde(default)]
    pub backend: Option<String>,
    /// Composer video model pin (`auto` or a provider/model id). Not an API key.
    #[serde(default)]
    pub video_model: Option<String>,
    /// When true, skip LLM planning and execute `actions` (Plan confirmation).
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub actions: Vec<ChatActionIn>,
    #[serde(default)]
    pub loras: Vec<LoraIn>,
}

#[derive(Debug, Clone)]
struct PlannedAction {
    action_type: String,
    args: serde_json::Value,
}

fn wants_image_intent(message: &str) -> bool {
    crate::router::infer_modality(message)
        .modality
        .is_still_image()
}

fn infer_chat_plan(message: &str) -> (String, Vec<PlannedAction>) {
    let m = message.to_lowercase();
    let mut actions = Vec::new();

    if m.contains("suggest") || m.contains("idea") || m.contains("brainstorm") {
        actions.push(PlannedAction {
            action_type: "suggest_ideas".into(),
            args: json!({ "topic": message }),
        });
    }
    if m.contains("civitai")
        || m.contains("search for lora")
        || m.contains("find a lora")
        || m.contains("find loras")
        || m.contains("search civitai")
    {
        let query = message
            .split_once("for ")
            .map(|(_, rest)| rest.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(message);
        actions.push(PlannedAction {
            action_type: "search_civitai".into(),
            args: json!({ "query": query.chars().take(80).collect::<String>() }),
        });
    }

    // Worker settings (Worker tab — safe API surface).
    let asks_worker_status = (m.contains("worker")
        && (m.contains("status")
            || m.contains("reachable")
            || m.contains("online")
            || m.contains("running")
            || m.contains("enabled")
            || m.contains("on?")
            || m.contains("off?")))
        || m.contains("is the worker")
        || m.contains("worker ok");
    let asks_list_loras = m.contains("list lora")
        || m.contains("list loras")
        || m.contains("installed lora")
        || m.contains("installed loras")
        || m.contains("what loras")
        || m.contains("which loras")
        || (m.contains("lora")
            && (m.contains("installed") || m.contains("on the worker") || m.contains("on disk")));
    let asks_list_checkpoints = m.contains("list checkpoint")
        || m.contains("list checkpoints")
        || m.contains("available checkpoint")
        || m.contains("which checkpoints")
        || m.contains("what checkpoints")
        || (m.contains("checkpoint")
            && (m.contains("list") || m.contains("show") || m.contains("available")));
    let asks_get_base = m.contains("active base")
        || m.contains("current base")
        || m.contains("what base")
        || m.contains("which base")
        || m.contains("base model")
        || m.contains("active checkpoint")
        || m.contains("current checkpoint");
    let asks_set_base = m.contains("set base")
        || m.contains("switch base")
        || m.contains("change base")
        || m.contains("use checkpoint")
        || m.contains("set checkpoint")
        || m.contains("set active base")
        || m.contains("switch checkpoint")
        || m.contains("change checkpoint");
    if asks_set_base {
        // Need a concrete path — list first so the user (or a follow-up) can pick.
        actions.push(PlannedAction {
            action_type: "list_checkpoints".into(),
            args: json!({}),
        });
        actions.push(PlannedAction {
            action_type: "get_base_model".into(),
            args: json!({}),
        });
    } else if asks_get_base {
        actions.push(PlannedAction {
            action_type: "get_base_model".into(),
            args: json!({}),
        });
    } else if asks_list_checkpoints {
        actions.push(PlannedAction {
            action_type: "list_checkpoints".into(),
            args: json!({}),
        });
    } else if asks_list_loras {
        actions.push(PlannedAction {
            action_type: "list_loras".into(),
            args: json!({}),
        });
    } else if asks_worker_status {
        actions.push(PlannedAction {
            action_type: "worker_status".into(),
            args: json!({}),
        });
    }

    if wants_image_intent(message) {
        if m.contains("run session") || m.contains("variations") || m.contains("batch") {
            actions.push(PlannedAction {
                action_type: "run_session".into(),
                args: json!({ "goal": message }),
            });
        } else {
            actions.push(PlannedAction {
                action_type: "generate_image".into(),
                args: json!({ "prompt": message, "batch": false }),
            });
        }
    }

    let reply = if actions.is_empty() {
        format!(
            "Happy to talk through it. Tell me when you want me to generate, search Civitai, tweak the worker base, or suggest ideas — I’ll wait until you ask."
        )
    } else if actions
        .iter()
        .any(|a| a.action_type == "generate_image" || a.action_type == "run_session")
    {
        "On it — I’ll make that image for you.".into()
    } else if actions.iter().any(|a| a.action_type == "search_civitai") {
        "I’ll search Civitai for matching assets.".into()
    } else if actions.iter().any(|a| {
        matches!(
            a.action_type.as_str(),
            "worker_status"
                | "list_checkpoints"
                | "list_loras"
                | "get_base_model"
                | "set_base_model"
        )
    }) {
        "Checking worker settings…".into()
    } else {
        "Here are some directions we could take.".into()
    };
    (reply, actions)
}

/// Cloud chat models often answer a refusal instead of a scene. That text must
/// not become the local diffusion prompt.
fn looks_like_model_refusal(text: &str) -> bool {
    let l = text
        .to_ascii_lowercase()
        .replace(['\u{2019}', '\u{2018}', '`'], "'");
    const MARKERS: &[&str] = &[
        "i can't",
        "i cannot",
        "i won't",
        "i will not",
        "i'm not able",
        "not able to",
        "can't help",
        "cannot help",
        "can't create",
        "cannot create",
        "can't generate",
        "cannot generate",
        "won't generate",
        "will not generate",
        "against my",
        "content policy",
        "i must refuse",
        "unable to create",
        "unable to generate",
        "as an ai",
    ];
    MARKERS.iter().any(|m| l.contains(m))
}

/// Drop a label or fence so the worker sees the keyword prompt.
fn normalize_sd_rewrite(text: &str) -> String {
    let mut t = text.trim().trim_matches('`').trim().to_string();
    if t.len() >= 2 {
        let bytes = t.as_bytes();
        let wrap = (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'');
        if wrap {
            t = t[1..t.len() - 1].trim().to_string();
        }
    }
    let lower = t.to_ascii_lowercase();
    for prefix in ["positive prompt:", "sd prompt:", "prompt:"] {
        if lower.starts_with(prefix) {
            t = t[prefix.len()..].trim().to_string();
            break;
        }
    }
    t
}

/// Adult local rewrite must be a scene prompt: not a refusal, not the raw line.
fn adult_rewrite_ok(text: &str, original: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || looks_like_model_refusal(t) || t.eq_ignore_ascii_case(original.trim()) {
        return false;
    }
    t.contains(',') || t.split_whitespace().count() >= 12
}

fn restore_refused_local_prompt(action: &mut PlannedAction, user_message: &str) {
    let key = match action.action_type.as_str() {
        "generate_image" | "generate_video" => "prompt",
        "run_session" => "goal",
        _ => return,
    };
    let current = action
        .args
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if current.is_empty() || looks_like_model_refusal(current) {
        if let Some(obj) = action.args.as_object_mut() {
            obj.insert(key.to_string(), json!(user_message));
        }
    }
}

fn parse_chat_plan(text: &str, fallback_message: &str) -> (String, Vec<PlannedAction>) {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(strip_json_fence(text)) {
        let reply = parsed
            .get("reply")
            .and_then(|r| r.as_str())
            .unwrap_or(text)
            .to_string();
        let mut actions = Vec::new();
        if let Some(arr) = parsed.get("actions").and_then(|a| a.as_array()) {
            for a in arr {
                let ty = a
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                if ty.is_empty() {
                    continue;
                }
                let args = a.get("args").cloned().unwrap_or(json!({}));
                actions.push(PlannedAction {
                    action_type: ty,
                    args,
                });
            }
        }
        return (reply, actions);
    }
    // Model returned prose — keep it, infer tools from user intent.
    let (fallback_reply, inferred) = infer_chat_plan(fallback_message);
    if inferred.is_empty() {
        (text.trim().to_string(), vec![])
    } else if text.trim().is_empty() {
        (fallback_reply, inferred)
    } else {
        (text.trim().to_string(), inferred)
    }
}

fn is_side_effect_action(ty: &str) -> bool {
    matches!(
        ty,
        "generate_image"
            | "generate_video"
            | "install_asset"
            | "cancel_install_job"
            | "clear_install_jobs"
            | "delete_installed"
            | "run_session"
            | "set_base_model"
            | "set_chat_models"
            | "set_cost_tier"
            | "set_alignment"
            | "set_privacy"
            | "set_generate_backend"
            | "set_automation_mode"
            | "favorite_asset"
            | "unfavorite_asset"
            | "pin_lora"
            | "unpin_lora"
            | "gallery_delete"
            | "worker_start"
            | "worker_stop"
    )
}

fn style_ref_from_args(args: &Value) -> Option<String> {
    args.get("styleRef")
        .or_else(|| args.get("style_ref"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn short_model_id(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

struct RouteFooter<'a> {
    route: &'a crate::router::RouteDecision,
    agent: Option<&'a str>,
    router: &'a str,
    image: &'a str,
    video: &'a str,
    /// Composer or executed backend: `local`, `openrouter`, or `cloud`.
    backend: &'a str,
    /// Active checkpoint display name (local only).
    base: &'a str,
    /// Pinned LoRA names, or `none`.
    lora: &'a str,
    /// Plan / Auto / YOLO.
    mode: &'a str,
}

fn footer_backend_kind(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "local" => "local",
        "cloud" | "openrouter" => "cloud",
        _ => "cloud",
    }
}

fn automation_mode_label(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "plan" => "Plan",
        "yolo" => "YOLO",
        _ => "Auto",
    }
}

fn format_route_visibility(f: &RouteFooter<'_>) -> String {
    let modality = f.route.modality.as_str();
    let agent = f.agent.map(short_model_id).unwrap_or("auto");
    let router = short_model_id(f.router);
    let kind = footer_backend_kind(f.backend);
    if kind == "local" {
        let base = if f.base.trim().is_empty() {
            "none"
        } else {
            f.base.trim()
        };
        let lora = if f.lora.trim().is_empty() {
            "none"
        } else {
            f.lora.trim()
        };
        format!(
            "Route: {modality} · agent={agent} · router={router} · base={base} · lora={lora} · mode={} · backend=local",
            automation_mode_label(f.mode)
        )
    } else {
        format!(
            "Route: {modality} · agent={agent} · router={router} · image={} · video={} · backend=cloud",
            short_model_id(f.image),
            short_model_id(f.video)
        )
    }
}

fn enrich_reply_with_route(
    reply: &str,
    route: &crate::router::RouteDecision,
    resolved_agent: Option<&str>,
    resolved_router: &str,
    resolved_image: &str,
    resolved_video: &str,
    action_results: &[Value],
    preferred_backend: Option<&str>,
    base: &str,
    lora: &str,
    mode: &str,
) -> String {
    let backend_from_gen = action_results.iter().find_map(|ar| {
        if ar.get("status").and_then(|s| s.as_str()) != Some("executed") {
            return None;
        }
        if !matches!(
            ar.get("type").and_then(|t| t.as_str()),
            Some("generate_image" | "generate_video" | "run_session")
        ) {
            return None;
        }
        ar.get("result")
            .and_then(|r| r.get("backend"))
            .and_then(|b| b.as_str())
            .map(str::to_string)
    });
    let preferred = preferred_backend
        .map(str::trim)
        .filter(|b| !b.is_empty() && *b != "auto");
    let backend = preferred.or(backend_from_gen.as_deref()).unwrap_or("cloud");
    let line = format_route_visibility(&RouteFooter {
        route,
        agent: resolved_agent,
        router: resolved_router,
        image: resolved_image,
        video: resolved_video,
        backend,
        base,
        lora,
        mode,
    });
    let trimmed = reply.trim();
    if trimmed.is_empty() {
        line
    } else if trimmed.contains("Route:") {
        trimmed.to_string()
    } else {
        format!("{trimmed}\n\n{line}")
    }
}

fn format_worker_action_summary(ar: &Value) -> Option<String> {
    let ty = ar.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let status = ar.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status == "error" {
        let err = ar.get("error").and_then(|e| e.as_str()).unwrap_or("error");
        return Some(format!("{ty} failed: {err}"));
    }
    if status == "proposed" {
        return Some(format!("Proposed: {ty}"));
    }
    let res = ar.get("result")?;
    match ty {
        "worker_status" => {
            let configured = res
                .get("workerConfigured")
                .and_then(|v| v.as_bool())
                .or_else(|| res.get("workerEnabled").and_then(|v| v.as_bool()));
            let reachable = res.get("reachable").and_then(|v| v.as_bool());
            let mut parts = Vec::new();
            if let Some(c) = configured {
                parts.push(if c {
                    "URL configured".into()
                } else {
                    "no worker URL".into()
                });
            }
            if let Some(r) = reachable {
                parts.push(if r {
                    "reachable".into()
                } else {
                    "not reachable".into()
                });
            }
            if let Some(m) = res.get("modelLoaded").and_then(|v| v.as_str()) {
                if !m.is_empty() {
                    parts.push(format!("loaded {m}"));
                }
            }
            Some(if parts.is_empty() {
                "Worker status checked.".into()
            } else {
                format!("Worker: {}.", parts.join(", "))
            })
        }
        "list_checkpoints" | "list_loras" => {
            let key = if ty == "list_loras" {
                "loras"
            } else {
                "checkpoints"
            };
            let label = if ty == "list_loras" {
                "LoRAs"
            } else {
                "Checkpoints"
            };
            let empty = if ty == "list_loras" {
                "No LoRAs on the worker yet — install one from Civitai first."
            } else {
                "No checkpoints on the worker yet — install one from Civitai first."
            };
            let items = res
                .get(key)
                .or_else(|| res.get("data"))
                .and_then(|d| d.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                return Some(empty.into());
            }
            let names: Vec<String> = items
                .iter()
                .take(8)
                .map(|c| {
                    let name =
                        c.get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or(if ty == "list_loras" {
                                "lora"
                            } else {
                                "checkpoint"
                            });
                    let path = c.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    let base = c.get("baseModel").and_then(|b| b.as_str()).unwrap_or("");
                    if base.is_empty() {
                        format!("{name} ({path})")
                    } else {
                        format!("{name} [{base}] ({path})")
                    }
                })
                .collect();
            let more = if items.len() > names.len() {
                format!(" … +{} more", items.len() - names.len())
            } else {
                String::new()
            };
            Some(format!(
                "{label} ({}): {}{}.",
                items.len(),
                names.join("; "),
                more
            ))
        }
        "get_base_model" | "set_base_model" => {
            let model = res
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("(none)");
            let base = res.get("baseModel").and_then(|b| b.as_str()).unwrap_or("");
            let verb = if ty == "set_base_model" {
                "Active base set to"
            } else {
                "Active base is"
            };
            Some(if base.is_empty() {
                format!("{verb} {model}.")
            } else {
                format!("{verb} {model} ({base}).")
            })
        }
        "gallery_list" => {
            let n = res
                .get("items")
                .and_then(|i| i.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!("Gallery: {n} item(s)."))
        }
        "gallery_get" => {
            let id = res.get("id").and_then(|i| i.as_str()).unwrap_or("?");
            let prompt = res
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .chars()
                .take(60)
                .collect::<String>();
            Some(if prompt.is_empty() {
                format!("Gallery item {id}.")
            } else {
                format!("Gallery {id}: {prompt}…")
            })
        }
        "list_styles" => {
            let n = res
                .get("items")
                .and_then(|i| i.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!("Styles: {n} pack(s)."))
        }
        "apply_style" => {
            let id = res.get("id").and_then(|i| i.as_str()).unwrap_or("?");
            let name = res
                .get("displayName")
                .and_then(|n| n.as_str())
                .unwrap_or(id);
            Some(format!(
                "Style ready: {name} (pass styleRef={id} on generate)."
            ))
        }
        "list_models" => {
            let chat_n = res
                .get("chat")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let img_n = res
                .get("openrouter")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let vid_n = res
                .get("video")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let local_n = res
                .get("local")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!(
                "Models: {chat_n} chat, {img_n} image, {vid_n} video, {local_n} local."
            ))
        }
        "get_settings" => {
            let zdr = res
                .get("privacy")
                .and_then(|p| p.get("zeroDataRetention"))
                .and_then(|z| z.as_bool())
                .unwrap_or(false);
            let chat = res.get("chatModel").and_then(|m| m.as_str()).unwrap_or("?");
            let image = res
                .get("imageModel")
                .and_then(|m| m.as_str())
                .unwrap_or("?");
            let router = res
                .get("routerModel")
                .and_then(|m| m.as_str())
                .unwrap_or("?");
            Some(format!(
                "Settings: agent={chat}, image={image}, router={router}, ZDR={}.",
                if zdr { "on" } else { "off" }
            ))
        }
        "get_route_defaults" => {
            let auto_img = res
                .get("auto")
                .and_then(|a| a.get("image"))
                .and_then(|m| m.as_str())
                .unwrap_or("?");
            let auto_vid = res
                .get("auto")
                .and_then(|a| a.get("video"))
                .and_then(|m| m.as_str())
                .unwrap_or("?");
            Some(format!(
                "Route defaults (Auto): image={}, video={}. Use set_chat_models to pin.",
                short_model_id(auto_img),
                short_model_id(auto_vid)
            ))
        }
        "set_chat_models" => {
            let chat = res.get("chatModel").and_then(|m| m.as_str()).unwrap_or("?");
            let image = res
                .get("imageModel")
                .and_then(|m| m.as_str())
                .unwrap_or("?");
            let router = res
                .get("routerModel")
                .and_then(|m| m.as_str())
                .unwrap_or("?");
            Some(format!(
                "Saved model prefs: agent={chat}, image={image}, router={router}."
            ))
        }
        "openrouter_metrics" => {
            if !res
                .get("configured")
                .and_then(|c| c.as_bool())
                .unwrap_or(false)
            {
                return Some("OpenRouter metrics: API key not configured.".into());
            }
            let usage = res
                .get("key")
                .and_then(|k| k.get("usage"))
                .and_then(|u| u.as_f64());
            let remaining = res
                .get("key")
                .and_then(|k| k.get("limitRemaining"))
                .and_then(|u| u.as_f64());
            Some(match (usage, remaining) {
                (Some(u), Some(r)) => format!("OpenRouter spend: ${u:.4} used, ${r:.4} remaining."),
                (Some(u), None) => format!("OpenRouter spend: ${u:.4} used."),
                _ => "OpenRouter metrics fetched.".into(),
            })
        }
        "gallery_delete" => {
            let id = res.get("id").and_then(|i| i.as_str()).unwrap_or("?");
            Some(format!("Removed gallery item {id}."))
        }
        "gallery_download" => {
            let url = res.get("url").and_then(|u| u.as_str()).unwrap_or("");
            Some(if url.is_empty() {
                "Gallery file is ready in the UI download control.".into()
            } else {
                format!("Download in the UI: {url}")
            })
        }
        "list_install_jobs" => {
            let n = res
                .get("data")
                .or_else(|| res.get("jobs"))
                .and_then(|d| d.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!("Download jobs: {n}."))
        }
        "cancel_install_job" => Some("Cancelled that download.".into()),
        "list_installed" => Some("Listed installed worker assets.".into()),
        "favorite_asset" | "unfavorite_asset" => {
            let n = res
                .get("favorites")
                .and_then(|f| f.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!("Favorites: {n}."))
        }
        "pin_lora" | "unpin_lora" => {
            let n = res
                .get("pinnedLoras")
                .and_then(|f| f.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(format!("Session LoRA pins: {n}."))
        }
        "set_cost_tier" => {
            let tier = res
                .get("autoFallbacks")
                .and_then(|a| a.get("costTier"))
                .and_then(|t| t.as_str())
                .unwrap_or("?");
            Some(format!("Cost tier set to {tier}."))
        }
        "set_privacy" | "set_alignment" | "set_generate_backend" | "set_automation_mode" => {
            Some(format!("{ty} saved."))
        }
        "generate_image" | "generate_video" => {
            let backend = res.get("backend").and_then(|b| b.as_str()).unwrap_or("?");
            let model = res
                .get("model")
                .and_then(|m| m.as_str())
                .map(short_model_id)
                .unwrap_or("?");
            let modality =
                res.get("modality")
                    .and_then(|m| m.as_str())
                    .unwrap_or(if ty == "generate_video" {
                        "video"
                    } else {
                        "picture"
                    });
            Some(format!("Generated {modality} via {backend} ({model})."))
        }
        _ => None,
    }
}

fn enrich_reply_with_worker_results(reply: &str, action_results: &[Value]) -> String {
    // ZDR/data-policy failures must replace optimistic "Generating…" replies.
    if let Some(err) = action_results.iter().find_map(|ar| {
        if ar.get("status").and_then(|s| s.as_str()) != Some("error") {
            return None;
        }
        if !matches!(
            ar.get("type").and_then(|t| t.as_str()),
            Some("generate_image" | "generate_video")
        ) {
            return None;
        }
        let err = ar.get("error").and_then(|e| e.as_str())?;
        if crate::openrouter::is_zdr_routing_error(err) {
            Some(err.to_string())
        } else {
            None
        }
    }) {
        return format!(
            "I couldn't generate that media because Zero Data Retention blocked OpenRouter routing.\n\n\
             {err}\n\n\
             Quick fixes: Freya Settings → Privacy → turn off “Zero Data Retention on Freya requests”, \
             or pick a ZDR-capable Image/Video model (often Google). \
             Account-wide / model-group ZDR: https://openrouter.ai/settings/privacy"
        );
    }

    let summaries: Vec<String> = action_results
        .iter()
        .filter_map(format_worker_action_summary)
        .collect();
    if summaries.is_empty() {
        return reply.to_string();
    }
    let block = summaries.join(" ");
    let trimmed = reply.trim();
    if trimmed.is_empty()
        || trimmed == "Checking worker settings…"
        || trimmed.eq_ignore_ascii_case("checking worker settings...")
    {
        block
    } else {
        format!("{trimmed}\n\n{block}")
    }
}

async fn chat_generate_backend(
    state: &AppState,
    pinned: &[LoraIn],
    preferred: Option<&str>,
) -> String {
    if let Some(b) = preferred
        .map(str::trim)
        .filter(|b| !b.is_empty() && *b != "auto")
    {
        return b.to_string();
    }
    if !pinned.is_empty() && state.worker.read().await.is_some() {
        return "local".into();
    }
    "auto".into()
}

/// Image model for chat generate actions: local never gets Auto/cloud/prose ids.
fn resolve_action_image_model(image_model: Option<&str>, backend: &str) -> Option<String> {
    let m = image_model.map(str::trim).filter(|s| !s.is_empty())?;
    if crate::router::is_auto_image(m) {
        return None;
    }
    if backend == "local" {
        return crate::base_models::filter_local_model(Some(m));
    }
    // OpenRouter path still rejects non-image prose via resolve_image_model later.
    Some(m.to_string())
}

fn action_error(ty: &str, args: &Value, error: impl Into<String>) -> Value {
    json!({
        "type": ty,
        "status": "error",
        "args": args,
        "error": error.into()
    })
}

fn action_ok(ty: &str, args: &Value, result: Value) -> Value {
    json!({
        "type": ty,
        "status": "executed",
        "args": args,
        "result": result
    })
}

fn api_err_msg(e: ApiError) -> String {
    match e {
        ApiError::Msg(m) => m,
        ApiError::Unauthorized(m) | ApiError::Unavailable(m) => m,
        ApiError::NoWorker => "No diffusion worker configured".into(),
        ApiError::NotFound => "not found".into(),
    }
}

/// Refuse args whose names look like credentials. Values are never echoed.
fn reject_secret_fields(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    for key in obj.keys() {
        let l = key.to_ascii_lowercase().replace(['_', '-'], "");
        if l.contains("apikey")
            || l.contains("apitoken")
            || l.contains("password")
            || l.contains("secret")
            || l == "token"
            || l.contains("admintoken")
            || l.contains("bearertoken")
            || l.contains("hftoken")
            || l.contains("workertoken")
        {
            return Some(format!("refusing to read or set {key}"));
        }
    }
    None
}

fn with_gallery_url(meta: &ArtifactMeta) -> Value {
    let mut v = serde_json::to_value(meta).unwrap_or_else(|_| json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "url".into(),
            json!(format!("/v1/gallery/{}/image", meta.id)),
        );
    }
    v
}

async fn persist_prefs(state: &AppState) -> bool {
    let persisted = state.persist_secrets().await.is_ok();
    let cfg = state.config.read().await;
    let mut or = state.openrouter.write().await;
    *or = crate::openrouter::OpenRouterClient::new(&cfg);
    persisted
}

async fn settings_value(state: &AppState) -> Value {
    serde_json::to_value(settings_public_from_state(state).await).unwrap_or(json!({}))
}

async fn execute_chat_action(
    state: &AppState,
    action: &PlannedAction,
    pinned: &[LoraIn],
    image_model: Option<&str>,
    video_model: Option<&str>,
    backend: Option<&str>,
) -> serde_json::Value {
    let ty = action.action_type.as_str();
    match ty {
        "suggest_ideas" => {
            let topic = action
                .args
                .get("topic")
                .and_then(|t| t.as_str())
                .unwrap_or("a memorable scene");
            let system = load_instruction_toned(&state, "suggest").await;
            let or = state.openrouter.read().await;
            match or.chat(&system, topic, None).await {
                Ok(text) => {
                    let mut ideas = Vec::new();
                    let mut note = String::new();
                    if let Ok(parsed) =
                        serde_json::from_str::<serde_json::Value>(strip_json_fence(&text))
                    {
                        if let Some(arr) = parsed.get("ideas").and_then(|i| i.as_array()) {
                            ideas = arr
                                .iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect();
                        }
                        if let Some(n) = parsed.get("sessionNote").and_then(|n| n.as_str()) {
                            note = n.to_string();
                        }
                    }
                    if ideas.is_empty() {
                        ideas = text
                            .lines()
                            .map(|l| {
                                l.trim()
                                    .trim_start_matches(|c: char| {
                                        c.is_ascii_digit() || c == '.' || c == ')'
                                    })
                                    .trim()
                            })
                            .filter(|l| !l.is_empty())
                            .take(5)
                            .map(str::to_string)
                            .collect();
                    }
                    json!({
                        "type": ty,
                        "status": "executed",
                        "args": action.args,
                        "result": { "ideas": ideas, "sessionNote": note }
                    })
                }
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "search_civitai" => {
            let q = action
                .args
                .get("query")
                .and_then(|q| q.as_str())
                .unwrap_or("")
                .trim();
            if q.is_empty() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing query"
                });
            }
            let civ = state.civitai.read().await;
            if !civ.configured() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "Civitai API key not configured"
                });
            }
            match civ.search(q, 12).await {
                Ok(items) => {
                    let mut assets: Vec<_> = items
                        .into_iter()
                        .filter(|c| crate::civitai::is_freya_asset_type(&c.model_type))
                        .collect();
                    assets.sort_by_key(|c| crate::civitai::asset_type_rank(&c.model_type));
                    assets.truncate(6);
                    json!({
                        "type": ty,
                        "status": "executed",
                        "args": action.args,
                        "result": { "query": q, "loras": assets }
                    })
                }
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "generate_image" => {
            let prompt = action
                .args
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .trim();
            if prompt.is_empty() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing prompt"
                });
            }
            let backend_choice = chat_generate_backend(state, pinned, backend).await;
            let req = GenerateRequest {
                prompt: prompt.to_string(),
                negative_prompt: None,
                enhance: Some("auto".into()),
                backend: Some(backend_choice.clone()),
                model: resolve_action_image_model(image_model, &backend_choice),
                style_ref: style_ref_from_args(&action.args),
                loras: pinned.to_vec(),
                size: None,
                seed: None,
                modality: Some("picture".into()),
                duration: None,
            };
            match run_generate(state, req).await {
                Ok(result) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": result
                }),
                Err(e) => {
                    let msg = match e {
                        ApiError::Msg(m) => m,
                        ApiError::Unauthorized(m) | ApiError::Unavailable(m) => m,
                        ApiError::NoWorker => "No diffusion worker configured".into(),
                        ApiError::NotFound => "not found".into(),
                    };
                    json!({
                        "type": ty,
                        "status": "error",
                        "args": action.args,
                        "error": msg
                    })
                }
            }
        }
        "generate_video" => {
            let prompt = action
                .args
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .trim();
            if prompt.is_empty() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing prompt"
                });
            }
            let modality = action
                .args
                .get("modality")
                .and_then(|m| m.as_str())
                .unwrap_or("video")
                .to_string();
            let backend_choice = chat_generate_backend(state, pinned, backend).await;
            let mut req = GenerateRequest {
                prompt: prompt.to_string(),
                negative_prompt: None,
                enhance: Some("auto".into()),
                backend: Some(backend_choice),
                model: video_model.map(str::to_string),
                style_ref: style_ref_from_args(&action.args),
                loras: vec![],
                size: None,
                seed: None,
                modality: Some(modality),
                duration: action.args.get("duration").and_then(|d| d.as_i64()),
            };
            if req.model.as_deref() == Some("auto") {
                req.model = None;
            }
            match run_generate(state, req).await {
                Ok(result) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": result
                }),
                Err(e) => {
                    let msg = match e {
                        ApiError::Msg(m) => m,
                        ApiError::Unauthorized(m) | ApiError::Unavailable(m) => m,
                        ApiError::NoWorker => "No diffusion worker configured".into(),
                        ApiError::NotFound => "not found".into(),
                    };
                    json!({
                        "type": ty,
                        "status": "error",
                        "args": action.args,
                        "error": msg
                    })
                }
            }
        }
        "run_session" => {
            let goal = action
                .args
                .get("goal")
                .and_then(|g| g.as_str())
                .unwrap_or("explore the scene")
                .trim();
            let backend_choice = chat_generate_backend(state, pinned, backend).await;
            let local_passthrough = backend_choice.eq_ignore_ascii_case("local")
                && state
                    .config
                    .read()
                    .await
                    .alignment_prefs()
                    .local_adult_passthrough();
            let batch: Vec<String> = if local_passthrough {
                Vec::new()
            } else {
                let system = load_instruction_toned(&state, "session").await;
                let or = state.openrouter.read().await;
                match or.chat(&system, goal, None).await {
                    Ok(text) => {
                        if let Ok(parsed) =
                            serde_json::from_str::<serde_json::Value>(strip_json_fence(&text))
                        {
                            parsed
                                .get("batchPrompts")
                                .and_then(|b| b.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|x| x.as_str().map(str::to_string))
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            vec![]
                        }
                    }
                    Err(_) => vec![],
                }
            };
            let prompts = if batch.is_empty() {
                vec![
                    format!("{goal} — wide establishing shot."),
                    format!("{goal} — closer framing."),
                    format!("{goal} — quieter mood."),
                ]
            } else {
                batch
            };
            let session_style = style_ref_from_args(&action.args);
            let mut results = Vec::new();
            for p in &prompts {
                let req = GenerateRequest {
                    prompt: p.clone(),
                    negative_prompt: None,
                    enhance: Some("auto".into()),
                    backend: Some(backend_choice.clone()),
                    model: resolve_action_image_model(image_model, &backend_choice),
                    style_ref: session_style.clone(),
                    loras: pinned.to_vec(),
                    size: None,
                    seed: None,
                    modality: Some("picture".into()),
                    duration: None,
                };
                match run_generate(state, req).await {
                    Ok(r) => results.push(r),
                    Err(e) => {
                        let msg = match e {
                            ApiError::Msg(m) => m,
                            ApiError::Unauthorized(m) | ApiError::Unavailable(m) => m,
                            ApiError::NoWorker => "No diffusion worker configured".into(),
                            ApiError::NotFound => "not found".into(),
                        };
                        results.push(json!({ "error": msg, "prompt": p }));
                    }
                }
            }
            json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": { "goal": goal, "batchPrompts": prompts, "results": results }
            })
        }
        "install_asset" => {
            let download_url = action
                .args
                .get("downloadUrl")
                .and_then(|u| u.as_str())
                .unwrap_or("");
            let version_id = action.args.get("versionId").and_then(|x| x.as_i64());
            let dest_path = action
                .args
                .get("destPath")
                .and_then(|d| d.as_str())
                .unwrap_or("loras/asset.safetensors");
            let name = action
                .args
                .get("name")
                .and_then(|n| n.as_str())
                .map(str::to_string);
            if download_url.is_empty() && version_id.is_none() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing downloadUrl or versionId"
                });
            }
            let install_req = InstallCivitaiRequest {
                download_url: download_url.to_string(),
                dest_path: dest_path.to_string(),
                name,
                base_model: action
                    .args
                    .get("baseModel")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                version_id,
                model_id: action
                    .args
                    .get("modelId")
                    .and_then(|x| x.as_i64())
                    .or_else(|| action.args.get("id").and_then(|x| x.as_i64())),
                file_id: action.args.get("fileId").and_then(|x| x.as_i64()),
                model_type: action
                    .args
                    .get("modelType")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                thumbnail_url: action
                    .args
                    .get("thumbnailUrl")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
            };
            match civitai_install_inner(state, install_req).await {
                Ok(v) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": v
                }),
                Err(e) => {
                    let msg = match e {
                        ApiError::Msg(m) => m,
                        ApiError::Unauthorized(m) | ApiError::Unavailable(m) => m,
                        ApiError::NoWorker => {
                            "LoRA install needs a diffusion worker — set the worker URL in Settings"
                                .into()
                        }
                        ApiError::NotFound => "not found".into(),
                    };
                    json!({
                        "type": ty,
                        "status": "error",
                        "args": action.args,
                        "error": msg
                    })
                }
            }
        }
        "worker_status" => {
            let (worker_enabled, worker_url) = {
                let cfg = state.config.read().await;
                (cfg.worker_enabled, cfg.diffusion_worker_url.clone())
            };
            let mut reachable = false;
            let mut detail = Value::Null;
            if let Some(client) = state.worker.read().await.as_ref() {
                if let Some(v) = client.stats_detail().await {
                    reachable = true;
                    detail = v;
                } else {
                    reachable = client.health().await;
                }
            }
            json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": {
                    "workerEnabled": worker_enabled,
                    "workerConfigured": worker_url.is_some(),
                    "reachable": reachable,
                    "modelLoaded": detail.get("modelLoaded").cloned().unwrap_or(Value::Null),
                    "vram": detail.get("vram").cloned().unwrap_or(Value::Null),
                    "loras": detail.get("loras").cloned().unwrap_or(Value::Null),
                    "checkpoints": detail.get("checkpoints").cloned().unwrap_or(Value::Null),
                    "engine": detail.get("engine").cloned().unwrap_or(Value::Null),
                }
            })
        }
        "list_checkpoints" => {
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "No diffusion worker configured"
                });
            };
            match client.list_checkpoints().await {
                Ok(items) => {
                    let checkpoints: Vec<Value> = items
                        .into_iter()
                        .map(|a| {
                            json!({
                                "id": a.id,
                                "name": a.name.unwrap_or_else(|| a.id.clone()),
                                "kind": a.kind,
                                "path": a.path,
                                "baseModel": a.base_model,
                                "backend": "local",
                            })
                        })
                        .collect();
                    json!({
                        "type": ty,
                        "status": "executed",
                        "args": action.args,
                        "result": { "checkpoints": checkpoints }
                    })
                }
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "list_loras" => {
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "No diffusion worker configured"
                });
            };
            match client.list_loras().await {
                Ok(items) => {
                    let loras: Vec<Value> = items
                        .into_iter()
                        .map(|a| {
                            json!({
                                "id": a.id,
                                "name": a.name.unwrap_or_else(|| a.id.clone()),
                                "kind": a.kind,
                                "path": a.path,
                                "baseModel": a.base_model,
                                "backend": "local",
                            })
                        })
                        .collect();
                    json!({
                        "type": ty,
                        "status": "executed",
                        "args": action.args,
                        "result": { "loras": loras }
                    })
                }
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "get_base_model" => {
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "No diffusion worker configured"
                });
            };
            match client.get_active_base().await {
                Ok(v) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": v
                }),
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "set_base_model" => {
            let model = action
                .args
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .trim();
            if model.is_empty() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing model path — list_checkpoints first, then set_base_model with args.model"
                });
            }
            let base_model = action
                .args
                .get("baseModel")
                .and_then(|b| b.as_str())
                .map(str::to_string);
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "No diffusion worker configured"
                });
            };
            match client.set_active_base(model, base_model.as_deref()).await {
                Ok(v) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": v
                }),
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "gallery_list" => {
            let query = action
                .args
                .get("query")
                .and_then(|q| q.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase());
            match state.gallery.list().await {
                Ok(items) => {
                    let filtered: Vec<_> = match &query {
                        Some(q) => items
                            .into_iter()
                            .filter(|m| {
                                m.prompt.to_ascii_lowercase().contains(q)
                                    || m.enhanced_prompt
                                        .as_deref()
                                        .map(|e| e.to_ascii_lowercase().contains(q))
                                        .unwrap_or(false)
                                    || m.id.to_ascii_lowercase().contains(q)
                            })
                            .take(24)
                            .collect(),
                        None => items.into_iter().take(24).collect(),
                    };
                    let items: Vec<Value> = filtered.iter().map(with_gallery_url).collect();
                    json!({
                        "type": ty,
                        "status": "executed",
                        "args": action.args,
                        "result": { "items": items, "query": query }
                    })
                }
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "gallery_get" => {
            let id = action
                .args
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .trim();
            if id.is_empty() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing id"
                });
            }
            match state.gallery.get(id).await {
                Ok(Some(meta)) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": with_gallery_url(&meta)
                }),
                Ok(None) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "not found"
                }),
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "list_styles" => match styles::load_styles(&state.packs_dir).await {
            Ok(items) => json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": { "items": items }
            }),
            Err(e) => json!({
                "type": ty,
                "status": "error",
                "args": action.args,
                "error": e.to_string()
            }),
        },
        "apply_style" => {
            let style_ref = style_ref_from_args(&action.args).or_else(|| {
                action
                    .args
                    .get("id")
                    .and_then(|i| i.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            });
            let Some(id) = style_ref else {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing styleRef — list_styles first"
                });
            };
            match styles::load_styles(&state.packs_dir).await {
                Ok(items) => {
                    if let Some(pack) = items.into_iter().find(|s| s.id == id) {
                        json!({
                            "type": ty,
                            "status": "executed",
                            "args": action.args,
                            "result": pack
                        })
                    } else {
                        json!({
                            "type": ty,
                            "status": "error",
                            "args": action.args,
                            "error": format!("unknown styleRef: {id}")
                        })
                    }
                }
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": e.to_string()
                }),
            }
        }
        "list_models" => match models_payload(state).await {
            Ok(v) => json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": v
            }),
            Err(e) => {
                let msg = match e {
                    ApiError::Msg(m) => m,
                    ApiError::Unauthorized(m) | ApiError::Unavailable(m) => m,
                    ApiError::NoWorker => "No diffusion worker configured".into(),
                    ApiError::NotFound => "not found".into(),
                };
                json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": msg
                })
            }
        },
        "get_settings" => {
            let settings = settings_public_from_state(state).await;
            json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": settings
            })
        }
        "get_route_defaults" => {
            let (cfg_snap, privacy, alignment, auto_fallbacks) = {
                let cfg = state.config.read().await;
                let or = state.openrouter.read().await;
                (
                    (
                        cfg.chat_model.clone(),
                        cfg.image_model.clone(),
                        cfg.router_model.clone(),
                        or.configured_chat_model().to_string(),
                        or.configured_image_model().to_string(),
                        or.configured_router_model().to_string(),
                    ),
                    or.privacy().clone(),
                    or.alignment().clone(),
                    or.auto_fallbacks().clone(),
                )
            };
            let (chat_model, image_model, router_model, _or_chat, or_image, or_router) = cfg_snap;
            let resolved_router = crate::router::resolve_router_model_prefs(
                &or_router,
                &privacy,
                &alignment,
                &auto_fallbacks,
            );
            let auto_image = crate::router::resolve_auto_fallback(
                crate::router::AutoFallbackKind::Image,
                &privacy,
                &alignment,
                &auto_fallbacks,
            );
            let auto_video = crate::router::resolve_auto_fallback(
                crate::router::AutoFallbackKind::Video,
                &privacy,
                &alignment,
                &auto_fallbacks,
            );
            let auto_agent = crate::router::resolve_auto_fallback(
                crate::router::AutoFallbackKind::Agent,
                &privacy,
                &alignment,
                &auto_fallbacks,
            );
            json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": {
                    "sentinels": {
                        "agent": crate::router::AUTO_AGENT,
                        "image": crate::router::AUTO_IMAGE,
                        "router": crate::router::AUTO_ROUTER,
                    },
                    "configured": {
                        "chatModel": chat_model,
                        "imageModel": image_model,
                        "routerModel": router_model,
                    },
                    "auto": {
                        "router": resolved_router.clone(),
                        "image": auto_image,
                        "video": auto_video,
                        "agent": auto_agent,
                    },
                    "autoFallbacks": auto_fallbacks,
                    "resolved": {
                        "router": resolved_router,
                        "imageConfigured": or_image,
                        "zdr": privacy.zero_data_retention,
                        "alignment": alignment.model_alignment,
                    },
                    "notes": [
                        "Pin agent/image/router with set_chat_models (persists to Settings).",
                        "Cloud vs Local is the Studio composer switch (backend), not a chat model pref.",
                        "Pin video with set_chat_models videoModel (id only). Cloud vs Local is set_generate_backend.",
                        "Configure Auto fallbacks on the Models tab when the router does not pin a model.",
                    ],
                }
            })
        }
        "set_chat_models" => {
            let chat = action
                .args
                .get("chatModel")
                .or_else(|| action.args.get("agentModel"))
                .and_then(|m| m.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let image = action
                .args
                .get("imageModel")
                .and_then(|m| m.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let router = action
                .args
                .get("routerModel")
                .and_then(|m| m.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let video = action
                .args
                .get("videoModel")
                .and_then(|m| m.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            if chat.is_none() && image.is_none() && router.is_none() && video.is_none() {
                return action_error(
                    ty,
                    &action.args,
                    "provide chatModel, imageModel, routerModel, and/or videoModel",
                );
            }
            if let Some(msg) = reject_secret_fields(&action.args) {
                return action_error(ty, &action.args, msg);
            }
            let video_norm = if let Some(v) = video.as_deref() {
                match crate::studio::normalize_video_model(v) {
                    Ok(id) => Some(id),
                    Err(e) => return action_error(ty, &action.args, e),
                }
            } else {
                None
            };
            {
                let mut cfg = state.config.write().await;
                if let Some(m) = chat {
                    cfg.chat_model = m;
                }
                if let Some(m) = image {
                    cfg.image_model = m;
                }
                if let Some(m) = router {
                    cfg.router_model = m;
                }
                if let Some(v) = video_norm {
                    cfg.studio.video_model = v;
                }
            }
            let persisted = state.persist_secrets().await.is_ok();
            {
                let cfg = state.config.read().await;
                let mut or = state.openrouter.write().await;
                *or = crate::openrouter::OpenRouterClient::new(&cfg);
            }
            let settings = settings_public_from_state(state).await;
            let mut out = serde_json::to_value(&settings).unwrap_or(json!({}));
            if let Some(obj) = out.as_object_mut() {
                obj.insert("persisted".into(), json!(persisted));
            }
            json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": out
            })
        }
        "openrouter_metrics" => match openrouter_metrics_payload(state).await {
            Ok(v) => json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": v
            }),
            Err(e) => {
                let msg = match e {
                    ApiError::Msg(m) => m,
                    ApiError::Unauthorized(m) | ApiError::Unavailable(m) => m,
                    ApiError::NoWorker => "No diffusion worker configured".into(),
                    ApiError::NotFound => "not found".into(),
                };
                json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": msg
                })
            }
        },
        "list_install_jobs" => {
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return action_error(ty, &action.args, "No diffusion worker configured");
            };
            match client.list_install_jobs().await {
                Ok(v) => action_ok(ty, &action.args, v),
                Err(e) => action_error(ty, &action.args, e.to_string()),
            }
        }
        "cancel_install_job" => {
            let id = action
                .args
                .get("jobId")
                .or_else(|| action.args.get("id"))
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() {
                return action_error(ty, &action.args, "missing jobId");
            }
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return action_error(ty, &action.args, "No diffusion worker configured");
            };
            match client.cancel_install_job(&id).await {
                Ok(v) => action_ok(ty, &action.args, v),
                Err(e) => action_error(ty, &action.args, e.to_string()),
            }
        }
        "clear_install_jobs" => {
            let include_queued = action
                .args
                .get("includeQueued")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return action_error(ty, &action.args, "No diffusion worker configured");
            };
            match client.clear_install_jobs(include_queued).await {
                Ok(v) => action_ok(ty, &action.args, v),
                Err(e) => action_error(ty, &action.args, e.to_string()),
            }
        }
        "list_installed" => match installed_payload(state).await {
            Ok(v) => action_ok(ty, &action.args, v),
            Err(e) => action_error(ty, &action.args, api_err_msg(e)),
        },
        "delete_installed" => {
            let path = action
                .args
                .get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if path.is_empty() || path.contains("..") {
                return action_error(ty, &action.args, "missing worker path");
            }
            let worker = state.worker.read().await;
            let Some(client) = worker.as_ref() else {
                return action_error(ty, &action.args, "No diffusion worker configured");
            };
            match client.delete_asset(&path).await {
                Ok(v) => action_ok(ty, &action.args, v),
                Err(e) => action_error(ty, &action.args, e.to_string()),
            }
        }
        "favorite_asset" => {
            if let Some(msg) = reject_secret_fields(&action.args) {
                return action_error(ty, &action.args, msg);
            }
            let card = crate::studio::asset_from_action_args(&action.args);
            let mut saved = None;
            let mut err = None;
            {
                let mut cfg = state.config.write().await;
                match crate::studio::favorite_add(&mut cfg.studio, &card) {
                    Ok(c) => saved = Some(c),
                    Err(e) => err = Some(e),
                }
            }
            if let Some(e) = err {
                return action_error(ty, &action.args, e);
            }
            let persisted = state.persist_secrets().await.is_ok();
            let favorites = state.config.read().await.studio.favorites.clone();
            action_ok(
                ty,
                &action.args,
                json!({ "favorites": favorites, "favorite": saved, "persisted": persisted }),
            )
        }
        "unfavorite_asset" => {
            let card = crate::studio::asset_from_action_args(&action.args);
            let removed = {
                let mut cfg = state.config.write().await;
                crate::studio::favorite_remove(&mut cfg.studio, &card)
            };
            let persisted = state.persist_secrets().await.is_ok();
            let favorites = state.config.read().await.studio.favorites.clone();
            action_ok(
                ty,
                &action.args,
                json!({ "favorites": favorites, "removed": removed, "persisted": persisted }),
            )
        }
        "list_favorites" => {
            let favorites = state.config.read().await.studio.favorites.clone();
            action_ok(ty, &action.args, json!({ "favorites": favorites }))
        }
        "pin_lora" => {
            if let Some(msg) = reject_secret_fields(&action.args) {
                return action_error(ty, &action.args, msg);
            }
            let card = crate::studio::asset_from_action_args(&action.args);
            let mut err = None;
            {
                let mut cfg = state.config.write().await;
                if let Err(e) = crate::studio::pin_add(&mut cfg.studio, &card) {
                    err = Some(e);
                }
            }
            if let Some(e) = err {
                return action_error(ty, &action.args, e);
            }
            let persisted = state.persist_secrets().await.is_ok();
            let pins = state.config.read().await.studio.pinned_loras.clone();
            action_ok(
                ty,
                &action.args,
                json!({ "pinnedLoras": pins, "persisted": persisted }),
            )
        }
        "unpin_lora" => {
            let card = crate::studio::asset_from_action_args(&action.args);
            {
                let mut cfg = state.config.write().await;
                crate::studio::pin_remove(&mut cfg.studio, &card);
            }
            let persisted = state.persist_secrets().await.is_ok();
            let pins = state.config.read().await.studio.pinned_loras.clone();
            action_ok(
                ty,
                &action.args,
                json!({ "pinnedLoras": pins, "persisted": persisted }),
            )
        }
        "list_pins" => {
            let pins = state.config.read().await.studio.pinned_loras.clone();
            action_ok(ty, &action.args, json!({ "pinnedLoras": pins }))
        }
        "set_cost_tier" => {
            let tier = action
                .args
                .get("costTier")
                .or_else(|| action.args.get("tier"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if tier.is_empty() {
                return action_error(ty, &action.args, "missing costTier (low|medium|high|max)");
            }
            {
                let mut cfg = state.config.write().await;
                let mut prefs = cfg.auto_fallbacks_prefs();
                prefs.cost_tier = tier;
                cfg.apply_auto_fallbacks(prefs);
            }
            let persisted = persist_prefs(state).await;
            let mut out = settings_value(state).await;
            if let Some(obj) = out.as_object_mut() {
                obj.insert("persisted".into(), json!(persisted));
            }
            action_ok(ty, &action.args, out)
        }
        "set_privacy" => {
            if let Some(msg) = reject_secret_fields(&action.args) {
                return action_error(ty, &action.args, msg);
            }
            let zdr = action
                .args
                .get("zeroDataRetention")
                .and_then(|v| v.as_bool());
            let data_collection = action
                .args
                .get("dataCollection")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if zdr.is_none() && data_collection.is_none() {
                return action_error(
                    ty,
                    &action.args,
                    "provide zeroDataRetention and/or dataCollection (allow|deny)",
                );
            }
            {
                let mut cfg = state.config.write().await;
                let mut prefs = cfg.privacy_prefs();
                if let Some(z) = zdr {
                    prefs.zero_data_retention = z;
                }
                if let Some(dc) = data_collection {
                    prefs.data_collection = dc;
                }
                cfg.apply_privacy(prefs);
            }
            let persisted = persist_prefs(state).await;
            let mut out = settings_value(state).await;
            if let Some(obj) = out.as_object_mut() {
                obj.insert("persisted".into(), json!(persisted));
            }
            action_ok(ty, &action.args, out)
        }
        "set_alignment" => {
            if let Some(msg) = reject_secret_fields(&action.args) {
                return action_error(ty, &action.args, msg);
            }
            let alignment = action
                .args
                .get("modelAlignment")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let nsfw = action
                .args
                .get("allowNsfwLocal")
                .or_else(|| action.args.get("showNsfw"))
                .and_then(|v| v.as_bool());
            let tone = action
                .args
                .get("instructionTone")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if alignment.is_none() && nsfw.is_none() && tone.is_none() {
                return action_error(
                    ty,
                    &action.args,
                    "provide modelAlignment, allowNsfwLocal, and/or instructionTone",
                );
            }
            {
                let mut cfg = state.config.write().await;
                let mut prefs = cfg.alignment_prefs();
                if let Some(a) = alignment {
                    prefs.model_alignment = a;
                }
                if let Some(n) = nsfw {
                    prefs.allow_nsfw_local = n;
                }
                if let Some(t) = tone {
                    prefs.instruction_tone = t;
                }
                cfg.apply_alignment(prefs);
            }
            let persisted = persist_prefs(state).await;
            let mut out = settings_value(state).await;
            if let Some(obj) = out.as_object_mut() {
                obj.insert("persisted".into(), json!(persisted));
            }
            action_ok(ty, &action.args, out)
        }
        "set_generate_backend" => {
            let raw = action
                .args
                .get("backend")
                .and_then(|b| b.as_str())
                .unwrap_or("");
            let backend = match crate::studio::normalize_backend(raw) {
                Ok(b) => b,
                Err(e) => return action_error(ty, &action.args, e),
            };
            {
                let mut cfg = state.config.write().await;
                cfg.studio.generate_backend = backend.clone();
            }
            let persisted = state.persist_secrets().await.is_ok();
            action_ok(
                ty,
                &action.args,
                json!({ "backend": backend, "persisted": persisted }),
            )
        }
        "set_automation_mode" => {
            let raw = action
                .args
                .get("mode")
                .or_else(|| action.args.get("automationMode"))
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let mode = match crate::studio::normalize_mode(raw) {
                Ok(m) => m,
                Err(e) => return action_error(ty, &action.args, e),
            };
            {
                let mut cfg = state.config.write().await;
                cfg.studio.automation_mode = mode.clone();
            }
            let persisted = state.persist_secrets().await.is_ok();
            action_ok(
                ty,
                &action.args,
                json!({ "automationMode": mode, "persisted": persisted }),
            )
        }
        "gallery_delete" => {
            let id = action
                .args
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() {
                return action_error(ty, &action.args, "missing id");
            }
            match state.gallery.delete(&id).await {
                Ok(true) => action_ok(ty, &action.args, json!({ "ok": true, "id": id })),
                Ok(false) => action_error(ty, &action.args, "not found"),
                Err(e) => action_error(ty, &action.args, e.to_string()),
            }
        }
        "gallery_download" => {
            let id = action
                .args
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() {
                return action_error(ty, &action.args, "missing id");
            }
            match state.gallery.get(&id).await {
                Ok(Some(meta)) => {
                    let mut item = with_gallery_url(&meta);
                    if let Some(obj) = item.as_object_mut() {
                        obj.insert(
                            "note".into(),
                            json!("Client downloads this url. Bytes are not returned here."),
                        );
                    }
                    action_ok(ty, &action.args, item)
                }
                Ok(None) => action_error(ty, &action.args, "not found"),
                Err(e) => action_error(ty, &action.args, e.to_string()),
            }
        }
        "worker_start" => {
            let (container, unit) = {
                let cfg = state.config.read().await;
                (
                    cfg.worker_container.clone(),
                    cfg.worker_systemd_unit.clone(),
                )
            };
            match crate::worker_ctl::start(&container, &unit).await {
                Ok(runtime) => action_ok(
                    ty,
                    &action.args,
                    json!({ "ok": true, "runtime": runtime.to_json() }),
                ),
                Err(e) => action_error(ty, &action.args, e),
            }
        }
        "worker_stop" => {
            let (container, unit) = {
                let cfg = state.config.read().await;
                (
                    cfg.worker_container.clone(),
                    cfg.worker_systemd_unit.clone(),
                )
            };
            match crate::worker_ctl::stop(&container, &unit).await {
                Ok(runtime) => action_ok(
                    ty,
                    &action.args,
                    json!({ "ok": true, "runtime": runtime.to_json() }),
                ),
                Err(e) => action_error(ty, &action.args, e),
            }
        }
        other => json!({
            "type": other,
            "status": "error",
            "args": action.args,
            "error": format!("unknown action type: {other}")
        }),
    }
}

async fn civitai_install_inner(
    state: &AppState,
    mut req: InstallCivitaiRequest,
) -> ApiResult<serde_json::Value> {
    let cfg = state.config.read().await;
    if !cfg.civitai_configured() {
        return Err(ApiError::Msg(
            "Civitai API key required — set it in Settings".into(),
        ));
    }
    drop(cfg);

    // Prefer client URL; otherwise resolve from versionId (+ optional fileId).
    if req.download_url.trim().is_empty() {
        let version_id = req
            .version_id
            .ok_or_else(|| ApiError::Msg("downloadUrl or versionId required to install".into()))?;
        let civ = state.civitai.read().await;
        let resolved = civ
            .resolve_version_download(version_id, req.file_id)
            .await
            .map_err(ApiError::from)?;
        req.download_url = resolved.download_url;
        if req.version_id.is_none() {
            req.version_id = Some(resolved.version_id);
        }
        if req.file_id.is_none() {
            req.file_id = resolved.file_id;
        }
        if req.base_model.is_none() {
            req.base_model = resolved.base_model;
        }
        if req.model_id.is_none() {
            req.model_id = resolved.model_id;
        }
        if req.thumbnail_url.is_none() {
            req.thumbnail_url = resolved.thumbnail_url;
        }
    }
    // Backfill modelId / thumbnail from version when client had a URL but omitted them.
    if req.model_id.is_none() || req.thumbnail_url.is_none() {
        if let Some(version_id) = req.version_id {
            let civ = state.civitai.read().await;
            if let Ok(resolved) = civ.resolve_version_download(version_id, req.file_id).await {
                if req.model_id.is_none() {
                    req.model_id = resolved.model_id;
                }
                if req.base_model.is_none() {
                    req.base_model = resolved.base_model;
                }
                if req.thumbnail_url.is_none() {
                    req.thumbnail_url = resolved.thumbnail_url;
                }
            }
        }
    }
    if !civitai_download_url_ok(&req.download_url) {
        return Err(ApiError::Msg(
            "downloadUrl must be an http(s) civitai.com URL".into(),
        ));
    }
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;

    // Reject duplicate dest: already on disk, or an active job targeting the same path.
    {
        let jobs_v = client.list_install_jobs().await.unwrap_or(Value::Null);
        if let Some(arr) = jobs_v.get("data").and_then(|x| x.as_array()) {
            for j in arr {
                let st = j.get("status").and_then(|x| x.as_str()).unwrap_or("");
                let dp = j
                    .get("destPath")
                    .or_else(|| j.get("path"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if dp == req.dest_path && matches!(st, "queued" | "downloading") {
                    return Err(ApiError::Msg(format!(
                        "already downloading to {dp} — wait or cancel the existing job"
                    )));
                }
            }
        }
        let mut on_disk = client.list_loras().await.unwrap_or_default();
        on_disk.extend(client.list_checkpoints().await.unwrap_or_default());
        if on_disk.iter().any(|a| a.path == req.dest_path) {
            return Err(ApiError::Msg(format!(
                "already installed at {} — delete it first",
                req.dest_path
            )));
        }
    }

    let auth = {
        let cfg = state.config.read().await;
        cfg.civitai_api_key.clone()
    };
    let url = req.download_url.clone();
    let meta = crate::worker::InstallMeta {
        name: req.name.clone(),
        base_model: req.base_model.clone(),
        version_id: req.version_id,
        model_id: req.model_id,
        model_type: req.model_type.clone(),
        thumbnail_url: req.thumbnail_url.clone(),
    };
    let queued = client
        .enqueue_install("civitai", &url, &req.dest_path, Some(&auth), Some(&meta))
        .await
        .map_err(ApiError::from)?;
    let job = queued.get("job").cloned().unwrap_or(Value::Null);
    let job_id = queued
        .get("jobId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let status = queued
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("queued");
    Ok(json!({
        "ok": true,
        "queued": true,
        "jobId": job_id,
        "status": status,
        "job": job,
        "destPath": req.dest_path,
        "name": req.name,
        "baseModel": req.base_model,
        "versionId": req.version_id,
        "modelId": req.model_id,
        "fileId": req.file_id,
        "modelType": req.model_type,
    }))
}

struct ActionRun {
    loras: Vec<LoraIn>,
    image_model: Option<String>,
    video_model: Option<String>,
    backend: Option<String>,
}

impl ActionRun {
    async fn exec(&mut self, state: &AppState, action: &PlannedAction) -> Value {
        let v = execute_chat_action(
            state,
            action,
            &self.loras,
            self.image_model.as_deref(),
            self.video_model.as_deref(),
            self.backend.as_deref(),
        )
        .await;
        self.absorb(&v);
        v
    }

    fn absorb(&mut self, v: &Value) {
        if v.get("status").and_then(|s| s.as_str()) != Some("executed") {
            return;
        }
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let Some(res) = v.get("result") else {
            return;
        };
        match ty {
            "set_chat_models" => {
                if let Some(m) = res.get("imageModel").and_then(|m| m.as_str()) {
                    if !m.is_empty() {
                        self.image_model = Some(m.to_string());
                    }
                }
                let video = res.get("videoModel").and_then(|m| m.as_str()).or_else(|| {
                    res.get("studio")
                        .and_then(|s| s.get("videoModel"))
                        .and_then(|m| m.as_str())
                });
                if let Some(m) = video {
                    if !m.is_empty() {
                        self.video_model = Some(m.to_string());
                    }
                }
            }
            "set_generate_backend" => {
                if let Some(b) = res.get("backend").and_then(|b| b.as_str()) {
                    self.backend = Some(if b == "local" {
                        "local".into()
                    } else {
                        "cloud".into()
                    });
                }
            }
            "pin_lora" | "unpin_lora" => {
                if let Some(arr) = res.get("pinnedLoras").and_then(|a| a.as_array()) {
                    self.loras = arr.iter().filter_map(lora_from_pin_value).collect();
                }
            }
            _ => {}
        }
    }
}

fn lora_from_pin_value(v: &Value) -> Option<LoraIn> {
    if !crate::studio::is_pinnable(v) {
        return None;
    }
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    Some(LoraIn {
        name: name.to_string(),
        scale: v.get("scale").and_then(|s| s.as_f64()).unwrap_or(0.8) as f32,
        path: v.get("path").and_then(|p| p.as_str()).map(str::to_string),
        base_model: v
            .get("baseModel")
            .and_then(|b| b.as_str())
            .map(str::to_string),
    })
}

pub async fn chat(
    State(state): State<AppState>,
    Json(mut req): Json<ChatRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    req.loras = session_lora_pins(std::mem::take(&mut req.loras));
    let mode = req
        .automation_mode
        .as_deref()
        .unwrap_or("auto")
        .to_lowercase();
    let mode = match mode.as_str() {
        "plan" | "auto" | "yolo" => mode,
        _ => "auto".into(),
    };

    let hist_blob = if req.messages.is_empty() {
        String::new()
    } else {
        req.messages
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let media_ask = crate::router::direct_media_ask(&req.message);

    // Freya-owned light router (legacy modality alignment: chat / picture / gif / video).
    let route_system = load_instruction_raw(&state, "route").await;
    let (privacy, alignment, auto_fallbacks, router_cfg) = {
        let or = state.openrouter.read().await;
        (
            or.privacy().clone(),
            or.alignment().clone(),
            or.auto_fallbacks().clone(),
            or.configured_router_model().to_string(),
        )
    };
    let route = {
        let or = state.openrouter.read().await;
        crate::router::classify_prefs(
            &or,
            &route_system,
            &req.message,
            &hist_blob,
            &router_cfg,
            &privacy,
            &alignment,
            &auto_fallbacks,
        )
        .await
    };

    let resolved_agent = crate::router::resolve_agent_model_prefs(
        req.model.as_deref(),
        &route,
        &privacy,
        &alignment,
        &auto_fallbacks,
    );
    let (configured_image, configured_router) = {
        let or = state.openrouter.read().await;
        (
            or.configured_image_model().to_string(),
            or.configured_router_model().to_string(),
        )
    };
    let resolved_router = crate::router::resolve_router_model_prefs(
        &configured_router,
        &privacy,
        &alignment,
        &auto_fallbacks,
    );
    let resolved_image = crate::router::resolve_image_model_choice_prefs(
        req.image_model.as_deref(),
        &route,
        &configured_image,
        &privacy,
        &alignment,
        &auto_fallbacks,
    );
    let video_default = crate::router::resolve_auto_fallback(
        crate::router::AutoFallbackKind::Video,
        &privacy,
        &alignment,
        &auto_fallbacks,
    );
    let stored_video = {
        let cfg = state.config.read().await;
        let v = cfg.studio.video_model.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    };
    let video_pin = req
        .video_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or(stored_video);
    let resolved_video = crate::router::resolve_video_model_choice_prefs(
        video_pin.as_deref(),
        &route,
        &video_default,
        &privacy,
        &alignment,
        &auto_fallbacks,
    );

    let (mut reply, planned) = if req.confirm && !req.actions.is_empty() {
        (
            "Running the confirmed actions.".to_string(),
            req.actions
                .iter()
                .map(|a| PlannedAction {
                    action_type: a.action_type.clone(),
                    args: a.args.clone(),
                })
                .collect::<Vec<_>>(),
        )
    } else if !req.confirm
        && (route.modality.is_motion()
            || matches!(
                media_ask,
                crate::router::DirectMedia::Gif | crate::router::DirectMedia::Video
            ))
    {
        // Execute gif/video via OpenRouter, or the local worker when backend=local.
        let modality = if route.modality.is_motion() {
            route.modality.as_str()
        } else if matches!(media_ask, crate::router::DirectMedia::Gif) {
            "gif"
        } else {
            "video"
        };
        let reply = if mode == "plan" {
            format!("Ready to generate {modality} — press Generate to run it.")
        } else {
            format!(
                "Generating {} — {}",
                modality,
                route.motion_backend_hint().unwrap_or(if modality == "gif" {
                    "local-gif"
                } else {
                    "openrouter-video"
                })
            )
        };
        (
            reply,
            vec![PlannedAction {
                action_type: "generate_video".into(),
                args: json!({
                    "prompt": req.message,
                    "modality": modality,
                }),
            }],
        )
    } else if !req.confirm
        && (matches!(
            media_ask,
            crate::router::DirectMedia::Picture | crate::router::DirectMedia::Series
        ) || (route.modality.is_still_image()
            && !crate::router::message_is_chat_not_media(&req.message)))
    {
        if matches!(media_ask, crate::router::DirectMedia::Series) {
            let reply = if mode == "plan" {
                "Ready to generate a short series — press Generate to run it.".to_string()
            } else {
                "On it — I’ll make a short series of images.".to_string()
            };
            (
                reply,
                vec![PlannedAction {
                    action_type: "run_session".into(),
                    args: json!({ "goal": req.message }),
                }],
            )
        } else {
            let reply = if mode == "plan" {
                "Ready to generate — press Generate to run it.".to_string()
            } else {
                "On it — I’ll make that image for you.".to_string()
            };
            (
                reply,
                vec![PlannedAction {
                    action_type: "generate_image".into(),
                    args: json!({ "prompt": req.message, "batch": false }),
                }],
            )
        }
    } else {
        let mut system = load_instruction_toned(&state, "converse").await;
        if alignment.local_adult_passthrough() {
            system.push_str(crate::prefs::LOCAL_ADULT_PASSTHROUGH_CLAUSE);
        }
        let mut user_blob = req.message.clone();
        if !hist_blob.is_empty() {
            user_blob = format!(
                "{}\n\n--- recent conversation ---\n{}",
                req.message, hist_blob
            );
        }
        if let Some(sess) = &req.session {
            user_blob = format!(
                "{}\n\n--- session context ---\n{}",
                user_blob,
                serde_json::to_string_pretty(sess).unwrap_or_default()
            );
        }
        user_blob = format!(
            "{}\n\n--- freya route ---\nmodality={} reason={}\nautomationMode={}",
            user_blob,
            route.modality.as_str(),
            route.reason,
            mode
        );
        if !req.loras.is_empty() {
            let pinned = req
                .loras
                .iter()
                .map(|l| {
                    format!(
                        "{}{}",
                        l.name,
                        l.base_model
                            .as_deref()
                            .map(|b| format!(" ({b})"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            user_blob = format!("{user_blob}\n\n--- pinned loras ---\n{pinned}");
        }
        let or = state.openrouter.read().await;
        match or
            .chat(&system, &user_blob, resolved_agent.as_deref())
            .await
        {
            Ok(text) => parse_chat_plan(&text, &req.message),
            Err(_) => infer_chat_plan(&req.message),
        }
    };

    // Obvious guidance never renders. A scene or outfit does not need
    // generate/draw/make — the router or the converse agent decides.
    let mut planned: Vec<PlannedAction> = if req.confirm {
        planned
    } else {
        planned
            .into_iter()
            .filter(|a| {
                if matches!(a.action_type.as_str(), "generate_video") {
                    true
                } else if matches!(a.action_type.as_str(), "generate_image" | "run_session") {
                    !crate::router::message_is_chat_not_media(&req.message)
                } else {
                    true
                }
            })
            .collect()
    };
    // Local + uncensored / Show NSFW: a refusal or empty tool prompt is not a
    // substitute scene. The user's text is what the worker should see.
    if req
        .backend
        .as_deref()
        .is_some_and(|b| b.eq_ignore_ascii_case("local"))
        && alignment.local_adult_passthrough()
    {
        for action in &mut planned {
            restore_refused_local_prompt(action, &req.message);
        }
    }

    // Prefer router-resolved image model when executing generates under Auto.
    let mut env = ActionRun {
        loras: req.loras.clone(),
        image_model: Some(resolved_image.clone()),
        video_model: Some(resolved_video.clone()),
        backend: req.backend.clone(),
    };

    let side_effects: Vec<_> = planned
        .iter()
        .filter(|a| is_side_effect_action(&a.action_type))
        .collect();

    // Plan mode: propose side effects; still run discovery tools.
    let mut pending_confirmation = false;
    let mut question: Option<serde_json::Value> = None;
    let mut action_results = Vec::new();

    if mode == "plan" && !req.confirm && !side_effects.is_empty() {
        pending_confirmation = true;
        for a in &planned {
            if is_side_effect_action(&a.action_type) {
                action_results.push(json!({
                    "type": a.action_type,
                    "status": "proposed",
                    "args": a.args,
                }));
            } else {
                action_results.push(env.exec(&state, a).await);
            }
        }
        if reply.is_empty() {
            reply =
                "Here’s what I’d do — confirm to run generate/install, or keep chatting.".into();
        }
    } else if mode == "auto" && !req.confirm {
        let install_count = planned
            .iter()
            .filter(|a| a.action_type == "install_asset")
            .count();
        if install_count > 1 {
            pending_confirmation = true;
            question = Some(json!({
                "kind": "lora",
                "text": "Several LoRA installs are queued — which should Freya install?",
            }));
            for a in &planned {
                let status = if a.action_type == "install_asset" {
                    "proposed"
                } else if is_side_effect_action(&a.action_type) {
                    // Hold generate until installs decided? Prefer propose generate too when installs ambiguous.
                    "proposed"
                } else {
                    // execute discovery
                    action_results.push(env.exec(&state, a).await);
                    continue;
                };
                action_results.push(json!({
                    "type": a.action_type,
                    "status": status,
                    "args": a.args,
                }));
            }
        } else {
            for a in &planned {
                action_results.push(env.exec(&state, a).await);
            }
        }
    } else {
        // YOLO, or confirm, or no side effects in plan with only discovery
        for a in &planned {
            if mode == "plan" && !req.confirm && is_side_effect_action(&a.action_type) {
                action_results.push(json!({
                    "type": a.action_type,
                    "status": "proposed",
                    "args": a.args,
                }));
                pending_confirmation = true;
            } else {
                action_results.push(env.exec(&state, a).await);
            }
        }
    }

    // Aggregate convenience fields for the UI
    let mut ideas = Vec::new();
    let mut loras = Vec::new();
    let mut artifacts = Vec::new();
    let mut civitai_query = String::new();
    for ar in &action_results {
        if let Some(res) = ar.get("result") {
            if let Some(arr) = res.get("ideas").and_then(|i| i.as_array()) {
                for x in arr {
                    if let Some(s) = x.as_str() {
                        ideas.push(json!(s));
                    }
                }
            }
            if let Some(arr) = res.get("loras").and_then(|i| i.as_array()) {
                loras.extend(arr.iter().cloned());
            }
            if let Some(q) = res.get("query").and_then(|q| q.as_str()) {
                civitai_query = q.to_string();
            }
            if matches!(
                ar.get("type").and_then(|t| t.as_str()),
                Some("generate_image" | "generate_video")
            ) {
                artifacts.push(res.clone());
            }
            if let Some(batch) = res.get("results").and_then(|r| r.as_array()) {
                artifacts.extend(batch.iter().cloned());
            }
        }
    }

    // Tool results arrive after the LLM reply — fold worker summaries into reply.
    let reply = enrich_reply_with_worker_results(&reply, &action_results);
    let effective_backend = env.backend.as_deref().or(req.backend.as_deref());
    let local_backend = effective_backend.is_some_and(|b| b.eq_ignore_ascii_case("local"));
    let base_name = if local_backend {
        local_base_display(&state).await
    } else {
        String::new()
    };
    let lora_line = if local_backend {
        pinned_lora_footer(&env.loras)
    } else {
        String::new()
    };
    let reply = enrich_reply_with_route(
        &reply,
        &route,
        resolved_agent.as_deref(),
        &resolved_router,
        &resolved_image,
        &resolved_video,
        &action_results,
        effective_backend,
        &base_name,
        &lora_line,
        &mode,
    );
    let footer_backend = if local_backend { "local" } else { "cloud" };

    Ok(Json(json!({
        "reply": reply,
        "actions": action_results,
        "pendingConfirmation": pending_confirmation,
        "question": question,
        "ideas": ideas,
        "loras": loras,
        "civitaiQuery": if civitai_query.is_empty() { Value::Null } else { json!(civitai_query) },
        "artifacts": artifacts,
        "automationMode": mode,
        "route": route,
        "routeContext": {
            "backend": footer_backend,
            "base": if local_backend { json!(base_name) } else { Value::Null },
            "lora": if local_backend { json!(lora_line) } else { Value::Null },
            "mode": automation_mode_label(&mode),
        },
        "resolvedModels": {
            "agent": resolved_agent,
            "router": resolved_router,
            "image": resolved_image,
            "video": resolved_video,
        },
    })))
}

fn pinned_lora_footer(loras: &[LoraIn]) -> String {
    let names: Vec<&str> = loras
        .iter()
        .map(|l| l.name.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    }
}

fn checkpoint_display_name(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file
        .trim_end_matches(".safetensors")
        .trim_end_matches(".ckpt")
        .trim_end_matches(".pt");
    if stem.is_empty() {
        "none".into()
    } else {
        stem.replace('_', " ")
    }
}

async fn local_base_display(state: &AppState) -> String {
    let worker = state.worker.read().await;
    let Some(client) = worker.as_ref() else {
        return "none".into();
    };
    let active = match client.get_active_base().await {
        Ok(v) => v,
        Err(_) => return "none".into(),
    };
    let model_path = active
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let family = active
        .get("baseModel")
        .and_then(|b| b.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if model_path.is_empty() {
        return if family.is_empty() {
            "none".into()
        } else {
            family
        };
    }
    if let Ok(items) = client.list_checkpoints().await {
        if let Some(hit) = items.iter().find(|c| c.path == model_path) {
            if let Some(name) = hit.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                return name.to_string();
            }
        }
    }
    checkpoint_display_name(&model_path)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub enhance: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub style_ref: Option<String>,
    #[serde(default)]
    pub loras: Vec<LoraIn>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
    /// `picture` | `gif` | `video` — omit to infer from prompt.
    #[serde(default)]
    pub modality: Option<String>,
    /// Video duration seconds (OpenRouter), when supported.
    #[serde(default)]
    pub duration: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoraIn {
    pub name: String,
    #[serde(default = "one")]
    pub scale: f32,
    /// Relative path on the worker (`loras/foo.safetensors`) when known.
    #[serde(default)]
    pub path: Option<String>,
    /// Civitai base family (SDXL 1.0, Pony, …) — used to pick a checkpoint.
    #[serde(default)]
    pub base_model: Option<String>,
}

fn one() -> f32 {
    1.0
}

/// Session pins are LoRA/adapter paths only — never treat checkpoints/VAE as LoRAs.
fn session_lora_pins(loras: Vec<LoraIn>) -> Vec<LoraIn> {
    loras
        .into_iter()
        .filter(|l| {
            let path = l.path.as_deref().unwrap_or("").to_ascii_lowercase();
            if path.starts_with("checkpoints/") || path.starts_with("vae/") {
                return false;
            }
            let name = l.name.to_ascii_lowercase();
            !(name.contains("checkpoint") && path.is_empty())
        })
        .collect()
}

pub async fn generate(
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    run_generate(&state, req).await.map(Json)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesRequest {
    pub goal: String,
    #[serde(default)]
    pub style_ref: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub loras: Vec<LoraIn>,
}

/// Short still series — same planner as the chat `run_session` action.
pub async fn generate_series(
    State(state): State<AppState>,
    Json(req): Json<SeriesRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let goal = req.goal.trim();
    if goal.is_empty() {
        return Err(ApiError::Msg("missing goal".into()));
    }
    let image = state.config.read().await.image_model.clone();
    let action = PlannedAction {
        action_type: "run_session".into(),
        args: json!({
            "goal": goal,
            "styleRef": req.style_ref,
        }),
    };
    let pins = session_lora_pins(req.loras);
    let v = execute_chat_action(
        &state,
        &action,
        &pins,
        Some(image.as_str()),
        None,
        req.backend.as_deref(),
    )
    .await;
    Ok(Json(v))
}

pub async fn run(
    State(state): State<AppState>,
    Json(mut req): Json<GenerateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let route_system = load_instruction_raw(&state, "route").await;
    let (route, configured_image, privacy, alignment, auto_fallbacks) = {
        let or = state.openrouter.read().await;
        let router_cfg = or.configured_router_model().to_string();
        let configured_image = or.configured_image_model().to_string();
        let privacy = or.privacy().clone();
        let alignment = or.alignment().clone();
        let auto_fallbacks = or.auto_fallbacks().clone();
        let route = crate::router::classify_prefs(
            &or,
            &route_system,
            &req.prompt,
            "",
            &router_cfg,
            &privacy,
            &alignment,
            &auto_fallbacks,
        )
        .await;
        (route, configured_image, privacy, alignment, auto_fallbacks)
    };
    if req.modality.is_none() && route.modality.is_motion() {
        req.modality = Some(route.modality.as_str().into());
    }

    // Auto-plan when backend/model unset — still respect Freya router for image model.
    // Skip cloud model fill when the client already pinned Local (use worker active base).
    let pinned_local = req.backend.as_deref() == Some("local");
    if req.backend.as_deref().unwrap_or("auto") == "auto" && req.model.is_none() {
        let system = load_instruction_toned(&state, "plan").await;
        let or = state.openrouter.read().await;
        if let Ok(plan_txt) = or.chat(&system, &req.prompt, None).await {
            if let Ok(plan) = serde_json::from_str::<serde_json::Value>(
                plan_txt
                    .trim()
                    .trim_start_matches("```json")
                    .trim_start_matches("```")
                    .trim_end_matches("```")
                    .trim(),
            ) {
                if let Some(b) = plan.get("backend").and_then(|x| x.as_str()) {
                    req.backend = Some(b.into());
                }
                if let Some(m) = plan.get("model").and_then(|x| x.as_str()) {
                    let default_image = crate::router::resolve_image_model_choice_prefs(
                        None,
                        &route,
                        &configured_image,
                        &privacy,
                        &alignment,
                        &auto_fallbacks,
                    );
                    let resolved = crate::openrouter::resolve_image_model(m, &default_image);
                    if resolved != m {
                        tracing::debug!(
                            planner_model = %m,
                            resolved = %resolved,
                            "ignored non-image planner model"
                        );
                    }
                    req.model = Some(resolved);
                }
                if let Some(s) = plan.get("styleId").and_then(|x| x.as_str()) {
                    req.style_ref = Some(s.into());
                }
                if req.enhance.is_none() {
                    if let Some(e) = plan.get("enhanceMode").and_then(|x| x.as_str()) {
                        req.enhance = Some(e.into());
                    }
                }
            }
        }
    }
    // Local + Auto: leave model unset so run_generate uses the worker active checkpoint.
    // OpenRouter path still needs a concrete image model id.
    if req.model.is_none() && !pinned_local && req.backend.as_deref() != Some("local") {
        req.model = Some(crate::router::resolve_image_model_choice_prefs(
            None,
            &route,
            &configured_image,
            &privacy,
            &alignment,
            &auto_fallbacks,
        ));
    }
    run_generate(&state, req).await.map(Json)
}

async fn run_generate(state: &AppState, mut req: GenerateRequest) -> ApiResult<serde_json::Value> {
    req.loras = session_lora_pins(std::mem::take(&mut req.loras));
    let styles = styles::load_styles(&state.packs_dir)
        .await
        .unwrap_or_default();
    let style: Option<StylePack> = req
        .style_ref
        .as_ref()
        .and_then(|id| styles.into_iter().find(|s| s.id == *id));

    if let Some(ref s) = style {
        if req.model.is_none() {
            req.model = s.open_router_model.clone().or(s.local_checkpoint.clone());
        }
        if req.backend.is_none() || req.backend.as_deref() == Some("auto") {
            req.backend = Some(s.backend.clone());
        }
        if req.size.is_none() {
            req.size = s.recommended_size.clone();
        }
        if req.loras.is_empty() {
            req.loras = s
                .loras
                .iter()
                .map(|l| LoraIn {
                    name: l.name.clone(),
                    scale: l.scale,
                    path: None,
                    base_model: None,
                })
                .collect();
        }
        if let Some(tw) = &s.trigger_words {
            if !req.prompt.contains(tw) {
                req.prompt = format!("{tw}, {}", req.prompt);
            }
        }
    }
    req.loras = session_lora_pins(std::mem::take(&mut req.loras));

    // LoRAs only apply on the local worker — prefer local when weights are pinned and a worker exists.
    if !req.loras.is_empty()
        && (req.backend.is_none() || req.backend.as_deref() == Some("auto"))
        && state.worker.read().await.is_some()
    {
        req.backend = Some("local".into());
    }

    let backend = req.backend.clone().unwrap_or_else(|| "openrouter".into());
    let enhance = req.enhance.clone().unwrap_or_else(|| "auto".into());
    let (alignment, privacy, auto_fallbacks, chat_pin) = {
        let cfg = state.config.read().await;
        (
            cfg.alignment_prefs(),
            cfg.privacy_prefs(),
            cfg.auto_fallbacks_prefs(),
            cfg.chat_model.clone(),
        )
    };
    let mut final_prompt = req.prompt.clone();
    let mut enhanced = None;

    // Local generate always rewrites when enhance is on. Ordinary text uses the
    // balanced Auto agent (or an explicit pin). An adult scene uses the
    // uncensored writer so the worker gets a checkpoint prompt, not a refusal.
    // Cloud image generation still uses the configured chat model.
    if crate::prefs::enhance_enabled(&enhance) {
        let local = backend.eq_ignore_ascii_case("local");
        let mode = if enhance == "prompt-sd" || local {
            "prompt-sd"
        } else {
            "prompt-prose"
        };
        let mut system = load_instruction_toned(&state, mode).await;
        let path = if local {
            Some(crate::router::local_enhance_path(
                &req.prompt,
                &alignment,
                Some(&chat_pin),
                &privacy,
                &auto_fallbacks,
            ))
        } else {
            None
        };
        if path.as_ref().is_some_and(|p| p.adult) {
            system.push_str(crate::prefs::LOCAL_ADULT_ENHANCE_CLAUSE);
        }
        let or = state.openrouter.read().await;
        let first = path.as_ref().map(|p| p.model.as_str());
        let rewritten = match or.chat(&system, &req.prompt, first).await {
            Ok(text) => Some(text),
            Err(e) => {
                tracing::debug!(error = %e, "prompt rewrite failed");
                None
            }
        };
        if let Some(p) = &path {
            tracing::info!(adult = p.adult, model = %p.model, "local prompt-sd writer");
            if p.adult {
                let accepted = rewritten
                    .as_deref()
                    .map(normalize_sd_rewrite)
                    .filter(|t| adult_rewrite_ok(t, &req.prompt));
                if let Some(text) = accepted {
                    final_prompt = text.clone();
                    enhanced = Some(text);
                } else {
                    let matrix = crate::router::local_adult_matrix_model(&privacy, &auto_fallbacks);
                    let retry = if matrix != p.model {
                        or.chat(&system, &req.prompt, Some(&matrix)).await.ok()
                    } else {
                        None
                    };
                    if let Some(text) = retry
                        .as_deref()
                        .map(normalize_sd_rewrite)
                        .filter(|t| adult_rewrite_ok(t, &req.prompt))
                    {
                        final_prompt = text.clone();
                        enhanced = Some(text);
                    } else {
                        return Err(ApiError::Msg(
                            "local prompt rewrite did not return a scene prompt".into(),
                        ));
                    }
                }
            } else if let Some(text) = rewritten.as_deref().map(normalize_sd_rewrite) {
                if !text.is_empty() && !looks_like_model_refusal(&text) {
                    final_prompt = text.clone();
                    enhanced = Some(text);
                }
            }
        } else if let Some(text) = rewritten {
            enhanced = Some(text.clone());
            final_prompt = text;
        }
    }

    let modality = resolve_generate_modality(&req);
    let (bytes, mime, model_used, backend_used) = if modality.is_motion() {
        run_motion_generate(state, &req, &final_prompt, modality).await?
    } else if backend == "local" {
        let worker = state.worker.read().await;
        let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
        // Prefer installed basename for lora_request so it matches on-disk files.
        let loras: Vec<LoraWeight> = req
            .loras
            .iter()
            .map(|l| {
                let lora_name = l
                    .path
                    .as_deref()
                    .and_then(|p| std::path::Path::new(p).file_stem())
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(l.name.as_str())
                    .to_string();
                LoraWeight {
                    lora_name,
                    lora_scale: l.scale,
                }
            })
            .collect();
        // Resolve base checkpoint: explicit local path → worker active → LoRA baseModel hint.
        // Never forward Auto / OpenRouter cloud ids / router prose as Diffusers repo ids.
        let mut model = crate::base_models::filter_local_model(req.model.as_deref());
        if model.is_none() {
            if let Some(raw) = req
                .model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                tracing::debug!(
                    ignored_model = %raw,
                    "local generate ignored non-checkpoint model; using worker active base"
                );
            }
        }
        let mut active_base_label: Option<String> = None;
        if let Ok(active) = client.get_active_base().await {
            active_base_label = active
                .get("baseModel")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            if model.is_none() {
                if let Some(m) = active.get("model").and_then(|x| x.as_str()) {
                    if !m.is_empty() {
                        model = Some(m.to_string());
                    }
                }
            }
        }
        if model.is_none() {
            let hints: Vec<_> = req
                .loras
                .iter()
                .map(|l| crate::base_models::LoraBaseHint {
                    base_model: l.base_model.clone(),
                })
                .collect();
            model = crate::base_models::resolve_model_from_loras(&hints);
        }
        if let (Some(active_b), Some(lora_b)) = (
            active_base_label.as_deref(),
            req.loras.iter().find_map(|l| l.base_model.as_deref()),
        ) {
            if !crate::base_models::bases_compatible(active_b, lora_b) {
                tracing::warn!(
                    active = %active_b,
                    lora_base = %lora_b,
                    "active worker base may not match pinned LoRA baseModel"
                );
            }
        }
        let bytes = client
            .generate(
                &final_prompt,
                model.as_deref(),
                &loras,
                req.size.as_deref(),
                req.seed,
                req.negative_prompt.as_deref(),
            )
            .await
            .map_err(ApiError::from)?;
        (
            bytes,
            "image/png".to_string(),
            model.unwrap_or_else(|| "local".into()),
            "local".to_string(),
        )
    } else {
        let or = state.openrouter.read().await;
        let default_image = or.default_image_model();
        let model = req.model.as_deref().map(|m| {
            if crate::router::is_auto_image(m) {
                default_image.clone()
            } else {
                crate::openrouter::resolve_image_model(m, &default_image)
            }
        });
        let img = or
            .generate_image(&final_prompt, model.as_deref(), req.size.as_deref())
            .await
            .map_err(ApiError::from)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&img.b64)
            .map_err(|e| ApiError::Msg(e.to_string()))?;
        (
            bytes,
            img.mime,
            model.unwrap_or(default_image),
            "openrouter".to_string(),
        )
    };

    let mut meta = ArtifactMeta {
        id: String::new(),
        prompt: req.prompt.clone(),
        enhanced_prompt: enhanced.clone(),
        backend: backend_used.clone(),
        model: model_used.clone(),
        style_ref: req.style_ref.clone(),
        loras: req.loras.iter().map(|l| l.name.clone()).collect(),
        created_at: String::new(),
        mime: mime.clone(),
        filename: String::new(),
    };
    let saved = state
        .gallery
        .save(&bytes, &mime, &mut meta)
        .await
        .map_err(ApiError::from)?;

    Ok(json!({
        "artifactId": saved.id,
        "backend": backend_used,
        "model": model_used,
        "enhancedPrompt": enhanced,
        "modality": modality.as_str(),
        "mime": mime,
        "url": format!("/v1/gallery/{}/image", saved.id),
        "meta": saved,
    }))
}

fn resolve_generate_modality(req: &GenerateRequest) -> crate::router::Modality {
    if let Some(m) = req
        .modality
        .as_deref()
        .and_then(crate::router::Modality::parse)
    {
        return m;
    }
    crate::router::infer_modality(&req.prompt).modality
}

async fn run_motion_generate(
    state: &AppState,
    req: &GenerateRequest,
    final_prompt: &str,
    modality: crate::router::Modality,
) -> ApiResult<(Vec<u8>, String, String, String)> {
    let want_gif = matches!(modality, crate::router::Modality::Gif);
    let backend = req.backend.as_deref().unwrap_or("auto");
    let pinned_local = backend == "local";

    // Local motion is a worker GIF (the engine has no mp4 T2V). Cloud video stays on OpenRouter.
    // Do not fall through to OpenRouter when the composer pinned Local.
    if pinned_local || (want_gif && backend == "auto") {
        let worker = state.worker.read().await;
        if let Some(client) = worker.as_ref() {
            let gif_model = crate::base_models::filter_local_model(req.model.as_deref());
            match client
                .generate_gif(
                    final_prompt,
                    gif_model.as_deref(),
                    req.size.as_deref(),
                    req.seed,
                    req.negative_prompt.as_deref(),
                )
                .await
            {
                Ok(bytes) => {
                    return Ok((
                        bytes,
                        "image/gif".into(),
                        gif_model.unwrap_or_else(|| "local-gif".into()),
                        "local".into(),
                    ));
                }
                Err(e) => {
                    if pinned_local {
                        return Err(ApiError::Msg(format!(
                            "local worker motion failed (GIF is the local video path): {e}"
                        )));
                    }
                    tracing::warn!(error = %e, "local gif failed; falling back to OpenRouter video");
                }
            }
        } else if pinned_local {
            return Err(ApiError::NoWorker);
        }
    }

    let or = state.openrouter.read().await;
    let privacy = or.privacy().clone();
    let alignment = or.alignment().clone();
    let auto_fallbacks = or.auto_fallbacks().clone();
    let video_default = crate::router::resolve_auto_fallback(
        crate::router::AutoFallbackKind::Video,
        &privacy,
        &alignment,
        &auto_fallbacks,
    );
    let model = crate::router::resolve_video_model_choice_prefs(
        req.model.as_deref(),
        &crate::router::RouteDecision {
            modality,
            reason: String::new(),
            agent_model: None,
            image_model: None,
            source: "generate".into(),
        },
        &video_default,
        &privacy,
        &alignment,
        &auto_fallbacks,
    );
    let duration = req.duration.or(if want_gif { Some(4) } else { Some(5) });
    let aspect = req.size.as_deref().and_then(size_to_video_aspect);
    let vid = or
        .generate_video(final_prompt, Some(&model), duration, aspect)
        .await
        .map_err(ApiError::from)?;
    let (bytes, mime) = if want_gif {
        match mp4_to_gif(&vid.bytes).await {
            Ok(gif) => (gif, "image/gif".into()),
            Err(e) => {
                tracing::warn!(error = %e, "gif convert failed; serving mp4");
                (vid.bytes, vid.mime)
            }
        }
    } else {
        (vid.bytes, vid.mime)
    };
    Ok((bytes, mime, vid.model, "openrouter".into()))
}

fn size_to_video_aspect(size: &str) -> Option<&'static str> {
    let s = size.to_ascii_lowercase();
    if s.contains("1024x1792") || s.contains("9:16") || s.contains("576x1024") {
        Some("9:16")
    } else if s.contains("1792x1024") || s.contains("16:9") || s.contains("1280x720") {
        Some("16:9")
    } else if s.contains("1024x1024") || s.contains("1:1") {
        Some("1:1")
    } else {
        None
    }
}

async fn mp4_to_gif(mp4: &[u8]) -> Result<Vec<u8>, String> {
    let id = uuid::Uuid::new_v4();
    let dir = std::env::temp_dir().join(format!("freya-gif-{id}"));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let in_path = dir.join("in.mp4");
    let out_path = dir.join("out.gif");
    let result = async {
        tokio::fs::write(&in_path, mp4)
            .await
            .map_err(|e| e.to_string())?;
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                in_path.to_str().unwrap_or("in.mp4"),
                "-vf",
                "fps=8,scale=512:-1:flags=lanczos",
                "-loop",
                "0",
                out_path.to_str().unwrap_or("out.gif"),
            ])
            .output()
            .await
            .map_err(|e| format!("ffmpeg: {e}"))?;
        if !status.status.success() {
            return Err(format!(
                "ffmpeg exit {}: {}",
                status.status,
                String::from_utf8_lossy(&status.stderr)
            ));
        }
        tokio::fs::read(&out_path).await.map_err(|e| e.to_string())
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
    result
}

pub async fn gallery_list(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let items = state.gallery.list().await.map_err(ApiError::from)?;
    let items: Vec<Value> = items.iter().map(with_gallery_url).collect();
    Ok(Json(json!({ "items": items })))
}

pub async fn gallery_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let meta = state
        .gallery
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(with_gallery_url(&meta)))
}

pub async fn gallery_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let removed = state.gallery.delete(&id).await.map_err(ApiError::from)?;
    if !removed {
        return Err(ApiError::NotFound);
    }
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub async fn gallery_image(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let meta = state
        .gallery
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound)?;
    let bytes = tokio::fs::read(state.gallery.image_path(&meta))
        .await
        .map_err(ApiError::from)?;
    let ct = crate::gallery::content_type_for_mime(&meta.mime);
    Ok(([(axum::http::header::CONTENT_TYPE, ct)], bytes).into_response())
}

pub async fn gallery_remix(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let meta = state
        .gallery
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(json!({
        // Prefer the user's natural-language prompt for remix; model brief is secondary
        "prompt": meta.prompt.clone(),
        "enhancedPrompt": meta.enhanced_prompt,
        "backend": meta.backend,
        "model": meta.model,
        "styleRef": meta.style_ref,
        "loras": meta.loras,
    })))
}

async fn models_payload(state: &AppState) -> ApiResult<serde_json::Value> {
    let (allow_nsfw, chat_providers) = {
        let cfg = state.config.read().await;
        (
            cfg.alignment_prefs().allow_nsfw_local,
            cfg.privacy_prefs().chat_providers,
        )
    };
    let or = state.openrouter.read().await;
    let mut cloud = or.list_image_models().await.unwrap_or_default();
    let chat_catalog = or.list_chat_catalog().await.unwrap_or_default();
    let mut chat = chat_catalog.models;
    let mut router_models = chat_catalog.router;
    let chat_available = chat_catalog.available;
    let mut video = or.list_video_models().await.unwrap_or_default();
    // Consistent picker order: vendor (id prefix before `/`) → name → id.
    crate::openrouter::sort_models_by_vendor_name(&mut cloud);
    crate::openrouter::sort_models_by_vendor_name(&mut chat);
    crate::openrouter::sort_models_by_vendor_name(&mut router_models);
    crate::openrouter::sort_video_models_by_vendor_name(&mut video);
    let mut local = Vec::new();
    let mut active_base = Value::Null;
    if let Some(w) = state.worker.read().await.as_ref() {
        if let Ok(loras) = w.list_loras().await {
            for a in loras {
                let name = a.name.clone().unwrap_or_else(|| a.id.clone());
                if !allow_nsfw
                    && (looks_nsfw_label(&a.id)
                        || looks_nsfw_label(&name)
                        || looks_nsfw_label(&a.path))
                {
                    continue;
                }
                local.push(json!({
                    "id": a.id,
                    "name": name,
                    "backend": "local",
                    "kind": a.kind,
                    "path": a.path,
                    "baseModel": a.base_model,
                    "sizeBytes": a.size_bytes,
                    "modelType": a.model_type,
                }));
            }
        }
        if let Ok(cps) = w.list_checkpoints().await {
            for a in cps {
                let name = a.name.clone().unwrap_or_else(|| a.id.clone());
                if !allow_nsfw
                    && (looks_nsfw_label(&a.id)
                        || looks_nsfw_label(&name)
                        || looks_nsfw_label(&a.path))
                {
                    continue;
                }
                local.push(json!({
                    "id": a.id,
                    "name": name,
                    "backend": "local",
                    "kind": "checkpoint",
                    "path": a.path,
                    "baseModel": a.base_model,
                    "sizeBytes": a.size_bytes,
                    "modelType": a.model_type,
                }));
            }
        }
        if let Ok(v) = w.get_active_base().await {
            active_base = v;
        }
    }
    Ok(json!({
        // Image-capable cloud models (OpenRouter GET /images/models).
        "openrouter": cloud.drain(..).map(|m| json!({
            "id": m.id, "name": m.name, "backend": m.backend, "modality": "image"
        })).collect::<Vec<_>>(),
        // Tool-calling text models for the Agent picker.
        "chat": chat.drain(..).map(|m| json!({
            "id": m.id, "name": m.name, "backend": m.backend, "modality": "chat"
        })).collect::<Vec<_>>(),
        // Structured-JSON / tool-calling text models for the Router picker.
        "router": router_models.drain(..).map(|m| json!({
            "id": m.id, "name": m.name, "backend": m.backend, "modality": "chat"
        })).collect::<Vec<_>>(),
        // Video: OpenRouter async POST /videos (+ poll). Freya generate/chat routes gif|video here.
        "video": video.drain(..).map(|m| json!({
            "id": m.id,
            "name": m.name,
            "backend": "openrouter",
            "modality": "video",
            "supportedDurations": m.supported_durations,
            "supportedResolutions": m.supported_resolutions,
            "supportedAspectRatios": m.supported_aspect_ratios,
        })).collect::<Vec<_>>(),
        "local": local,
        "activeBase": active_base,
        "baseModelCatalog": crate::base_models::catalog(),
        // Vendor prefixes from denylist-passing chat catalog + current checkbox prefs.
        "chatProviders": {
            "available": chat_available,
            "excluded": chat_providers.excluded,
            "legacyOnly": chat_providers.legacy_only,
        },
    }))
}

pub async fn models(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    models_payload(&state).await.map(Json)
}

pub async fn styles_list(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let items = styles::load_styles(&state.packs_dir)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
pub struct CivitaiSearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Optional comma-separated Civitai ModelTypes (e.g. `LORA,Checkpoint,VAE`).
    /// Empty / omitted → Freya defaults (adapters, checkpoints, embeddings, VAE).
    #[serde(default)]
    pub types: Option<String>,
}

fn default_limit() -> u32 {
    20
}

fn parse_civitai_types(raw: Option<&str>) -> Vec<String> {
    let owned: Vec<String> = raw
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if owned.is_empty() {
        crate::civitai::DEFAULT_SEARCH_TYPES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        owned
    }
}

pub async fn civitai_search(
    State(state): State<AppState>,
    Query(q): Query<CivitaiSearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let allow_nsfw = {
        let cfg = state.config.read().await;
        cfg.alignment_prefs().allow_nsfw_local
    };
    let civ = state.civitai.read().await;
    let types = parse_civitai_types(q.types.as_deref());
    let type_refs: Vec<&str> = types.iter().map(String::as_str).collect();
    let mut items = civ
        .search_types(&q.q, q.limit, &type_refs)
        .await
        .map_err(ApiError::from)?;
    if !allow_nsfw {
        items.retain(|c| !looks_nsfw_label(&c.name) && !c.nsfw);
    }
    Ok(Json(json!({ "items": items })))
}

fn civitai_detail_response(detail: crate::civitai::CivitaiModelDetail) -> Json<serde_json::Value> {
    // Nested shape for the UI modal: { model, related: [{ model, note }] }.
    let related: Vec<_> = detail
        .related
        .iter()
        .map(|r| {
            json!({
                "relation": "compatible_checkpoint",
                "note": detail.base_model.as_ref().map(|b| format!("Popular checkpoint for {b}")),
                "model": r,
            })
        })
        .collect();
    let mut model = serde_json::to_value(&detail).unwrap_or(json!({}));
    if let Some(obj) = model.as_object_mut() {
        // Keep related nested under the response envelope, not duplicated on model.
        obj.remove("related");
    }
    Json(json!({ "model": model, "related": related }))
}

/// Full Civitai model detail + related compatible checkpoints (when applicable).
pub async fn civitai_model(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let civ = state.civitai.read().await;
    let detail = civ.get_model(id).await.map_err(ApiError::from)?;
    Ok(civitai_detail_response(detail))
}

/// Resolve Installed assets that only stored `versionId` (no `modelId` in sidecar meta).
pub async fn civitai_model_version(
    State(state): State<AppState>,
    axum::extract::Path(version_id): axum::extract::Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let civ = state.civitai.read().await;
    let detail = civ
        .get_model_by_version(version_id)
        .await
        .map_err(ApiError::from)?;
    Ok(civitai_detail_response(detail))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCivitaiRequest {
    /// Direct Civitai download URL. Optional when `versionId` is set (resolved server-side).
    #[serde(default)]
    pub download_url: String,
    pub dest_path: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Civitai base family — stored beside the weight for LoRA→checkpoint selection.
    #[serde(default)]
    pub base_model: Option<String>,
    #[serde(default)]
    pub version_id: Option<i64>,
    /// Civitai model id — stored in sidecar meta for Installed → detail.
    #[serde(default)]
    pub model_id: Option<i64>,
    #[serde(default)]
    pub file_id: Option<i64>,
    #[serde(default)]
    pub model_type: Option<String>,
    /// Preview URL persisted in sidecar for Installed card thumbs.
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

/// Allowlist Civitai hosts before attaching the API token (never send token to untrusted URLs).
fn civitai_download_url_ok(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "https" | "http") {
        return false;
    }
    match parsed.host_str() {
        Some(h) => {
            h.eq_ignore_ascii_case("civitai.com")
                || h.to_ascii_lowercase().ends_with(".civitai.com")
        }
        None => false,
    }
}

/// Install a Civitai download onto the configured diffusion worker (remote or in-cluster URL).
pub async fn civitai_install(
    State(state): State<AppState>,
    Json(req): Json<InstallCivitaiRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    civitai_install_inner(&state, req).await.map(Json)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionSetting {
    pub id: String,
    pub label: String,
    /// Effective text (override if set, else pack/builtin default).
    pub text: String,
    /// Pack / builtin default (for Reset UI).
    pub default_text: String,
    pub overridden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPublic {
    pub open_router_configured: bool,
    pub civitai_configured: bool,
    pub hf_configured: bool,
    pub diffusion_worker_url: Option<String>,
    pub worker_enabled: bool,
    /// Agent model (`auto` = internal Auto; `openrouter/auto` is the explicit upstream router).
    pub chat_model: String,
    /// Image model (`auto` = Auto → Flux NL default).
    pub image_model: String,
    /// Freya intent router model (`auto` = Llama 3.1 8B-class).
    pub router_model: String,
    /// Whether `FREYA_API_TOKEN` / rotated runtime token is set (never returns the value).
    pub api_token_configured: bool,
    /// Whether operator admin login (`FREYA_ADMIN_PASSWORD`) is configured.
    pub admin_login_configured: bool,
    /// True when bearer and/or admin password is required for `/v1`.
    pub auth_required: bool,
    /// OpenRouter privacy (ZDR / data_collection) + Freya chat-provider checkboxes.
    pub privacy: PrivacyPrefs,
    /// Freya-side model catalog / tone prefs (not OpenRouter API flags).
    pub alignment: AlignmentPrefs,
    /// Configurable Auto fallbacks when the router does not pin a concrete model.
    pub auto_fallbacks: AutoFallbacksPrefs,
    /// Editable system prompts (packs defaults + Settings overrides).
    pub instructions: Vec<InstructionSetting>,
    /// Favorites, LoRA pins, composer backend, video pin. No secrets.
    pub studio: crate::studio::StudioPrefs,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPutResponse {
    #[serde(flatten)]
    pub settings: SettingsPublic,
    /// Whether the secrets snapshot was written to `FREYA_SECRETS_FILE`.
    pub persisted: bool,
}

async fn settings_public_from_state(state: &AppState) -> SettingsPublic {
    let (base, overrides) = {
        let cfg = state.config.read().await;
        (
            SettingsPublic {
                open_router_configured: cfg.openrouter_configured(),
                civitai_configured: cfg.civitai_configured(),
                hf_configured: !cfg.hf_token.is_empty(),
                diffusion_worker_url: cfg.diffusion_worker_url.clone(),
                worker_enabled: cfg.worker_enabled,
                chat_model: cfg.chat_model.clone(),
                image_model: cfg.image_model.clone(),
                router_model: cfg.router_model.clone(),
                api_token_configured: !cfg.api_token.trim().is_empty(),
                admin_login_configured: cfg.admin_login_configured(),
                auth_required: cfg.auth_required(),
                privacy: cfg.privacy_prefs(),
                alignment: cfg.alignment_prefs(),
                auto_fallbacks: cfg.auto_fallbacks_prefs(),
                instructions: Vec::new(),
                studio: cfg.studio.clone(),
            },
            cfg.instruction_overrides.clone(),
        )
    };
    let mut instructions = Vec::with_capacity(styles::EDITABLE_INSTRUCTION_MODES.len());
    for id in styles::EDITABLE_INSTRUCTION_MODES {
        let default_text = styles::load_instruction_default(&state.packs_dir, id).await;
        let overridden = overrides
            .get(*id)
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);
        let text = if overridden {
            overrides
                .get(*id)
                .cloned()
                .unwrap_or_else(|| default_text.clone())
        } else {
            default_text.clone()
        };
        instructions.push(InstructionSetting {
            id: (*id).to_string(),
            label: styles::instruction_mode_label(id),
            text,
            default_text,
            overridden,
        });
    }
    SettingsPublic {
        instructions,
        ..base
    }
}

async fn load_instruction_toned(state: &AppState, mode: &str) -> String {
    let (tone, overrides) = {
        let cfg = state.config.read().await;
        (cfg.alignment_prefs(), cfg.instruction_overrides.clone())
    };
    let raw = styles::load_instruction(&state.packs_dir, mode, Some(&overrides)).await;
    tone.apply_tone(&raw)
}

async fn load_instruction_raw(state: &AppState, mode: &str) -> String {
    let overrides = {
        let cfg = state.config.read().await;
        cfg.instruction_overrides.clone()
    };
    styles::load_instruction(&state.packs_dir, mode, Some(&overrides)).await
}

/// Public auth probe — no bearer required. Used by the UI to show Sign in / locked state.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatusResponse {
    pub auth_required: bool,
    pub admin_login_configured: bool,
    pub api_token_configured: bool,
    pub open_router_configured: bool,
    pub admin_username: String,
    pub note: String,
}

pub async fn auth_status(State(state): State<AppState>) -> Json<AuthStatusResponse> {
    let cfg = state.config.read().await;
    let admin_login = cfg.admin_login_configured();
    let note = if !cfg.auth_required() {
        "API open (local/dev).".into()
    } else if admin_login {
        "Sign in required.".into()
    } else {
        "API token required.".into()
    };
    Json(AuthStatusResponse {
        auth_required: cfg.auth_required(),
        admin_login_configured: admin_login,
        api_token_configured: !cfg.api_token.trim().is_empty(),
        open_router_configured: cfg.openrouter_configured(),
        admin_username: if cfg.admin_user.trim().is_empty() {
            "admin".into()
        } else {
            cfg.admin_user.clone()
        },
        note,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthLoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthLoginResponse {
    pub token: String,
    pub username: String,
    pub note: String,
}

/// Exchange admin username/password for a browser session token.
pub async fn auth_login(
    State(state): State<AppState>,
    Json(req): Json<AuthLoginRequest>,
) -> ApiResult<Json<AuthLoginResponse>> {
    let (admin_user, admin_password, mut token) = {
        let cfg = state.config.read().await;
        (
            if cfg.admin_user.trim().is_empty() {
                "admin".to_string()
            } else {
                cfg.admin_user.trim().to_string()
            },
            cfg.admin_password.trim().to_string(),
            cfg.api_token.trim().to_string(),
        )
    };
    if admin_password.is_empty() {
        return Err(ApiError::Unavailable(
            "Admin login is not configured on this Studio".into(),
        ));
    }
    let user_ok = const_eq_str(req.username.trim(), &admin_user);
    let pass_ok = const_eq_str(req.password.trim(), &admin_password);
    if !user_ok || !pass_ok {
        return Err(ApiError::Unauthorized(
            "Invalid username or password".into(),
        ));
    }
    if token.is_empty() {
        // No machine bearer yet — mint one so the UI can stay on Bearer after login.
        token = generate_api_token();
        {
            let mut cfg = state.config.write().await;
            cfg.api_token = token.clone();
        }
        let _ = state.persist_secrets().await;
        let _ = crate::k8s_secret::try_sync_api_token_secret(&token).await;
    }
    Ok(Json(AuthLoginResponse {
        token,
        username: admin_user,
        note: "Signed in.".into(),
    }))
}

fn const_eq_str(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

pub async fn settings_get(State(state): State<AppState>) -> Json<SettingsPublic> {
    Json(settings_public_from_state(&state).await)
}

pub async fn studio_get(State(state): State<AppState>) -> Json<crate::studio::StudioPrefs> {
    Json(state.config.read().await.studio.clone())
}

pub async fn studio_put(
    State(state): State<AppState>,
    Json(patch): Json<crate::studio::StudioPatch>,
) -> ApiResult<Json<serde_json::Value>> {
    {
        let mut cfg = state.config.write().await;
        crate::studio::apply_patch(&mut cfg.studio, patch).map_err(ApiError::Msg)?;
    }
    let persisted = state.persist_secrets().await.is_ok();
    let studio = state.config.read().await.studio.clone();
    Ok(Json(json!({ "studio": studio, "persisted": persisted })))
}

/// OpenRouter usage / credits snapshot for the Metrics UI.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterMetricsResponse {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<crate::openrouter::OpenRouterKeyUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits: Option<crate::openrouter::OpenRouterCredits>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_error: Option<String>,
    pub notes: Vec<String>,
}

async fn openrouter_metrics_payload(state: &AppState) -> ApiResult<OpenRouterMetricsResponse> {
    let configured = {
        let cfg = state.config.read().await;
        cfg.openrouter_configured()
    };
    let notes = vec![
        "GET /api/v1/key works with Freya’s normal completion key: per-key spend (day/week/month) and optional key spend cap.".into(),
        "GET /api/v1/credits (account purchased − used) requires an OpenRouter Management API key — unavailable with a normal key.".into(),
        "OpenRouter does not expose a rich recent-transactions feed for completion keys; Freya shows lifetime + period usage from /key.".into(),
    ];
    if !configured {
        return Ok(OpenRouterMetricsResponse {
            configured: false,
            key: None,
            key_error: Some("OpenRouter API key not configured".into()),
            credits: None,
            credits_error: None,
            notes,
        });
    }

    let or = state.openrouter.read().await;
    let (key, key_error) = match or.key_usage().await {
        Ok(k) => (Some(k), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let (credits, credits_error) = match or.account_credits().await {
        Ok(c) => (Some(c), None),
        Err(e) => (None, Some(e.to_string())),
    };

    Ok(OpenRouterMetricsResponse {
        configured: true,
        key,
        key_error,
        credits,
        credits_error,
        notes,
    })
}

pub async fn openrouter_metrics(
    State(state): State<AppState>,
) -> ApiResult<Json<OpenRouterMetricsResponse>> {
    openrouter_metrics_payload(&state).await.map(Json)
}

/// Generate or rotate the studio API bearer. Shown once in the response; also stored in
/// `FREYA_SECRETS_FILE` so local/kind restarts keep it when that path is durable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthTokenResponse {
    pub token: String,
    pub rotated: bool,
    pub persisted: bool,
    /// Whether the Kubernetes Secret named by `FREYA_API_TOKEN_SECRET_NAME` was patched.
    pub secret_synced: bool,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_commands: Option<Vec<String>>,
}

pub async fn auth_token_create(
    State(state): State<AppState>,
) -> ApiResult<Json<AuthTokenResponse>> {
    let token = generate_api_token();
    let (had_token, secret_name, secret_key, ns_hint) = {
        let cfg = state.config.read().await;
        (
            !cfg.api_token.trim().is_empty(),
            cfg.api_token_secret_name.clone(),
            cfg.api_token_secret_key.clone(),
            std::env::var("FREYA_NAMESPACE").unwrap_or_else(|_| "freya".into()),
        )
    };
    {
        let mut cfg = state.config.write().await;
        cfg.api_token = token.clone();
    }
    let persisted = state.persist_secrets().await.is_ok();
    let secret_synced = crate::k8s_secret::try_sync_api_token_secret(&token).await;

    let sec = if secret_name.trim().is_empty() {
        "freya-api-token".to_string()
    } else {
        secret_name
    };
    let key = if secret_key.trim().is_empty() {
        "token".to_string()
    } else {
        secret_key
    };
    let sync_commands = if secret_synced {
        Some(vec![format!(
            "kubectl -n {ns_hint} rollout restart deploy/freya-mcp  # pick up Secret {sec}"
        )])
    } else {
        Some(vec![
            format!(
                "kubectl -n {ns_hint} create secret generic {sec} --from-literal={key}='{token}' --dry-run=client -o yaml | kubectl apply -f -"
            ),
            format!("kubectl -n {ns_hint} rollout restart deploy/freya-mcp"),
            "./scripts/sync-api-token-from-pvc.sh".into(),
        ])
    };
    let note = match (persisted, secret_synced) {
        (true, true) => "Machine API token rotated and synced.".into(),
        (true, false) => {
            "Machine API token rotated (secrets file). Sync freya-api-token if MCP needs it.".into()
        }
        (false, true) => "Machine API token rotated (in-memory + Secret).".into(),
        (false, false) => "Machine API token rotated (in-memory only) — copy it now.".into(),
    };
    Ok(Json(AuthTokenResponse {
        token,
        rotated: had_token,
        persisted,
        secret_synced,
        note,
        sync_commands,
    }))
}

fn generate_api_token() -> String {
    // 64 hex chars ≈ openssl rand -hex 32
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    #[serde(default)]
    pub openrouter_api_key: Option<String>,
    /// User-provided Civitai API key for search + LoRA installs.
    #[serde(default)]
    pub civitai_api_key: Option<String>,
    #[serde(default)]
    pub hf_token: Option<String>,
    #[serde(default)]
    pub diffusion_worker_url: Option<String>,
    #[serde(default)]
    pub chat_model: Option<String>,
    #[serde(default)]
    pub image_model: Option<String>,
    #[serde(default)]
    pub router_model: Option<String>,
    #[serde(default)]
    pub privacy: Option<PrivacyPrefs>,
    #[serde(default)]
    pub alignment: Option<AlignmentPrefs>,
    #[serde(default)]
    pub auto_fallbacks: Option<AutoFallbacksPrefs>,
    /// Partial merge of system-prompt overrides. Empty string for a mode clears that override
    /// (reset to pack default). Absent keys are left unchanged.
    #[serde(default)]
    pub instruction_overrides: Option<std::collections::HashMap<String, String>>,
}

pub async fn settings_put(
    State(state): State<AppState>,
    Json(upd): Json<SettingsUpdate>,
) -> ApiResult<Json<SettingsPutResponse>> {
    let models_changed =
        upd.chat_model.is_some() || upd.image_model.is_some() || upd.router_model.is_some();
    let prefs_changed =
        upd.privacy.is_some() || upd.alignment.is_some() || upd.auto_fallbacks.is_some();
    {
        let mut cfg = state.config.write().await;
        if let Some(url) = upd.diffusion_worker_url {
            if url.is_empty() {
                cfg.diffusion_worker_url = None;
            } else {
                cfg.diffusion_worker_url = Some(url);
                // Worker is “on” when a URL is configured — no separate enable UX.
                cfg.worker_enabled = true;
            }
        }
        if let Some(m) = upd.chat_model {
            cfg.chat_model = m;
        }
        if let Some(m) = upd.image_model {
            cfg.image_model = m;
        }
        if let Some(m) = upd.router_model {
            cfg.router_model = m;
        }
        if let Some(p) = upd.privacy.clone() {
            cfg.apply_privacy(p);
        }
        if let Some(a) = upd.alignment.clone() {
            cfg.apply_alignment(a);
        }
        if let Some(f) = upd.auto_fallbacks.clone() {
            cfg.apply_auto_fallbacks(f);
        }
        if let Some(patch) = upd.instruction_overrides.clone() {
            for (mode, text) in patch {
                if !styles::is_editable_instruction_mode(&mode) {
                    continue;
                }
                if text.trim().is_empty() {
                    cfg.instruction_overrides.remove(&mode);
                } else {
                    cfg.instruction_overrides.insert(mode, text);
                }
            }
        }
    }

    let secrets = RuntimeSecrets {
        openrouter_api_key: upd.openrouter_api_key,
        civitai_api_key: upd.civitai_api_key,
        hf_token: upd.hf_token,
        diffusion_worker_url: None, // URL already applied above; persist_secrets writes full snapshot
        worker_enabled: None,
        api_token: None,
        chat_model: None,
        image_model: None,
        router_model: None,
        privacy: None,
        alignment: None,
        auto_fallbacks: None,
        legacy_upstream_auto_migrated: false,
        instruction_overrides: None, // persist_secrets writes full snapshot from cfg
        studio: None,
    };
    state.apply_secrets(secrets).await;
    state.rebuild_worker_client().await;
    let persisted = state.persist_secrets().await.is_ok();
    if !persisted {
        tracing::warn!("FREYA_SECRETS_FILE write failed — keys may not survive API restart");
    }

    // OpenRouterClient captures models + privacy/alignment at construction — rebuild after changes.
    if models_changed || prefs_changed {
        let cfg = state.config.read().await;
        let mut or = state.openrouter.write().await;
        *or = crate::openrouter::OpenRouterClient::new(&cfg);
    }

    Ok(Json(SettingsPutResponse {
        settings: settings_public_from_state(&state).await,
        persisted,
    }))
}

#[derive(Debug)]
pub enum ApiError {
    Msg(String),
    Unauthorized(String),
    Unavailable(String),
    NotFound,
    NoWorker,
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Msg(e.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::Msg(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (code, msg) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            ApiError::NoWorker => (
                StatusCode::BAD_REQUEST,
                "No diffusion worker configured — set worker URL or deploy FreyaDiffusionWorker"
                    .into(),
            ),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            ApiError::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            ApiError::Msg(m) => (StatusCode::BAD_REQUEST, m),
        };
        (code, Json(json!({ "error": msg }))).into_response()
    }
}

#[cfg(test)]
mod route_footer_tests {
    use super::*;

    fn decision(modality: crate::router::Modality) -> crate::router::RouteDecision {
        crate::router::RouteDecision {
            modality,
            reason: "t".into(),
            agent_model: None,
            image_model: None,
            source: "t".into(),
        }
    }

    #[test]
    fn local_refusal_prompt_is_replaced_with_the_user_scene() {
        let mut action = PlannedAction {
            action_type: "generate_image".into(),
            args: json!({
                "prompt": "I can't create explicit images, but here is a tasteful portrait."
            }),
        };
        restore_refused_local_prompt(&mut action, "adult scene as written");
        assert_eq!(
            action.args.get("prompt").and_then(|p| p.as_str()),
            Some("adult scene as written")
        );
        let mut kept = PlannedAction {
            action_type: "generate_image".into(),
            args: json!({ "prompt": "adult scene as written, closer crop" }),
        };
        restore_refused_local_prompt(&mut kept, "adult scene as written");
        assert_eq!(
            kept.args.get("prompt").and_then(|p| p.as_str()),
            Some("adult scene as written, closer crop")
        );
    }

    #[test]
    fn local_footer_uses_base_lora_and_mode() {
        let line = format_route_visibility(&RouteFooter {
            route: &decision(crate::router::Modality::Chat),
            agent: Some("google/gemini-2.5-flash"),
            router: "google/gemini-2.5-flash",
            image: "google/gemini-2.5-flash-image",
            video: "google/veo-3.1",
            backend: "local",
            base: "Pony Diffusion V6 XL",
            lora: "none",
            mode: "auto",
        });
        assert_eq!(
            line,
            "Route: chat · agent=gemini-2.5-flash · router=gemini-2.5-flash · base=Pony Diffusion V6 XL · lora=none · mode=Auto · backend=local"
        );
        assert!(!line.contains("image="));
        assert!(!line.contains("video="));
    }

    #[test]
    fn cloud_footer_keeps_image_and_video_models() {
        let line = format_route_visibility(&RouteFooter {
            route: &decision(crate::router::Modality::Picture),
            agent: Some("google/gemini-2.5-flash"),
            router: "google/gemini-2.5-flash",
            image: "google/gemini-2.5-flash-image",
            video: "google/veo-3.1",
            backend: "openrouter",
            base: "",
            lora: "",
            mode: "yolo",
        });
        assert_eq!(
            line,
            "Route: picture · agent=gemini-2.5-flash · router=gemini-2.5-flash · image=gemini-2.5-flash-image · video=veo-3.1 · backend=cloud"
        );
        assert!(!line.contains("base="));
        assert!(!line.contains("lora="));
    }
}
