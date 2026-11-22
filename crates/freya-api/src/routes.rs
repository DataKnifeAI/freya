use crate::gallery::ArtifactMeta;
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
    let worker_ok = if worker_enabled {
        let w = state.worker.read().await;
        match w.as_ref() {
            Some(c) => c.health().await,
            None => false,
        }
    } else {
        false
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

    let mut vram = worker_payload
        .get("vram")
        .cloned()
        .unwrap_or(Value::Null);
    let mut host = worker_payload
        .get("host")
        .cloned()
        .unwrap_or(Value::Null);
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
        "layout": worker_payload.get("layout").cloned().unwrap_or(Value::Null),
        "worker": worker_payload,
    }))
}

pub async fn worker_checkpoints(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let items = client.list_checkpoints().await.map_err(ApiError::from)?;
    Ok(Json(json!({
        "data": items.into_iter().map(|a| json!({
            "id": a.id,
            "name": a.name.unwrap_or_else(|| a.id.clone()),
            "kind": a.kind,
            "path": a.path,
            "baseModel": a.base_model,
            "backend": "local",
        })).collect::<Vec<_>>(),
    })))
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
        (cfg.worker_container.clone(), cfg.worker_systemd_unit.clone())
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
        (cfg.worker_container.clone(), cfg.worker_systemd_unit.clone())
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
    let system = styles::load_instruction(&state.packs_dir, &req.mode).await;
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
    let m = message.to_lowercase();
    const KEYS: &[&str] = &[
        "generate",
        "make an image",
        "make me an image",
        "create an image",
        "draw ",
        "draw me",
        "render ",
        "paint ",
        "produce an image",
        "run session",
        "run a session",
        "make a picture",
        "create a picture",
    ];
    KEYS.iter().any(|k| m.contains(k))
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
            "Happy to talk through it. Tell me when you want me to generate, search Civitai, or suggest ideas — I’ll wait until you ask."
        )
    } else if actions.iter().any(|a| a.action_type == "generate_image" || a.action_type == "run_session") {
        "On it — I’ll make that image for you.".into()
    } else if actions.iter().any(|a| a.action_type == "search_civitai") {
        "I’ll search Civitai for matching assets.".into()
    } else {
        "Here are some directions we could take.".into()
    };
    (reply, actions)
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
    matches!(ty, "generate_image" | "install_asset" | "run_session")
}

