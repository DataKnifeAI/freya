//! Civitai `baseModel` → local checkpoint / HF id hints for the diffusion worker.
//!
//! Remote vLLM image endpoints may not support SD checkpoints; the local GPU worker
//! (`FREYA_MODELS_DIR` on the game drive / PVC) is the path that hosts these bases.

use serde_json::{json, Value};

/// Known Civitai base-family labels → preferred HuggingFace / engine model id.
/// Used when no local checkpoint is selected yet (hint only — stub worker echoes it).
pub fn suggest_hf_model(base_model: &str) -> Option<&'static str> {
    let b = normalize_base(base_model);
    Some(match b.as_str() {
        "sd 1.5" | "sd1.5" | "stable diffusion 1.5" => "runwayml/stable-diffusion-v1-5",
        "sd 2.1" | "sd2.1" => "stabilityai/stable-diffusion-2-1",
        "sdxl" | "sdxl 1.0" | "stable diffusion xl 1.0" => {
            "stabilityai/stable-diffusion-xl-base-1.0"
        }
        "sdxl turbo" | "sdxl-turbo" => "stabilityai/sdxl-turbo",
        "pony" | "pony diffusion" => "checkpoints/ponyDiffusionV6XL.safetensors",
        "illustrious" => "checkpoints/illustrious.safetensors",
        "flux" | "flux.1 d" | "flux.1 s" | "flux1" => "black-forest-labs/FLUX.1-dev",
        "noobai" => "checkpoints/noobai.safetensors",
        _ => return None,
    })
}

/// Human family label for UI / compatibility checks.
pub fn family_label(base_model: &str) -> String {
    let b = normalize_base(base_model);
    match b.as_str() {
        s if s.starts_with("sd 1.5") || s == "sd1.5" => "SD 1.5".into(),
        s if s.starts_with("sd 2") => "SD 2.x".into(),
        s if s.contains("sdxl") => "SDXL".into(),
        s if s.contains("pony") => "Pony".into(),
        s if s.contains("illustrious") => "Illustrious".into(),
        s if s.contains("flux") => "Flux".into(),
        s if s.contains("noobai") => "NoobAI".into(),
        _ => base_model.trim().to_string(),
    }
}

/// True when two Civitai-ish base labels are compatible enough for LoRA apply.
pub fn bases_compatible(a: &str, b: &str) -> bool {
    if a.trim().is_empty() || b.trim().is_empty() {
        return true;
    }
    family_label(a).eq_ignore_ascii_case(&family_label(b))
}

fn normalize_base(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// Static catalog for UI / API discovery (does not hit Civitai).
pub fn catalog() -> Vec<Value> {
    vec![
        entry("SD 1.5", "runwayml/stable-diffusion-v1-5", "~4GB VRAM"),
        entry(
            "SDXL 1.0",
            "stabilityai/stable-diffusion-xl-base-1.0",
            "~6–8GB VRAM",
        ),
        entry(
            "Pony",
            "checkpoints/ponyDiffusionV6XL.safetensors",
            "SDXL-family; install a Pony checkpoint under checkpoints/",
        ),
        entry(
            "Illustrious",
            "checkpoints/illustrious.safetensors",
            "SDXL-family; install under checkpoints/",
        ),
        entry(
            "Flux",
            "black-forest-labs/FLUX.1-dev",
            "~20GB+ VRAM; local 4090 may need offload / quantized build",
        ),
    ]
}

fn entry(base: &str, model: &str, note: &str) -> Value {
    json!({
        "baseModel": base,
        "family": family_label(base),
        "suggestedModel": model,
        "note": note,
        "localOnly": true,
    })
}

/// Pick a generate `model` from pinned LoRA metadata when the client omitted one.
pub fn resolve_model_from_loras(loras: &[LoraBaseHint]) -> Option<String> {
    for l in loras {
        if let Some(b) = l.base_model.as_deref().filter(|s| !s.is_empty()) {
            if let Some(m) = suggest_hf_model(b) {
                return Some(m.to_string());
            }
            return Some(format!("checkpoints/{}.safetensors", sanitize_name(b)));
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct LoraBaseHint {
    pub base_model: Option<String>,
}

fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdxl_maps() {
        assert_eq!(
            suggest_hf_model("SDXL 1.0"),
            Some("stabilityai/stable-diffusion-xl-base-1.0")
        );
        assert!(bases_compatible("SDXL 1.0", "SDXL"));
        assert!(!bases_compatible("SD 1.5", "Pony"));
    }

    #[test]
    fn resolve_from_lora() {
        let hints = vec![LoraBaseHint {
            base_model: Some("Pony".into()),
        }];
        assert!(resolve_model_from_loras(&hints)
            .unwrap()
            .contains("pony"));
    }
}
