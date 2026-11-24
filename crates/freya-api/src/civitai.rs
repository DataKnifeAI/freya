//! Civitai search / download URL helpers (requires user-provided API key).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use tracing::debug;

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

/// One page from `GET /api/v1/images`. The next cursor or page is Civitai's, not a local cap.
#[derive(Debug, Clone)]
pub struct CommunityImagePage {
    pub items: Vec<CommunityImage>,
    pub next_cursor: Option<String>,
    pub next_page: Option<u32>,
}

/// First page is about 20. Civitai accepts up to 200; keep a smaller ceiling.
const COMMUNITY_IMAGE_LIMIT_MAX: u32 = 100;

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

    /// Stream bytes from an allowlisted Civitai image/video CDN URL (follows redirects).
    pub async fn fetch_media(&self, url: &str) -> Result<(Vec<u8>, String)> {
        if !civitai_media_url_ok(url) {
            return Err(anyhow!("media url not allowed"));
        }
        let resp = self
            .http
            .get(url.trim())
            .send()
            .await
            .context("civitai media upstream")?;
        if !resp.status().is_success() {
            return Err(anyhow!("civitai media upstream {}", resp.status()));
        }
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = resp.bytes().await.context("civitai media body")?;
        Ok((bytes.to_vec(), ct))
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
    /// `allow_nsfw` is Freya's Show NSFW preference. Recommended resources that are themselves
    /// NSFW are omitted when it is off.
    pub async fn get_model(&self, id: i64, allow_nsfw: bool) -> Result<CivitaiModelDetail> {
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
        self.attach_recommended(&mut detail, allow_nsfw).await;
        // Public v1 has no dependency graph. When the model lists recommended checkpoints,
        // those are the suggestion rows. Otherwise fall back to popular checkpoints for the family.
        let has_recommended_ckpt = detail
            .recommended_resources
            .iter()
            .any(|r| r.resource_type.eq_ignore_ascii_case("Checkpoint"));
        if !has_recommended_ckpt && !detail.model_type.eq_ignore_ascii_case("Checkpoint") {
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
        file_hash: Option<&str>,
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
        } else if let Some(hash) = file_hash.and_then(hash_token) {
            pick_file_by_hash(&version.files, &hash)
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
    pub async fn get_model_by_version(
        &self,
        version_id: i64,
        allow_nsfw: bool,
    ) -> Result<CivitaiModelDetail> {
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
        let mut detail = self.get_model(model_id, allow_nsfw).await?;
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
                detail.recommended_resources = ver.recommended_resources.clone();
                if let Some(url) = detail.civitai_url.clone() {
                    detail.civitai_url = Some(with_version_query(&url, ver.id));
                }
            }
        }
        Ok(detail)
    }

    /// `GET /api/v1/models/{id}` omits `recommendedResources`. Civitai MCP `get_model` and
    /// `get_model_version` wrap that same REST document and do not return the list either.
    /// `model.getById` on tRPC does. Failures leave the detail usable — we never invent a
    /// resource that was not in that payload.
    async fn attach_recommended(&self, detail: &mut CivitaiModelDetail, allow_nsfw: bool) {
        let mut any = false;
        for ver in &mut detail.model_versions {
            ver.recommended_resources = filter_recommended(&ver.recommended_resources, allow_nsfw);
            if !ver.recommended_resources.is_empty() {
                any = true;
            }
        }
        if !any && detail.id > 0 {
            match self.fetch_trpc_recommended(detail.id).await {
                Ok(rows) => {
                    for ver in &mut detail.model_versions {
                        if let Some((_, list)) = rows.iter().find(|(id, _)| *id == ver.id) {
                            ver.recommended_resources = filter_recommended(list, allow_nsfw);
                        }
                    }
                }
                Err(err) => debug!("recommended resources skipped: {err}"),
            }
        }
        detail.recommended_resources = detail
            .model_versions
            .iter()
            .find(|ver| Some(ver.id) == detail.version_id)
            .map(|ver| ver.recommended_resources.clone())
            .unwrap_or_default();
    }

    async fn fetch_trpc_recommended(
        &self,
        model_id: i64,
    ) -> Result<Vec<(i64, Vec<CivitaiRecommendedResource>)>> {
        let input = format!(r#"{{"json":{{"id":{model_id}}}}}"#);
        let url = format!(
            "https://civitai.com/api/trpc/model.getById?input={}",
            urlencoding_lite(&input)
        );
        let resp = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(8))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("civitai recommended resources")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("civitai recommended resources: {status}"));
        }
        let v: Value = serde_json::from_str(&text).context("civitai recommended resources json")?;
        let versions = v
            .pointer("/result/data/json/modelVersions")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(versions
            .iter()
            .filter_map(|ver| {
                let id = ver.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
                let list = parse_recommended_resources(ver);
                if id > 0 && !list.is_empty() {
                    Some((id, list))
                } else {
                    None
                }
            })
            .collect())
    }

    /// One version payload, including example-image meta. Does not walk related models.
    pub async fn version_example(&self, version_id: i64) -> Result<VersionExample> {
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let url = format!("https://civitai.com/api/v1/model-versions/{version_id}");
        let v = self
            .get_json_timeout(&url, std::time::Duration::from_secs(8))
            .await
            .context("civitai get model version meta")?;
        if v.get("error").is_some() {
            return Err(anyhow!(
                "civitai model version {}: {}",
                version_id,
                v.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("not found")
            ));
        }
        let ver = parse_model_version(&v, 12);
        let model_name = v
            .pointer("/model/name")
            .and_then(|n| n.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Ok(VersionExample {
            version_id: ver.id,
            version_name: ver.name,
            model_name,
            base_model: ver.base_model,
            example: ver.example_meta,
        })
    }

    /// Community posts made with a model or version.
    ///
    /// `search_images` / `get_image` on `https://mcp.civitai.com/mcp` are public reads, but the
    /// tool result keeps only image id, post id, and version ids. The text prompt is cut to
    /// 120 characters and omits negative, sampler, steps, CFG, seed, and resources. `get_image`
    /// looks at `meta.prompt`, while `GET /images?imageId=` nests that object under `meta.meta`,
    /// so the prompt is dropped. Those tools also have no NSFW argument: browsing level follows
    /// the API key only when Cloudflare country headers are present, which this server does not
    /// invent. The images list with `withMeta=true` is the payload that has the fields, and
    /// `nsfw` / `browsingLevel` is how Show NSFW is asked. No API key: the read is public.
    /// File download URLs stay on the version REST used by install.
    pub async fn community_images(
        &self,
        model_id: Option<i64>,
        version_id: Option<i64>,
        allow_nsfw: bool,
        limit: u32,
        cursor: Option<&str>,
        page: Option<u32>,
    ) -> Result<CommunityImagePage> {
        let model_id = model_id.filter(|n| *n > 0);
        let version_id = version_id.filter(|n| *n > 0);
        if model_id.is_none() && version_id.is_none() {
            return Err(anyhow!("modelId or modelVersionId is required"));
        }
        let limit = limit.clamp(1, COMMUNITY_IMAGE_LIMIT_MAX);
        let cursor = clean_image_cursor(cursor);
        let page = if cursor.is_some() {
            None
        } else {
            page.filter(|n| *n > 0 && *n <= 10_000)
        };
        let url = images_list_url(
            model_id,
            version_id,
            allow_nsfw,
            limit,
            cursor.as_deref(),
            page,
        );
        let v = self
            .get_public_json(&url)
            .await
            .context("civitai community images")?;
        let mut items = parse_community_images_limited(&v, version_id, allow_nsfw, limit as usize);
        items = dedupe_community_images(items);
        self.fill_resource_ids(&mut items).await;
        let (next_cursor, next_page) = if items.is_empty() {
            (None, None)
        } else {
            images_next(&v)
        };
        Ok(CommunityImagePage {
            items,
            next_cursor,
            next_page,
        })
    }

    /// Fill Civitai version ids for generation resources that only named a hash,
    /// and names for version ids that arrived without one. A miss stays as a
    /// filename or hash — callers must not invent a download for those.
    async fn fill_resource_ids(&self, images: &mut [CommunityImage]) {
        let mut hashes = Vec::new();
        for image in images.iter() {
            for resource in &image.resources {
                if resource.model_version_id.is_some_and(|n| n > 0) {
                    continue;
                }
                let Some(hash) = resource.hash.clone() else {
                    continue;
                };
                if !hashes.iter().any(|h: &String| h == &hash) {
                    hashes.push(hash);
                }
            }
        }
        hashes.truncate(24);
        for chunk in hashes.chunks(4) {
            let pending = chunk.iter().cloned().map(|hash| {
                let client = self.clone();
                async move {
                    let found = client.version_by_hash(&hash).await.ok();
                    (hash, found)
                }
            });
            for (hash, found) in futures::future::join_all(pending).await {
                let Some(found) = found else {
                    continue;
                };
                for image in images.iter_mut() {
                    for resource in &mut image.resources {
                        if resource.hash.as_deref() == Some(hash.as_str())
                            && resource.model_version_id.is_none()
                        {
                            apply_resolved_version(resource, &found);
                        }
                    }
                }
            }
        }

        let mut version_ids = Vec::new();
        for image in images.iter() {
            for resource in &image.resources {
                if !resource.name.trim().is_empty() {
                    continue;
                }
                let Some(id) = resource.model_version_id.filter(|n| *n > 0) else {
                    continue;
                };
                if !version_ids.contains(&id) {
                    version_ids.push(id);
                }
            }
        }
        version_ids.truncate(24);
        for chunk in version_ids.chunks(4) {
            let pending = chunk.iter().copied().map(|id| {
                let client = self.clone();
                async move {
                    let found = client.version_by_id_brief(id).await.ok();
                    (id, found)
                }
            });
            for (id, found) in futures::future::join_all(pending).await {
                let Some(found) = found else {
                    continue;
                };
                for image in images.iter_mut() {
                    for resource in &mut image.resources {
                        if resource.model_version_id == Some(id) && resource.name.trim().is_empty()
                        {
                            apply_resolved_version(resource, &found);
                        }
                    }
                }
            }
        }
    }

    async fn version_by_hash(&self, hash: &str) -> Result<ResolvedVersion> {
        let hash = hash_token(hash).ok_or_else(|| anyhow!("bad civitai hash"))?;
        let url = format!("https://civitai.com/api/v1/model-versions/by-hash/{hash}");
        let v = self.catalog_json(&url).await?;
        parse_resolved_version(&v).ok_or_else(|| anyhow!("civitai hash {hash}: not a version"))
    }

    async fn version_by_id_brief(&self, version_id: i64) -> Result<ResolvedVersion> {
        let url = format!("https://civitai.com/api/v1/model-versions/{version_id}");
        let v = self.catalog_json(&url).await?;
        parse_resolved_version(&v)
            .ok_or_else(|| anyhow!("civitai version {version_id}: unreadable"))
    }

    /// Public catalog first. Retry with the API key only when the public read
    /// is refused, so a 404 does not wait on a second call.
    async fn catalog_json(&self, url: &str) -> Result<Value> {
        let timeout = std::time::Duration::from_secs(8);
        match self.get_public_json_timeout(url, timeout).await {
            Ok(v) => Ok(v),
            Err(err) if self.configured() && catalog_retry_with_key(&err) => {
                self.get_json_timeout(url, timeout).await
            }
            Err(err) => Err(err),
        }
    }

    /// Latest version's primary file. Used when a generation resource has a model id only.
    pub async fn resolve_model_download(&self, model_id: i64) -> Result<ResolvedDownload> {
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let url = format!("https://civitai.com/api/v1/models/{model_id}");
        let v = self.get_json(&url).await.context("civitai get model")?;
        if v.get("error").is_some() {
            return Err(anyhow!(
                "civitai model {}: {}",
                model_id,
                v.get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("not found")
            ));
        }
        let detail = parse_model_detail(&v);
        let version_id = detail
            .version_id
            .ok_or_else(|| anyhow!("civitai model {model_id}: no version"))?;
        self.resolve_version_download(version_id, detail.file_id, None)
            .await
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        self.get_json_timeout(url, std::time::Duration::from_secs(30))
            .await
    }

    async fn get_json_timeout(&self, url: &str, timeout: std::time::Duration) -> Result<Value> {
        let resp = self
            .http
            .get(url)
            .timeout(timeout)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("civitai search")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("civitai {status}: {text}"));
        }
        serde_json::from_str(&text).context("civitai json")
    }

    /// Public catalog read. Omits `Authorization` so an account browsing level cannot
    /// override the explicit `nsfw` / `browsingLevel` query.
    async fn get_public_json(&self, url: &str) -> Result<Value> {
        self.get_public_json_timeout(url, std::time::Duration::from_secs(20))
            .await
    }

    async fn get_public_json_timeout(
        &self,
        url: &str,
        timeout: std::time::Duration,
    ) -> Result<Value> {
        let resp = self
            .http
            .get(url)
            .timeout(timeout)
            .header("Accept", "application/json")
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

/// Download target resolved from a version (+ optional file) id.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResolvedDownload {
    pub download_url: String,
    pub version_id: i64,
    pub version_name: Option<String>,
    pub base_model: Option<String>,
    pub file_id: Option<i64>,
    pub file_name: Option<String>,
    pub file_format: Option<String>,
    pub model_id: Option<i64>,
    pub thumbnail_url: Option<String>,
}

