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
            "You are Freya, a friendly image-generation expert. Help with prompts, model choice (OpenRouter cloud vs local LoRA), and troubleshooting. Be warm and clear."
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
            "You are Freya. Rewrite the user's intent as a rich natural-language image prompt for modern models (Flux, GPT Image, Gemini). One paragraph, vivid and specific. Output only the prompt."
        }
        "plan" => {
            "You are Freya's planner. Given user intent, reply with JSON only: {\"backend\":\"openrouter\"|\"local\",\"model\":\"...\",\"styleId\":null|\"...\",\"enhanceMode\":\"prompt-prose\"|\"prompt-sd\",\"rationale\":\"short\"}. Prefer openrouter unless the user needs a specific LoRA/checkpoint."
        }
        _ => "You are Freya, a helpful image assistant.",
    }
}
