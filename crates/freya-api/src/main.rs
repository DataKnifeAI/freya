mod auto_matrix;
mod base_models;
mod civitai;
mod config;
mod disk;
mod gallery;
mod gpu;
mod k8s_secret;
mod openrouter;
mod prefs;
mod router;
mod routes;
mod state;
mod studio;
mod styles;
mod worker;
mod worker_ctl;

use axum::extract::Request;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use base64::Engine;
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
    let require_auth = cfg.auth_required();
    if !require_auth {
        warn!(
            "FREYA_API_TOKEN and FREYA_ADMIN_PASSWORD unset — /v1 routes are open (local/dev only). \
             Operator generateAdminSecrets (default) provisions admin login + API bearer in-cluster."
        );
    } else if cfg.admin_login_configured() {
        info!(
            "admin login configured — UI sign-in or bearer required on /v1 (except auth status/login + gallery images)"
        );
    } else {
        info!("FREYA_API_TOKEN set — bearer auth required on /v1 routes (except gallery images)");
    }

    let state = AppState::from_config(cfg).await?;

    let protected = Router::new()
        .route("/v1/assist", post(routes::assist))
        .route("/v1/chat", post(routes::chat))
        .route("/v1/generate", post(routes::generate))
        .route("/v1/series", post(routes::generate_series))
        .route("/v1/run", post(routes::run))
        .route("/v1/gallery", get(routes::gallery_list))
        .route(
            "/v1/gallery/{id}",
            get(routes::gallery_get).delete(routes::gallery_delete),
        )
        .route("/v1/gallery/{id}/image", get(routes::gallery_image))
        .route("/v1/gallery/{id}/remix", post(routes::gallery_remix))
        .route("/v1/models", get(routes::models))
        .route("/v1/styles", get(routes::styles_list))
        .route("/v1/civitai/search", get(routes::civitai_search))
        .route("/v1/civitai/lookup", get(routes::civitai_lookup))
        .route("/v1/civitai/models/{id}", get(routes::civitai_model))
        .route(
            "/v1/civitai/model-versions/{id}",
            get(routes::civitai_model_version),
        )
        .route("/v1/civitai/install", post(routes::civitai_install))
        .route("/v1/civitai/jobs", get(routes::civitai_jobs_list))
        .route("/v1/civitai/jobs/clear", post(routes::civitai_jobs_clear))
        .route("/v1/civitai/jobs/{id}", get(routes::civitai_job_get))
        .route(
            "/v1/civitai/jobs/{id}/cancel",
            post(routes::civitai_job_cancel),
        )
        .route(
            "/v1/settings",
            get(routes::settings_get).put(routes::settings_put),
        )
        .route(
            "/v1/studio",
            get(routes::studio_get).put(routes::studio_put),
        )
        .route("/v1/metrics/openrouter", get(routes::openrouter_metrics))
        .route("/v1/auth/token", post(routes::auth_token_create))
        .route("/v1/worker/stats", get(routes::worker_stats))
        .route("/v1/worker/start", post(routes::worker_start))
        .route("/v1/worker/stop", post(routes::worker_stop))
        .route("/v1/worker/installed", get(routes::worker_installed))
        .route("/v1/worker/loras", get(routes::worker_loras))
        .route("/v1/worker/checkpoints", get(routes::worker_checkpoints))
        .route("/v1/worker/embeddings", get(routes::worker_embeddings))
        .route("/v1/worker/vae", get(routes::worker_vaes))
        .route(
            "/v1/worker/assets",
            axum::routing::delete(routes::worker_asset_delete),
        )
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
        .route("/v1/auth/status", get(routes::auth_status))
        .route("/v1/auth/login", post(routes::auth_login))
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
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
    } else if has_token {
        // Auth enabled, no explicit origins: do not use permissive `*`.
        CorsLayer::new()
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
    } else {
        CorsLayer::permissive()
    }
}

/// When API bearer and/or admin password is set, require auth on `/v1` methods
/// except OPTIONS (CORS) and gallery image GETs (`<img src>` cannot send Authorization).
/// Accepts `Authorization: Bearer <api-token>` or HTTP Basic (`admin` + admin-password),
/// matching game-operator Server Manager basic auth as a fallback.
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

    let (api_token, admin_user, admin_password, auth_required) = {
        let cfg = state.config.read().await;
        (
            cfg.api_token.trim().to_string(),
            cfg.admin_user.trim().to_string(),
            cfg.admin_password.trim().to_string(),
            cfg.auth_required(),
        )
    };
    if !auth_required {
        return Ok(next.run(req).await);
    }

    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !api_token.is_empty() {
        if let Some(t) = auth.strip_prefix("Bearer ") {
            if const_eq(t.trim(), &api_token) {
                return Ok(next.run(req).await);
            }
        }
    }

    if !admin_password.is_empty() {
        if let Some((user, pass)) = parse_basic_auth(auth) {
            let expect_user = if admin_user.is_empty() {
                "admin"
            } else {
                admin_user.as_str()
            };
            if const_eq(&user, expect_user) && const_eq(&pass, &admin_password) {
                return Ok(next.run(req).await);
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

fn parse_basic_auth(header: &str) -> Option<(String, String)> {
    let encoded = header.strip_prefix("Basic ")?.trim();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let text = String::from_utf8(raw).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

fn const_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
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
