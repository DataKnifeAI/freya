//! Freya-owned intent router (light model).
//!
//! Aligns modalities with legacy Freya (`legacy/docs/`):
//! - **chat** — MagicPrompt Chat / Ollama dolphin3 (`make llm`)
//! - **picture** — still image / ComfyUI (`make cui`); Flux for NL, SDXL ecosystem for LoRAs
//! - **gif** — SwarmUI video output format (`gif-hd` / webp), not still-image
//! - **video** — SwarmUI T2V / I2V (`make sui`; Wan / LTX)

use crate::openrouter::OpenRouterClient;
use crate::prefs::{AlignmentPrefs, AutoFallbacksPrefs, PrivacyPrefs};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Settings / UI Auto sentinels. Internal Freya routing — not OpenRouter's upstream router.
pub const AUTO_AGENT: &str = "auto";
pub const AUTO_IMAGE: &str = "auto";
pub const AUTO_ROUTER: &str = "auto";
/// Explicit opt-in for OpenRouter's upstream router. Never an Auto sentinel.
pub const UPSTREAM_ROUTER: &str = "openrouter/auto";

/// Medium + balanced. Cells live in `packs/auto-models.json` (ZDR off and ZDR on match for chat).
/// Router / agent: DeepSeek V4 Pro (tools + JSON, not an uncensored specialist, not Claude/GPT).
pub const DEFAULT_ROUTER_MODEL: &str = "deepseek/deepseek-v4-pro";
/// Picture Auto medium, ZDR off: Seedream 4.5 (capable default; Flux is the uncensored row).
pub const DEFAULT_AUTO_IMAGE_MODEL: &str = "bytedance-seed/seedream-4.5";
/// Picture Auto medium when ZDR is on. Microsoft MAI is on the ZDR image catalog; Flux is not.
pub const DEFAULT_AUTO_IMAGE_MODEL_ZDR: &str = "microsoft/mai-image-2.7";
/// Agent Auto medium: tool-calling chat. Not `openrouter/auto`.
pub const DEFAULT_AUTO_AGENT_MODEL: &str = "deepseek/deepseek-v4-pro";
/// Same id: DeepSeek V4 Pro is on the ZDR chat catalog.
pub const DEFAULT_AUTO_AGENT_MODEL_ZDR: &str = "deepseek/deepseek-v4-pro";
pub const DEFAULT_AUTO_AGENT_MODEL_SAFETY: &str = "openai/gpt-5-mini";
/// Cydonia's description is uncensored. It has JSON, not tools. Tool calls may be weaker.
pub const DEFAULT_AUTO_AGENT_MODEL_UNCENSORED: &str = "thedrummer/cydonia-24b-v4.1";
pub const DEFAULT_ROUTER_MODEL_ZDR: &str = "deepseek/deepseek-v4-pro";
pub const DEFAULT_ROUTER_MODEL_SAFETY: &str = "openai/gpt-5-mini";

/// Whether routing must prefer ZDR / no-collection endpoints.
pub fn privacy_requires_strict(privacy: &PrivacyPrefs) -> bool {
    privacy.zero_data_retention || privacy.data_collection.eq_ignore_ascii_case("deny")
}

/// Built-in Auto matrix: role × privacy × alignment × cost.
/// User overrides in `AutoFallbacksPrefs` win one cell. Ids come from
/// `packs/auto-models.json` (researched against OpenRouter). A missing file
/// uses the small fallback in `auto_matrix` so the API still boots.
///
/// Video: `GET /models?output_modalities=video&zdr=true` lists no models.
/// ZDR video cells repeat the non-ZDR picks and OpenRouter may reject them.
pub fn builtin_auto_fallback(
    kind: AutoFallbackKind,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    cost: &str,
) -> String {
    let strict = privacy_requires_strict(privacy);
    let role = match kind {
        AutoFallbackKind::Router => "router",
        AutoFallbackKind::Agent => "agent",
        AutoFallbackKind::Image => "image",
        AutoFallbackKind::Video => "video",
    };
    crate::auto_matrix::model_for(role, strict, alignment.model_alignment.as_str(), cost)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoFallbackKind {
    Agent,
    Image,
    Video,
    Router,
}

/// Resolve a concrete Auto fallback: user Models-tab pin → else built-in matrix.
pub fn resolve_auto_fallback(
    kind: AutoFallbackKind,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> String {
    let strict = privacy_requires_strict(privacy);
    let user = match (kind, strict) {
        (AutoFallbackKind::Agent, true) => fallbacks.agent_zdr.as_str(),
        (AutoFallbackKind::Agent, false) => fallbacks.agent.as_str(),
        (AutoFallbackKind::Image, true) => fallbacks.image_zdr.as_str(),
        (AutoFallbackKind::Image, false) => fallbacks.image.as_str(),
        (AutoFallbackKind::Video, true) => fallbacks.video_zdr.as_str(),
        (AutoFallbackKind::Video, false) => fallbacks.video.as_str(),
        (AutoFallbackKind::Router, _) => fallbacks.router.as_str(),
    };
    let user = user.trim();
    if !user.is_empty() {
        let user_id = if kind == AutoFallbackKind::Image {
            match crate::openrouter::canonical_image_model(user) {
                Some(id) => id,
                None => {
                    return builtin_auto_fallback(kind, privacy, alignment, &fallbacks.cost_tier);
                }
            }
        } else {
            user.to_string()
        };
        // When strict privacy is on, ignore non-Google Flux-ish image/video overrides.
        if strict {
            match kind {
                AutoFallbackKind::Image
                    if !user_id.starts_with("google/")
                        && (user_id.contains("flux")
                            || user_id.contains("seedream")
                            || user_id.contains("black-forest")) =>
                {
                    return builtin_auto_fallback(kind, privacy, alignment, &fallbacks.cost_tier)
                        .to_string();
                }
                AutoFallbackKind::Video
                    if !user_id.starts_with("google/")
                        && (user_id.contains("seedance")
                            || user_id.contains("kling")
                            || user_id.contains("hailuo")
                            || user_id.contains("flux")) =>
                {
                    return builtin_auto_fallback(kind, privacy, alignment, &fallbacks.cost_tier)
                        .to_string();
                }
                _ => {}
            }
        }
        return user_id;
    }
    builtin_auto_fallback(kind, privacy, alignment, &fallbacks.cost_tier).to_string()
}

/// Auto still-image default, Privacy-aware (legacy helper — prefers built-in ZDR matrix).
pub fn default_auto_image_model(zdr: bool) -> &'static str {
    if zdr {
        DEFAULT_AUTO_IMAGE_MODEL_ZDR
    } else {
        DEFAULT_AUTO_IMAGE_MODEL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Chat,
    Picture,
    Gif,
    Video,
}

impl Modality {
    pub fn as_str(self) -> &'static str {
        match self {
            Modality::Chat => "chat",
            Modality::Picture => "picture",
            Modality::Gif => "gif",
            Modality::Video => "video",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "chat" | "other" | "text" => Some(Modality::Chat),
            "picture" | "image" | "img" | "still" => Some(Modality::Picture),
            "gif" | "animation" | "animated" | "anim" => Some(Modality::Gif),
            "video" | "t2v" | "i2v" | "clip" | "movie" => Some(Modality::Video),
            _ => None,
        }
    }

    /// Still-image generate path (ComfyUI / OpenRouter images).
    pub fn is_still_image(self) -> bool {
        matches!(self, Modality::Picture)
    }

    /// Motion family (gif → short animation / video → T2V).
    pub fn is_motion(self) -> bool {
        matches!(self, Modality::Gif | Modality::Video)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub modality: Modality,
    #[serde(default)]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_model: Option<String>,
    /// How the decision was produced.
    #[serde(default)]
    pub source: String,
}

impl RouteDecision {
    /// Legacy hard-reject copy — always `None` now (motion executes when capable).
    #[allow(dead_code)]
    pub fn unsupported_message(&self) -> Option<String> {
        None
    }

    /// Preferred motion backend hint for logging / UI.
    pub fn motion_backend_hint(&self) -> Option<&'static str> {
        match self.modality {
            Modality::Gif => Some("openrouter-video|local-gif"),
            Modality::Video => Some("openrouter-video"),
            _ => None,
        }
    }
}

/// Default Auto video model (balanced medium). No video id is on the ZDR catalog,
/// so the ZDR balanced-medium cell is the same id.
pub const DEFAULT_AUTO_VIDEO_MODEL: &str = crate::openrouter::DEFAULT_AUTO_VIDEO_MODEL;
pub const DEFAULT_AUTO_VIDEO_MODEL_ZDR: &str = "bytedance/seedance-2.5-flash";

pub fn default_auto_video_model(zdr: bool) -> &'static str {
    if zdr {
        DEFAULT_AUTO_VIDEO_MODEL_ZDR
    } else {
        DEFAULT_AUTO_VIDEO_MODEL
    }
}

