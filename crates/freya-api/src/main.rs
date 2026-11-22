mod base_models;
mod civitai;
mod config;
mod gallery;
mod gpu;
mod openrouter;
mod routes;
mod state;
mod styles;
mod worker;
mod worker_ctl;

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
            "FREYA_API_TOKEN unset — /v1 routes are open (local/dev only). \
             Set FREYA_API_TOKEN (and FreyaStudio apiTokenSecretRef in-cluster) for non-dev."
        );
    } else {
        info!("FREYA_API_TOKEN set — bearer auth required on /v1 routes (except gallery images)");
    }

    let state = AppState::from_config(cfg).await?;

    let protected = Router::new()
        .route("/v1/assist", post(routes::assist))
        .route("/v1/chat", post(routes::chat))
        .route("/v1/generate", post(routes::generate))
        .route("/v1/run", post(routes::run))
        .route("/v1/gallery", get(routes::gallery_list))
        .route("/v1/gallery/{id}", get(routes::gallery_get))
        .route("/v1/gallery/{id}/image", get(routes::gallery_image))
        .route("/v1/gallery/{id}/remix", post(routes::gallery_remix))
        .route("/v1/models", get(routes::models))
        .route("/v1/styles", get(routes::styles_list))
        .route("/v1/civitai/search", get(routes::civitai_search))
        .route("/v1/civitai/models/{id}", get(routes::civitai_model))
        .route("/v1/civitai/install", post(routes::civitai_install))
        .route(
            "/v1/settings",
            get(routes::settings_get).put(routes::settings_put),
        )
        .route("/v1/auth/token", post(routes::auth_token_create))
        .route("/v1/worker/stats", get(routes::worker_stats))
        .route("/v1/worker/start", post(routes::worker_start))
        .route("/v1/worker/stop", post(routes::worker_stop))
        .route("/v1/worker/enable", post(routes::worker_enable))
        .route("/v1/worker/disable", post(routes::worker_disable))
        .route("/v1/worker/checkpoints", get(routes::worker_checkpoints))
        .route(
            "/v1/worker/base-model",
            get(routes::worker_base_model_get).put(routes::worker_base_model_put),
        )
        .route("/v1/base-models", get(routes::base_models_catalog))
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

/// When `FREYA_API_TOKEN` is set, require `Authorization: Bearer <token>` on all `/v1` methods
/// except OPTIONS (CORS) and gallery image GETs (`<img src>` cannot send Authorization).
async fn require_api_token(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = req.method().clone();
    if method == Method::OPTIONS {
        return Ok(next.run(req).await);
    }
    // UUID gallery ids are unguessable; list/detail/search still require the bearer.
    if matches!(method, Method::GET | Method::HEAD) && is_gallery_image_path(req.uri().path()) {
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

fn is_gallery_image_path(path: &str) -> bool {
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    matches!(
        parts.as_slice(),
        ["", "v1", "gallery", id, "image"] if !id.is_empty()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_image_path_match() {
        assert!(is_gallery_image_path("/v1/gallery/abc-123/image"));
        assert!(is_gallery_image_path("/v1/gallery/abc-123/image/"));
        assert!(!is_gallery_image_path("/v1/gallery"));
        assert!(!is_gallery_image_path("/v1/gallery/abc-123"));
        assert!(!is_gallery_image_path("/v1/civitai/search"));
    }
}
