//! Freya operator — reconciles FreyaStudio, FreyaGeneration, FreyaDiffusionWorker, FreyaModelAsset.

mod reconcile;

use anyhow::Context;
use freya_crd::{
    FreyaDiffusionWorker, FreyaGeneration, FreyaModelAsset, FreyaStudio, API_GROUP, API_VERSION,
};
use futures::StreamExt;
use kube::runtime::controller::{Action, Controller};
use kube::runtime::watcher::Config;
use kube::{Api, Client};
use std::sync::Arc;
use tracing::{error, info};

#[derive(Clone)]
pub struct Ctx {
    pub client: Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!(
        group = API_GROUP,
        version = API_VERSION,
        "freya-operator starting"
    );

    let client = Client::try_default()
        .await
        .context("create kube client (in-cluster or KUBECONFIG)")?;
    let ctx = Arc::new(Ctx {
        client: client.clone(),
    });

    let studios = Api::<FreyaStudio>::all(client.clone());
    let generations = Api::<FreyaGeneration>::all(client.clone());
    let workers = Api::<FreyaDiffusionWorker>::all(client.clone());
    let assets = Api::<FreyaModelAsset>::all(client.clone());

    let studio_ctrl = Controller::new(studios, Config::default())
        .run(
            reconcile::studio::reconcile,
            reconcile::studio::error_policy,
            ctx.clone(),
        )
        .for_each(|res| async move {
            match res {
                Ok((obj, _)) => info!(name = %obj.name, "studio reconcile ok"),
                Err(e) => error!(error = %e, "studio reconcile error"),
            }
        });

    let gen_ctrl = Controller::new(generations, Config::default())
        .run(
            reconcile::generation::reconcile,
            reconcile::generation::error_policy,
            ctx.clone(),
        )
        .for_each(|res| async move {
            match res {
                Ok((obj, _)) => info!(name = %obj.name, "generation reconcile ok"),
                Err(e) => error!(error = %e, "generation reconcile error"),
            }
        });

    let worker_ctrl = Controller::new(workers, Config::default())
        .run(
            reconcile::worker::reconcile,
            reconcile::worker::error_policy,
            ctx.clone(),
        )
        .for_each(|res| async move {
            match res {
                Ok((obj, _)) => info!(name = %obj.name, "worker reconcile ok"),
                Err(e) => error!(error = %e, "worker reconcile error"),
            }
        });

    let asset_ctrl = Controller::new(assets, Config::default())
        .run(
            reconcile::asset::reconcile,
            reconcile::asset::error_policy,
            ctx.clone(),
        )
        .for_each(|res| async move {
            match res {
                Ok((obj, _)) => info!(name = %obj.name, "asset reconcile ok"),
                Err(e) => error!(error = %e, "asset reconcile error"),
            }
        });

    tokio::select! {
        _ = studio_ctrl => {},
        _ = gen_ctrl => {},
        _ = worker_ctrl => {},
        _ = asset_ctrl => {},
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown signal received");
        }
    }

    Ok(())
}

#[allow(dead_code)]
fn _action_requeue() -> Action {
    Action::requeue(std::time::Duration::from_secs(60))
}
