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
        let mut url = format!("{}{}", self.base, path);
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
        let url = format!("{}{}", self.base, path);
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
