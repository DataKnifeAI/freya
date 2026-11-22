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
use serde_json::json;

type ApiResult<T> = Result<T, ApiError>;

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    let worker_ok = {
        let w = state.worker.read().await;
        match w.as_ref() {
            Some(c) => c.health().await,
            None => false,
        }
    };
    Json(json!({
        "ok": true,
        "openRouter": cfg.openrouter_configured(),
        "civitai": cfg.civitai_configured(),
        "diffusionWorker": worker_ok,
    }))
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
                    match civ.search(q, 8).await {
                        Ok(items) => {
                            let loras: Vec<_> = items
                                .into_iter()
                                .filter(|c| {
                                    c.model_type.eq_ignore_ascii_case("LORA")
                                        || c.model_type.eq_ignore_ascii_case("LoCon")
                                        || c.model_type.to_lowercase().contains("lora")
                                })
                                .take(6)
                                .collect();
                            out["loras"] = json!(if loras.is_empty() {
                                // fall back to whatever matched the vibe
                                civ.search(q, 6).await.unwrap_or_default()
                            } else {
                                loras
                            });
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
    /// Composer Cloud | Local preference: `openrouter` | `local` (optional).
    #[serde(default)]
    pub backend: Option<String>,
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
    crate::router::infer_modality(message).modality.is_still_image()
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
        || (m.contains("lora") && (m.contains("installed") || m.contains("on the worker") || m.contains("on disk")));
    let asks_list_checkpoints = m.contains("list checkpoint")
        || m.contains("list checkpoints")
        || m.contains("available checkpoint")
        || m.contains("which checkpoints")
        || m.contains("what checkpoints")
        || (m.contains("checkpoint") && (m.contains("list") || m.contains("show") || m.contains("available")));
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
    } else if actions.iter().any(|a| a.action_type == "generate_image" || a.action_type == "run_session") {
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
            | "run_session"
            | "set_base_model"
            | "set_chat_models"
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
    let backend = preferred
        .or(backend_from_gen.as_deref())
        .unwrap_or("cloud");
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
        let err = ar
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("error");
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
            let key = if ty == "list_loras" { "loras" } else { "checkpoints" };
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
                    let name = c
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or(if ty == "list_loras" { "lora" } else { "checkpoint" });
                    let path = c.get("path").and_then(|p| p.as_str()).unwrap_or("");
                    let base = c
                        .get("baseModel")
                        .and_then(|b| b.as_str())
                        .unwrap_or("");
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
            let base = res
                .get("baseModel")
                .and_then(|b| b.as_str())
                .unwrap_or("");
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
            Some(format!("Style ready: {name} (pass styleRef={id} on generate)."))
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
            let image = res.get("imageModel").and_then(|m| m.as_str()).unwrap_or("?");
            let router = res.get("routerModel").and_then(|m| m.as_str()).unwrap_or("?");
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
            let image = res.get("imageModel").and_then(|m| m.as_str()).unwrap_or("?");
            let router = res.get("routerModel").and_then(|m| m.as_str()).unwrap_or("?");
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
        "generate_image" | "generate_video" => {
            let backend = res.get("backend").and_then(|b| b.as_str()).unwrap_or("?");
            let model = res
                .get("model")
                .and_then(|m| m.as_str())
                .map(short_model_id)
                .unwrap_or("?");
            let modality = res
                .get("modality")
                .and_then(|m| m.as_str())
                .unwrap_or(if ty == "generate_video" { "video" } else { "picture" });
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
    if let Some(b) = preferred.map(str::trim).filter(|b| !b.is_empty() && *b != "auto") {
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
            let system = load_instruction_toned(&state, "session").await;
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
            let backend_choice = chat_generate_backend(state, pinned, backend).await;
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
            match client
                .set_active_base(model, base_model.as_deref())
                .await
            {
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
                    json!({
                        "type": ty,
                        "status": "executed",
                        "args": action.args,
                        "result": { "items": filtered, "query": query }
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
                    "result": meta
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
                        "Video Auto follows Privacy/Alignment-aware defaults; composer video pin is per-request.",
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
            if chat.is_none() && image.is_none() && router.is_none() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "provide chatModel and/or imageModel and/or routerModel"
                });
            }
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
        let version_id = req.version_id.ok_or_else(|| {
            ApiError::Msg("downloadUrl or versionId required to install".into())
        })?;
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
    let resolved_video = crate::router::resolve_video_model_choice_prefs(
        req.image_model.as_deref(),
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
        let system = load_instruction_toned(&state, "converse").await;
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
    let planned: Vec<PlannedAction> = if req.confirm {
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

    // Prefer router-resolved image model when executing generates under Auto.
    let image_model_for_actions = Some(resolved_image.as_str());
    let video_model_for_actions = Some(resolved_video.as_str());
    let backend_for_actions = req.backend.as_deref();

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
                action_results.push(
                    execute_chat_action(
                        &state,
                        a,
                        &req.loras,
                        image_model_for_actions,
                        video_model_for_actions,
                        backend_for_actions,
                    )
                    .await,
                );
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
                        execute_chat_action(
                        &state,
                        a,
                        &req.loras,
                        image_model_for_actions,
                        video_model_for_actions,
                        backend_for_actions,
                    )
                    .await,
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
                    .push(
                        execute_chat_action(
                            &state,
                            a,
                            &req.loras,
                            image_model_for_actions,
                            video_model_for_actions,
                            backend_for_actions,
                        )
                        .await,
                    );
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
                    .push(
                        execute_chat_action(
                            &state,
                            a,
                            &req.loras,
                            image_model_for_actions,
                            video_model_for_actions,
                            backend_for_actions,
                        )
                        .await,
                    );
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
    let local_backend = req
        .backend
        .as_deref()
        .is_some_and(|b| b.eq_ignore_ascii_case("local"));
    let base_name = if local_backend {
        local_base_display(&state).await
    } else {
        String::new()
    };
    let lora_line = if local_backend {
        pinned_lora_footer(&req.loras)
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
        req.backend.as_deref(),
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
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoraIn {
    pub name: String,
    #[serde(default = "one")]
    pub scale: f32,
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
                    req.model = Some(m.into());
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
        let loras: Vec<LoraWeight> = req
            .loras
            .iter()
            .map(|l| LoraWeight {
                lora_name: l.name.clone(),
                lora_scale: l.scale,
            })
            .collect();
        let bytes = client
            .generate(
                &final_prompt,
                req.model.as_deref(),
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
            req.model.clone().unwrap_or_else(|| "local".into()),
            "local".to_string(),
        )
    } else {
        let or = state.openrouter.read().await;
        let img = or
            .generate_image(&final_prompt, req.model.as_deref(), req.size.as_deref())
            .await
            .map_err(ApiError::from)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&img.b64)
            .map_err(|e| ApiError::Msg(e.to_string()))?;
        (
            bytes,
            img.mime,
            req.model
                .clone()
                .unwrap_or_else(|| or.default_image_model().to_string()),
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
    let mut local = Vec::new();
    if let Some(w) = state.worker.read().await.as_ref() {
        if let Ok(loras) = w.list_loras().await {
            for a in loras {
                local.push(json!({
                    "id": a.id,
                    "name": a.id,
                    "backend": "local",
                    "kind": a.kind,
                    "path": a.path,
                }));
            }
        }
    }
    Ok(Json(json!({
        "openrouter": cloud.drain(..).map(|m| json!({
            "id": m.id, "name": m.name, "backend": m.backend
        })).collect::<Vec<_>>(),
        "local": local,
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
}

fn default_limit() -> u32 {
    20
}

pub async fn civitai_search(
    State(state): State<AppState>,
    Query(q): Query<CivitaiSearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let civ = state.civitai.read().await;
    let items = civ.search(&q.q, q.limit).await.map_err(ApiError::from)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCivitaiRequest {
    pub download_url: String,
    pub dest_path: String,
    #[serde(default)]
    pub name: Option<String>,
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
    // Pass the key as authToken (Bearer on download) — never append to the URL
    // (reqwest errors include the full URL and would leak the secret).
    let auth = cfg.civitai_api_key.clone();
    let url = req.download_url.clone();
    drop(cfg);
    client
        .install_asset("civitai", &url, &req.dest_path, Some(&auth))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({
        "ok": true,
        "destPath": req.dest_path,
        "name": req.name,
    })))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPublic {
    pub open_router_configured: bool,
    pub civitai_configured: bool,
    pub hf_configured: bool,
    pub diffusion_worker_url: Option<String>,
    pub chat_model: String,
    pub image_model: String,
}

pub async fn settings_get(State(state): State<AppState>) -> Json<SettingsPublic> {
    let cfg = state.config.read().await;
    Json(SettingsPublic {
        open_router_configured: cfg.openrouter_configured(),
        civitai_configured: cfg.civitai_configured(),
        hf_configured: !cfg.hf_token.is_empty(),
        diffusion_worker_url: cfg.diffusion_worker_url.clone(),
        chat_model: cfg.chat_model.clone(),
        image_model: cfg.image_model.clone(),
    })
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
                *state.worker.write().await = None;
            } else {
                cfg.diffusion_worker_url = Some(url.clone());
                *state.worker.write().await = Some(
                    crate::worker::DiffusionWorkerClient::new(url)
                        .with_token(cfg.worker_token.clone()),
                );
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
    };
    state.apply_secrets(secrets).await;
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
