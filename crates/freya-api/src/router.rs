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
        // When strict privacy is on, ignore non-Google Flux-ish image/video overrides.
        if strict {
            match kind {
                AutoFallbackKind::Image
                    if !user.starts_with("google/")
                        && (user.contains("flux")
                            || user.contains("seedream")
                            || user.contains("black-forest")) =>
                {
                    return builtin_auto_fallback(kind, privacy, alignment, &fallbacks.cost_tier)
                        .to_string();
                }
                AutoFallbackKind::Video
                    if !user.starts_with("google/")
                        && (user.contains("seedance")
                            || user.contains("kling")
                            || user.contains("hailuo")
                            || user.contains("flux")) =>
                {
                    return builtin_auto_fallback(kind, privacy, alignment, &fallbacks.cost_tier)
                        .to_string();
                }
                _ => {}
            }
        }
        return user.to_string();
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
    let auto_default = resolve_auto_fallback(AutoFallbackKind::Video, privacy, alignment, fallbacks);
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
    let auto_default = resolve_auto_fallback(AutoFallbackKind::Image, privacy, alignment, fallbacks);
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

fn looks_like_tag_prompt(lower: &str) -> bool {
    if lower.contains("<lora:") || lower.contains("<lyco:") || lower.contains("(lora:") {
        return true;
    }
    let commas = lower.matches(',').count();
    commas >= 3 && !lower.contains('?')
}

fn visual_cue_count(lower: &str) -> usize {
    const CUES: &[&str] = &[
        "cartoon",
        "meme",
        "illustration",
        "illustrated",
        "painting",
        "portrait",
        "sketch",
        "artwork",
        "comic",
        "anime",
        "watercolor",
        "oil painting",
        "background",
        "lighting",
        "wearing",
        "standing",
        "sitting",
        "close-up",
        "close up",
        "wide shot",
        "cinematic",
        "linework",
        "line art",
        "landscape",
        "sunset",
        "moonlight",
        "waterfall",
        "armor",
        "wings",
        "cowboy",
        "outline",
        "studio light",
        "digital art",
        "concept art",
        "photograph",
        "in the style",
        "masterpiece",
        "highly detailed",
        "1girl",
        "1boy",
        "photo of",
        "picture of",
        "image of",
        "drawing of",
        "shot of",
    ];
    CUES.iter().filter(|c| lower.contains(*c)).count()
}

