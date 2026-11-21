//! Freya CRD types for `freya.dataknife.ai/v1alpha1`.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const API_GROUP: &str = "freya.dataknife.ai";
pub const API_VERSION: &str = "v1alpha1";

/// Image reference for Freya components.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_policy: Option<String>,
}

/// Reference to a Secret key holding credentials.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeyRef {
    pub name: String,
    #[serde(default = "default_api_key_key")]
    pub key: String,
}

fn default_api_key_key() -> String {
    "apiKey".into()
}

/// Default Secret key name for Civitai tokens.
pub fn default_civitai_key() -> String {
    "apiKey".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterSpec {
    pub secret_ref: SecretKeyRef,
    /// Default chat / plan model id (OpenRouter).
    #[serde(default = "default_chat_model")]
    pub chat_model: String,
    /// Default cloud image model id (OpenRouter).
    #[serde(default = "default_image_model")]
    pub image_model: String,
    #[serde(default = "default_openrouter_base")]
    pub base_url: String,
}

fn default_chat_model() -> String {
    "openrouter/auto".into()
}

fn default_image_model() -> String {
    "black-forest-labs/flux.2-klein-4b".into()
}

fn default_openrouter_base() -> String {
    "https://openrouter.ai/api/v1".into()
}

impl Default for OpenRouterSpec {
    fn default() -> Self {
        Self {
            secret_ref: SecretKeyRef {
                name: "freya-openrouter".into(),
                key: default_api_key_key(),
            },
            chat_model: default_chat_model(),
            image_model: default_image_model(),
            base_url: default_openrouter_base(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChatBackendKind {
    #[default]
    Openrouter,
    Vllm,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatBackendSpec {
    #[serde(default)]
    pub kind: ChatBackendKind,
    /// When kind=vllm, OpenAI-compatible base URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct IngressSpec {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StorageSpec {
    #[serde(default = "default_gallery_size")]
    pub gallery_size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,
}

fn default_gallery_size() -> String {
    "20Gi".into()
}

impl Default for StorageSpec {
    fn default() -> Self {
        Self {
            gallery_size: default_gallery_size(),
            storage_class_name: None,
        }
    }
}

/// Optional Civitai credentials for automated model/LoRA installs (never inline the key).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiSpec {
    /// Secret holding the Civitai API token (create from UI or `kubectl create secret`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<SecretKeyRef>,
}

/// Optional Hugging Face token for gated checkpoints.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<SecretKeyRef>,
}

/// Namespaced Freya studio: API + UI + gallery.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "freya.dataknife.ai",
    version = "v1alpha1",
    kind = "FreyaStudio",
    plural = "freyastudios",
    shortname = "fstudio",
    namespaced,
    status = "FreyaStudioStatus",
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct FreyaStudioSpec {
    #[serde(default)]
    pub open_router: OpenRouterSpec,
    /// Civitai API key for browsing/installing LoRAs & checkpoints onto workers.
    #[serde(default)]
    pub civitai: CivitaiSpec,
    #[serde(default)]
    pub hugging_face: HuggingFaceSpec,
    #[serde(default)]
    pub chat_backend: ChatBackendSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_image: Option<ImageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_image: Option<ImageRef>,
    #[serde(default)]
    pub storage: StorageSpec,
    #[serde(default)]
    pub ingress: IngressSpec,
    /// Optional FreyaDiffusionWorker name in the same namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffusion_worker_ref: Option<String>,
    #[serde(default = "default_replicas")]
    pub replicas: i32,
}

fn default_replicas() -> i32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FreyaStudioStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}

/// Generation backend selection.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GenerationBackend {
    #[default]
    Auto,
    Openrouter,
    Local,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoraRef {
    pub name: String,
    #[serde(default = "default_lora_scale")]
    pub scale: f32,
}

fn default_lora_scale() -> f32 {
    1.0
}

/// Declarative image generation request.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "freya.dataknife.ai",
    version = "v1alpha1",
    kind = "FreyaGeneration",
    plural = "freyagenerations",
    shortname = "fgen",
    namespaced,
    status = "FreyaGenerationStatus",
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
    printcolumn = r#"{"name":"Backend","type":"string","jsonPath":".status.backend"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct FreyaGenerationSpec {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// `off` | `on` | `auto`
    #[serde(default = "default_enhance")]
    pub enhance: String,
    #[serde(default)]
    pub backend: GenerationBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loras: Vec<LoraRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// FreyaStudio that owns API routing (same namespace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub studio_ref: Option<String>,
}

fn default_enhance() -> String {
    "auto".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FreyaGenerationStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhanced_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Curated style pack (Civitai-inspired metadata, not weight hosting by itself).
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "freya.dataknife.ai",
    version = "v1alpha1",
    kind = "FreyaStyle",
    plural = "freyastyles",
    shortname = "fstyle",
    namespaced,
    status = "FreyaStyleStatus"
)]
#[serde(rename_all = "camelCase")]
pub struct FreyaStyleSpec {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub example_prompt: String,
    #[serde(default)]
    pub backend: GenerationBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_router_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_checkpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loras: Vec<LoraRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_words: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FreyaStyleStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DiffusionEngine {
    #[default]
    #[serde(rename = "vllm-omni")]
    VllmOmni,
    #[serde(rename = "sglang")]
    Sglang,
}

/// Where the diffusion / LoRA engine runs.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum WorkerPlacement {
    /// Operator creates a GPU Deployment in-cluster.
    #[default]
    InCluster,
    /// Reach a remote physical (or other) host Freya already can HTTP to.
    External,
}

/// Optional GPU worker for local SD / LoRA (vLLM-Omni by default).
///
/// Placement:
/// - `inCluster` — operator deploys the engine on a GPU node when the cluster has GPUs.
/// - `external` — point `externalEndpoint` at a remote physical system running vLLM-Omni
///   (or compatible `/v1/images/generations` + LoRA install API). Freya UI/API use that
///   endpoint; `FreyaModelAsset` installs LoRAs onto the remote worker for the UI catalog.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "freya.dataknife.ai",
    version = "v1alpha1",
    kind = "FreyaDiffusionWorker",
    plural = "freyadiffusionworkers",
    shortname = "fdiff",
    namespaced,
    status = "FreyaDiffusionWorkerStatus",
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#,
    printcolumn = r#"{"name":"Engine","type":"string","jsonPath":".spec.engine"}"#,
    printcolumn = r#"{"name":"Placement","type":"string","jsonPath":".spec.placement"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct FreyaDiffusionWorkerSpec {
    #[serde(default)]
    pub placement: WorkerPlacement,
    /// Required when `placement: external` — e.g. `http://gpu-box.lan:8091`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_endpoint: Option<String>,
    #[serde(default)]
    pub engine: DiffusionEngine,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
    /// HuggingFace / local model id passed to the engine (in-cluster).
    #[serde(default = "default_local_model")]
    pub model: String,
    #[serde(default = "default_gpu_count")]
    pub gpu_count: i32,
    #[serde(default = "default_models_size")]
    pub models_pvc_size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,
    #[serde(default = "default_worker_port")]
    pub port: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_selector: Option<std::collections::BTreeMap<String, String>>,
    /// Optional admin/API token Secret for remote LoRA install + generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_secret_ref: Option<SecretKeyRef>,
}

