//! Thin HTTP client for freya-api.

use anyhow::{anyhow, Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct FreyaApiClient {
    http: Client,
    base: String,
    token: String,
}

impl FreyaApiClient {
    pub fn new(base_url: &str, token: &str) -> Result<Self> {
        let base = base_url.trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err(anyhow!("FREYA_API_URL must not be empty"));
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .context("build HTTP client")?;
        Ok(Self {
            http,
            base,
            token: token.to_string(),
        })
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, None).await
    }

    pub async fn get_json_query(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
        let mut url = join_api_url(&self.base, path)?;
        if !query.is_empty() {
            let qs: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect();
            url.push('?');
            url.push_str(&qs.join("&"));
        }
        self.request_url(Method::GET, &url, None).await
    }

    pub async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::POST, path, Some(body.clone())).await
    }

    pub async fn put_json(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::PUT, path, Some(body.clone())).await
    }

    pub async fn delete_json(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::DELETE, path, Some(body.clone())).await
    }

    async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let url = join_api_url(&self.base, path)?;
        self.request_url(method, &url, body).await
    }

    async fn request_url(&self, method: Method, url: &str, body: Option<Value>) -> Result<Value> {
        let mut req = self.http.request(method.clone(), url);
        if !self.token.is_empty() {
            req = req.header(AUTHORIZATION, format!("Bearer {}", self.token));
        }
        if body.is_some() {
            req = req.header(CONTENT_TYPE, "application/json");
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("{method} {url}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status == StatusCode::UNAUTHORIZED {
            return Err(anyhow!(
                "freya-api returned 401 Unauthorized — check FREYA_API_TOKEN matches Studio apiTokenSecretRef"
            ));
        }
        if !status.is_success() {
            return Err(anyhow!("freya-api {status}: {text}"));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).with_context(|| format!("parse JSON from {url}"))
    }
}

/// Single path segment: a UUID (hyphenated or simple) or `[A-Za-z0-9_-]+`.
/// Rejects `/`, `?`, `#`, `%`, `.`, and `..`. Does not percent-encode.
fn is_safe_path_segment(id: &str) -> bool {
    !id.is_empty()
        && !id.contains('/')
        && !id.contains('?')
        && !id.contains('#')
        && !id.contains('%')
        && !id.contains('.')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn safe_path_segment(id: &str) -> Result<&str> {
    let id = id.trim();
    if !is_safe_path_segment(id) {
        return Err(anyhow!("invalid id"));
    }
    Ok(id)
}

/// Join a fixed route around one tool-supplied id. Errors before a URL exists
/// when the id is not a single safe segment.
pub(crate) fn bind_path_segment(prefix: &str, id: &str, suffix: &str) -> Result<String> {
    let id = safe_path_segment(id)?;
    reject_unsafe_api_path(prefix)?;
    if !suffix.is_empty() {
        reject_unsafe_api_path(suffix)?;
    }
    let path = format!("{prefix}/{id}{suffix}");
    reject_unsafe_api_path(&path)?;
    Ok(path)
}

fn reject_unsafe_api_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.contains("//") {
        return Err(anyhow!("refusing API path"));
    }
    for seg in path.split('/') {
        if seg.is_empty() {
            continue;
        }
        if !is_safe_path_segment(seg) {
            return Err(anyhow!("refusing API path"));
        }
    }
    Ok(())
}

/// Build `base + path` only when every segment is safe and URL parsing keeps
/// the path on that route (no dot-segment or query rewrite).
fn join_api_url(base: &str, path: &str) -> Result<String> {
    reject_unsafe_api_path(path)?;
    let url = format!("{base}{path}");
    let parsed = reqwest::Url::parse(&url).context("parse API url")?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(anyhow!("refusing API path"));
    }
    let base_parsed = reqwest::Url::parse(base).context("parse API base")?;
    let base_path = base_parsed.path().trim_end_matches('/');
    let expected = format!("{base_path}{path}");
    if parsed.path() != expected || parsed.host_str() != base_parsed.host_str() {
        return Err(anyhow!("refusing API path that leaves the intended route"));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_mcp_path_injection() {
        let base = "http://freya-api.internal";
        for id in [
            "../auth/token",
            "x?x",
            "../../auth/token?",
            "../../worker/start?",
        ] {
            assert!(
                bind_path_segment("/v1/civitai/jobs", id, "/cancel").is_err(),
                "{id}"
            );
            assert!(bind_path_segment("/v1/gallery", id, "").is_err(), "{id}");
            assert!(
                bind_path_segment("/v1/gallery", id, "/image").is_err(),
                "{id}"
            );
            let raw_path = format!("/v1/civitai/jobs/{id}/cancel");
            assert!(
                join_api_url(base, &raw_path).is_err(),
                "{id} must not become a request URL"
            );
            let parsed = reqwest::Url::parse(&format!("{base}{raw_path}")).unwrap();
            let intended = format!("/v1/civitai/jobs/{id}/cancel");
            assert_ne!(
                parsed.path(),
                intended,
                "{id} should leave the cancel route when interpolated raw"
            );
            assert!(
                parsed.query().is_some()
                    || !parsed.path().starts_with("/v1/civitai/jobs/")
                    || !parsed.path().ends_with("/cancel"),
                "raw URL still on the cancel route: {parsed}"
            );
        }

        for id in [
            "",
            ".",
            "..",
            "a/b",
            "a%2e%2e",
            "a#b",
            "a.b",
            "a b",
            "urn:uuid:550e8400-e29b-41d4-a716-446655440000",
        ] {
            assert!(
                bind_path_segment("/v1/civitai/jobs", id, "/cancel").is_err(),
                "{id}"
            );
        }

        let id = "550e8400-e29b-41d4-a716-446655440000";
        let path = bind_path_segment("/v1/civitai/jobs", id, "/cancel").unwrap();
        assert_eq!(path, format!("/v1/civitai/jobs/{id}/cancel"));
        assert!(!path.contains('%'));
        let url = join_api_url(base, &path).unwrap();
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert_eq!(parsed.path(), path);
        assert!(parsed.query().is_none());
        assert!(parsed.fragment().is_none());

        let gallery = bind_path_segment("/v1/gallery", "job_42-OK", "").unwrap();
        let gurl = join_api_url(base, &gallery).unwrap();
        assert_eq!(
            reqwest::Url::parse(&gurl).unwrap().path(),
            "/v1/gallery/job_42-OK"
        );

        // Fixed routes, including worker start/stop, stay on their path.
        for path in [
            "/v1/health",
            "/v1/worker/start",
            "/v1/worker/stop",
            "/v1/worker/base-model",
            "/v1/civitai/jobs/clear",
            "/v1/metrics/openrouter",
        ] {
            let url = join_api_url(base, path).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_eq!(reqwest::Url::parse(&url).unwrap().path(), path);
        }
    }
}
