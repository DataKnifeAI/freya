use crate::civitai::CommunityImage;
use crate::gallery::ArtifactMeta;
use crate::state::{AppState, RuntimeSecrets};
use crate::styles::{self, StylePack};
use crate::worker::LoraWeight;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use futures::stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio::sync::mpsc;

type ApiResult<T> = Result<T, ApiError>;

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let (open_router, civitai, worker_url) = {
        let cfg = state.config.read().await;
        (
            cfg.openrouter_configured(),
            cfg.civitai_configured(),
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
        "openRouter": cfg.openrouter_configured(),
        "civitai": cfg.civitai_configured(),
        "diffusionWorker": worker_ok,
        "workerConfigured": worker_url.is_some(),
    }))
}

/// Proxy worker GPU/runtime stats; enrich VRAM from host `nvidia-smi` when worker returns null.
pub async fn worker_stats(State(state): State<AppState>) -> impl IntoResponse {
    let (url, container, unit) = {
        let cfg = state.config.read().await;
        (
            cfg.diffusion_worker_url.clone(),
            cfg.worker_container.clone(),
            cfg.worker_systemd_unit.clone(),
        )
    };
    let runtime = crate::worker_ctl::detect(&container, &unit).await;
    let mut worker_payload = Value::Null;
    let mut reachable = false;
    let mut have_stats = false;
    {
        let w = state.worker.read().await;
        if let Some(c) = w.as_ref() {
            if let Some(detail) = c.stats_detail().await {
                reachable = true;
                have_stats = true;
                worker_payload = detail;
            } else if c.health().await {
                reachable = true;
            }
        }
    }

    let mut vram = worker_payload.get("vram").cloned().unwrap_or(Value::Null);
    let mut host = worker_payload.get("host").cloned().unwrap_or(Value::Null);
    // Host nvidia-smi fills a worker that reported stats with null VRAM
    // (GPU-less container). A failed /v1/stats must not borrow the host GPU.
    if have_stats && vram.is_null() {
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
        "hash": a.hash,
        "backend": "local",
        "installed": true,
    })
}

fn asset_label_key(raw: &str) -> String {
    let file = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let stem = file
        .trim_end_matches(".safetensors")
        .trim_end_matches(".ckpt")
        .trim_end_matches(".pt")
        .trim_end_matches(".gguf")
        .trim_end_matches(".bin");
    stem.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn card_label(v: &Value) -> String {
    v.get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string()
}

fn card_version(v: &Value) -> Option<i64> {
    v.get("versionId")
        .and_then(|x| x.as_i64())
        .filter(|n| *n > 0)
}

fn json_field_missing(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) if s.trim().is_empty() => true,
        Some(Value::Array(a)) if a.is_empty() => true,
        _ => false,
    }
}

/// Name or file-stem equality. Different version ids are not the same asset.
fn lookup_same_asset(local: &Value, search: &Value) -> bool {
    match (card_version(local), card_version(search)) {
        (Some(a), Some(b)) if a == b => return true,
        (Some(_), Some(_)) => return false,
        _ => {}
    }
    let local_path = local.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let search_file = search
        .get("fileName")
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let local_keys = [
        asset_label_key(&card_label(local)),
        asset_label_key(local_path),
    ];
    let search_keys = [
        asset_label_key(&card_label(search)),
        asset_label_key(search_file),
    ];
    local_keys
        .iter()
        .any(|lk| !lk.is_empty() && search_keys.iter().any(|sk| !sk.is_empty() && sk == lk))
}

fn query_hits_label(query: &str, label: &str) -> bool {
    let q = asset_label_key(query);
    let n = asset_label_key(label);
    if q.len() < 3 || n.is_empty() {
        return false;
    }
    if q == n {
        return true;
    }
    (q.len() >= 4 && n.contains(&q)) || (n.len() >= 4 && q.contains(&n))
}

fn merge_installed_lookup(local: &Value, search: Option<&Value>) -> Value {
    let mut out = local.clone();
    let Some(obj) = out.as_object_mut() else {
        return local.clone();
    };
    obj.insert("installed".into(), json!(true));
    let Some(search) = search else {
        return out;
    };
    for key in [
        "modelId",
        "versionId",
        "versionName",
        "thumbnailUrl",
        "imageUrls",
        "baseModel",
        "modelType",
        "triggerWord",
        "downloadUrl",
        "civitaiUrl",
        "fileId",
        "fileName",
        "fileFormat",
        "stats",
    ] {
        if json_field_missing(obj.get(key)) {
            if let Some(v) = search.get(key) {
                if !json_field_missing(Some(v)) {
                    obj.insert(key.to_string(), v.clone());
                }
            }
        }
    }
    // Search `id` is the Civitai model id. Installed `id` is a file stem — do not copy it over.
    if json_field_missing(obj.get("modelId")) {
        if let Some(id) = search.get("id").and_then(|x| x.as_i64()).filter(|n| *n > 0) {
            obj.insert("modelId".into(), json!(id));
        }
    }
    out
}

/// Installed name/version matches first, then Civitai hits that are not that asset.
/// Does not pin or download.
fn order_lookup_assets(installed: &[Value], search: &[Value], query: &str) -> Vec<Value> {
    let mut used_paths = std::collections::HashSet::new();
    let mut consumed = std::collections::HashSet::new();
    let mut existing = Vec::new();

    let push_existing = |existing: &mut Vec<Value>,
                         used_paths: &mut std::collections::HashSet<String>,
                         local: &Value,
                         hit: Option<&Value>| {
        let path = local
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();
        if path.is_empty() || !used_paths.insert(path) {
            return;
        }
        existing.push(merge_installed_lookup(local, hit));
    };

    if asset_label_key(query).len() >= 3 {
        for local in installed {
            let name = card_label(local);
            let stem = local.get("path").and_then(|p| p.as_str()).unwrap_or("");
            if query_hits_label(query, &name) || query_hits_label(query, stem) {
                let hit_idx = search.iter().position(|s| lookup_same_asset(local, s));
                push_existing(
                    &mut existing,
                    &mut used_paths,
                    local,
                    hit_idx.and_then(|i| search.get(i)),
                );
                if let Some(i) = hit_idx {
                    consumed.insert(i);
                }
            }
        }
    }

    for (i, hit) in search.iter().enumerate() {
        if consumed.contains(&i) {
            continue;
        }
        if let Some(local) = installed.iter().find(|l| lookup_same_asset(l, hit)) {
            push_existing(&mut existing, &mut used_paths, local, Some(hit));
            consumed.insert(i);
        }
    }

    let mut ordered = existing;
    for (i, hit) in search.iter().enumerate() {
        if consumed.contains(&i) {
            continue;
        }
        let mut card = hit.clone();
        if let Some(obj) = card.as_object_mut() {
            obj.insert("installed".into(), json!(false));
        }
        ordered.push(card);
    }
    ordered
}

/// Installed row belongs to this agent search type.
/// A set `modelType` must match. Untyped adapter files in `loras/` match LoRA / LoCon / DoRA / LyCORIS.
fn installed_matches_search_type(asset: &Value, canon: &str) -> bool {
    let mt = asset
        .get("modelType")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if !mt.is_empty() {
        return crate::civitai::same_search_type(mt, canon);
    }
    let kind = asset
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if !kind.is_empty() {
        if let Ok(got) = crate::civitai::canonical_search_type(kind) {
            if matches!(canon, "LORA" | "LoCon" | "DoRA" | "LyCORIS")
                && matches!(got, "LORA" | "LoCon" | "DoRA" | "LyCORIS")
            {
                return true;
            }
            return got == canon;
        }
    }
    let path = asset.get("path").and_then(|v| v.as_str()).unwrap_or("");
    match canon {
        "LORA" | "LoCon" | "DoRA" | "LyCORIS" => path.starts_with("loras/"),
        "Checkpoint" => path.starts_with("checkpoints/"),
        "TextualInversion" => path.starts_with("embeddings/"),
        "VAE" => path.starts_with("vae/"),
        _ => false,
    }
}

fn search_type_arg(args: &Value) -> &str {
    for key in ["type", "modelType", "types"] {
        if let Some(t) = args.get(key).and_then(|v| v.as_str()) {
            let t = t.trim();
            if !t.is_empty() {
                return t;
            }
        }
    }
    ""
}

/// Search one Civitai type. Installed name/version matches first, then other hits.
/// Does not pin or download.
async fn typed_civitai_lookup(
    state: &AppState,
    query: &str,
    type_raw: &str,
    limit: u32,
) -> Result<Value, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("missing query".into());
    }
    let canon = crate::civitai::canonical_search_type(type_raw)?;
    let cap = limit.clamp(1, 20) as usize;
    let fetch = (cap as u32).max(12).min(20);
    let allow_nsfw = state.config.read().await.alignment_prefs().allow_nsfw_local;
    let searched = {
        let civ = state.civitai.read().await;
        if !civ.configured() {
            return Err("Civitai API key not configured".into());
        }
        civ.search_types(query, fetch, &[canon], allow_nsfw).await
    };
    let items = searched.map_err(|e| e.to_string())?;
    let mut assets: Vec<_> = items
        .into_iter()
        .filter(|c| crate::civitai::same_search_type(&c.model_type, canon))
        .collect();
    assets.sort_by_key(|c| crate::civitai::asset_type_rank(&c.model_type));
    let search_vals: Vec<Value> = assets
        .iter()
        .map(|c| serde_json::to_value(c).unwrap_or_else(|_| json!({})))
        .collect();
    let installed: Vec<Value> = installed_asset_values(state)
        .await
        .into_iter()
        .filter(|a| installed_matches_search_type(a, canon))
        .collect();
    let mut loras = order_lookup_assets(&installed, &search_vals, query);
    loras.truncate(cap);
    Ok(json!({
        "query": query,
        "type": canon,
        "loras": loras,
    }))
}

async fn installed_asset_values(state: &AppState) -> Vec<Value> {
    let Ok(v) = installed_payload(state).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in ["loras", "checkpoints", "embeddings", "vaes"] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            out.extend(arr.iter().cloned());
        }
    }
    out
}

