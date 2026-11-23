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
    pub fn new(base: impl Into<String>) -> Self {
        Self::new_with_tls(base, false)
    }

    pub fn new_with_tls(base: impl Into<String>, tls_insecure: bool) -> Self {
        let base = base.into().trim_end_matches('/').to_string();
        // Public HTTPS keeps verification. The flag only applies to loopback,
        // private, link-local, and cluster DNS. Same rule as the operator.
        let skip = tls_insecure && tls_skip_allowed(&base);
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(skip)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            base,
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
        self.http
            .get(format!("{}/health", self.base))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Value>().await.ok()
    }

    /// Authenticated `/v1/stats`. A failed call is not replaced with `/health`
    /// (`{ok:true}` has no GPU or inventory). Use [`Self::health`] for reachability.
    pub async fn stats_detail(&self) -> Option<Value> {
        let resp = self
            .apply_auth(self.http.get(format!("{}/v1/stats", self.base)))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = resp.json::<Value>().await.ok()?;
        if inventory_stats(&body) {
            Some(body)
        } else {
            None
        }
    }

    pub async fn list_loras(&self) -> Result<Vec<LocalAsset>> {
        let resp = self
            .apply_auth(self.http.get(format!("{}/v1/loras", self.base)))
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
                    base_model: a
                        .get("baseModel")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                    name: a.get("name").and_then(|x| x.as_str()).map(str::to_string),
                    size_bytes: a.get("sizeBytes").and_then(|x| x.as_u64()).or_else(|| {
                        a.get("sizeBytes")
                            .and_then(|x| x.as_i64())
                            .map(|n| n as u64)
                    }),
                    model_type: a
                        .get("modelType")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
                    version_id: a.get("versionId").and_then(|x| x.as_i64()),
                    model_id: a.get("modelId").and_then(|x| x.as_i64()),
                    source: a.get("source").and_then(|x| x.as_str()).map(str::to_string),
                    thumbnail_url: a
                        .get("thumbnailUrl")
                        .and_then(|x| x.as_str())
                        .map(str::to_string),
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
    ) -> Result<()> {
        let mut body = serde_json::json!({
            "source": source,
            "url": url,
            "destPath": dest_path,
        });
        if let Some(t) = auth_token.map(str::trim).filter(|t| !t.is_empty()) {
            body["authToken"] = Value::String(t.into());
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

/// True when the JSON is worker inventory, not the liveness `{ok:true}` body.
pub(crate) fn inventory_stats(v: &Value) -> bool {
    v.get("modelsDir").is_some()
        || v.get("loras").is_some()
        || v.get("checkpoints").is_some()
        || v.get("vram").is_some()
        || v.get("disk").is_some()
}

/// Public HTTPS is verified. Skip only for loopback, private, link-local, or cluster DNS.
/// Keep in sync with `freya_operator::reconcile::tls_skip_allowed`.
pub(crate) fn tls_skip_allowed(endpoint: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host_is_local_or_private(host)
}

fn host_is_local_or_private(host: &str) -> bool {
    let h = host
        .trim()
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if h.is_empty()
        || h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".cluster.local")
        || h.ends_with(".svc")
        || !h.contains('.')
    {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            let [a, b, _, _] = ip.octets();
            ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || a == 0
                || a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.is_unicast_link_local() || (ip.octets()[0] & 0xfe) == 0xfc
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tls_tests {
    use super::{inventory_stats, tls_skip_allowed};

    #[test]
    fn health_ok_is_not_inventory() {
        assert!(!inventory_stats(&serde_json::json!({ "ok": true })));
        assert!(!inventory_stats(
            &serde_json::json!({ "ok": true, "service": "freya-worker" })
        ));
        assert!(inventory_stats(
            &serde_json::json!({ "ok": true, "vram": null, "loras": 0, "modelsDir": "/models" })
        ));
    }

    #[test]
    fn public_https_does_not_skip_verify() {
        assert!(!tls_skip_allowed("https://vllm.dataknife.net"));
        assert!(tls_skip_allowed("https://192.168.3.125:8091"));
        assert!(tls_skip_allowed("https://127.0.0.1:8091"));
        assert!(tls_skip_allowed(
            "https://freya-gpu.freya.svc.cluster.local:8091"
        ));
        assert!(tls_skip_allowed("http://freya-worker:8091"));
    }
}
