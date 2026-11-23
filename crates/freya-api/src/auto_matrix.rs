//! Built-in Auto fallback matrix, loaded from `packs/auto-models.json`.
//!
//! The packs file is the source of truth. A missing or invalid file does not
//! stop the process: a small built-in matrix (one id per alignment, every cost)
//! is used so routing still returns a model.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::warn;

static MATRIX: OnceLock<AutoMatrix> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
struct CostSet {
    low: String,
    medium: String,
    high: String,
    max: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AlignSet {
    balanced: CostSet,
    safety: CostSet,
    uncensored: CostSet,
}

#[derive(Debug, Clone, Deserialize)]
struct PrivacySet {
    default: AlignSet,
    zdr: AlignSet,
}

#[derive(Debug, Clone, Deserialize)]
struct AutoMatrix {
    router: PrivacySet,
    agent: PrivacySet,
    image: PrivacySet,
    video: PrivacySet,
    #[serde(default)]
    notes: Vec<String>,
}

/// Load `packs/auto-models.json` once. Later calls are no-ops.
pub fn init(packs_dir: &Path) {
    let matrix = matrix_from_packs(packs_dir);
    if MATRIX.set(matrix).is_err() {
        warn!("auto matrix already installed; packs file was not applied");
    }
}

/// Role × privacy × alignment × cost. Unknown alignment is balanced.
/// Unknown cost is medium. Empty cells cannot occur in a parsed file.
pub fn model_for(role: &str, strict: bool, alignment: &str, cost: &str) -> String {
    let matrix = current();
    let privacy = match role {
        "router" => &matrix.router,
        "agent" => &matrix.agent,
        "image" => &matrix.image,
        "video" => &matrix.video,
        _ => &matrix.agent,
    };
    let align = if strict { &privacy.zdr } else { &privacy.default };
    let row = match alignment {
        "safety" => &align.safety,
        "uncensored" => &align.uncensored,
        _ => &align.balanced,
    };
    match cost {
        "low" | "cheap" => row.low.clone(),
        "high" => row.high.clone(),
        "max" | "maximum" => row.max.clone(),
        _ => row.medium.clone(),
    }
}

fn current() -> &'static AutoMatrix {
    MATRIX.get_or_init(|| {
        let bundled = repo_packs_file();
        if bundled.is_file() {
            if let Some(matrix) = read_matrix(&bundled) {
                return matrix;
            }
        }
        small_fallback()
    })
}

fn repo_packs_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/auto-models.json")
}

fn matrix_from_packs(packs_dir: &Path) -> AutoMatrix {
    let path = packs_dir.join("auto-models.json");
    if let Some(matrix) = read_matrix(&path) {
        tracing::info!(
            path = %path.display(),
            notes = matrix.notes.len(),
            "loaded auto fallback matrix"
        );
        return matrix;
    }
    warn!(
        path = %path.display(),
        "auto-models.json missing or invalid; using the small built-in matrix"
    );
    small_fallback()
}

fn read_matrix(path: &Path) -> Option<AutoMatrix> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<AutoMatrix>(&text) {
        Ok(matrix) if matrix_complete(&matrix) => Some(matrix),
        Ok(_) => {
            warn!(path = %path.display(), "auto-models.json has an empty cell");
            None
        }
        Err(err) => {
            warn!(path = %path.display(), %err, "auto-models.json did not parse");
            None
        }
    }
}

fn matrix_complete(matrix: &AutoMatrix) -> bool {
    [&matrix.router, &matrix.agent, &matrix.image, &matrix.video]
        .into_iter()
        .all(privacy_complete)
}

fn privacy_complete(block: &PrivacySet) -> bool {
    align_complete(&block.default) && align_complete(&block.zdr)
}

fn align_complete(block: &AlignSet) -> bool {
    cost_complete(&block.balanced)
        && cost_complete(&block.safety)
        && cost_complete(&block.uncensored)
}

fn cost_complete(row: &CostSet) -> bool {
    !row.low.trim().is_empty()
        && !row.medium.trim().is_empty()
        && !row.high.trim().is_empty()
        && !row.max.trim().is_empty()
}

/// One id per alignment so boot still routes when the packs file is absent.
/// Costs share that id. This is not the researched matrix.
fn small_fallback() -> AutoMatrix {
    fn cost(id: &str) -> CostSet {
        CostSet {
            low: id.to_string(),
            medium: id.to_string(),
            high: id.to_string(),
            max: id.to_string(),
        }
    }
    fn align(balanced: &str, safety: &str, uncensored: &str) -> AlignSet {
        AlignSet {
            balanced: cost(balanced),
            safety: cost(safety),
            uncensored: cost(uncensored),
        }
    }
    fn privacy(balanced: &str, safety: &str, uncensored: &str) -> PrivacySet {
        let row = align(balanced, safety, uncensored);
        PrivacySet {
            default: row.clone(),
            zdr: row,
        }
    }
    AutoMatrix {
        router: privacy(
            "deepseek/deepseek-v4-pro",
            "openai/gpt-5-mini",
            "thedrummer/cydonia-24b-v4.1",
        ),
        agent: privacy(
            "deepseek/deepseek-v4-pro",
            "openai/gpt-5-mini",
            "thedrummer/cydonia-24b-v4.1",
        ),
        image: privacy(
            "bytedance-seed/seedream-4.5",
            "openai/gpt-image-2",
            "black-forest-labs/flux-3-image",
        ),
        video: privacy(
            "bytedance/seedance-2.5-flash",
            "google/veo-3.1-fast",
            "x-ai/grok-imagine-video",
        ),
        notes: vec![
            "Small built-in matrix. packs/auto-models.json was missing or invalid.".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_file_fills_every_cell() {
        let path = repo_packs_file();
        let matrix = read_matrix(&path).expect("packs/auto-models.json");
        assert!(matrix.notes.iter().any(|n| n.contains("video") && n.contains("ZDR")));
        let roles = [&matrix.router, &matrix.agent, &matrix.image, &matrix.video];
        assert_eq!(roles.len(), 4);
        assert!(matrix_complete(&matrix));
    }

    #[test]
    fn uncensored_chat_is_not_llama_claude_or_gpt() {
        let _ = current();
        for role in ["router", "agent"] {
            for strict in [false, true] {
                for cost in ["low", "medium", "high", "max"] {
                    let id = model_for(role, strict, "uncensored", cost);
                    let l = id.to_ascii_lowercase();
                    assert!(
                        l.contains("dolphin") || l.contains("cydonia"),
                        "{role} strict={strict} {cost} -> {id}"
                    );
                    assert!(!l.contains("llama"), "{id}");
                    assert!(!l.starts_with("anthropic/"), "{id}");
                    assert!(!l.starts_with("openai/"), "{id}");
                }
            }
        }
    }

    #[test]
    fn missing_file_uses_small_fallback() {
        let matrix = matrix_from_packs(Path::new("/tmp/freya-auto-matrix-missing"));
        assert_eq!(matrix.agent.default.balanced.low, matrix.agent.default.balanced.max);
        assert!(!matrix.video.default.safety.medium.is_empty());
        assert!(!matrix.video.zdr.uncensored.max.is_empty());
    }
}
