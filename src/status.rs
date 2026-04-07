use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct PaneStatus {
    pub pane: String,
    pub status: String,
    pub ts: i64,
}

pub fn state_dir() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("hive").join("panes")
}

fn encode_pane(pane: &str) -> String {
    pane.replace(['/', ':'], "_")
}

pub fn file_for(pane: &str) -> PathBuf {
    state_dir().join(format!("{}.json", encode_pane(pane)))
}

pub fn write(pane: &str, status: &str) -> Result<()> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let entry = PaneStatus {
        pane: pane.to_string(),
        status: status.to_string(),
        ts: chrono::Utc::now().timestamp(),
    };
    let json = serde_json::to_string(&entry)?;
    let path = file_for(pane);
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

pub fn read(path: &Path) -> Option<PaneStatus> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn list() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(state_dir()) {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|x| x == "json") {
                out.push(e.path());
            }
        }
    }
    out
}

