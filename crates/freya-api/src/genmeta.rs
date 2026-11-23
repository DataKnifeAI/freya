//! Sampler / CFG / steps / clip skip for a local generate.
//!
//! Guidance comes from the selected Civitai version's example-image `meta`
//! (the first image that has steps or a sampler). LoRA versions are preferred
//! over the checkpoint. When that meta is missing, one family row applies:
//! Illustrious, Pony, and SDXL share Euler a / cfg 7 / 28 steps / clip skip 2;
//! SD 1.5 uses Euler a / cfg 7 / 20 steps / clip skip 1.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExampleMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cfg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_skip: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

impl ExampleMeta {
    pub fn has_sampling(&self) -> bool {
        self.steps.filter(|n| *n > 0).is_some()
            || self
                .sampler
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FamilyDefault {
    pub kind: &'static str,
    pub label: String,
    pub sampler: &'static str,
    pub cfg: f64,
    pub steps: u32,
    pub clip_skip: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecommendedGen {
    pub sampler: String,
    pub cfg: f64,
    pub steps: u32,
    pub clip_skip: u32,
    pub negative_prompt: Option<String>,
    pub size: Option<String>,
    /// `civitai` or `family`.
    pub source: &'static str,
    pub source_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerationSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cfg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_skip: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// Absent means the next generate picks a new seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<u32>,
    /// `civitai` | `family` | `user` | `agent` | `last`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
    /// Hand edits and agent writes stay until the user asks for the example again.
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommend_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler_applied: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_skip_applied: Option<bool>,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            sampler: None,
            cfg: None,
            steps: None,
            clip_skip: None,
            negative_prompt: None,
            seed: None,
            size: None,
            quantity: None,
            source: None,
            source_label: None,
            locked: false,
            recommend_key: None,
            last_seed: None,
            sampler_applied: None,
            sampler_note: None,
            clip_skip_applied: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GenPatch {
    pub sampler: Option<String>,
    pub clear_sampler: bool,
    pub cfg: Option<f64>,
    pub clear_cfg: bool,
    pub steps: Option<u32>,
    pub clear_steps: bool,
    pub clip_skip: Option<u32>,
    pub clear_clip_skip: bool,
    pub negative_prompt: Option<String>,
    pub seed: Option<i64>,
    pub clear_seed: bool,
    pub size: Option<String>,
    pub clear_size: bool,
    pub quantity: Option<u32>,
}

impl GenPatch {
    pub fn any(&self) -> bool {
        self.sampler.is_some()
            || self.clear_sampler
            || self.cfg.is_some()
            || self.clear_cfg
            || self.steps.is_some()
            || self.clear_steps
            || self.clip_skip.is_some()
            || self.clear_clip_skip
            || self.negative_prompt.is_some()
            || self.seed.is_some()
            || self.clear_seed
            || self.size.is_some()
            || self.clear_size
            || self.quantity.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerPlan {
    pub requested: String,
    pub applied: String,
    pub scheduler: &'static str,
    pub note: Option<String>,
}

/// First example image whose meta has steps or a sampler. Order is Civitai's
/// image list; the first entry is the version's primary image.
pub fn example_from_images<'a>(images: impl IntoIterator<Item = &'a Value>) -> Option<ExampleMeta> {
    for img in images {
        let Some(meta) = img.get("meta").filter(|m| m.is_object()) else {
            continue;
        };
        let parsed = example_from_meta(meta);
        if parsed.has_sampling() {
            return Some(parsed);
        }
    }
    None
}

pub fn example_from_meta(meta: &Value) -> ExampleMeta {
    let obj = meta.as_object();
    let get = |names: &[&str]| -> Option<&Value> {
        let obj = obj?;
        for (k, v) in obj {
            let nk = norm_key(k);
            if names.iter().any(|n| nk == *n) {
                return Some(v);
            }
        }
        None
    };
    let sampler = get(&["sampler"])
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(80).collect());
    let cfg = get(&["cfgscale", "cfg", "guidancescale", "guidance"])
        .and_then(num_f64)
        .filter(|n| n.is_finite() && *n >= 0.0);
    let steps = get(&["steps", "numinferencesteps"])
        .and_then(num_u32)
        .filter(|n| *n > 0);
    let clip_skip = get(&["clipskip"]).and_then(num_u32).filter(|n| *n > 0);
    let negative_prompt = get(&["negativeprompt", "negative"])
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(2000).collect());
    let size = get(&["size"])
        .and_then(|v| v.as_str())
        .and_then(clean_size)
        .or_else(|| {
            let w = get(&["width"]).and_then(num_u32)?;
            let h = get(&["height"]).and_then(num_u32)?;
            clean_size(&format!("{w}x{h}"))
        });
    ExampleMeta {
        sampler,
        cfg,
        steps,
        clip_skip,
        negative_prompt,
        size,
    }
}

pub fn family_default(base: &str) -> FamilyDefault {
    let b = base.to_ascii_lowercase();
    let sd15 = !b.contains("xl")
        && !b.contains("illustrious")
        && !b.contains("pony")
        && !b.contains("noob")
        && (b.contains("1.5")
            || b.contains("sd15")
            || b.contains("sd 1.4")
            || b.contains("sd1.4")
            || b.contains("v1-5")
            || b.contains("v1.5"));
    if sd15 {
        return FamilyDefault {
            kind: "sd15",
            label: "SD1.5".into(),
            sampler: "Euler a",
            cfg: 7.0,
            steps: 20,
            clip_skip: 1,
        };
    }
    let label = if b.contains("illustrious") {
        "Illustrious"
    } else if b.contains("pony") {
        "Pony"
    } else if b.contains("sdxl") || b.contains("xl") {
        "SDXL"
    } else if b.trim().is_empty() {
        "Illustrious/Pony/SDXL"
    } else {
        "Illustrious/Pony/SDXL"
    };
    FamilyDefault {
        kind: "sdxl",
        label: label.into(),
        sampler: "Euler a",
        cfg: 7.0,
        steps: 28,
        clip_skip: 2,
    }
}

pub fn recommend_from_example(
    example: Option<&ExampleMeta>,
    base: &str,
    asset_label: &str,
) -> RecommendedGen {
    let fam = family_default(base);
    let Some(ex) = example.filter(|e| e.has_sampling()) else {
        return RecommendedGen {
            sampler: fam.sampler.into(),
            cfg: fam.cfg,
            steps: fam.steps,
            clip_skip: fam.clip_skip,
            negative_prompt: None,
            size: None,
            source: "family",
            source_label: format!("Family default ({}) — no example image meta", fam.label),
        };
    };
    let who = if asset_label.trim().is_empty() {
        "selected version".to_string()
    } else {
        asset_label.trim().to_string()
    };
    RecommendedGen {
        sampler: ex
            .sampler
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| fam.sampler.into()),
        cfg: ex.cfg.unwrap_or(fam.cfg),
        steps: ex.steps.filter(|n| *n > 0).unwrap_or(fam.steps),
        clip_skip: ex.clip_skip.filter(|n| *n > 0).unwrap_or(fam.clip_skip),
        negative_prompt: ex.negative_prompt.clone(),
        size: ex.size.clone(),
        source: "civitai",
        source_label: format!("Civitai example on {who}"),
    }
}

impl GenerationSettings {
    pub fn sampling_complete(&self) -> bool {
        self.sampler
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
            && self.cfg.is_some()
            && self.steps.is_some()
            && self.clip_skip.is_some()
    }

    pub fn apply_patch(&mut self, patch: &GenPatch, source: &str, lock: bool) {
        if patch.clear_sampler {
            self.sampler = None;
        } else if let Some(s) = patch.sampler.as_deref() {
            let t = s.trim();
            self.sampler = if t.is_empty() {
                None
            } else {
                Some(t.chars().take(80).collect())
            };
        }
        if patch.clear_cfg {
            self.cfg = None;
        } else if let Some(n) = patch.cfg {
            self.cfg = Some(clamp_cfg(n));
        }
        if patch.clear_steps {
            self.steps = None;
        } else if let Some(n) = patch.steps {
            self.steps = Some(clamp_steps(n));
        }
        if patch.clear_clip_skip {
            self.clip_skip = None;
        } else if let Some(n) = patch.clip_skip {
            self.clip_skip = Some(clamp_clip(n));
        }
        if let Some(n) = patch.negative_prompt.as_deref() {
            let t = n.trim();
            self.negative_prompt = if t.is_empty() {
                None
            } else {
                Some(t.chars().take(2000).collect())
            };
        }
        if patch.clear_seed {
            self.seed = None;
        } else if let Some(seed) = patch.seed {
            self.seed = Some(seed);
        }
        if patch.clear_size {
            self.size = None;
        } else if let Some(s) = patch.size.as_deref() {
            self.size = clean_size(s);
        }
        if let Some(n) = patch.quantity {
            self.quantity = Some(clamp_quantity(n));
        }
        if lock {
            self.locked = true;
            self.source = Some(source.to_string());
            self.source_label = Some(match source {
                "agent" => "Set from chat".into(),
                "last" => "Reused from the last image".into(),
                "gallery" => "Community image".into(),
                _ => "Hand edit".into(),
            });
        }
    }

    pub fn apply_recommendation(&mut self, rec: &RecommendedGen, key: &str) {
        self.sampler = Some(rec.sampler.clone());
        self.cfg = Some(rec.cfg);
        self.steps = Some(rec.steps);
        self.clip_skip = Some(rec.clip_skip);
        self.negative_prompt = rec.negative_prompt.clone();
        if self
            .size
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
        {
            self.size = rec.size.clone();
        }
        self.source = Some(rec.source.into());
        self.source_label = Some(rec.source_label.clone());
        self.recommend_key = Some(key.to_string());
        self.locked = false;
        self.sampler_applied = None;
        self.sampler_note = None;
        self.clip_skip_applied = None;
    }
}

pub fn patch_from_value(v: &Value) -> GenPatch {
    let mut p = GenPatch::default();
    let Some(obj) = v.as_object() else {
        return p;
    };
    if obj.contains_key("sampler") {
        match obj.get("sampler") {
            Some(Value::Null) => p.clear_sampler = true,
            Some(Value::String(s)) if s.trim().is_empty() => p.clear_sampler = true,
            Some(other) => {
                if let Some(s) = other.as_str() {
                    p.sampler = Some(s.to_string());
                }
            }
            None => {}
        }
    }
    if obj.contains_key("cfg") || obj.contains_key("guidance") || obj.contains_key("guidanceScale")
    {
        let raw = obj
            .get("cfg")
            .or_else(|| obj.get("guidanceScale"))
            .or_else(|| obj.get("guidance"));
        match raw {
            Some(Value::Null) => p.clear_cfg = true,
            Some(n) => {
                if let Some(x) = num_f64(n) {
                    p.cfg = Some(x);
                }
            }
            None => {}
        }
    }
    if obj.contains_key("steps") {
        match obj.get("steps") {
            Some(Value::Null) => p.clear_steps = true,
            Some(n) => {
                if let Some(x) = num_u32(n) {
                    p.steps = Some(x);
                }
            }
            None => {}
        }
    }
    if obj.contains_key("clipSkip") || obj.contains_key("clip_skip") {
        let raw = obj.get("clipSkip").or_else(|| obj.get("clip_skip"));
        match raw {
            Some(Value::Null) => p.clear_clip_skip = true,
            Some(n) => {
                if let Some(x) = num_u32(n) {
                    p.clip_skip = Some(x);
                }
            }
            None => {}
        }
    }
    if obj.contains_key("negativePrompt") || obj.contains_key("negative_prompt") {
        let raw = obj
            .get("negativePrompt")
            .or_else(|| obj.get("negative_prompt"));
        if let Some(Value::String(s)) = raw {
            p.negative_prompt = Some(s.clone());
        } else if matches!(raw, Some(Value::Null)) {
            p.negative_prompt = Some(String::new());
        }
    }
    if obj.get("clearSeed").and_then(|v| v.as_bool()) == Some(true) {
        p.clear_seed = true;
    } else if obj.contains_key("seed") {
        match obj.get("seed") {
            Some(Value::Null) => p.clear_seed = true,
            Some(Value::String(s)) if s.trim().is_empty() => p.clear_seed = true,
            Some(n) => {
                if let Some(seed) = num_i64(n) {
                    p.seed = Some(seed);
                }
            }
            None => {}
        }
    }
    if obj.contains_key("size") {
        match obj.get("size") {
            Some(Value::Null) => p.clear_size = true,
            Some(Value::String(s)) if s.trim().is_empty() => p.clear_size = true,
            Some(Value::String(s)) => p.size = Some(s.clone()),
            _ => {}
        }
    }
    if obj.contains_key("quantity") || obj.contains_key("n") {
        let raw = obj.get("quantity").or_else(|| obj.get("n"));
        if let Some(n) = raw.and_then(num_u32) {
            p.quantity = Some(n);
        }
    }
    p
}

/// Pull obvious numeric settings out of a chat line ("30 steps", "cfg 5", "seed 12").
pub fn patch_from_message(message: &str) -> GenPatch {
    let mut p = GenPatch::default();
    let lower = message.to_ascii_lowercase();
    if let Some(n) = r_steps(&lower) {
        p.steps = Some(n);
    }
    if let Some(n) = capture_f64_cfg(&lower) {
        p.cfg = Some(n);
    }
    if let Some(n) = capture_u32_clip(&lower) {
        p.clip_skip = Some(n);
    }
    if let Some(n) = capture_i64_seed(&lower) {
        p.seed = Some(n);
    }
    if let Some(n) = capture_quantity(&lower) {
        p.quantity = Some(n);
    }
    if let Some(s) = sampler_phrase(&lower) {
        p.sampler = Some(s);
    }
    p
}

pub fn wants_reuse(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    const KEYS: &[&str] = &[
        "same settings",
        "same setting",
        "same seed",
        "use the same",
        "redo",
        "regenerate",
        "do it again",
        "do that again",
        "run it again",
        "generate again",
        "try again",
        "same sampler",
    ];
    KEYS.iter().any(|k| m.contains(k))
}

/// The message only names sampler / cfg / steps / seed / quantity, with no scene.
pub fn settings_command_only(message: &str) -> bool {
    if !patch_from_message(message).any() {
        return false;
    }
    let mut m = message.to_ascii_lowercase();
    for phrase in [
        "clip skip",
        "cfg scale",
        "cfg",
        "steps",
        "step",
        "seed",
        "images",
        "quantity",
        "dpm++ 2m karras",
        "dpm++ 2m",
        "euler ancestral",
        "euler a",
        "euler",
        "sampler",
    ] {
        m = m.replace(phrase, " ");
    }
    const NOISE: &[&str] = &[
        "please", "set", "to", "of", "the", "a", "an", "use", "using", "and", "with", "my", "me",
        "at", "for", "skip", "scale", "it", "them", "those", "on", "next", "generate",
    ];
    m.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .all(|w| NOISE.contains(&w) || w.chars().all(|c| c.is_ascii_digit()))
}

/// The message is only a request to repeat, with no new scene.
pub fn reuse_command_only(message: &str) -> bool {
    if !wants_reuse(message) {
        return false;
    }
    let mut m = message.to_ascii_lowercase();
    const KEYS: &[&str] = &[
        "same settings",
        "same setting",
        "same seed",
        "use the same settings",
        "use the same seed",
        "use the same",
        "do it again",
        "do that again",
        "run it again",
        "generate again",
        "try again",
        "same sampler",
        "regenerate",
        "redo",
    ];
    for k in KEYS {
        m = m.replace(k, " ");
    }
    const NOISE: &[&str] = &[
        "please", "the", "it", "that", "this", "image", "picture", "photo", "with", "use", "using",
        "again", "settings", "setting", "seed", "meta", "one", "a", "an", "me", "my", "just",
        "last", "previous", "same",
    ];
    m.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .all(|w| NOISE.contains(&w))
}

/// Map a sampler name onto a Diffusers scheduler. Exact for Euler a, Euler, and
/// DPM++ 2M Karras. Anything else uses the closest of those three and says so.
pub fn scheduler_plan(sampler: &str) -> SchedulerPlan {
    let requested = sampler.trim();
    let key = norm_sampler(requested);
    let (applied, scheduler, exact) = if matches!(
        key.as_str(),
        "euler a" | "eulera" | "euler ancestral" | "euler ancestral discrete"
    ) {
        ("Euler a", "EulerAncestralDiscreteScheduler", true)
    } else if key == "euler" || key == "euler discrete" {
        ("Euler", "EulerDiscreteScheduler", true)
    } else if matches!(
        key.as_str(),
        "dpm++ 2m karras" | "dpm++ 2m karas" | "dpmpp 2m karras" | "dpm++2m karras"
    ) {
        ("DPM++ 2M Karras", "DPMSolverMultistepScheduler", true)
    } else if key.contains("euler") && (key.contains("ancestral") || key.ends_with(" a")) {
        ("Euler a", "EulerAncestralDiscreteScheduler", false)
    } else if key.contains("euler") {
        ("Euler", "EulerDiscreteScheduler", false)
    } else if key.contains("dpm") || key.contains("karras") || key.contains("dpmpp") {
        ("DPM++ 2M Karras", "DPMSolverMultistepScheduler", false)
    } else {
        ("Euler a", "EulerAncestralDiscreteScheduler", false)
    };
    let note = if exact || requested.is_empty() {
        None
    } else {
        Some(format!(
            "requested {requested}; applied {applied} ({scheduler})"
        ))
    };
    SchedulerPlan {
        requested: if requested.is_empty() {
            applied.to_string()
        } else {
            requested.to_string()
        },
        applied: applied.to_string(),
        scheduler,
        note,
    }
}

pub fn clamp_steps(n: u32) -> u32 {
    n.clamp(1, 80)
}

pub fn clamp_cfg(n: f64) -> f64 {
    if !n.is_finite() {
        return 7.0;
    }
    n.clamp(0.0, 30.0)
}

pub fn clamp_clip(n: u32) -> u32 {
    n.clamp(1, 12)
}

pub fn clamp_quantity(n: u32) -> u32 {
    n.clamp(1, 4)
}

pub fn random_seed() -> i64 {
    let n = uuid::Uuid::new_v4().as_u128() as u64;
    let seed = (n % 2_147_483_646) as i64;
    if seed == 0 {
        1
    } else {
        seed
    }
}

pub fn clean_size(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase().replace('*', "x");
    let (a, b) = s.split_once('x')?;
    let w: u32 = a.trim().parse().ok()?;
    let h: u32 = b.trim().parse().ok()?;
    if !(64..=2048).contains(&w) || !(64..=2048).contains(&h) {
        return None;
    }
    Some(format!("{w}x{h}"))
}

fn norm_key(k: &str) -> String {
    k.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn norm_sampler(name: &str) -> String {
    let mut s = name.trim().to_ascii_lowercase().replace('_', " ");
    s = s.replace("dpmpp", "dpm++");
    s = s.replace('+', "+");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn num_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_u64().map(|n| n as f64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn num_u32(v: &Value) -> Option<u32> {
    v.as_u64()
        .map(|n| n as u32)
        .or_else(|| v.as_i64().filter(|n| *n >= 0).map(|n| n as u32))
        .or_else(|| v.as_f64().map(|n| n as u32))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn num_i64(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|n| n as i64))
        .or_else(|| v.as_f64().map(|n| n as i64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn r_steps(lower: &str) -> Option<u32> {
    // "28 steps" or "steps 28" or "steps: 28" or "steps to 28"
    for (i, _) in lower.match_indices("step") {
        let before = &lower[..i];
        let after = &lower[i + "step".len()..];
        if let Some(n) = trailing_u32(before) {
            return Some(n);
        }
        if let Some(n) = leading_u32(after.trim_start_matches('s')) {
            return Some(n);
        }
    }
    None
}

fn capture_f64_cfg(lower: &str) -> Option<f64> {
    for key in ["cfg scale", "cfgscale", "cfg"] {
        if let Some(idx) = lower.find(key) {
            let after = &lower[idx + key.len()..];
            if let Some(n) = leading_f64(after) {
                return Some(n);
            }
        }
    }
    None
}

fn capture_u32_clip(lower: &str) -> Option<u32> {
    let key = "clip skip";
    if let Some(idx) = lower.find(key) {
        return leading_u32(&lower[idx + key.len()..]);
    }
    if let Some(idx) = lower.find("clipskip") {
        return leading_u32(&lower[idx + "clipskip".len()..]);
    }
    None
}

fn capture_i64_seed(lower: &str) -> Option<i64> {
    let idx = lower.find("seed")?;
    let after = &lower[idx + "seed".len()..];
    let rest = after.trim_start();
    let rest = rest
        .trim_start_matches("to ")
        .trim_start_matches("of ")
        .trim_start_matches(':')
        .trim_start_matches('=')
        .trim();
    let token = rest.split_whitespace().next().unwrap_or("");
    let token = token.trim_matches(|c: char| !c.is_ascii_digit() && c != '-');
    token.parse().ok()
}

fn capture_quantity(lower: &str) -> Option<u32> {
    for key in ["images", "quantity"] {
        if let Some(idx) = lower.find(key) {
            if let Some(n) = trailing_u32(&lower[..idx]) {
                return Some(n);
            }
            if key == "quantity" {
                if let Some(n) = leading_u32(&lower[idx + key.len()..]) {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn sampler_phrase(lower: &str) -> Option<String> {
    const PHRASES: &[&str] = &[
        "dpm++ 2m karras",
        "dpm++ 2m sde karras",
        "dpm++ sde karras",
        "dpm++ 2m sde",
        "dpm++ 2m",
        "dpm++ sde",
        "euler a",
        "euler ancestral",
        "euler",
    ];
    for p in PHRASES {
        if lower.contains(p) {
            return Some(match *p {
                "euler a" | "euler ancestral" => "Euler a".into(),
                "euler" => "Euler".into(),
                "dpm++ 2m karras" => "DPM++ 2M Karras".into(),
                other => other.to_string(),
            });
        }
    }
    None
}

fn leading_u32(s: &str) -> Option<u32> {
    let s = s.trim_start();
    let s = s
        .trim_start_matches("to ")
        .trim_start_matches("of ")
        .trim_start_matches(':')
        .trim_start_matches('=')
        .trim_start();
    let token: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if token.is_empty() {
        None
    } else {
        token.parse().ok()
    }
}

fn leading_f64(s: &str) -> Option<f64> {
    let s = s.trim_start();
    let s = s
        .trim_start_matches("to ")
        .trim_start_matches("of ")
        .trim_start_matches(':')
        .trim_start_matches('=')
        .trim_start();
    let token: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if token.is_empty() {
        None
    } else {
        token.parse().ok()
    }
}

fn trailing_u32(s: &str) -> Option<u32> {
    let s = s.trim_end();
    let token: String = s
        .chars()
        .rev()
        .skip_while(|c| c.is_whitespace() || *c == ':' || *c == '=')
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if token.is_empty() {
        None
    } else {
        token.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn family_rows_are_small() {
        let xl = family_default("Illustrious");
        assert_eq!(xl.sampler, "Euler a");
        assert_eq!(xl.cfg, 7.0);
        assert_eq!(xl.steps, 28);
        assert_eq!(xl.clip_skip, 2);
        assert_eq!(family_default("Pony").steps, 28);
        assert_eq!(family_default("SDXL 1.0").steps, 28);
        assert_eq!(family_default("").steps, 28);
        let sd = family_default("SD 1.5");
        assert_eq!(sd.steps, 20);
        assert_eq!(sd.clip_skip, 1);
        assert_eq!(sd.sampler, "Euler a");
    }

    #[test]
    fn prefers_first_image_meta_with_steps_or_sampler() {
        let images = vec![
            json!({ "url": "a", "meta": { "prompt": "only a prompt" } }),
            json!({
                "url": "b",
                "meta": {
                    "sampler": "DPM++ 2M Karras",
                    "cfgScale": 4.5,
                    "steps": 32,
                    "clipSkip": 2,
                    "negativePrompt": "blurry",
                    "Size": "832x1216"
                }
            }),
            json!({ "url": "c", "meta": { "sampler": "Euler", "steps": 10 } }),
        ];
        let refs: Vec<&Value> = images.iter().collect();
        let ex = example_from_images(refs).unwrap();
        assert_eq!(ex.sampler.as_deref(), Some("DPM++ 2M Karras"));
        assert_eq!(ex.cfg, Some(4.5));
        assert_eq!(ex.steps, Some(32));
        assert_eq!(ex.clip_skip, Some(2));
        assert_eq!(ex.negative_prompt.as_deref(), Some("blurry"));
        assert_eq!(ex.size.as_deref(), Some("832x1216"));
        let rec = recommend_from_example(Some(&ex), "Illustrious", "Style LoRA v1");
        assert_eq!(rec.source, "civitai");
        assert!(rec.source_label.contains("Style LoRA"));
        assert_eq!(rec.cfg, 4.5);
    }

    #[test]
    fn missing_meta_uses_family_and_fills_gaps() {
        let images = vec![json!({ "meta": { "prompt": "hi" } })];
        let refs: Vec<&Value> = images.iter().collect();
        assert!(example_from_images(refs).is_none());
        let partial = ExampleMeta {
            sampler: Some("Euler".into()),
            cfg: None,
            steps: None,
            clip_skip: None,
            negative_prompt: Some("lowres".into()),
            size: None,
        };
        let rec = recommend_from_example(Some(&partial), "Pony", "Pony ckpt");
        assert_eq!(rec.source, "civitai");
        assert_eq!(rec.sampler, "Euler");
        assert_eq!(rec.cfg, 7.0);
        assert_eq!(rec.steps, 28);
        assert_eq!(rec.clip_skip, 2);
        assert_eq!(rec.negative_prompt.as_deref(), Some("lowres"));
        let none = recommend_from_example(None, "SD 1.5", "");
        assert_eq!(none.source, "family");
        assert_eq!(none.steps, 20);
        assert_eq!(none.clip_skip, 1);
    }

    #[test]
    fn scheduler_maps_the_three_names_and_says_when_it_is_closest() {
        let euler_a = scheduler_plan("Euler a");
        assert_eq!(euler_a.applied, "Euler a");
        assert_eq!(euler_a.scheduler, "EulerAncestralDiscreteScheduler");
        assert!(euler_a.note.is_none());
        let euler = scheduler_plan("Euler");
        assert_eq!(euler.scheduler, "EulerDiscreteScheduler");
        let dpm = scheduler_plan("DPM++ 2M Karras");
        assert_eq!(dpm.applied, "DPM++ 2M Karras");
        assert_eq!(dpm.scheduler, "DPMSolverMultistepScheduler");
        assert!(dpm.note.is_none());
        let close = scheduler_plan("DPM++ SDE Karras");
        assert_eq!(close.applied, "DPM++ 2M Karras");
        let note = close.note.unwrap();
        assert!(note.contains("requested DPM++ SDE Karras"));
        assert!(note.contains("DPMSolverMultistepScheduler"));
        let lms = scheduler_plan("LMS");
        assert_eq!(lms.applied, "Euler a");
        assert!(lms.note.unwrap().contains("requested LMS"));
    }

    #[test]
    fn reuse_phrases_and_message_settings() {
        assert!(wants_reuse("redo that"));
        assert!(wants_reuse("use the same settings"));
        assert!(reuse_command_only("redo"));
        assert!(reuse_command_only("please use the same settings"));
        assert!(!reuse_command_only("redo it at night in the rain"));
        let p = patch_from_message(
            "draw her, 30 steps, cfg 5.5, clip skip 2, seed 99, 2 images, euler a",
        );
        assert_eq!(p.steps, Some(30));
        assert_eq!(p.cfg, Some(5.5));
        assert_eq!(p.clip_skip, Some(2));
        assert_eq!(p.seed, Some(99));
        assert_eq!(p.quantity, Some(2));
        assert_eq!(p.sampler.as_deref(), Some("Euler a"));
    }

    #[test]
    fn hand_edit_locks_and_clears_seed() {
        let mut s = GenerationSettings::default();
        s.apply_recommendation(
            &recommend_from_example(None, "Illustrious", ""),
            "family:illustrious",
        );
        assert!(!s.locked);
        assert_eq!(s.steps, Some(28));
        let patch = patch_from_value(&json!({
            "steps": 40,
            "seed": null,
            "clearSeed": true,
            "negativePrompt": "watermark"
        }));
        s.apply_patch(&patch, "user", true);
        assert!(s.locked);
        assert_eq!(s.steps, Some(40));
        assert_eq!(s.cfg, Some(7.0));
        assert!(s.seed.is_none());
        assert_eq!(s.negative_prompt.as_deref(), Some("watermark"));
        assert_eq!(s.source.as_deref(), Some("user"));
    }
}
