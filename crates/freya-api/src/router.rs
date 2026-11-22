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
        "would help",
        "models that",
        "you able",
        "are you able",
        "did we ",
        "did you ",
        "have we ",
        "have you ",
        "was that generated",
        "using the lora?",
    ];
    contains_any(lower, ADVICE)
}

/// Obvious motion request. The bare words "video" or "gif" are not enough.
fn obvious_motion(lower: &str) -> Option<DirectMedia> {
    let gif = lower.contains("gif of")
        || lower.contains("make a gif")
        || lower.contains("animated gif")
        || lower.contains(".gif")
        || lower.contains("looping animation")
        || lower.contains("animate as gif")
        || (has_word(lower, "gif") && (lower.contains("make ") || lower.contains("generate ")));
    if gif {
        return Some(DirectMedia::Gif);
    }
    const VIDEO: &[&str] = &[
        "text-to-video",
        "image-to-video",
        "text to video",
        "image to video",
        "make a video",
        "make a clip",
        "video of",
        "short video",
        "animate this",
        "short clip",
    ];
    if contains_any(lower, VIDEO) {
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
        "generate the image",
        "generate this",
        "render a ",
        "render an ",
        "paint a ",
        "paint an ",
        "paint me ",
    ];
    contains_any(lower, PHRASES)
}

/// No request to render yet. Bare scenes and ambiguous turns are chat,
/// even when the route model called them picture.
pub fn message_is_chat_not_media(message: &str) -> bool {
    let lower = message.trim().to_lowercase();
    !lower.is_empty() && direct_media_ask(message) == DirectMedia::None
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

/// Override the route model: an obvious render becomes that modality.
/// Anything else, including a visual description, stays chat.
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
    // Drop a mistaken `openrouter/` prefix. Invented slugs
    // (`openrouter/google-gemini-pro-image-1`) become empty and fall through to Auto.
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
    if s.to_ascii_lowercase().starts_with("openrouter/") {
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
    fn pasted_scene_without_render_stays_chat() {
        let scene =
            "A cartoon meme of a grumpy cat in a cowboy hat, thick outlines, flat yellow background";
        let d = infer_modality(scene);
        assert_eq!(d.modality, Modality::Chat);
        assert_eq!(direct_media_ask(scene), DirectMedia::None);
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
    }

    #[test]
    fn advice_stays_chat_until_they_ask_to_render() {
        let chat_turns = [
            "you able to make an image of Frankie Foster?",
            "did we generate an image of Frankie Foster using the lora?",
            "Frankie Foster in cozy winter layers, a chunky cream knit sweater, plaid scarf, and beanie over her red hair, holding a coffee cup in a snowy front yard with the foster home behind her.",
            "I want to create some placeholder image for this Freya app, any assets \\ models that would help?",
            "how should I prompt Frankie Foster for a night out, and which LoRA?",
        ];
        for turn in chat_turns {
            assert_eq!(infer_modality(turn).modality, Modality::Chat, "{turn}");
            assert_eq!(direct_media_ask(turn), DirectMedia::None, "{turn}");
        }
        for turn in [
            "generate",
            "again",
            "draw this",
            "make it",
            "make the picture",
            "can you draw me a cat?",
        ] {
            assert_eq!(infer_modality(turn).modality, Modality::Picture, "{turn}");
        }
    }

    #[test]
    fn coerce_keeps_chat_for_a_visual_description() {
        let chat = RouteDecision {
            modality: Modality::Chat,
            reason: "ambiguous".into(),
            agent_model: None,
            image_model: None,
            source: "router".into(),
        };
        let described = coerce_route_to_media_ask(
            chat.clone(),
            "An illustration of a red bicycle in the rain, wet street, cinematic lighting",
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(described.modality, Modality::Chat);
        assert!(described.image_model.is_none());

        let wanted = coerce_route_to_media_ask(
            RouteDecision {
                modality: Modality::Picture,
                reason: "this is the scene they want seen".into(),
                agent_model: None,
                image_model: Some("google/gemini-2.5-flash-image".into()),
                source: "router".into(),
            },
            "An illustration of a red bicycle in the rain, wet street, cinematic lighting",
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(wanted.modality, Modality::Chat);
        assert!(wanted.image_model.is_none());
        assert!(wanted.source.contains("chat-guard"));

        let eager = RouteDecision {
            modality: Modality::Picture,
            reason: "scene".into(),
            agent_model: None,
            image_model: Some("google/gemini-2.5-flash-image".into()),
            source: "router".into(),
        };
        let guarded = coerce_route_to_media_ask(
            eager,
            "did we generate an image of Frankie Foster using the lora?",
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(guarded.modality, Modality::Chat);
        assert!(guarded.image_model.is_none());
        assert!(guarded.source.contains("chat-guard"));

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
}
