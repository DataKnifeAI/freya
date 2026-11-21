use crate::civitai::CivitaiClient;
use crate::config::Config;
use crate::gallery::Gallery;
use crate::openrouter::OpenRouterClient;
use crate::worker::DiffusionWorkerClient;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub gallery: Gallery,
    pub packs_dir: PathBuf,
    pub openrouter: Arc<RwLock<OpenRouterClient>>,
    pub civitai: Arc<RwLock<CivitaiClient>>,
    pub worker: Arc<RwLock<Option<DiffusionWorkerClient>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSecrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub civitai_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hf_token: Option<String>,
    /// Persisted worker URL so Settings survive API restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffusion_worker_url: Option<String>,
    /// When false, API skips local gens / treats worker as unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_enabled: Option<bool>,
}

impl AppState {
    pub async fn from_config(cfg: Config) -> anyhow::Result<Self> {
        let gallery = Gallery::open(cfg.gallery_dir.clone()).await?;
        let openrouter = OpenRouterClient::new(&cfg);
        let civitai = CivitaiClient::new(cfg.civitai_api_key.clone());
        let worker = if cfg.worker_enabled {
            cfg.diffusion_worker_url.as_ref().map(|u| {
                DiffusionWorkerClient::new_with_tls(u.clone(), cfg.worker_tls_insecure)
                    .with_token(cfg.worker_token.clone())
            })
        } else {
            None
        };
        let packs_dir = cfg.packs_dir.clone();
        let state = Self {
            config: Arc::new(RwLock::new(cfg)),
            gallery,
            packs_dir,
            openrouter: Arc::new(RwLock::new(openrouter)),
            civitai: Arc::new(RwLock::new(civitai)),
            worker: Arc::new(RwLock::new(worker)),
        };
        state.load_secrets_file().await?;
        Ok(state)
    }

    async fn load_secrets_file(&self) -> anyhow::Result<()> {
        let path = {
            let cfg = self.config.read().await;
            cfg.secrets_file.clone()
        };
        if !path.exists() {
            return Ok(());
        }
        let raw = tokio::fs::read(&path).await?;
        let secrets: RuntimeSecrets = serde_json::from_slice(&raw).unwrap_or_default();
        self.apply_secrets(secrets).await;
        Ok(())
    }

    pub async fn apply_secrets(&self, secrets: RuntimeSecrets) {
        let mut cfg = self.config.write().await;
        if let Some(k) = secrets.openrouter_api_key {
            if !k.is_empty() {
                cfg.openrouter_api_key = k.clone();
                let mut or = self.openrouter.write().await;
                *or = OpenRouterClient::new(&cfg).with_key(k);
            }
        }
        if let Some(k) = secrets.civitai_api_key {
            if !k.is_empty() {
                cfg.civitai_api_key = k.clone();
                let mut civ = self.civitai.write().await;
                *civ = CivitaiClient::new(k);
            }
        }
        if let Some(k) = secrets.hf_token {
            if !k.is_empty() {
                cfg.hf_token = k;
            }
        }
        if let Some(url) = secrets.diffusion_worker_url {
            if url.trim().is_empty() {
                cfg.diffusion_worker_url = None;
            } else {
                cfg.diffusion_worker_url = Some(url);
            }
        }
        if let Some(enabled) = secrets.worker_enabled {
            cfg.worker_enabled = enabled;
        }
        drop(cfg);
        self.rebuild_worker_client().await;
    }

    /// Attach or clear the in-memory worker client from URL + enabled flag.
    pub async fn rebuild_worker_client(&self) {
        let cfg = self.config.read().await;
        let next = if cfg.worker_enabled {
            cfg.diffusion_worker_url.as_ref().map(|u| {
                DiffusionWorkerClient::new_with_tls(u.clone(), cfg.worker_tls_insecure)
                    .with_token(cfg.worker_token.clone())
            })
        } else {
            None
        };
        *self.worker.write().await = next;
    }

    pub async fn persist_secrets(&self) -> anyhow::Result<()> {
        let cfg = self.config.read().await;
        if let Some(parent) = cfg.secrets_file.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let secrets = RuntimeSecrets {
            openrouter_api_key: Some(cfg.openrouter_api_key.clone()),
            civitai_api_key: Some(cfg.civitai_api_key.clone()),
            hf_token: Some(cfg.hf_token.clone()),
            diffusion_worker_url: cfg.diffusion_worker_url.clone(),
            worker_enabled: Some(cfg.worker_enabled),
        };
        let raw = serde_json::to_vec_pretty(&secrets)?;
        tokio::fs::write(&cfg.secrets_file, raw).await?;
        Ok(())
    }
}
