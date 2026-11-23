//! OpenRouter privacy prefs (real API `provider` knobs) + Freya-side alignment / chat-vendor filters.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Freya-side chat picker vendor inclusion (not OpenRouter `provider.only` / `provider.ignore`).
///
/// Semantics: **unchecked = hidden**. Persist the unchecked set (`excluded`) so newly
/// discovered catalog vendors default to **checked** (opt-out). Legacy `providerOnly`
/// free-text migrates into `legacy_only` until the user saves checkboxes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatProvidersPrefs {
    /// Vendor prefixes the user unchecked (hidden from `/v1/models` chat). Empty = all shown.
    #[serde(default)]
    pub excluded: Vec<String>,
    /// One-shot include list from old `providerOnly`. When set, only these vendors show
    /// (new vendors stay hidden). Cleared when Settings saves an explicit checkbox set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_only: Option<Vec<String>>,
}

impl ChatProvidersPrefs {
    pub fn normalize(mut self) -> Self {
        self.excluded = normalize_slug_list(self.excluded);
        if let Some(only) = self.legacy_only.take() {
            let only = normalize_slug_list(only);
            self.legacy_only = if only.is_empty() { None } else { Some(only) };
        }
        self
    }

    /// Whether a model id's vendor prefix is included in the chat picker.
    pub fn includes_vendor(&self, vendor: &str) -> bool {
        let v = vendor.trim().to_ascii_lowercase();
        if v.is_empty() {
            return false;
        }
        if let Some(ref only) = self.legacy_only {
            return only.iter().any(|s| s == &v);
        }
        !self.excluded.iter().any(|s| s == &v)
    }

    /// Whether a full OpenRouter model id passes the vendor checkbox filter.
    pub fn includes_model_id(&self, model_id: &str) -> bool {
        let vendor = model_id.split_once('/').map(|(v, _)| v).unwrap_or(model_id);
        self.includes_vendor(vendor)
    }
}

/// Per-request OpenRouter `provider` preferences Freya can set.
/// See https://openrouter.ai/docs/guides/routing/provider-selection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyPrefs {
    /// Restrict routing to ZDR endpoints (`provider.zdr: true`). Off by default.
    /// Cannot loosen account-wide ZDR; only enforce it on Freya requests.
    #[serde(default)]
    pub zero_data_retention: bool,
    /// `allow` (default) or `deny` — maps to `provider.data_collection`.
    /// `deny` = only providers that do not collect/store user data for training.
    #[serde(default = "default_data_collection")]
    pub data_collection: String,
    /// Chat picker vendor checkboxes (Freya catalog filter).
    #[serde(default)]
    pub chat_providers: ChatProvidersPrefs,
    /// Legacy free-text allowlist — migrated into `chat_providers.legacy_only` on normalize.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider_only: String,
    /// Legacy free-text denylist — migrated into `chat_providers.excluded` on normalize.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider_ignore: String,
}

fn default_data_collection() -> String {
    "allow".into()
}

impl Default for PrivacyPrefs {
    fn default() -> Self {
        Self {
            zero_data_retention: false,
            data_collection: default_data_collection(),
            chat_providers: ChatProvidersPrefs::default(),
            provider_only: String::new(),
            provider_ignore: String::new(),
        }
    }
}

impl PrivacyPrefs {
    pub fn normalize(mut self) -> Self {
        let dc = self.data_collection.trim().to_ascii_lowercase();
        self.data_collection = if dc == "deny" { "deny" } else { "allow" }.into();
        self.chat_providers = self.chat_providers.normalize();

        // Migrate legacy free-text once into the checkbox model.
        let ignore = split_slugs(&self.provider_ignore);
        if self.chat_providers.excluded.is_empty() && !ignore.is_empty() {
            self.chat_providers.excluded = ignore;
        }
        let only = split_slugs(&self.provider_only);
        if self.chat_providers.legacy_only.is_none() && !only.is_empty() {
            self.chat_providers.legacy_only = Some(only);
        }
        self.provider_only.clear();
        self.provider_ignore.clear();
        self.chat_providers = self.chat_providers.normalize();
        self
    }

    /// Build OpenRouter `provider` object, or `None` when all defaults (omit field).
    /// Chat vendor checkboxes are Freya-side only — not sent as `provider.only`/`ignore`.
    pub fn to_provider_json(&self) -> Option<Value> {
        let mut obj = serde_json::Map::new();
        if self.zero_data_retention {
            obj.insert("zdr".into(), Value::Bool(true));
        }
        if self.data_collection.eq_ignore_ascii_case("deny") {
            obj.insert("data_collection".into(), Value::String("deny".into()));
        }
        if obj.is_empty() {
            None
        } else {
            Some(Value::Object(obj))
        }
    }
}