async fn execute_chat_action(
    state: &AppState,
    action: &PlannedAction,
    pinned: &[LoraIn],
    image_model: Option<&str>,
) -> serde_json::Value {
    let ty = action.action_type.as_str();
    match ty {
        "suggest_ideas" => {
            let topic = action
                .args
                .get("topic")
                .and_then(|t| t.as_str())
                .unwrap_or("a memorable scene");
            let system = styles::load_instruction(&state.packs_dir, "suggest").await;
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
                            .map(|l| l.trim().trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')').trim())
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
            let mut req = GenerateRequest {
                prompt: prompt.to_string(),
                negative_prompt: None,
                enhance: Some("auto".into()),
                backend: Some(if pinned.is_empty() {
                    "auto".into()
                } else if state.worker.read().await.is_some() {
                    "local".into()
                } else {
                    "auto".into()
                }),
                model: image_model.map(str::to_string),
                style_ref: None,
                loras: pinned.to_vec(),
                size: None,
                seed: None,
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
            let system = styles::load_instruction(&state.packs_dir, "session").await;
            let or = state.openrouter.read().await;
            let batch: Vec<String> = match or.chat(&system, goal, None).await {
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
            let mut results = Vec::new();
            for p in &prompts {
                let mut req = GenerateRequest {
                    prompt: p.clone(),
                    negative_prompt: None,
                    enhance: Some("auto".into()),
                    backend: Some("auto".into()),
                    model: image_model.map(str::to_string),
                    style_ref: None,
                    loras: pinned.to_vec(),
                    size: None,
                    seed: None,
                };
                if req.model.as_deref() == Some("auto") {
                    req.model = None;
                }
                match run_generate(state, req).await {
                    Ok(r) => results.push(r),
                    Err(e) => {
                        let msg = match e {
                            ApiError::Msg(m) => m,
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
            if download_url.is_empty() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing downloadUrl"
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
                version_id: action
                    .args
                    .get("versionId")
                    .and_then(|x| x.as_i64()),
                model_type: action
                    .args
                    .get("modelType")
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
    req: InstallCivitaiRequest,
) -> ApiResult<serde_json::Value> {
    let cfg = state.config.read().await;
    if !cfg.civitai_configured() {
        return Err(ApiError::Msg(
            "Civitai API key required — set it in Settings".into(),
        ));
    }
    if !civitai_download_url_ok(&req.download_url) {
        return Err(ApiError::Msg(
            "downloadUrl must be an http(s) civitai.com URL".into(),
        ));
    }
    let worker = state.worker.read().await;
    let client = worker.as_ref().ok_or(ApiError::NoWorker)?;
    let auth = cfg.civitai_api_key.clone();
    let url = req.download_url.clone();
    drop(cfg);
    let meta = crate::worker::InstallMeta {
        name: req.name.clone(),
        base_model: req.base_model.clone(),
        version_id: req.version_id,
        model_type: req.model_type.clone(),
    };
    client
        .install_asset("civitai", &url, &req.dest_path, Some(&auth), Some(&meta))
        .await
        .map_err(ApiError::from)?;
    Ok(json!({
        "ok": true,
        "destPath": req.dest_path,
        "name": req.name,
        "baseModel": req.base_model,
        "versionId": req.version_id,
        "modelType": req.model_type,
    }))
}

pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let mode = req
        .automation_mode
        .as_deref()
        .unwrap_or("auto")
        .to_lowercase();
    let mode = match mode.as_str() {
        "plan" | "auto" | "yolo" => mode,
        _ => "auto".into(),
    };

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
    } else {
        let system = styles::load_instruction(&state.packs_dir, "converse").await;
        let mut user_blob = req.message.clone();
        if !req.messages.is_empty() {
            let hist: Vec<_> = req
                .messages
                .iter()
                .rev()
                .take(8)
                .rev()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect();
            user_blob = format!(
                "{}\n\n--- recent conversation ---\n{}",
                req.message,
                hist.join("\n")
            );
        }
        if let Some(sess) = &req.session {
            user_blob = format!(
                "{}\n\n--- session context ---\n{}",
                user_blob,
                serde_json::to_string_pretty(sess).unwrap_or_default()
            );
        }
        let or = state.openrouter.read().await;
        match or.chat(&system, &user_blob, req.model.as_deref()).await {
            Ok(text) => parse_chat_plan(&text, &req.message),
            Err(_) => infer_chat_plan(&req.message),
        }
    };

    // Safety net: never invent generate when the user did not ask, unless they confirmed.
    let planned: Vec<PlannedAction> = if req.confirm {
        planned
    } else {
        planned
            .into_iter()
            .filter(|a| {
                if matches!(a.action_type.as_str(), "generate_image" | "run_session") {
                    wants_image_intent(&req.message)
                        || a.args
                            .get("prompt")
                            .and_then(|p| p.as_str())
                            .map(|p| wants_image_intent(p))
                            .unwrap_or(false)
                } else {
                    true
                }
            })
            .collect()
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
                action_results.push(execute_chat_action(&state, a, &req.loras, req.image_model.as_deref()).await);
            }
        }
        if reply.is_empty() {
            reply = "Here’s what I’d do — confirm to run generate/install, or keep chatting.".into();
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
                    action_results.push(
                        execute_chat_action(&state, a, &req.loras, req.image_model.as_deref()).await,
                    );
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
                action_results
                    .push(execute_chat_action(&state, a, &req.loras, req.image_model.as_deref()).await);
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
                action_results
                    .push(execute_chat_action(&state, a, &req.loras, req.image_model.as_deref()).await);
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
            if ar.get("type").and_then(|t| t.as_str()) == Some("generate_image") {
                artifacts.push(res.clone());
            }
            if let Some(batch) = res.get("results").and_then(|r| r.as_array()) {
                artifacts.extend(batch.iter().cloned());
            }
        }
    }

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
    })))
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

pub async fn generate(
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    run_generate(&state, req).await.map(Json)
}

