//! Civitai search / download URL helpers (requires user-provided API key).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        if !self.configured() {
            return Err(anyhow!(
                "Civitai API key not set — add it in Freya Settings or CIVITAI_API_KEY"
            ));
        }
        let url = format!(
            "https://civitai.com/api/v1/models?limit={limit}&query={}",
            urlencoding_lite(query)
        );
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
        Ok(out)
    }
}

fn urlencoding_lite(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".into(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
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
