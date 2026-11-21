//! Civitai search / download URL helpers (requires user-provided API key).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Installable weight types Freya surfaces by default (not every Civitai ModelType).
pub const DEFAULT_SEARCH_TYPES: &[&str] = &[
    "LORA",
    "LoCon",
    "DoRA",
    "Checkpoint",
    "TextualInversion",
    "VAE",
];

/// True when `model_type` is a Freya-supported installable asset.
pub fn is_freya_asset_type(model_type: &str) -> bool {
    let t = model_type.trim();
    DEFAULT_SEARCH_TYPES
        .iter()
        .any(|allowed| t.eq_ignore_ascii_case(allowed))
        || t.eq_ignore_ascii_case("LyCORIS")
}

/// Rank adapters ahead of checkpoints / embeddings / VAEs for Suggest.
pub fn asset_type_rank(model_type: &str) -> u8 {
    let t = model_type.to_ascii_lowercase();
    if t == "lora" || t == "locon" || t == "dora" || t == "lycoris" || t.contains("lora") {
        0
    } else if t == "checkpoint" {
        1
    } else if t == "textualinversion" {
        2
    } else if t == "vae" {
        3
    } else {
        4
    }
}

#[derive(Clone)]
pub struct CivitaiClient {
    http: reqwest::Client,
    api_key: String,
}

impl CivitaiClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }

    pub fn configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<CivitaiModelCard>> {
        self.search_types(query, limit, DEFAULT_SEARCH_TYPES).await
    }

    /// Search Civitai, optionally restricting to `types` (passed as repeated `types=` params).
    pub async fn search_types(
        &self,
        query: &str,
        limit: u32,
        types: &[&str],
    ) -> Result<Vec<CivitaiModelCard>> {
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let mut url = format!(
            "https://civitai.com/api/v1/models?limit={limit}&query={}",
            urlencoding_lite(query)
        );
        for t in types {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                url.push_str("&types=");
                url.push_str(&urlencoding_lite(trimmed));
            }
        }
        let v = self.get_json(&url).await.context("civitai search")?;
        let mut out = Vec::new();
        if let Some(items) = v.get("items").and_then(|i| i.as_array()) {
            for item in items {
                out.push(parse_model_card(item, false));
            }
        }
        Ok(out)
    }

    /// Full model detail (+ related checkpoints for non-checkpoint assets when baseModel is known).
    pub async fn get_model(&self, id: i64) -> Result<CivitaiModelDetail> {
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let url = format!("https://civitai.com/api/v1/models/{id}");
        let v = self.get_json(&url).await.context("civitai get model")?;
        if v.get("error").is_some() {
            return Err(anyhow!(
                "civitai model {}: {}",
                id,
                v.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("not found")
            ));
        }
        let mut detail = parse_model_detail(&v);
        // Civitai has no first-class dependency graph; use version baseModel → popular checkpoints.
        if !detail.model_type.eq_ignore_ascii_case("Checkpoint") {
            if let Some(base) = detail
                .base_model
                .as_deref()
                .filter(|b| !b.is_empty() && !b.eq_ignore_ascii_case("Other"))
            {
                match self.related_checkpoints(base, 4).await {
                    Ok(related) => {
                        detail.related = related
                            .into_iter()
                            .filter(|c| c.id != detail.id)
                            .collect();
                        if detail.needs_base.is_none() {
                            detail.needs_base = Some(format!("Needs base model family: {base}"));
                        }
                    }
                    Err(_) => {
                        detail.needs_base = Some(format!("Needs base model family: {base}"));
                    }
                }
            }
        }
        Ok(detail)
    }

    async fn related_checkpoints(&self, base_model: &str, limit: u32) -> Result<Vec<CivitaiModelCard>> {
        let url = format!(
            "https://civitai.com/api/v1/models?limit={limit}&types=Checkpoint&baseModels={}&sort={}",
            urlencoding_lite(base_model),
            urlencoding_lite("Most Downloaded")
        );
        let v = self.get_json(&url).await.context("civitai related checkpoints")?;
        let mut out = Vec::new();
        if let Some(items) = v.get("items").and_then(|i| i.as_array()) {
            for item in items {
                out.push(parse_model_card(item, false));
            }
        }
        Ok(out)
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let resp = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("civitai http")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("civitai {status}: {text}"));
        }
        serde_json::from_str(&text).context("civitai json")
    }
}