fn parse_trained_words(ver: &Value) -> Vec<String> {
    let mut trained_words = Vec::new();
    if let Some(words) = ver.get("trainedWords").and_then(|w| w.as_array()) {
        for w in words {
            if let Some(s) = w.as_str() {
                let trimmed = s.trim().trim_end_matches(',');
                if !trimmed.is_empty() {
                    trained_words.push(trimmed.to_string());
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
    let hashes = f
        .get("hashes")
        .and_then(|h| h.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
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
        hashes,
    }
}

fn civitai_model_page(id: i64, version_id: Option<i64>, nsfw: bool) -> String {
    let host = if nsfw {
        "https://civitai.red"
    } else {
        "https://civitai.com"
    };
    match version_id {
        Some(v) if v > 0 => format!("{host}/models/{id}?modelVersionId={v}"),
        _ => format!("{host}/models/{id}"),
    }
}

fn with_version_query(url: &str, version_id: i64) -> String {
    let base = url.split('?').next().unwrap_or(url);
    if version_id > 0 {
        format!("{base}?modelVersionId={version_id}")
    } else {
        base.to_string()
    }
}

fn value_is_nsfw(v: &Value) -> bool {
    v.get("nsfw").and_then(|x| x.as_bool()).unwrap_or(false)
        || v.get("nsfwLevel")
            .and_then(|x| x.as_i64())
            .map(|n| n > 1)
            .unwrap_or(false)
}

/// Map Civitai `recommendedResources` (nested `model.getById` shape, or a flat
/// `{id,name,type,modelVersionId,baseModel}` object) onto the detail payload.
/// Does not add resources that are absent from the object.
fn parse_recommended_resources(ver: &Value) -> Vec<CivitaiRecommendedResource> {
    let Some(arr) = ver.get("recommendedResources").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in arr {
        if let Some(rec) = parse_recommended_resource(item) {
            let key = (rec.id, rec.model_version_id);
            if !out
                .iter()
                .any(|e: &CivitaiRecommendedResource| (e.id, e.model_version_id) == key)
            {
                out.push(rec);
            }
        }
    }
    out
}

fn parse_recommended_resource(item: &Value) -> Option<CivitaiRecommendedResource> {
    if let Some(model) = item.get("model").filter(|m| m.get("id").is_some()) {
        let id = model.get("id").and_then(|x| x.as_i64())?;
        let name = model
            .get("name")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        let resource_type = model
            .get("type")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Checkpoint")
            .to_string();
        let model_version_id = item
            .get("modelVersionId")
            .and_then(|x| x.as_i64())
            .or_else(|| item.get("id").and_then(|x| x.as_i64()));
        let base_model = item
            .get("baseModel")
            .and_then(|x| x.as_str())
            .or_else(|| model.get("baseModel").and_then(|x| x.as_str()))
            .map(|s| s.to_string());
        return Some(CivitaiRecommendedResource {
            id,
            name: name.to_string(),
            resource_type,
            model_version_id,
            base_model,
            nsfw: value_is_nsfw(model) || value_is_nsfw(item),
        });
    }
    let id = item
        .get("id")
        .and_then(|x| x.as_i64())
        .or_else(|| item.get("modelId").and_then(|x| x.as_i64()))?;
    let name = item
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let resource_type = item
        .get("type")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Checkpoint")
        .to_string();
    Some(CivitaiRecommendedResource {
        id,
        name: name.to_string(),
        resource_type,
        model_version_id: item.get("modelVersionId").and_then(|x| x.as_i64()),
        base_model: item
            .get("baseModel")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        nsfw: value_is_nsfw(item),
    })
}

fn filter_recommended(
    list: &[CivitaiRecommendedResource],
    allow_nsfw: bool,
) -> Vec<CivitaiRecommendedResource> {
    list.iter()
        .filter(|r| allow_nsfw || !r.nsfw)
        .cloned()
        .collect()
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
    let images = ver.get("images").and_then(|i| i.as_array());
    if let Some(images) = images {
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
    let example_meta = ver
        .get("images")
        .and_then(|i| i.as_array())
        .and_then(|images| crate::genmeta::example_from_images(images.iter().take(24)));
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
        example_meta,
        recommended_resources: parse_recommended_resources(ver),
    }
}

fn pick_primary_file(files: &[CivitaiModelFile]) -> Option<&CivitaiModelFile> {
    files.iter().find(|f| f.primary).or_else(|| files.first())
}

fn pick_file_by_hash<'a>(files: &'a [CivitaiModelFile], hash: &str) -> Option<&'a CivitaiModelFile> {
    files.iter().find(|f| {
        f.hashes
            .values()
            .any(|h| hashes_match(h, hash))
    })
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

    let nsfw_level = item.get("nsfwLevel").and_then(|x| x.as_i64());
    let nsfw = item.get("nsfw").and_then(|x| x.as_bool()).unwrap_or(false)
        || nsfw_level.map(|n| n > 1).unwrap_or(false)
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
        civitai_url: Some(civitai_model_page(id, version_id, nsfw)),
        nsfw,
        nsfw_level,
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
        nsfw: card.nsfw,
        nsfw_level: card.nsfw_level,
        needs_base,
        model_versions: card.model_versions.clone(),
        recommended_resources: card
            .model_versions
            .iter()
            .find(|ver| Some(ver.id) == card.version_id)
            .map(|ver| ver.recommended_resources.clone())
            .unwrap_or_default(),
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

/// Community images for one model or version.
///
/// Show NSFW off: `nsfw=false` and browsing level 1 (PG). On: browsing level 31
/// (PG through XXX) so mature posts are included. `nsfw=true` on this endpoint
/// returns only mature posts, which would hide SFW examples of the same version.
fn images_list_url(
    model_id: Option<i64>,
    version_id: Option<i64>,
    allow_nsfw: bool,
    limit: u32,
    cursor: Option<&str>,
    page: Option<u32>,
) -> String {
    let limit = limit.clamp(1, COMMUNITY_IMAGE_LIMIT_MAX);
    let mut url =
        format!("https://civitai.com/api/v1/images?limit={limit}&withMeta=true&type=image");
    if allow_nsfw {
        url.push_str("&browsingLevel=31");
    } else {
        url.push_str("&nsfw=false&browsingLevel=1");
    }
    if let Some(id) = model_id {
        url.push_str(&format!("&modelId={id}"));
    }
    if let Some(id) = version_id {
        url.push_str(&format!("&modelVersionId={id}"));
    }
    if let Some(cursor) = clean_image_cursor(cursor) {
        url.push_str("&cursor=");
        url.push_str(&urlencoding_lite(&cursor));
    } else if let Some(page) = page.filter(|n| *n > 0 && *n <= 10_000) {
        url.push_str(&format!("&page={page}"));
    }
    url
}

fn parse_community_images(
    payload: &Value,
    context_version: Option<i64>,
    allow_nsfw: bool,
) -> Vec<CommunityImage> {
    parse_community_images_limited(
        payload,
        context_version,
        allow_nsfw,
        COMMUNITY_IMAGE_LIMIT_MAX as usize,
    )
}

fn parse_community_images_limited(
    payload: &Value,
    context_version: Option<i64>,
    allow_nsfw: bool,
    limit: usize,
) -> Vec<CommunityImage> {
    let Some(items) = payload.get("items").and_then(|i| i.as_array()) else {
        return Vec::new();
    };
    let limit = limit.clamp(1, COMMUNITY_IMAGE_LIMIT_MAX as usize);
    let mut out = Vec::new();
    for item in items {
        if item.get("type").and_then(|t| t.as_str()) == Some("video") {
            let has_url = item
                .get("url")
                .and_then(|u| u.as_str())
                .map(|s| s.starts_with("https://"))
                .unwrap_or(false);
            if !has_url {
                continue;
            }
        }
        let nsfw = image_is_nsfw(item);
        if nsfw && !allow_nsfw {
            continue;
        }
        let Some(image) = parse_community_image(item, context_version, nsfw) else {
            continue;
        };
        out.push(image);
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn clean_image_cursor(cursor: Option<&str>) -> Option<String> {
    let raw = cursor?.trim();
    if raw.is_empty() || raw.len() > 512 {
        return None;
    }
    if raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some(raw.to_string())
}

/// `metadata.nextCursor`, or `cursor` / `page` on `metadata.nextPage`.
fn images_next(payload: &Value) -> (Option<String>, Option<u32>) {
    let Some(meta) = payload.get("metadata") else {
        return (None, None);
    };
    let cursor = match meta.get("nextCursor") {
        Some(Value::String(s)) => clean_image_cursor(Some(s)),
        Some(Value::Number(n)) => n.as_i64().filter(|n| *n >= 0).map(|n| n.to_string()),
        _ => None,
    };
    let next_url = meta.get("nextPage").and_then(|v| v.as_str()).unwrap_or("");
    let cursor = cursor
        .or_else(|| query_param(next_url, "cursor").and_then(|s| clean_image_cursor(Some(&s))));
    if cursor.is_some() {
        return (cursor, None);
    }
    let page = query_param(next_url, "page")
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n > 0 && *n <= 10_000);
    (None, page)
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// Grid preview. The original `url` stays the download and the detail image.
fn civitai_preview_url(url: &str) -> String {
    if civitai_video_url(url) {
        return url.to_string();
    }
    if let Some(start) = url.find("/width=") {
        let after = &url[start + "/width=".len()..];
        if let Some(slash) = after.find('/') {
            let mut out = String::with_capacity(url.len() + 8);
            out.push_str(&url[..start]);
            out.push_str("/width=768");
            out.push_str(&after[slash..]);
            return out;
        }
    }
    url.replace("/original=true/", "/width=768/")
}

fn civitai_video_url(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains(".mp4") || u.contains(".webm") || u.contains(".mov")
}

/// Stable dedupe key: same Civitai file with thumbnail vs original paths counts once.
pub fn canonical_community_media_key(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() || !s.starts_with("https://") {
        return None;
    }
    let parsed = reqwest::Url::parse(s).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !civitai_media_host_ok(&host) {
        return None;
    }
    let path = parsed
        .path()
        .replace("/width=768/", "/original=true/")
        .replace("/width=512/", "/original=true/");
    let file = path.rsplit('/').next()?.trim();
    if file.is_empty() {
        return None;
    }
    Some(format!("{host}/{file}").to_ascii_lowercase())
}

pub fn civitai_media_host_ok(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h == "image.civitai.com"
        || h == "image.civitai.red"
        || h == "blobs-b2.civitai.com"
        || h.ends_with(".image.civitai.com")
}

/// Allowlisted Civitai CDN hosts for studio media proxy (`<video>` / download).
pub fn civitai_media_url_ok(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    parsed
        .host_str()
        .is_some_and(civitai_media_host_ok)
}

pub fn dedupe_community_images(items: Vec<CommunityImage>) -> Vec<CommunityImage> {
    let mut out = Vec::with_capacity(items.len());
    let mut seen_ids = HashSet::new();
    let mut seen_urls = HashSet::new();
    for image in items {
        let id_key = image.id.to_string();
        if seen_ids.contains(&id_key) {
            continue;
        }
        let url_key = canonical_community_media_key(
            image
                .url
                .as_deref()
                .or(image.thumbnail_url.as_deref()),
        );
        if url_key
            .as_ref()
            .is_some_and(|k| seen_urls.contains(k))
        {
            continue;
        }
        seen_ids.insert(id_key);
        if let Some(k) = url_key {
            seen_urls.insert(k);
        }
        out.push(image);
    }
    out
}

fn parse_community_image(
    item: &Value,
    context_version: Option<i64>,
    nsfw: bool,
) -> Option<CommunityImage> {
    let id = item.get("id").and_then(value_i64).filter(|n| *n > 0)?;
    let model_version_ids: Vec<i64> = item
        .get("modelVersionIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(value_i64)
                .filter(|n| *n > 0)
                .collect()
        })
        .unwrap_or_default();
    let link_version = context_version
        .filter(|n| *n > 0)
        .or_else(|| model_version_ids.first().copied());
    let meta = image_generation_meta(item);
    let prompt = meta.and_then(|m| meta_string(m, &["prompt"]));
    let negative_prompt = meta.and_then(|m| meta_string(m, &["negativeprompt", "negative"]));
    let sampler = meta.and_then(|m| meta_string(m, &["sampler"]));
    let steps = meta.and_then(|m| meta_u32(m, &["steps"]));
    let cfg = meta.and_then(|m| meta_f64(m, &["cfgscale", "cfg"]));
    let seed = meta.and_then(|m| meta_i64(m, &["seed"]));
    let clip_skip = meta.and_then(|m| meta_u32(m, &["clipskip"]));
    let size = meta
        .and_then(|m| meta_string(m, &["size"]))
        .or_else(|| image_size(item));
    let url = item
        .get("url")
        .and_then(|u| u.as_str())
        .map(str::trim)
        .filter(|s| s.starts_with("https://"))
        .map(str::to_string);
    let nsfw_level = match item.get("nsfwLevel") {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    let media_type = item
        .get("type")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(CommunityImage {
        id,
        post_id: item.get("postId").and_then(value_i64).filter(|n| *n > 0),
        prompt,
        negative_prompt,
        sampler,
        steps,
        cfg,
        seed,
        clip_skip,
        size,
        url: url.clone(),
        thumbnail_url: url.as_deref().map(civitai_preview_url),
        media_type,
        nsfw,
        nsfw_level,
        civitai_url: community_image_page(id, link_version, nsfw),
        resources: meta.map(parse_image_resources).unwrap_or_default(),
        model_version_ids,
        checkpoint: None,
        checkpoint_installed: false,
        checkpoint_different: false,
        missing_resources: Vec::new(),
    })
}

fn image_generation_meta(item: &Value) -> Option<&Value> {
    let meta = item.get("meta").filter(|m| m.is_object())?;
    if meta_has_generation(meta) {
        return Some(meta);
    }
    meta.get("meta")
        .filter(|m| m.is_object() && meta_has_generation(m))
}

fn meta_has_generation(meta: &Value) -> bool {
    meta.get("prompt").is_some()
        || meta.get("negativePrompt").is_some()
        || meta.get("sampler").is_some()
        || meta.get("steps").is_some()
        || meta.get("resources").is_some()
        || meta.get("civitaiResources").is_some()
}

fn image_is_nsfw(item: &Value) -> bool {
    if item.get("nsfw").and_then(|x| x.as_bool()) == Some(true) {
        return true;
    }
    match item.get("nsfwLevel") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(1) > 1,
        Some(Value::String(s)) => {
            let l = s.trim().to_ascii_lowercase();
            !(l.is_empty() || l == "none" || l == "pg" || l == "sfw" || l == "1")
        }
        _ => item
            .get("browsingLevel")
            .and_then(value_i64)
            .is_some_and(|n| n > 1),
    }
}

fn community_image_page(image_id: i64, version_id: Option<i64>, nsfw: bool) -> String {
    let host = if nsfw {
        "https://civitai.red"
    } else {
        "https://civitai.com"
    };
    match version_id {
        Some(v) if v > 0 => format!("{host}/images/{image_id}?modelVersionId={v}"),
        _ => format!("{host}/images/{image_id}"),
    }
}

fn parse_image_resources(meta: &Value) -> Vec<CommunityImageResource> {
    let mut out = Vec::new();
    for key in ["resources", "civitaiResources"] {
        let Some(arr) = meta.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        for item in arr {
            let Some(resource) = parse_one_image_resource(item) else {
                continue;
            };
            upsert_image_resource(&mut out, resource);
        }
    }
    if let Some(model) = meta_string(meta, &["model"]) {
        upsert_image_resource(
            &mut out,
            CommunityImageResource {
                name: model.chars().take(120).collect(),
                resource_type: "Checkpoint".into(),
                weight: None,
                model_version_id: None,
                model_id: None,
                hash: meta_string(meta, &["modelhash"]).and_then(|h| hash_token(&h)),
                installable: None,
                dest_path: None,
            },
        );
    }
    if let Some(obj) = meta.get("hashes").and_then(|h| h.as_object()) {
        for (key, value) in obj {
            let Some(hash) = value.as_str().and_then(hash_token) else {
                continue;
            };
            let nk = norm_meta_key(key);
            if nk == "model" || nk == "checkpoint" {
                if let Some(existing) = out
                    .iter_mut()
                    .find(|r| is_checkpoint_type(&r.resource_type))
                {
                    if existing.hash.is_none() {
                        existing.hash = Some(hash);
                    }
                }
                continue;
            }
            let label = key
                .split_once(':')
                .map(|(_, name)| name.trim())
                .unwrap_or(key);
            if let Some(existing) = out
                .iter_mut()
                .find(|r| !r.name.trim().is_empty() && r.name.eq_ignore_ascii_case(label))
            {
                if existing.hash.is_none() {
                    existing.hash = Some(hash);
                }
            }
        }
    }
    out
}

fn parse_one_image_resource(item: &Value) -> Option<CommunityImageResource> {
    // Weight 0 is listed on the post but was not applied.
    if item
        .get("weight")
        .and_then(value_f64)
        .is_some_and(|w| w == 0.0)
    {
        return None;
    }
    let name = item
        .get("name")
        .and_then(|n| n.as_str())
        .or_else(|| item.get("modelName").and_then(|n| n.as_str()))
        .or_else(|| item.get("modelVersionName").and_then(|n| n.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(120).collect::<String>())
        .unwrap_or_default();
    let hash = item
        .get("hash")
        .or_else(|| item.get("modelHash"))
        .or_else(|| item.get("sha256"))
        .and_then(|v| v.as_str())
        .and_then(hash_token);
    let model_version_id = item
        .get("modelVersionId")
        .or_else(|| item.get("versionId"))
        .and_then(value_i64)
        .filter(|n| *n > 0);
    let model_id = item.get("modelId").and_then(value_i64).filter(|n| *n > 0);
    if name.is_empty() && hash.is_none() && model_version_id.is_none() && model_id.is_none() {
        return None;
    }
    let resource_type = item
        .get("type")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("model")
        .to_string();
    Some(CommunityImageResource {
        name,
        resource_type,
        weight: item.get("weight").and_then(value_f64),
        model_version_id,
        model_id,
        hash,
        installable: None,
        dest_path: None,
    })
}

fn upsert_image_resource(out: &mut Vec<CommunityImageResource>, resource: CommunityImageResource) {
    if let Some(existing) = out.iter_mut().find(|e| same_image_resource(e, &resource)) {
        merge_image_resource(existing, resource);
        return;
    }
    out.push(resource);
}

fn same_image_resource(a: &CommunityImageResource, b: &CommunityImageResource) -> bool {
    match (
        a.model_version_id.filter(|n| *n > 0),
        b.model_version_id.filter(|n| *n > 0),
    ) {
        (Some(x), Some(y)) => return x == y,
        _ => {}
    }
    if let (Some(x), Some(y)) = (a.hash.as_deref(), b.hash.as_deref()) {
        return hashes_match(x, y);
    }
    let an = asset_stem_key(&a.name);
    let bn = asset_stem_key(&b.name);
    !an.is_empty() && an == bn && kinds_compatible(&a.resource_type, &b.resource_type)
}

fn merge_image_resource(dst: &mut CommunityImageResource, src: CommunityImageResource) {
    if dst.name.trim().is_empty() {
        dst.name = src.name;
    }
    if is_checkpoint_type(&src.resource_type) && !is_checkpoint_type(&dst.resource_type) {
        dst.resource_type = src.resource_type;
    }
    if dst.weight.is_none() {
        dst.weight = src.weight;
    }
    if dst.model_version_id.is_none() {
        dst.model_version_id = src.model_version_id;
    }
    if dst.model_id.is_none() {
        dst.model_id = src.model_id;
    }
    if dst.hash.is_none() {
        dst.hash = src.hash;
    }
}

fn catalog_retry_with_key(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    !(msg.contains("civitai 404") || msg.contains("civitai 400"))
}

struct ResolvedVersion {
    version_id: i64,
    model_id: Option<i64>,
    model_name: Option<String>,
    version_name: Option<String>,
    file_name: Option<String>,
    model_type: Option<String>,
}

fn parse_resolved_version(v: &Value) -> Option<ResolvedVersion> {
    let version_id = v.get("id").and_then(value_i64).filter(|n| *n > 0)?;
    let model_name = v
        .pointer("/model/name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(120).collect());
    let model_type = v
        .pointer("/model/type")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let version_name = v
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(120).collect());
    let file_name = v
        .get("files")
        .and_then(|f| f.as_array())
        .and_then(|files| {
            files
                .iter()
                .find(|f| f.get("primary").and_then(|p| p.as_bool()) == Some(true))
                .or_else(|| files.first())
        })
        .and_then(|f| f.get("name").and_then(|n| n.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(ResolvedVersion {
        version_id,
        model_id: v.get("modelId").and_then(value_i64).filter(|n| *n > 0),
        model_name,
        version_name,
        file_name,
        model_type,
    })
}

fn apply_resolved_version(resource: &mut CommunityImageResource, resolved: &ResolvedVersion) {
    if resource.model_version_id.is_none() {
        resource.model_version_id = Some(resolved.version_id);
    }
    if resource.model_id.is_none() {
        resource.model_id = resolved.model_id;
    }
    if resource.name.trim().is_empty() {
        resource.name = resolved_display_name(resolved);
    }
    if resource.resource_type.trim().is_empty() {
        if let Some(ty) = resolved.model_type.clone() {
            resource.resource_type = ty;
        }
    }
}

fn resolved_display_name(resolved: &ResolvedVersion) -> String {
    if let Some(file) = resolved.file_name.as_deref() {
        let stem = file_stem(file);
        if !stem.is_empty() {
            return stem;
        }
    }
    if let Some(name) = resolved.model_name.clone() {
        return name;
    }
    if let Some(name) = resolved.version_name.clone() {
        return name;
    }
    format!("version {}", resolved.version_id)
}

fn file_stem(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let lower = file.to_ascii_lowercase();
    let stem = if let Some(suffix) = [".safetensors", ".ckpt", ".pt", ".gguf", ".bin"]
        .iter()
        .find(|suffix| lower.ends_with(*suffix))
    {
        &file[..file.len() - suffix.len()]
    } else {
        file
    };
    stem.trim().chars().take(120).collect()
}

fn hash_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(['"', '\'']);
    let hex = trimmed.rsplit([':', '/']).next().unwrap_or(trimmed).trim();
    // AutoV2 is 10 hex; shorter CRC32/AutoV1 values collide on by-hash.
    if hex.len() < 10 || hex.len() > 128 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

fn hashes_match(a: &str, b: &str) -> bool {
    let a = a.trim().to_ascii_lowercase();
    let b = b.trim().to_ascii_lowercase();
    if a.len() < 10 || b.len() < 10 {
        return false;
    }
    a == b || a.starts_with(&b) || b.starts_with(&a)
}

pub(crate) fn is_checkpoint_type(raw: &str) -> bool {
    matches!(
        norm_meta_key(raw).as_str(),
        "checkpoint" | "model" | "ckpt" | "base"
    )
}

fn is_lora_type(raw: &str) -> bool {
    let key = norm_meta_key(raw);
    matches!(key.as_str(), "lora" | "locon" | "dora" | "lycoris" | "lyco") || key.contains("lora")
}

fn is_embedding_type(raw: &str) -> bool {
    matches!(
        norm_meta_key(raw).as_str(),
        "textualinversion" | "embedding" | "embed" | "textinversion"
    )
}

fn is_install_candidate(raw: &str) -> bool {
    is_checkpoint_type(raw) || is_lora_type(raw) || is_embedding_type(raw)
}

fn kind_bucket(raw: &str) -> &'static str {
    if is_checkpoint_type(raw) {
        "checkpoint"
    } else if is_embedding_type(raw) {
        "embedding"
    } else if is_lora_type(raw) {
        "lora"
    } else {
        "other"
    }
}

fn kinds_compatible(resource_type: &str, asset_kind: &str) -> bool {
    let resource = kind_bucket(resource_type);
    let asset = kind_bucket(asset_kind);
    resource == asset || asset == "other" || resource == "other"
}

fn asset_stem_key(raw: &str) -> String {
    file_stem(raw)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn resource_label(resource: &CommunityImageResource) -> String {
    let name = resource.name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    if let Some(id) = resource.model_version_id.filter(|n| *n > 0) {
        return format!("version {id}");
    }
    if let Some(hash) = resource.hash.as_deref() {
        return format!("hash {hash}");
    }
    "checkpoint".into()
}

fn resource_installable(resource: &CommunityImageResource) -> bool {
    resource.model_version_id.is_some_and(|n| n > 0) || resource.model_id.is_some_and(|n| n > 0)
}

fn canonical_install_type(raw: &str) -> String {
    let key = norm_meta_key(raw);
    if is_checkpoint_type(raw) {
        "Checkpoint".into()
    } else if is_embedding_type(raw) {
        "TextualInversion".into()
    } else if key == "locon" {
        "LoCon".into()
    } else if key == "dora" {
        "DoRA".into()
    } else if key == "lycoris" || key == "lyco" {
        "LyCORIS".into()
    } else {
        "LORA".into()
    }
}

fn safe_file_stem(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "model".into()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn dest_path_for_resource(resource: &CommunityImageResource) -> String {
    let folder = if is_checkpoint_type(&resource.resource_type) {
        "checkpoints"
    } else if is_embedding_type(&resource.resource_type) {
        "embeddings"
    } else {
        "loras"
    };
    let stem = safe_file_stem(&resource_label(resource));
    format!("{folder}/{stem}.safetensors")
}

fn checkpoint_of(image: &CommunityImage) -> Option<&CommunityImageResource> {
    let mut found: Option<&CommunityImageResource> = None;
    for resource in &image.resources {
        if !is_checkpoint_type(&resource.resource_type) {
            continue;
        }
        match found {
            None => found = Some(resource),
            Some(prev) if prev.name.trim().is_empty() && !resource.name.trim().is_empty() => {
                found = Some(resource);
            }
            _ => {}
        }
    }
    found
}

pub fn resource_matches(resource: &CommunityImageResource, asset: &InstalledAssetMatch) -> bool {
    match (
        resource.model_version_id.filter(|n| *n > 0),
        asset.version_id.filter(|n| *n > 0),
    ) {
        (Some(a), Some(b)) => return a == b,
        _ => {}
    }
    match (
        resource.model_id.filter(|n| *n > 0),
        asset.model_id.filter(|n| *n > 0),
    ) {
        (Some(a), Some(b)) if a != b => return false,
        (Some(_), Some(_)) if resource.model_version_id.filter(|n| *n > 0).is_none() => {
            return kinds_compatible(&resource.resource_type, &asset.kind);
        }
        _ => {}
    }
    if let (Some(left), Some(right)) = (resource.hash.as_deref(), asset.hash.as_deref()) {
        if hashes_match(left, right) {
            return true;
        }
    }
    if !kinds_compatible(&resource.resource_type, &asset.kind) {
        return false;
    }
    let want = asset_stem_key(&resource.name);
    if want.is_empty() {
        return false;
    }
    let name = asset_stem_key(&asset.name);
    let path = asset_stem_key(&asset.path);
    (!name.is_empty() && want == name) || (!path.is_empty() && want == path)
}

/// Compare each post's checkpoint and other generation resources to installed files.
pub fn annotate_community_images(images: &mut [CommunityImage], inventory: &StudioInventory) {
    for image in images {
        annotate_community_image(image, inventory);
    }
}

fn annotate_community_image(image: &mut CommunityImage, inventory: &StudioInventory) {
    let checkpoint = checkpoint_of(image).cloned();
    match checkpoint.as_ref() {
        Some(resource) => {
            let installed = inventory
                .installed
                .iter()
                .any(|asset| resource_matches(resource, asset));
            let same_base = inventory
                .current_base
                .as_ref()
                .is_some_and(|asset| resource_matches(resource, asset));
            image.checkpoint = Some(resource_label(resource));
            image.checkpoint_installed = installed;
            image.checkpoint_different = inventory.base_known && !same_base;
        }
        None => {
            image.checkpoint = None;
            image.checkpoint_installed = false;
            image.checkpoint_different = false;
        }
    }
    image.missing_resources = image
        .resources
        .iter()
        .filter(|resource| {
            is_install_candidate(&resource.resource_type)
                && !inventory
                    .installed
                    .iter()
                    .any(|asset| resource_matches(resource, asset))
        })
        .map(|resource| {
            let mut missing = resource.clone();
            if missing.name.trim().is_empty() {
                missing.name = resource_label(resource);
            }
            missing.resource_type = canonical_install_type(&resource.resource_type);
            missing.installable = Some(resource_installable(resource));
            missing.dest_path = Some(dest_path_for_resource(&missing));
            missing
        })
        .collect();
}

/// Worker files the gallery can match by name, hash, or Civitai id.
#[derive(Debug, Clone, Default)]
pub struct StudioInventory {
    pub installed: Vec<InstalledAssetMatch>,
    /// Active base, when the worker answered. Empty means no base is selected.
    pub current_base: Option<InstalledAssetMatch>,
    /// False when the worker is configured but the base read failed.
    pub base_known: bool,
}

#[derive(Debug, Clone)]
pub struct InstalledAssetMatch {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub version_id: Option<i64>,
    pub model_id: Option<i64>,
    pub hash: Option<String>,
}

fn meta_string(meta: &Value, names: &[&str]) -> Option<String> {
    let obj = meta.as_object()?;
    for (k, v) in obj {
        let nk = norm_meta_key(k);
        if names.iter().any(|n| nk == *n) {
            let text = v.as_str()?.trim();
            if text.is_empty() {
                return None;
            }
            return Some(text.chars().take(4000).collect());
        }
    }
    None
}

fn meta_f64(meta: &Value, names: &[&str]) -> Option<f64> {
    let obj = meta.as_object()?;
    for (k, v) in obj {
        let nk = norm_meta_key(k);
        if names.iter().any(|n| nk == *n) {
            return value_f64(v).filter(|n| n.is_finite() && *n >= 0.0);
        }
    }
    None
}

fn meta_u32(meta: &Value, names: &[&str]) -> Option<u32> {
    meta_f64(meta, names).and_then(|n| {
        if n > 0.0 && n <= u32::MAX as f64 {
            Some(n.round() as u32)
        } else {
            None
        }
    })
}

fn meta_i64(meta: &Value, names: &[&str]) -> Option<i64> {
    let obj = meta.as_object()?;
    for (k, v) in obj {
        let nk = norm_meta_key(k);
        if names.iter().any(|n| nk == *n) {
            return value_i64(v);
        }
    }
    None
}

fn norm_meta_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn value_i64(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| {
            v.as_f64().and_then(|n| {
                if n.is_finite() {
                    Some(n.round() as i64)
                } else {
                    None
                }
            })
        })
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn value_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn image_size(item: &Value) -> Option<String> {
    let w = item.get("width").and_then(value_i64).filter(|n| *n > 0)?;
    let h = item.get("height").and_then(value_i64).filter(|n| *n > 0)?;
    Some(format!("{w}x{h}"))
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
    fn community_images_dedupe_id_and_canonical_url() {
        let items = vec![
            CommunityImage {
                id: 1,
                post_id: None,
                prompt: Some("a".into()),
                negative_prompt: None,
                sampler: None,
                steps: None,
                cfg: None,
                seed: None,
                clip_skip: None,
                size: None,
                url: Some(
                    "https://image.civitai.com/x/a/original=true/a.jpeg".into(),
                ),
                thumbnail_url: Some(
                    "https://image.civitai.com/x/a/width=768/a.jpeg".into(),
                ),
                media_type: Some("image".into()),
                nsfw: false,
                nsfw_level: None,
                civitai_url: "https://civitai.com/images/1".into(),
                resources: vec![],
                model_version_ids: vec![],
                checkpoint: None,
                checkpoint_installed: false,
                checkpoint_different: false,
                missing_resources: vec![],
            },
            CommunityImage {
                id: 2,
                post_id: None,
                prompt: Some("dup".into()),
                negative_prompt: None,
                sampler: None,
                steps: None,
                cfg: None,
                seed: None,
                clip_skip: None,
                size: None,
                url: Some(
                    "https://image.civitai.com/x/a/width=512/a.jpeg".into(),
                ),
                thumbnail_url: None,
                media_type: Some("image".into()),
                nsfw: false,
                nsfw_level: None,
                civitai_url: "https://civitai.com/images/2".into(),
                resources: vec![],
                model_version_ids: vec![],
                checkpoint: None,
                checkpoint_installed: false,
                checkpoint_different: false,
                missing_resources: vec![],
            },
            CommunityImage {
                id: 1,
                post_id: None,
                prompt: Some("repeat id".into()),
                negative_prompt: None,
                sampler: None,
                steps: None,
                cfg: None,
                seed: None,
                clip_skip: None,
                size: None,
                url: Some(
                    "https://image.civitai.com/x/b/original=true/b.jpeg".into(),
                ),
                thumbnail_url: None,
                media_type: Some("image".into()),
                nsfw: false,
                nsfw_level: None,
                civitai_url: "https://civitai.com/images/1".into(),
                resources: vec![],
                model_version_ids: vec![],
                checkpoint: None,
                checkpoint_installed: false,
                checkpoint_different: false,
                missing_resources: vec![],
            },
        ];
        let deduped = dedupe_community_images(items);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].id, 1);
    }

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
        assert_eq!(
            card.civitai_url.as_deref(),
            Some("https://civitai.com/models/42?modelVersionId=100")
        );
    }

    #[test]
    fn nsfw_level_uses_civitai_red_and_keeps_recommended_checkpoint() {
        let item = json!({
            "id": 1319790,
            "name": "Frankie Foster",
            "type": "LORA",
            "nsfw": false,
            "nsfwLevel": 3,
            "modelVersions": [{
                "id": 1490040,
                "name": "-Illustrious XL-",
                "baseModel": "Illustrious",
                "files": [],
                "recommendedResources": [{
                    "id": 1410435,
                    "name": "v11.0",
                    "baseModel": "Illustrious",
                    "model": {
                        "id": 827184,
                        "name": "WAI-illustrious-SDXL",
                        "type": "Checkpoint",
                        "nsfw": true
                    }
                }]
            }]
        });
        let detail = parse_model_detail(&item);
        assert_eq!(
            detail.civitai_url.as_deref(),
            Some("https://civitai.red/models/1319790?modelVersionId=1490040")
        );
        assert!(detail.nsfw);
        assert_eq!(detail.nsfw_level, Some(3));
        assert_eq!(detail.recommended_resources.len(), 1);
        let rec = &detail.recommended_resources[0];
        assert_eq!(rec.id, 827184);
        assert_eq!(rec.name, "WAI-illustrious-SDXL");
        assert_eq!(rec.resource_type, "Checkpoint");
        assert_eq!(rec.model_version_id, Some(1410435));
        assert_eq!(rec.base_model.as_deref(), Some("Illustrious"));
        assert!(rec.nsfw);
        assert!(filter_recommended(&detail.recommended_resources, false).is_empty());
        assert_eq!(
            filter_recommended(&detail.recommended_resources, true).len(),
            1
        );
    }

    #[test]
    fn flat_recommended_resource_is_kept() {
        let item = json!({
            "recommendedResources": [{
                "id": 827184,
                "name": "WAI-illustrious-SDXL",
                "type": "Checkpoint",
                "modelVersionId": 1410435,
                "baseModel": "Illustrious",
                "nsfw": false
            }]
        });
        let list = parse_recommended_resources(&item);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, 827184);
        assert_eq!(list[0].model_version_id, Some(1410435));
        assert!(!list[0].nsfw);
        assert_eq!(filter_recommended(&list, false).len(), 1);
    }

    #[test]
    fn community_images_keep_prompt_meta_and_color_the_link() {
        let sfw_url = images_list_url(Some(1319790), Some(1490040), false, 40, None, None);
        assert!(sfw_url.starts_with("https://civitai.com/api/v1/images?"));
        assert!(sfw_url.contains("withMeta=true"));
        assert!(sfw_url.contains("nsfw=false"));
        assert!(sfw_url.contains("browsingLevel=1"));
        assert!(sfw_url.contains("modelId=1319790"));
        assert!(sfw_url.contains("modelVersionId=1490040"));
        assert!(sfw_url.contains("limit=40"));
        assert!(!sfw_url.contains("cursor="));
        assert!(!sfw_url.contains("mcp.civitai.com"));
        let mature = images_list_url(Some(1319790), Some(1490040), true, 12, None, None);
        assert!(mature.contains("browsingLevel=31"));
        assert!(!mature.contains("nsfw=true"));

        let payload = json!({
            "items": [
                {
                    "id": 11,
                    "postId": 22,
                    "type": "image",
                    "nsfw": false,
                    "nsfwLevel": "None",
                    "width": 768,
                    "height": 1024,
                    "url": "https://image.civitai.com/x/abc/original=true/abc.jpeg",
                    "modelVersionIds": [1490040, 1410435],
                    "meta": {
                        "prompt": "red hair, smile",
                        "negativePrompt": "lowres",
                        "sampler": "Euler a",
                        "steps": 23,
                        "cfgScale": 7,
                        "seed": 3321325136_i64,
                        "clipSkip": 2,
                        "Size": "768x1024",
                        "Model": "example-checkpoint",
                        "resources": [
                            {"name": "FrankieXL", "type": "lora", "weight": 1},
                            {"name": "example-checkpoint", "type": "checkpoint", "weight": 1}
                        ]
                    }
                },
                {
                    "id": 12,
                    "type": "image",
                    "nsfwLevel": "X",
                    "url": "https://image.civitai.com/x/def/original=true/def.jpeg",
                    "meta": { "prompt": "mature post" }
                },
                {
                    "id": 13,
                    "type": "video",
                    "meta": { "prompt": "skip me" }
                },
                {
                    "id": 14,
                    "type": "video",
                    "nsfw": false,
                    "url": "https://image.civitai.com/xG1nkqKTMzGDvpLrqFT7WA/2bfe830d-46ae-40f8-8d6f-6166e09ac559/original=true/61422271.mp4",
                    "meta": { "prompt": "walking loop" }
                }
            ]
        });
        let sfw = parse_community_images(&payload, Some(1490040), false);
        let ids: Vec<i64> = sfw.iter().map(|i| i.id).collect();
        assert!(ids == vec![11, 14], "unexpected ids: {ids:?}");
        let video = sfw.iter().find(|i| i.id == 14).expect("mp4 video post");
        assert_eq!(video.media_type.as_deref(), Some("video"));
        assert!(video.url.as_deref().unwrap().ends_with("61422271.mp4"));
        assert_eq!(sfw[0].prompt.as_deref(), Some("red hair, smile"));
        assert_eq!(sfw[0].negative_prompt.as_deref(), Some("lowres"));
        assert_eq!(sfw[0].sampler.as_deref(), Some("Euler a"));
        assert_eq!(sfw[0].steps, Some(23));
        assert_eq!(sfw[0].cfg, Some(7.0));
        assert_eq!(sfw[0].seed, Some(3321325136));
        assert_eq!(sfw[0].clip_skip, Some(2));
        assert_eq!(sfw[0].size.as_deref(), Some("768x1024"));
        assert!(sfw[0]
            .thumbnail_url
            .as_deref()
            .unwrap()
            .contains("/width=768/"));
        assert_eq!(
            sfw[0].civitai_url,
            "https://civitai.com/images/11?modelVersionId=1490040"
        );
        assert!(!sfw[0].nsfw);
        assert!(sfw[0].resources.iter().any(|r| r.name == "FrankieXL"));
        assert_eq!(
            sfw[0]
                .resources
                .iter()
                .filter(|r| r.name == "example-checkpoint")
                .count(),
            1
        );

        let both = parse_community_images(&payload, Some(1490040), true);
        assert_eq!(both.len(), 3);
        assert_eq!(
            both[1].civitai_url,
            "https://civitai.red/images/12?modelVersionId=1490040"
        );
        assert!(both[1].nsfw);
        assert_eq!(both[2].id, 14);

        let nested = json!({
            "items": [{
                "id": 14,
                "nsfwLevel": "None",
                "meta": { "id": 1, "meta": { "prompt": "nested prompt", "sampler": "Euler" } }
            }]
        });
        let nested_items = parse_community_images(&nested, None, false);
        assert_eq!(nested_items[0].prompt.as_deref(), Some("nested prompt"));
        assert_eq!(nested_items[0].civitai_url, "https://civitai.com/images/14");

        let paged = images_list_url(Some(1), None, false, 20, Some("next-cursor"), Some(4));
        assert!(paged.contains("limit=20"));
        assert!(paged.contains("cursor=next-cursor"));
        assert!(!paged.contains("page="));
        let by_page = images_list_url(Some(1), None, true, 20, None, Some(2));
        assert!(by_page.contains("page=2"));
        assert!(by_page.contains("browsingLevel=31"));
        let wide: Vec<_> = (1..=15)
            .map(|id| {
                json!({
                    "id": id,
                    "type": "image",
                    "nsfwLevel": "None",
                    "url": format!("https://image.civitai.com/x/{id}/original=true/{id}.jpeg"),
                    "meta": { "prompt": "page" }
                })
            })
            .collect();
        let page_payload = json!({
            "items": wide,
            "metadata": {
                "nextCursor": "15",
                "nextPage": "https://civitai.com/api/v1/images?limit=20&cursor=15"
            }
        });
        let page_items = parse_community_images_limited(&page_payload, None, false, 20);
        assert_eq!(page_items.len(), 15);
        assert_eq!(images_next(&page_payload).0.as_deref(), Some("15"));
        assert!(images_next(&page_payload).1.is_none());
        let page_only = json!({
            "metadata": { "nextPage": "https://civitai.com/api/v1/images?page=3&limit=20" }
        });
        assert_eq!(images_next(&page_only), (None, Some(3)));

        let mut known_base = sfw.clone();
        annotate_community_images(
            &mut known_base,
            &StudioInventory {
                base_known: true,
                ..StudioInventory::default()
            },
        );
        assert_eq!(
            known_base[0].checkpoint.as_deref(),
            Some("example-checkpoint")
        );
        assert!(!known_base[0].checkpoint_installed);
        assert!(known_base[0].checkpoint_different);
        assert!(known_base[0].missing_resources.iter().any(|r| {
            r.name == "example-checkpoint" && r.installable == Some(false) && r.dest_path.is_some()
        }));
        assert!(known_base[0]
            .missing_resources
            .iter()
            .any(|r| r.name == "FrankieXL" && r.installable == Some(false)));
    }

    #[test]
    fn community_checkpoint_compares_hash_name_and_version() {
        let payload = json!({
            "items": [{
                "id": 61422506,
                "postId": 13731164,
                "type": "image",
                "nsfwLevel": "None",
                "url": "https://image.civitai.com/x/abc/original=true/abc.jpeg",
                "meta": {
                    "prompt": "FrankieFosterSC, red hair",
                    "Model": "waiNSFWIllustrious_v100",
                    "Model hash": "7e87327f89",
                    "hashes": { "model": "7e87327f89", "lora:FrankieXL": "3480fafddb0f" },
                    "resources": [
                        {"hash": "3480fafddb0f", "name": "FrankieXL", "type": "lora", "weight": 1},
                        {"hash": "7e87327f89", "name": "waiNSFWIllustrious_v100", "type": "model"},
                        {"name": "unused", "type": "lora", "weight": 0}
                    ]
                }
            }, {
                "id": 119646452,
                "type": "image",
                "nsfwLevel": "None",
                "meta": {
                    "prompt": "on-site generation",
                    "civitaiResources": [
                        {"type": "checkpoint", "modelVersionId": 2514310},
                        {"type": "lora", "weight": 1, "modelVersionId": 1490040}
                    ]
                }
            }]
        });
        let mut items = parse_community_images(&payload, Some(1490040), false);
        assert_eq!(items.len(), 2);
        let ckpt = items[0]
            .resources
            .iter()
            .find(|r| r.name == "waiNSFWIllustrious_v100")
            .unwrap();
        assert_eq!(ckpt.hash.as_deref(), Some("7e87327f89"));
        assert!(is_checkpoint_type(&ckpt.resource_type));
        assert_eq!(
            items[0]
                .resources
                .iter()
                .filter(|r| r.name == "waiNSFWIllustrious_v100")
                .count(),
            1
        );
        let lora = items[0]
            .resources
            .iter()
            .find(|r| r.name == "FrankieXL")
            .unwrap();
        assert_eq!(lora.hash.as_deref(), Some("3480fafddb0f"));
        assert!(items[0].resources.iter().all(|r| r.name != "unused"));
        assert!(items[1].resources.iter().any(|r| {
            r.model_version_id == Some(2514310) && is_checkpoint_type(&r.resource_type)
        }));

        let resolved = parse_resolved_version(&json!({
            "id": 1367422,
            "modelId": 827184,
            "name": "v10.0",
            "model": {"name": "WAI-illustrious-SDXL", "type": "Checkpoint"},
            "files": [{"name": "WAI-Nsfw-Illustrious-10.safetensors", "primary": true}]
        }))
        .unwrap();
        apply_resolved_version(
            items[0]
                .resources
                .iter_mut()
                .find(|r| r.hash.as_deref() == Some("7e87327f89"))
                .unwrap(),
            &resolved,
        );
        assert_eq!(items[0].resources[1].model_version_id, Some(1367422));
        assert_eq!(
            items[0]
                .resources
                .iter()
                .find(|r| r.hash.as_deref() == Some("7e87327f89"))
                .unwrap()
                .name,
            "waiNSFWIllustrious_v100"
        );
        assert_eq!(
            items[0]
                .resources
                .iter()
                .find(|r| r.hash.as_deref() == Some("7e87327f89"))
                .unwrap()
                .model_id,
            Some(827184)
        );

        let current = InstalledAssetMatch {
            name: "WAI-Nsfw-Illustrious-10".into(),
            path: "checkpoints/WAI-Nsfw-Illustrious-10.safetensors".into(),
            kind: "checkpoint".into(),
            version_id: Some(1367422),
            model_id: Some(827184),
            hash: Some("7e87327f89".into()),
        };
        annotate_community_images(
            &mut items,
            &StudioInventory {
                installed: vec![current.clone()],
                current_base: Some(current),
                base_known: true,
            },
        );
        assert_eq!(
            items[0].checkpoint.as_deref(),
            Some("waiNSFWIllustrious_v100")
        );
        assert!(items[0].checkpoint_installed);
        assert!(!items[0].checkpoint_different);
        assert!(items[0]
            .missing_resources
            .iter()
            .all(|r| r.name != "waiNSFWIllustrious_v100"));
        let missing_lora = items[0]
            .missing_resources
            .iter()
            .find(|r| r.name == "FrankieXL")
            .unwrap();
        assert_eq!(missing_lora.installable, Some(false));
        assert_eq!(
            missing_lora.dest_path.as_deref(),
            Some("loras/FrankieXL.safetensors")
        );

        items[0]
            .resources
            .iter_mut()
            .find(|r| r.name == "FrankieXL")
            .unwrap()
            .model_version_id = Some(1490040);
        let other = InstalledAssetMatch {
            name: "other".into(),
            path: "checkpoints/other.safetensors".into(),
            kind: "checkpoint".into(),
            version_id: Some(1),
            model_id: Some(2),
            hash: None,
        };
        annotate_community_images(
            &mut items,
            &StudioInventory {
                installed: vec![other.clone()],
                current_base: Some(other),
                base_known: true,
            },
        );
        assert!(items[0].checkpoint_different);
        assert!(!items[0].checkpoint_installed);
        let missing_ckpt = items[0]
            .missing_resources
            .iter()
            .find(|r| r.name == "waiNSFWIllustrious_v100")
            .unwrap();
        assert_eq!(missing_ckpt.installable, Some(true));
        assert_eq!(missing_ckpt.model_version_id, Some(1367422));
        assert_eq!(missing_ckpt.model_id, Some(827184));
        assert_eq!(
            missing_ckpt.dest_path.as_deref(),
            Some("checkpoints/waiNSFWIllustrious_v100.safetensors")
        );
        assert!(items[0]
            .missing_resources
            .iter()
            .any(|r| r.name == "FrankieXL" && r.installable == Some(true)));

        let mut named = parse_community_images(
            &json!({
                "items": [{
                    "id": 1,
                    "nsfwLevel": "None",
                    "meta": { "Model": "waiNSFWIllustrious_v100", "prompt": "x" }
                }]
            }),
            None,
            false,
        );
        annotate_community_images(
            &mut named,
            &StudioInventory {
                installed: vec![InstalledAssetMatch {
                    name: "waiNSFWIllustrious_v100".into(),
                    path: "checkpoints/waiNSFWIllustrious_v100.safetensors".into(),
                    kind: "checkpoint".into(),
                    version_id: None,
                    model_id: None,
                    hash: None,
                }],
                current_base: Some(InstalledAssetMatch {
                    name: "waiNSFWIllustrious_v100".into(),
                    path: "checkpoints/waiNSFWIllustrious_v100.safetensors".into(),
                    kind: "checkpoint".into(),
                    version_id: None,
                    model_id: None,
                    hash: None,
                }),
                base_known: true,
            },
        );
        assert!(named[0].checkpoint_installed);
        assert!(!named[0].checkpoint_different);
        assert!(named[0].missing_resources.is_empty());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionExample {
    pub version_id: i64,
    pub version_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<crate::genmeta::ExampleMeta>,
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
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub hashes: std::collections::HashMap<String, String>,
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
    /// Sampling fields from the first example image that has steps or a sampler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_meta: Option<crate::genmeta::ExampleMeta>,
    /// Creator-listed resources for this version (checkpoints, LoRAs, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommended_resources: Vec<CivitaiRecommendedResource>,
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
    /// From Civitai `nsfw` / nsfwLevel — used when Freya NSFW filter is off.
    #[serde(default)]
    pub nsfw: bool,
    /// Civitai `nsfwLevel` (1 = SFW). Levels above 1 open on civitai.red.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw_level: Option<i64>,
    /// All versions + file variants (present on search when Civitai returns them; always on detail).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_versions: Vec<CivitaiModelVersion>,
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
    pub file_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_format: Option<String>,
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
    /// From Civitai `nsfw` / nsfwLevel (also a name heuristic). Drives the civitai.red link.
    #[serde(default)]
    pub nsfw: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw_level: Option<i64>,
    /// Human hint when adapters need a compatible checkpoint family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_base: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_versions: Vec<CivitaiModelVersion>,
    /// Recommended resources for the selected version (id, name, type, modelVersionId, baseModel).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommended_resources: Vec<CivitaiRecommendedResource>,
    /// Popular checkpoints for the same baseModel family (best-effort related assets).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<CivitaiModelCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CivitaiRecommendedResource {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_version_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_model: Option<String>,
    /// Recommended model is NSFW (`nsfw` true or nsfwLevel > 1).
    #[serde(default)]
    pub nsfw: bool,
}

/// One checkpoint, LoRA, or embedding named on a community image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommunityImageResource {
    pub name: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_version_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<i64>,
    /// AutoV2 / SHA256 from generation data. Not a download by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// False when the post has a name or hash and no Civitai model or version id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installable: Option<bool>,
    /// Worker path for `install_asset`. Set on missing resources only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest_path: Option<String>,
}

/// Community post made with a model version. Bytes stay on Civitai's image host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommunityImage {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampler: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cfg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_skip: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    /// Civitai item type (`image`, `video`, …) when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsfw_level: Option<String>,
    pub civitai_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<CommunityImageResource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_version_ids: Vec<i64>,
    /// Checkpoint name from this post's generation data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
    /// True when that checkpoint file is already on the worker.
    #[serde(default)]
    pub checkpoint_installed: bool,
    /// True when it is not the studio's current base. False when the base is unknown.
    #[serde(default)]
    pub checkpoint_different: bool,
    /// Checkpoint, LoRAs, and embeddings from this post that are not on the worker.
    #[serde(default)]
    pub missing_resources: Vec<CommunityImageResource>,
}
