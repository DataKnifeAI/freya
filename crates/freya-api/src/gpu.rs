//! Host GPU stats via `nvidia-smi` (used when the worker cannot see the GPU).

use serde_json::{json, Value};
use tokio::process::Command;

/// Parse `nvidia-smi` CSV into `{ usedMiB, totalMiB, utilPercent, gpus }` or `Null`.
pub async fn collect_host_vram() -> Value {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.used,memory.total,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await;
    let Ok(out) = out else {
        return Value::Null;
    };
    if !out.status.success() {
        return Value::Null;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut gpus = Vec::new();
    let mut used_total = 0u64;
    let mut mem_total = 0u64;
    let mut util_sum = 0u64;
    for line in text.lines() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 5 {
            continue;
        }
        let index: u64 = parts[0].parse().unwrap_or(0);
        let name = parts[1].to_string();
        let used: u64 = parts[2].parse().unwrap_or(0);
        let total: u64 = parts[3].parse().unwrap_or(0);
        let util: u64 = parts[4].parse().unwrap_or(0);
        used_total += used;
        mem_total += total;
        util_sum += util;
        gpus.push(json!({
            "index": index,
            "name": name,
            "usedMiB": used,
            "totalMiB": total,
            "utilPercent": util,
        }));
    }
    if gpus.is_empty() {
        return Value::Null;
    }
    let n = gpus.len() as u64;
    json!({
        "usedMiB": used_total,
        "totalMiB": mem_total,
        "utilPercent": util_sum / n.max(1),
        "gpus": gpus,
        "source": "host-nvidia-smi",
    })
}

pub fn hostname_string() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "unknown".into())
}
