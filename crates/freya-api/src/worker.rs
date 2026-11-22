//! Local / remote diffusion worker client (vLLM-Omni compatible + Freya LoRA install).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone)]
pub struct DiffusionWorkerClient {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl DiffusionWorkerClient {
    #[allow(dead_code)]
    pub fn new(base: impl Into<String>) -> Self {
        Self::new_with_tls(base, false)
    }

    pub fn new_with_tls(base: impl Into<String>, tls_insecure: bool) -> Self {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(tls_insecure)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
            token: None,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        let t = token.into();
        self.token = if t.trim().is_empty() { None } else { Some(t) };
        self
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    pub async fn health(&self) -> bool {
        self.health_detail().await.is_some()
    }

    /// Fetch worker `/health` JSON when reachable.
    pub async fn health_detail(&self) -> Option<Value> {
        let resp = self
            .http
            .get(format!("{}/health", self.base))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Value>().await.ok()
    }

    /// Prefer authenticated `/v1/stats`, fall back to public `/health`.
    pub async fn stats_detail(&self) -> Option<Value> {
        let resp = self
            .apply_auth(self.http.get(format!("{}/v1/stats", self.base)))
            .send()
            .await
            .ok();
        if let Some(resp) = resp {
            if resp.status().is_success() {
                if let Ok(v) = resp.json::<Value>().await {
                    return Some(v);
                }
            }
        }
        self.health_detail().await
    }

    pub async fn list_loras(&self) -> Result<Vec<LocalAsset>> {
        self.list_assets("/v1/loras").await
    }

    pub async fn list_checkpoints(&self) -> Result<Vec<LocalAsset>> {
        self.list_assets("/v1/checkpoints").await
    }

    async fn list_assets(&self, path: &str) -> Result<Vec<LocalAsset>> {
        let resp = self
            .apply_auth(self.http.get(format!("{}{path}", self.base)))
            .send()
            .await
            .with_context(|| format!("list assets {path}"))?;
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
                    base_model: a
                        .get("baseModel")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                    name: a
                        .get("name")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                });
            }
        }
        Ok(out)
    }

    pub async fn get_active_base(&self) -> Result<Value> {
        let resp = self
            .apply_auth(self.http.get(format!("{}/v1/base-model", self.base)))
            .send()
            .await
            .context("get active base")?;
        if !resp.status().is_success() {
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("get base-model failed: {t}"));
        }
        Ok(resp.json().await.unwrap_or(Value::Null))
    }

    pub async fn set_active_base(&self, model: &str, base_model: Option<&str>) -> Result<Value> {
        let mut body = serde_json::json!({ "model": model });
        if let Some(b) = base_model.map(str::trim).filter(|s| !s.is_empty()) {
            body["baseModel"] = Value::String(b.into());
        }
        let resp = self
            .apply_auth(
                self.http
                    .put(format!("{}/v1/base-model", self.base))
                    .json(&body),
            )
            .send()
            .await
            .context("set active base")?;
        if !resp.status().is_success() {
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("set base-model failed: {t}"));
        }
        Ok(resp.json().await.unwrap_or(Value::Null))
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
            .apply_auth(
                self.http
                    .post(format!("{}/v1/images/generations", self.base))
                    .json(&body),
            )
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

    pub async fn install_asset(
        &self,
        source: &str,
        url: &str,
        dest_path: &str,
        auth_token: Option<&str>,
        meta: Option<&InstallMeta>,
    ) -> Result<()> {
        let mut body = serde_json::json!({
            "source": source,
            "url": url,
            "destPath": dest_path,
        });
        if let Some(t) = auth_token.map(str::trim).filter(|t| !t.is_empty()) {
            body["authToken"] = Value::String(t.into());
        }
        if let Some(m) = meta {
            if let Some(n) = m.name.as_deref().filter(|s| !s.is_empty()) {
                body["name"] = Value::String(n.into());
            }
            if let Some(b) = m.base_model.as_deref().filter(|s| !s.is_empty()) {
                body["baseModel"] = Value::String(b.into());
            }
            if let Some(v) = m.version_id {
                body["versionId"] = Value::from(v);
            }
            if let Some(t) = m.model_type.as_deref().filter(|s| !s.is_empty()) {
                body["modelType"] = Value::String(t.into());
            }
        }
        let resp = self
            .apply_auth(
                self.http
                    .post(format!("{}/v1/assets/install", self.base))
                    .json(&body),
            )
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

#[derive(Debug, Clone, Default)]
pub struct InstallMeta {
    pub name: Option<String>,
    pub base_model: Option<String>,
    pub version_id: Option<i64>,
    pub model_type: Option<String>,
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
    #[serde(default)]
    pub base_model: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}
