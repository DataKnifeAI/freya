//! Non-secret Studio UI state: favorites, session LoRA pins, backend, video pin, mode.
//!
//! API keys, tokens, and passwords never belong here. Cards drop those fields.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const CARD_KEYS: &[&str] = &[
    "id",
    "name",
    "modelType",
    "kind",
    "downloadUrl",
    "destPath",
    "triggerWord",
    "baseModel",
    "path",
    "versionId",
    "modelId",
    "fileId",
    "thumbnailUrl",
    "civitaiUrl",
    "scale",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPrefs {
    /// `cloud` | `local`. Empty = UI has not synced a choice.
    #[serde(default)]
    pub generate_backend: String,
    /// Video model id, or `auto`. Empty = unset.
    #[serde(default)]
    pub video_model: String,
    /// `plan` | `auto` | `yolo`. Empty = unset.
    #[serde(default)]
    pub automation_mode: String,
    #[serde(default)]
    pub favorites: Vec<Value>,
    #[serde(default)]
    pub pinned_loras: Vec<Value>,
    /// Sampler, CFG, steps, clip skip, seed, and the rest of the next local generate.
    #[serde(default)]
    pub generation: crate::genmeta::GenerationSettings,
}

impl Default for StudioPrefs {
    fn default() -> Self {
        Self {
            generate_backend: String::new(),
            video_model: String::new(),
            automation_mode: String::new(),
            favorites: Vec::new(),
            pinned_loras: Vec::new(),
            generation: crate::genmeta::GenerationSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StudioPatch {
    #[serde(default)]
    pub generate_backend: Option<String>,
    #[serde(default)]
    pub video_model: Option<String>,
    #[serde(default)]
    pub automation_mode: Option<String>,
    #[serde(default)]
    pub favorites: Option<Vec<Value>>,
    #[serde(default)]
    pub pinned_loras: Option<Vec<Value>>,
    #[serde(default)]
    pub generation: Option<crate::genmeta::GenerationSettings>,
}

pub fn normalize_backend(raw: &str) -> Result<String, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "local" => Ok("local".into()),
        "cloud" | "openrouter" => Ok("cloud".into()),
        other => Err(format!("backend must be cloud or local (got {other})")),
    }
}

pub fn normalize_mode(raw: &str) -> Result<String, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "plan" => Ok("plan".into()),
        "yolo" => Ok("yolo".into()),
        "auto" => Ok("auto".into()),
        other => Err(format!("mode must be plan, auto, or yolo (got {other})")),
    }
}

pub fn normalize_video_model(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("auto") {
        return Ok("auto".into());
    }
    if t.contains(char::is_whitespace) || t.len() > 160 {
        return Err("videoModel must be a model id".into());
    }
    if looks_secret_text(t) {
        return Err("videoModel must be an id, not a key".into());
    }
    Ok(t.to_string())
}

/// Merge a patch. Favorites and pins are replaced only when the field is present.
pub fn apply_patch(prefs: &mut StudioPrefs, patch: StudioPatch) -> Result<(), String> {
    if let Some(b) = patch.generate_backend {
        prefs.generate_backend = normalize_backend(&b)?;
    }
    if let Some(v) = patch.video_model {
        prefs.video_model = normalize_video_model(&v)?;
    }
    if let Some(m) = patch.automation_mode {
        prefs.automation_mode = normalize_mode(&m)?;
    }
    if let Some(favs) = patch.favorites {
        prefs.favorites = sanitize_list(&favs, false)?;
    }
    if let Some(pins) = patch.pinned_loras {
        prefs.pinned_loras = sanitize_list(&pins, true)?;
    }
    if let Some(generation) = patch.generation {
        prefs.generation = generation;
    }
    Ok(())
}

pub fn favorite_add(prefs: &mut StudioPrefs, card: &Value) -> Result<Value, String> {
    let card = sanitize_card(card, false)?;
    let key = card_key(&card);
    prefs.favorites.retain(|c| card_key(c) != key);
    prefs.favorites.insert(0, card.clone());
    prefs.favorites.truncate(200);
    Ok(card)
}

pub fn favorite_remove(prefs: &mut StudioPrefs, card: &Value) -> bool {
    let key = card_key(card);
    let before = prefs.favorites.len();
    prefs
        .favorites
        .retain(|c| card_key(c) != key && !same_loose(c, card));
    before != prefs.favorites.len()
}

