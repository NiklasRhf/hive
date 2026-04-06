use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub keybindings: KeybindingConfig,
    #[serde(default = "default_picker_size")]
    pub picker: PopupSize,
    #[serde(default = "default_jump_size")]
    pub jump: PopupSize,
    #[serde(default = "default_stats_size")]
    pub stats: PopupSize,
    #[serde(default, rename = "project")]
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Deserialize)]
pub struct PopupSize {
    #[serde(default = "default_popup_width")]
    pub width: u8,
    #[serde(default = "default_popup_height")]
    pub height: u8,
}

fn default_popup_width() -> u8 {
    60
}
fn default_popup_height() -> u8 {
    50
}
fn default_picker_size() -> PopupSize {
    PopupSize {
        width: 60,
        height: 60,
    }
}
fn default_jump_size() -> PopupSize {
    PopupSize {
        width: 60,
        height: 40,
    }
}
fn default_stats_size() -> PopupSize {
    PopupSize {
        width: 70,
        height: 50,
    }
}

#[derive(Debug, Deserialize)]
pub struct NotificationConfig {
    #[serde(default = "default_x")]
    pub x: i32,
    #[serde(default = "default_y")]
    pub y: i32,
    #[serde(default = "default_notif_width")]
    pub width: u16,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            x: default_x(),
            y: default_y(),
            width: default_notif_width(),
        }
    }
}

fn default_x() -> i32 {
    1560
}
fn default_y() -> i32 {
    10
}
fn default_notif_width() -> u16 {
    300
}

#[derive(Debug, Deserialize)]
pub struct KeybindingConfig {
    #[serde(default = "default_picker_key")]
    pub picker: String,
    #[serde(default = "default_jump_key")]
    pub jump: String,
    #[serde(default = "default_stats_key")]
    pub stats: String,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            picker: default_picker_key(),
            jump: default_jump_key(),
            stats: default_stats_key(),
        }
    }
}

fn default_picker_key() -> String {
    "M-s".to_string()
}
fn default_jump_key() -> String {
    "M-n".to_string()
}
fn default_stats_key() -> String {
    "M-g".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub path: String,
    pub cmd: Option<String>,
    pub worktree: Option<WorktreeConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorktreeConfig {
    pub base: String,
    #[serde(default)]
    pub copy_dirs: Vec<String>,
    #[serde(default)]
    pub copy_files: Vec<String>,
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = dirs::config_dir()
            .context("could not determine config directory")?
            .join("hive")
            .join("config.toml");
        Self::load_from(&config_path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut config: Config =
            toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))?;

        for project in &mut config.projects {
            project.path = expand_tilde(&project.path).to_string_lossy().into_owned();
            if let Some(wt) = &mut project.worktree {
                wt.base = expand_tilde(&wt.base).to_string_lossy().into_owned();
            }
        }

        Ok(config)
    }

    pub fn find_project(&self, name: &str) -> Option<&ProjectConfig> {
        self.projects.iter().find(|p| p.name == name)
    }
}
