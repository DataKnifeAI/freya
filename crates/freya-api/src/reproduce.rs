//! Apply Civitai community posts or gallery sidecars into studio generation state.

use crate::civitai::{resource_matches, CommunityImage, CommunityImageResource, StudioInventory};
use crate::gallery::{ArtifactCheckpointMeta, ArtifactMeta};
use crate::genmeta::{patch_from_value, GenPatch, GenerationSettings};
use crate::studio;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;

pub fn lora_weight_default(weight: Option<f64>) -> f32 {
    weight
        .filter(|w| w.is_finite() && *w >= 0.0 && *w <= 2.0)
        .map(|w| w as f32)
        .unwrap_or(1.0)
}

fn is_lora_kind(kind: &str) -> bool {
    let t = kind.to_ascii_lowercase().replace([' ', '_', '-'], "");
    matches!(
        t.as_str(),
        "lora" | "locon" | "dora" | "lycoris" | "textualinversion" | "embedding" | "lyco"
    )
}

fn is_checkpoint_kind(kind: &str) -> bool {
    let t = kind.to_ascii_lowercase();
    matches!(t.as_str(), "checkpoint" | "model")
}

fn installed_loras_from_resources(
    resources: &[CommunityImageResource],
    inventory: &StudioInventory,
) -> Vec<(CommunityImageResource, crate::civitai::InstalledAssetMatch)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for resource in resources {
        if !is_lora_kind(&resource.resource_type) {
            continue;
        }
        let hit = inventory
            .installed
            .iter()
            .find(|asset| resource_matches(resource, asset));
        if let Some(asset) = hit {
            let key = format!("{}:{}", asset.path, asset.name);
            if seen.insert(key) {
                out.push((resource.clone(), asset.clone()));
            }
        }
    }
    out
}

fn checkpoint_resource(resources: &[CommunityImageResource]) -> Option<CommunityImageResource> {
    resources
        .iter()
        .find(|r| is_checkpoint_kind(&r.resource_type))
        .cloned()
        .or_else(|| {
            resources
                .iter()
                .find(|r| !r.name.trim().is_empty() && is_checkpoint_kind(&r.resource_type))
                .cloned()
        })
}

pub fn installed_checkpoint_for_image(
    image: &CommunityImage,
    inventory: &StudioInventory,
) -> Option<crate::civitai::InstalledAssetMatch> {
    if let Some(resource) = checkpoint_resource(&image.resources) {
        if let Some(hit) = inventory
            .installed
            .iter()
            .find(|asset| resource_matches(&resource, asset))
        {
            return Some(hit.clone());
        }
    }
    let ck = image.checkpoint.as_deref()?.trim();
    if ck.is_empty() {
        return None;
    }
    inventory.installed.iter().find(|asset| {
        asset.name.eq_ignore_ascii_case(ck)
            || asset
                .path
                .rsplit(['/', '\\'])
                .next()
                .is_some_and(|stem| stem.eq_ignore_ascii_case(ck))
    }).cloned()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReproductionApplyResult {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub settings: GenerationSettings,
    pub pinned_loras: Vec<Value>,
    pub missing_checkpoint: Option<String>,
    pub missing_loras: Vec<String>,
    pub base_set: bool,
    pub base_note: String,
    pub enhance: String,
}

pub fn meta_to_patch(meta: &ArtifactMeta) -> GenPatch {
    let v = json!({
        "sampler": meta.sampler,
        "cfg": meta.cfg,
        "steps": meta.steps,
        "clipSkip": meta.clip_skip,
        "negativePrompt": meta.negative_prompt.as_deref().unwrap_or(""),
        "seed": meta.seed,
        "size": meta.size,
        "quantity": meta.quantity,
        "enhance": "off",
    });
    patch_from_value(&v)
}

pub fn community_to_patch(image: &CommunityImage) -> GenPatch {
    let v = json!({
        "sampler": image.sampler,
        "cfg": image.cfg,
        "steps": image.steps,
        "clipSkip": image.clip_skip,
        "negativePrompt": image.negative_prompt.as_deref().unwrap_or(""),
        "seed": image.seed,
        "size": image.size,
        "enhance": "off",
    });
    patch_from_value(&v)
}

pub fn pins_from_artifact_meta(meta: &ArtifactMeta) -> Vec<Value> {
    if !meta.lora_pins.is_empty() {
        return meta
            .lora_pins
            .iter()
            .map(|l| {
                json!({
                    "name": l.name,
                    "path": l.path,
                    "scale": l.scale,
                    "versionId": l.version_id,
                    "modelId": l.model_id,
                })
            })
            .collect();
    }
    meta.loras.iter().map(|n| json!({ "name": n, "scale": 1.0 })).collect()
}

pub fn pins_from_community(
    image: &CommunityImage,
    inventory: &StudioInventory,
) -> (Vec<Value>, Vec<String>) {
    let mut missing = Vec::new();
    let pins: Vec<Value> = installed_loras_from_resources(&image.resources, inventory)
        .into_iter()
        .map(|(resource, asset)| {
            json!({
                "name": asset.name,
                "path": asset.path,
                "modelType": asset.kind,
                "scale": lora_weight_default(resource.weight),
                "versionId": resource.model_version_id.or(asset.version_id),
                "modelId": resource.model_id.or(asset.model_id),
            })
        })
        .collect();
    for resource in &image.resources {
        if !is_lora_kind(&resource.resource_type) {
            continue;
        }
        let installed = inventory
            .installed
            .iter()
            .any(|asset| resource_matches(resource, asset));
        if !installed && !resource.name.trim().is_empty() {
            missing.push(resource.name.clone());
        }
    }
    (pins, missing)
}

pub fn checkpoint_meta_from_path(
    path: &str,
    display: Option<&str>,
) -> Option<ArtifactCheckpointMeta> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let name = display
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.rsplit(['/', '\\']).next().unwrap_or(path))
        .to_string();
    Some(ArtifactCheckpointMeta {
        name,
        path: Some(path.to_string()),
        model_id: None,
        version_id: None,
    })
}

