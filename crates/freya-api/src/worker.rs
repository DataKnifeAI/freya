//! Local / remote diffusion worker client (vLLM-Omni compatible + Freya LoRA install).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone)]
pub struct DiffusionWorkerClient {
    http: reqwest::Client,
    base: String,
}

impl DiffusionWorkerClient {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn health(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn list_loras(&self) -> Result<Vec<LocalAsset>> {
        let resp = self
            .http
            .get(format!("{}/v1/loras", self.base))
            .send()
            .await
            .context("list loras")?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        let mut out = Vec::new();
        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            for a in arr {
                out.push(LocalAsset {
                    id: a
                        .get("id")
                        .or_else(|| a.get("name"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    kind: a
                        .get("kind")
                        .and_then(|x| x.as_str())
                        .unwrap_or("lora")
                        .to_string(),
                    path: a
                        .get("path")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                });
            }
        }
        Ok(out)
    }

    pub async fn generate(
        &self,
        prompt: &str,
        model: Option<&str>,
        loras: &[LoraWeight],
        size: Option<&str>,
        seed: Option<i64>,
        negative: Option<&str>,
    ) -> Result<Vec<u8>> {
        let mut body = serde_json::json!({
            "prompt": prompt,
            "n": 1,
            "response_format": "b64_json",
        });
        if let Some(m) = model {
            body["model"] = Value::String(m.into());
        }
        if let Some(s) = size {
            body["size"] = Value::String(s.into());
        }
        if let Some(seed) = seed {
            body["seed"] = Value::from(seed);
        }
        if let Some(neg) = negative {
            body["negative_prompt"] = Value::String(neg.into());
        }
        if !loras.is_empty() {
            body["lora_request"] = serde_json::to_value(loras)?;
        }

        let resp = self
            .http
            .post(format!("{}/v1/images/generations", self.base))
            .json(&body)
            .send()
            .await
            .context("worker generate")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("worker {status}: {text}"));
        }
        let v: Value = serde_json::from_str(&text)?;
        let b64 = v
            .pointer("/data/0/b64_json")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("worker missing b64_json"))?;
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("decode worker image")
    }

    pub async fn install_asset(&self, source: &str, url: &str, dest_path: &str) -> Result<()> {
        let body = serde_json::json!({
            "source": source,
            "url": url,
            "destPath": dest_path,
        });
        let resp = self
            .http
            .post(format!("{}/v1/assets/install", self.base))
            .json(&body)
            .send()
            .await
            .context("worker install")?;
        if !resp.status().is_success() {
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("install failed: {t}"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraWeight {
    pub lora_name: String,
    #[serde(default = "one")]
    pub lora_scale: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAsset {
    pub id: String,
    pub kind: String,
    pub path: String,
}