pub async fn run(
    State(state): State<AppState>,
    Json(mut req): Json<GenerateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    // Auto-plan when backend/model unset.
    if req.backend.as_deref().unwrap_or("auto") == "auto" && req.model.is_none() {
        let system = styles::load_instruction(&state.packs_dir, "plan").await;
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
                    // Planners sometimes emit chat routers (openrouter/auto) — keep default image model.
                    let default_image = {
                        let cfg = state.config.read().await;
                        cfg.image_model.clone()
                    };
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
    run_generate(&state, req).await.map(Json)
}

async fn run_generate(state: &AppState, mut req: GenerateRequest) -> ApiResult<serde_json::Value> {
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

    // LoRAs only apply on the local worker — prefer local when weights are pinned and a worker exists.
    if !req.loras.is_empty()
        && (req.backend.is_none() || req.backend.as_deref() == Some("auto"))
        && state.worker.read().await.is_some()
    {
        req.backend = Some("local".into());
    }

    let backend = req.backend.clone().unwrap_or_else(|| "openrouter".into());
    let enhance = req.enhance.clone().unwrap_or_else(|| "auto".into());
    let mut final_prompt = req.prompt.clone();
    let mut enhanced = None;

    let wants_enhance =
        enhance == "on" || enhance == "auto" || enhance == "prompt-prose" || enhance == "prompt-sd";
    if wants_enhance && enhance != "off" {
        let mode = if enhance == "prompt-sd" || backend == "local" {
            "prompt-sd"
        } else {
            "prompt-prose"
        };
        let system = styles::load_instruction(&state.packs_dir, mode).await;
        let or = state.openrouter.read().await;
        if let Ok(text) = or.chat(&system, &req.prompt, None).await {
            enhanced = Some(text.clone());
            final_prompt = text;
        }
    }

    let (bytes, mime, model_used, backend_used) = if backend == "local" {
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
        // Resolve base checkpoint: explicit model → worker active → LoRA baseModel hint.
        let mut model = req.model.clone();
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
        let default_image = or.default_image_model().to_string();
        let model = req
            .model
            .as_deref()
            .map(|m| crate::openrouter::resolve_image_model(m, &default_image));
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
        "url": format!("/v1/gallery/{}/image", saved.id),
        "meta": saved,
    }))
}

pub async fn gallery_list(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let items = state.gallery.list().await.map_err(ApiError::from)?;
    Ok(Json(json!({ "items": items })))
}

pub async fn gallery_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtifactMeta>> {
    let meta = state
        .gallery
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(meta))
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
    let ct = if meta.mime.contains("jpeg") {
        "image/jpeg"
    } else {
        "image/png"
    };
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

