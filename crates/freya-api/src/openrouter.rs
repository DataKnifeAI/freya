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
            return Err(anyhow!("openrouter {status}: {text}"));
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
        let model = model.unwrap_or(&self.image_model);
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

        // Prefer dedicated images API; fall back to chat modalities if needed.
        match self.auth_post("/images", body.clone()).await {
            Ok(v) => extract_image(&v),
            Err(_) => {
                let chat_body = serde_json::json!({
                    "model": model,
                    "messages": [{"role": "user", "content": prompt}],
                    "modalities": ["image", "text"]
                });
                let v = self.auth_post("/chat/completions", chat_body).await?;
                extract_image_from_chat(&v)
            }
        }
    }
}

fn size_to_aspect(size: &str) -> Option<&'static str> {
    match size {
        "1024x1024" | "1:1" => Some("1:1"),
        "1024x576" | "16:9" => Some("16:9"),
        "576x1024" | "9:16" => Some("9:16"),
        _ => None,
    }
}

fn extract_image(v: &Value) -> Result<GeneratedImage> {
    if let Some(b64) = v.pointer("/data/0/b64_json").and_then(|x| x.as_str()) {
        return Ok(GeneratedImage {
            b64: b64.to_string(),
            mime: "image/png".into(),
        });
    }
    if let Some(url) = v.pointer("/data/0/url").and_then(|x| x.as_str()) {
        if let Some(rest) = url.strip_prefix("data:image/png;base64,") {
            return Ok(GeneratedImage {
                b64: rest.to_string(),
                mime: "image/png".into(),
            });
        }
        return Err(anyhow!("image url responses not downloaded yet: {url}"));
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
            id: "openai/gpt-image-2".into(),
            name: "GPT Image 2".into(),
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
