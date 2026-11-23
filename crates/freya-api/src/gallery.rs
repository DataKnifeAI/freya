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
///
/// Magic bytes win over the stored mime. A GIF file is always `image/gif`
/// so preview responses are not relabeled as PNG/WebP (which shows one frame
/// or a broken image). Bytes are never transcoded here.
pub fn content_type_for_bytes(bytes: &[u8], mime: &str) -> &'static str {
    if let Some(sniffed) = sniff_media_type(bytes) {
        return sniffed;
    }
    content_type_for_mime(mime)
}

fn sniff_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Some("image/gif");
    }
    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 3 && bytes.starts_with(b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("video/mp4");
    }
    if bytes.len() >= 4 && bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        return Some("video/webm");
    }
    None
}

/// Content-Type from the stored artifact mime when the bytes are not a known signature.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_bytes_stay_image_gif_even_if_mime_says_still() {
        let bytes = b"GIF89a\x01\x00\x01\x00";
        assert_eq!(content_type_for_bytes(bytes, "image/png"), "image/gif");
        assert_eq!(content_type_for_bytes(bytes, "image/webp"), "image/gif");
        assert_eq!(content_type_for_bytes(bytes, "image/gif"), "image/gif");
        assert_eq!(content_type_for_bytes(b"GIF87a", "image/png"), "image/gif");
    }

    #[test]
    fn stills_and_video_keep_their_types() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        assert_eq!(content_type_for_bytes(png, "image/gif"), "image/png");
        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBP");
        assert_eq!(content_type_for_bytes(&webp, "image/png"), "image/webp");
        let mut mp4 = vec![0, 0, 0, 24];
        mp4.extend_from_slice(b"ftypisom");
        assert_eq!(content_type_for_bytes(&mp4, "image/gif"), "video/mp4");
        assert_eq!(
            content_type_for_bytes(b"not-a-signature", "image/gif"),
            "image/gif"
        );
    }
}
