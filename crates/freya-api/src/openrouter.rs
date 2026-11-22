use crate::config::Config;
use crate::prefs::{
    chat_model_matches_alignment, inject_provider, AlignmentPrefs, AutoFallbacksPrefs,
    ChatProvidersPrefs, PrivacyPrefs,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone)]
pub struct OpenRouterClient {
    http: reqwest::Client,
    base: String,
    api_key: String,
    chat_model: String,
    image_model: String,
    router_model: String,
    privacy: PrivacyPrefs,
    alignment: AlignmentPrefs,
    auto_fallbacks: AutoFallbacksPrefs,
}

impl OpenRouterClient {
    pub fn new(cfg: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: cfg.chat_url().trim_end_matches('/').to_string(),
            api_key: cfg.openrouter_api_key.clone(),
            chat_model: cfg.chat_model.clone(),
            image_model: cfg.image_model.clone(),
            router_model: cfg.router_model.clone(),
            privacy: cfg.privacy_prefs(),
            alignment: cfg.alignment_prefs(),
            auto_fallbacks: cfg.auto_fallbacks_prefs(),
        }
    }

    pub fn with_key(mut self, key: String) -> Self {
        self.api_key = key;
        self
    }

    #[allow(dead_code)]
    pub fn privacy(&self) -> &PrivacyPrefs {
        &self.privacy
    }

    #[allow(dead_code)]
    pub fn alignment(&self) -> &AlignmentPrefs {
        &self.alignment
    }

    pub fn default_image_model(&self) -> String {
        if crate::router::is_auto_image(&self.image_model) {
            crate::router::resolve_auto_fallback(
                crate::router::AutoFallbackKind::Image,
                &self.privacy,
                &self.alignment,
                &self.auto_fallbacks,
            )
        } else {
            self.image_model.clone()
        }
    }

    /// ZDR Auto still-image default (Google image endpoints usually pass ZDR/safety retention).
    pub fn zdr_fallback_image_model(&self) -> String {
        let mut privacy = self.privacy.clone();
        privacy.zero_data_retention = true;
        crate::router::resolve_auto_fallback(
            crate::router::AutoFallbackKind::Image,
            &privacy,
            &self.alignment,
            &self.auto_fallbacks,
        )
    }

    pub fn auto_fallbacks(&self) -> &AutoFallbacksPrefs {
        &self.auto_fallbacks
    }

    pub fn configured_image_model(&self) -> &str {
        &self.image_model
    }

    pub fn configured_router_model(&self) -> &str {
        &self.router_model
    }

    #[allow(dead_code)]
    pub fn configured_chat_model(&self) -> &str {
        &self.chat_model
    }

    async fn auth_post(&self, path: &str, body: Value) -> Result<Value> {
        if self.api_key.is_empty() {
            return Err(anyhow!("OPENROUTER_API_KEY not configured"));
        }
        let url = format!("{}{path}", self.base);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://dataknife.ai/freya")
            .header("X-Title", "Freya")
            .json(&body)
            .send()
            .await
            .context("openrouter request")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("{}", humanize_openrouter_error(status.as_u16(), &text)));
        }
        serde_json::from_str(&text).context("parse openrouter json")
    }

    async fn auth_get(&self, path: &str) -> Result<Value> {
        if self.api_key.is_empty() {
            return Err(anyhow!("OPENROUTER_API_KEY not configured"));
        }
        let url = format!("{}{path}", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://dataknife.ai/freya")
            .header("X-Title", "Freya")
            .send()
            .await
            .context("openrouter request")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("{}", humanize_openrouter_error(status.as_u16(), &text)));
        }
        serde_json::from_str(&text).context("parse openrouter json")
    }

    /// Per-key usage / spend caps via `GET /api/v1/key` (works with a normal completion key).
    pub async fn key_usage(&self) -> Result<OpenRouterKeyUsage> {
        let v = self.auth_get("/key").await?;
        let data = v.get("data").cloned().unwrap_or(v);
        Ok(OpenRouterKeyUsage {
            label: data
                .get("label")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            usage: data.get("usage").and_then(|x| x.as_f64()),
            usage_daily: data.get("usage_daily").and_then(|x| x.as_f64()),
            usage_weekly: data.get("usage_weekly").and_then(|x| x.as_f64()),
            usage_monthly: data.get("usage_monthly").and_then(|x| x.as_f64()),
            limit: data.get("limit").and_then(|x| {
                if x.is_null() {
                    None
                } else {
                    x.as_f64()
                }
            }),
            limit_remaining: data.get("limit_remaining").and_then(|x| {
                if x.is_null() {
                    None
                } else {
                    x.as_f64()
                }
            }),
            limit_reset: data
                .get("limit_reset")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            is_free_tier: data.get("is_free_tier").and_then(|x| x.as_bool()),
            is_management_key: data
                .get("is_management_key")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
        })
    }

    /// Account credits via `GET /api/v1/credits` (requires a Management API key).
    pub async fn account_credits(&self) -> Result<OpenRouterCredits> {
        let v = self.auth_get("/credits").await?;
        let data = v.get("data").cloned().unwrap_or(v);
        let total_credits = data
            .get("total_credits")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        let total_usage = data
            .get("total_usage")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        Ok(OpenRouterCredits {
            total_credits,
            total_usage,
            remaining: total_credits - total_usage,
        })
    }

    pub async fn chat(&self, system: &str, user: &str, model: Option<&str>) -> Result<String> {
        let model = model.unwrap_or(&self.chat_model);
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]
        });
        inject_provider(&mut body, &self.privacy);
        let v = self.auth_post("/chat/completions", body).await?;
        v.pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("missing chat content"))
    }

    pub async fn list_image_models(&self) -> Result<Vec<ImageModel>> {
        if self.api_key.is_empty() {
            return Ok(filter_fallback_image(
                &fallback_models(),
                self.privacy.zero_data_retention,
            ));
        }
        // When Freya ZDR is on, list only models with ZDR-capable endpoints
        // (`GET /models?output_modalities=image&zdr=true`). Dedicated /images/models
        // does not expose the zdr query filter.
        if self.privacy.zero_data_retention {
            let url = format!("{}/models?output_modalities=image&zdr=true", self.base);
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.api_key)
                .send()
                .await
                .context("list image models (zdr)")?;
            if resp.status().is_success() {
                let v: Value = resp.json().await.unwrap_or(Value::Null);
                let out = parse_image_models_from_catalog(&v);
                if !out.is_empty() {
                    return Ok(out);
                }
            }
            return Ok(filter_fallback_image(&fallback_models(), true));
        }

        let url = format!("{}/images/models", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("list image models")?;
        if !resp.status().is_success() {
            return Ok(fallback_models());
        }
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        let out = parse_image_models_from_catalog(&v);
        if out.is_empty() {
            return Ok(fallback_models());
        }
        Ok(out)
    }

    /// Discover chat/agent models via `GET /models`, filtered to text/chat
    /// candidates (denylist: routing suffixes, non-text modalities, image/embed/…).
    /// When ZDR is on, requests `?zdr=true` (OpenRouter catalog filter).
    /// Alignment prefs further filter by `top_provider.is_moderated` / name heuristics.
    /// User chat-provider checkboxes (Settings) filter vendor prefixes last.
    #[allow(dead_code)]
    pub async fn list_chat_models(&self) -> Result<Vec<ImageModel>> {
        Ok(self.list_chat_catalog().await?.models)
    }

    /// Chat catalog plus the vendor prefixes present before checkbox filtering.
    pub async fn list_chat_catalog(&self) -> Result<ChatCatalog> {
        let chat_providers = self.privacy.chat_providers.clone();
        if self.api_key.is_empty() {
            return Ok(self.fallback_chat_catalog());
        }
        let url = if self.privacy.zero_data_retention {
            format!("{}/models?zdr=true", self.base)
        } else {
            format!("{}/models", self.base)
        };
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("list chat models")?;
        if !resp.status().is_success() {
            return Ok(self.fallback_chat_catalog());
        }
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        let mut agent_scored: Vec<(i64, ImageModel)> = Vec::new();
        let mut router_scored: Vec<(i64, ImageModel)> = Vec::new();
        let mut available_set = std::collections::BTreeSet::new();
        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            for m in arr {
                let agent_ok = is_agent_capable(m);
                let router_ok = is_router_capable(m);
                if !agent_ok && !router_ok {
                    continue;
                }
                let id = m
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() {
                    continue;
                }
                available_set.insert(openrouter_vendor(&id).to_ascii_lowercase());
                if !chat_model_matches_alignment(m, &self.alignment) {
                    continue;
                }
                if !chat_providers.includes_model_id(&id) {
                    continue;
                }
                let name = display_name_for_catalog(m, &id);
                let created = m.get("created").and_then(|c| c.as_i64()).unwrap_or(0);
                let model = ImageModel {
                    id,
                    name,
                    backend: "openrouter".into(),
                };
                if agent_ok {
                    agent_scored.push((created, model.clone()));
                }
                if router_ok {
                    router_scored.push((created, model));
                }
            }
        }
        if agent_scored.is_empty() && router_scored.is_empty() && available_set.is_empty() {
            return Ok(self.fallback_chat_catalog());
        }
        let mut models = order_pinned(agent_scored);
        let mut router = order_pinned(router_scored);
        if crate::router::privacy_requires_strict(&self.privacy) {
            // Upstream router is not on the ZDR catalog.
            models.retain(|m| !is_upstream_router_id(&m.id));
            router.retain(|m| !is_upstream_router_id(&m.id));
        } else if let Some(existing) = models.iter_mut().find(|m| is_upstream_router_id(&m.id)) {
            existing.name = "OpenRouter router (upstream)".into();
        } else {
            // Catalog omitted it (or alignment hid it). Offer it only when privacy is not strict
            // and alignment did not reject a row we already saw. Balanced always keeps it.
            if self.alignment.model_alignment != "safety" {
                ensure_upstream_agent(&mut models);
            }
        }
        Ok(ChatCatalog {
            models,
            router,
            available: available_set.into_iter().collect(),
        })
    }

    /// Discover OpenRouter video models via `GET /videos/models`.
    pub async fn list_video_models(&self) -> Result<Vec<VideoModel>> {
        if self.api_key.is_empty() {
            return Ok(Vec::new());
        }
        // When Freya ZDR is on, prefer the ZDR-filtered modality catalog.
        if self.privacy.zero_data_retention {
            let url = format!("{}/models?output_modalities=video&zdr=true", self.base);
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.api_key)
                .send()
                .await
                .context("list video models (zdr)")?;
            if resp.status().is_success() {
                let v: Value = resp.json().await.unwrap_or(Value::Null);
                let out = parse_video_models(&v);
                if !out.is_empty() {
                    return Ok(out);
                }
            }
        }
        // Prefer dedicated video catalog; fall back to modality filter on /models.
        let url = format!("{}/videos/models", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("list video models")?;
        if resp.status().is_success() {
            let v: Value = resp.json().await.unwrap_or(Value::Null);
            let out = parse_video_models(&v);
            if !out.is_empty() {
                return Ok(out);
            }
        }
        let url = format!("{}/models?output_modalities=video", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("list video models (modality)")?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        Ok(parse_video_models(&v))
    }

    pub async fn generate_image(
        &self,
        prompt: &str,
        model: Option<&str>,
        size: Option<&str>,
    ) -> Result<GeneratedImage> {
        let default_image = self.default_image_model();
        let model = resolve_image_model(model.unwrap_or(&default_image), &default_image);
        match self.generate_image_once(prompt, &model, size).await {
            Ok(img) => Ok(img),
            Err(e) if self.privacy.zero_data_retention && is_zdr_routing_error(&e.to_string()) => {
                let fallback = self.zdr_fallback_image_model();
                if model == fallback {
                    return Err(e);
                }
                tracing::warn!(
                    model = %model,
                    fallback = %fallback,
                    "image ZDR routing failed; retrying with ZDR-capable model"
                );
                match self.generate_image_once(prompt, &fallback, size).await {
                    Ok(img) => Ok(img),
                    Err(e2) => {
                        if is_zdr_routing_error(&e2.to_string()) {
                            Err(anyhow!("{}", humanize_openrouter_error(404, &e2.to_string())))
                        } else {
                            Err(e2)
                        }
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn generate_image_once(
        &self,
        prompt: &str,
        model: &str,
        size: Option<&str>,
    ) -> Result<GeneratedImage> {
        // Gemini / Nano Banana family: chat completions + modalities.
        // Diffusion models (FLUX, GPT Image, …): dedicated /images API only —
        // do not fall back to chat (wrong modalities error masks ZDR/credits).
        if is_chat_image_model(model) {
            return self
                .generate_image_via_chat(prompt, model)
                .await
                .map_err(|e| anyhow!("{}", e));
        }

        let mut body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "n": 1,
        });
        if let Some(size) = size {
            if let Some(ratio) = size_to_aspect(size) {
                body["aspect_ratio"] = Value::String(ratio.into());
            } else {
                body["size"] = Value::String(size.into());
            }
        }
        inject_provider(&mut body, &self.privacy);

        let v = self.auth_post("/images", body).await?;
        extract_image(&self.http, &v).await
    }

    async fn generate_image_via_chat(&self, prompt: &str, model: &str) -> Result<GeneratedImage> {
        let mut chat_body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "modalities": ["image", "text"]
        });
        inject_provider(&mut chat_body, &self.privacy);
        let v = self.auth_post("/chat/completions", chat_body).await?;
        extract_image_from_chat(&v)
    }

    /// Async text-to-video: submit → poll → download bytes (mp4).
    pub async fn generate_video(
        &self,
        prompt: &str,
        model: Option<&str>,
        duration_secs: Option<i64>,
        aspect_ratio: Option<&str>,
    ) -> Result<GeneratedVideo> {
        if self.api_key.is_empty() {
            return Err(anyhow!("OPENROUTER_API_KEY not configured"));
        }
        let default_video = crate::router::resolve_auto_fallback(
            crate::router::AutoFallbackKind::Video,
            &self.privacy,
            &self.alignment,
            &self.auto_fallbacks,
        );
        let model = resolve_video_model(model.unwrap_or(""), &default_video);
        let zdr_video_fallback = {
            let mut privacy = self.privacy.clone();
            privacy.zero_data_retention = true;
            crate::router::resolve_auto_fallback(
                crate::router::AutoFallbackKind::Video,
                &privacy,
                &self.alignment,
                &self.auto_fallbacks,
            )
        };
        match self
            .generate_video_once(prompt, &model, duration_secs, aspect_ratio)
            .await
        {
            Ok(v) => Ok(v),
            Err(e)
                if self.privacy.zero_data_retention
                    && is_zdr_routing_error(&e.to_string())
                    && model != zdr_video_fallback =>
            {
                tracing::warn!(
                    model = %model,
                    fallback = %zdr_video_fallback,
                    "video ZDR routing failed; retrying with ZDR-capable model"
                );
                self.generate_video_once(
                    prompt,
                    &zdr_video_fallback,
                    duration_secs,
                    aspect_ratio,
                )
                .await
                .map_err(|e2| {
                    if is_zdr_routing_error(&e2.to_string()) {
                        anyhow!("{}", humanize_openrouter_error(404, &e2.to_string()))
                    } else {
                        e2
                    }
                })
            }
            Err(e) => Err(e),
        }
    }

    async fn generate_video_once(
        &self,
        prompt: &str,
        model: &str,
        duration_secs: Option<i64>,
        aspect_ratio: Option<&str>,
    ) -> Result<GeneratedVideo> {
        let mut body = serde_json::json!({
            "model": model,
            "prompt": prompt,
        });
        if let Some(d) = duration_secs.filter(|d| *d > 0) {
            body["duration"] = Value::from(d);
        }
        if let Some(ar) = aspect_ratio.map(str::trim).filter(|s| !s.is_empty()) {
            body["aspect_ratio"] = Value::String(ar.into());
        }
        inject_provider(&mut body, &self.privacy);

        let submit = self.auth_post_status("/videos", body, &[202, 200]).await?;
        let job_id = submit
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("video job missing id"))?
            .to_string();
        let polling_url = submit
            .get("polling_url")
            .and_then(|x| x.as_str())
            .map(|u| {
                if u.starts_with("http") {
                    u.to_string()
                } else {
                    format!("{}{}", self.base.trim_end_matches("/api/v1"), u)
                }
            })
            .unwrap_or_else(|| format!("{}/videos/{job_id}", self.base));

        let completed = self.poll_video_job(&polling_url).await?;
        let bytes = self.download_video_bytes(&job_id, &completed).await?;
        let mime = sniff_video_mime(&bytes).unwrap_or("video/mp4").to_string();
        let _ = &job_id; // retained for logs / future job status API
        tracing::info!(%job_id, %model, bytes = bytes.len(), "openrouter video ready");
        Ok(GeneratedVideo {
            bytes,
            mime,
            model: model.to_string(),
            job_id,
        })
    }

    async fn auth_post_status(&self, path: &str, body: Value, ok: &[u16]) -> Result<Value> {
        if self.api_key.is_empty() {
            return Err(anyhow!("OPENROUTER_API_KEY not configured"));
        }
        let url = format!("{}{path}", self.base);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://dataknife.ai/freya")
            .header("X-Title", "Freya")
            .json(&body)
            .send()
            .await
            .context("openrouter request")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !ok.contains(&status.as_u16()) {
            return Err(anyhow!("{}", humanize_openrouter_error(status.as_u16(), &text)));
        }
        serde_json::from_str(&text).context("parse openrouter json")
    }

    async fn poll_video_job(&self, polling_url: &str) -> Result<Value> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        loop {
            if std::time::Instant::now() > deadline {
                return Err(anyhow!("video generation timed out"));
            }
            let resp = self
                .http
                .get(polling_url)
                .bearer_auth(&self.api_key)
                .send()
                .await
                .context("poll video job")?;
            let status_code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !status_code.is_success() {
                return Err(anyhow!("{}", humanize_openrouter_error(status_code.as_u16(), &text)));
            }
            let v: Value = serde_json::from_str(&text).context("parse video poll json")?;
            let st = v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match st.as_str() {
                "completed" => return Ok(v),
                "failed" | "cancelled" | "expired" => {
                    let err = v
                        .get("error")
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| st.clone());
                    return Err(anyhow!("video job {st}: {err}"));
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                }
            }
        }
    }

    async fn download_video_bytes(&self, job_id: &str, completed: &Value) -> Result<Vec<u8>> {
        // Prefer authenticated content proxy, then signed URLs.
        let content_url = format!("{}/videos/{job_id}/content", self.base);
        let resp = self
            .http
            .get(&content_url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("download video content")?;
        if resp.status().is_success() {
            return Ok(resp.bytes().await.context("video content body")?.to_vec());
        }
        let urls = completed
            .get("urls")
            .and_then(|u| u.as_array())
            .into_iter()
            .flatten()
            .chain(
                completed
                    .get("unsigned_urls")
                    .and_then(|u| u.as_array())
                    .into_iter()
                    .flatten(),
            );
        for u in urls {
            let Some(url) = u.as_str() else { continue };
            let resp = self
                .http
                .get(url)
                .bearer_auth(&self.api_key)
                .send()
                .await
                .context("download video url")?;
            if resp.status().is_success() {
                return Ok(resp.bytes().await.context("video url body")?.to_vec());
            }
        }
        Err(anyhow!("video completed but no downloadable URL"))
    }
}