/// Merge reproduction pins into studio prefs (does not download; skips missing LoRAs).
pub fn merge_reproduction_pins(prefs: &mut studio::StudioPrefs, new_pins: &[Value]) {
    for card in new_pins {
        let key = studio::card_key(card);
        if prefs
            .pinned_loras
            .iter()
            .any(|p| studio::card_key(p) == key)
        {
            continue;
        }
        if let Ok(clean) = studio::sanitize_card(card, true) {
            let _ = studio::pin_add(prefs, &clean);
        }
    }
}

pub fn apply_community_image(
    image: &CommunityImage,
    inventory: &StudioInventory,
    settings: &mut GenerationSettings,
    prefs: &mut studio::StudioPrefs,
) -> ReproductionApplyResult {
    let patch = community_to_patch(image);
    settings.apply_patch(&patch, "gallery", true);
    settings.enhance = Some("off".into());
    settings.source_label = Some("Community image".into());

    let (pins, missing_loras) = pins_from_community(image, inventory);
    merge_reproduction_pins(prefs, &pins);

    let base = installed_checkpoint_for_image(image, inventory);
    let missing_checkpoint = if base.is_none() {
        image.checkpoint.clone()
    } else {
        None
    };
    let base_note = match (&base, &image.checkpoint) {
        (Some(b), _) => format!("Checkpoint on worker: {}", b.name),
        (None, Some(ck)) => format!("Checkpoint “{ck}” is not installed."),
        _ => String::new(),
    };

    ReproductionApplyResult {
        prompt: image.prompt.clone().unwrap_or_default(),
        negative_prompt: image.negative_prompt.clone(),
        settings: settings.clone(),
        pinned_loras: prefs.pinned_loras.clone(),
        missing_checkpoint,
        missing_loras,
        base_set: base.is_some(),
        base_note,
        enhance: "off".into(),
    }
}

pub fn apply_gallery_meta(
    meta: &ArtifactMeta,
    inventory: &StudioInventory,
    settings: &mut GenerationSettings,
    prefs: &mut studio::StudioPrefs,
) -> ReproductionApplyResult {
    let patch = meta_to_patch(meta);
    settings.apply_patch(&patch, "gallery", true);
    settings.enhance = Some("off".into());
    settings.source_label = Some("Gallery artifact".into());

    let pin_values = pins_from_artifact_meta(meta);
    let mut missing_loras = Vec::new();
    let resolved: Vec<Value> = if !meta.lora_pins.is_empty() {
        meta.lora_pins
            .iter()
            .filter_map(|l| {
                let resource = CommunityImageResource {
                    name: l.name.clone(),
                    resource_type: "LORA".into(),
                    weight: Some(l.scale as f64),
                    model_version_id: l.version_id,
                    model_id: l.model_id,
                    hash: None,
                    installable: None,
                    dest_path: None,
                };
                let hit = inventory
                    .installed
                    .iter()
                    .find(|asset| {
                        resource_matches(&resource, asset)
                            || l.path.as_deref() == Some(asset.path.as_str())
                    });
                if let Some(asset) = hit {
                    Some(json!({
                        "name": asset.name,
                        "path": asset.path,
                        "scale": l.scale,
                        "versionId": l.version_id.or(asset.version_id),
                        "modelId": l.model_id.or(asset.model_id),
                    }))
                } else {
                    missing_loras.push(l.name.clone());
                    None
                }
            })
            .collect()
    } else {
        pin_values
    };
    merge_reproduction_pins(prefs, &resolved);

    let base_note = meta
        .checkpoint
        .as_ref()
        .map(|c| {
            if let Some(path) = c.path.as_deref() {
                if inventory
                    .installed
                    .iter()
                    .any(|a| a.path == path)
                {
                    format!("Checkpoint: {}", c.name)
                } else {
                    format!("Checkpoint “{}” is not installed.", c.name)
                }
            } else {
                c.name.clone()
            }
        })
        .unwrap_or_default();

    let missing_checkpoint = meta.checkpoint.as_ref().and_then(|c| {
        let installed = c.path.as_deref().is_some_and(|path| {
            inventory.installed.iter().any(|a| a.path == path)
        });
        if installed {
            None
        } else {
            Some(c.name.clone())
        }
    });

    ReproductionApplyResult {
        prompt: meta.prompt.clone(),
        negative_prompt: meta.negative_prompt.clone(),
        settings: settings.clone(),
        pinned_loras: prefs.pinned_loras.clone(),
        missing_checkpoint,
        missing_loras,
        base_set: meta.checkpoint.is_some(),
        base_note,
        enhance: "off".into(),
    }
}