fn parse_model_card(item: &Value, detailed: bool) -> CivitaiModelCard {
    let id = item.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
    let name = item
        .get("name")
        .and_then(|x| x.as_str())
        .unwrap_or("untitled")
        .to_string();
    let model_type = item
        .get("type")
        .and_then(|x| x.as_str())
        .unwrap_or("Checkpoint")
        .to_string();
    let creator = item
        .pointer("/creator/username")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());
    let download_count = item
        .pointer("/stats/downloadCount")
        .and_then(|x| x.as_i64());
    let thumbs_up = item
        .pointer("/stats/thumbsUpCount")
        .and_then(|x| x.as_i64());

    let mut download_url = None;
    let mut version_id = None;
    let mut version_name = None;
    let mut example_prompt = None;
    let mut thumbnail = None;
    let mut base_model = None;
    let mut trained_words: Vec<String> = Vec::new();
    let mut image_urls: Vec<String> = Vec::new();

    if let Some(versions) = item.get("modelVersions").and_then(|m| m.as_array()) {
        if let Some(ver) = versions.first() {
            version_id = ver.get("id").and_then(|x| x.as_i64());
            version_name = ver
                .get("name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            base_model = ver
                .get("baseModel")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    item.get("baseModels")
                        .and_then(|b| b.as_array())
                        .and_then(|a| a.first())
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                });
            if let Some(words) = ver.get("trainedWords").and_then(|w| w.as_array()) {
                for w in words {
                    if let Some(s) = w.as_str() {
                        let trimmed = s.trim().trim_end_matches(',');
                        if !trimmed.is_empty() {
                            trained_words.push(trimmed.to_string());
                        }
                    }
                }
            }
            if let Some(files) = ver.get("files").and_then(|f| f.as_array()) {
                download_url = files
                    .iter()
                    .find(|f| f.get("primary").and_then(|p| p.as_bool()) == Some(true))
                    .or_else(|| files.first())
                    .and_then(|f| f.get("downloadUrl"))
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string());
            }
            if let Some(images) = ver.get("images").and_then(|i| i.as_array()) {
                for img in images.iter().take(if detailed { 12 } else { 1 }) {
                    if let Some(u) = img.get("url").and_then(|u| u.as_str()) {
                        if thumbnail.is_none() {
                            thumbnail = Some(u.to_string());
                        }
                        image_urls.push(u.to_string());
                        if example_prompt.is_none() {
                            example_prompt = img
                                .pointer("/meta/prompt")
                                .and_then(|p| p.as_str())
                                .map(|s| s.to_string());
                        }
                    }
                }
            }
        }
    }

    let description = item
        .get("description")
        .and_then(|d| d.as_str())
        .map(strip_html_brief);

    let trigger_word = trained_words.first().cloned();
    let stats = if download_count.is_some() || thumbs_up.is_some() {
        Some(CivitaiStats {
            download_count,
            thumbs_up_count: thumbs_up,
        })
    } else {
        None
    };

    CivitaiModelCard {
        id,
        name,
        model_type,
        version_id,
        version_name,
        download_url,
        thumbnail_url: thumbnail,
        example_prompt,
        base_model,
        trained_words: if trained_words.is_empty() {
            None
        } else {
            Some(trained_words)
        },
        trigger_word,
        description,
        creator,
        download_count,
        thumbs_up,
        stats,
        image_urls: if image_urls.is_empty() {
            None
        } else {
            Some(image_urls)
        },
        civitai_url: Some(format!("https://civitai.com/models/{id}")),
    }
}

fn parse_model_detail(item: &Value) -> CivitaiModelDetail {
    let card = parse_model_card(item, true);
    let needs_base = card.base_model.as_ref().and_then(|b| {
        if card.model_type.eq_ignore_ascii_case("Checkpoint") {
            None
        } else {
            Some(format!("Needs base model family: {b}"))
        }
    });
    CivitaiModelDetail {
        id: card.id,
        name: card.name,
        model_type: card.model_type,
        version_id: card.version_id,
        version_name: card.version_name,
        download_url: card.download_url,
        thumbnail_url: card.thumbnail_url,
        example_prompt: card.example_prompt,
        base_model: card.base_model,
        trained_words: card.trained_words,
        trigger_word: card.trigger_word,
        description: card.description,
        creator: card.creator,
        download_count: card.download_count,
        thumbs_up: card.thumbs_up,
        image_urls: card.image_urls,
        civitai_url: card.civitai_url,
        needs_base,
        related: Vec::new(),
    }
}

/// Strip HTML tags and collapse whitespace for a short plain-text blurb.
fn strip_html_brief(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 420 {
        let trimmed: String = collapsed.chars().take(417).collect();
        format!("{trimmed}…")
    } else {
        collapsed
    }
}

fn urlencoding_lite(s: &str) -> String {
    // Percent-encode UTF-8 bytes (not truncated chars) for non-ASCII queries.
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiModelCard {
    pub id: i64,
    pub name: String,
    pub model_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trained_words: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_word: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbs_up: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<CivitaiStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub civitai_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbs_up_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiModelDetail {
    pub id: i64,
    pub name: String,
    pub model_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trained_words: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_word: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbs_up: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub civitai_url: Option<String>,
    /// Human hint when adapters need a compatible checkpoint family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_base: Option<String>,
    /// Popular checkpoints for the same baseModel family (best-effort related assets).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<CivitaiModelCard>,
}