/// Default OpenRouter T2V model (text-to-video; not avatar/lip-sync).
pub const DEFAULT_AUTO_VIDEO_MODEL: &str = "bytedance/seedance-2.5-flash";

pub fn resolve_video_model(requested: &str, default_video: &str) -> String {
    let m = requested.trim();
    if m.is_empty() || m.eq_ignore_ascii_case("auto") {
        return default_video.to_string();
    }
    // Avatar models need photo/audio — not a drop-in for text-to-video Auto.
    if m.contains("heygen") || m.contains("avatar") {
        return default_video.to_string();
    }
    m.to_string()
}

fn sniff_video_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("video/mp4");
    }
    if bytes.len() >= 4 && &bytes[0..4] == b"\x1aE\xdf\xa3" {
        return Some("video/webm");
    }
    if bytes.len() >= 6 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
        return Some("image/gif");
    }
    None
}

/// Reject chat-router ids that planners sometimes emit as "image" models.
pub fn resolve_image_model(requested: &str, default_image: &str) -> String {
    let m = requested.trim();
    if m.is_empty() || !is_plausible_image_model(m) {
        return default_image.to_string();
    }
    m.to_string()
}

fn is_plausible_image_model(model: &str) -> bool {
    let raw = model.trim();
    let m = raw.to_ascii_lowercase();
    if m.is_empty() || m.starts_with("openrouter/auto") || crate::router::is_auto_image(raw) {
        return false;
    }
    // Router / planner prose echoed as a model id (not a real HF/OpenRouter slug).
    if raw.contains(char::is_whitespace)
        || raw.contains(',')
        || m == "none"
        || m.starts_with("none,")
        || m.contains("zdr-aware")
        || m.contains("letting freya")
        || m.contains("auto default")
    {
        return false;
    }
    // Positive signals for OpenRouter image endpoints.
    if m.contains("image")
        || m.contains("flux")
        || m.contains("seedream")
        || m.contains("dall-e")
        || m.contains("stable-diffusion")
        || m.contains("sdxl")
        || m.contains("sd3")
        || m.contains("recraft")
        || m.contains("riverflow")
        || m.contains("muse-image")
        || m.contains("grok-imagine")
        || m.contains("nano-banana")
        || m.contains("mai-image")
        || m.contains("hy-image")
        || m.contains("hy-world")
    {
        return true;
    }
    // Chat / LLM ids that planners often emit by mistake.
    if m.starts_with("anthropic/")
        || m.starts_with("openai/gpt-4")
        || m.starts_with("openai/o1")
        || m.starts_with("openai/o3")
        || m.starts_with("meta-llama/")
        || m.starts_with("qwen/qwen3")
        || m.starts_with("qwen/qwen2")
        || m.starts_with("deepseek/")
        || m.starts_with("mistralai/")
        || (m.starts_with("google/gemini") && !m.contains("image") && !m.contains("banana"))
    {
        return false;
    }
    // Video-only ids are not still-image models.
    if m.contains("seedance")
        || m.contains("veo")
        || m.contains("kling")
        || m.contains("hailuo")
        || m.contains("t2v")
    {
        return false;
    }
    // Bare tokens (`ldm-xxl-256`) are not OpenRouter ids. Catalog ids are org/name.
    raw.contains('/')
}