/// Questions, how-it-works, and LoRA/model clarification — not a prompt to render.
pub fn message_is_chat_not_media(message: &str) -> bool {
    let lower = message.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if looks_like_tag_prompt(&lower) && !lower.trim_end().ends_with('?') {
        return false;
    }
    const PHRASES: &[&str] = &[
        "how does",
        "how do ",
        "how can ",
        "how would ",
        "what is ",
        "what are ",
        "what was ",
        "what's ",
        "whats ",
        "why does",
        "why is ",
        "why do ",
        "explain ",
        "tell me how",
        "tell me about",
        "tell me what",
        "which lora",
        "what lora",
        "which model",
        "what model",
        "which checkpoint",
        "what checkpoint",
        "which base",
        "what base",
        "clarify",
        "before we generate",
        "before generating",
        "don't generate",
        "do not generate",
        "not yet",
        "how it works",
        "how this works",
        "what should i",
        "should i use",
        "can you explain",
        "could you explain",
        "help me understand",
        "search civitai",
        "find a lora",
        "find loras",
        "suggest idea",
        "brainstorm",
        "list lora",
        "list checkpoint",
        "worker status",
    ];
    if contains_any(&lower, PHRASES) {
        return true;
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    let first = words.first().copied().unwrap_or("");
    if matches!(
        first,
        "who" | "what" | "why" | "how" | "when" | "where" | "which" | "whose"
    ) && !(first == "how" && words.get(1) == Some(&"about"))
        && (lower.trim_end().ends_with('?') || words.len() < 14)
    {
        return true;
    }
    if words.len() < 24
        && (lower.starts_with("can you ")
            || lower.starts_with("could you ")
            || lower.starts_with("would you ")
            || lower.starts_with("please explain"))
    {
        let imperative = contains_any(
            &lower,
            &[
                "draw", "generat", "illustrat", "render", "paint", "video", "gif", "animat",
                "picture", "image",
            ],
        );
        if !imperative {
            return true;
        }
    }
    false
}

fn looks_like_prose_scene(lower: &str) -> bool {
    let stripped = lower.trim().trim_matches(|c: char| {
        matches!(c, '"' | '\'' | '“' | '”' | '«' | '»')
    });
    if stripped.contains('?') {
        return false;
    }
    let words: Vec<&str> = stripped.split_whitespace().collect();
    if words.len() < 6 {
        return false;
    }
    let cues = visual_cue_count(stripped);
    if cues >= 2 && words.len() >= 6 {
        return true;
    }
    if cues >= 1 && words.len() >= 8 {
        return true;
    }
    let starts_scene = stripped.starts_with("a ")
        || stripped.starts_with("an ")
        || stripped.starts_with("prompt:")
        || stripped.starts_with("\"")
        || stripped.starts_with("“");
    starts_scene && words.len() >= 10 && (stripped.contains(',') || stripped.contains(" with "))
}

/// Classify a message as media to produce, without requiring the word "generate".
pub fn direct_media_ask(message: &str) -> DirectMedia {
    let lower = message.trim().to_lowercase();
    if lower.is_empty() || message_is_chat_not_media(message) {
        return DirectMedia::None;
    }
    let has_gif_token = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t == "gif")
        || lower.contains(".gif")
        || lower.contains("animated gif");
    if has_gif_token
        || lower.contains("looping animation")
        || lower.contains("animated meme")
        || lower.contains("short animation")
        || lower.contains("animate as gif")
    {
        return DirectMedia::Gif;
    }
    const VIDEO: &[&str] = &[
        "video",
        "t2v",
        "i2v",
        "text-to-video",
        "image-to-video",
        "text to video",
        "image to video",
        "make a clip",
        "generate a clip",
        "motion clip",
        "animate this into",
        "short clip",
        "wan 2",
        "ltx video",
        "mp4",
        "webm",
    ];
    if contains_any(&lower, VIDEO)
        || (lower.contains("animat") && !lower.contains("still") && !has_gif_token)
    {
        return DirectMedia::Video;
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
        "several versions",
        "a few versions",
        "a few images",
        "a few pictures",
        "several images",
        "several pictures",
        "variations",
        "run session",
        "run a session",
        "batch of",
    ];
    if contains_any(&lower, SERIES) {
        return DirectMedia::Series;
    }
    const PICTURE: &[&str] = &[
        "generate",
        "make an image",
        "make me an image",
        "create an image",
        "make a picture",
        "create a picture",
        "draw ",
        "draw me",
        "render ",
        "paint ",
        "produce an image",
        "still image",
        "illustration",
        "generate this",
        "image prompt",
    ];
    if contains_any(&lower, PICTURE) || lower.starts_with("prompt:") {
        return DirectMedia::Picture;
    }
    if looks_like_tag_prompt(&lower) || looks_like_prose_scene(&lower) {
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

/// Force picture/gif/video when the text is a prompt, and chat when it is a question.
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
            decision.reason = "Chat: question, how-it-works, or LoRA/character clarification.".into();
            if !decision.source.contains("chat-guard") {
                decision.source = format!("{}+chat-guard", decision.source);
            }
        }
        return decision;
    };
    if decision.modality != modality {
        decision.modality = modality;
        decision.reason = match ask {
            DirectMedia::Series => {
                "Visual series prompt — stills, without requiring the word generate.".into()
            }
            DirectMedia::Picture => {
                "Pasted or described still — picture, without requiring the word generate.".into()
            }
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
            reason: "Heuristic: still prompt (scene, meme, illustration, or quoted prompt).".into(),
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
            reason: "Heuristic: series of stills.".into(),
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
    // OpenRouter ids are `org/name`. Bare tokens (`light`, `ldm-xxl-256`, `chat-0.4.0`)
    // are router prose, not models — drop them so Auto fallbacks are used.
    if !s.contains('/') {
        return None;
    }
    if is_auto_image(s) || is_auto_agent(s) || is_upstream_router(s) {
        return None;
    }
    Some(s.to_string())
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
        "Privacy: ZDR={} data_collection={}. Alignment: {}. Prefer concrete OpenRouter model ids that satisfy these. Safety: moderated high-refusal models (GPT, Claude, Cohere; GPT Image or Google image; Veo for video). Balanced: a capable default, not an uncensored specialist and not the strictest safety model. Uncensored: models whose name or description is uncensored (Dolphin, Venice, Cydonia) or, for image and video, the least-moderated catalog option (Flux, Krea, Grok). Do not suggest Llama-instruct, Claude, or GPT as uncensored. When ZDR or deny-collection is on, stay on ids from the ZDR catalog. No video model is on that catalog.",
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
                return coerce_route_to_media_ask(
                    d,
                    user_message,
                    privacy,
                    alignment,
                    fallbacks,
                );
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
    fn pasted_scene_is_picture_without_generate() {
        let d = infer_modality(
            "A cartoon meme of a grumpy cat in a cowboy hat, thick outlines, flat yellow background",
        );
        assert_eq!(d.modality, Modality::Picture);
        assert_eq!(
            direct_media_ask(
                "A cartoon meme of a grumpy cat in a cowboy hat, thick outlines, flat yellow background"
            ),
            DirectMedia::Picture
        );
    }

    #[test]
    fn series_and_questions_stay_distinct() {
        assert_eq!(
            direct_media_ask("a series of illustrations of a lighthouse in a storm"),
            DirectMedia::Series
        );
        assert_eq!(
            infer_modality("a series of illustrations of a lighthouse in a storm").modality,
            Modality::Picture
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
    fn coerce_overrides_chat_router_for_visual_prompt() {
        let chat = RouteDecision {
            modality: Modality::Chat,
            reason: "ambiguous".into(),
            agent_model: None,
            image_model: None,
            source: "router".into(),
        };
        let d = coerce_route_to_media_ask(
            chat,
            "An illustration of a red bicycle in the rain, wet street, cinematic lighting",
            &PrivacyPrefs::default(),
            &AlignmentPrefs::default(),
            &AutoFallbacksPrefs::default(),
        );
        assert_eq!(d.modality, Modality::Picture);
        assert!(d.image_model.is_some());
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
        assert!(resolve_image_model_choice(
            Some("openai/gpt-image-1-mini"),
            &route,
            AUTO_IMAGE
        )
        .contains("gpt-image"));
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
        let route = RouteDecision {
            modality: Modality::Picture,
            reason: "t".into(),
            agent_model: None,
            image_model: Some(
                "None, letting Freya use Auto default (ZDR-aware).".into(),
            ),
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
