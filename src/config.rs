use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub keybindings: KeybindingConfig,
    #[serde(default)]
    pub picker: PickerConfig,
    #[serde(default)]
    pub jump: JumpConfig,
    #[serde(default, rename = "project")]
    pub projects: Vec<ProjectConfig>,
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

#[derive(Debug, Deserialize)]
pub struct KeybindingConfig {
    #[serde(default = "default_picker_key")]
    pub picker: String,
    #[serde(default = "default_jump_key")]
    pub jump: String,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            picker: default_picker_key(),
            jump: default_jump_key(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PickerConfig {
    #[serde(default = "default_picker_width")]
    pub width: u8,
    #[serde(default = "default_picker_height")]
    pub height: u8,
}

impl Default for PickerConfig {
    fn default() -> Self {
        Self {
            width: default_picker_width(),
            height: default_picker_height(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct JumpConfig {
    #[serde(default = "default_jump_width")]
    pub width: u8,
    #[serde(default = "default_jump_height")]
    pub height: u8,
}

impl Default for JumpConfig {
    fn default() -> Self {
        Self {
            width: default_jump_width(),
            height: default_jump_height(),
        }
    }
}

fn default_picker_key() -> String {
    "M-s".to_string()
}
fn default_jump_key() -> String {
    "M-j".to_string()
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
fn default_picker_width() -> u8 {
    60
}
fn default_picker_height() -> u8 {
    60
}
fn default_jump_width() -> u8 {
    60
}
fn default_jump_height() -> u8 {
    40
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