fn is_chat_image_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("gemini") && m.contains("image")
        || m.contains("nano-banana")
        || m.contains("gemini-nano-banana")
}

fn size_to_aspect(size: &str) -> Option<&'static str> {
    match size {
        "1024x1024" | "1:1" => Some("1:1"),
        "1024x576" | "16:9" => Some("16:9"),
        "576x1024" | "9:16" => Some("9:16"),
        _ => None,
    }
}

/// Per-key usage from OpenRouter `GET /api/v1/key` (USD credits).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterKeyUsage {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_daily: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_weekly: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_monthly: Option<f64>,
    /// Key spend cap (USD), or null when unlimited.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    /// Remaining under the key cap (null when unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_reset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_free_tier: Option<bool>,
    pub is_management_key: bool,
}

/// Account-level purchased vs spent credits (`GET /api/v1/credits`, management key).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterCredits {
    pub total_credits: f64,
    pub total_usage: f64,
    pub remaining: f64,
}

/// True when OpenRouter refused routing because no ZDR/data-policy endpoint matched.
pub fn is_zdr_routing_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("zdr")
        || lower.contains("zero data retention")
        || lower.contains("data-policy")
        || lower.contains("data policy")
        || lower.contains("no endpoints matching")
        || (lower.contains("guardrail")
            && (lower.contains("endpoint") || lower.contains("excluded")))
}

