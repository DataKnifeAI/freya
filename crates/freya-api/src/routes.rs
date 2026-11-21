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
