//! MCP tools wrapping freya-api `/v1/*` routes.

use crate::api::FreyaApiClient;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities,
    ServerConfig,
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiSearchArgs {
    /// Search query.
    pub q: String,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Comma-separated types, e.g. `LORA,Checkpoint`.
    #[serde(default)]
    pub types: Option<String>,
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
        let path = format!("/v1/gallery/{}", args.id);
        let v = self.api.get_json(&path).await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(description = "Search Civitai models/LoRAs via Freya API")]
    async fn civitai_search(
        &self,
        Parameters(args): Parameters<CivitaiSearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut q = vec![("q".into(), args.q)];
        if let Some(limit) = args.limit {
            q.push(("limit".into(), limit.to_string()));
        }
        if let Some(types) = args.types {
            q.push(("types".into(), types));
        }
        let v = self
            .api
            .get_json_query("/v1/civitai/search", &q)
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

    #[tool(description = "List Freya style packs (id / displayName / tags); pass styleRef to generate_image)")]
    async fn list_styles(&self) -> Result<CallToolResult, McpError> {
        let v = self.api.get_json("/v1/styles").await.map_err(map_err)?;
        json_ok(v)
    }

    #[tool(
        description = "OpenRouter key spend / credits snapshot (GET /v1/metrics/openrouter)"
    )]
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
            "Freya image studio MCP. Tools proxy freya-api: health, settings, generate_image, \
             gallery_list, gallery_get, civitai_search, worker_stats, worker_checkpoints, \
             worker_get_base_model, worker_set_base_model, models, list_styles, \
             openrouter_metrics. Prefer OpenRouter generate_image (backend openrouter) \
             unless the user wants local GPU (backend=local) with a configured reachable \
             worker. Pass styleRef from list_styles when applying a pack. Worker base tools \
             match the Studio Worker tab (not host URL). There is no enable/disable worker tool."
                .to_string(),
        )
    }
}