fn humanize_openrouter_error(status: u16, text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("zdr")
        || lower.contains("zero data retention")
        || lower.contains("no endpoints matching")
        || (lower.contains("guardrail")
            && (lower.contains("endpoint") || lower.contains("zdr") || lower.contains("data")))
    {
        return format!(
            "OpenRouter blocked media routing (ZDR/data-policy, HTTP {status}). \
             No Zero Data Retention endpoints matched this image/video model. \
             Fix options: (1) Freya Settings → Privacy → turn off “Zero Data Retention on Freya requests”, \
             or pick a ZDR-capable model (often Google image/video) from the Image picker; \
             (2) OpenRouter account privacy / model-group ZDR at https://openrouter.ai/settings/privacy \
             (Freya can only tighten ZDR, not loosen account guardrails). Details: {text}"
        );
    }
    if status == 402
        || lower.contains("insufficient credits")
        || lower.contains("at least $1.00 in balance")
    {
        return format!(
            "OpenRouter needs more credits for image output (HTTP {status}). \
             Add credits at https://openrouter.ai/settings/credits. Details: {text}"
        );
    }
    format!("openrouter {status}: {text}")
}

async fn extract_image(http: &reqwest::Client, v: &Value) -> Result<GeneratedImage> {
    if let Some(b64) = v.pointer("/data/0/b64_json").and_then(|x| x.as_str()) {
        return Ok(GeneratedImage {
            b64: b64.to_string(),
            mime: "image/png".into(),
        });
    }
    if let Some(url) = v.pointer("/data/0/url").and_then(|x| x.as_str()) {
        if let Some(rest) = url
            .strip_prefix("data:image/png;base64,")
            .or_else(|| url.strip_prefix("data:image/jpeg;base64,"))
        {
            let mime = if url.contains("jpeg") {
                "image/jpeg"
            } else {
                "image/png"
            };
            return Ok(GeneratedImage {
                b64: rest.to_string(),
                mime: mime.into(),
            });
        }
        // Remote URL — download bytes.
        let resp = http
            .get(url)
            .send()
            .await
            .context("download openrouter image url")?;
        if !resp.status().is_success() {
            return Err(anyhow!("image url download failed: {}", resp.status()));
        }
        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/png")
            .to_string();
        let bytes = resp.bytes().await.context("read image bytes")?;
        use base64::Engine;
        return Ok(GeneratedImage {
            b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            mime,
        });
    }
    Err(anyhow!("no image in openrouter response"))
}

