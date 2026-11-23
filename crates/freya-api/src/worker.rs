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
        self.list_assets("/v1/loras").await
    }

    pub async fn list_checkpoints(&self) -> Result<Vec<LocalAsset>> {
        self.list_assets("/v1/checkpoints").await
    }

    pub async fn list_embeddings(&self) -> Result<Vec<LocalAsset>> {
        self.list_assets("/v1/embeddings").await
    }

    pub async fn list_vaes(&self) -> Result<Vec<LocalAsset>> {
        self.list_assets("/v1/vae").await
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

    /// Local multi-frame GIF via Diffusers engine (`POST /v1/videos/generations` format=gif).
    pub async fn generate_gif(
        &self,
        prompt: &str,
        model: Option<&str>,
        size: Option<&str>,
        seed: Option<i64>,
        negative: Option<&str>,
    ) -> Result<Vec<u8>> {
        let mut body = serde_json::json!({
            "prompt": prompt,
            "format": "gif",
            "frames": 6,
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
        let resp = self
            .apply_auth(
                self.http
                    .post(format!("{}/v1/videos/generations", self.base))
                    .json(&body),
            )
            .send()
            .await
            .context("worker gif generate")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("worker gif {status}: {text}"));
        }
        let v: Value = serde_json::from_str(&text)?;
        let b64 = v
            .pointer("/data/0/b64_json")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("worker gif missing b64_json"))?;
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("decode worker gif")
    }

    fn install_body(
        &self,
        source: &str,
        url: &str,
        dest_path: &str,
        auth_token: Option<&str>,
        meta: Option<&InstallMeta>,
        r#async: bool,
    ) -> Value {
        let mut body = serde_json::json!({
            "source": source,
            "url": url,
            "destPath": dest_path,
            "async": r#async,
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
            if let Some(mid) = m.model_id {
                body["modelId"] = Value::from(mid);
            }
            if let Some(t) = m.model_type.as_deref().filter(|s| !s.is_empty()) {
                body["modelType"] = Value::String(t.into());
            }
            if let Some(thumb) = m.thumbnail_url.as_deref().filter(|s| !s.is_empty()) {
                body["thumbnailUrl"] = Value::String(thumb.into());
            }
        }
        body
    }

    /// Synchronous install (blocks until complete). Prefer `enqueue_install` for UI/agent.
    pub async fn install_asset(
        &self,
        source: &str,
        url: &str,
        dest_path: &str,
        auth_token: Option<&str>,
        meta: Option<&InstallMeta>,
    ) -> Result<()> {
        let body = self.install_body(source, url, dest_path, auth_token, meta, false);
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

    /// Enqueue a background install job; returns worker JSON (`jobId`, `status`, `job`, …).
    pub async fn enqueue_install(
        &self,
        source: &str,
        url: &str,
        dest_path: &str,
        auth_token: Option<&str>,
        meta: Option<&InstallMeta>,
    ) -> Result<Value> {
        let body = self.install_body(source, url, dest_path, auth_token, meta, true);
        let resp = self
            .apply_auth(
                self.http
                    .post(format!("{}/v1/assets/jobs", self.base))
                    .json(&body),
            )
            .send()
            .await
            .context("worker enqueue install")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("enqueue failed: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn list_install_jobs(&self) -> Result<Value> {
        let resp = self
            .apply_auth(self.http.get(format!("{}/v1/assets/jobs", self.base)))
            .send()
            .await
            .context("list install jobs")?;
        if !resp.status().is_success() {
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("list jobs failed: {t}"));
        }
        Ok(resp.json().await.unwrap_or(Value::Null))
    }

    pub async fn get_install_job(&self, id: &str) -> Result<Value> {
        let url = worker_job_url(&self.base, id, false)?;
        let resp = self
            .apply_auth(self.http.get(url))
            .send()
            .await
            .context("get install job")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.as_u16() == 404 {
            return Err(anyhow!("job not found"));
        }
        if !status.is_success() {
            return Err(anyhow!("get job failed: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn cancel_install_job(&self, id: &str) -> Result<Value> {
        let url = worker_job_url(&self.base, id, true)?;
        let resp = self
            .apply_auth(self.http.post(url))
            .send()
            .await
            .context("cancel install job")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("cancel failed: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// Clear finished install jobs on the worker (succeeded/failed/cancelled).
    pub async fn clear_install_jobs(&self, include_queued: bool) -> Result<Value> {
        let body = serde_json::json!({ "includeQueued": include_queued });
        let resp = self
            .apply_auth(
                self.http
                    .post(format!("{}/v1/assets/jobs/clear", self.base))
                    .json(&body),
            )
            .send()
            .await
            .context("clear install jobs")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("clear jobs failed: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn delete_asset(&self, path: &str) -> Result<Value> {
        let body = serde_json::json!({ "path": path });
        let resp = self
            .apply_auth(
                self.http
                    .delete(format!("{}/v1/assets", self.base))
                    .json(&body),
            )
            .send()
            .await
            .context("delete asset")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("delete failed: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }
}

#[derive(Debug, Clone, Default)]
pub struct InstallMeta {
    pub name: Option<String>,
    pub base_model: Option<String>,
    pub version_id: Option<i64>,
    pub model_id: Option<i64>,
    pub model_type: Option<String>,
    pub thumbnail_url: Option<String>,
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

/// Single path segment: UUID or `[A-Za-z0-9_-]+`. No `/ ? # % . ..`.
/// Same rule as freya-mcp. Does not percent-encode.
fn safe_path_segment(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty()
        || id.contains('/')
        || id.contains('?')
        || id.contains('#')
        || id.contains('%')
        || id.contains('.')
        || id.contains("..")
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(anyhow!("invalid job id"));
    }
    Ok(id)
}

fn worker_job_url(base: &str, id: &str, cancel: bool) -> Result<String> {
    let id = safe_path_segment(id)?;
    let base = base.trim_end_matches('/');
    let suffix = if cancel {
        format!("/v1/assets/jobs/{id}/cancel")
    } else {
        format!("/v1/assets/jobs/{id}")
    };
    let url = format!("{base}{suffix}");
    let parsed = reqwest::Url::parse(&url).context("parse worker job url")?;
    if parsed.query().is_some() || parsed.fragment().is_some() || !parsed.path().ends_with(&suffix)
    {
        return Err(anyhow!("invalid job id"));
    }
    Ok(url)
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
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub model_type: Option<String>,
    #[serde(default)]
    pub version_id: Option<i64>,
    #[serde(default)]
    pub model_id: Option<i64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
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
    use super::{inventory_stats, tls_skip_allowed, worker_job_url};

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
    fn job_id_path_rejects_injection() {
        for id in ["../auth/token", "x?x", "../../worker/start?"] {
            assert!(worker_job_url("http://worker.internal", id, true).is_err());
            assert!(worker_job_url("http://worker.internal", id, false).is_err());
        }
        let url = worker_job_url(
            "http://worker.internal",
            "550e8400-e29b-41d4-a716-446655440000",
            true,
        )
        .unwrap();
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert_eq!(
            parsed.path(),
            "/v1/assets/jobs/550e8400-e29b-41d4-a716-446655440000/cancel"
        );
        assert!(parsed.query().is_none());
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
