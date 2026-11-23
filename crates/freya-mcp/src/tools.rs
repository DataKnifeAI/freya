//! MCP tools wrapping freya-api `/v1/*` routes.

use crate::api::FreyaApiClient;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn json_ok(value: Value) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn map_err(err: impl std::fmt::Display) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

#[derive(Clone)]
pub struct FreyaMcp {
    api: Arc<FreyaApiClient>,
    #[allow(dead_code)] // read by #[tool_handler] macro
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateArgs {
    /// Image prompt (natural language).
    pub prompt: String,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    /// `auto` | `on` | `off` | `prompt-prose` | `prompt-sd`
    #[serde(default)]
    pub enhance: Option<String>,
    /// `openrouter` | `local` | `auto`
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub style_ref: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GalleryGetArgs {
    /// Artifact UUID from gallery_list / generate_image.
    pub id: String,
}

/// One Civitai model type. Call `civitai_search` again to search a different type.
/// `Model` is not a Civitai type (use `Checkpoint` for a base). Embedding is `TextualInversion`.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
pub enum CivitaiAssetType {
    #[serde(rename = "LORA", alias = "lora", alias = "LoRA", alias = "Lora")]
    #[schemars(rename = "LORA")]
    Lora,
    #[serde(rename = "LoCon", alias = "locon", alias = "LoCON")]
    #[schemars(rename = "LoCon")]
    LoCon,
    #[serde(rename = "DoRA", alias = "dora", alias = "DORA")]
    #[schemars(rename = "DoRA")]
    DoRA,
    #[serde(
        rename = "LyCORIS",
        alias = "lycoris",
        alias = "Lycoris",
        alias = "lyco"
    )]
    #[schemars(rename = "LyCORIS")]
    LyCORIS,
    #[serde(rename = "Checkpoint", alias = "checkpoint", alias = "checkpoints")]
    #[schemars(rename = "Checkpoint")]
    Checkpoint,
    #[serde(
        rename = "TextualInversion",
        alias = "textualinversion",
        alias = "embedding",
        alias = "embeddings",
        alias = "Embedding"
    )]
    #[schemars(rename = "TextualInversion")]
    TextualInversion,
    #[serde(rename = "VAE", alias = "vae", alias = "vaes")]
    #[schemars(rename = "VAE")]
    Vae,
}