fn extract_image_from_chat(v: &Value) -> Result<GeneratedImage> {
    if let Some(images) = v
        .pointer("/choices/0/message/images")
        .and_then(|x| x.as_array())
    {
        if let Some(url) = images
            .first()
            .and_then(|i| i.pointer("/image_url/url"))
            .and_then(|u| u.as_str())
        {
            if let Some(rest) = url
                .strip_prefix("data:image/png;base64,")
                .or_else(|| url.strip_prefix("data:image/jpeg;base64,"))
            {
                let mime = if url.contains("jpeg") {
                    "image/jpeg"
                } else {
                    "image/png"
                };
                return Ok(GeneratedImage {
                    b64: rest.to_string(),
                    mime: mime.into(),
                });
            }
        }
    }
    Err(anyhow!("no image in chat completion"))
}

fn fallback_models() -> Vec<ImageModel> {
    vec![
        ImageModel {
            id: "black-forest-labs/flux.2-klein-4b".into(),
            name: "FLUX.2 Klein 4B".into(),
            backend: "openrouter".into(),
        },
        ImageModel {
            id: "google/gemini-2.5-flash-image".into(),
            name: "Gemini 2.5 Flash Image".into(),
            backend: "openrouter".into(),
        },
        ImageModel {
            id: "openai/gpt-image-1-mini".into(),
            name: "GPT Image 1 Mini".into(),
            backend: "openrouter".into(),
        },
    ]
}

fn filter_fallback_image(models: &[ImageModel], zdr: bool) -> Vec<ImageModel> {
    if !zdr {
        return models.to_vec();
    }
    // Heuristic when catalog fetch fails: prefer Google image models (Safety Retention / ZDR).
    let google: Vec<_> = models
        .iter()
        .filter(|m| m.id.starts_with("google/"))
        .cloned()
        .collect();
    if !google.is_empty() {
        return google;
    }
    vec![ImageModel {
        id: crate::router::DEFAULT_AUTO_IMAGE_MODEL_ZDR.into(),
        name: "Gemini 2.5 Flash Image (ZDR fallback)".into(),
        backend: "openrouter".into(),
    }]
}

fn parse_image_models_from_catalog(v: &Value) -> Vec<ImageModel> {
    let mut out = Vec::new();
    let Some(arr) = v.get("data").and_then(|d| d.as_array()) else {
        return out;
    };
    for m in arr {
        let id = m
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() || is_upstream_router_id(&id) || !keeps_declared_output(m, "image") {
            continue;
        }
        let name = m
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(&id)
            .to_string();
        out.push(ImageModel {
            id,
            name,
            backend: "openrouter".into(),
        });
    }
    out
}

fn fallback_chat_models() -> Vec<ImageModel> {
    vec![
        ImageModel {
            id: "anthropic/claude-sonnet-5.5".into(),
            name: "Claude Sonnet 5.5".into(),
            backend: "openrouter".into(),
        },
        ImageModel {
            id: "google/gemini-4-neon".into(),
            name: "Gemini 4 Neon".into(),
            backend: "openrouter".into(),
        },
        ImageModel {
            id: "openai/gpt-5.2".into(),
            name: "GPT-5.2".into(),
            backend: "openrouter".into(),
        },
    ]
}

