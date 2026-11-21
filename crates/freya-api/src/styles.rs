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
    Ok(out)
}

pub async fn load_instruction(packs_dir: &Path, mode: &str) -> String {
    let path = packs_dir.join("instructions").join(format!("{mode}.txt"));
    if let Ok(s) = fs::read_to_string(path).await {
        return s;
    }
    builtin_instruction(mode).to_string()
}

fn builtin_styles() -> Vec<StylePack> {
    vec![
        StylePack {
            id: "ethereal-moon".into(),
            display_name: "Ethereal Moon Landscape".into(),
            tags: vec!["landscape".into(), "fantasy".into(), "moon".into()],
            example_prompt: "ethereal mystical landscape under a radiant red moon, twisted blossoming tree on rocky terrain, misty mountains, serene lake reflecting moonlight, magical atmosphere, highly detailed".into(),
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
            example_prompt: "anime style portrait, soft lighting, expressive eyes, detailed hair, clean linework, vibrant colors".into(),
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
            "You are Freya, a friendly image studio agent. Users describe goals in everyday language — you plan, find LoRAs, write model briefs, and iterate like an agentic coding session but for images. Never teach comma-separated tags unless a pinned LoRA needs one trigger word."
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
            "You are Freya's planner for agentic image sessions. Reply with JSON only: {\"backend\":\"openrouter\"|\"local\",\"model\":\"...\",\"styleId\":null|\"...\",\"enhanceMode\":\"prompt-prose\"|\"prompt-sd\",\"civitaiQuery\":null|\"short search\",\"rationale\":\"short\"}. Prefer openrouter + prompt-prose. Use local + prompt-sd only for pinned LoRAs."
        }
        "suggest" => {
            "You are Freya. Reply with JSON only: {\"ideas\":[\"nl alt 1\",\"nl alt 2\"],\"civitaiQuery\":\"short LoRA search\",\"sessionNote\":\"what to try next\"}. Ideas are everyday prose, never SD tags. civitaiQuery is 2–6 words for Civitai."
        }
        "session" => {
            "You are Freya running an agentic image session (like agentic coding, for images). Given the user's goal and optional session history, reply with JSON only: {\"goal\":\"restated goal\",\"steps\":[{\"kind\":\"search_loras\"|\"generate\"|\"iterate\",\"prompt\":\"natural language\",\"note\":\"why\"}],\"civitaiQuery\":\"short search or empty\",\"batchPrompts\":[\"nl 1\",\"nl 2\",\"nl 3\"],\"sessionNote\":\"friendly next step\"}. batchPrompts are 2–4 natural-language variations to generate as a set. Never use SD keyword soup."
        }
        _ => "You are Freya, a helpful image studio agent.",
    }
}
