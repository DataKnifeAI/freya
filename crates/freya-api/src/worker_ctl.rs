//! Local worker process control (docker preferred, systemd fallback).

use serde_json::{json, Value};
use tokio::process::Command;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Docker,
    Systemd,
    None,
}

#[derive(Debug, Clone)]
pub struct RuntimeStatus {
    pub kind: RuntimeKind,
    pub name: String,
    pub running: Option<bool>,
    pub detail: String,
}

impl RuntimeStatus {
    pub fn to_json(&self) -> Value {
        json!({
            "kind": match self.kind {
                RuntimeKind::Docker => "docker",
                RuntimeKind::Systemd => "systemd",
                RuntimeKind::None => "none",
            },
            "name": self.name,
            "running": self.running,
            "detail": self.detail,
        })
    }
}

/// Prefer docker when the named container exists; else systemd unit if active/loaded.
pub async fn detect(container: &str, unit: &str) -> RuntimeStatus {
    if let Some(st) = docker_status(container).await {
        return st;
    }
    if let Some(st) = systemd_status(unit).await {
        return st;
    }
    RuntimeStatus {
        kind: RuntimeKind::None,
        name: container.to_string(),
        running: None,
        detail: format!(
            "no docker container `{container}` and no systemd unit `{unit}` — start/stop unavailable for remote workers"
        ),
    }
}

async fn docker_status(name: &str) -> Option<RuntimeStatus> {
    let out = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", name])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let running = String::from_utf8_lossy(&out.stdout).trim() == "true";
    Some(RuntimeStatus {
        kind: RuntimeKind::Docker,
        name: name.to_string(),
        running: Some(running),
        detail: if running {
            "docker container running".into()
        } else {
            "docker container stopped".into()
        },
    })
}

async fn systemd_status(unit: &str) -> Option<RuntimeStatus> {
    let out = Command::new("systemctl")
        .args(["show", unit, "-p", "ActiveState", "--value"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if state.is_empty() || state == "not-found" || state == "inactive" {
        // Distinguish "unit exists but inactive" vs missing: try is-enabled / list-unit-files.
        let check = Command::new("systemctl")
            .args(["cat", unit])
            .output()
            .await
            .ok()?;
        if !check.status.success() {
            return None;
        }
    }
    let running = state == "active";
    Some(RuntimeStatus {
        kind: RuntimeKind::Systemd,
        name: unit.to_string(),
        running: Some(running),
        detail: format!("systemd ActiveState={state}"),
    })
}

pub async fn start(container: &str, unit: &str) -> Result<RuntimeStatus, String> {
    let detected = detect(container, unit).await;
    match detected.kind {
        RuntimeKind::Docker => {
            let out = Command::new("docker")
                .args(["start", container])
                .output()
                .await
                .map_err(|e| format!("docker start failed: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(format!("docker start: {err}"));
            }
            Ok(docker_status(container)
                .await
                .unwrap_or(detected))
        }
        RuntimeKind::Systemd => {
            let out = Command::new("systemctl")
                .args(["start", unit])
                .output()
                .await
                .map_err(|e| format!("systemctl start failed: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(format!("systemctl start: {err}"));
            }
            Ok(systemd_status(unit).await.unwrap_or(detected))
        }
        RuntimeKind::None => Err(detected.detail),
    }
}

pub async fn stop(container: &str, unit: &str) -> Result<RuntimeStatus, String> {
    let detected = detect(container, unit).await;
    match detected.kind {
        RuntimeKind::Docker => {
            // Short grace period so UI/proxy don't hang on the default 10s SIGTERM wait.
            let out = Command::new("docker")
                .args(["stop", "-t", "2", container])
                .output()
                .await
                .map_err(|e| format!("docker stop failed: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(format!("docker stop: {err}"));
            }
            Ok(docker_status(container)
                .await
                .unwrap_or(detected))
        }
        RuntimeKind::Systemd => {
            let out = Command::new("systemctl")
                .args(["stop", unit])
                .output()
                .await
                .map_err(|e| format!("systemctl stop failed: {e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(format!("systemctl stop: {err}"));
            }
            Ok(systemd_status(unit).await.unwrap_or(detected))
        }
        RuntimeKind::None => {
            warn!(%detected.detail, "worker stop requested but no local runtime");
            Err(detected.detail)
        }
    }
}