fn filter_fallback_chat(
    models: &[ImageModel],
    alignment: &AlignmentPrefs,
    chat_providers: &ChatProvidersPrefs,
) -> Vec<ImageModel> {
    models
        .iter()
        .filter(|m| {
            if !chat_providers.includes_model_id(&m.id) {
                return false;
            }
            if alignment.model_alignment == "balanced" {
                return true;
            }
            let fake = serde_json::json!({
                "id": m.id,
                "name": m.name,
            });
            chat_model_matches_alignment(&fake, alignment)
        })
        .cloned()
        .collect()
}

fn vendors_from_models(models: &[ImageModel]) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for m in models {
        let v = openrouter_vendor(&m.id).to_ascii_lowercase();
        if !v.is_empty() {
            set.insert(v);
        }
    }
    set.into_iter().collect()
}

/// Agent (`models`) and router catalogs after capability / ZDR / alignment / vendor filters.
/// `available` is vendor prefixes seen before alignment and checkbox filters.
#[derive(Debug, Clone, Default)]
pub struct ChatCatalog {
    /// Tool-calling text models for the Agent picker.
    pub models: Vec<ImageModel>,
    /// Structured-JSON / tool-calling text models for the Router picker.
    pub router: Vec<ImageModel>,
    pub available: Vec<String>,
}

fn display_name_for_catalog(m: &Value, id: &str) -> String {
    if is_upstream_router_id(id) {
        return "OpenRouter router (upstream)".into();
    }
    m.get("name")
        .and_then(|x| x.as_str())
        .unwrap_or(id)
        .to_string()
}

fn is_upstream_router_id(id: &str) -> bool {
    let id = id.trim().to_ascii_lowercase();
    id == "openrouter/auto" || id.starts_with("openrouter/auto")
}

fn order_pinned(mut scored: Vec<(i64, ImageModel)>) -> Vec<ImageModel> {
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    let mut out: Vec<ImageModel> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pin in CHAT_PINNED_IDS {
        if let Some((_, model)) = scored.iter().find(|(_, m)| m.id == *pin) {
            if seen.insert(model.id.clone()) {
                out.push(model.clone());
            }
        }
    }
    for (_, model) in scored {
        if seen.insert(model.id.clone()) {
            out.push(model);
        }
    }
    out
}

fn ensure_upstream_agent(models: &mut Vec<ImageModel>) {
    if let Some(existing) = models.iter_mut().find(|m| is_upstream_router_id(&m.id)) {
        existing.name = "OpenRouter router (upstream)".into();
        return;
    }
    models.push(ImageModel {
        id: "openrouter/auto".into(),
        name: "OpenRouter router (upstream)".into(),
        backend: "openrouter".into(),
    });
}

impl OpenRouterClient {
    fn fallback_chat_catalog(&self) -> ChatCatalog {
        let chat_providers = self.privacy.chat_providers.clone();
        let mut models = filter_fallback_chat(
            &fallback_chat_models(),
            &self.alignment,
            &chat_providers,
        );
        let router = models.clone();
        if !crate::router::privacy_requires_strict(&self.privacy) {
            ensure_upstream_agent(&mut models);
        }
        let available = vendors_from_models(&fallback_chat_models());
        ChatCatalog {
            models,
            router,
            available,
        }
    }
}

fn supported_parameters(m: &Value) -> Option<Vec<String>> {
    let arr = m.get("supported_parameters")?.as_array()?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|x| x.as_str().map(|s| s.to_ascii_lowercase()))
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn params_include(params: &[String], names: &[&str]) -> bool {
    names.iter().any(|n| params.iter().any(|p| p == n))
}

/// Text-chat hygiene: drop routing suffixes, embeddings, image-gen ids.
/// Requires text output and a chat/completions endpoint when endpoints are listed.
/// Missing `supported_parameters` does not make a model agentic.
fn is_text_chat_candidate(m: &Value) -> bool {
    let id = m.get("id").and_then(|x| x.as_str()).unwrap_or("");
    if id.is_empty() {
        return false;
    }
    // Skip provider routing suffixes (:batch, :nitro, :free, …).
    if id.contains(':') {
        return false;
    }
    let id_l = id.to_ascii_lowercase();
    if id_l.contains("embed")
        || id_l.contains("whisper")
        || id_l.contains("tts")
        || id_l.contains("moderation")
        || id_l.contains("upscale")
    {
        return false;
    }
    // Image-generation oriented ids (even when they expose chat completions).
    if id_l.contains("nano-banana")
        || id_l.contains("gpt-image")
        || id_l.contains("dall-e")
        || id_l.contains("flux")
        || id_l.contains("seedream")
        || id_l.contains("imagen")
        || id_l.contains("recraft")
        || (id_l.contains("image") && !id_l.contains("chatgpt"))
    {
        return false;
    }

    let arch = m.get("architecture").unwrap_or(&Value::Null);
    let outs = arch
        .get("output_modalities")
        .and_then(|o| o.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !outs.iter().any(|o| *o == "text") {
        return false;
    }
    // Pure image (or image+audio) generators — not agent chat.
    if outs.iter().all(|o| *o != "text") {
        return false;
    }

    if let Some(ends) = m.get("endpoints").and_then(|e| e.as_array()) {
        if !ends.is_empty() {
            let chat_ok = ends.iter().any(|e| {
                e.as_str()
                    .map(|s| s.contains("chat/completions"))
                    .unwrap_or(false)
            });
            if !chat_ok {
                return false;
            }
        }
    }
    true
}

/// Agent picker: text chat with an explicit tools signal.
/// `openrouter/auto` is included even without the flag (explicit upstream option, not Auto).
fn is_agent_capable(m: &Value) -> bool {
    let id = m.get("id").and_then(|x| x.as_str()).unwrap_or("");
    if is_upstream_router_id(id) {
        return true;
    }
    if !is_text_chat_candidate(m) {
        return false;
    }
    match supported_parameters(m) {
        Some(params) => params_include(&params, &["tools", "tool_choice"]),
        None => false,
    }
}

/// Router picker: text chat that can return structured JSON or call tools.
/// Not the unfiltered chat dump. `openrouter/auto` is included only when it passes
/// (it is not the Auto sentinel).
fn is_router_capable(m: &Value) -> bool {
    if is_upstream_router_id(m.get("id").and_then(|x| x.as_str()).unwrap_or("")) {
        // Upstream router is a concrete option when the catalog lists it and it can do JSON/tools.
    }
    if !is_text_chat_candidate(m) {
        return false;
    }
    match supported_parameters(m) {
        Some(params) => params_include(
            &params,
            &[
                "response_format",
                "structured_outputs",
                "json_schema",
                "tools",
            ],
        ),
        None => false,
    }
}

fn keeps_declared_output(m: &Value, modality: &str) -> bool {
    let Some(arr) = m
        .get("architecture")
        .and_then(|a| a.get("output_modalities"))
        .and_then(|o| o.as_array())
    else {
        return true;
    };
    if arr.is_empty() {
        return true;
    }
    arr.iter()
        .any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case(modality)))
}