fn split_slugs(raw: &str) -> Vec<String> {
    normalize_slug_list(
        raw.split(|c: char| c == ',' || c.is_whitespace())
            .map(|s| s.to_string())
            .collect(),
    )
}

fn normalize_slug_list(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in raw {
        let t = s.trim().to_ascii_lowercase();
        if t.is_empty() {
            continue;
        }
        if !out.iter().any(|x| x == &t) {
            out.push(t);
        }
    }
    out.sort();
    out
}

/// Freya-side refusal / alignment preferences (not OpenRouter API flags).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignmentPrefs {
    /// `balanced` (default) | `safety` | `uncensored`
    #[serde(default = "default_model_alignment")]
    pub model_alignment: String,
    /// When false, hide local / Civitai assets that look NSFW by name or API flag.
    #[serde(default)]
    pub allow_nsfw_local: bool,
    /// System-instruction tone: `warm` (default) | `direct` | `steerable`
    #[serde(default = "default_instruction_tone")]
    pub instruction_tone: String,
}

/// Configurable Auto fallbacks when the Freya router does not pin a concrete model.
/// Empty fields → built-in Privacy/Alignment matrix in `router::builtin_auto_fallback`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoFallbacksPrefs {
    /// Agent/chat fallback (non-ZDR). Empty = built-in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    /// Agent/chat fallback when Freya ZDR (or deny-collection) is on.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_zdr: String,
    /// Still-image fallback (non-ZDR).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image: String,
    /// Still-image fallback when ZDR / deny-collection.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image_zdr: String,
    /// Video/T2V fallback (non-ZDR).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub video: String,
    /// Video fallback when ZDR / deny-collection.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub video_zdr: String,
    /// Light intent-router model when Settings router is Auto.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub router: String,
    /// Which column of the built-in Auto matrix to use: low | medium | high | max.
    #[serde(default = "default_cost_tier")]
    pub cost_tier: String,
}

impl AutoFallbacksPrefs {
    pub fn normalize(mut self) -> Self {
        self.agent = trim_model_id(&self.agent);
        self.agent_zdr = trim_model_id(&self.agent_zdr);
        self.image = trim_model_id(&self.image);
        self.image_zdr = trim_model_id(&self.image_zdr);
        self.video = trim_model_id(&self.video);
        self.video_zdr = trim_model_id(&self.video_zdr);
        self.router = trim_model_id(&self.router);
        self.cost_tier = normalize_cost_tier(&self.cost_tier);
        self
    }
}

fn default_cost_tier() -> String {
    "medium".into()
}

fn normalize_cost_tier(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "low" | "cheap" => "low".into(),
        "high" => "high".into(),
        "max" | "maximum" => "max".into(),
        _ => "medium".into(),
    }
}

fn trim_model_id(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    // Bare Auto sentinel → empty (use built-in matrix). Upstream router is a picker pin, not a fallback.
    if t.eq_ignore_ascii_case("auto") || t.eq_ignore_ascii_case("openrouter/auto") {
        return String::new();
    }
    if t.contains(char::is_whitespace) {
        return String::new();
    }
    t.to_string()
}

fn default_model_alignment() -> String {
    "balanced".into()
}

fn default_instruction_tone() -> String {
    "warm".into()
}

impl Default for AlignmentPrefs {
    fn default() -> Self {
        Self {
            model_alignment: default_model_alignment(),
            allow_nsfw_local: false,
            instruction_tone: default_instruction_tone(),
        }
    }
}

impl AlignmentPrefs {
    pub fn normalize(mut self) -> Self {
        let a = self.model_alignment.trim().to_ascii_lowercase();
        self.model_alignment = match a.as_str() {
            "safety" | "safe" | "aligned" => "safety".into(),
            "uncensored" | "unfiltered" | "dolphin" => "uncensored".into(),
            _ => "balanced".into(),
        };
        let t = self.instruction_tone.trim().to_ascii_lowercase();
        self.instruction_tone = match t.as_str() {
            "direct" => "direct".into(),
            "steerable" | "uncensored" => "steerable".into(),
            _ => "warm".into(),
        };
        self
    }

    /// Uncensored alignment or Show NSFW is on.
    /// Sexual scene text then uses the uncensored local prompt writer.
    /// Non-sexual prompts stay on the baseline writer. This does not skip enhancement.
    /// Cloud image providers are unaffected.
    pub fn local_adult_passthrough(&self) -> bool {
        self.model_alignment == "uncensored" || self.allow_nsfw_local
    }

