//! Freya MCP server — Streamable HTTP at `/mcp`, proxying freya-api `/v1/*`.

mod api;
mod tools;

use anyhow::Context;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
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
    #[arg(long, env = "FREYA_API_URL", default_value = "http://127.0.0.1:8080")]
    api_url: String,

    /// Bearer token for freya-api (same Secret as Studio `apiTokenSecretRef`).
    /// Required. `/mcp` returns 401 when this is empty unless
    /// `--allow-unauthenticated` is set.
    #[arg(long, env = "FREYA_API_TOKEN", default_value = "")]
    api_token: String,

    /// Accept `/mcp` with no bearer. Off by default. Local dev only.
    #[arg(long, env = "FREYA_MCP_ALLOW_UNAUTHENTICATED", default_value_t = false)]
    allow_unauthenticated: bool,

    /// Comma-separated Host headers allowed by Streamable HTTP (DNS-rebinding guard).
    /// Empty or `*` means loopback only. It does not disable the check.
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
    let raw_hosts = args.allowed_hosts.trim();
    if raw_hosts.is_empty() || raw_hosts == "*" {
        info!(
            ?hosts,
            "MCP Host allowlist defaulted to loopback (empty or * does not disable it)"
        );
    } else {
        info!(?hosts, "MCP Host header allowlist");
    }
    config = config.with_allowed_hosts(hosts);

    let api_for_factory = api.clone();
    let mcp = StreamableHttpService::new(
        move || Ok(FreyaMcp::new(api_for_factory.clone())),
        LocalSessionManager::default().into(),
        config,
    );

    let mcp_auth = McpAuth {
        token: args.api_token.clone(),
        allow_unauthenticated: args.allow_unauthenticated,
    };
    let protected = Router::new()
        .nest_service("/mcp", mcp)
        .layer(middleware::from_fn_with_state(mcp_auth, require_mcp_token));
    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(protected);

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

#[derive(Clone)]
struct McpAuth {
    token: String,
    allow_unauthenticated: bool,
}

async fn require_mcp_token(
    State(auth): State<McpAuth>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if auth.token.is_empty() {
        if auth.allow_unauthenticated {
            tracing::warn!(
                "FREYA_MCP_ALLOW_UNAUTHENTICATED is set; /mcp is accepting unauthenticated calls"
            );
            return Ok(next.run(req).await);
        }
        return Err(StatusCode::UNAUTHORIZED);
    }
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    if presented == auth.token {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "freya-mcp" }))
}

fn loopback_hosts() -> Vec<String> {
    vec!["localhost".into(), "127.0.0.1".into(), "::1".into()]
}

fn parse_allowed_hosts(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "*" {
        return loopback_hosts();
    }
    let hosts: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "*")
        .map(str::to_string)
        .collect();
    if hosts.is_empty() {
        loopback_hosts()
    } else {
        hosts
    }
}

#[cfg(test)]
mod tests {
    use super::parse_allowed_hosts;

    #[test]
    fn empty_or_star_stays_on_loopback() {
        let loopback = || vec!["localhost".to_string(), "127.0.0.1".into(), "::1".into()];
        assert_eq!(parse_allowed_hosts(""), loopback());
        assert_eq!(parse_allowed_hosts("*"), loopback());
        assert_eq!(
            parse_allowed_hosts(" freya.example, localhost "),
            vec!["freya.example".to_string(), "localhost".into()]
        );
    }
}