/// Always surface these (when OpenRouter still lists them) ahead of the newest-N window.
const CHAT_PINNED_IDS: &[&str] = &[
    "anthropic/claude-sonnet-5.5",
    "anthropic/claude-opus-5.5",
    "google/gemini-4-neon",
    "openai/gpt-5.2",
    "openai/gpt-4.1-mini",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageModel {
    pub id: String,
    pub name: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoModel {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_durations: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_resolutions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_aspect_ratios: Vec<String>,
}

/// OpenRouter vendor slug: id prefix before `/` (`openai/gpt-5.2` → `openai`).
pub fn openrouter_vendor(id: &str) -> &str {
    id.split_once('/').map(|(v, _)| v).unwrap_or(id)
}

/// Sort catalogs by vendor, then display name, then id (case-insensitive).
pub fn sort_models_by_vendor_name(models: &mut [ImageModel]) {
    models.sort_by(|a, b| {
        openrouter_vendor(&a.id)
            .to_ascii_lowercase()
            .cmp(&openrouter_vendor(&b.id).to_ascii_lowercase())
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.id.to_ascii_lowercase().cmp(&b.id.to_ascii_lowercase()))
    });
}

pub fn sort_video_models_by_vendor_name(models: &mut [VideoModel]) {
    models.sort_by(|a, b| {
        openrouter_vendor(&a.id)
            .to_ascii_lowercase()
            .cmp(&openrouter_vendor(&b.id).to_ascii_lowercase())
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.id.to_ascii_lowercase().cmp(&b.id.to_ascii_lowercase()))
    });
}

fn parse_video_models(v: &Value) -> Vec<VideoModel> {
    let mut out = Vec::new();
    let Some(arr) = v.get("data").and_then(|d| d.as_array()) else {
        return out;
    };
    for m in arr {
        let id = m
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() || is_upstream_router_id(&id) || !keeps_declared_output(m, "video") {
            continue;
        }
        let name = m
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(&id)
            .to_string();
        let supported_durations = m
            .get("supported_durations")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
            .unwrap_or_default();
        let supported_resolutions = m
            .get("supported_resolutions")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let supported_aspect_ratios = m
            .get("supported_aspect_ratios")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(VideoModel {
            id,
            name,
            supported_durations,
            supported_resolutions,
            supported_aspect_ratios,
        });
    }
    out
}

#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub b64: String,
    pub mime: String,
}

#[derive(Debug, Clone)]
pub struct GeneratedVideo {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub model: String,
    pub job_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_chat_router_as_image_model() {
        assert_eq!(
            resolve_image_model("openrouter/auto", "black-forest-labs/flux.2-klein-4b"),
            "black-forest-labs/flux.2-klein-4b"
        );
        assert_eq!(
            resolve_image_model(
                "None, letting Freya use Auto default (ZDR-aware).",
                "black-forest-labs/flux.2-klein-4b"
            ),
            "black-forest-labs/flux.2-klein-4b"
        );
        assert_eq!(
            resolve_image_model("google/gemini-2.5-pro", "black-forest-labs/flux.2-klein-4b"),
            "black-forest-labs/flux.2-klein-4b"
        );
        assert_eq!(
            resolve_image_model("openai/gpt-4o-mini", "black-forest-labs/flux.2-klein-4b"),
            "black-forest-labs/flux.2-klein-4b"
        );
        assert_eq!(
            resolve_image_model(
                "black-forest-labs/flux.2-klein-4b",
                "black-forest-labs/flux.2-klein-4b"
            ),
            "black-forest-labs/flux.2-klein-4b"
        );
        assert_eq!(
            resolve_image_model(
                "google/gemini-2.5-flash-image",
                "black-forest-labs/flux.2-klein-4b"
            ),
            "google/gemini-2.5-flash-image"
        );
    }

    #[test]
    fn detects_chat_image_models() {
        assert!(is_chat_image_model("google/gemini-2.5-flash-image"));
        assert!(!is_chat_image_model("black-forest-labs/flux.2-klein-4b"));
    }