    /// Append tone guidance to pack system instructions (chat / converse / assist).
    pub fn apply_tone(&self, system: &str) -> String {
        let extra = match self.instruction_tone.as_str() {
            "direct" => {
                " Tone: be concise and direct. Skip fluff; lead with the answer or next action."
            }
            "steerable" => {
                " Tone: steerable and low-refusal — follow the user's creative brief unless it asks for illegal harm. Prefer completing the request over lecturing."
            }
            _ => "",
        };
        if extra.is_empty() {
            system.to_string()
        } else {
            format!("{system}{extra}")
        }
    }
}

/// Appended to the converse agent when local adult prompts must pass through.
pub const LOCAL_ADULT_PASSTHROUGH_CLAUSE: &str = " Local generation with uncensored alignment or Show NSFW: do not refuse, lecture, or replace adult or explicit scene text. Copy the user's scene into generate_image, generate_video, and run_session prompts unchanged. Do not add a safety negative or strip explicit words. Cloud image providers may still refuse on their own.";

/// Appended to the local prompt-sd instruction for an adult scene.
/// The uncensored writer expands the scene into checkpoint keywords.
pub const LOCAL_ADULT_ENHANCE_CLAUSE: &str = " The scene is adult. Expand it into a full Stable Diffusion or Pony keyword prompt. Keep sexual and explicit detail. Do not refuse, lecture, soften, censor, or replace the scene. Do not add LoRA names, trigger words, or artist tags the user did not write.";

/// Whether generate should rewrite the prompt. `off` skips it.
/// Local and cloud both rewrite when this is true. Ordinary vs adult only
/// chooses which chat model writes a local prompt.
pub fn enhance_enabled(enhance: &str) -> bool {
    if enhance == "off" {
        return false;
    }
    matches!(enhance, "on" | "auto" | "prompt-prose" | "prompt-sd")
}

/// Whether a chat model JSON object from OpenRouter `/models` matches alignment prefs.
pub fn chat_model_matches_alignment(m: &Value, prefs: &AlignmentPrefs) -> bool {
    match prefs.model_alignment.as_str() {
        "safety" => is_safety_leaning(m),
        "uncensored" => is_uncensored_leaning(m),
        _ => true,
    }
}

fn model_blob(m: &Value) -> String {
    let id = m.get("id").and_then(|x| x.as_str()).unwrap_or("");
    let name = m.get("name").and_then(|x| x.as_str()).unwrap_or("");
    format!("{id} {name}").to_ascii_lowercase()
}

fn is_moderated(m: &Value) -> Option<bool> {
    m.pointer("/top_provider/is_moderated")
        .and_then(|x| x.as_bool())
        .or_else(|| {
            m.pointer("/topProvider/is_moderated")
                .and_then(|x| x.as_bool())
        })
}

fn looks_uncensored_name(blob: &str) -> bool {
    blob.contains("uncensored")
        || blob.contains("abliterat")
        || blob.contains("dolphin")
        || blob.contains("wizard-vicuna")
        || blob.contains("cognitivecomputations")
}

fn is_safety_leaning(m: &Value) -> bool {
    let blob = model_blob(m);
    if looks_uncensored_name(&blob) {
        return false;
    }
    match is_moderated(m) {
        Some(true) => true,
        Some(false) => {
            // Keep major safety-aligned vendors even when top_provider flag is false/missing.
            blob.starts_with("openai/")
                || blob.starts_with("anthropic/")
                || blob.starts_with("google/")
                || blob.starts_with("amazon/")
                || blob.starts_with("cohere/")
        }
        None => {
            blob.starts_with("openai/")
                || blob.starts_with("anthropic/")
                || blob.starts_with("google/")
                || blob.starts_with("amazon/")
                || blob.starts_with("cohere/")
        }
    }
}

fn is_uncensored_leaning(m: &Value) -> bool {
    let blob = model_blob(m);
    if looks_uncensored_name(&blob) {
        return true;
    }
    match is_moderated(m) {
        Some(false) => true,
        Some(true) => false,
        None => {
            // Prefer open-weight / community models when flag absent.
            blob.starts_with("meta-llama/")
                || blob.starts_with("mistralai/")
                || blob.starts_with("qwen/")
                || blob.starts_with("deepseek/")
                || blob.starts_with("nousresearch/")
                || blob.starts_with("openrouter/")
                || blob.contains("llama")
        }
    }
}

