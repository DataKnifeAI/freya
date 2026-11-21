use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMeta {
    pub id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhanced_prompt: Option<String>,
    pub backend: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loras: Vec<String>,
    pub created_at: String,
    pub mime: String,
    pub filename: String,
}

#[derive(Clone)]
pub struct Gallery {
    root: PathBuf,
}

impl Gallery {
    pub async fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).await?;
        Ok(Self { root })
    }

    pub async fn save(
        &self,
        bytes: &[u8],
        mime: &str,
        meta: &mut ArtifactMeta,
    ) -> Result<ArtifactMeta> {
        let id = Uuid::new_v4().to_string();
        let ext = if mime.contains("jpeg") || mime.contains("jpg") {
            "jpg"
        } else {
            "png"
        };
        let filename = format!("{id}.{ext}");
        meta.id = id.clone();
        meta.filename = filename.clone();
        meta.created_at = Utc::now().to_rfc3339();
        meta.mime = mime.to_string();

        let img_path = self.root.join(&filename);
        let meta_path = self.root.join(format!("{id}.json"));
        fs::write(&img_path, bytes).await.context("write image")?;
        let json = serde_json::to_vec_pretty(meta)?;
        fs::write(&meta_path, json).await.context("write meta")?;
        Ok(meta.clone())
    }

    pub async fn list(&self) -> Result<Vec<ArtifactMeta>> {
        let mut out = Vec::new();
        let mut rd = fs::read_dir(&self.root).await?;
        while let Some(ent) = rd.next_entry().await? {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read(&path).await?;
            if let Ok(m) = serde_json::from_slice::<ArtifactMeta>(&raw) {
                out.push(m);
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    pub async fn get(&self, id: &str) -> Result<Option<ArtifactMeta>> {
        let path = self.root.join(format!("{id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(path).await?;
        Ok(Some(serde_json::from_slice(&raw)?))
    }

    pub fn image_path(&self, meta: &ArtifactMeta) -> PathBuf {
        self.root.join(&meta.filename)
    }
}
