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

/// One Civitai model type per agent search. `Model` is a file role / Styles bucket, not a type.
pub const AGENT_SEARCH_TYPES: &[&str] = &[
    "LORA",
    "LoCon",
    "DoRA",
    "LyCORIS",
    "Checkpoint",
    "TextualInversion",
    "VAE",
];

pub const AGENT_SEARCH_TYPES_LABEL: &str =
    "LORA, LoCon, DoRA, LyCORIS, Checkpoint, TextualInversion, VAE";

/// Map a tool/UI type word onto one Civitai `types=` value.
/// Embedding is TextualInversion. Model is rejected (not a model type).
pub fn canonical_search_type(raw: &str) -> Result<&'static str, String> {
    if raw.contains(',') {
        return Err(format!(
            "search one type per call. type is one of {AGENT_SEARCH_TYPES_LABEL}. Embedding is TextualInversion. Model is not a Civitai type."
        ));
    }
    let key: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if key.is_empty() {
        return Err(format!(
            "type is required, one of {AGENT_SEARCH_TYPES_LABEL}. Embedding is TextualInversion. Model is not a Civitai type. Call once per type. Does not pin or download."
        ));
    }
    if key == "model" || key == "models" {
        return Err(
            "Model is not a Civitai model type (file role Model is the weight file; Styles uses Model for untyped files). Search Checkpoint for a base, or one of LORA, LoCon, DoRA, LyCORIS, TextualInversion, VAE."
                .into(),
        );
    }
    let canon = match key.as_str() {
        "lora" | "loras" => "LORA",
        "locon" => "LoCon",
        "dora" => "DoRA",
        "lycoris" | "lyco" => "LyCORIS",
        "checkpoint" | "checkpoints" => "Checkpoint",
        "textualinversion" | "embedding" | "embeddings" => "TextualInversion",
        "vae" | "vaes" => "VAE",
        _ => {
            return Err(format!(
                "unknown Civitai type {raw:?}. Search one of {AGENT_SEARCH_TYPES_LABEL}. Embedding is TextualInversion."
            ));
        }
    };
    if !AGENT_SEARCH_TYPES.contains(&canon) {
        return Err(format!(
            "unknown Civitai type {raw:?}. Search one of {AGENT_SEARCH_TYPES_LABEL}."
        ));
    }
    Ok(canon)
}

