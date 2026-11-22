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
        let ext = mime_to_ext(mime);
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
        let Some(id) = sanitize_gallery_id(id) else {
            return Ok(None);
        };
        let path = self.root.join(format!("{id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(path).await?;
        Ok(Some(serde_json::from_slice(&raw)?))
    }

    /// Remove artifact bytes + metadata. Returns `false` if id is invalid or missing.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let Some(id) = sanitize_gallery_id(id) else {
            return Ok(false);
        };
        let Some(meta) = self.get(id).await? else {
            return Ok(false);
        };
        let img = self.image_path(&meta);
        let meta_path = self.root.join(format!("{id}.json"));
        if img.exists() {
            fs::remove_file(&img)
                .await
                .with_context(|| format!("remove gallery image {}", img.display()))?;
        }
        fs::remove_file(&meta_path)
            .await
            .with_context(|| format!("remove gallery meta {}", meta_path.display()))?;
        Ok(true)
    }

    pub fn image_path(&self, meta: &ArtifactMeta) -> PathBuf {
        self.root.join(&meta.filename)
    }
}

/// Gallery ids are UUIDs written by [`Gallery::save`]. Reject path traversal.
fn sanitize_gallery_id(id: &str) -> Option<&str> {
    if Uuid::parse_str(id).is_ok() {
        Some(id)
    } else {
        None
    }
}

fn mime_to_ext(mime: &str) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m.contains("jpeg") || m.contains("jpg") {
        "jpg"
    } else if m.contains("webp") {
        "webp"
    } else if m.contains("gif") {
        "gif"
    } else if m.contains("webm") {
        "webm"
    } else if m.contains("mp4") || m.contains("mpeg") || m.contains("quicktime") {
        "mp4"
    } else if m.contains("png") {
        "png"
    } else {
        "bin"
    }
}

/// Content-Type for serving a gallery artifact.
pub fn content_type_for_mime(mime: &str) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m.contains("jpeg") || m.contains("jpg") {
        "image/jpeg"
    } else if m.contains("webp") {
        "image/webp"
    } else if m.contains("gif") {
        "image/gif"
    } else if m.contains("webm") {
        "video/webm"
    } else if m.contains("mp4") || m.contains("mpeg") || m.contains("quicktime") {
        "video/mp4"
    } else if m.contains("png") {
        "image/png"
    } else if !mime.is_empty() {
        // Pass through unknown but non-empty mime when possible — fall back to octet-stream.
        "application/octet-stream"
    } else {
        "application/octet-stream"
    }
}

pub fn is_video_mime(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.contains("mp4") || m.contains("webm") || m.contains("quicktime") || m.starts_with("video/")
}

pub fn is_animated_image_mime(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.contains("gif") || m.contains("webp")
}
