mod civitai;
mod config;
mod gallery;
mod openrouter;
mod routes;
mod state;
mod styles;
mod worker;

use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use config::Config;
use state::AppState;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

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
    let state = AppState::from_config(cfg).await?;

    let app = Router::new()
        .route("/healthz", get(routes::health))
        .route("/v1/health", get(routes::health))
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
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "freya-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}