pub fn pin_add(prefs: &mut StudioPrefs, card: &Value) -> Result<Value, String> {
    let card = sanitize_card(card, true)?;
    let key = card_key(&card);
    prefs.pinned_loras.retain(|c| card_key(c) != key);
    prefs.pinned_loras.push(card.clone());
    prefs.pinned_loras.truncate(24);
    Ok(card)
}

/// Pin or update scale on an existing pin without dropping other pin fields.
pub fn pin_upsert(prefs: &mut StudioPrefs, card: &Value) -> Result<Value, String> {
    let clean = sanitize_card(card, true)?;
    let key = card_key(&clean);
    if let Some(existing) = prefs
        .pinned_loras
        .iter_mut()
        .find(|c| card_key(c) == key)
    {
        if let (Some(obj), Some(scale)) = (existing.as_object_mut(), clean.get("scale")) {
            obj.insert("scale".into(), scale.clone());
        }
        return Ok(existing.clone());
    }
    pin_add(prefs, &clean)
}

pub fn pin_remove(prefs: &mut StudioPrefs, card: &Value) -> bool {
    let before = prefs.pinned_loras.len();
    prefs
        .pinned_loras
        .retain(|c| card_key(c) != card_key(card) && !same_loose(c, card));
    before != prefs.pinned_loras.len()
}

fn sanitize_list(items: &[Value], pins: bool) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for item in items {
        let card = sanitize_card(item, pins)?;
        let key = card_key(&card);
        if out.iter().any(|c| card_key(c) == key) {
            continue;
        }
        out.push(card);
        if out.len() >= if pins { 24 } else { 200 } {
            break;
        }
    }
    Ok(out)
}

pub fn sanitize_card(card: &Value, pin: bool) -> Result<Value, String> {
    let obj = card
        .as_object()
        .ok_or_else(|| "asset must be an object".to_string())?;
    if pin && !is_pinnable(card) {
        return Err(
            "session pins are LoRAs and embeddings — set checkpoints with set_base_model".into(),
        );
    }
    let mut out = Map::new();
    for key in CARD_KEYS {
        let Some(val) = obj.get(*key) else {
            continue;
        };
        if val.is_null() {
            continue;
        }
        match *key {
            "downloadUrl" | "thumbnailUrl" | "civitaiUrl" => {
                if let Some(url) = val.as_str().and_then(clean_http_url) {
                    out.insert((*key).to_string(), json!(url));
                }
            }
            "path" | "destPath" => {
                if let Some(path) = val.as_str().and_then(clean_rel_path) {
                    out.insert((*key).to_string(), json!(path));
                }
            }
            "name" | "modelType" | "kind" | "triggerWord" | "baseModel" => {
                if let Some(s) = val.as_str() {
                    let s = s.trim();
                    if !s.is_empty() && !looks_secret_text(s) && s.len() <= 240 {
                        out.insert((*key).to_string(), json!(s));
                    }
                }
            }
            "id" => {
                if let Some(s) = val.as_str() {
                    let s = s.trim();
                    if !s.is_empty() && !looks_secret_text(s) && s.len() <= 160 {
                        out.insert("id".into(), json!(s));
                    }
                } else if let Some(n) = val.as_i64() {
                    out.insert("id".into(), json!(n));
                }
            }
            "versionId" | "modelId" | "fileId" => {
                if let Some(n) = val.as_i64() {
                    out.insert((*key).to_string(), json!(n));
                }
            }
            "scale" => {
                if let Some(n) = val.as_f64() {
                    if (0.0..=2.0).contains(&n) {
                        out.insert("scale".into(), json!(n));
                    }
                }
            }
            _ => {}
        }
    }
    if !out.contains_key("scale") && pin {
        out.insert("scale".into(), json!(1.0));
    }
    let has_id = out.contains_key("id") || out.contains_key("path") || out.contains_key("name");
    if !has_id {
        return Err("asset needs a name, id, or path".into());
    }
    Ok(Value::Object(out))
}