/// True when a Civitai card `type` is the requested agent search type.
pub fn same_search_type(model_type: &str, canon: &str) -> bool {
    match canonical_search_type(model_type) {
        Ok(got) => got == canon,
        Err(_) => false,
    }
}

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

    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        nsfw: bool,
    ) -> Result<Vec<CivitaiModelCard>> {
        self.search_types(query, limit, DEFAULT_SEARCH_TYPES, nsfw)
            .await
    }

    /// Search Civitai, optionally restricting to `types` (passed as repeated `types=` params).
    /// `nsfw` sets Civitai's `nsfw` query. False is their SFW catalog; true includes mature models.
    pub async fn search_types(
        &self,
        query: &str,
        limit: u32,
        types: &[&str],
        nsfw: bool,
    ) -> Result<Vec<CivitaiModelCard>> {
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let url = models_search_url(query, limit, types, nsfw);
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
                        detail.related =
                            related.into_iter().filter(|c| c.id != detail.id).collect();
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

    async fn related_checkpoints(
        &self,
        base_model: &str,
        limit: u32,
    ) -> Result<Vec<CivitaiModelCard>> {
        let url = format!(
            "https://civitai.com/api/v1/models?limit={limit}&types=Checkpoint&baseModels={}&sort={}",
            urlencoding_lite(base_model),
            urlencoding_lite("Most Downloaded")
        );
        let v = self
            .get_json(&url)
            .await
            .context("civitai related checkpoints")?;
        let mut out = Vec::new();
        if let Some(items) = v.get("items").and_then(|i| i.as_array()) {
            for item in items {
                out.push(parse_model_card(item, false));
            }
        }
        Ok(out)
    }

    /// Resolve a download URL from a Civitai model version (+ optional file id).
    pub async fn resolve_version_download(
        &self,
        version_id: i64,
        file_id: Option<i64>,
    ) -> Result<ResolvedDownload> {
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let url = format!("https://civitai.com/api/v1/model-versions/{version_id}");
        let v = self
            .get_json(&url)
            .await
            .context("civitai get model version")?;
        if v.get("error").is_some() {
            return Err(anyhow!(
                "civitai model version {}: {}",
                version_id,
                v.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("not found")
            ));
        }
        let version = parse_model_version(&v, 1);
        let model_id = v.get("modelId").and_then(|x| x.as_i64());
        let file = if let Some(fid) = file_id {
            version
                .files
                .iter()
                .find(|f| f.id == Some(fid))
                .or_else(|| pick_primary_file(&version.files))
        } else {
            pick_primary_file(&version.files)
        }
        .ok_or_else(|| anyhow!("civitai model version {version_id}: no files"))?;
        let download_url = file.download_url.clone().ok_or_else(|| {
            anyhow!("civitai model version {version_id}: file has no downloadUrl")
        })?;
        Ok(ResolvedDownload {
            download_url,
            version_id: version.id,
            version_name: Some(version.name),
            base_model: version.base_model,
            file_id: file.id,
            file_name: file.name.clone(),
            file_format: file.format.clone().or_else(|| file.file_type.clone()),
            model_id,
            thumbnail_url: version
                .image_urls
                .as_ref()
                .and_then(|imgs| imgs.first().cloned()),
        })
    }

    /// Resolve a version id → full model detail (for Installed cards that only have versionId).
    pub async fn get_model_by_version(&self, version_id: i64) -> Result<CivitaiModelDetail> {
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let url = format!("https://civitai.com/api/v1/model-versions/{version_id}");
        let v = self
            .get_json(&url)
            .await
            .context("civitai get model version")?;
        if v.get("error").is_some() {
            return Err(anyhow!(
                "civitai model version {}: {}",
                version_id,
                v.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("not found")
            ));
        }
        let model_id = v
            .get("modelId")
            .and_then(|x| x.as_i64())
            .ok_or_else(|| anyhow!("civitai model version {version_id}: missing modelId"))?;
        let mut detail = self.get_model(model_id).await?;
        // Prefer the installed version when present on the model.
        if detail.model_versions.iter().any(|ver| ver.id == version_id) {
            if let Some(ver) = detail
                .model_versions
                .iter()
                .find(|ver| ver.id == version_id)
                .cloned()
            {
                detail.version_id = Some(ver.id);
                detail.version_name = Some(ver.name.clone());
                detail.base_model = ver.base_model.clone().or(detail.base_model);
                detail.trained_words = ver.trained_words.clone().or(detail.trained_words);
                detail.trigger_word = detail
                    .trained_words
                    .as_ref()
                    .and_then(|w| w.first().cloned())
                    .or(detail.trigger_word);
                if let Some(imgs) = &ver.image_urls {
                    if !imgs.is_empty() {
                        detail.image_urls = Some(imgs.clone());
                        detail.thumbnail_url = Some(imgs[0].clone());
                    }
                }
                if let Some(file) = pick_primary_file(&ver.files) {
                    detail.file_id = file.id;
                    detail.file_name = file.name.clone();
                    detail.file_format = file.format.clone().or_else(|| file.file_type.clone());
                    detail.download_url = file.download_url.clone();
                }
            }
        }
        Ok(detail)
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("civitai search")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("civitai {status}: {text}"));
        }
        let v: Value = serde_json::from_str(&text)?;
        let mut out = Vec::new();
        if let Some(items) = v.get("items").and_then(|i| i.as_array()) {
            for item in items {
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
                let mut download_url = None;
                let mut version_id = None;
                let mut example_prompt = None;
                let mut thumbnail = None;
                if let Some(versions) = item.get("modelVersions").and_then(|m| m.as_array()) {
                    if let Some(ver) = versions.first() {
                        version_id = ver.get("id").and_then(|x| x.as_i64());
                        if let Some(files) = ver.get("files").and_then(|f| f.as_array()) {
                            download_url = files
                                .first()
                                .and_then(|f| f.get("downloadUrl"))
                                .and_then(|u| u.as_str())
                                .map(|s| s.to_string());
                        }
                        if let Some(images) = ver.get("images").and_then(|i| i.as_array()) {
                            if let Some(img) = images.first() {
                                thumbnail = img
                                    .get("url")
                                    .and_then(|u| u.as_str())
                                    .map(|s| s.to_string());
                                example_prompt = img
                                    .pointer("/meta/prompt")
                                    .and_then(|p| p.as_str())
                                    .map(|s| s.to_string());
                            }
                        }
                    }
                }
                out.push(CivitaiModelCard {
                    id,
                    name,
                    model_type,
                    version_id,
                    download_url,
                    thumbnail_url: thumbnail,
                    example_prompt,
                });
            }
        }
    }
    trained_words
}