    #[test]
    fn sorts_models_by_vendor_then_name() {
        let mut models = vec![
            ImageModel {
                id: "openai/gpt-5.2".into(),
                name: "GPT-5.2".into(),
                backend: "openrouter".into(),
            },
            ImageModel {
                id: "anthropic/claude-opus-5.5".into(),
                name: "Claude Opus 5.5".into(),
                backend: "openrouter".into(),
            },
            ImageModel {
                id: "anthropic/claude-sonnet-5.5".into(),
                name: "Claude Sonnet 5.5".into(),
                backend: "openrouter".into(),
            },
            ImageModel {
                id: "google/gemini-4-neon".into(),
                name: "Gemini 4 Neon".into(),
                backend: "openrouter".into(),
            },
        ];
        sort_models_by_vendor_name(&mut models);
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "anthropic/claude-opus-5.5",
                "anthropic/claude-sonnet-5.5",
                "google/gemini-4-neon",
                "openai/gpt-5.2",
            ]
        );
    }

    #[test]
    fn filters_agent_and_router_capabilities() {
        let chat = serde_json::json!({
            "id": "anthropic/claude-sonnet-5.5",
            "name": "Anthropic: Claude Sonnet 5.5",
            "architecture": { "output_modalities": ["text"] },
            "endpoints": ["/api/v1/chat/completions"],
            "supported_parameters": ["tools", "tool_choice", "response_format"]
        });
        assert!(is_text_chat_candidate(&chat));
        assert!(is_agent_capable(&chat));
        assert!(is_router_capable(&chat));

        // Tools missing and no JSON params → not agent, not router (do not guess).
        let bare = serde_json::json!({
            "id": "writer/palmyra-x5",
            "architecture": { "output_modalities": ["text"] },
            "endpoints": ["/api/v1/chat/completions"],
            "supported_parameters": ["temperature", "max_tokens"]
        });
        assert!(is_text_chat_candidate(&bare));
        assert!(!is_agent_capable(&bare));
        assert!(!is_router_capable(&bare));

        // JSON without tools → router only (dolphin-class).
        let dolphin = serde_json::json!({
            "id": "cognitivecomputations/dolphin-mistral-24b-venice-edition",
            "name": "Dolphin Mistral 24B Venice",
            "architecture": { "output_modalities": ["text"] },
            "endpoints": ["/api/v1/chat/completions"],
            "supported_parameters": ["response_format", "temperature"]
        });
        assert!(!is_agent_capable(&dolphin));
        assert!(is_router_capable(&dolphin));

        // Missing supported_parameters → exclude from both.
        let missing = serde_json::json!({
            "id": "meta-llama/llama-3.1-8b-instruct",
            "architecture": { "output_modalities": ["text"] },
            "endpoints": ["/api/v1/chat/completions"]
        });
        assert!(is_text_chat_candidate(&missing));
        assert!(!is_agent_capable(&missing));
        assert!(!is_router_capable(&missing));

        // Live Llama 3.1 8B parameter set qualifies as router and agent.
        let llama = serde_json::json!({
            "id": "meta-llama/llama-3.1-8b-instruct",
            "architecture": { "output_modalities": ["text"] },
            "endpoints": ["/api/v1/chat/completions"],
            "supported_parameters": ["response_format", "structured_outputs", "tools", "tool_choice", "temperature"]
        });
        assert!(is_agent_capable(&llama));
        assert!(is_router_capable(&llama));

        let batch = serde_json::json!({
            "id": "google/gemini-4-neon:batch",
            "architecture": { "output_modalities": ["text"] },
            "endpoints": ["/api/v1/chat/completions"],
            "supported_parameters": ["tools", "response_format"]
        });
        assert!(!is_text_chat_candidate(&batch));
        assert!(!is_agent_capable(&batch));

        let free = serde_json::json!({
            "id": "cognitivecomputations/dolphin-mistral-24b-venice-edition:free",
            "architecture": { "output_modalities": ["text"] },
            "endpoints": ["/api/v1/chat/completions"],
            "supported_parameters": ["tools"]
        });
        assert!(!is_agent_capable(&free));

        let image = serde_json::json!({
            "id": "google/gemini-2.5-flash-image",
            "architecture": { "output_modalities": ["text", "image"] },
            "endpoints": ["/api/v1/chat/completions"],
            "supported_parameters": ["tools"]
        });
        assert!(!is_agent_capable(&image));
        assert!(!keeps_declared_output(&image, "video"));
        assert!(keeps_declared_output(&image, "image"));

        let embed = serde_json::json!({
            "id": "openai/text-embedding-3-large",
            "architecture": { "output_modalities": ["embeddings"] },
            "endpoints": ["/api/v1/embeddings"],
            "supported_parameters": ["tools"]
        });
        assert!(!is_agent_capable(&embed));

        // Upstream router is an explicit agent option even with no tools flag.
        let upstream = serde_json::json!({
            "id": "openrouter/auto",
            "architecture": { "output_modalities": ["text", "image"] },
            "supported_parameters": ["temperature"]
        });
        assert!(is_agent_capable(&upstream));
        assert!(!is_router_capable(&upstream));

        let text_only = serde_json::json!({
            "id": "openai/gpt-4o-mini",
            "architecture": { "output_modalities": ["text"] }
        });
        assert!(!keeps_declared_output(&text_only, "image"));
        assert!(!keeps_declared_output(&text_only, "video"));
    }

    #[test]
    fn humanizes_zdr() {
        let msg = humanize_openrouter_error(
            404,
            r#"{"error":{"message":"ZDR violation (guardrail): 1 endpoint excluded"}}"#,
        );
        assert!(msg.contains("ZDR/data-policy") || msg.contains("ZDR"));
        assert!(msg.contains("settings/privacy"));
        assert!(msg.contains("Freya Settings"));
        assert!(is_zdr_routing_error(&msg));
    }

    #[test]
    fn detects_zdr_routing_errors() {
        assert!(is_zdr_routing_error(
            "OpenRouter blocked media routing (ZDR/data-policy, HTTP 404). No endpoints matching"
        ));
        assert!(is_zdr_routing_error(
            r#"{"error":{"message":"No endpoints found that match your data policy (ZDR)"}}"#
        ));
        assert!(!is_zdr_routing_error("openrouter 402: insufficient credits"));
    }

    #[test]
    fn filters_fallback_image_for_zdr() {
        let all = fallback_models();
        let zdr = filter_fallback_image(&all, true);
        assert!(zdr.iter().all(|m| m.id.starts_with("google/")));
    }
}