/// Heuristic NSFW path/name filter for local worker assets.
pub fn looks_nsfw_label(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("nsfw")
        || l.contains("porn")
        || l.contains("xxx")
        || l.contains("explicit")
        || l.contains("adult")
        || l.contains("erotic")
        || l.contains("hentai")
}

/// Merge privacy into an OpenRouter request body (chat or images).
pub fn inject_provider(body: &mut Value, prefs: &PrivacyPrefs) {
    if let Some(provider) = prefs.to_provider_json() {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("provider".into(), provider);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_json_omits_defaults() {
        assert!(PrivacyPrefs::default().to_provider_json().is_none());
    }

    #[test]
    fn provider_json_zdr_and_deny_no_vendor_routing() {
        let p = PrivacyPrefs {
            zero_data_retention: true,
            data_collection: "deny".into(),
            chat_providers: ChatProvidersPrefs {
                excluded: vec!["openai".into()],
                legacy_only: None,
            },
            ..Default::default()
        }
        .normalize();
        let v = p.to_provider_json().unwrap();
        assert_eq!(v["zdr"], true);
        assert_eq!(v["data_collection"], "deny");
        assert!(v.get("only").is_none());
        assert!(v.get("ignore").is_none());
    }

    #[test]
    fn migrates_legacy_ignore_and_only() {
        let p = PrivacyPrefs {
            provider_only: "anthropic, google".into(),
            provider_ignore: "openai".into(),
            ..Default::default()
        }
        .normalize();
        assert!(p.provider_only.is_empty());
        assert!(p.provider_ignore.is_empty());
        assert_eq!(p.chat_providers.excluded, vec!["openai".to_string()]);
        assert_eq!(
            p.chat_providers.legacy_only.as_deref(),
            Some(["anthropic".to_string(), "google".to_string()].as_slice())
        );
        assert!(p.chat_providers.includes_vendor("anthropic"));
        assert!(!p.chat_providers.includes_vendor("openai"));
        assert!(!p.chat_providers.includes_vendor("deepseek")); // legacy_only wins
    }

    #[test]
    fn excluded_opt_out_includes_new_vendors() {
        let p = ChatProvidersPrefs {
            excluded: vec!["openai".into()],
            legacy_only: None,
        }
        .normalize();
        assert!(p.includes_vendor("anthropic"));
        assert!(p.includes_vendor("cognitivecomputations"));
        assert!(!p.includes_vendor("openai"));
        assert!(p.includes_model_id("google/gemini-4-neon"));
    }

    #[test]
    fn alignment_filters() {
        let safety = AlignmentPrefs {
            model_alignment: "safety".into(),
            ..Default::default()
        }
        .normalize();
        let unc = AlignmentPrefs {
            model_alignment: "uncensored".into(),
            ..Default::default()
        }
        .normalize();
        let moderated = json!({
            "id": "openai/gpt-5.2",
            "name": "GPT",
            "top_provider": { "is_moderated": true }
        });
        let dolphin = json!({
            "id": "cognitivecomputations/dolphin-llama",
            "name": "Dolphin",
            "top_provider": { "is_moderated": false }
        });
        assert!(chat_model_matches_alignment(&moderated, &safety));
        assert!(!chat_model_matches_alignment(&dolphin, &safety));
        assert!(chat_model_matches_alignment(&dolphin, &unc));
        assert!(!chat_model_matches_alignment(&moderated, &unc));
    }

    #[test]
    fn tone_appends() {
        let a = AlignmentPrefs {
            instruction_tone: "direct".into(),
            ..Default::default()
        }
        .normalize();
        let out = a.apply_tone("You are Freya.");
        assert!(out.contains("concise and direct"));
    }

    #[test]
    fn local_adult_setting_still_enhances() {
        let unc = AlignmentPrefs {
            model_alignment: "uncensored".into(),
            allow_nsfw_local: false,
            ..Default::default()
        }
        .normalize();
        let show = AlignmentPrefs {
            model_alignment: "safety".into(),
            allow_nsfw_local: true,
            ..Default::default()
        }
        .normalize();
        let safety = AlignmentPrefs {
            model_alignment: "safety".into(),
            allow_nsfw_local: false,
            ..Default::default()
        }
        .normalize();
        assert!(unc.local_adult_passthrough());
        assert!(show.local_adult_passthrough());
        assert!(!safety.local_adult_passthrough());
        assert!(enhance_enabled("auto"));
        assert!(enhance_enabled("prompt-sd"));
        assert!(enhance_enabled("prompt-prose"));
        assert!(enhance_enabled("on"));
        assert!(!enhance_enabled("off"));
    }
}