fn default_local_model() -> String {
    "stabilityai/stable-diffusion-xl-base-1.0".into()
}

fn default_gpu_count() -> i32 {
    1
}

fn default_models_size() -> String {
    "100Gi".into()
}

fn default_worker_port() -> i32 {
    8091
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FreyaDiffusionWorkerStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// Declarative model/LoRA download into a worker PVC.
#[derive(CustomResource, Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "freya.dataknife.ai",
    version = "v1alpha1",
    kind = "FreyaModelAsset",
    plural = "freyamodelassets",
    shortname = "fasset",
    namespaced,
    status = "FreyaModelAssetStatus",
    printcolumn = r#"{"name":"Phase","type":"string","jsonPath":".status.phase"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct FreyaModelAssetSpec {
    /// `huggingface` | `civitai` | `url`
    pub source: String,
    pub url: String,
    /// Relative path under the worker models PVC (e.g. `loras/my.safetensors`).
    pub dest_path: String,
    pub worker_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<SecretKeyRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FreyaModelAssetStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kube::CustomResourceExt;

    #[test]
    fn studio_crd_has_group() {
        let crd = FreyaStudio::crd();
        assert_eq!(crd.spec.group, API_GROUP);
    }

    #[test]
    fn all_crds_render() {
        let _ = FreyaStudio::crd();
        let _ = FreyaGeneration::crd();
        let _ = FreyaStyle::crd();
        let _ = FreyaDiffusionWorker::crd();
        let _ = FreyaModelAsset::crd();
    }
}
