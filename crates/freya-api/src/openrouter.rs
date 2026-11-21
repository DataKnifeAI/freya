use crate::config::Config;
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
}

impl OpenRouterClient {
    pub fn new(cfg: &Config) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: cfg.chat_url().trim_end_matches('/').to_string(),
            api_key: cfg.openrouter_api_key.clone(),
            chat_model: cfg.chat_model.clone(),
            image_model: cfg.image_model.clone(),
        }
    }

    pub fn with_key(mut self, key: String) -> Self {
        self.api_key = key;
        self
    }

    pub fn default_image_model(&self) -> &str {
        &self.image_model
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

    pub async fn chat(&self, system: &str, user: &str, model: Option<&str>) -> Result<String> {
        let model = model.unwrap_or(&self.chat_model);
        let body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]
        });
        let v = self.auth_post("/chat/completions", body).await?;
        v.pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("missing chat content"))
    }

    pub async fn list_image_models(&self) -> Result<Vec<ImageModel>> {
        if self.api_key.is_empty() {
            return Ok(fallback_models());
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
        let mut out = Vec::new();
        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            for m in arr {
                let id = m
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() {
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
        }
        if out.is_empty() {
            return Ok(fallback_models());
        }
        Ok(out)
    }

    pub async fn generate_image(
        &self,
        prompt: &str,
        model: Option<&str>,
        size: Option<&str>,
    ) -> Result<GeneratedImage> {
        let model = resolve_image_model(model.unwrap_or(&self.image_model), &self.image_model);

        // Gemini / Nano Banana family: chat completions + modalities.
        // Diffusion models (FLUX, GPT Image, …): dedicated /images API only —
        // do not fall back to chat (wrong modalities error masks ZDR/credits).
        if is_chat_image_model(&model) {
            return self
                .generate_image_via_chat(prompt, &model)
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

        let v = self.auth_post("/images", body).await?;
        extract_image(&self.http, &v).await
    }

    async fn generate_image_via_chat(&self, prompt: &str, model: &str) -> Result<GeneratedImage> {
        let chat_body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "modalities": ["image", "text"]
        });
        let v = self.auth_post("/chat/completions", chat_body).await?;
        extract_image_from_chat(&v)
    }
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
    let m = model.to_ascii_lowercase();
    if m.is_empty() || m.starts_with("openrouter/auto") {
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
    // Unknown — allow through (OpenRouter will 404 if invalid).
    true
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

fn humanize_openrouter_error(status: u16, text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("zdr") || lower.contains("zero data retention") || lower.contains("guardrail")
    {
        return format!(
            "OpenRouter blocked image routing (ZDR/guardrail, HTTP {status}). \
             Disable Zero Data Retention for image/other model groups at \
             https://openrouter.ai/settings/privacy (or pick a Google image model \
             that passes your guardrails). Details: {text}"
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageModel {
    pub id: String,
    pub name: String,
    pub backend: String,
}

#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub b64: String,
    pub mime: String,
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
    fn humanizes_zdr() {
        let msg = humanize_openrouter_error(
            404,
            r#"{"error":{"message":"ZDR violation (guardrail): 1 endpoint excluded"}}"#,
        );
        assert!(msg.contains("ZDR/guardrail"));
        assert!(msg.contains("settings/privacy"));
    }
}
