//! Filesystem capacity for gallery PVC / data dirs (df-style via statvfs).

use serde_json::{json, Value};
use std::ffi::CString;
use std::path::Path;

/// Return `{ path, usedBytes, freeBytes, totalBytes, usedPercent }` or `Null`.
pub fn collect_path_disk(path: &Path) -> Value {
    let probe = if path.exists() {
        path.to_path_buf()
    } else if let Some(parent) = path.parent() {
        parent.to_path_buf()
    } else {
        return Value::Null;
    };
    let Some(s) = probe.to_str() else {
        return Value::Null;
    };
    let Ok(c_path) = CString::new(s) else {
        return Value::Null;
    };

    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut st) != 0 {
            return Value::Null;
        }
        let bsize = st.f_frsize as u64;
        if bsize == 0 {
            return Value::Null;
        }
        let total = (st.f_blocks as u64).saturating_mul(bsize);
        let free = (st.f_bavail as u64).saturating_mul(bsize);
        let free_root = (st.f_bfree as u64).saturating_mul(bsize);
        let used = total.saturating_sub(free_root);
        let pct = if total > 0 {
            ((used as f64 / total as f64) * 100.0).round() as u64
        } else {
            0
        };
        json!({
            "path": path.display().to_string(),
            "usedBytes": used,
            "freeBytes": free,
            "totalBytes": total,
            "usedPercent": pct.min(100),
        })
    }
}