impl CivitaiAssetType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lora => "LORA",
            Self::LoCon => "LoCon",
            Self::DoRA => "DoRA",
            Self::LyCORIS => "LyCORIS",
            Self::Checkpoint => "Checkpoint",
            Self::TextualInversion => "TextualInversion",
            Self::Vae => "VAE",
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiSearchArgs {
    /// Search query.
    pub q: String,
    /// Required. Exactly one of `LORA`, `LoCon`, `DoRA`, `LyCORIS`, `Checkpoint`, `TextualInversion`, `VAE`.
    /// Embedding is `TextualInversion`. Model is not a Civitai type.
    /// One call searches that type only; call again for a different type. Does not pin or download.
    #[serde(rename = "type")]
    pub asset_type: CivitaiAssetType,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetBaseModelArgs {
    /// Worker checkpoint path, e.g. `checkpoints/foo.safetensors`.
    pub model: String,
    /// Optional Civitai-style base family label (SDXL 1.0, Pony, …).
    #[serde(default)]
    pub base_model: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VideoArgs {
    pub prompt: String,
    /// `gif` or `video`.
    #[serde(default)]
    pub modality: Option<String>,
    #[serde(default)]
    pub duration: Option<i64>,
    /// `openrouter` | `local` | `cloud` | `auto`
    #[serde(default)]
    pub backend: Option<String>,
    /// Model id only, never an API key.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub style_ref: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesArgs {
    pub goal: String,
    #[serde(default)]
    pub style_ref: Option<String>,
    /// `openrouter` | `local` | `cloud` | `auto`
    #[serde(default)]
    pub backend: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstallArgs {
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub version_id: Option<i64>,
    #[serde(default)]
    pub file_id: Option<i64>,
    pub dest_path: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_model: Option<String>,
    #[serde(default)]
    pub model_type: Option<String>,
    #[serde(default)]
    pub model_id: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobIdArgs {
    pub job_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClearJobsArgs {
    /// Also drop idle queued jobs. Active downloads stay.
    #[serde(default)]
    pub include_queued: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssetPathArgs {
    /// Worker-relative path such as `loras/foo.safetensors`.
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssetCardArgs {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    /// `LORA`, `LoCon`, `TextualInversion`, … Checkpoints cannot be pinned.
    #[serde(default)]
    pub model_type: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub version_id: Option<i64>,
    #[serde(default)]
    pub model_id: Option<i64>,
    #[serde(default)]
    pub base_model: Option<String>,
    #[serde(default)]
    pub trigger_word: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CostTierArgs {
    /// `low` | `medium` | `high` | `max`
    pub cost_tier: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyArgs {
    #[serde(default)]
    pub zero_data_retention: Option<bool>,
    /// `allow` or `deny`
    #[serde(default)]
    pub data_collection: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlignmentArgs {
    /// `balanced` | `safety` | `uncensored`
    #[serde(default)]
    pub model_alignment: Option<String>,
    /// Show NSFW local / Civitai assets.
    #[serde(default)]
    pub allow_nsfw_local: Option<bool>,
    /// `warm` | `direct` | `steerable`
    #[serde(default)]
    pub instruction_tone: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackendArgs {
    /// `cloud` or `local`
    pub backend: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModeArgs {
    /// `plan` | `auto` | `yolo`
    pub mode: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelPinArgs {
    /// Agent/chat model id, or `auto`.
    #[serde(default)]
    pub chat_model: Option<String>,
    #[serde(default)]
    pub image_model: Option<String>,
    #[serde(default)]
    pub router_model: Option<String>,
    #[serde(default)]
    pub video_model: Option<String>,
}

fn card_key(card: &Value) -> String {
    if let Some(p) = card.get("path").and_then(|p| p.as_str()) {
        let p = p.trim();
        if !p.is_empty() {
            return format!("path:{p}");
        }
    }
    if let Some(s) = card.get("id").and_then(|i| i.as_str()) {
        let s = s.trim();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    if let Some(n) = card.get("id").and_then(|i| i.as_i64()) {
        return n.to_string();
    }
    card.get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn same_name(stored: &Value, query: &Value) -> bool {
    let a = stored.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let b = query.get("name").and_then(|n| n.as_str()).unwrap_or("");
    !a.is_empty() && a.eq_ignore_ascii_case(b)
}

fn asset_card_json(args: &AssetCardArgs) -> Value {
    json!({
        "id": args.id,
        "name": args.name,
        "modelType": args.model_type,
        "path": args.path,
        "downloadUrl": args.download_url,
        "versionId": args.version_id,
        "modelId": args.model_id,
        "baseModel": args.base_model,
        "triggerWord": args.trigger_word,
    })
}

#[tool_router]
impl FreyaMcp {
    pub fn new(api: Arc<FreyaApiClient>) -> Self {
        Self {
            api,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Freya API health summary (OpenRouter / Civitai / worker flags)")]
    async fn health(&self) -> Result<CallToolResult, McpError> {
        let v = self.api.get_json("/v1/health").await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Freya settings summary (models, flags; never returns secret values)")]
    async fn settings(&self) -> Result<CallToolResult, McpError> {
        let v = self.api.get_json("/v1/settings").await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(
        description = "Generate an image via Freya (OpenRouter by default, or local worker when backend=local)"
    )]
    async fn generate_image(
        &self,
        Parameters(args): Parameters<GenerateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let body = json!({
            "prompt": args.prompt,
            "negativePrompt": args.negative_prompt,
            "enhance": args.enhance,
            "backend": args.backend,
            "model": args.model,
            "styleRef": args.style_ref,
            "size": args.size,
            "seed": args.seed,
        });
        let v = self
            .api
            .post_json("/v1/generate", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "List Freya gallery artifacts (newest first)")]
    async fn gallery_list(&self) -> Result<CallToolResult, McpError> {
        let v = self.api.get_json("/v1/gallery").await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Get one Freya gallery artifact metadata by id")]
    async fn gallery_get(
        &self,
        Parameters(args): Parameters<GalleryGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = crate::api::bind_path_segment("/v1/gallery", &args.id, "").map_err(map_err)?;
        let v = self.api.get_json(&path).await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(
        description = "Search one Civitai asset type. Required type is exactly one of LORA, LoCon, DoRA, LyCORIS, Checkpoint, TextualInversion (embedding), VAE. Call again to search a different type. Model is not a Civitai type (use Checkpoint for a base). Installed matches come first, then other Civitai hits. Does not pin or download."
    )]
    async fn civitai_search(
        &self,
        Parameters(args): Parameters<CivitaiSearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut q = vec![
            ("q".into(), args.q),
            ("type".into(), args.asset_type.as_str().to_string()),
        ];
        if let Some(limit) = args.limit {
            q.push(("limit".into(), limit.to_string()));
        }
        let v = self
            .api
            .get_json_query("/v1/civitai/lookup", &q)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Diffusion worker stats/status via Freya API")]
    async fn worker_stats(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .get_json("/v1/worker/stats")
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "List local checkpoints on the diffusion worker")]
    async fn worker_checkpoints(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .get_json("/v1/worker/checkpoints")
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Read the worker's active base checkpoint / base model")]
    async fn worker_get_base_model(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .get_json("/v1/worker/base-model")
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Set the worker's active base checkpoint (Worker tab)")]
    async fn worker_set_base_model(
        &self,
        Parameters(args): Parameters<SetBaseModelArgs>,
    ) -> Result<CallToolResult, McpError> {
        let body = json!({
            "model": args.model,
            "baseModel": args.base_model,
        });
        let v = self
            .api
            .put_json("/v1/worker/base-model", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "List available Freya models (OpenRouter + local discovery)")]
    async fn models(&self) -> Result<CallToolResult, McpError> {
        let v = self.api.get_json("/v1/models").await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(
        description = "List Freya style packs (id / displayName / tags); pass styleRef to generate_image)"
    )]
    async fn list_styles(&self) -> Result<CallToolResult, McpError> {
        let v = self.api.get_json("/v1/styles").await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(
        description = "Generate a gif or video (modality gif|video). Model id only, never a key."
    )]
    async fn generate_video(
        &self,
        Parameters(args): Parameters<VideoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let body = json!({
            "prompt": args.prompt,
            "modality": args.modality.unwrap_or_else(|| "video".into()),
            "duration": args.duration,
            "backend": args.backend,
            "model": args.model,
            "styleRef": args.style_ref,
        });
        let v = self
            .api
            .post_json("/v1/generate", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Generate a short series of stills for one goal")]
    async fn generate_series(
        &self,
        Parameters(args): Parameters<SeriesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let body = json!({
            "goal": args.goal,
            "styleRef": args.style_ref,
            "backend": args.backend,
        });
        let v = self
            .api
            .post_json("/v1/series", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Enqueue a Civitai install (downloadUrl or versionId, plus destPath)")]
    async fn civitai_install(
        &self,
        Parameters(args): Parameters<InstallArgs>,
    ) -> Result<CallToolResult, McpError> {
        let body = json!({
            "downloadUrl": args.download_url.unwrap_or_default(),
            "versionId": args.version_id,
            "fileId": args.file_id,
            "destPath": args.dest_path,
            "name": args.name,
            "baseModel": args.base_model,
            "modelType": args.model_type,
            "modelId": args.model_id,
        });
        let v = self
            .api
            .post_json("/v1/civitai/install", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "List Civitai download jobs on the worker")]
    async fn civitai_jobs(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .get_json("/v1/civitai/jobs")
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Cancel one queued or active Civitai download by jobId")]
    async fn civitai_cancel_job(
        &self,
        Parameters(args): Parameters<JobIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = crate::api::bind_path_segment("/v1/civitai/jobs", &args.job_id, "/cancel")
            .map_err(map_err)?;
        let v = self
            .api
            .post_json(&path, &json!({}))
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Clear finished Civitai jobs. includeQueued also drops idle queued jobs.")]
    async fn civitai_clear_jobs(
        &self,
        Parameters(args): Parameters<ClearJobsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let body = json!({ "includeQueued": args.include_queued.unwrap_or(false) });
        let v = self
            .api
            .post_json("/v1/civitai/jobs/clear", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "List installed LoRAs, checkpoints, embeddings, and VAEs")]
    async fn list_installed(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .get_json("/v1/worker/installed")
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Delete one installed worker file by relative path")]
    async fn delete_installed(
        &self,
        Parameters(args): Parameters<AssetPathArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .delete_json("/v1/worker/assets", &json!({ "path": args.path }))
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Delete one gallery generation by id when the user asked to remove it")]
    async fn gallery_delete(
        &self,
        Parameters(args): Parameters<GalleryGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = crate::api::bind_path_segment("/v1/gallery", &args.id, "").map_err(map_err)?;
        let v = self
            .api
            .delete_json(&path, &json!({}))
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(
        description = "Return the gallery file URL and filename the UI download button uses. Does not return image bytes."
    )]
    async fn gallery_download(
        &self,
        Parameters(args): Parameters<GalleryGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = crate::api::bind_path_segment("/v1/gallery", &args.id, "").map_err(map_err)?;
        let image =
            crate::api::bind_path_segment("/v1/gallery", &args.id, "/image").map_err(map_err)?;
        let mut v = self.api.get_json(&path).await.map_err(map_err)?;
        if let Some(obj) = v.as_object_mut() {
            obj.entry("url".to_string()).or_insert_with(|| json!(image));
            obj.insert(
                "note".into(),
                json!("The Studio download button saves this file. Bytes are not in this tool result."),
            );
        }
        json_ok(v)
    }

    #[tool(
        description = "Read Studio favorites, LoRA pins, backend, video pin, and mode. No secrets."
    )]
    async fn studio(&self) -> Result<CallToolResult, McpError> {
        let v = self.api.get_json("/v1/studio").await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Favorite a Civitai or installed asset card (not a secret)")]
    async fn favorite_asset(
        &self,
        Parameters(args): Parameters<AssetCardArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.merge_studio_list("favorites", &asset_card_json(&args), true)
            .await
    }

    #[tool(description = "Remove an asset from favorites by id, path, or name")]
    async fn unfavorite_asset(
        &self,
        Parameters(args): Parameters<AssetCardArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.merge_studio_list("favorites", &asset_card_json(&args), false)
            .await
    }

    #[tool(description = "Pin a LoRA or embedding for the session. Checkpoints are rejected.")]
    async fn pin_lora(
        &self,
        Parameters(args): Parameters<AssetCardArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.merge_studio_list("pinnedLoras", &asset_card_json(&args), true)
            .await
    }

    #[tool(description = "Unpin a session LoRA by id, path, or name")]
    async fn unpin_lora(
        &self,
        Parameters(args): Parameters<AssetCardArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.merge_studio_list("pinnedLoras", &asset_card_json(&args), false)
            .await
    }

    #[tool(description = "Set Auto cost tier: low, medium, high, or max")]
    async fn set_cost_tier(
        &self,
        Parameters(args): Parameters<CostTierArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut st = self.api.get_json("/v1/settings").await.map_err(map_err)?;
        let fallbacks = st
            .get_mut("autoFallbacks")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| map_err("settings missing autoFallbacks"))?;
        fallbacks.insert("costTier".into(), json!(args.cost_tier));
        let body =
            json!({ "autoFallbacks": st.get("autoFallbacks").cloned().unwrap_or(json!({})) });
        let v = self
            .api
            .put_json("/v1/settings", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(
        description = "Set privacy toggles: zeroDataRetention and/or dataCollection allow|deny. Not API keys."
    )]
    async fn set_privacy(
        &self,
        Parameters(args): Parameters<PrivacyArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut st = self.api.get_json("/v1/settings").await.map_err(map_err)?;
        let privacy = st
            .get_mut("privacy")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| map_err("settings missing privacy"))?;
        if let Some(z) = args.zero_data_retention {
            privacy.insert("zeroDataRetention".into(), json!(z));
        }
        if let Some(dc) = args.data_collection {
            privacy.insert("dataCollection".into(), json!(dc));
        }
        let body = json!({ "privacy": st.get("privacy").cloned().unwrap_or(json!({})) });
        let v = self
            .api
            .put_json("/v1/settings", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Set alignment, Show NSFW (allowNsfwLocal), and/or instruction tone")]
    async fn set_alignment(
        &self,
        Parameters(args): Parameters<AlignmentArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut st = self.api.get_json("/v1/settings").await.map_err(map_err)?;
        let alignment = st
            .get_mut("alignment")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| map_err("settings missing alignment"))?;
        if let Some(a) = args.model_alignment {
            alignment.insert("modelAlignment".into(), json!(a));
        }
        if let Some(n) = args.allow_nsfw_local {
            alignment.insert("allowNsfwLocal".into(), json!(n));
        }
        if let Some(t) = args.instruction_tone {
            alignment.insert("instructionTone".into(), json!(t));
        }
        let body = json!({ "alignment": st.get("alignment").cloned().unwrap_or(json!({})) });
        let v = self
            .api
            .put_json("/v1/settings", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Set the composer backend to cloud or local")]
    async fn set_generate_backend(
        &self,
        Parameters(args): Parameters<BackendArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .put_json("/v1/studio", &json!({ "generateBackend": args.backend }))
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Set Plan / Auto / YOLO")]
    async fn set_automation_mode(
        &self,
        Parameters(args): Parameters<ModeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .put_json("/v1/studio", &json!({ "automationMode": args.mode }))
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Pin agent, router, image, and/or video model ids. Never pass API keys.")]
    async fn set_model_pins(
        &self,
        Parameters(args): Parameters<ModelPinArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut body = serde_json::Map::new();
        if let Some(m) = args.chat_model {
            body.insert("chatModel".into(), json!(m));
        }
        if let Some(m) = args.image_model {
            body.insert("imageModel".into(), json!(m));
        }
        if let Some(m) = args.router_model {
            body.insert("routerModel".into(), json!(m));
        }
        let mut out = json!({});
        if !body.is_empty() {
            out = self
                .api
                .put_json("/v1/settings", &Value::Object(body))
                .await
                .map_err(map_err)?;
        }
        if let Some(m) = args.video_model {
            let studio = self
                .api
                .put_json("/v1/studio", &json!({ "videoModel": m }))
                .await
                .map_err(map_err)?;
            if let Some(obj) = out.as_object_mut() {
                obj.insert("studio".into(), studio);
            } else {
                out = studio;
            }
        }
        json_ok(out)
    }

    #[tool(description = "Start the local diffusion worker container or unit")]
    async fn worker_start(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .post_json("/v1/worker/start", &json!({}))
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Stop the local diffusion worker container or unit")]
    async fn worker_stop(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .post_json("/v1/worker/stop", &json!({}))
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    async fn merge_studio_list(
        &self,
        field: &str,
        card: &Value,
        add: bool,
    ) -> Result<CallToolResult, McpError> {
        let studio = self.api.get_json("/v1/studio").await.map_err(map_err)?;
        let mut list = studio
            .get(field)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let key = card_key(card);
        list.retain(|item| card_key(item) != key && !same_name(item, card));
        if add {
            list.insert(0, card.clone());
        }
        let body = json!({ field: list });
        let v = self
            .api
            .put_json("/v1/studio", &body)
            .await
            .map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "OpenRouter key spend / credits snapshot (GET /v1/metrics/openrouter)")]
    async fn openrouter_metrics(&self) -> Result<CallToolResult, McpError> {
        let v = self
            .api
            .get_json("/v1/metrics/openrouter")
            .await
            .map_err(map_err)?;
        json_ok(v)
    }
}

#[tool_handler]
impl ServerHandler for FreyaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new("freya-mcp", env!("CARGO_PKG_VERSION")))
        .with_protocol_version(ProtocolVersion::V_2025_03_26)
        .with_instructions(
            "Freya image studio MCP. Tools proxy freya-api for generate, gallery, Civitai \
             install/jobs, installed assets, favorites, LoRA pins, cost/privacy/alignment, \
             cloud vs local, and model id pins. Never read or set API keys, tokens, or passwords. \
             gallery_download returns the URL the UI saves; it does not return image bytes. \
             Pin LoRAs only — checkpoints use worker_set_base_model. Do not change the worker host URL."
                .to_string(),
        )
    }
}