pub fn is_auto_video(model: &str) -> bool {
    let m = model.trim();
    m.is_empty() || m.eq_ignore_ascii_case(AUTO_IMAGE) || m.eq_ignore_ascii_case(AUTO_AGENT)
}

#[allow(dead_code)]
pub fn resolve_video_model_choice(
    pinned: Option<&str>,
    route: &RouteDecision,
    configured_default: &str,
) -> String {
    resolve_video_model_choice_prefs(
        pinned,
        route,
        configured_default,
        &PrivacyPrefs::default(),
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
}

pub fn resolve_video_model_choice_zdr(
    pinned: Option<&str>,
    route: &RouteDecision,
    configured_default: &str,
    zdr: bool,
) -> String {
    let mut privacy = PrivacyPrefs::default();
    privacy.zero_data_retention = zdr;
    resolve_video_model_choice_prefs(
        pinned,
        route,
        configured_default,
        &privacy,
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
}

pub fn resolve_video_model_choice_prefs(
    pinned: Option<&str>,
    route: &RouteDecision,
    configured_default: &str,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> String {
    let auto_default =
        resolve_auto_fallback(AutoFallbackKind::Video, privacy, alignment, fallbacks);
    if let Some(p) = pinned.map(str::trim).filter(|s| !s.is_empty()) {
        if !is_auto_video(p) {
            return crate::openrouter::resolve_video_model(p, &auto_default);
        }
    }
    if let Some(sug) = route
        .image_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !is_auto_video(s) && !is_auto_image(s))
    {
        // Reuse imageModel slot from router JSON only for a real catalog video id.
        // Invented ids like `seedance/t2vdv-1.5` contain the family name and 400.
        let sug_l = sug.to_ascii_lowercase();
        let known_video = sug_l.starts_with("bytedance/seedance")
            || sug_l.starts_with("google/veo")
            || sug_l.contains("/kling")
            || (sug_l.starts_with("minimax/") && sug_l.contains("hailuo"))
            || sug_l.contains("wan-")
            || sug_l.contains("/ltx")
            || sug_l.starts_with("x-ai/grok-imagine-video")
            || sug_l.starts_with("black-forest-labs/flux-3-video")
            || sug_l.starts_with("black-forest-labs/flux-video")
            || sug_l.starts_with("runway/gen-");
        if known_video {
            if privacy_requires_strict(privacy)
                && !sug.starts_with("google/")
                && (sug.contains("seedance") || sug.contains("kling") || sug.contains("hailuo"))
            {
                return crate::openrouter::resolve_video_model(&auto_default, &auto_default);
            }
            return crate::openrouter::resolve_video_model(sug, &auto_default);
        }
    }
    let fallback = if is_auto_video(configured_default) || is_auto_image(configured_default) {
        auto_default.clone()
    } else {
        configured_default.to_string()
    };
    crate::openrouter::resolve_video_model(&fallback, &auto_default)
}

pub fn is_upstream_router(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m == UPSTREAM_ROUTER || m.starts_with("openrouter/auto")
}

pub fn is_auto_agent(model: &str) -> bool {
    let m = model.trim();
    // `openrouter/auto` is an explicit model id, not Freya Auto.
    !is_upstream_router(m)
        && (m.is_empty() || m.eq_ignore_ascii_case(AUTO_AGENT) || m.eq_ignore_ascii_case("auto"))
}

pub fn is_auto_image(model: &str) -> bool {
    let m = model.trim();
    m.is_empty() || m.eq_ignore_ascii_case(AUTO_IMAGE) || m.eq_ignore_ascii_case(AUTO_AGENT)
}

pub fn is_auto_router(model: &str) -> bool {
    let m = model.trim();
    m.is_empty() || m.eq_ignore_ascii_case(AUTO_ROUTER) || m.eq_ignore_ascii_case(AUTO_AGENT)
}

pub fn resolve_router_model(configured: &str) -> String {
    resolve_router_model_prefs(
        configured,
        &PrivacyPrefs::default(),
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
}

pub fn resolve_router_model_prefs(
    configured: &str,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> String {
    if is_auto_router(configured) {
        resolve_auto_fallback(AutoFallbackKind::Router, privacy, alignment, fallbacks)
    } else {
        configured.trim().to_string()
    }
}

pub fn resolve_agent_model(pinned: Option<&str>, route: &RouteDecision) -> Option<String> {
    resolve_agent_model_prefs(
        pinned,
        route,
        &PrivacyPrefs::default(),
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
}

/// When Auto: prefer router suggestion → Privacy/Alignment-aware (configurable) fallback.
pub fn resolve_agent_model_prefs(
    pinned: Option<&str>,
    route: &RouteDecision,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> Option<String> {
    if let Some(p) = pinned.map(str::trim).filter(|s| !s.is_empty()) {
        if !is_auto_agent(p) {
            return Some(p.to_string());
        }
    }
    if let Some(sug) = route
        .agent_model
        .as_deref()
        .and_then(sanitize_router_model_suggestion)
    {
        // Upstream router is opt-in on the picker, never a router suggestion.
        if is_upstream_router(&sug) {
            return Some(resolve_auto_fallback(
                AutoFallbackKind::Agent,
                privacy,
                alignment,
                fallbacks,
            ));
        }
        // Uncensored Auto must not be overridden by a high-refusal suggestion
        // (route used to ask for Llama). A pinned model is returned above.
        if alignment.model_alignment == "uncensored" && !keeps_uncensored_agent(&sug) {
            return Some(resolve_auto_fallback(
                AutoFallbackKind::Agent,
                privacy,
                alignment,
                fallbacks,
            ));
        }
        return Some(sug);
    }
    Some(resolve_auto_fallback(
        AutoFallbackKind::Agent,
        privacy,
        alignment,
        fallbacks,
    ))
}

#[allow(dead_code)]
pub fn resolve_image_model_choice(
    pinned: Option<&str>,
    route: &RouteDecision,
    configured_default: &str,
) -> String {
    resolve_image_model_choice_prefs(
        pinned,
        route,
        configured_default,
        &PrivacyPrefs::default(),
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
}

pub fn resolve_image_model_choice_zdr(
    pinned: Option<&str>,
    route: &RouteDecision,
    configured_default: &str,
    zdr: bool,
) -> String {
    let mut privacy = PrivacyPrefs::default();
    privacy.zero_data_retention = zdr;
    resolve_image_model_choice_prefs(
        pinned,
        route,
        configured_default,
        &privacy,
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
}

pub fn resolve_image_model_choice_prefs(
    pinned: Option<&str>,
    route: &RouteDecision,
    configured_default: &str,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> String {
    let auto_default =
        resolve_auto_fallback(AutoFallbackKind::Image, privacy, alignment, fallbacks);
    if let Some(p) = pinned.map(str::trim).filter(|s| !s.is_empty()) {
        if !is_auto_image(p) {
            return crate::openrouter::resolve_image_model(p, &auto_default);
        }
    }
    if let Some(sug) = route
        .image_model
        .as_deref()
        .and_then(sanitize_router_model_suggestion)
    {
        // When strict privacy is on, ignore non-Google Flux-ish suggestions that often lack ZDR.
        if privacy_requires_strict(privacy)
            && !sug.starts_with("google/")
            && (sug.contains("flux") || sug.contains("seedream") || sug.contains("black-forest"))
        {
            return crate::openrouter::resolve_image_model(&auto_default, &auto_default);
        }
        return crate::openrouter::resolve_image_model(&sug, &auto_default);
    }
    let fallback = if is_auto_image(configured_default) {
        auto_default.clone()
    } else {
        configured_default.to_string()
    };
    crate::openrouter::resolve_image_model(&fallback, &auto_default)
}

/// What the latest message is asking Freya to produce, if anything.
/// Series is still a picture modality; callers use this to pick `run_session`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectMedia {
    None,
    Picture,
    Series,
    Gif,
    Video,
}

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

fn has_word(lower: &str, word: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t == word)
}

fn trim_punct(lower: &str) -> &str {
    lower
        .trim()
        .trim_matches(|c: char| matches!(c, '?' | '!' | '.' | '"' | '\'' | '“' | '”' | '«' | '»'))
}

/// Comma-tag soup or an explicit `<lora:>` prompt. A character name is not this.
fn looks_like_tag_prompt(lower: &str) -> bool {
    if lower.contains("<lora:") || lower.contains("<lyco:") || lower.contains("(lora:") {
        return true;
    }
    if lower.contains('?') {
        return false;
    }
    let commas = lower.matches(',').count();
    if commas < 4 {
        return false;
    }
    let words = lower.split_whitespace().count();
    words >= 6 && commas * 2 >= words
}

/// Whole message is "do that again", not a sentence that happens to say again.
fn is_whole_message_redo(lower: &str) -> bool {
    matches!(
        trim_punct(lower),
        "again"
            | "do it again"
            | "do that again"
            | "another"
            | "another one"
            | "another image"
            | "another picture"
            | "one more"
            | "once more"
            | "regenerate"
            | "redo"
            | "same again"
            | "generate"
            | "generate it"
            | "generate this"
            | "generate again"
            | "make it"
            | "draw it"
            | "draw this"
            | "render it"
    )
}

/// Obvious "help me decide". Not every question, and not a look they want seen.
fn obvious_guidance(lower: &str) -> bool {
    const HOLDS: &[&str] = &[
        "don't generate",
        "dont generate",
        "do not generate",
        "don't draw",
        "do not draw",
        "before we generate",
        "before generating",
        "not yet",
    ];
    if contains_any(lower, HOLDS) {
        return true;
    }
    const ADVICE: &[&str] = &[
        "how does",
        "how do ",
        "how do i",
        "how can ",
        "how would ",
        "how to prompt",
        "how should i",
        "what should i",
        "what should we",
        "which lora",
        "what lora",
        "which model",
        "what model",
        "which checkpoint",
        "what checkpoint",
        "which base",
        "what base",
        "which style",
        "what style",
        "should i use",
        "can you explain",
        "could you explain",
        "help me understand",
        "help me decide",
        "help me choose",
        "help me pick",
        "search civitai",
        "find a lora",
        "find loras",
        "suggest idea",
        "brainstorm",
        "any ideas",
        "any assets",
        "which assets",
        "models that",
        "you able",
        "are you able",
        "did we ",
        "did you ",
        "have we ",
        "have you ",
        "was that generated",
    ];
    if contains_any(lower, ADVICE) {
        return true;
    }
    // Broad fragments only count when the message is not already a render.
    if obvious_still(lower) || obvious_motion(lower).is_some() {
        return false;
    }
    const SOFT: &[&str] = &["using the lora?", "would help"];
    contains_any(lower, SOFT)
}

/// Obvious motion request. The bare words "video" or "gif" are not enough.
fn obvious_motion(lower: &str) -> Option<DirectMedia> {
    let asked = lower.contains('?');
    let gif_imperative = lower.contains("make a gif")
        || lower.contains("generate a gif")
        || lower.contains("animated gif")
        || lower.contains("animate as gif")
        || lower.contains("looping animation");
    let gif_loose = lower.contains("gif of")
        || lower.contains(".gif")
        || (has_word(lower, "gif") && (lower.contains("make ") || lower.contains("generate ")));
    if gif_imperative || (!asked && gif_loose) {
        return Some(DirectMedia::Gif);
    }
    const VIDEO_IMPERATIVE: &[&str] = &[
        "text-to-video",
        "image-to-video",
        "text to video",
        "image to video",
        "make a video",
        "make a clip",
        "generate a video",
        "animate this",
    ];
    const VIDEO_LOOSE: &[&str] = &["video of", "short video", "short clip"];
    if contains_any(lower, VIDEO_IMPERATIVE) || (!asked && contains_any(lower, VIDEO_LOOSE)) {
        Some(DirectMedia::Video)
    } else {
        None
    }
}

fn obvious_series(lower: &str) -> bool {
    if lower.contains('?') {
        return false;
    }
    if contains_any(lower, &["run session", "run a session"]) {
        return true;
    }
    const SERIES: &[&str] = &[
        "series of",
        "image series",
        "picture series",
        "set of images",
        "set of pictures",
        "set of stills",
        "storyboard",
        "sequence of images",
        "sequence of pictures",
    ];
    if !contains_any(lower, SERIES) {
        return false;
    }
    // A bare "series of illustrations…" is a description, not a request to render.
    has_word(lower, "generate")
        || has_word(lower, "draw")
        || has_word(lower, "render")
        || has_word(lower, "paint")
        || lower.contains("make ")
}

/// Obvious "render this now". A character name or a visual noun is not enough.
fn obvious_still(lower: &str) -> bool {
    if is_whole_message_redo(lower)
        || looks_like_tag_prompt(lower)
        || lower.trim().starts_with("prompt:")
    {
        return true;
    }
    const PHRASES: &[&str] = &[
        "draw me",
        "draw a ",
        "draw an ",
        "draw the ",
        "make an image",
        "make a picture",
        "make me an image",
        "make me a picture",
        "make the image",
        "make the picture",
        "generate an image",
        "generate a picture",
        "generate a ",
        "generate an ",
        "generate the image",
        "generate this",
        "generate that",
        "please generate",
        "create an image",
        "create a picture",
        "create me an image",
        "create me a picture",
        "render a ",
        "render an ",
        "paint a ",
        "paint an ",
        "paint me ",
    ];
    contains_any(lower, PHRASES)
}

/// Obvious "help me decide". A scene, outfit, or character description is not
/// this, even when the user never said generate, draw, or make.
pub fn message_is_chat_not_media(message: &str) -> bool {
    let lower = message.trim().to_lowercase();
    !lower.is_empty() && obvious_guidance(&lower)
}

/// Obvious render only. A scene, outfit, or character name is not a render.
pub fn direct_media_ask(message: &str) -> DirectMedia {
    let lower = message.trim().to_lowercase();
    if lower.is_empty() || obvious_guidance(&lower) {
        return DirectMedia::None;
    }
    if let Some(kind) = obvious_motion(&lower) {
        return kind;
    }
    if obvious_series(&lower) {
        return DirectMedia::Series;
    }
    if obvious_still(&lower) {
        return DirectMedia::Picture;
    }
    DirectMedia::None
}

fn modality_for_ask(ask: DirectMedia) -> Option<Modality> {
    match ask {
        DirectMedia::None => None,
        DirectMedia::Picture | DirectMedia::Series => Some(Modality::Picture),
        DirectMedia::Gif => Some(Modality::Gif),
        DirectMedia::Video => Some(Modality::Video),
    }
}

/// Obvious render wins. Obvious guidance stays chat.
/// A scene, outfit, or character description keeps the route model's call.
/// Heuristics do not force that look back to chat.
pub fn coerce_route_to_media_ask(
    mut decision: RouteDecision,
    message: &str,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> RouteDecision {
    let ask = direct_media_ask(message);
    let Some(modality) = modality_for_ask(ask) else {
        if decision.modality != Modality::Chat && message_is_chat_not_media(message) {
            decision.modality = Modality::Chat;
            decision.image_model = None;
            if decision.agent_model.is_none() {
                decision.agent_model = Some(resolve_auto_fallback(
                    AutoFallbackKind::Agent,
                    privacy,
                    alignment,
                    fallbacks,
                ));
            }
            decision.reason = "Guidance: they want help deciding, not a picture yet.".into();
            if !decision.source.contains("chat-guard") {
                decision.source = format!("{}+chat-guard", decision.source);
            }
        }
        return decision;
    };
    if decision.modality != modality {
        decision.modality = modality;
        decision.reason = match ask {
            DirectMedia::Series => "Asked to render a series of stills now.".into(),
            DirectMedia::Picture => "Asked to render a still now.".into(),
            DirectMedia::Gif => "GIF / looping animation wording.".into(),
            DirectMedia::Video => "Video / motion wording.".into(),
            DirectMedia::None => decision.reason,
        };
        if !decision.source.contains("media-ask") {
            decision.source = format!("{}+media-ask", decision.source);
        }
    }
    match modality {
        Modality::Picture if decision.image_model.is_none() => {
            decision.image_model = Some(resolve_auto_fallback(
                AutoFallbackKind::Image,
                privacy,
                alignment,
                fallbacks,
            ));
        }
        Modality::Gif | Modality::Video if decision.image_model.is_none() => {
            decision.image_model = Some(resolve_auto_fallback(
                AutoFallbackKind::Video,
                privacy,
                alignment,
                fallbacks,
            ));
        }
        _ => {}
    }
    decision
}

/// Fast heuristic when the light router model is unavailable.
pub fn infer_modality(message: &str) -> RouteDecision {
    infer_modality_prefs(
        message,
        &PrivacyPrefs::default(),
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
}

pub fn infer_modality_prefs(
    message: &str,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> RouteDecision {
    let ask = direct_media_ask(message);
    let decision = match ask {
        DirectMedia::Gif => RouteDecision {
            modality: Modality::Gif,
            reason: "Heuristic: GIF / looping animation wording (legacy SwarmUI gif-hd).".into(),
            agent_model: None,
            image_model: Some(resolve_auto_fallback(
                AutoFallbackKind::Video,
                privacy,
                alignment,
                fallbacks,
            )),
            source: "heuristic".into(),
        },
        DirectMedia::Video => RouteDecision {
            modality: Modality::Video,
            reason: "Heuristic: video / T2V / I2V wording (legacy SwarmUI video).".into(),
            agent_model: None,
            image_model: Some(resolve_auto_fallback(
                AutoFallbackKind::Video,
                privacy,
                alignment,
                fallbacks,
            )),
            source: "heuristic".into(),
        },
        DirectMedia::Picture => RouteDecision {
            modality: Modality::Picture,
            reason: "Heuristic: asked to render a still now.".into(),
            agent_model: None,
            image_model: Some(resolve_auto_fallback(
                AutoFallbackKind::Image,
                privacy,
                alignment,
                fallbacks,
            )),
            source: "heuristic".into(),
        },
        DirectMedia::Series => RouteDecision {
            modality: Modality::Picture,
            reason: "Heuristic: asked for a series of stills.".into(),
            agent_model: None,
            image_model: Some(resolve_auto_fallback(
                AutoFallbackKind::Image,
                privacy,
                alignment,
                fallbacks,
            )),
            source: "heuristic".into(),
        },
        DirectMedia::None => RouteDecision {
            modality: Modality::Chat,
            reason: "Heuristic: conversational / no media ask.".into(),
            agent_model: Some(resolve_auto_fallback(
                AutoFallbackKind::Agent,
                privacy,
                alignment,
                fallbacks,
            )),
            image_model: None,
            source: "heuristic".into(),
        },
    };
    coerce_route_to_media_ask(decision, message, privacy, alignment, fallbacks)
}

/// Drop router prose / null-ish placeholders that LLMs echo from the prompt
/// (e.g. `"None, letting Freya use Auto default (ZDR-aware)."`).
/// Router suggestions that stay on the uncensored agent row.
/// Llama-instruct, Claude, and GPT are not in that row.
fn keeps_uncensored_agent(id: &str) -> bool {
    let l = id.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "uncensored",
        "dolphin",
        "cydonia",
        "venice",
        "abliterat",
        "cognitivecomputations",
        "thedrummer",
        "wizard-vicuna",
    ];
    MARKERS.iter().any(|m| l.contains(m))
}

/// Which chat model writes a local checkpoint prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEnhancePath {
    /// Alignment allows adult scenes and the user text is sexual.
    pub adult: bool,
    /// Concrete chat model id. Never the raw user one-liner.
    pub model: String,
}

/// Local prompt-sd writer.
/// Ordinary requests use the balanced Auto agent (the normal matrix cell) or
/// an explicit chat pin. Adult requests use the uncensored matrix cell
/// (Cydonia at medium and above, Dolphin Venice at low) unless the pin is
/// already an uncensored-safe model.
pub fn local_enhance_path(
    prompt: &str,
    alignment: &AlignmentPrefs,
    pinned_chat: Option<&str>,
    privacy: &PrivacyPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> LocalEnhancePath {
    let adult = alignment.local_adult_passthrough() && prompt_is_adult_scene(prompt);
    let strict = privacy_requires_strict(privacy);
    let cost = fallbacks.cost_tier.as_str();
    let model = if adult {
        match explicit_chat_pin(pinned_chat) {
            // Adult rewrite only: a pinned Llama, Claude, GPT, or Gemini is
            // moderated, so the uncensored matrix model writes this checkpoint prompt.
            Some(id) if !moderated_chat_family(id) && !is_upstream_router(id) => id.to_string(),
            _ => crate::auto_matrix::model_for("agent", strict, "uncensored", cost),
        }
    } else {
        match explicit_chat_pin(pinned_chat) {
            Some(id) => id.to_string(),
            None => crate::auto_matrix::model_for("agent", strict, "balanced", cost),
        }
    };
    LocalEnhancePath { adult, model }
}

/// Uncensored Auto chat cell for a local adult rewrite (matrix, not the pin).
pub fn local_adult_matrix_model(privacy: &PrivacyPrefs, fallbacks: &AutoFallbacksPrefs) -> String {
    crate::auto_matrix::model_for(
        "agent",
        privacy_requires_strict(privacy),
        "uncensored",
        fallbacks.cost_tier.as_str(),
    )
}

fn explicit_chat_pin(pinned: Option<&str>) -> Option<&str> {
    let p = pinned.map(str::trim).filter(|s| !s.is_empty())?;
    if is_auto_agent(p) {
        None
    } else {
        Some(p)
    }
}

/// Llama, Claude, GPT, and Gemini. Dolphin and Cydonia are not in this set.
fn moderated_chat_family(id: &str) -> bool {
    let l = id.trim().to_ascii_lowercase();
    if l.is_empty() || keeps_uncensored_agent(&l) {
        return false;
    }
    l.contains("llama")
        || l.contains("claude")
        || l.starts_with("anthropic/")
        || l.starts_with("openai/")
        || l.contains("gpt")
        || l.contains("gemini")
}

/// Sexual or adult scene text. Uncensored alignment alone is not a match.
pub fn prompt_is_adult_scene(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    const TERMS: &[&str] = &[
        "nsfw",
        "nude",
        "nudes",
        "naked",
        "topless",
        "bottomless",
        "porn",
        "hentai",
        "erotic",
        "erotica",
        "lewd",
        "ecchi",
        "ahegao",
        "orgasm",
        "orgasmic",
        "penis",
        "vagina",
        "vulva",
        "clitoris",
        "nipple",
        "nipples",
        "areola",
        "genitals",
        "genitalia",
        "intercourse",
        "blowjob",
        "handjob",
        "fellatio",
        "cunnilingus",
        "bondage",
        "bdsm",
        "fetish",
        "lingerie",
        "panties",
        "thong",
        "boobs",
        "cock",
        "cocks",
        "dick",
        "pussy",
        "cum",
        "cumshot",
        "cumming",
        "fucking",
        "fuck",
        "fucked",
        "sex",
        "sexual",
        "sexually",
        "xxx",
        "undressed",
        "stripper",
        "whore",
        "slut",
        "dildo",
        "erection",
        "softcore",
        "rule34",
        "r34",
        "yaoi",
        "incest",
        "milf",
        "anal",
        "orgy",
        "threesome",
        "gangbang",
        "creampie",
        "deepthroat",
        "moaning",
        "making love",
        "clothes off",
        "no clothes",
        "without clothes",
        "spread legs",
        "spread her legs",
        "spread his legs",
        "in the nude",
        "adult scene",
        "adult content",
        "sex scene",
        "blow job",
        "hand job",
        "jerking off",
        "get naked",
        "gets naked",
        "18+",
    ];
    if TERMS.iter().any(|term| contains_term(&l, term)) {
        return true;
    }
    // "breast" / "breasts" / "tits" are ordinary in food and bird names.
    contextual_term(
        &l,
        "breast",
        &["chicken", "turkey", "duck", "goose", "quail"],
        &["pocket", "cancer", "milk"],
    ) || contextual_term(
        &l,
        "breasts",
        &["chicken", "turkey", "duck", "goose", "quail"],
        &[],
    ) || contextual_term(
        &l,
        "tits",
        &[
            "blue",
            "great",
            "crested",
            "bearded",
            "coal",
            "marsh",
            "willow",
            "long-tailed",
        ],
        &[],
    )
}

/// Word-bounded `term` that is not sitting next to an ordinary neighbor.
fn contextual_term(hay: &str, term: &str, deny_prev: &[&str], deny_next: &[&str]) -> bool {
    let bytes = hay.as_bytes();
    let needle = term.as_bytes();
    if needle.is_empty() || needle.len() > bytes.len() {
        return false;
    }
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let end = i + needle.len();
            let after_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                let prev = previous_word(hay, i);
                let next = next_word(hay, end);
                let denied =
                    deny_prev.iter().any(|w| prev == *w) || deny_next.iter().any(|w| next == *w);
                if !denied {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

fn previous_word(hay: &str, at: usize) -> &str {
    let head = hay[..at].trim_end();
    let start = head
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .map(|i| i + 1)
        .unwrap_or(0);
    &head[start..]
}

fn next_word(hay: &str, end: usize) -> &str {
    let tail = hay[end..].trim_start();
    let n = tail
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(tail.len());
    &tail[..n]
}

fn contains_term(hay: &str, term: &str) -> bool {
    let bytes = hay.as_bytes();
    let needle = term.as_bytes();
    if needle.is_empty() || needle.len() > bytes.len() {
        return false;
    }
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let end = i + needle.len();
            let after_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn sanitize_router_model_suggestion(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower == "null"
        || lower == "none"
        || lower == "undefined"
        || lower.starts_with("none,")
        || lower.contains("zdr-aware")
        || lower.contains("letting freya")
        || lower.contains("auto default")
        || lower.contains("freya will apply")
    {
        return None;
    }
    // Real model ids are compact (org/name or path); prose has spaces.
    if s.contains(char::is_whitespace) {
        return None;
    }
    // Strip a mistaken `openrouter/` prefix. Invented slugs
    // (`openrouter/google-gemini-pro-image-1`) become empty and fall through to Auto.
    // A real first-party id (`openrouter/horizon-alpha`) stays.
    let s = crate::openrouter::canonicalize_catalog_id(s);
    if s.is_empty() {
        return None;
    }
    // OpenRouter ids are `provider/model`. Bare tokens (`light`, `ldm-xxl-256`)
    // are router prose, not models — drop them so Auto fallbacks are used.
    if !s.contains('/') {
        return None;
    }
    if is_auto_image(&s) || is_auto_agent(&s) || is_upstream_router(&s) {
        return None;
    }
    Some(s)
}

fn parse_route_json(text: &str) -> Option<RouteDecision> {
    let trimmed = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: Value = serde_json::from_str(trimmed).ok()?;
    let modality = v
        .get("modality")
        .and_then(|x| x.as_str())
        .and_then(Modality::parse)?;
    let reason = v
        .get("reason")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let agent_model = v
        .get("agentModel")
        .and_then(|x| x.as_str())
        .and_then(sanitize_router_model_suggestion);
    let image_model = v
        .get("imageModel")
        .and_then(|x| x.as_str())
        .and_then(sanitize_router_model_suggestion);
    Some(RouteDecision {
        modality,
        reason,
        agent_model,
        image_model,
        source: "router".into(),
    })
}

/// Context injected into the router user blob so Auto picks respect Privacy/Alignment.
pub fn router_prefs_context(privacy: &PrivacyPrefs, alignment: &AlignmentPrefs) -> String {
    format!(
        "Privacy: ZDR={} data_collection={}. Alignment: {}. Prefer concrete provider/model ids that satisfy these (image examples: google/gemini-2.5-flash-image, google/gemini-3-pro-image). Never prefix openrouter/ onto a provider/model id and never invent openrouter/google-gemini-pro-image-1. Safety: moderated high-refusal models (GPT, Claude, Cohere; GPT Image or Google image; Veo for video). Balanced: a capable default, not an uncensored specialist and not the strictest safety model. Uncensored: models whose name or description is uncensored (Dolphin, Venice, Cydonia) or, for image and video, the least-moderated catalog option (Flux, Krea, Grok). Do not suggest Llama-instruct, Claude, or GPT as uncensored. When ZDR or deny-collection is on, stay on ids from the ZDR catalog. No video model is on that catalog.",
        if privacy.zero_data_retention {
            "on"
        } else {
            "off"
        },
        privacy.data_collection,
        alignment.model_alignment
    )
}

/// Classify with the configured light router model; fall back to Privacy/Alignment-aware heuristics.
pub async fn classify(
    or: &OpenRouterClient,
    system: &str,
    user_message: &str,
    recent: &str,
    router_model_configured: &str,
) -> RouteDecision {
    classify_prefs(
        or,
        system,
        user_message,
        recent,
        router_model_configured,
        &PrivacyPrefs::default(),
        &AlignmentPrefs::default(),
        &AutoFallbacksPrefs::default(),
    )
    .await
}

pub async fn classify_prefs(
    or: &OpenRouterClient,
    system: &str,
    user_message: &str,
    recent: &str,
    router_model_configured: &str,
    privacy: &PrivacyPrefs,
    alignment: &AlignmentPrefs,
    fallbacks: &AutoFallbacksPrefs,
) -> RouteDecision {
    let model = resolve_router_model_prefs(router_model_configured, privacy, alignment, fallbacks);
    let prefs_ctx = router_prefs_context(privacy, alignment);
    let mut blob = format!("{user_message}\n\n--- freya prefs ---\n{prefs_ctx}");
    if !recent.trim().is_empty() {
        blob = format!("{blob}\n\n--- recent conversation ---\n{recent}");
    }
    match or.chat(system, &blob, Some(&model)).await {
        Ok(text) => {
            if let Some(mut d) = parse_route_json(&text) {
                if d.reason.is_empty() {
                    d.reason = format!("Routed as {} via {model}", d.modality.as_str());
                }
                return coerce_route_to_media_ask(d, user_message, privacy, alignment, fallbacks);
            }
            tracing::debug!(raw = %text, "router returned non-JSON; using heuristic");
            let mut h = infer_modality_prefs(user_message, privacy, alignment, fallbacks);
            h.source = "heuristic-after-router".into();
            coerce_route_to_media_ask(h, user_message, privacy, alignment, fallbacks)
        }
        Err(e) => {
            tracing::debug!(error = %e, "router model failed; using heuristic");
            infer_modality_prefs(user_message, privacy, alignment, fallbacks)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_gif_not_picture() {
        let d = infer_modality("make a gif of a cat waving");
        assert_eq!(d.modality, Modality::Gif);
    }

    #[test]
    fn heuristic_video() {
        let d = infer_modality("text-to-video of a harbor at dusk");
        assert_eq!(d.modality, Modality::Video);
    }

    #[test]
    fn invented_video_id_falls_back_to_default() {
        let route = RouteDecision {
            modality: Modality::Video,
            reason: "t".into(),
            agent_model: None,
            image_model: Some("seedance/t2vdv-1.5".into()),
            source: "t".into(),
        };
        assert_eq!(
            resolve_video_model_choice_prefs(
                Some(AUTO_IMAGE),
                &route,
                AUTO_IMAGE,
                &PrivacyPrefs::default(),
                &AlignmentPrefs::default(),
                &AutoFallbacksPrefs::default(),
            ),
            DEFAULT_AUTO_VIDEO_MODEL
        );
    }

    #[test]
    fn heuristic_picture() {
        let d = infer_modality("draw me a winged warrior by a waterfall");
        assert_eq!(d.modality, Modality::Picture);
    }

    #[test]
    fn heuristic_chat() {
        let d = infer_modality("what LoRA would fit a Nordic look?");
        assert_eq!(d.modality, Modality::Chat);
    }

    #[test]
    fn heuristic_leaves_a_scene_for_the_route_model() {
        let scene =
            "A cartoon meme of a grumpy cat in a cowboy hat, thick outlines, flat yellow background";
        // No obvious render verb and no obvious advice. The fallback stays chat.
        // A picture call from the route model is kept.
        let d = infer_modality(scene);
        assert_eq!(d.modality, Modality::Chat);
        assert_eq!(direct_media_ask(scene), DirectMedia::None);
        assert!(!message_is_chat_not_media(scene));
        let kept = coerce_route_to_media_ask(
            RouteDecision {
                modality: Modality::Picture,
                reason: "scene meant to be seen".into(),
                agent_model: None,
                image_model: Some("google/gemini-2.5-flash-image".into()),
                source: "router".into(),
            },
            scene,
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(kept.modality, Modality::Picture);
        assert!(kept.image_model.is_some());
        assert!(!kept.source.contains("chat-guard"));
    }

    #[test]
    fn series_of_stills_is_picture() {
        assert_eq!(
            direct_media_ask("a series of illustrations of a lighthouse in a storm"),
            DirectMedia::None
        );
        assert_eq!(
            infer_modality("a series of illustrations of a lighthouse in a storm").modality,
            Modality::Chat
        );
        assert_eq!(
            direct_media_ask("make a series of illustrations of a lighthouse in a storm"),
            DirectMedia::Series
        );
        assert_eq!(
            infer_modality("how does flux work?").modality,
            Modality::Chat
        );
        assert_eq!(
            infer_modality("can you clarify the hair color before we generate?").modality,
            Modality::Chat
        );
        assert!(!message_is_chat_not_media(
            "a series of illustrations of a lighthouse in a storm"
        ));
    }

    #[test]
    fn obvious_guidance_stays_chat_without_an_explicit_render() {
        let chat_turns = [
            "you able to make an image of Frankie Foster?",
            "did we generate an image of Frankie Foster using the lora?",
            "I want to create some placeholder image for this Freya app, any assets \\ models that would help?",
            "how should I prompt Frankie Foster for a night out, and which LoRA?",
            "what should I do for Frankie Foster",
        ];
        let picture = RouteDecision {
            modality: Modality::Picture,
            reason: "model called this a still".into(),
            agent_model: None,
            image_model: Some("google/gemini-2.5-flash-image".into()),
            source: "router".into(),
        };
        for turn in chat_turns {
            assert!(message_is_chat_not_media(turn), "{turn}");
            assert_eq!(infer_modality(turn).modality, Modality::Chat, "{turn}");
            assert_eq!(direct_media_ask(turn), DirectMedia::None, "{turn}");
            let guarded = coerce_route_to_media_ask(
                picture.clone(),
                turn,
                &PrivacyPrefs::default(),
                &AlignmentPrefs::default(),
                &AutoFallbacksPrefs::default(),
            );
            assert_eq!(guarded.modality, Modality::Chat, "{turn}");
            assert!(guarded.image_model.is_none(), "{turn}");
            assert!(guarded.source.contains("chat-guard"), "{turn}");
        }
        // A question that hands over the picture is an obvious render.
        assert_eq!(
            infer_modality("can you make an image of Frankie Foster?").modality,
            Modality::Picture
        );
        for turn in [
            "generate",
            "again",
            "draw this",
            "make it",
            "make the picture",
            "can you draw me a cat?",
            "please generate",
            "generate a cat",
            "generate that",
            "create an image of a harbor",
            "generate an image of Frankie using the lora?",
            "draw me a cat that would help tell the story",
        ] {
            assert_eq!(infer_modality(turn).modality, Modality::Picture, "{turn}");
        }
        assert_eq!(
            direct_media_ask("what gif should I make?"),
            DirectMedia::None
        );
        assert_eq!(
            direct_media_ask("is that video of Frankie any good?"),
            DirectMedia::None
        );
        assert_eq!(
            infer_modality("can you make a gif of a cat?").modality,
            Modality::Gif
        );
        assert_eq!(
            infer_modality("make a video of a cat").modality,
            Modality::Video
        );
    }

    #[test]
    fn ordinary_wording_is_not_an_adult_scene() {
        assert!(!prompt_is_adult_scene("chicken breast on a plate"));
        assert!(!prompt_is_adult_scene("chicken breasts on a plate"));
        assert!(!prompt_is_adult_scene("a pair of blue tits on a branch"));
        assert!(!prompt_is_adult_scene(
            "explicit instructions for the checkpoint install"
        ));
        assert!(!prompt_is_adult_scene(
            "portrait of Yuri in a school uniform"
        ));
        assert!(!prompt_is_adult_scene("hardcore parkour on a rooftop"));
        assert!(!prompt_is_adult_scene("straddling a fence at dusk"));
        assert!(prompt_is_adult_scene("nude woman on a bed, explicit sex"));
        assert!(prompt_is_adult_scene("her breasts, soft light"));
        assert!(prompt_is_adult_scene("close crop of her tits"));
    }

    #[test]
    fn coerce_keeps_picture_for_a_look_they_want_seen() {
        let chat = RouteDecision {
            modality: Modality::Chat,
            reason: "ambiguous".into(),
            agent_model: None,
            image_model: None,
            source: "router".into(),
        };
        let outfit = "Frankie Foster in cozy winter layers, a chunky cream knit sweater, plaid scarf, and beanie over her red hair, holding a coffee cup in a snowy front yard with the foster home behind her.";
        // Heuristic does not invent a render, and does not call the outfit advice.
        assert_eq!(direct_media_ask(outfit), DirectMedia::None);
        assert!(!message_is_chat_not_media(outfit));
        assert_eq!(infer_modality(outfit).modality, Modality::Chat);
        // A character name alone does not force picture.
        assert_eq!(direct_media_ask("Frankie Foster"), DirectMedia::None);
        assert!(!message_is_chat_not_media("Frankie Foster"));
        let name = coerce_route_to_media_ask(
            chat.clone(),
            "Frankie Foster",
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(name.modality, Modality::Chat);

        let described = coerce_route_to_media_ask(
            chat.clone(),
            "An illustration of a red bicycle in the rain, wet street, cinematic lighting",
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(described.modality, Modality::Chat);
        assert!(described.image_model.is_none());

        for look in [
            outfit,
            "An illustration of a red bicycle in the rain, wet street, cinematic lighting",
        ] {
            let wanted = coerce_route_to_media_ask(
                RouteDecision {
                    modality: Modality::Picture,
                    reason: "this is the scene they want seen".into(),
                    agent_model: None,
                    image_model: Some("google/gemini-2.5-flash-image".into()),
                    source: "router".into(),
                },
                look,
                &PrivacyPrefs::default(),
                &AlignmentPrefs::default(),
                &AutoFallbacksPrefs::default(),
            );
            assert_eq!(wanted.modality, Modality::Picture, "{look}");
            assert_eq!(
                wanted.image_model.as_deref(),
                Some("google/gemini-2.5-flash-image"),
                "{look}"
            );
            assert!(!wanted.source.contains("chat-guard"), "{look}");
        }

        let drawn = coerce_route_to_media_ask(
            chat,
            "draw this",
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(drawn.modality, Modality::Picture);
        assert!(drawn.image_model.is_some());
    }

    #[test]
    fn pin_beats_route_suggestion() {
        let route = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: Some("openai/gpt-4.1-mini".into()),
            image_model: Some("google/gemini-2.5-flash-image".into()),
            source: "t".into(),
        };
        assert_eq!(
            resolve_agent_model(Some("anthropic/claude-sonnet-5.5"), &route).as_deref(),
            Some("anthropic/claude-sonnet-5.5")
        );
        assert!(
            resolve_image_model_choice(Some("openai/gpt-image-1-mini"), &route, AUTO_IMAGE)
                .contains("gpt-image")
        );
    }

    #[test]
    fn auto_image_resolves_flux() {
        let route = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: None,
            image_model: None,
            source: "t".into(),
        };
        assert_eq!(
            resolve_image_model_choice(Some(AUTO_IMAGE), &route, AUTO_IMAGE),
            DEFAULT_AUTO_IMAGE_MODEL
        );
    }

    #[test]
    fn auto_image_zdr_prefers_google() {
        let route = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: None,
            image_model: Some("black-forest-labs/flux.2-klein-4b".into()),
            source: "t".into(),
        };
        assert_eq!(
            resolve_image_model_choice_zdr(Some(AUTO_IMAGE), &route, AUTO_IMAGE, true),
            DEFAULT_AUTO_IMAGE_MODEL_ZDR
        );
        assert_eq!(
            resolve_image_model_choice_zdr(Some(AUTO_IMAGE), &route, AUTO_IMAGE, false),
            "black-forest-labs/flux.2-klein-4b"
        );
    }

    #[test]
    fn sanitize_drops_zdr_prose_image_model() {
        assert!(sanitize_router_model_suggestion("ldm-xxl-256").is_none());
        assert!(sanitize_router_model_suggestion("light").is_none());
        assert!(sanitize_router_model_suggestion(
            "None, letting Freya use Auto default (ZDR-aware)."
        )
        .is_none());
        assert!(sanitize_router_model_suggestion("null").is_none());
        assert_eq!(
            sanitize_router_model_suggestion("black-forest-labs/flux.2-klein-4b").as_deref(),
            Some("black-forest-labs/flux.2-klein-4b")
        );
        assert!(sanitize_router_model_suggestion("openrouter/google-gemini-pro-image-1").is_none());
        assert_eq!(
            sanitize_router_model_suggestion("openrouter/google/gemini-3-pro-image").as_deref(),
            Some("google/gemini-3-pro-image")
        );
        assert_eq!(
            sanitize_router_model_suggestion("openrouter/horizon-alpha").as_deref(),
            Some("openrouter/horizon-alpha")
        );
        let first_party = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: Some("openrouter/horizon-alpha".into()),
            image_model: Some("openrouter/horizon-alpha".into()),
            source: "t".into(),
        };
        assert_eq!(
            resolve_agent_model(Some(AUTO_AGENT), &first_party).as_deref(),
            Some("openrouter/horizon-alpha")
        );
        assert_eq!(
            resolve_image_model_choice(Some(AUTO_IMAGE), &first_party, AUTO_IMAGE),
            DEFAULT_AUTO_IMAGE_MODEL
        );
        let route = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: None,
            image_model: Some("None, letting Freya use Auto default (ZDR-aware).".into()),
            source: "t".into(),
        };
        // Prose suggestion ignored → Auto default.
        assert_eq!(
            resolve_image_model_choice_zdr(Some(AUTO_IMAGE), &route, AUTO_IMAGE, false),
            DEFAULT_AUTO_IMAGE_MODEL
        );
    }

    #[test]
    fn parse_route_json_drops_prose_image_model() {
        let d = parse_route_json(
            r#"{"modality":"picture","reason":"gen","agentModel":null,"imageModel":"None, letting Freya use Auto default (ZDR-aware)."}"#,
        )
        .expect("parse");
        assert!(d.image_model.is_none());
    }

    #[test]
    fn alignment_safety_picks_moderated_agent() {
        let privacy = PrivacyPrefs::default();
        let alignment = AlignmentPrefs {
            model_alignment: "safety".into(),
            ..Default::default()
        };
        let fb = AutoFallbacksPrefs::default();
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Agent, &privacy, &alignment, &fb),
            DEFAULT_AUTO_AGENT_MODEL_SAFETY
        );
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Router, &privacy, &alignment, &fb),
            DEFAULT_ROUTER_MODEL_SAFETY
        );
        assert_eq!(DEFAULT_AUTO_AGENT_MODEL_SAFETY, "openai/gpt-5-mini");
        assert_eq!(DEFAULT_ROUTER_MODEL_SAFETY, "openai/gpt-5-mini");
        let uncensored = AlignmentPrefs {
            model_alignment: "uncensored".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Agent, &privacy, &uncensored, &fb),
            "thedrummer/cydonia-24b-v4.1"
        );
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Router, &privacy, &uncensored, &fb),
            "thedrummer/cydonia-24b-v4.1"
        );
        let low = AutoFallbacksPrefs {
            cost_tier: "low".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Agent, &privacy, &uncensored, &low),
            "cognitivecomputations/dolphin-mistral-24b-venice-edition"
        );
        let deny = PrivacyPrefs {
            data_collection: "deny".into(),
            ..Default::default()
        };
        let balanced = AlignmentPrefs::default();
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Image, &deny, &balanced, &fb),
            DEFAULT_AUTO_IMAGE_MODEL_ZDR
        );
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Image, &deny, &alignment, &fb),
            "google/gemini-2.5-flash-image"
        );
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Video, &deny, &balanced, &fb),
            DEFAULT_AUTO_VIDEO_MODEL_ZDR
        );
        assert!(
            !resolve_auto_fallback(AutoFallbackKind::Video, &deny, &uncensored, &fb).is_empty()
        );
    }

    #[test]
    fn user_fallback_overrides_builtin() {
        let privacy = PrivacyPrefs::default();
        let alignment = AlignmentPrefs::default();
        let fb = AutoFallbacksPrefs {
            image: "openai/gpt-image-1".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Image, &privacy, &alignment, &fb),
            "openai/gpt-image-1"
        );
    }

    #[test]
    fn invented_openrouter_image_id_uses_real_catalog_id() {
        let route = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: None,
            image_model: Some("openrouter/google-gemini-pro-image-1".into()),
            source: "t".into(),
        };
        assert_eq!(
            resolve_image_model_choice(Some(AUTO_IMAGE), &route, AUTO_IMAGE),
            DEFAULT_AUTO_IMAGE_MODEL
        );
        assert_eq!(
            resolve_image_model_choice(
                Some("openrouter/google-gemini-pro-image-1"),
                &route,
                AUTO_IMAGE
            ),
            DEFAULT_AUTO_IMAGE_MODEL
        );
        assert_eq!(
            resolve_image_model_choice(Some("google/gemini-3-pro-image"), &route, AUTO_IMAGE),
            "google/gemini-3-pro-image"
        );
        assert_eq!(
            resolve_image_model_choice(
                Some("openrouter/google/gemini-3-pro-image"),
                &route,
                AUTO_IMAGE
            ),
            "google/gemini-3-pro-image"
        );
        let privacy = PrivacyPrefs::default();
        let alignment = AlignmentPrefs::default();
        let fb = AutoFallbacksPrefs {
            image: "openrouter/google-gemini-pro-image-1".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Image, &privacy, &alignment, &fb),
            DEFAULT_AUTO_IMAGE_MODEL
        );
        let d = parse_route_json(
            r#"{"modality":"picture","reason":"gen","agentModel":null,"imageModel":"openrouter/google-gemini-pro-image-1"}"#,
        )
        .expect("parse");
        assert!(d.image_model.is_none());
    }

    #[test]
    fn agent_auto_returns_concrete_fallback() {
        let route = RouteDecision {
            modality: Modality::Chat,
            reason: "t".into(),
            agent_model: None,
            image_model: None,
            source: "t".into(),
        };
        let privacy = PrivacyPrefs::default();
        let alignment = AlignmentPrefs {
            model_alignment: "uncensored".into(),
            ..Default::default()
        };
        let fb = AutoFallbacksPrefs::default();
        assert_eq!(
            resolve_agent_model_prefs(Some(AUTO_AGENT), &route, &privacy, &alignment, &fb)
                .as_deref(),
            Some(DEFAULT_AUTO_AGENT_MODEL_UNCENSORED)
        );
    }

    #[test]
    fn uncensored_auto_ignores_llama_suggestion() {
        let route = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: Some("meta-llama/llama-3.1-8b-instruct".into()),
            image_model: None,
            source: "t".into(),
        };
        let privacy = PrivacyPrefs {
            zero_data_retention: true,
            ..Default::default()
        };
        let alignment = AlignmentPrefs {
            model_alignment: "uncensored".into(),
            ..Default::default()
        };
        let fb = AutoFallbacksPrefs::default();
        assert_eq!(
            resolve_agent_model_prefs(Some("auto"), &route, &privacy, &alignment, &fb).as_deref(),
            Some(DEFAULT_AUTO_AGENT_MODEL_UNCENSORED)
        );
        assert_eq!(
            resolve_agent_model_prefs(
                Some("meta-llama/llama-3.1-8b-instruct"),
                &route,
                &privacy,
                &alignment,
                &fb
            )
            .as_deref(),
            Some("meta-llama/llama-3.1-8b-instruct")
        );
        let cydonia = RouteDecision {
            agent_model: Some("thedrummer/cydonia-24b-v4.1".into()),
            ..route
        };
        assert_eq!(
            resolve_agent_model_prefs(Some("auto"), &cydonia, &privacy, &alignment, &fb).as_deref(),
            Some("thedrummer/cydonia-24b-v4.1")
        );
    }

    #[test]
    fn upstream_router_is_opt_in_and_cost_selects_column() {
        assert_eq!(AUTO_AGENT, "auto");
        assert!(!is_auto_agent(UPSTREAM_ROUTER));
        assert!(is_auto_agent("auto"));
        assert!(!is_auto_router(UPSTREAM_ROUTER));
        let privacy = PrivacyPrefs::default();
        let alignment = AlignmentPrefs::default();
        let fb = AutoFallbacksPrefs::default();
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Router, &privacy, &alignment, &fb),
            DEFAULT_ROUTER_MODEL
        );
        assert_ne!(
            resolve_auto_fallback(AutoFallbackKind::Agent, &privacy, &alignment, &fb),
            UPSTREAM_ROUTER
        );
        let low = AutoFallbacksPrefs {
            cost_tier: "low".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Router, &privacy, &alignment, &low),
            "meta-llama/llama-3.1-8b-instruct"
        );
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Image, &privacy, &alignment, &low),
            "bytedance-seed/seedream-5-0-flash"
        );
        let route = RouteDecision {
            modality: Modality::Chat,
            reason: "t".into(),
            agent_model: Some(UPSTREAM_ROUTER.into()),
            image_model: None,
            source: "t".into(),
        };
        assert_eq!(
            resolve_agent_model_prefs(Some("auto"), &route, &privacy, &alignment, &fb).as_deref(),
            Some(DEFAULT_AUTO_AGENT_MODEL)
        );
        assert_eq!(
            resolve_agent_model_prefs(Some(UPSTREAM_ROUTER), &route, &privacy, &alignment, &fb)
                .as_deref(),
            Some(UPSTREAM_ROUTER)
        );
        let strict = PrivacyPrefs {
            zero_data_retention: true,
            ..Default::default()
        };
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Agent, &strict, &alignment, &fb),
            DEFAULT_AUTO_AGENT_MODEL_ZDR
        );
        assert_eq!(
            resolve_auto_fallback(AutoFallbackKind::Image, &strict, &alignment, &fb),
            DEFAULT_AUTO_IMAGE_MODEL_ZDR
        );
        assert_eq!(DEFAULT_AUTO_AGENT_MODEL, DEFAULT_AUTO_AGENT_MODEL_ZDR);
        assert_eq!(DEFAULT_ROUTER_MODEL, DEFAULT_ROUTER_MODEL_ZDR);
    }

    fn align(alignment: &str, nsfw: bool) -> AlignmentPrefs {
        AlignmentPrefs {
            model_alignment: alignment.into(),
            allow_nsfw_local: nsfw,
            ..Default::default()
        }
        .normalize()
    }

    fn tier(cost: &str) -> AutoFallbacksPrefs {
        AutoFallbacksPrefs {
            cost_tier: cost.into(),
            ..Default::default()
        }
    }

    #[test]
    fn local_enhance_path_ordinary_vs_adult() {
        let privacy = PrivacyPrefs::default();
        let medium = tier("medium");
        let low = tier("low");
        let unc = align("uncensored", false);
        let show = align("safety", true);
        let balanced = align("balanced", false);
        let scene = "nude woman on a bed, explicit sex";
        let barn = "a red barn at sunset";

        let ordinary = local_enhance_path(barn, &unc, Some("auto"), &privacy, &medium);
        assert!(!ordinary.adult);
        assert_eq!(ordinary.model, "deepseek/deepseek-v4-pro");

        let ordinary_low = local_enhance_path(barn, &unc, None, &privacy, &low);
        assert!(!ordinary_low.adult);
        assert_eq!(ordinary_low.model, "meta-llama/llama-3.1-8b-instruct");

        let show_landscape = local_enhance_path(
            "portrait of a scientist in a lab coat",
            &show,
            Some("auto"),
            &privacy,
            &medium,
        );
        assert!(!show_landscape.adult);
        assert_eq!(show_landscape.model, "deepseek/deepseek-v4-pro");

        let not_adult_words = local_enhance_path(
            "essex countryside, cocktail on the patio, structural analysis",
            &unc,
            None,
            &privacy,
            &medium,
        );
        assert!(!not_adult_words.adult);
        assert_eq!(not_adult_words.model, "deepseek/deepseek-v4-pro");

        let adult = local_enhance_path(scene, &unc, Some("auto"), &privacy, &medium);
        assert!(adult.adult);
        assert_eq!(adult.model, "thedrummer/cydonia-24b-v4.1");

        let adult_low = local_enhance_path(scene, &show, None, &privacy, &low);
        assert!(adult_low.adult);
        assert_eq!(
            adult_low.model,
            "cognitivecomputations/dolphin-mistral-24b-venice-edition"
        );
        assert_eq!(adult_low.model, local_adult_matrix_model(&privacy, &low));

        let adult_high = local_enhance_path(scene, &unc, None, &privacy, &tier("high"));
        assert_eq!(adult_high.model, "thedrummer/cydonia-24b-v4.1");

        // Sexual text without Uncensored or Show NSFW stays on the baseline writer.
        let gated = local_enhance_path(scene, &balanced, None, &privacy, &medium);
        assert!(!gated.adult);
        assert_eq!(gated.model, "deepseek/deepseek-v4-pro");

        let pinned = local_enhance_path(
            barn,
            &unc,
            Some("mistralai/mistral-small"),
            &privacy,
            &medium,
        );
        assert!(!pinned.adult);
        assert_eq!(pinned.model, "mistralai/mistral-small");

        for moderated in [
            "meta-llama/llama-3.1-8b-instruct",
            "anthropic/claude-sonnet-5.5",
            "openai/gpt-5-mini",
            "google/gemini-4-neon",
        ] {
            let path = local_enhance_path(scene, &unc, Some(moderated), &privacy, &medium);
            assert!(path.adult, "{moderated}");
            assert_eq!(path.model, "thedrummer/cydonia-24b-v4.1", "{moderated}");
        }

        let dolphin_pin = local_enhance_path(
            scene,
            &unc,
            Some("cognitivecomputations/dolphin-mistral-24b-venice-edition"),
            &privacy,
            &medium,
        );
        assert!(dolphin_pin.adult);
        assert_eq!(
            dolphin_pin.model,
            "cognitivecomputations/dolphin-mistral-24b-venice-edition"
        );
    }
}
