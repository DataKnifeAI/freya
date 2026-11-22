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
        "pony" | "pony diffusion" => "checkpoints/pony-diffusion-v6-xl.safetensors",
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
            "checkpoints/pony-diffusion-v6-xl.safetensors",
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

/// True when `model` is a local checkpoint path the Diffusers worker can load as a file
/// (not an OpenRouter Auto sentinel, cloud id, or router prose).
///
/// Local generate must never pass strings like `"None, letting Freya use Auto default
/// (ZDR-aware)."` or `black-forest-labs/flux…` when the user picked Auto — those become
/// invalid HF repo ids. Prefer the worker active base instead.
pub fn is_usable_local_model(model: &str) -> bool {
    let m = model.trim();
    if m.is_empty() {
        return false;
    }
    // Auto / cloud sentinels and router prose (spaces, commas, parentheses).
    if crate::router::is_auto_image(m) {
        return false;
    }
    if m.contains(char::is_whitespace) || m.contains(',') || m.contains('(') {
        return false;
    }
    let lower = m.to_ascii_lowercase();
    if lower == "none"
        || lower.starts_with("none,")
        || lower.contains("zdr-aware")
        || lower.contains("letting freya")
        || lower.contains("auto default")
    {
        return false;
    }
    // Checkpoint file / relative path under models PVC.
    if m.ends_with(".safetensors")
        || m.starts_with("checkpoints/")
        || m.starts_with("models/")
        || (m.starts_with('/') && m.contains(".safetensors"))
    {
        return true;
    }
    false
}

/// Keep only a local-usable checkpoint ref; otherwise `None` (worker active base).
pub fn filter_local_model(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|s| !s.is_empty() && is_usable_local_model(s))
        .map(str::to_string)
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

    #[test]
    fn local_model_rejects_auto_prose_and_cloud() {
        assert!(is_usable_local_model(
            "checkpoints/pony-diffusion-v6-xl.safetensors"
        ));
        assert!(is_usable_local_model("checkpoints/foo.safetensors"));
        assert!(!is_usable_local_model("auto"));
        assert!(!is_usable_local_model("openrouter/auto"));
        assert!(!is_usable_local_model(
            "None, letting Freya use Auto default (ZDR-aware)."
        ));
        assert!(!is_usable_local_model(
            "black-forest-labs/flux.2-klein-4b"
        ));
        assert!(!is_usable_local_model("google/gemini-2.5-flash-image"));
        assert!(filter_local_model(Some("auto")).is_none());
        assert!(filter_local_model(Some(
            "checkpoints/pony-diffusion-v6-xl.safetensors"
        ))
        .is_some());
    }
}