fn parse_model_file(f: &Value) -> CivitaiModelFile {
    let size_kb = f
        .get("sizeKB")
        .and_then(|x| x.as_f64())
        .or_else(|| f.get("sizeKB").and_then(|x| x.as_i64()).map(|n| n as f64));
    let size_bytes = size_kb.map(|kb| (kb * 1024.0).round() as i64).or_else(|| {
        f.get("size")
            .and_then(|x| x.as_i64())
            .or_else(|| f.get("sizeBytes").and_then(|x| x.as_i64()))
    });
    let format = f
        .pointer("/metadata/format")
        .and_then(|x| x.as_str())
        .or_else(|| f.get("format").and_then(|x| x.as_str()))
        .map(|s| s.to_string());
    let file_type = f
        .get("type")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    CivitaiModelFile {
        id: f.get("id").and_then(|x| x.as_i64()),
        name: f
            .get("name")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        size_kb,
        size_bytes,
        file_type,
        format,
        primary: f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false),
        download_url: f
            .get("downloadUrl")
            .and_then(|u| u.as_str())
            .map(|s| s.to_string()),
    }
}

fn parse_model_version(ver: &Value, image_limit: usize) -> CivitaiModelVersion {
    let trained_words = parse_trained_words(ver);
    let files: Vec<CivitaiModelFile> = ver
        .get("files")
        .and_then(|f| f.as_array())
        .map(|arr| arr.iter().map(parse_model_file).collect())
        .unwrap_or_default();
    let mut image_urls = Vec::new();
    let mut example_prompt = None;
    if let Some(images) = ver.get("images").and_then(|i| i.as_array()) {
        for img in images.iter().take(image_limit) {
            if let Some(u) = img.get("url").and_then(|u| u.as_str()) {
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
    CivitaiModelVersion {
        id: ver.get("id").and_then(|x| x.as_i64()).unwrap_or(0),
        name: ver
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("untitled")
            .to_string(),
        base_model: ver
            .get("baseModel")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        created_at: ver
            .get("createdAt")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        trained_words: if trained_words.is_empty() {
            None
        } else {
            Some(trained_words)
        },
        files,
        image_urls: if image_urls.is_empty() {
            None
        } else {
            Some(image_urls)
        },
        example_prompt,
    }
}

fn pick_primary_file(files: &[CivitaiModelFile]) -> Option<&CivitaiModelFile> {
    files.iter().find(|f| f.primary).or_else(|| files.first())
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

    let image_limit = if detailed { 12 } else { 1 };
    let model_versions: Vec<CivitaiModelVersion> = item
        .get("modelVersions")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .map(|ver| parse_model_version(ver, image_limit))
                .collect()
        })
        .unwrap_or_default();

    // Default selection: first version + primary/first file (preserves prior behavior).
    let mut download_url = None;
    let mut version_id = None;
    let mut version_name = None;
    let mut file_id = None;
    let mut file_name = None;
    let mut file_format = None;
    let mut example_prompt = None;
    let mut thumbnail = None;
    let mut base_model = None;
    let mut trained_words: Vec<String> = Vec::new();
    let mut image_urls: Vec<String> = Vec::new();

    if let Some(ver) = model_versions.first() {
        version_id = Some(ver.id);
        version_name = Some(ver.name.clone());
        base_model = ver.base_model.clone().or_else(|| {
            item.get("baseModels")
                .and_then(|b| b.as_array())
                .and_then(|a| a.first())
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        });
        if let Some(words) = ver.trained_words.as_ref() {
            trained_words = words.clone();
        }
        if let Some(file) = pick_primary_file(&ver.files) {
            download_url = file.download_url.clone();
            file_id = file.id;
            file_name = file.name.clone();
            file_format = file.format.clone().or_else(|| file.file_type.clone());
        }
        if let Some(imgs) = ver.image_urls.as_ref() {
            for u in imgs {
                if thumbnail.is_none() {
                    thumbnail = Some(u.clone());
                }
                image_urls.push(u.clone());
            }
        }
        example_prompt = ver.example_prompt.clone();
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

    let nsfw = item.get("nsfw").and_then(|x| x.as_bool()).unwrap_or(false)
        || item
            .get("nsfwLevel")
            .and_then(|x| x.as_i64())
            .map(|n| n > 1)
            .unwrap_or(false)
        || crate::prefs::looks_nsfw_label(&name);

    CivitaiModelCard {
        id,
        name,
        model_type,
        version_id,
        version_name,
        file_id,
        file_name,
        file_format,
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
        nsfw,
        model_versions,
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
        file_id: card.file_id,
        file_name: card.file_name,
        file_format: card.file_format,
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
        model_versions: card.model_versions,
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

/// Civitai `GET /api/v1/models` search URL on `civitai.com` (same catalog as `civitai.red`).
/// Always sends `nsfw`: their default is false, which drops mature models from search
/// even when `GET /models/{id}` still returns them.
fn models_search_url(query: &str, limit: u32, types: &[&str], nsfw: bool) -> String {
    let mut url = format!(
        "https://civitai.com/api/v1/models?limit={limit}&nsfw={}&query={}",
        if nsfw { "true" } else { "false" },
        urlencoding_lite(query)
    );
    for t in types {
        let trimmed = t.trim();
        if !trimmed.is_empty() {
            url.push_str("&types=");
            url.push_str(&urlencoding_lite(trimmed));
        }
    }
    url
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_search_type_is_one_enum_value() {
        assert!(AGENT_SEARCH_TYPES.contains(&"LORA"));
        assert_eq!(AGENT_SEARCH_TYPES.len(), 7);
        assert_eq!(canonical_search_type("LORA").unwrap(), "LORA");
        assert_eq!(canonical_search_type("lora").unwrap(), "LORA");
        assert_eq!(canonical_search_type("LoCon").unwrap(), "LoCon");
        assert_eq!(canonical_search_type("dora").unwrap(), "DoRA");
        assert_eq!(canonical_search_type("lycoris").unwrap(), "LyCORIS");
        assert_eq!(canonical_search_type("checkpoint").unwrap(), "Checkpoint");
        assert_eq!(
            canonical_search_type("embedding").unwrap(),
            "TextualInversion"
        );
        assert_eq!(
            canonical_search_type("TextualInversion").unwrap(),
            "TextualInversion"
        );
        assert_eq!(canonical_search_type("vae").unwrap(), "VAE");
        assert!(canonical_search_type("").unwrap_err().contains("required"));
        assert!(canonical_search_type("Model")
            .unwrap_err()
            .contains("not a Civitai"));
        assert!(canonical_search_type("LORA,VAE")
            .unwrap_err()
            .contains("one type"));
        assert!(canonical_search_type("Upscaler").is_err());
        assert!(same_search_type("Embedding", "TextualInversion"));
        assert!(!same_search_type("Checkpoint", "LORA"));
    }

    #[test]
    fn search_url_sends_nsfw_from_show_nsfw() {
        let off = models_search_url("WAI-illustrious", 12, &["Checkpoint"], false);
        assert!(off.starts_with("https://civitai.com/api/v1/models?"));
        assert!(off.contains("nsfw=false"));
        assert!(!off.contains("nsfw=true"));
        assert!(off.contains("types=Checkpoint"));
        assert!(off.contains("query=WAI-illustrious"));
        let on = models_search_url("WAI-illustrious", 12, &["Checkpoint"], true);
        assert!(on.contains("nsfw=true"));
        assert!(!on.contains("nsfw=false"));
        let spaced = models_search_url("WAI illustrious", 12, &["Checkpoint"], true);
        assert!(spaced.contains("query=WAI%20illustrious"));
    }

    #[test]
    fn parse_card_defaults_to_first_version_primary_file() {
        let item = json!({
            "id": 42,
            "name": "Test LoRA",
            "type": "LORA",
            "nsfw": false,
            "creator": { "username": "alice" },
            "stats": { "downloadCount": 10, "thumbsUpCount": 2 },
            "modelVersions": [
                {
                    "id": 100,
                    "name": "v1",
                    "baseModel": "SDXL 1.0",
                    "createdAt": "2024-01-01T00:00:00.000Z",
                    "trainedWords": ["trigger"],
                    "files": [
                        {
                            "id": 1,
                            "name": "model.ckpt",
                            "sizeKB": 100.0,
                            "type": "Model",
                            "metadata": { "format": "PickleTensor" },
                            "primary": false,
                            "downloadUrl": "https://civitai.com/api/download/models/1"
                        },
                        {
                            "id": 2,
                            "name": "model.safetensors",
                            "sizeKB": 90.0,
                            "type": "Model",
                            "metadata": { "format": "SafeTensor" },
                            "primary": true,
                            "downloadUrl": "https://civitai.com/api/download/models/2"
                        }
                    ],
                    "images": [{ "url": "https://example.com/a.png", "meta": { "prompt": "hi" } }]
                },
                {
                    "id": 101,
                    "name": "v2",
                    "baseModel": "Pony",
                    "files": [{
                        "id": 3,
                        "name": "v2.safetensors",
                        "primary": true,
                        "downloadUrl": "https://civitai.com/api/download/models/3"
                    }]
                }
            ]
        });
        let card = parse_model_card(&item, true);
        assert_eq!(card.version_id, Some(100));
        assert_eq!(card.version_name.as_deref(), Some("v1"));
        assert_eq!(card.file_id, Some(2));
        assert_eq!(card.file_name.as_deref(), Some("model.safetensors"));
        assert_eq!(card.file_format.as_deref(), Some("SafeTensor"));
        assert_eq!(
            card.download_url.as_deref(),
            Some("https://civitai.com/api/download/models/2")
        );
        assert_eq!(card.base_model.as_deref(), Some("SDXL 1.0"));
        assert_eq!(card.model_versions.len(), 2);
        assert_eq!(card.model_versions[0].files.len(), 2);
        assert_eq!(card.model_versions[1].base_model.as_deref(), Some("Pony"));
        let detail = parse_model_detail(&item);
        assert_eq!(detail.model_versions.len(), 2);
        assert_eq!(detail.file_id, Some(2));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiModelFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_kb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    /// Civitai file `type` (e.g. Model, Pruned Model, Training Data).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    /// Format from metadata (SafeTensor, PickleTensor, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default)]
    pub primary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiModelVersion {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trained_words: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<CivitaiModelFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_prompt: Option<String>,
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
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_prompt: Option<String>,
}