/// Search results already put installed matches first. A bare list is the library.
fn collect_chat_lookup_cards(action_results: &[Value]) -> Vec<Value> {
    let mut search_cards = Vec::new();
    let mut listed = Vec::new();
    let mut saw_search = false;
    for ar in action_results {
        if ar.get("status").and_then(|s| s.as_str()) != Some("executed") {
            continue;
        }
        let ty = ar.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let Some(res) = ar.get("result") else {
            continue;
        };
        let push_arr = |dest: &mut Vec<Value>, v: Option<&Value>| {
            if let Some(arr) = v.and_then(|x| x.as_array()) {
                dest.extend(arr.iter().cloned());
            }
        };
        if ty == "search_civitai" {
            saw_search = true;
            push_arr(&mut search_cards, res.get("loras"));
            continue;
        }
        match ty {
            "list_loras" => push_arr(&mut listed, res.get("loras")),
            "list_checkpoints" => push_arr(&mut listed, res.get("checkpoints")),
            "list_installed" => {
                for key in ["loras", "checkpoints", "embeddings", "vaes"] {
                    push_arr(&mut listed, res.get(key));
                }
            }
            _ => {}
        }
    }
    if saw_search {
        search_cards
    } else {
        listed
    }
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
                let allow_nsfw = state.config.read().await.alignment_prefs().allow_nsfw_local;
                let civ = state.civitai.read().await;
                if civ.configured() {
                    match civ.search(q, 12, allow_nsfw).await {
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
    /// Composer Cloud | Local preference: `openrouter` | `local` | `cloud` (optional).
    #[serde(default)]
    pub backend: Option<String>,
    /// Composer video model pin (`auto` or a provider/model id). Not an API key.
    #[serde(default)]
    pub video_model: Option<String>,
    /// Compact prior-tool memory (installed, pinned, active base). Not shown in the transcript.
    #[serde(default)]
    pub tool_context: Option<String>,
    /// When true, skip LLM planning and execute `actions` (Plan confirmation).
    #[serde(default)]
    pub confirm: bool,
    /// NDJSON status lines (`Thinking`, `Searching`, `Generating`, `Downloading`) then one result.
    #[serde(default)]
    pub stream: bool,
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

/// Keyword fallback only. The converse prompt requires the model to set `type`.
fn infer_civitai_search_type(message: &str) -> &'static str {
    let m = message.to_ascii_lowercase();
    let words: Vec<&str> = m
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |w: &str| words.iter().any(|x| *x == w);
    if has("embedding") || has("embeddings") || m.contains("textual inversion") {
        "TextualInversion"
    } else if has("lycoris") || has("lyco") {
        "LyCORIS"
    } else if has("locon") {
        "LoCon"
    } else if has("dora") {
        "DoRA"
    } else if has("vae") || has("vaes") {
        "VAE"
    } else if has("checkpoint") || has("checkpoints") {
        "Checkpoint"
    } else {
        "LORA"
    }
}

fn stamp_gen_args(args: &mut Value, message: &str) {
    let patch = crate::genmeta::patch_from_message(message);
    let Some(obj) = args.as_object_mut() else {
        return;
    };
    if let Some(s) = patch.sampler.as_deref() {
        obj.insert("sampler".into(), json!(s));
    }
    if let Some(n) = patch.cfg {
        obj.insert("cfg".into(), json!(n));
    }
    if let Some(n) = patch.steps {
        obj.insert("steps".into(), json!(n));
    }
    if let Some(n) = patch.clip_skip {
        obj.insert("clipSkip".into(), json!(n));
    }
    if let Some(n) = patch.seed {
        obj.insert("seed".into(), json!(n));
    }
    if let Some(n) = patch.quantity {
        obj.insert("quantity".into(), json!(n));
    }
    if crate::genmeta::wants_reuse(message) {
        obj.insert("reuse".into(), json!(true));
    }
    if patch.any() {
        obj.insert("persistMeta".into(), json!(true));
    }
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
        let query = subject_query(message);
        let query = if query.chars().count() >= 2 {
            query
        } else {
            message.chars().take(80).collect()
        };
        let search_type = infer_civitai_search_type(message);
        actions.push(PlannedAction {
            action_type: "search_civitai".into(),
            args: json!({
                "query": query,
                "type": search_type,
            }),
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

    if wants_image_intent(message) || crate::genmeta::wants_reuse(message) {
        if m.contains("run session") || m.contains("variations") || m.contains("batch") {
            let mut args = json!({ "goal": message });
            stamp_gen_args(&mut args, message);
            actions.push(PlannedAction {
                action_type: "run_session".into(),
                args,
            });
        } else {
            let mut args = json!({ "prompt": message, "batch": false });
            stamp_gen_args(&mut args, message);
            actions.push(PlannedAction {
                action_type: "generate_image".into(),
                args,
            });
        }
    } else if crate::genmeta::settings_command_only(message) {
        // "set steps to 30" with no scene — write the shared store, do not render.
        let mut args = json!({});
        stamp_gen_args(&mut args, message);
        actions.push(PlannedAction {
            action_type: "set_generation_meta".into(),
            args,
        });
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
    // Phrase-shaped refusals. Bare "i can't" / "against my" show up in scene text.
    const MARKERS: &[&str] = &[
        "i can't help",
        "i cannot help",
        "i can't assist",
        "i cannot assist",
        "can't help",
        "cannot help",
        "can't assist",
        "cannot assist",
        "can't create",
        "cannot create",
        "can't generate",
        "cannot generate",
        "won't generate",
        "will not generate",
        "i won't create",
        "i will not create",
        "i'm not able",
        "i am not able",
        "not able to generate",
        "not able to create",
        "against my guidelines",
        "against my policy",
        "against my programming",
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

fn sd_prompt_shape(text: &str) -> bool {
    text.contains(',') || text.split_whitespace().count() >= 12
}

/// Adult local rewrite must be a scene prompt, not a refusal.
/// An echo of the user line counts only when that line is already checkpoint-shaped.
/// A short raw scene is not accepted in place of a rewrite.
fn adult_rewrite_ok(text: &str, original: &str) -> bool {
    let t = text.trim();
    let original = original.trim();
    if t.is_empty() || looks_like_model_refusal(t) {
        return false;
    }
    if t.eq_ignore_ascii_case(original) {
        return sd_prompt_shape(original);
    }
    sd_prompt_shape(t)
}

/// Second uncensored writer when the first cell refuses or errors.
/// Auto's first writer is already the matrix cell, so the other cost tier is the retry.
fn adult_retry_model(
    first: &str,
    privacy: &crate::prefs::PrivacyPrefs,
    fallbacks: &crate::prefs::AutoFallbacksPrefs,
) -> Option<String> {
    let matrix = crate::router::local_adult_matrix_model(privacy, fallbacks);
    if matrix != first {
        return Some(matrix);
    }
    let mut other = fallbacks.clone();
    other.cost_tier = if fallbacks.cost_tier.eq_ignore_ascii_case("low") {
        "medium".into()
    } else {
        "low".into()
    };
    let alt = crate::router::local_adult_matrix_model(privacy, &other);
    if alt == first {
        None
    } else {
        Some(alt)
    }
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
            | "set_generation_meta"
            | "recommend_generation_meta"
            | "apply_gallery_artifact"
            | "apply_community_generation"
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
        // Lookup failures stay in the tool disclosure. Do not paste tool names into the reply.
        if matches!(ty, "generate_image" | "generate_video" | "run_session") {
            return Some("I couldn't finish that generation.".into());
        }
        return None;
    }
    if status == "proposed" {
        return None;
    }
    let res = ar.get("result")?;
    match ty {
        "worker_status" => {
            let configured = res.get("workerConfigured").and_then(|v| v.as_bool());
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
        "set_generation_meta" | "recommend_generation_meta" => {
            let label = res
                .get("settings")
                .and_then(|s| s.get("sourceLabel"))
                .and_then(|s| s.as_str())
                .unwrap_or("Generation settings saved.");
            Some(label.to_string())
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
    if !pinned.is_empty() && local_worker_ready(state).await {
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
            let raw = action
                .args
                .get("query")
                .and_then(|q| q.as_str())
                .unwrap_or("")
                .trim();
            let short = subject_query(raw);
            let q = if short.chars().count() >= 2 {
                short
            } else {
                raw.to_string()
            };
            let mut args = action.args.clone();
            if let Some(obj) = args.as_object_mut() {
                obj.insert("query".to_string(), json!(q));
            }
            let type_raw = search_type_arg(&args);
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(6) as u32;
            match typed_civitai_lookup(state, &q, type_raw, limit).await {
                Ok(result) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": args,
                    "result": result
                }),
                Err(e) => json!({
                    "type": ty,
                    "status": "error",
                    "args": args,
                    "error": e
                }),
            }
        }
        "civitai_images" => {
            let model_id = json_i64(action.args.get("modelId"));
            let version_id = json_i64(action.args.get("modelVersionId"));
            if model_id.filter(|n| *n > 0).is_none() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "modelId is required"
                });
            }
            let limit = action
                .args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as u32;
            let cursor = action.args.get("cursor").and_then(|v| v.as_str());
            let page_no = action
                .args
                .get("page")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let allow_nsfw = state.config.read().await.alignment_prefs().allow_nsfw_local;
            match load_community_images(
                state, model_id, version_id, allow_nsfw, limit, cursor, page_no,
            )
            .await
            {
                Ok(page) => json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": {
                        "modelId": model_id,
                        "modelVersionId": version_id,
                        "nsfw": allow_nsfw,
                        "count": page.items.len(),
                        "items": page.items,
                        "nextCursor": page.next_cursor,
                        "nextPage": page.next_page,
                        "note": "Community image posts. Pass nextCursor or nextPage for another page. Does not download or generate."
                    }
                }),
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
            let req = image_generate_request(
                prompt.to_string(),
                &action.args,
                backend_choice.clone(),
                resolve_action_image_model(image_model, &backend_choice),
                style_ref_from_args(&action.args),
                pinned.to_vec(),
            );
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
            let mut req = image_generate_request(
                prompt.to_string(),
                &action.args,
                backend_choice,
                video_model.map(str::to_string),
                style_ref_from_args(&action.args),
                vec![],
            );
            req.modality = Some(modality);
            req.duration = action.args.get("duration").and_then(|d| d.as_i64());
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
                let req = image_generate_request(
                    p.clone(),
                    &action.args,
                    backend_choice.clone(),
                    resolve_action_image_model(image_model, &backend_choice),
                    session_style.clone(),
                    pinned.to_vec(),
                );
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
            let failed = results
                .iter()
                .filter_map(|r| r.get("error").and_then(|e| e.as_str()))
                .collect::<Vec<_>>();
            if !results.is_empty() && failed.len() == results.len() {
                json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": failed.first().copied().unwrap_or("series generation failed"),
                    "result": { "goal": goal, "batchPrompts": prompts, "results": results }
                })
            } else {
                json!({
                    "type": ty,
                    "status": "executed",
                    "args": action.args,
                    "result": { "goal": goal, "batchPrompts": prompts, "results": results }
                })
            }
        }
        "install_asset" => {
            let download_url = action
                .args
                .get("downloadUrl")
                .and_then(|u| u.as_str())
                .unwrap_or("");
            let version_id = action.args.get("versionId").and_then(|x| x.as_i64());
            let model_id = action
                .args
                .get("modelId")
                .and_then(|x| x.as_i64())
                .or_else(|| action.args.get("id").and_then(|x| x.as_i64()));
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
            if download_url.is_empty() && version_id.is_none() && model_id.is_none() {
                return json!({
                    "type": ty,
                    "status": "error",
                    "args": action.args,
                    "error": "missing downloadUrl, versionId, or modelId"
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
                model_id,
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
            let worker_url = state.config.read().await.diffusion_worker_url.clone();
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
                    let checkpoints: Vec<Value> = items.into_iter().map(local_asset_json).collect();
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
                    let loras: Vec<Value> = items.into_iter().map(local_asset_json).collect();
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
            json!({
                "type": ty,
                "status": "executed",
                "args": action.args,
                "result": route_defaults_payload(state).await
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
                if let Err(e) = crate::studio::pin_upsert(&mut cfg.studio, &card) {
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
        "get_generation_meta" => {
            action_ok(ty, &action.args, generation_meta_value(state, false).await)
        }
        "recommend_generation_meta" => {
            action_ok(ty, &action.args, generation_meta_value(state, true).await)
        }
        "set_generation_meta" => {
            if let Some(msg) = reject_secret_fields(&action.args) {
                return action_error(ty, &action.args, msg);
            }
            let patch = crate::genmeta::patch_from_value(&action.args);
            let pin_weights = action
                .args
                .get("loraWeights")
                .or_else(|| action.args.get("lora_weights"))
                .and_then(|v| v.as_array());
            if !patch.any() && pin_weights.is_none() {
                return action_error(
                    ty,
                    &action.args,
                    "provide sampler, cfg, steps, clipSkip, negativePrompt, seed, size, quantity, enhance, or loraWeights",
                );
            }
            let mut settings = state.config.read().await.studio.generation.clone();
            if patch.any() {
                settings.apply_patch(&patch, "agent", true);
                write_generation(state, &settings).await;
            }
            if let Some(weights) = pin_weights {
                let mut cfg = state.config.write().await;
                for w in weights {
                    let name = w.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
                    let path = w.get("path").and_then(|p| p.as_str()).unwrap_or("").trim();
                    let scale = w.get("scale").and_then(|s| s.as_f64());
                    if name.is_empty() && path.is_empty() {
                        continue;
                    }
                    let mut card = json!({ "name": name, "path": path });
                    if let Some(s) = scale {
                        card["scale"] = json!(s);
                    }
                    let _ = crate::studio::pin_upsert(&mut cfg.studio, &card);
                }
                let _ = state.persist_secrets().await;
            }
            let pins = state.config.read().await.studio.pinned_loras.clone();
            action_ok(
                ty,
                &action.args,
                json!({ "settings": settings, "pinnedLoras": pins, "persisted": true }),
            )
        }
        "apply_gallery_artifact" => {
            let id = action
                .args
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let Some(id) = id else {
                return action_error(ty, &action.args, "missing id");
            };
            let meta = match state.gallery.get(id).await {
                Ok(Some(m)) => m,
                Ok(None) => return action_error(ty, &action.args, "gallery artifact not found"),
                Err(e) => return action_error(ty, &action.args, e.to_string()),
            };
            let inventory = studio_inventory(state).await;
            let mut settings = state.config.read().await.studio.generation.clone();
            let mut studio = state.config.read().await.studio.clone();
            let out = crate::reproduce::apply_gallery_meta(&meta, &inventory, &mut settings, &mut studio);
            {
                let mut cfg = state.config.write().await;
                cfg.studio.generation = settings.clone();
                cfg.studio.pinned_loras = studio.pinned_loras.clone();
            }
            let _ = state.persist_secrets().await;
            if let Some(ck) = meta.checkpoint.as_ref().and_then(|c| c.path.as_deref()) {
                if inventory
                    .installed
                    .iter()
                    .any(|a| a.path == ck)
                {
                    if let Some(w) = state.worker.read().await.as_ref() {
                        let family = meta
                            .base_model
                            .as_deref()
                            .or_else(|| meta.checkpoint.as_ref().map(|c| c.name.as_str()));
                        let _ = w.set_active_base(ck, family).await;
                    }
                }
            }
            write_generation(state, &settings).await;
            action_ok(ty, &action.args, serde_json::to_value(&out).unwrap_or(json!({})))
        }
        "apply_community_generation" => {
            let image: CommunityImage = match serde_json::from_value(action.args.clone()) {
                Ok(i) => i,
                Err(e) => {
                    return action_error(ty, &action.args, format!("invalid community image: {e}"));
                }
            };
            let inventory = studio_inventory(state).await;
            let mut settings = state.config.read().await.studio.generation.clone();
            let mut studio = state.config.read().await.studio.clone();
            let out =
                crate::reproduce::apply_community_image(&image, &inventory, &mut settings, &mut studio);
            {
                let mut cfg = state.config.write().await;
                cfg.studio.generation = settings.clone();
                cfg.studio.pinned_loras = studio.pinned_loras.clone();
            }
            let _ = state.persist_secrets().await;
            if let Some(base) = crate::reproduce::installed_checkpoint_for_image(&image, &inventory) {
                if let Some(w) = state.worker.read().await.as_ref() {
                    let _ = w
                        .set_active_base(&base.path, image.checkpoint.as_deref())
                        .await;
                }
            }
            write_generation(state, &settings).await;
            action_ok(ty, &action.args, serde_json::to_value(&out).unwrap_or(json!({})))
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

    // Prefer client URL; otherwise resolve from versionId, or modelId's latest version.
    if req.download_url.trim().is_empty() && req.version_id.filter(|n| *n > 0).is_none() {
        let model_id = req.model_id.filter(|n| *n > 0).ok_or_else(|| {
            ApiError::Msg("downloadUrl, versionId, or modelId required to install".into())
        })?;
        let civ = state.civitai.read().await;
        let resolved = civ
            .resolve_model_download(model_id)
            .await
            .map_err(ApiError::from)?;
        req.version_id = Some(resolved.version_id);
        req.download_url = resolved.download_url;
        if req.file_id.is_none() {
            req.file_id = resolved.file_id;
        }
        if req.base_model.is_none() {
            req.base_model = resolved.base_model;
        }
        if req.thumbnail_url.is_none() {
            req.thumbnail_url = resolved.thumbnail_url;
        }
    }
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

#[derive(Clone, Default)]
struct Phase {
    tx: Option<mpsc::Sender<String>>,
}

impl Phase {
    fn emit(&self, status: &str) {
        let Some(tx) = &self.tx else {
            return;
        };
        let line = json!({ "type": "status", "status": status }).to_string();
        let _ = tx.try_send(line);
    }
}

fn phase_for_action(ty: &str) -> Option<&'static str> {
    match ty {
        "generate_image" | "generate_video" | "run_session" => Some("Generating"),
        "install_asset" => Some("Downloading"),
        "search_civitai"
        | "civitai_images"
        | "suggest_ideas"
        | "list_loras"
        | "list_checkpoints"
        | "list_installed"
        | "list_styles"
        | "list_models"
        | "list_favorites"
        | "list_pins"
        | "list_install_jobs"
        | "gallery_list"
        | "gallery_get"
        | "get_settings"
        | "get_route_defaults"
        | "worker_status"
        | "get_base_model"
        | "apply_style"
        | "get_generation_meta"
        | "recommend_generation_meta" => Some("Searching"),
        _ => None,
    }
}

struct ActionRun {
    loras: Vec<LoraIn>,
    image_model: Option<String>,
    video_model: Option<String>,
    backend: Option<String>,
    phase: Phase,
}

impl ActionRun {
    async fn exec(&mut self, state: &AppState, action: &PlannedAction) -> Value {
        if let Some(label) = phase_for_action(&action.action_type) {
            self.phase.emit(label);
        }
        let v = execute_chat_action(
            state,
            action,
            &self.loras,
            self.image_model.as_deref(),
            self.video_model.as_deref(),
            self.backend.as_deref(),
        )
        .await;
        self.absorb(action, &v);
        v
    }

    fn absorb(&mut self, action: &PlannedAction, v: &Value) {
        if v.get("status").and_then(|s| s.as_str()) != Some("executed") {
            return;
        }
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let Some(res) = v.get("result") else {
            return;
        };
        match ty {
            "set_chat_models" => {
                // The result is the full settings snapshot. Copy a model only
                // when this action pinned it, so a chat pin does not replace
                // the router-resolved image id with stored Auto.
                if action_arg_present(&action.args, "imageModel") {
                    if let Some(m) = res.get("imageModel").and_then(|m| m.as_str()) {
                        if !m.is_empty() {
                            self.image_model = Some(m.to_string());
                        }
                    }
                }
                if action_arg_present(&action.args, "videoModel") {
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

fn action_arg_present(args: &Value, key: &str) -> bool {
    args.get(key)
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
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
        scale: v.get("scale").and_then(|s| s.as_f64()).unwrap_or(1.0) as f32,
        path: v.get("path").and_then(|p| p.as_str()).map(str::to_string),
        base_model: v
            .get("baseModel")
            .and_then(|b| b.as_str())
            .map(str::to_string),
        version_id: v.get("versionId").and_then(|x| json_i64(Some(x))),
        model_id: v
            .get("modelId")
            .and_then(|x| json_i64(Some(x)))
            .or_else(|| v.get("id").and_then(|x| json_i64(Some(x)))),
    })
}

const QUERY_INSTRUCTION: &[&str] = &[
    "a",
    "an",
    "the",
    "please",
    "me",
    "my",
    "of",
    "to",
    "and",
    "or",
    "with",
    "for",
    "on",
    "in",
    "at",
    "it",
    "this",
    "that",
    "image",
    "images",
    "picture",
    "pictures",
    "photo",
    "photos",
    "pic",
    "are",
    "you",
    "your",
    "able",
    "can",
    "could",
    "would",
    "should",
    "make",
    "makes",
    "making",
    "create",
    "creating",
    "generate",
    "generating",
    "draw",
    "drawing",
    "paint",
    "render",
    "want",
    "need",
    "help",
    "do",
    "does",
    "did",
    "is",
    "am",
    "be",
    "was",
    "were",
    "i",
    "im",
    "we",
    "our",
    "us",
    "just",
    "like",
    "some",
    "any",
    "what",
    "how",
    "which",
    "who",
    "when",
    "where",
    "why",
    "will",
    "let",
    "show",
    "give",
    "get",
    "got",
    "using",
    "use",
    "used",
    "via",
    "from",
    "into",
    "about",
    "character",
    "characters",
    "style",
    "reference",
    "lora",
    "loras",
    "checkpoint",
    "checkpoints",
    "model",
    "models",
    "asset",
    "assets",
    "base",
    "please",
    "look",
    "looking",
    "see",
    "standing",
    "sitting",
    "walking",
    "holding",
    "wearing",
    "wear",
    "wears",
    "worn",
    "dressed",
    "an",
    "by",
    "as",
    "if",
    "but",
    "so",
    "not",
    "no",
    "yes",
    "now",
    "then",
    "here",
    "there",
    "also",
    "really",
    "very",
    "prompt",
    "art",
    "artwork",
    "illustration",
];

/// Clothing, lighting, and scene filler. Dropped unless they are the only name.
const QUERY_SCENE: &[&str] = &[
    "dress",
    "dresses",
    "shirt",
    "skirt",
    "pants",
    "jeans",
    "jacket",
    "coat",
    "hat",
    "shoes",
    "boots",
    "outfit",
    "outfits",
    "clothing",
    "clothes",
    "hoodie",
    "gown",
    "armor",
    "armour",
    "cape",
    "cloak",
    "lingerie",
    "bikini",
    "stockings",
    "gloves",
    "uniform",
    "costume",
    "heels",
    "socks",
    "shorts",
    "sweater",
    "blouse",
    "suit",
    "scarf",
    "helmet",
    "crown",
    "necklace",
    "lighting",
    "light",
    "lights",
    "neon",
    "cinematic",
    "shadow",
    "shadows",
    "moonlight",
    "sunlight",
    "lamp",
    "lamps",
    "glow",
    "glowing",
    "dark",
    "night",
    "bright",
    "dim",
    "bokeh",
    "sunset",
    "sunrise",
    "dawn",
    "dusk",
    "spotlight",
    "emerald",
    "crimson",
    "scarlet",
    "pink",
    "red",
    "blue",
    "black",
    "white",
    "gold",
    "golden",
    "silver",
    "purple",
    "violet",
    "green",
    "yellow",
    "orange",
    "teal",
    "cyan",
    "ivory",
    "nude",
    "background",
    "foreground",
];

fn query_key(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn query_display(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '\'')
}

fn query_listed(key: &str, list: &[&str]) -> bool {
    list.iter().any(|w| *w == key)
}

struct QueryTok {
    display: String,
    key: String,
    cap: bool,
}

fn query_tokens(message: &str) -> Vec<QueryTok> {
    message
        .split_whitespace()
        .filter_map(|raw| {
            let key = query_key(raw);
            if key.is_empty() {
                return None;
            }
            let cap = raw
                .chars()
                .find(|c| c.is_alphabetic())
                .is_some_and(|c| c.is_uppercase());
            let display = query_display(raw);
            if display.is_empty() {
                return None;
            }
            Some(QueryTok {
                display: display.to_string(),
                key,
                cap,
            })
        })
        .collect()
}

fn finish_query(words: &[String]) -> String {
    words.join(" ").chars().take(80).collect()
}

/// Civitai query is the subject (character or asset name, 2–6 words), not the prompt.
/// Proper nouns win. Question words, scene prose, clothing, and lighting do not.
fn subject_query(message: &str) -> String {
    let raw = message.trim();
    if raw.is_empty() {
        return String::new();
    }
    let tokens = query_tokens(raw);
    let mut spans: Vec<(Vec<String>, bool)> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut scene_only = true;
    let flush =
        |spans: &mut Vec<(Vec<String>, bool)>, cur: &mut Vec<String>, scene_only: &mut bool| {
            if !cur.is_empty() {
                spans.push((std::mem::take(cur), *scene_only));
                *scene_only = true;
            }
        };
    for tok in &tokens {
        let instruction = query_listed(&tok.key, QUERY_INSTRUCTION);
        let scene = query_listed(&tok.key, QUERY_SCENE);
        if tok.cap && !instruction && cur.len() < 6 {
            if !scene {
                scene_only = false;
            }
            cur.push(tok.display.clone());
        } else if !cur.is_empty() && !tok.cap && !instruction && !scene && cur.len() < 6 {
            scene_only = false;
            cur.push(tok.display.clone());
        } else {
            flush(&mut spans, &mut cur, &mut scene_only);
        }
    }
    flush(&mut spans, &mut cur, &mut scene_only);
    let named: Vec<&(Vec<String>, bool)> = spans.iter().filter(|(_, scene)| !scene).collect();
    let pool: Vec<&(Vec<String>, bool)> = if named.is_empty() {
        spans.iter().collect()
    } else {
        named
    };
    if let Some((words, _)) = pool.iter().max_by_key(|(words, _)| words.len()) {
        if !words.is_empty() {
            return finish_query(words);
        }
    }
    let strict: Vec<String> = tokens
        .iter()
        .filter(|t| !query_listed(&t.key, QUERY_INSTRUCTION) && !query_listed(&t.key, QUERY_SCENE))
        .take(6)
        .map(|t| t.display.clone())
        .collect();
    if !strict.is_empty() {
        return finish_query(&strict);
    }
    let loose: Vec<String> = tokens
        .iter()
        .filter(|t| !query_listed(&t.key, QUERY_INSTRUCTION))
        .take(4)
        .map(|t| t.display.clone())
        .collect();
    finish_query(&loose)
}

fn sentence_is_tool_noise(sentence: &str) -> bool {
    let lower = sentence.to_ascii_lowercase();
    lower.contains("freya route")
        || lower.contains("search_civitai")
        || lower.contains("civitai_images")
        || lower.contains("generate_image")
        || lower.contains("generate_video")
        || lower.contains("run_session")
        || lower.contains("install_asset")
}

fn tool_noise_line(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    if lower.starts_with("proposed:") {
        return true;
    }
    if let Some((head, _)) = lower.split_once(" failed:") {
        return head.contains('_') && head.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    }
    false
}

fn scrub_sentences(line: &str) -> String {
    let mut out = String::new();
    let mut start = 0usize;
    for (i, ch) in line.char_indices() {
        if ch == '.' || ch == '!' || ch == '?' {
            let sentence = line[start..=i].trim();
            if !sentence.is_empty() && !sentence_is_tool_noise(sentence) {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(sentence);
            }
            start = i + ch.len_utf8();
        }
    }
    let tail = line[start..].trim();
    if !tail.is_empty() && !sentence_is_tool_noise(tail) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(tail);
    }
    out
}

/// Drop tool-call lines and route-machinery sentences from the visible reply.
fn scrub_tool_noise(reply: &str) -> String {
    reply
        .split('\n')
        .filter(|line| !tool_noise_line(line))
        .map(scrub_sentences)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn suggestion_names(cards: &[Value], limit: usize) -> Vec<String> {
    let mut names = Vec::new();
    let push = |names: &mut Vec<String>, card: &Value| {
        if names.len() >= limit {
            return;
        }
        let Some(name) = card.get("name").and_then(|n| n.as_str()).map(str::trim) else {
            return;
        };
        if name.is_empty() || names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            return;
        }
        names.push(name.to_string());
    };
    for card in cards.iter().filter(|c| {
        c.get("installed").and_then(|v| v.as_bool()) == Some(true)
            || c.get("path")
                .and_then(|p| p.as_str())
                .is_some_and(|p| !p.is_empty())
    }) {
        push(&mut names, card);
    }
    for card in cards {
        push(&mut names, card);
    }
    names
}

/// Vague first turn: name a couple of matches and explain pin. A clear render only notes the Agent tab.
fn note_subject_lookup(reply: &str, modality: &str, cards: &[Value]) -> String {
    let names = suggestion_names(cards, 2);
    let reply = reply.trim();
    if modality == "chat" {
        if reply.to_lowercase().contains("pin")
            && reply.to_lowercase().contains("base")
            && reply.to_lowercase().contains("lora")
        {
            return reply.to_string();
        }
        let lead = if names.is_empty() {
            "I looked for a matching LoRA and base.".to_string()
        } else if names.len() == 1 {
            format!("A match: {}.", names[0])
        } else {
            format!("A couple of matches: {} and {}.", names[0], names[1])
        };
        let pin = " Pin one on the Agent tab to use it. That installs the LoRA if it is not on the worker, then pins it, and installs or selects the matching base checkpoint.";
        if reply.is_empty() {
            format!("{lead}{pin}")
        } else {
            format!("{reply}\n\n{lead}{pin}")
        }
    } else if names.is_empty() || reply.to_lowercase().contains("agent tab") {
        reply.to_string()
    } else {
        format!(
            "{reply}\n\nMatching assets are on the Agent tab ({}) if you want to pin a LoRA or switch the base.",
            names.join(", ")
        )
    }
}

/// First prompt: installed-first Civitai lookup, one call per agent search type. Does not pin or install.
async fn run_initial_subject_lookup(state: &AppState, message: &str) -> Vec<Value> {
    let query = subject_query(message);
    if query.chars().count() < 2 {
        return Vec::new();
    }
    let configured = state.civitai.read().await.configured();
    if !configured {
        return vec![json!({
            "type": "search_civitai",
            "status": "error",
            "args": { "query": query, "type": "LORA" },
            "error": "Civitai API key not configured"
        })];
    }
    let mut joins = Vec::new();
    for ty in crate::civitai::AGENT_SEARCH_TYPES {
        let state = state.clone();
        let query = query.clone();
        let limit: u32 = if *ty == "LORA" || *ty == "Checkpoint" {
            4
        } else {
            2
        };
        joins.push(tokio::spawn(async move {
            let args = json!({ "query": query, "type": ty, "limit": limit });
            match typed_civitai_lookup(&state, &query, ty, limit).await {
                Ok(result) => json!({
                    "type": "search_civitai",
                    "status": "executed",
                    "args": args,
                    "result": result
                }),
                Err(e) => json!({
                    "type": "search_civitai",
                    "status": "error",
                    "args": args,
                    "error": e
                }),
            }
        }));
    }
    let mut out = Vec::new();
    for join in joins {
        match join.await {
            Ok(v) => out.push(v),
            Err(e) => out.push(json!({
                "type": "search_civitai",
                "status": "error",
                "error": e.to_string()
            })),
        }
    }
    out
}

pub async fn chat(State(state): State<AppState>, Json(req): Json<ChatRequest>) -> Response {
    if !req.stream {
        return match chat_run(state, req, Phase::default()).await {
            Ok(v) => Json(v).into_response(),
            Err(e) => e.into_response(),
        };
    }
    let (tx, rx) = mpsc::channel(32);
    let phase = Phase {
        tx: Some(tx.clone()),
    };
    phase.emit("Thinking");
    tokio::spawn(async move {
        let line = match chat_run(state, req, phase).await {
            Ok(v) => json!({ "type": "result", "data": v }).to_string(),
            Err(e) => json!({ "type": "error", "error": api_err_msg(e) }).to_string(),
        };
        let _ = tx.send(line).await;
    });
    let body = Body::from_stream(stream::unfold(rx, |mut rx| async move {
        let line = rx.recv().await?;
        Some((Ok::<String, Infallible>(format!("{line}\n")), rx))
    }));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn chat_run(
    state: AppState,
    mut req: ChatRequest,
    phase: Phase,
) -> ApiResult<serde_json::Value> {
    phase.emit("Thinking");
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

    let mut hist_blob = if req.messages.is_empty() {
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
    if let Some(ctx) = req
        .tool_context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let ctx: String = ctx.chars().take(2000).collect();
        hist_blob = if hist_blob.is_empty() {
            format!("--- prior tools ---\n{ctx}")
        } else {
            format!("{hist_blob}\n\n--- prior tools ---\n{ctx}")
        };
    }
    let initial_turn = !req.confirm && req.messages.is_empty();

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
        let mut args = json!({
            "prompt": req.message,
            "modality": modality,
        });
        stamp_gen_args(&mut args, &req.message);
        (
            reply,
            vec![PlannedAction {
                action_type: "generate_video".into(),
                args,
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
            let mut args = json!({ "goal": req.message });
            stamp_gen_args(&mut args, &req.message);
            (
                reply,
                vec![PlannedAction {
                    action_type: "run_session".into(),
                    args,
                }],
            )
        } else {
            let reply = if mode == "plan" {
                "Ready to generate — press Generate to run it.".to_string()
            } else {
                "On it — I’ll make that image for you.".to_string()
            };
            let mut args = json!({ "prompt": req.message, "batch": false });
            stamp_gen_args(&mut args, &req.message);
            (
                reply,
                vec![PlannedAction {
                    action_type: "generate_image".into(),
                    args,
                }],
            )
        }
    } else if !req.confirm && crate::genmeta::wants_reuse(&req.message) {
        let reply = if mode == "plan" {
            "Ready to generate again with the same settings — press Generate to run it.".to_string()
        } else {
            "Using the same settings as the last image.".to_string()
        };
        let mut args = json!({ "prompt": req.message, "batch": false });
        stamp_gen_args(&mut args, &req.message);
        (
            reply,
            vec![PlannedAction {
                action_type: "generate_image".into(),
                args,
            }],
        )
    } else if !req.confirm && crate::genmeta::settings_command_only(&req.message) {
        let mut args = json!({});
        stamp_gen_args(&mut args, &req.message);
        (
            "Updated the generation settings.".to_string(),
            vec![PlannedAction {
                action_type: "set_generation_meta".into(),
                args,
            }],
        )
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
    // That includes an explicit Local pin, Auto GIF, and Auto with pinned LoRAs
    // (those still run on the worker). A cloud still does not.
    let worker_ready = local_worker_ready(&state).await;
    let backend_set = req
        .backend
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let pinned_local = backend_set.is_some_and(|b| b.eq_ignore_ascii_case("local"));
    let auto_backend = backend_set
        .map(|b| b.eq_ignore_ascii_case("auto"))
        .unwrap_or(true);
    let lora_local = auto_backend && worker_ready && !req.loras.is_empty();
    let auto_gif = routes_to_local_worker(
        req.backend.as_deref(),
        crate::router::Modality::Gif,
        worker_ready,
    ) && crate::router::direct_media_ask(&req.message)
        == crate::router::DirectMedia::Gif;
    if alignment.local_adult_passthrough() && (pinned_local || auto_gif || lora_local) {
        for action in &mut planned {
            restore_refused_local_prompt(action, &req.message);
        }
    }

    // First prompt looks up the subject (installed first, then each Civitai type).
    // The fan-out replaces any single search the planner already chose so types are not dropped.
    let lookup_results = if initial_turn {
        planned.retain(|a| a.action_type != "search_civitai");
        phase.emit("Searching");
        run_initial_subject_lookup(&state, &req.message).await
    } else {
        Vec::new()
    };

    // Prefer router-resolved image model when executing generates under Auto.
    let mut env = ActionRun {
        loras: req.loras.clone(),
        image_model: Some(resolved_image.clone()),
        video_model: Some(resolved_video.clone()),
        backend: req.backend.clone(),
        phase: phase.clone(),
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

    if !lookup_results.is_empty() {
        let mut merged = lookup_results;
        merged.append(&mut action_results);
        action_results = merged;
    }

    // Aggregate convenience fields for the UI
    let mut ideas = Vec::new();
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
    // Search payload is already installed-first. Do not prepend the full library.
    let loras = collect_chat_lookup_cards(&action_results);

    // Tool results arrive after the LLM reply — fold worker summaries into reply.
    let reply = enrich_reply_with_worker_results(&reply, &action_results);
    let reply = scrub_tool_noise(&reply);
    let reply = if initial_turn {
        note_subject_lookup(&reply, route.modality.as_str(), &loras)
    } else {
        reply
    };
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

    Ok(json!({
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
    }))
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
    #[serde(default)]
    pub steps: Option<u32>,
    #[serde(default, alias = "guidance", alias = "guidanceScale")]
    pub cfg: Option<f64>,
    #[serde(default)]
    pub sampler: Option<String>,
    #[serde(default)]
    pub clip_skip: Option<u32>,
    #[serde(default, alias = "n")]
    pub quantity: Option<u32>,
    /// Repeat the last gallery sidecar (sampler, cfg, steps, clip skip, seed, …).
    #[serde(default)]
    pub reuse_meta: Option<bool>,
    /// Write request sampling fields into the shared generation store.
    #[serde(default)]
    pub persist_meta: Option<bool>,
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
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    pub version_id: Option<i64>,
    /// Civitai model id when known; non-numeric strings in JSON are dropped (legacy clients sent filenames).
    #[serde(default, deserialize_with = "deserialize_optional_i64")]
    pub model_id: Option<i64>,
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
    if v.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("series generation failed");
        return Err(ApiError::Msg(msg.to_string()));
    }
    Ok(Json(v))
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

fn image_generate_request(
    prompt: String,
    args: &Value,
    backend: String,
    model: Option<String>,
    style_ref: Option<String>,
    loras: Vec<LoraIn>,
) -> GenerateRequest {
    let from_args = crate::genmeta::patch_from_value(args);
    let from_text = crate::genmeta::patch_from_message(&prompt);
    let pick_s = |a: &Option<String>, b: &Option<String>| a.clone().or_else(|| b.clone());
    let reuse = args.get("reuse").and_then(|v| v.as_bool()).unwrap_or(false)
        || args
            .get("reuseMeta")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        || crate::genmeta::wants_reuse(&prompt);
    let persist = args
        .get("persistMeta")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || from_args.any()
        || from_text.any();
    GenerateRequest {
        prompt,
        negative_prompt: pick_s(&from_args.negative_prompt, &from_text.negative_prompt),
        enhance: pick_s(&from_args.enhance, &from_text.enhance),
        backend: Some(backend),
        model,
        style_ref,
        loras,
        size: pick_s(&from_args.size, &from_text.size).and_then(|s| crate::genmeta::clean_size(&s)),
        seed: if from_args.clear_seed {
            None
        } else {
            from_args.seed.or(from_text.seed)
        },
        modality: Some("picture".into()),
        duration: None,
        steps: from_args.steps.or(from_text.steps),
        cfg: from_args.cfg.or(from_text.cfg),
        sampler: pick_s(&from_args.sampler, &from_text.sampler),
        clip_skip: from_args.clip_skip.or(from_text.clip_skip),
        quantity: from_args.quantity.or(from_text.quantity),
        reuse_meta: Some(reuse),
        persist_meta: Some(persist),
    }
}

struct PreparedGen {
    sampler: String,
    cfg: f64,
    steps: u32,
    clip_skip: u32,
    negative: Option<String>,
    seed: i64,
    size: Option<String>,
    quantity: u32,
    source: String,
    source_label: String,
}

struct SelAsset {
    kind: &'static str,
    name: String,
    version_id: Option<i64>,
    base: Option<String>,
}

fn pin_version(pin: &Value) -> Option<i64> {
    pin.get("versionId")
        .and_then(|v| v.as_i64())
        .or_else(|| pin.get("version_id").and_then(|v| v.as_i64()))
}

fn sel_from_pin(pin: &Value) -> Option<SelAsset> {
    if !crate::studio::is_pinnable(pin) {
        return None;
    }
    let name = pin
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    Some(SelAsset {
        kind: "lora",
        name,
        version_id: pin_version(pin),
        base: pin
            .get("baseModel")
            .and_then(|b| b.as_str())
            .map(str::to_string),
    })
}

async fn selection_assets(state: &AppState, req: &GenerateRequest) -> (Vec<SelAsset>, String) {
    let pins = state.config.read().await.studio.pinned_loras.clone();
    let mut assets = Vec::new();
    if req.loras.is_empty() {
        for pin in &pins {
            if let Some(sel) = sel_from_pin(pin) {
                assets.push(sel);
            }
        }
    } else {
        for lora in &req.loras {
            let matched = pins.iter().find(|pin| {
                let name = pin.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let path = pin.get("path").and_then(|p| p.as_str()).unwrap_or("");
                name.eq_ignore_ascii_case(&lora.name)
                    || path == lora.path.as_deref().unwrap_or("")
                    || path.ends_with(&format!("/{}.safetensors", lora.name))
            });
            assets.push(SelAsset {
                kind: "lora",
                name: lora.name.clone(),
                version_id: lora.version_id.or_else(|| matched.and_then(pin_version)),
                base: lora.base_model.clone().or_else(|| {
                    matched
                        .and_then(|p| p.get("baseModel"))
                        .and_then(|b| b.as_str())
                        .map(str::to_string)
                }),
            });
        }
    }
    let mut family = assets
        .iter()
        .find_map(|a| a.base.clone())
        .unwrap_or_default();
    if let Some(client) = state.worker.read().await.as_ref() {
        if let Ok(active) = client.get_active_base().await {
            let model_path = active
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let base = active
                .get("baseModel")
                .and_then(|b| b.as_str())
                .map(str::to_string);
            if family.is_empty() {
                family = base.clone().unwrap_or_default();
            }
            let mut version_id = None;
            if let Ok(cps) = client.list_checkpoints().await {
                if let Some(hit) = cps.iter().find(|c| c.path == model_path) {
                    version_id = hit.version_id;
                    if family.is_empty() {
                        family = hit.base_model.clone().unwrap_or_default();
                    }
                }
            }
            if !model_path.is_empty() || version_id.is_some() {
                assets.push(SelAsset {
                    kind: "checkpoint",
                    name: checkpoint_display_name(&model_path),
                    version_id,
                    base,
                });
            }
        }
    }
    (assets, family)
}

fn selection_key(assets: &[SelAsset], family: &str) -> String {
    if assets.is_empty() {
        return format!("family:{family}");
    }
    assets
        .iter()
        .map(|a| {
            format!(
                "{}:{}:{}:{}",
                a.kind,
                a.version_id.unwrap_or(0),
                a.name,
                a.base.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

async fn recommend_current(
    state: &AppState,
    req: &GenerateRequest,
) -> crate::genmeta::RecommendedGen {
    let (assets, family) = selection_assets(state, req).await;
    let civ_ok = state.civitai.read().await.configured();
    if civ_ok {
        for kind in ["lora", "checkpoint"] {
            for asset in assets.iter().filter(|a| a.kind == kind) {
                let Some(vid) = asset.version_id else {
                    continue;
                };
                let fetched = {
                    let civ = state.civitai.read().await;
                    civ.version_example(vid).await
                };
                let Ok(ver) = fetched else {
                    continue;
                };
                let base = ver
                    .base_model
                    .clone()
                    .or_else(|| asset.base.clone())
                    .unwrap_or_else(|| family.clone());
                if ver
                    .example
                    .as_ref()
                    .map(|e| e.has_sampling())
                    .unwrap_or(false)
                {
                    let label = match &ver.model_name {
                        Some(n) if !n.is_empty() => format!("{n} {}", ver.version_name),
                        _ => format!("{} {}", asset.name, ver.version_name),
                    };
                    return crate::genmeta::recommend_from_example(
                        ver.example.as_ref(),
                        &base,
                        &label,
                    );
                }
            }
        }
    }
    let base = if family.is_empty() {
        assets
            .iter()
            .find_map(|a| a.base.clone())
            .unwrap_or_default()
    } else {
        family
    };
    crate::genmeta::recommend_from_example(None, &base, "")
}

async fn write_generation(state: &AppState, settings: &crate::genmeta::GenerationSettings) {
    {
        let mut cfg = state.config.write().await;
        cfg.studio.generation = settings.clone();
    }
    let _ = state.persist_secrets().await;
}

async fn prepare_generation(
    state: &AppState,
    req: &mut GenerateRequest,
    client_size: Option<String>,
    style_size: Option<String>,
) -> PreparedGen {
    if req.reuse_meta.unwrap_or(false) && crate::genmeta::reuse_command_only(&req.prompt) {
        if let Ok(Some(last)) = state.gallery.latest_generation().await {
            if !last.prompt.trim().is_empty() {
                req.prompt = last.prompt.clone();
            }
        }
    }

    let mut stored = state.config.read().await.studio.generation.clone();
    if req.reuse_meta.unwrap_or(false) {
        if let Ok(Some(last)) = state.gallery.latest_generation().await {
            if let Some(s) = last.sampler.clone().filter(|s| !s.trim().is_empty()) {
                stored.sampler = Some(s);
            }
            if let Some(c) = last.cfg {
                stored.cfg = Some(c);
            }
            if let Some(s) = last.steps {
                stored.steps = Some(s);
            }
            if let Some(c) = last.clip_skip {
                stored.clip_skip = Some(c);
            }
            stored.negative_prompt = last.negative_prompt.clone();
            stored.seed = last.seed.or(stored.seed);
            if let Some(sz) = last.size.clone() {
                stored.size = Some(sz);
            }
            if let Some(q) = last.quantity {
                stored.quantity = Some(q);
            }
            stored.locked = true;
            stored.source = Some("last".into());
            stored.source_label = Some(format!(
                "Reused from the last image{}",
                last.seed
                    .map(|s| format!(" (seed {s})"))
                    .unwrap_or_default()
            ));
            write_generation(state, &stored).await;
        }
    }

    let (assets, family) = selection_assets(state, req).await;
    let key = selection_key(&assets, &family);
    let refresh = !req.reuse_meta.unwrap_or(false)
        && !stored.locked
        && (!stored.sampling_complete() || stored.recommend_key.as_deref() != Some(key.as_str()));
    let fill_gaps =
        !req.reuse_meta.unwrap_or(false) && stored.locked && !stored.sampling_complete();
    if refresh || fill_gaps {
        let rec = recommend_current(state, req).await;
        if refresh {
            stored.apply_recommendation(&rec, &key);
            write_generation(state, &stored).await;
        } else {
            if stored
                .sampler
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                stored.sampler = Some(rec.sampler);
            }
            if stored.cfg.is_none() {
                stored.cfg = Some(rec.cfg);
            }
            if stored.steps.is_none() {
                stored.steps = Some(rec.steps);
            }
            if stored.clip_skip.is_none() {
                stored.clip_skip = Some(rec.clip_skip);
            }
            write_generation(state, &stored).await;
        }
    }

    let mut working = stored.clone();
    if let Some(s) = req.sampler.clone().filter(|s| !s.trim().is_empty()) {
        working.sampler = Some(s);
    }
    if let Some(c) = req.cfg {
        working.cfg = Some(crate::genmeta::clamp_cfg(c));
    }
    if let Some(s) = req.steps {
        working.steps = Some(crate::genmeta::clamp_steps(s));
    }
    if let Some(c) = req.clip_skip {
        working.clip_skip = Some(crate::genmeta::clamp_clip(c));
    }
    if let Some(n) = req.negative_prompt.clone() {
        working.negative_prompt = {
            let t = n.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
    }
    if let Some(seed) = req.seed {
        working.seed = Some(seed);
    }
    if let Some(q) = req.quantity {
        working.quantity = Some(crate::genmeta::clamp_quantity(q));
    }
    if let Some(sz) = client_size.as_deref().and_then(crate::genmeta::clean_size) {
        working.size = Some(sz);
    }

    if req.persist_meta.unwrap_or(false) && !req.reuse_meta.unwrap_or(false) {
        working.locked = true;
        working.source = Some("agent".into());
        working.source_label = Some("Set from chat".into());
        write_generation(state, &working).await;
    }

    let fam = crate::genmeta::family_default(&family);
    let sampler = working
        .sampler
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| fam.sampler.into());
    let cfg = crate::genmeta::clamp_cfg(working.cfg.unwrap_or(fam.cfg));
    let steps = crate::genmeta::clamp_steps(working.steps.unwrap_or(fam.steps));
    let clip_skip = crate::genmeta::clamp_clip(working.clip_skip.unwrap_or(fam.clip_skip));
    let seed = working.seed.unwrap_or_else(crate::genmeta::random_seed);
    let quantity = crate::genmeta::clamp_quantity(working.quantity.unwrap_or(1));
    let size = working
        .size
        .clone()
        .or_else(|| style_size.as_deref().and_then(crate::genmeta::clean_size));
    PreparedGen {
        sampler,
        cfg,
        steps,
        clip_skip,
        negative: working.negative_prompt.clone(),
        seed,
        size,
        quantity,
        source: working.source.clone().unwrap_or_else(|| "family".into()),
        source_label: working
            .source_label
            .clone()
            .unwrap_or_else(|| fam_label_fallback(&fam)),
    }
}

fn fam_label_fallback(fam: &crate::genmeta::FamilyDefault) -> String {
    format!("Family default ({})", fam.label)
}

async fn run_generate(state: &AppState, mut req: GenerateRequest) -> ApiResult<serde_json::Value> {
    let client_size = req.size.clone();
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
                    version_id: None,
                    model_id: None,
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
        && local_worker_ready(&state).await
    {
        req.backend = Some("local".into());
    }

    let style_size = if client_size.is_none() {
        req.size.clone()
    } else {
        None
    };
    let prepared = prepare_generation(state, &mut req, client_size, style_size).await;
    req.negative_prompt = prepared.negative.clone();
    req.seed = Some(prepared.seed);
    req.size = prepared.size.clone();
    req.sampler = Some(prepared.sampler.clone());
    req.cfg = Some(prepared.cfg);
    req.steps = Some(prepared.steps);
    req.clip_skip = Some(prepared.clip_skip);
    req.quantity = Some(prepared.quantity);

    let backend = req.backend.clone().unwrap_or_else(|| "openrouter".into());
    let (stored_locked, stored_enhance) = {
        let cfg = state.config.read().await;
        (
            cfg.studio.generation.locked,
            cfg.studio.generation.enhance.clone(),
        )
    };
    let enhance = crate::prefs::resolve_generate_enhance(
        req.enhance.as_deref(),
        stored_enhance.as_deref(),
        stored_locked,
    );
    let enhance_on = crate::prefs::enhance_enabled(&enhance);
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
        let modality_for_writer = resolve_generate_modality(&req);
        let worker_ready = local_worker_ready(&state).await;
        // Local stills, and Auto GIF (the worker path), use the local writer.
        // Ordinary text stays on the baseline model. Adult text uses Cydonia/Dolphin.
        let local =
            routes_to_local_worker(req.backend.as_deref(), modality_for_writer, worker_ready);
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
                } else if let Some(retry_model) =
                    adult_retry_model(&p.model, &privacy, &auto_fallbacks)
                {
                    tracing::info!(
                        first = %p.model,
                        retry = %retry_model,
                        "local adult prompt rewrite retry"
                    );
                    let retry = or.chat(&system, &req.prompt, Some(&retry_model)).await.ok();
                    if let Some(text) = retry
                        .as_deref()
                        .map(normalize_sd_rewrite)
                        .filter(|t| adult_rewrite_ok(t, &req.prompt))
                    {
                        final_prompt = text.clone();
                        enhanced = Some(text);
                    } else {
                        // A failed or refused rewrite must not abort the generate.
                        // The worker still receives the user's scene.
                        tracing::warn!(
                            first = %p.model,
                            retry = %retry_model,
                            "local adult prompt rewrite unusable; using the original scene"
                        );
                    }
                } else {
                    tracing::warn!(
                        model = %p.model,
                        "local adult prompt rewrite unusable; using the original scene"
                    );
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
    struct Batch {
        images: Vec<(Vec<u8>, i64)>,
        mime: String,
        model: String,
        backend: String,
        base_model: Option<String>,
        checkpoint_path: Option<String>,
        sampler_applied: Option<String>,
        sampler_note: Option<String>,
        clip_skip_applied: bool,
        steps: u32,
        cfg: f64,
    }
    let batch = if modality.is_motion() {
        let motion = run_motion_generate(state, &req, &final_prompt, modality).await?;
        let seed = motion
            .applied
            .as_ref()
            .and_then(|a| a.images.first())
            .and_then(|i| i.seed)
            .unwrap_or(prepared.seed);
        let (steps, cfg, applied, note, clip_ok) = motion
            .applied
            .as_ref()
            .map(|a| {
                (
                    a.steps.unwrap_or(prepared.steps),
                    a.guidance.unwrap_or(prepared.cfg),
                    a.scheduler.clone().or(a.sampler.clone()),
                    a.sampler_note.clone(),
                    a.clip_skip_applied.unwrap_or(false),
                )
            })
            .unwrap_or_else(|| {
                (
                    prepared.steps,
                    prepared.cfg,
                    None,
                    Some(
                        "recorded only; this path does not apply sampler, steps, or clip skip"
                            .into(),
                    ),
                    false,
                )
            });
        Batch {
            images: vec![(motion.bytes, seed)],
            mime: motion.mime,
            model: motion.model,
            backend: motion.backend,
            base_model: None,
            checkpoint_path: None,
            sampler_applied: applied,
            sampler_note: note,
            clip_skip_applied: clip_ok,
            steps,
            cfg,
        }
    } else if backend == "local" {
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
        let worker_prompt = if loras.is_empty() {
            final_prompt.clone()
        } else {
            crate::genmeta::strip_a1111_resource_tags(&final_prompt)
        };
        let call = crate::worker::GenCall {
            prompt: worker_prompt.clone(),
            model: model.clone(),
            loras,
            size: req.size.clone(),
            seed: req.seed,
            negative: req.negative_prompt.clone(),
            steps: req.steps,
            guidance: req.cfg,
            sampler: req.sampler.clone(),
            clip_skip: req.clip_skip,
            quantity: req.quantity,
        };
        let result = client.generate(&call).await.map_err(ApiError::from)?;
        let images = result
            .images
            .iter()
            .enumerate()
            .map(|(i, img)| {
                (
                    img.bytes.clone(),
                    img.seed
                        .unwrap_or_else(|| prepared.seed.wrapping_add(i as i64)),
                )
            })
            .collect();
        Batch {
            images,
            mime: "image/png".into(),
            model: model.clone().unwrap_or_else(|| "local".into()),
            backend: "local".into(),
            base_model: active_base_label,
            checkpoint_path: model.clone(),
            sampler_applied: result.scheduler.clone().or(result.sampler.clone()),
            sampler_note: result.sampler_note.clone(),
            clip_skip_applied: result.clip_skip_applied.unwrap_or(false),
            steps: result.steps.unwrap_or(prepared.steps),
            cfg: result.guidance.unwrap_or(prepared.cfg),
        }
    } else {
        let or = state.openrouter.read().await;
        let img = or
            .generate_image(&final_prompt, req.model.as_deref(), req.size.as_deref())
            .await
            .map_err(ApiError::from)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&img.b64)
            .map_err(|e| ApiError::Msg(e.to_string()))?;
        let note = if prepared.quantity > 1 {
            "OpenRouter image API generates one image; sampler, steps, clip skip, and quantity are stored only"
        } else {
            "recorded only; OpenRouter image API does not apply sampler, steps, or clip skip"
        };
        Batch {
            images: vec![(bytes, prepared.seed)],
            mime: img.mime,
            model: model.unwrap_or(default_image),
            backend: "openrouter".into(),
            base_model: None,
            checkpoint_path: None,
            sampler_applied: None,
            sampler_note: Some(note.into()),
            clip_skip_applied: false,
            steps: prepared.steps,
            cfg: prepared.cfg,
        }
    };

    let lora_names: Vec<String> = req.loras.iter().map(|l| l.name.clone()).collect();
    let lora_pins: Vec<crate::gallery::ArtifactLoraMeta> = req
        .loras
        .iter()
        .map(|l| crate::gallery::ArtifactLoraMeta {
            name: l.name.clone(),
            path: l.path.clone(),
            scale: l.scale,
            model_id: l.model_id,
            version_id: l.version_id,
        })
        .collect();
    let checkpoint = batch
        .checkpoint_path
        .as_deref()
        .and_then(|p| crate::reproduce::checkpoint_meta_from_path(p, batch.base_model.as_deref()));
    let worker_prompt_stored = if backend == "local" && !req.loras.is_empty() {
        let stripped = crate::genmeta::strip_a1111_resource_tags(&final_prompt);
        if stripped != final_prompt {
            Some(stripped)
        } else {
            None
        }
    } else {
        None
    };
    let mut saved_all = Vec::new();
    for (bytes, seed) in &batch.images {
        let mut meta = ArtifactMeta {
            id: String::new(),
            prompt: req.prompt.clone(),
            worker_prompt: worker_prompt_stored.clone(),
            enhanced_prompt: enhanced.clone(),
            backend: batch.backend.clone(),
            model: batch.model.clone(),
            style_ref: req.style_ref.clone(),
            loras: lora_names.clone(),
            lora_pins: lora_pins.clone(),
            checkpoint: checkpoint.clone(),
            enhance_on: Some(enhance_on),
            created_at: String::new(),
            mime: batch.mime.clone(),
            filename: String::new(),
            negative_prompt: prepared.negative.clone(),
            sampler: Some(prepared.sampler.clone()),
            sampler_applied: batch.sampler_applied.clone(),
            sampler_note: batch.sampler_note.clone(),
            cfg: Some(batch.cfg),
            steps: Some(batch.steps),
            clip_skip: Some(prepared.clip_skip),
            clip_skip_applied: Some(batch.clip_skip_applied),
            seed: Some(*seed),
            size: prepared.size.clone(),
            base_model: batch.base_model.clone(),
            quantity: Some(prepared.quantity),
            gen_source: Some(prepared.source.clone()),
        };
        let saved = state
            .gallery
            .save(bytes, &batch.mime, &mut meta)
            .await
            .map_err(ApiError::from)?;
        saved_all.push(saved);
    }
    let saved = saved_all
        .first()
        .cloned()
        .ok_or_else(|| ApiError::Msg("worker returned no images".into()))?;
    {
        let mut cfgw = state.config.write().await;
        let g = &mut cfgw.studio.generation;
        g.last_seed = saved.seed;
        g.sampler_applied = batch.sampler_applied.clone();
        g.sampler_note = batch.sampler_note.clone();
        g.clip_skip_applied = Some(batch.clip_skip_applied);
        if g.sampler.is_none() {
            g.sampler = Some(prepared.sampler.clone());
        }
        if g.cfg.is_none() {
            g.cfg = Some(batch.cfg);
        }
        if g.steps.is_none() {
            g.steps = Some(batch.steps);
        }
        if g.clip_skip.is_none() {
            g.clip_skip = Some(prepared.clip_skip);
        }
    }
    let _ = state.persist_secrets().await;

    let extras: Vec<Value> = saved_all.iter().skip(1).map(with_gallery_url).collect();
    Ok(json!({
        "artifactId": saved.id,
        "backend": batch.backend,
        "model": batch.model,
        "enhancedPrompt": enhanced,
        "modality": modality.as_str(),
        "mime": batch.mime,
        "url": format!("/v1/gallery/{}/image", saved.id),
        "meta": saved,
        "seed": prepared.seed,
        "seeds": saved_all.iter().filter_map(|m| m.seed).collect::<Vec<_>>(),
        "sampler": prepared.sampler,
        "samplerApplied": batch.sampler_applied,
        "samplerNote": batch.sampler_note,
        "cfg": batch.cfg,
        "steps": batch.steps,
        "clipSkip": prepared.clip_skip,
        "clipSkipApplied": batch.clip_skip_applied,
        "negativePrompt": prepared.negative,
        "size": prepared.size,
        "quantity": prepared.quantity,
        "baseModel": batch.base_model,
        "genSource": prepared.source,
        "sourceLabel": prepared.source_label,
        "results": extras,
    }))
}

/// Local jobs run when a saved worker URL has attached the client.
async fn local_worker_ready(state: &AppState) -> bool {
    state.worker.read().await.is_some()
}

/// Local stills when the composer pinned Local. Auto GIF uses the worker when one is up.
/// Cloud video and omitted-backend stills stay on OpenRouter.
fn routes_to_local_worker(
    backend: Option<&str>,
    modality: crate::router::Modality,
    worker_ready: bool,
) -> bool {
    let pinned = backend.map(str::trim).filter(|s| !s.is_empty());
    match pinned {
        Some(s) if s.eq_ignore_ascii_case("local") => true,
        Some(s) if s.eq_ignore_ascii_case("auto") => {
            worker_ready && modality == crate::router::Modality::Gif
        }
        None => worker_ready && modality == crate::router::Modality::Gif,
        _ => false,
    }
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

struct MotionOut {
    bytes: Vec<u8>,
    mime: String,
    model: String,
    backend: String,
    applied: Option<crate::worker::WorkerGenResult>,
}

async fn run_motion_generate(
    state: &AppState,
    req: &GenerateRequest,
    final_prompt: &str,
    modality: crate::router::Modality,
) -> ApiResult<MotionOut> {
    let want_gif = matches!(modality, crate::router::Modality::Gif);
    let backend = req.backend.as_deref().unwrap_or("auto");
    let pinned_local = backend == "local";

    // Local motion is a worker GIF (the engine has no mp4 T2V). Cloud video stays on OpenRouter.
    // Do not fall through to OpenRouter when the composer pinned Local.
    if pinned_local || (want_gif && backend == "auto") {
        let worker = state.worker.read().await;
        if let Some(client) = worker.as_ref() {
            let gif_model = crate::base_models::filter_local_model(req.model.as_deref());
            let call = crate::worker::GenCall {
                prompt: final_prompt.to_string(),
                model: gif_model.clone(),
                loras: vec![],
                size: req.size.clone(),
                seed: req.seed,
                negative: req.negative_prompt.clone(),
                steps: req.steps,
                guidance: req.cfg,
                sampler: req.sampler.clone(),
                clip_skip: req.clip_skip,
                quantity: Some(1),
            };
            match client.generate_gif(&call).await {
                Ok(result) => {
                    let bytes = result
                        .images
                        .first()
                        .map(|i| i.bytes.clone())
                        .unwrap_or_default();
                    return Ok(MotionOut {
                        bytes,
                        mime: "image/gif".into(),
                        model: gif_model.unwrap_or_else(|| "local-gif".into()),
                        backend: "local".into(),
                        applied: Some(result),
                    });
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
    Ok(MotionOut {
        bytes,
        mime,
        model: vid.model,
        backend: "openrouter".into(),
        applied: None,
    })
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
    // Original file bytes. GIF stays image/gif (no PNG/WebP poster).
    let ct = crate::gallery::content_type_for_bytes(&bytes, &meta.mime);
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, ct),
            (axum::http::header::CONTENT_DISPOSITION, "inline"),
        ],
        bytes,
    )
        .into_response())
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
        "loraPins": meta.lora_pins,
        "checkpoint": meta.checkpoint,
    })))
}

/// Load sidecar meta into generation settings, pins, and enhance-off (does not download).
pub async fn gallery_apply(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let meta = state
        .gallery
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound)?;
    let inventory = studio_inventory(&state).await;
    let mut settings = state.config.read().await.studio.generation.clone();
    let mut studio = state.config.read().await.studio.clone();
    let out = crate::reproduce::apply_gallery_meta(&meta, &inventory, &mut settings, &mut studio);
    {
        let mut cfg = state.config.write().await;
        cfg.studio.generation = settings.clone();
        cfg.studio.pinned_loras = studio.pinned_loras.clone();
    }
    let persisted = state.persist_secrets().await.is_ok();
    if let Some(ck) = meta.checkpoint.as_ref().and_then(|c| c.path.as_deref()) {
        if inventory.installed.iter().any(|a| a.path == ck) {
            if let Some(w) = state.worker.read().await.as_ref() {
                let family = meta
                    .base_model
                    .as_deref()
                    .or_else(|| meta.checkpoint.as_ref().map(|c| c.name.as_str()));
                let _ = w.set_active_base(ck, family).await;
            }
        }
    }
    write_generation(&state, &settings).await;
    Ok(Json(json!({ "applied": out, "meta": meta, "persisted": persisted })))
}

pub async fn reproduction_community_apply(
    State(state): State<AppState>,
    Json(image): Json<CommunityImage>,
) -> ApiResult<Json<serde_json::Value>> {
    let inventory = studio_inventory(&state).await;
    let mut settings = state.config.read().await.studio.generation.clone();
    let mut studio = state.config.read().await.studio.clone();
    let out = crate::reproduce::apply_community_image(&image, &inventory, &mut settings, &mut studio);
    {
        let mut cfg = state.config.write().await;
        cfg.studio.generation = settings.clone();
        cfg.studio.pinned_loras = studio.pinned_loras.clone();
    }
    let persisted = state.persist_secrets().await.is_ok();
    if let Some(base) = crate::reproduce::installed_checkpoint_for_image(&image, &inventory) {
        if let Some(w) = state.worker.read().await.as_ref() {
            let _ = w
                .set_active_base(&base.path, image.checkpoint.as_deref())
                .await;
        }
    }
    write_generation(&state, &settings).await;
    Ok(Json(json!({ "applied": out, "persisted": persisted })))
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

pub async fn route_defaults(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(route_defaults_payload(&state).await)
}

async fn route_defaults_payload(state: &AppState) -> Value {
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
    })
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

#[derive(Debug, Deserialize)]
pub struct CivitaiLookupQuery {
    pub q: String,
    /// One agent search type: LORA, LoCon, DoRA, LyCORIS, Checkpoint, TextualInversion, VAE.
    #[serde(rename = "type")]
    pub asset_type: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

fn default_limit() -> u32 {
    20
}

pub async fn civitai_search(
    State(state): State<AppState>,
    Query(q): Query<CivitaiSearchQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let civ = state.civitai.read().await;
    let types = parse_civitai_types(q.types.as_deref());
    let type_refs: Vec<&str> = types.iter().map(String::as_str).collect();
    let mut items = civ
        .search_types(&q.q, q.limit, &type_refs, allow_nsfw)
        .await
        .map_err(ApiError::from)?;
    if !allow_nsfw {
        items.retain(|c| !looks_nsfw_label(&c.name) && !c.nsfw);
    }
    Ok(Json(json!({ "items": items })))
}

/// One Civitai type. Installed matches first, then other hits. Does not pin or download.
pub async fn civitai_lookup(
    State(state): State<AppState>,
    Query(q): Query<CivitaiLookupQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = q.limit.unwrap_or(12).clamp(1, 20);
    let value = typed_civitai_lookup(&state, &q.q, &q.asset_type, limit)
        .await
        .map_err(ApiError::Msg)?;
    Ok(Json(value))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiImagesQuery {
    /// Civitai model id. Optional when `modelVersionId` is set.
    #[serde(default)]
    pub model_id: Option<i64>,
    #[serde(default)]
    pub model_version_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Opaque `nextCursor` from the previous images response.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Page number when metadata has `nextPage` and no cursor.
    #[serde(default)]
    pub page: Option<u32>,
}

fn json_i64(v: Option<&serde_json::Value>) -> Option<i64> {
    let v = v?;
    v.as_i64()
        .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| {
            v.as_str()
                .and_then(|s| s.trim().parse().ok())
                .filter(|n: &i64| *n > 0)
        })
}

fn deserialize_optional_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(opt.and_then(|v| json_i64(Some(&v))))
}

fn installed_match(asset: crate::worker::LocalAsset) -> crate::civitai::InstalledAssetMatch {
    let kind = if asset.path.starts_with("checkpoints/") {
        "checkpoint"
    } else if asset.path.starts_with("embeddings/") {
        "embedding"
    } else if asset.path.starts_with("loras/") {
        "lora"
    } else {
        asset.model_type.as_deref().unwrap_or(asset.kind.as_str())
    };
    crate::civitai::InstalledAssetMatch {
        name: asset.name.unwrap_or_else(|| {
            asset
                .path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(asset.id.as_str())
                .to_string()
        }),
        path: asset.path,
        kind: kind.to_string(),
        version_id: asset.version_id,
        model_id: asset.model_id,
        hash: asset.hash,
    }
}

async fn studio_inventory(state: &AppState) -> crate::civitai::StudioInventory {
    let worker = state.worker.read().await;
    let Some(client) = worker.as_ref() else {
        return crate::civitai::StudioInventory {
            installed: Vec::new(),
            current_base: None,
            base_known: true,
        };
    };
    let mut installed = Vec::new();
    if let Ok(items) = client.list_checkpoints().await {
        installed.extend(items.into_iter().map(installed_match));
    }
    if let Ok(items) = client.list_loras().await {
        installed.extend(items.into_iter().map(installed_match));
    }
    if let Ok(items) = client.list_embeddings().await {
        installed.extend(items.into_iter().map(installed_match));
    }
    let base = client.get_active_base().await.ok();
    let Some(base) = base else {
        return crate::civitai::StudioInventory {
            installed,
            current_base: None,
            base_known: false,
        };
    };
    let path = base
        .get("model")
        .and_then(|m| m.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let current_base = path.map(|path| {
        installed
            .iter()
            .find(|asset| asset.path == path)
            .cloned()
            .unwrap_or_else(|| crate::civitai::InstalledAssetMatch {
                name: path
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(path.as_str())
                    .to_string(),
                path,
                kind: "checkpoint".into(),
                version_id: None,
                model_id: None,
                hash: None,
            })
    });
    crate::civitai::StudioInventory {
        installed,
        current_base,
        base_known: true,
    }
}

async fn load_community_images(
    state: &AppState,
    model_id: Option<i64>,
    version_id: Option<i64>,
    allow_nsfw: bool,
    limit: u32,
    cursor: Option<&str>,
    page: Option<u32>,
) -> Result<crate::civitai::CommunityImagePage, anyhow::Error> {
    let mut page = {
        let civ = state.civitai.read().await;
        civ.community_images(model_id, version_id, allow_nsfw, limit, cursor, page)
            .await?
    };
    let inventory = studio_inventory(state).await;
    crate::civitai::annotate_community_images(&mut page.items, &inventory);
    Ok(page)
}

/// Community images for the model or version in context. Pages follow Civitai's cursor.
pub async fn civitai_images(
    State(state): State<AppState>,
    Query(q): Query<CivitaiImagesQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let model_id = q.model_id.filter(|n| *n > 0);
    let version_id = q.model_version_id.filter(|n| *n > 0);
    if model_id.is_none() && version_id.is_none() {
        return Err(ApiError::Msg(
            "modelId or modelVersionId is required".into(),
        ));
    }
    let allow_nsfw = state.config.read().await.alignment_prefs().allow_nsfw_local;
    let limit = q.limit.unwrap_or(20);
    let page = load_community_images(
        &state,
        model_id,
        version_id,
        allow_nsfw,
        limit,
        q.cursor.as_deref(),
        q.page,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(json!({
        "modelId": model_id,
        "modelVersionId": version_id,
        "nsfw": allow_nsfw,
        "count": page.items.len(),
        "items": page.items,
        "nextCursor": page.next_cursor,
        "nextPage": page.next_page,
        "upstream": "https://civitai.com/api/v1/images",
        "note": "Community image posts. Pass nextCursor or nextPage for another page. Does not download or generate."
    })))
}

#[derive(Debug, Deserialize)]
pub struct CivitaiMediaQuery {
    pub url: String,
}

/// Proxy Civitai community image/video bytes for same-origin `<video>` playback.
pub async fn civitai_media(
    State(state): State<AppState>,
    Query(q): Query<CivitaiMediaQuery>,
) -> ApiResult<Response> {
    let url = q.url.trim();
    if url.is_empty() || !crate::civitai::civitai_media_url_ok(url) {
        return Err(ApiError::Msg("url must be https on image.civitai.com".into()));
    }
    let civ = state.civitai.read().await;
    let (bytes, ct) = civ
        .fetch_media(url)
        .await
        .map_err(|e| ApiError::Msg(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, ct),
            (header::CONTENT_DISPOSITION, "inline".into()),
            (
                header::CACHE_CONTROL,
                "public, max-age=86400, stale-while-revalidate=604800".into(),
            ),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".into()),
        ],
        bytes,
    )
        .into_response())
}

fn civitai_detail_response(detail: crate::civitai::CivitaiModelDetail) -> Json<serde_json::Value> {
    // Recommended checkpoints are the base suggestion rows. Popular-family hits stay the fallback.
    let recommended: Vec<_> = detail
        .recommended_resources
        .iter()
        .filter(|r| r.resource_type.eq_ignore_ascii_case("Checkpoint"))
        .map(|r| {
            json!({
                "relation": "recommended_checkpoint",
                "note": "Recommended base checkpoint",
                "model": {
                    "id": r.id,
                    "modelId": r.id,
                    "name": r.name,
                    "modelType": r.resource_type,
                    "versionId": r.model_version_id,
                    "baseModel": r.base_model,
                    "nsfw": r.nsfw,
                }
            })
        })
        .collect();
    let related: Vec<_> = if !recommended.is_empty() {
        recommended
    } else {
        detail
            .related
            .iter()
            .map(|r| {
                json!({
                    "relation": "compatible_checkpoint",
                    "note": detail.base_model.as_ref().map(|b| format!("Popular checkpoint for {b}")),
                    "model": r,
                })
            })
            .collect()
    };
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
    let allow_nsfw = state.config.read().await.alignment_prefs().allow_nsfw_local;
    let civ = state.civitai.read().await;
    let detail = civ
        .get_model(id, allow_nsfw)
        .await
        .map_err(ApiError::from)?;
    Ok(civitai_detail_response(detail))
}

/// Resolve Installed assets that only stored `versionId` (no `modelId` in sidecar meta).
pub async fn civitai_model_version(
    State(state): State<AppState>,
    axum::extract::Path(version_id): axum::extract::Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let allow_nsfw = state.config.read().await.alignment_prefs().allow_nsfw_local;
    let civ = state.civitai.read().await;
    let detail = civ
        .get_model_by_version(version_id, allow_nsfw)
        .await
        .map_err(ApiError::from)?;
    Ok(civitai_detail_response(detail))
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
    /// Agent model (`auto` = internal Auto; `openrouter/auto` is the explicit upstream router).
    pub chat_model: String,
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

fn blank_generate_request() -> GenerateRequest {
    GenerateRequest {
        prompt: String::new(),
        negative_prompt: None,
        enhance: None,
        backend: None,
        model: None,
        style_ref: None,
        loras: vec![],
        size: None,
        seed: None,
        modality: None,
        duration: None,
        steps: None,
        cfg: None,
        sampler: None,
        clip_skip: None,
        quantity: None,
        reuse_meta: None,
        persist_meta: None,
    }
}

async fn generation_meta_value(state: &AppState, force: bool) -> Value {
    let blank = blank_generate_request();
    let (assets, family) = selection_assets(state, &blank).await;
    let key = selection_key(&assets, &family);
    let mut settings = state.config.read().await.studio.generation.clone();
    let refresh = force
        || (!settings.locked
            && (!settings.sampling_complete()
                || settings.recommend_key.as_deref() != Some(key.as_str())));
    if refresh {
        let rec = recommend_current(state, &blank).await;
        let seed = settings.seed;
        let quantity = settings.quantity;
        let size = settings.size.clone();
        let last_seed = settings.last_seed;
        settings.apply_recommendation(&rec, &key);
        settings.seed = seed;
        settings.quantity = quantity;
        if size.as_deref().is_some_and(|s| !s.trim().is_empty()) {
            settings.size = size;
        }
        settings.last_seed = last_seed;
        write_generation(state, &settings).await;
    }
    json!({ "settings": settings, "persisted": true })
}

pub async fn generation_meta_get(State(state): State<AppState>) -> Json<Value> {
    Json(generation_meta_value(&state, false).await)
}

pub async fn generation_meta_recommend(State(state): State<AppState>) -> Json<Value> {
    Json(generation_meta_value(&state, true).await)
}

pub async fn generation_meta_put(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    if let Some(msg) = reject_secret_fields(&body) {
        return Err(ApiError::Msg(msg));
    }
    let patch = crate::genmeta::patch_from_value(&body);
    let source = match body.get("source").and_then(|s| s.as_str()) {
        Some("gallery") => "gallery",
        _ => "user",
    };
    let mut settings = state.config.read().await.studio.generation.clone();
    if patch.any() {
        settings.apply_patch(&patch, source, true);
    }
    if source == "gallery" {
        if let Some(label) = body
            .get("sourceLabel")
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            settings.source_label = Some(label.chars().take(160).collect());
        }
    }
    let pin_weights = body
        .get("loraWeights")
        .or_else(|| body.get("lora_weights"))
        .and_then(|v| v.as_array());
    if pin_weights.is_some() || patch.any() {
        write_generation(&state, &settings).await;
    }
    if let Some(weights) = pin_weights {
        let mut cfg = state.config.write().await;
        for w in weights {
            let name = w.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
            let path = w.get("path").and_then(|p| p.as_str()).unwrap_or("").trim();
            let scale = w.get("scale").and_then(|s| s.as_f64());
            if name.is_empty() && path.is_empty() {
                continue;
            }
            let mut card = json!({ "name": name, "path": path });
            if let Some(s) = scale {
                card["scale"] = json!(s);
            }
            let _ = crate::studio::pin_upsert(&mut cfg.studio, &card);
        }
        let _ = state.persist_secrets().await;
    }
    let pins = state.config.read().await.studio.pinned_loras.clone();
    Ok(Json(json!({ "settings": settings, "pinnedLoras": pins, "persisted": true })))
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
    fn refusal_heuristic_ignores_scene_wording() {
        assert!(looks_like_model_refusal(
            "I can't create explicit images, but here is a tasteful portrait."
        ));
        assert!(!looks_like_model_refusal(
            "she whispered i can't breathe, pressed against my chest"
        ));
        assert!(!looks_like_model_refusal(
            "not able to stand, knees against my thighs"
        ));
    }

    #[test]
    fn adult_rewrite_accepts_an_echo_of_a_checkpoint_prompt() {
        let scene = "nude woman, on a bed, soft window light, detailed skin";
        assert!(adult_rewrite_ok(scene, scene));
        assert!(!adult_rewrite_ok("nude woman", "nude woman"));
        assert!(adult_retry_model(
            "thedrummer/cydonia-24b-v4.1",
            &crate::prefs::PrivacyPrefs::default(),
            &crate::prefs::AutoFallbacksPrefs {
                cost_tier: "medium".into(),
                ..Default::default()
            },
        )
        .is_some_and(|id| id != "thedrummer/cydonia-24b-v4.1"));
    }

    #[test]
    fn chat_pin_does_not_replace_the_resolved_image_model() {
        let mut run = ActionRun {
            loras: vec![],
            image_model: Some("google/gemini-2.5-flash-image".into()),
            video_model: Some("google/veo-3.1".into()),
            backend: None,
            phase: Phase::default(),
        };
        let chat_only = PlannedAction {
            action_type: "set_chat_models".into(),
            args: json!({ "chatModel": "deepseek/deepseek-v4-pro" }),
        };
        run.absorb(
            &chat_only,
            &json!({
                "status": "executed",
                "type": "set_chat_models",
                "result": {
                    "chatModel": "deepseek/deepseek-v4-pro",
                    "imageModel": "auto",
                    "videoModel": "auto"
                }
            }),
        );
        assert_eq!(
            run.image_model.as_deref(),
            Some("google/gemini-2.5-flash-image")
        );
        assert_eq!(run.video_model.as_deref(), Some("google/veo-3.1"));
        let image_pin = PlannedAction {
            action_type: "set_chat_models".into(),
            args: json!({ "imageModel": "black-forest-labs/flux.2-klein-4b" }),
        };
        run.absorb(
            &image_pin,
            &json!({
                "status": "executed",
                "type": "set_chat_models",
                "result": { "imageModel": "black-forest-labs/flux.2-klein-4b" }
            }),
        );
        assert_eq!(
            run.image_model.as_deref(),
            Some("black-forest-labs/flux.2-klein-4b")
        );
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

    #[test]
    fn lookup_prefers_installed_version_and_keeps_other_hits() {
        let installed = vec![json!({
            "id": "eagle_wing",
            "name": "Eagle Wing Warrior",
            "path": "loras/eagle_wing.safetensors",
            "versionId": 100,
            "modelId": null,
            "thumbnailUrl": null,
            "kind": "lora"
        })];
        let search = vec![
            json!({
                "id": 901,
                "name": "Eagle Wing Warrior",
                "versionId": 100,
                "thumbnailUrl": "https://img/eagle.png",
                "modelType": "LORA"
            }),
            json!({
                "id": 902,
                "name": "Valkyrie Feather Armor",
                "versionId": 200,
                "thumbnailUrl": "https://img/val.png",
                "modelType": "LORA"
            }),
        ];
        let ordered = order_lookup_assets(&installed, &search, "eagle wing");
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0]["installed"], json!(true));
        assert_eq!(ordered[0]["path"], "loras/eagle_wing.safetensors");
        assert_eq!(ordered[0]["versionId"], 100);
        assert_eq!(ordered[0]["modelId"], 901);
        assert_eq!(ordered[0]["thumbnailUrl"], "https://img/eagle.png");
        assert_eq!(ordered[1]["installed"], json!(false));
        assert_eq!(ordered[1]["name"], "Valkyrie Feather Armor");
        assert_eq!(ordered[1].get("path"), None);
    }

    #[test]
    fn installed_filter_keeps_only_the_requested_type() {
        let lora = json!({
            "name": "Eagle Wing",
            "path": "loras/eagle.safetensors",
            "kind": "lora"
        });
        let typed_lora = json!({
            "name": "Eagle LoRA",
            "path": "loras/eagle_lora.safetensors",
            "kind": "lora",
            "modelType": "LORA"
        });
        let locon = json!({
            "name": "Eagle LoCon",
            "path": "loras/eagle_locon.safetensors",
            "kind": "lora",
            "modelType": "LoCon"
        });
        let ckpt = json!({
            "name": "Eagle Wing",
            "path": "checkpoints/eagle.safetensors",
            "kind": "checkpoint",
            "modelType": "Checkpoint"
        });
        let emb = json!({
            "name": "Eagle Token",
            "path": "embeddings/eagle.pt",
            "kind": "embedding"
        });
        let vae = json!({
            "name": "Eagle VAE",
            "path": "vae/eagle.safetensors",
            "kind": "vae"
        });
        assert!(installed_matches_search_type(&lora, "LORA"));
        assert!(installed_matches_search_type(&lora, "LoCon"));
        assert!(installed_matches_search_type(&typed_lora, "LORA"));
        assert!(!installed_matches_search_type(&typed_lora, "LoCon"));
        assert!(installed_matches_search_type(&locon, "LoCon"));
        assert!(!installed_matches_search_type(&locon, "LORA"));
        assert!(!installed_matches_search_type(&ckpt, "LORA"));
        assert!(installed_matches_search_type(&ckpt, "Checkpoint"));
        assert!(installed_matches_search_type(&emb, "TextualInversion"));
        assert!(!installed_matches_search_type(&emb, "VAE"));
        assert!(installed_matches_search_type(&vae, "VAE"));

        let installed = vec![lora, ckpt];
        let kept: Vec<_> = installed
            .iter()
            .filter(|a| installed_matches_search_type(a, "LORA"))
            .cloned()
            .collect();
        let search = vec![json!({
            "id": 901,
            "name": "Eagle Wing XL",
            "versionId": 200,
            "modelType": "LORA"
        })];
        let ordered = order_lookup_assets(&kept, &search, "eagle wing");
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0]["installed"], json!(true));
        assert_eq!(ordered[0]["path"], "loras/eagle.safetensors");
        assert_eq!(ordered[1]["installed"], json!(false));
        assert_eq!(ordered[1]["name"], "Eagle Wing XL");
    }

    #[test]
    fn lookup_keeps_a_different_version_in_the_new_list() {
        let installed = vec![json!({
            "name": "Eagle Wing Warrior",
            "path": "loras/eagle.safetensors",
            "versionId": 100,
            "modelId": 901,
            "thumbnailUrl": "https://img/installed.png"
        })];
        let search = vec![json!({
            "id": 901,
            "name": "Eagle Wing Warrior XL",
            "versionId": 101,
            "thumbnailUrl": "https://img/xl.png"
        })];
        let ordered = order_lookup_assets(&installed, &search, "eagle wing warrior");
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0]["installed"], json!(true));
        assert_eq!(ordered[0]["path"], "loras/eagle.safetensors");
        assert_eq!(ordered[0]["versionId"], 100);
        assert_eq!(ordered[0]["thumbnailUrl"], "https://img/installed.png");
        assert_eq!(ordered[1]["installed"], json!(false));
        assert_eq!(ordered[1]["versionId"], 101);
    }

    #[test]
    fn lookup_without_installed_assets_lists_search_hits() {
        let search = vec![json!({ "id": 1, "name": "A", "versionId": 2 })];
        let ordered = order_lookup_assets(&[], &search, "a");
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0]["installed"], json!(false));
    }

    #[test]
    fn chat_cards_use_search_order_not_the_full_library() {
        let actions = vec![
            json!({
                "type": "list_loras",
                "status": "executed",
                "result": { "loras": [ { "name": "Unrelated", "path": "loras/unrelated.safetensors" } ] }
            }),
            json!({
                "type": "search_civitai",
                "status": "executed",
                "result": { "query": "eagle", "loras": [
                    { "name": "Eagle Wing Warrior", "path": "loras/eagle.safetensors", "installed": true, "versionId": 100 },
                    { "name": "Valkyrie", "installed": false, "versionId": 200 }
                ] }
            }),
        ];
        let cards = collect_chat_lookup_cards(&actions);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0]["name"], "Eagle Wing Warrior");
        assert_eq!(cards[1]["name"], "Valkyrie");
    }

    #[test]
    fn subject_query_keeps_the_named_subject() {
        assert_eq!(
            subject_query("what should I do for Frankie Foster?"),
            "Frankie Foster"
        );
        assert_eq!(
            subject_query("are you able to make an image of Frankie Foster?"),
            "Frankie Foster"
        );
        assert_eq!(
            subject_query("are you able make Frankie Foster"),
            "Frankie Foster"
        );
        assert_eq!(
            subject_query("Frankie Foster in an emerald dress under neon lighting"),
            "Frankie Foster"
        );
        assert_eq!(subject_query("Emerald Dress"), "Emerald Dress");
        let scene = subject_query("a woman in an emerald dress in a neon diner at night");
        assert_eq!(scene, "woman diner");
        assert!(!scene.to_lowercase().contains("emerald"));
        assert!(!scene.to_lowercase().contains("dress"));
        assert!(!scene.to_lowercase().contains("neon"));
    }

    #[test]
    fn scrub_drops_tool_names_and_keeps_the_reply() {
        let raw = "I can help with Frankie Foster. I see you're on Freya Route mode.\n\nsearch_civitai failed: civitai search";
        let cleaned = scrub_tool_noise(raw);
        assert!(cleaned.contains("Frankie Foster"));
        assert!(!cleaned.to_lowercase().contains("freya route"));
        assert!(!cleaned.contains("search_civitai"));
    }

    #[test]
    fn lora_in_ignores_filename_model_id() {
        let raw = json!({
            "name": "Frankie",
            "modelId": "Frankie_Foster_-_Foster_s_Home_for_Imaginary_Friends_-_Illustrious_XL_-_by_YeiyeiArt",
            "path": "loras/Frankie_Foster_-_Foster_s_Home_for_Imaginary_Friends_-_Illustrious_XL_-_by_YeiyeiArt.safetensors"
        });
        let l: LoraIn = serde_json::from_value(raw).expect("deserialize lora");
        assert!(l.model_id.is_none());
        assert_eq!(l.name, "Frankie");
    }

    #[test]
    fn lora_in_parses_numeric_model_id_string() {
        let raw = json!({ "name": "x", "modelId": "1319790" });
        let l: LoraIn = serde_json::from_value(raw).expect("deserialize lora");
        assert_eq!(l.model_id, Some(1319790));
    }

    #[test]
    fn vague_lookup_note_mentions_pin_lora_and_base() {
        let cards = vec![
            json!({ "name": "Foster LoRA", "path": "loras/foster.safetensors", "installed": true }),
            json!({ "name": "Other Foster", "installed": false }),
        ];
        let note = note_subject_lookup("Happy to help you choose.", "chat", &cards);
        let lower = note.to_lowercase();
        assert!(lower.contains("foster lora"));
        assert!(lower.contains("pin"));
        assert!(lower.contains("lora"));
        assert!(lower.contains("base"));
        assert!(!lower.contains("```"));
    }

    #[test]
    fn detailed_lookup_note_does_not_ask_a_question() {
        let cards = vec![json!({ "name": "Neon Diner", "installed": false })];
        let note = note_subject_lookup("On it — I’ll make that image for you.", "picture", &cards);
        assert!(note.contains("Agent tab"));
        assert!(note.contains("Neon Diner"));
        assert!(!note.contains('?'));
    }
}
