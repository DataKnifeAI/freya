mod civitai;
mod config;
mod gallery;
mod openrouter;
mod routes;
mod state;
mod styles;
mod worker;

use axum::extract::Request;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use config::Config;
use state::AppState;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::parse();
    let bind = cfg.bind.clone();
    let cors = build_cors(&cfg);
    let require_auth = !cfg.api_token.trim().is_empty();
    if !require_auth {
        warn!(
            "FREYA_API_TOKEN unset — mutating /v1 routes are open (local/dev only). \
             Set FREYA_API_TOKEN and optionally FREYA_CORS_ORIGINS for non-dev."
        );
    } else {
        info!("FREYA_API_TOKEN set — bearer auth required on mutating /v1 routes");
    }

    let state = AppState::from_config(cfg).await?;

    let protected = Router::new()
        .route("/v1/assist", post(routes::assist))
        .route("/v1/generate", post(routes::generate))
        .route("/v1/run", post(routes::run))
        .route("/v1/gallery", get(routes::gallery_list))
        .route("/v1/gallery/{id}", get(routes::gallery_get))
        .route("/v1/gallery/{id}/image", get(routes::gallery_image))
        .route("/v1/gallery/{id}/remix", post(routes::gallery_remix))
        .route("/v1/models", get(routes::models))
        .route("/v1/styles", get(routes::styles_list))
        .route("/v1/civitai/search", get(routes::civitai_search))
        .route("/v1/civitai/install", post(routes::civitai_install))
        .route(
            "/v1/settings",
            get(routes::settings_get).put(routes::settings_put),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_token,
        ));

    let app = Router::new()
        .route("/healthz", get(routes::health))
        .route("/v1/health", get(routes::health))
        .merge(protected)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "freya-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_cors(cfg: &Config) -> CorsLayer {
    let origins = cfg.cors_origins.trim();
    let has_token = !cfg.api_token.trim().is_empty();
    if !origins.is_empty() {
        let list: Vec<HeaderValue> = origins
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(list))
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
    } else if has_token {
        // Auth enabled, no explicit origins: do not use permissive `*`.
        CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
    } else {
        CorsLayer::permissive()
    }
}

/// When `FREYA_API_TOKEN` is set, require `Authorization: Bearer <token>` on mutating methods.
/// GET/HEAD/OPTIONS stay open so health + static UI reads work without a token on every image.
async fn require_api_token(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = req.method().clone();
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(next.run(req).await);
    }

    let expected = {
        let cfg = state.config.read().await;
        cfg.api_token.trim().to_string()
    };
    if expected.is_empty() {
        return Ok(next.run(req).await);
    }

    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let ok = auth
        .strip_prefix("Bearer ")
        .map(|t| t == expected)
        .unwrap_or(false);
    if !ok {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}
