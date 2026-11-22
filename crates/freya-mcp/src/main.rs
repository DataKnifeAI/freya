//! Freya MCP server — Streamable HTTP at `/mcp`, proxying freya-api `/v1/*`.

mod api;
mod tools;

use anyhow::Context;
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::api::FreyaApiClient;
use crate::tools::FreyaMcp;

#[derive(Debug, Parser)]
#[command(name = "freya-mcp", about = "Freya MCP Streamable HTTP server")]
struct Args {
    /// Listen address (host:port).
    #[arg(long, env = "FREYA_MCP_BIND", default_value = "0.0.0.0:8090")]
    bind: String,

    /// In-cluster Freya API base URL (no trailing slash).
    #[arg(
        long,
        env = "FREYA_API_URL",
        default_value = "http://127.0.0.1:8080"
    )]
    api_url: String,

    /// Bearer token for freya-api (same Secret as Studio `apiTokenSecretRef`).
    #[arg(long, env = "FREYA_API_TOKEN", default_value = "")]
    api_token: String,

    /// Comma-separated Host headers allowed by Streamable HTTP (DNS-rebinding guard).
    /// Empty or `*` disables the check (typical behind Ingress). Local default: localhost.
    #[arg(long, env = "FREYA_MCP_ALLOWED_HOSTS", default_value = "")]
    allowed_hosts: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,freya_mcp=debug".into()),
        )
        .init();

    let args = Args::parse();
    let api = FreyaApiClient::new(&args.api_url, &args.api_token)?;
    let api = Arc::new(api);

    let ct = CancellationToken::new();
    let mut config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_cancellation_token(ct.child_token());
    let hosts = parse_allowed_hosts(&args.allowed_hosts);
    if hosts.is_empty() {
        // Behind Ingress / in-cluster Service DNS — Host is already gated by the mesh.
        config = config.disable_allowed_hosts();
        info!("MCP Host header allowlist disabled (FREYA_MCP_ALLOWED_HOSTS empty or *)");
    } else {
        info!(?hosts, "MCP Host header allowlist");
        config = config.with_allowed_hosts(hosts);
    }

    let api_for_factory = api.clone();
    let mcp = StreamableHttpService::new(
        move || Ok(FreyaMcp::new(api_for_factory.clone())),
        LocalSessionManager::default().into(),
        config,
    );

    let app = Router::new()
        .route("/healthz", get(healthz))
        .nest_service("/mcp", mcp);

    let addr: SocketAddr = args
        .bind
        .parse()
        .with_context(|| format!("invalid FREYA_MCP_BIND / --bind: {}", args.bind))?;
    info!(
        %addr,
        api = %args.api_url,
        token_configured = !args.api_token.is_empty(),
        "freya-mcp listening (Streamable HTTP at /mcp)"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "freya-mcp" }))
}

fn parse_allowed_hosts(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "*" {
        return Vec::new();
    }
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
