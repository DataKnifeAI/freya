use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylePack {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub example_prompt: String,
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_router_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_checkpoint: Option<String>,
    #[serde(default)]
    pub loras: Vec<StyleLora>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_words: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleLora {
    pub name: String,
    #[serde(default = "one")]
    pub scale: f32,
}

fn one() -> f32 {
    1.0
}

fn default_backend() -> String {
    "openrouter".into()
}

pub async fn load_styles(packs_dir: &Path) -> Result<Vec<StylePack>> {
    let dir = packs_dir.join("styles");
    if !dir.exists() {
        return Ok(builtin_styles());
    }
    let mut out = Vec::new();
    let mut rd = fs::read_dir(&dir).await?;
    while let Some(ent) = rd.next_entry().await? {
        let path = ent.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("yaml") && ext != Some("yml") {
            continue;
        }
        let raw = fs::read_to_string(&path).await?;
        if let Ok(s) = serde_yaml::from_str::<StylePack>(&raw) {
            out.push(s);
        }
    }
    if out.is_empty() {
        return Ok(builtin_styles());
    }
    // Featured brand pack first so UI "Use example" matches freya.png NL seed.
    out.sort_by(|a, b| {
        let rank = |id: &str| if id == "freya-winged-warrior" { 0 } else { 1 };
        rank(&a.id).cmp(&rank(&b.id)).then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

/// Assist `mode` must be a single path segment (no `/`, `..`, or absolute paths).
pub fn is_safe_instruction_mode(mode: &str) -> bool {
    !mode.is_empty()
        && mode.len() <= 64
        && mode
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Modes exposed in Settings for view/edit (packs/instructions defaults + overrides).
pub const EDITABLE_INSTRUCTION_MODES: &[&str] = &[
    "converse",
    "route",
    "chat",
    "plan",
    "suggest",
    "session",
    "prompt-prose",
    "prompt-sd",
];

pub fn is_editable_instruction_mode(mode: &str) -> bool {
    is_safe_instruction_mode(mode) && EDITABLE_INSTRUCTION_MODES.contains(&mode)
}

pub fn instruction_mode_label(mode: &str) -> String {
    match mode {
        "converse" => "Converse (chat agent)".into(),
        "route" => "Route (intent classifier)".into(),
        "chat" => "Chat (assist)".into(),
        "plan" => "Plan".into(),
        "suggest" => "Suggest".into(),
        "session" => "Session".into(),
        "prompt-prose" => "Enhance — prose brief".into(),
        "prompt-sd" => "Enhance — SD keywords".into(),
        _ => mode.to_string(),
    }
}

/// Pack / builtin default only (ignores Settings overrides).
pub async fn load_instruction_default(packs_dir: &Path, mode: &str) -> String {
    if !is_safe_instruction_mode(mode) {
        return builtin_instruction("chat").to_string();
    }
    let dir = packs_dir.join("instructions");
    let path = dir.join(format!("{mode}.txt"));
    // Containment: refuse anything that resolves outside the instructions dir.
    if let (Ok(dir_c), Ok(path_c)) = (fs::canonicalize(&dir).await, fs::canonicalize(&path).await) {
        if path_c.starts_with(&dir_c) {
            if let Ok(s) = fs::read_to_string(path_c).await {
                return s;
            }
        }
    } else if let Ok(s) = fs::read_to_string(&path).await {
        // Dir may not exist yet — safe basename already enforced above.
        return s;
    }
    builtin_instruction(mode).to_string()
}

/// Prefer a non-empty Settings override when present; else pack file / builtin.
pub async fn load_instruction(
    packs_dir: &Path,
    mode: &str,
    overrides: Option<&std::collections::HashMap<String, String>>,
) -> String {
    if let Some(map) = overrides {
        if let Some(text) = map.get(mode) {
            if !text.trim().is_empty() {
                return text.clone();
            }
        }
    }
    load_instruction_default(packs_dir, mode).await
}

fn builtin_styles() -> Vec<StylePack> {
    // NL examples (not SD tag soup). First entry seeds UI "Use example" — freya.png brand art.
    vec![
        StylePack {
            id: "freya-winged-warrior".into(),
            display_name: "Freya (brand art)".into(),
            tags: vec!["character".into(), "fantasy".into(), "wings".into()],
            example_prompt: "A young woman with large brown eagle wings and fantasy leather armor, standing in a green canyon with a waterfall behind her".into(),
            backend: "openrouter".into(),
            open_router_model: Some("black-forest-labs/flux.2-klein-4b".into()),
            local_checkpoint: None,
            loras: vec![],
            trigger_words: None,
            recommended_size: Some("1024x1024".into()),
            thumbnail_url: None,
        },
        StylePack {
            id: "ethereal-moon".into(),
            display_name: "Ethereal Moon Landscape".into(),
            tags: vec!["landscape".into(), "fantasy".into(), "moon".into()],
            example_prompt: "A mystical landscape under a radiant red moon — a twisted blossoming tree on rocky ground, misty mountains beyond a lake that mirrors the moonlight".into(),
            backend: "openrouter".into(),
            open_router_model: Some("black-forest-labs/flux.2-klein-4b".into()),
            local_checkpoint: None,
            loras: vec![],
            trigger_words: None,
            recommended_size: Some("1024x1024".into()),
            thumbnail_url: None,
        },
        StylePack {
            id: "anime-portrait".into(),
            display_name: "Anime Portrait (cloud)".into(),
            tags: vec!["anime".into(), "portrait".into()],
            example_prompt: "An anime-style portrait with soft lighting, expressive eyes, and clean colorful linework".into(),
            backend: "openrouter".into(),
            open_router_model: Some("black-forest-labs/flux.2-klein-4b".into()),
            local_checkpoint: None,
            loras: vec![],
            trigger_words: None,
            recommended_size: Some("768x1024".into()),
            thumbnail_url: None,
        },
    ]
}

pub fn builtin_instruction(mode: &str) -> &'static str {
    match mode {
        "chat" => {
            "You are Freya, a friendly image studio guide. Users describe scenes in everyday language — you handle model briefs, LoRA discovery, and planning. Prefer Installed base+LoRA combos before Civitai search; ask a short follow-up when character likeness is ambiguous. Never teach comma-separated tags unless a selected LoRA needs one trigger word. Help them iterate: suggest what to try next, not how to prompt engineer."
        }
        "converse" => {
            "You are Freya, a friendly agentic image studio guide. Reply JSON only: {\"reply\":\"...\",\"actions\":[...]}. Respect freya route modality and automationMode (plan|auto|yolo). If modality is picture, gif, or video, or the message is a visual prompt (scene, meme, cartoon, illustration, quoted prompt, series), generate — the word generate is optional. Questions, LoRA/model clarification, and how-it-works stay chat with no generate. Prefer list_loras/list_checkpoints (Installed) before search_civitai. Session pins are for LoRAs/adapters/embeddings only — never pin Checkpoints; use set_base_model for active base. install_asset queues a download and does not auto-pin. Plan: propose side effects. Auto: act on clear intent, ask when installs are ambiguous. YOLO: pick and go."
        }
        "vision" => {
            "You are Freya. Describe the image in first person as if you are inside the scene. Use \"I see\", \"I notice\". 150–300 words. Output only the description."
        }
        "caption" => {
            "You are Freya. Describe the image in third person: subjects, setting, composition, style, lighting. Plain prose, 150–300 words. Output only the caption."
        }
        "prompt" | "prompt-sd" => {
            "You are Freya. Output ONLY an optimized positive prompt for Stable Diffusion. One continuous comma-separated keyword list. Keep trigger words and underscores intact. No negative prompt. No preamble."
        }
        "prompt-prose" => {
            "You are Freya. Expand the user's natural-language description into a rich image brief for modern models: one continuous paragraph of vivid prose. Do NOT output comma-separated Stable Diffusion keyword lists. Output ONLY the prose brief."
        }
        "plan" => {
            "You are Freya's planner for agentic image sessions. Reply with JSON only: {\"backend\":\"openrouter\"|\"local\",\"model\":null|\"provider/model\",\"styleId\":null|\"...\",\"enhanceMode\":\"prompt-prose\"|\"prompt-sd\",\"civitaiQuery\":null|\"short search\",\"rationale\":\"short\"}. Prefer openrouter + prompt-prose. model is a real provider/model image id (black-forest-labs/flux.2-klein-4b, google/gemini-2.5-flash-image, or google/gemini-3-pro-image) or null. Never prefix openrouter/ onto an id. Never use openrouter/auto, chat-only ids, or invented ids such as openrouter/google-gemini-pro-image-1. Use local + prompt-sd for pinned/Installed LoRAs; set civitaiQuery only when Installed has no match."
        }
        "suggest" => {
            "You are Freya. Reply with JSON only: {\"ideas\":[\"nl alt 1\",\"nl alt 2\"],\"civitaiQuery\":\"short LoRA search or empty\",\"sessionNote\":\"what to try next\"}. Ideas are everyday prose, never SD tags. civitaiQuery is 2–6 words for Civitai when weights would help."
        }
        "session" => {
            "You are Freya running an agentic image session. Reply JSON only: {\"goal\":\"...\",\"steps\":[{\"kind\":\"search_loras\"|\"generate\"|\"iterate\",\"prompt\":\"nl\",\"note\":\"why\"}],\"civitaiQuery\":\"short or empty\",\"batchPrompts\":[\"nl 1\",\"nl 2\",\"nl 3\"],\"sessionNote\":\"next\"}. Prefer Installed weights; search Civitai only if needed. Clarify ambiguous character likeness before heavy search/generate. Never SD keyword soup."
        }
        "route" => {
            "You are Freya's internal request router. Classify into chat|picture|gif|video. A pasted visual prompt (scene, meme, cartoon, illustration, quoted prompt, tag list, series of stills) is picture even without the word generate. gif/video when they describe motion. Chat only for questions, how-it-works, and LoRA/model clarification. Respect Privacy/Alignment/cost. Under uncensored leave agentModel null so Auto uses Dolphin or Cydonia. Do not suggest Llama-instruct, Claude, or GPT as uncensored. Suggest concrete provider/model ids (image: google/gemini-2.5-flash-image or google/gemini-3-pro-image). Never prefix openrouter/ onto an id. Never suggest openrouter/auto or invented ids such as openrouter/google-gemini-pro-image-1. Reply JSON only: {\"modality\":\"...\",\"reason\":\"...\",\"agentModel\":null,\"imageModel\":null}."
        }
        _ => "You are Freya, a helpful image studio agent.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn instruction_mode_rejects_path_traversal() {
        assert!(is_safe_instruction_mode("chat"));
        assert!(is_safe_instruction_mode("prompt-prose"));
        assert!(!is_safe_instruction_mode("../etc/passwd"));
        assert!(!is_safe_instruction_mode("/etc/passwd"));
        assert!(!is_safe_instruction_mode("foo/bar"));
        assert!(!is_safe_instruction_mode(""));
    }

    #[tokio::test]
    async fn load_instruction_ignores_unsafe_mode() {
        let packs = PathBuf::from("/nonexistent-packs-dir");
        let text = load_instruction(&packs, "../../etc/passwd", None).await;
        assert_eq!(text, builtin_instruction("chat"));
    }

    #[tokio::test]
    async fn load_instruction_prefers_override() {
        let packs = PathBuf::from("/nonexistent-packs-dir");
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("chat".into(), "Custom chat system prompt.".into());
        let text = load_instruction(&packs, "chat", Some(&overrides)).await;
        assert_eq!(text, "Custom chat system prompt.");
        let empty = load_instruction(&packs, "chat", None).await;
        assert_eq!(empty, builtin_instruction("chat"));
    }

    #[test]
    fn builtin_styles_feature_freya_brand_nl() {
        let styles = builtin_styles();
        assert_eq!(styles[0].id, "freya-winged-warrior");
        assert!(styles[0].example_prompt.contains("eagle wings"));
        // Prose NL, not SD tag soup (no weight syntax).
        assert!(!styles[0].example_prompt.contains(":1."));
    }
}
