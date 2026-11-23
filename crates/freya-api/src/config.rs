use crate::prefs::{AlignmentPrefs, AutoFallbacksPrefs, ChatProvidersPrefs, PrivacyPrefs};
use crate::studio::StudioPrefs;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(name = "freya-api", about = "Freya automation API")]
pub struct Config {
    #[arg(long, env = "FREYA_BIND", default_value = "0.0.0.0:8080")]
    pub bind: String,

    #[arg(long, env = "OPENROUTER_API_KEY", default_value = "")]
    pub openrouter_api_key: String,

    #[arg(
        long,
        env = "OPENROUTER_BASE_URL",
        default_value = "https://openrouter.ai/api/v1"
    )]
    pub openrouter_base_url: String,

    #[arg(long, env = "FREYA_CHAT_MODEL", default_value = "openrouter/auto")]
    pub chat_model: String,

    #[arg(
        long,
        env = "FREYA_IMAGE_MODEL",
        default_value = "black-forest-labs/flux.2-klein-4b"
    )]
    pub image_model: String,

    #[arg(long, env = "FREYA_CHAT_BACKEND", default_value = "openrouter")]
    pub chat_backend: String,

    #[arg(long, env = "FREYA_CHAT_BASE_URL")]
    pub chat_base_url: Option<String>,

    #[arg(long, env = "FREYA_GALLERY_DIR", default_value = "./data/gallery")]
    pub gallery_dir: PathBuf,

    #[arg(long, env = "FREYA_PACKS_DIR", default_value = "./packs")]
    pub packs_dir: PathBuf,

    #[arg(long, env = "FREYA_DIFFUSION_WORKER_URL")]
    pub diffusion_worker_url: Option<String>,

    /// Civitai API key — used for search/install of LoRAs onto workers. Never logged.
    #[arg(long, env = "CIVITAI_API_KEY", default_value = "")]
    pub civitai_api_key: String,

    #[arg(long, env = "HF_TOKEN", default_value = "")]
    pub hf_token: String,

    /// Optional path to persist runtime secrets written from the UI (local/dev).
    #[arg(
        long,
        env = "FREYA_SECRETS_FILE",
        default_value = "./data/secrets.json"
    )]
    pub secrets_file: PathBuf,

    /// Optional bearer token for `/v1/*` routes. Empty = open (local/dev only).
    #[arg(long, env = "FREYA_API_TOKEN", default_value = "")]
    pub api_token: String,

    /// Bearer token the API sends to the diffusion worker (`FREYA_WORKER_TOKEN`).
    #[arg(long, env = "FREYA_WORKER_TOKEN", default_value = "")]
    pub worker_token: String,

    /// Accept invalid/self-signed TLS when calling the diffusion worker (local/dev Ingress).
    #[arg(long, env = "FREYA_WORKER_TLS_INSECURE", default_value_t = false)]
    pub worker_tls_insecure: bool,

    /// Docker container name for local start/stop (`docker start|stop`).
    #[arg(
        long,
        env = "FREYA_WORKER_CONTAINER",
        default_value = "freya-local-worker"
    )]
    pub worker_container: String,

    /// Optional systemd unit name if the worker is not a docker container.
    #[arg(
        long,
        env = "FREYA_WORKER_SYSTEMD_UNIT",
        default_value = "freya-worker.service"
    )]
    pub worker_systemd_unit: String,

    /// Comma-separated allowed CORS origins. Empty + no token → permissive (dev warning).
    /// Empty + token set → same-origin only (no `*`).
    #[arg(long, env = "FREYA_CORS_ORIGINS", default_value = "")]
    pub cors_origins: String,

    /// OpenRouter per-request ZDR (`provider.zdr`). Persisted via Settings / secrets file.
    #[arg(long, env = "FREYA_OPENROUTER_ZDR", default_value_t = false)]
    pub openrouter_zdr: bool,

    /// OpenRouter `provider.data_collection`: allow|deny.
    #[arg(
        long,
        env = "FREYA_OPENROUTER_DATA_COLLECTION",
        default_value = "allow"
    )]
    pub openrouter_data_collection: String,

    /// Comma-separated chat vendors unchecked in Settings (Freya catalog filter).
    #[arg(long, env = "FREYA_CHAT_PROVIDERS_EXCLUDED", default_value = "")]
    pub chat_providers_excluded: String,

    /// Legacy comma-separated include-only vendors (from old `FREYA_OPENROUTER_PROVIDER_ONLY`).
    #[arg(long, env = "FREYA_OPENROUTER_PROVIDER_ONLY", default_value = "")]
    pub openrouter_provider_only: String,

    /// Legacy comma-separated ignore list (from old `FREYA_OPENROUTER_PROVIDER_IGNORE`).
    #[arg(long, env = "FREYA_OPENROUTER_PROVIDER_IGNORE", default_value = "")]
    pub openrouter_provider_ignore: String,

    /// Model catalog alignment: balanced|safety|uncensored (Freya-side filter).
    #[arg(long, env = "FREYA_MODEL_ALIGNMENT", default_value = "balanced")]
    pub model_alignment: String,

    /// When false, filter NSFW-looking local/Civitai assets from lists/search.
    #[arg(long, env = "FREYA_ALLOW_NSFW_LOCAL", default_value_t = false)]
    pub allow_nsfw_local: bool,

    /// System instruction tone: warm|direct|steerable.
    #[arg(long, env = "FREYA_INSTRUCTION_TONE", default_value = "warm")]
    pub instruction_tone: String,

    /// Per-mode system prompt overrides (Settings). Empty / missing → packs/instructions defaults.
    /// Not a CLI flag — loaded from `FREYA_SECRETS_FILE`.
    #[arg(skip)]
    pub instruction_overrides: HashMap<String, String>,

    /// Configurable Auto fallbacks (Models tab). Empty fields → Privacy/Alignment built-ins.
    /// Not a CLI flag — loaded from `FREYA_SECRETS_FILE`.
    #[arg(skip)]
    pub auto_fallbacks: AutoFallbacksPrefs,

    /// One-shot: the old Auto sentinel `openrouter/auto` was rewritten to internal `auto`.
    /// After this is set, an explicit upstream selection is kept.
    #[arg(skip)]
    pub legacy_upstream_auto_migrated: bool,

    /// Favorites, LoRA pins, composer backend, video pin, automation mode.
    /// Not a CLI flag — loaded from `FREYA_SECRETS_FILE`. No API keys.
    #[arg(skip)]
    pub studio: StudioPrefs,
}

impl Config {
    pub fn openrouter_configured(&self) -> bool {
        !self.openrouter_api_key.trim().is_empty()
    }

    pub fn civitai_configured(&self) -> bool {
        !self.civitai_api_key.trim().is_empty()
    }

    pub fn chat_url(&self) -> String {
        if self.chat_backend == "vllm" {
            self.chat_base_url
                .clone()
                .unwrap_or_else(|| self.openrouter_base_url.clone())
        } else {
            self.openrouter_base_url.clone()
        }
    }
}
