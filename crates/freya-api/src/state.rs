use crate::civitai::CivitaiClient;
use crate::config::Config;
use crate::gallery::Gallery;
use crate::openrouter::OpenRouterClient;
use crate::prefs::{AlignmentPrefs, AutoFallbacksPrefs, PrivacyPrefs};
use crate::worker::DiffusionWorkerClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Studio bearer (`FREYA_API_TOKEN`). Overrides env when present so UI rotate survives restarts
    /// when the secrets file lives on durable storage (e.g. gallery PVC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
    /// Persisted model picks (Auto sentinels allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_model: Option<String>,
    /// OpenRouter privacy prefs (ZDR / data_collection) + Freya chat-provider checkboxes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<PrivacyPrefs>,
    /// Freya-side model alignment / NSFW / tone prefs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<AlignmentPrefs>,
    /// Configurable Auto fallbacks when the router does not pin a concrete model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fallbacks: Option<AutoFallbacksPrefs>,
    /// Set after the old `openrouter/auto` Auto sentinel is rewritten to internal `auto`.
    #[serde(default)]
    pub legacy_upstream_auto_migrated: bool,
    /// Per-mode system prompt overrides (non-empty values only). Empty map omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_overrides: Option<HashMap<String, String>>,
}

impl AppState {
    pub async fn from_config(cfg: Config) -> anyhow::Result<Self> {
        crate::auto_matrix::init(&cfg.packs_dir);
        let gallery = Gallery::open(cfg.gallery_dir.clone()).await?;
        let openrouter = OpenRouterClient::new(&cfg);
        let civitai = CivitaiClient::new(cfg.civitai_api_key.clone());
        // Client attaches when a worker URL is set (enable flag kept for API compat only).
        let worker = cfg.diffusion_worker_url.as_ref().map(|u| {
            DiffusionWorkerClient::new_with_tls(u.clone(), cfg.worker_tls_insecure)
                .with_token(cfg.worker_token.clone())
        });
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
        state.migrate_legacy_upstream_auto().await?;
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
        self.apply_secrets_inner(secrets, true).await;
        Ok(())
    }

    pub async fn apply_secrets(&self, secrets: RuntimeSecrets) {
        self.apply_secrets_inner(secrets, false).await;
    }

    async fn apply_secrets_inner(&self, secrets: RuntimeSecrets, from_disk: bool) {
        let mut cfg = self.config.write().await;
        if assign_secret(&mut cfg.openrouter_api_key, secrets.openrouter_api_key, from_disk) {
            let key = cfg.openrouter_api_key.clone();
            let mut or = self.openrouter.write().await;
            *or = OpenRouterClient::new(&cfg).with_key(key);
        }
        if assign_secret(&mut cfg.civitai_api_key, secrets.civitai_api_key, from_disk) {
            let key = cfg.civitai_api_key.clone();
            let mut civ = self.civitai.write().await;
            *civ = CivitaiClient::new(key);
        }
        let _ = assign_secret(&mut cfg.hf_token, secrets.hf_token, from_disk);
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
        if let Some(tok) = secrets.api_token {
            // Empty string clears auth (local/dev). Non-empty overrides env / previous value.
            cfg.api_token = tok;
        }
        let mut models_changed = false;
        if let Some(m) = secrets.chat_model {
            if !m.trim().is_empty() {
                cfg.chat_model = m;
                models_changed = true;
            }
        }
        if let Some(m) = secrets.image_model {
            if !m.trim().is_empty() {
                cfg.image_model = m;
                models_changed = true;
            }
        }
        if let Some(m) = secrets.router_model {
            if !m.trim().is_empty() {
                cfg.router_model = m;
                models_changed = true;
            }
        }
        let mut prefs_changed = false;
        if let Some(p) = secrets.privacy {
            cfg.apply_privacy(p);
            prefs_changed = true;
        }
        if let Some(a) = secrets.alignment {
            cfg.apply_alignment(a);
            prefs_changed = true;
        }
        if let Some(f) = secrets.auto_fallbacks {
            cfg.apply_auto_fallbacks(f);
            prefs_changed = true;
        }
        if secrets.legacy_upstream_auto_migrated {
            cfg.legacy_upstream_auto_migrated = true;
        }
        if let Some(overrides) = secrets.instruction_overrides {
            cfg.instruction_overrides = normalize_instruction_overrides(overrides);
        }
        if models_changed || prefs_changed {
            let key = cfg.openrouter_api_key.clone();
            let mut or = self.openrouter.write().await;
            *or = OpenRouterClient::new(&cfg).with_key(key);
        }
        drop(cfg);
        self.rebuild_worker_client().await;
    }