pub fn is_pinnable(item: &Value) -> bool {
    let path = item.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let path_l = path.to_ascii_lowercase();
    if path_l.starts_with("checkpoints/") || path_l.starts_with("vae/") {
        return false;
    }
    let t = item
        .get("modelType")
        .or_else(|| item.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace([' ', '_', '-'], "");
    if t.is_empty() {
        if path_l.starts_with("loras/") || path_l.starts_with("embeddings/") {
            return true;
        }
        return path.is_empty();
    }
    if matches!(t.as_str(), "checkpoint" | "vae" | "model") {
        return false;
    }
    matches!(
        t.as_str(),
        "lora"
            | "locon"
            | "dora"
            | "lycoris"
            | "textualinversion"
            | "embedding"
            | "hypernet"
            | "hypernetwork"
    )
}

pub fn card_key(card: &Value) -> String {
    if let Some(p) = card.get("path").and_then(|p| p.as_str()) {
        let p = p.trim();
        if !p.is_empty() {
            return format!("path:{p}");
        }
    }
    if let Some(s) = card.get("id").and_then(|i| i.as_str()) {
        let s = s.trim();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    if let Some(n) = card.get("id").and_then(|i| i.as_i64()) {
        return n.to_string();
    }
    card.get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn same_loose(stored: &Value, query: &Value) -> bool {
    let sk = card_key(stored);
    let qk = card_key(query);
    if !sk.is_empty() && sk == qk {
        return true;
    }
    let sn = stored.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let qn = query.get("name").and_then(|n| n.as_str()).unwrap_or("");
    !sn.is_empty() && sn.eq_ignore_ascii_case(qn)
}

fn clean_rel_path(raw: &str) -> Option<String> {
    let t = raw.trim().trim_start_matches('/');
    if t.is_empty() || t.contains("..") || t.contains('\\') || t.contains('\0') {
        return None;
    }
    let ok = t.starts_with("loras/")
        || t.starts_with("checkpoints/")
        || t.starts_with("embeddings/")
        || t.starts_with("vae/")
        || t.starts_with("textual_inversion/");
    if !ok {
        return None;
    }
    Some(t.to_string())
}

fn clean_http_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if !(raw.starts_with("https://") || raw.starts_with("http://")) {
        return None;
    }
    if raw.contains('@') {
        return None;
    }
    let (base, query) = raw.split_once('?').unwrap_or((raw, ""));
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let k = p.split('=').next().unwrap_or("").to_ascii_lowercase();
            !matches!(
                k.as_str(),
                "token" | "key" | "api_key" | "apikey" | "access_token" | "secret"
            )
        })
        .collect();
    let cleaned = if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    };
    if looks_secret_text(&cleaned) {
        return None;
    }
    Some(cleaned)
}

fn looks_secret_text(raw: &str) -> bool {
    let l = raw.to_ascii_lowercase();
    l.contains("api_key")
        || l.contains("apikey")
        || l.contains("api-key")
        || l.contains("secret")
        || l.contains("password")
        || l.contains("bearer ")
        || l.contains("sk-or-")
        || l.contains("hf_")
}

pub fn asset_from_action_args(args: &Value) -> Value {
    args.get("asset")
        .filter(|a| a.is_object())
        .cloned()
        .unwrap_or_else(|| args.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_rejects_checkpoint_and_strips_token() {
        let ckpt = json!({"name": "Pony", "modelType": "Checkpoint", "path": "checkpoints/pony.safetensors"});
        assert!(sanitize_card(&ckpt, true).is_err());
        let lora = json!({
            "name": "Style",
            "modelType": "LORA",
            "path": "loras/style.safetensors",
            "downloadUrl": "https://civitai.com/api/download/models/5?token=secret&type=Model",
            "apiKey": "nope",
        });
        let clean = sanitize_card(&lora, true).unwrap();
        assert_eq!(
            clean.get("downloadUrl").and_then(|u| u.as_str()),
            Some("https://civitai.com/api/download/models/5?type=Model")
        );
        assert!(clean.get("apiKey").is_none());
        let mut prefs = StudioPrefs::default();
        pin_add(&mut prefs, &lora).unwrap();
        assert!(pin_add(&mut prefs, &ckpt).is_err());
        assert_eq!(prefs.pinned_loras.len(), 1);
    }

    #[test]
    fn backend_and_mode_normalize() {
        assert_eq!(normalize_backend("openrouter").unwrap(), "cloud");
        assert_eq!(normalize_backend("local").unwrap(), "local");
        assert!(normalize_backend("http://evil").is_err());
        assert_eq!(normalize_mode("YOLO").unwrap(), "yolo");
        assert!(normalize_video_model("sk-or-v1-abc").is_err());
    }
}