pub async fn models(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let or = state.openrouter.read().await;
    let mut cloud = or.list_image_models().await.unwrap_or_default();
    let chat = or.list_chat_models().await.unwrap_or_default();
    let video = or.list_video_models().await.unwrap_or_default();
    let mut local = Vec::new();
    let mut active_base = Value::Null;
    if let Some(w) = state.worker.read().await.as_ref() {
        if let Ok(loras) = w.list_loras().await {
            for a in loras {
                local.push(json!({
                    "id": a.id,
                    "name": a.name.unwrap_or_else(|| a.id.clone()),
                    "backend": "local",
                    "kind": a.kind,
                    "path": a.path,
                    "baseModel": a.base_model,
                }));
            }
        }
        if let Ok(cps) = w.list_checkpoints().await {
            for a in cps {
                local.push(json!({
                    "id": a.id,
                    "name": a.name.unwrap_or_else(|| a.id.clone()),
                    "backend": "local",
                    "kind": "checkpoint",
                    "path": a.path,
                    "baseModel": a.base_model,
                }));
            }
        }
        if let Ok(v) = w.get_active_base().await {
            active_base = v;
        }
    }
    Ok(Json(json!({
        // Image-capable cloud models (OpenRouter GET /images/models).
        "openrouter": cloud.drain(..).map(|m| json!({
            "id": m.id, "name": m.name, "backend": m.backend, "modality": "image"
        })).collect::<Vec<_>>(),
        // Text chat models for the agent picker (filtered subset of GET /models).
        "chat": chat.into_iter().map(|m| json!({
            "id": m.id, "name": m.name, "backend": m.backend, "modality": "chat"
        })).collect::<Vec<_>>(),
        // Video is async on OpenRouter (POST /videos + poll) — listed for discovery; Freya UI
        // does not generate video yet.
        "video": video.into_iter().map(|m| json!({
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
    })))
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
    let civ = state.civitai.read().await;
    let types = parse_civitai_types(q.types.as_deref());
    let type_refs: Vec<&str> = types.iter().map(String::as_str).collect();
    let items = civ
        .search_types(&q.q, q.limit, &type_refs)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({ "items": items })))
}

/// Full Civitai model detail + related compatible checkpoints (when applicable).
pub async fn civitai_model(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let civ = state.civitai.read().await;
    let detail = civ.get_model(id).await.map_err(ApiError::from)?;
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
    let model = json!({
        "id": detail.id,
        "name": detail.name,
        "modelType": detail.model_type,
        "versionId": detail.version_id,
        "versionName": detail.version_name,
        "downloadUrl": detail.download_url,
        "thumbnailUrl": detail.thumbnail_url,
        "examplePrompt": detail.example_prompt,
        "baseModel": detail.base_model,
        "trainedWords": detail.trained_words,
        "triggerWord": detail.trigger_word,
        "description": detail.description,
        "creator": detail.creator,
        "imageUrls": detail.image_urls,
        "civitaiUrl": detail.civitai_url,
        "needsBase": detail.needs_base,
        "downloadCount": detail.download_count,
        "thumbsUp": detail.thumbs_up,
        "stats": {
            "downloadCount": detail.download_count,
            "thumbsUpCount": detail.thumbs_up,
        },
    });
    Ok(Json(json!({ "model": model, "related": related })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCivitaiRequest {
    pub download_url: String,
    pub dest_path: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Civitai base family — stored beside the weight for LoRA→checkpoint selection.
    #[serde(default)]
    pub base_model: Option<String>,
    #[serde(default)]
    pub version_id: Option<i64>,
    #[serde(default)]
    pub model_type: Option<String>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPublic {
    pub open_router_configured: bool,
    pub civitai_configured: bool,
    pub hf_configured: bool,
    pub diffusion_worker_url: Option<String>,
    pub worker_enabled: bool,
    pub chat_model: String,
    pub image_model: String,
    /// Whether `FREYA_API_TOKEN` / rotated runtime token is set (never returns the value).
    pub api_token_configured: bool,
}

pub async fn settings_get(State(state): State<AppState>) -> Json<SettingsPublic> {
    let cfg = state.config.read().await;
    Json(SettingsPublic {
        open_router_configured: cfg.openrouter_configured(),
        civitai_configured: cfg.civitai_configured(),
        hf_configured: !cfg.hf_token.is_empty(),
        diffusion_worker_url: cfg.diffusion_worker_url.clone(),
        worker_enabled: cfg.worker_enabled,
        chat_model: cfg.chat_model.clone(),
        image_model: cfg.image_model.clone(),
        api_token_configured: !cfg.api_token.trim().is_empty(),
    })
}

/// Generate or rotate the studio API bearer. Shown once in the response; also stored in
/// `FREYA_SECRETS_FILE` so local/kind restarts keep it when that path is durable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthTokenResponse {
    pub token: String,
    pub rotated: bool,
    pub persisted: bool,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_commands: Option<Vec<String>>,
}

pub async fn auth_token_create(State(state): State<AppState>) -> ApiResult<Json<AuthTokenResponse>> {
    let token = generate_api_token();
    let had_token = {
        let cfg = state.config.read().await;
        !cfg.api_token.trim().is_empty()
    };
    {
        let mut cfg = state.config.write().await;
        cfg.api_token = token.clone();
    }
    let persisted = state.persist_secrets().await.is_ok();
    let sync_commands = Some(vec![
        format!(
            "kubectl -n freya create secret generic freya-api-token --from-literal=token='{token}' --dry-run=client -o yaml | kubectl apply -f -"
        ),
        "kubectl -n freya rollout restart deploy/freya-api".into(),
    ]);
    Ok(Json(AuthTokenResponse {
        token,
        rotated: had_token,
        persisted,
        note: if persisted {
            "Token applied immediately. Copy it now — it is not shown again. Stored in this browser after you confirm, and on the API secrets file when writable."
                .into()
        } else {
            "Token applied in-memory for this process. Copy it now. Persist with the kubectl commands below (or fix FREYA_SECRETS_FILE) so restarts keep it."
                .into()
        },
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
}

pub async fn settings_put(
    State(state): State<AppState>,
    Json(upd): Json<SettingsUpdate>,
) -> ApiResult<Json<SettingsPublic>> {
    let models_changed = upd.chat_model.is_some() || upd.image_model.is_some();
    {
        let mut cfg = state.config.write().await;
        if let Some(url) = upd.diffusion_worker_url {
            if url.is_empty() {
                cfg.diffusion_worker_url = None;
            } else {
                cfg.diffusion_worker_url = Some(url);
            }
        }
        if let Some(m) = upd.chat_model {
            cfg.chat_model = m;
        }
        if let Some(m) = upd.image_model {
            cfg.image_model = m;
        }
    }

    let secrets = RuntimeSecrets {
        openrouter_api_key: upd.openrouter_api_key,
        civitai_api_key: upd.civitai_api_key,
        hf_token: upd.hf_token,
        diffusion_worker_url: None, // URL already applied above; persist_secrets writes full snapshot
        worker_enabled: None,
        api_token: None,
    };
    state.apply_secrets(secrets).await;
    state.rebuild_worker_client().await;
    let _ = state.persist_secrets().await;

    // OpenRouterClient captures chat/image model at construction — rebuild after changes.
    if models_changed {
        let cfg = state.config.read().await;
        let mut or = state.openrouter.write().await;
        *or = crate::openrouter::OpenRouterClient::new(&cfg);
    }

    Ok(settings_get(State(state)).await)
}

#[derive(Debug)]
pub enum ApiError {
    Msg(String),
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
            ApiError::Msg(m) => (StatusCode::BAD_REQUEST, m),
        };
        (code, Json(json!({ "error": msg }))).into_response()
    }
}