    /// Old Auto was stored as `openrouter/auto`. Rewrite that sentinel to internal `auto` once.
    async fn migrate_legacy_upstream_auto(&self) -> anyhow::Result<()> {
        let changed = {
            let mut cfg = self.config.write().await;
            if cfg.legacy_upstream_auto_migrated {
                false
            } else {
                if cfg.chat_model.trim().eq_ignore_ascii_case("openrouter/auto") {
                    cfg.chat_model = "auto".into();
                }
                cfg.legacy_upstream_auto_migrated = true;
                true
            }
        };
        if !changed {
            return Ok(());
        }
        {
            let cfg = self.config.read().await;
            let key = cfg.openrouter_api_key.clone();
            let mut or = self.openrouter.write().await;
            *or = OpenRouterClient::new(&cfg).with_key(key);
        }
        if let Err(e) = self.persist_secrets().await {
            tracing::warn!(error = %e, "legacy auto migration could not persist");
        }
        Ok(())
    }

    /// Attach or clear the in-memory worker client from the configured URL.
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
            openrouter_api_key: none_if_blank(&cfg.openrouter_api_key),
            civitai_api_key: none_if_blank(&cfg.civitai_api_key),
            hf_token: none_if_blank(&cfg.hf_token),
            diffusion_worker_url: cfg.diffusion_worker_url.clone(),
            worker_enabled: Some(cfg.worker_enabled),
            api_token: if cfg.api_token.trim().is_empty() {
                None
            } else {
                Some(cfg.api_token.clone())
            },
            chat_model: Some(cfg.chat_model.clone()),
            image_model: Some(cfg.image_model.clone()),
            router_model: Some(cfg.router_model.clone()),
            privacy: Some(cfg.privacy_prefs()),
            alignment: Some(cfg.alignment_prefs()),
            auto_fallbacks: Some(cfg.auto_fallbacks_prefs()),
            legacy_upstream_auto_migrated: cfg.legacy_upstream_auto_migrated,
            instruction_overrides: if cfg.instruction_overrides.is_empty() {
                None
            } else {
                Some(cfg.instruction_overrides.clone())
            },
        };
        let raw = serde_json::to_vec_pretty(&secrets)?;
        tokio::fs::write(&cfg.secrets_file, raw).await?;
        Ok(())
    }
}

fn none_if_blank(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Disk loads must not replace an env/Secret key with a blank. A live Settings clear still applies.
fn assign_secret(current: &mut String, incoming: Option<String>, from_disk: bool) -> bool {
    match incoming {
        Some(k) if k.trim().is_empty() => {
            if from_disk || current.trim().is_empty() {
                false
            } else {
                current.clear();
                true
            }
        }
        Some(k) => {
            *current = k;
            true
        }
        None => false,
    }
}

/// Keep only safe mode ids with non-empty trimmed text.
pub fn normalize_instruction_overrides(raw: HashMap<String, String>) -> HashMap<String, String> {
    raw.into_iter()
        .filter(|(k, v)| crate::styles::is_editable_instruction_mode(k) && !v.trim().is_empty())
        .map(|(k, v)| (k, v))
        .collect()
}
