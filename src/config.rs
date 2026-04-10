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
    #[serde(default)]
    pub dock: DockConfig,
    #[serde(default)]
    pub panel: PanelConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub sound: SoundConfig,
    #[serde(default, rename = "project")]
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)] // fields are only read when built with --features voice
pub struct VoiceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_voice_model")]
    pub model: String,
    #[serde(default)]
    pub input_device: Option<String>,
    #[serde(default)]
    pub output_device: Option<String>,
    #[serde(default = "default_voice_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_voice_language")]
    pub language: String,
    #[serde(default)]
    pub vim_mode: bool,
    #[serde(default = "default_voice_always_on_hotkey")]
    pub always_on_hotkey: String,
    #[serde(default = "default_voice_oneshot_hotkey")]
    pub oneshot_hotkey: String,
    #[serde(default = "default_voice_hold_enabled")]
    pub hold_to_talk: bool,
    #[serde(default = "default_voice_hold_key")]
    pub hold_key: String,
    #[serde(default = "default_vad_threshold")]
    pub vad_threshold: f32,
    #[serde(default = "default_vad_silence_ms")]
    pub vad_silence_ms: u32,
    #[serde(default = "default_vad_min_speech_ms")]
    pub vad_min_speech_ms: u32,
    #[serde(default)]
    pub aliases: VoiceAliases,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_voice_model(),
            input_device: None,
            output_device: None,
            hotkey: default_voice_hotkey(),
            language: default_voice_language(),
            vim_mode: false,
            always_on_hotkey: default_voice_always_on_hotkey(),
            oneshot_hotkey: default_voice_oneshot_hotkey(),
            hold_to_talk: default_voice_hold_enabled(),
            hold_key: default_voice_hold_key(),
            vad_threshold: default_vad_threshold(),
            vad_silence_ms: default_vad_silence_ms(),
            vad_min_speech_ms: default_vad_min_speech_ms(),
            aliases: VoiceAliases::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct VoiceAliases {
    #[serde(default)]
    pub picker: Vec<String>,
    #[serde(default)]
    pub dock: Vec<String>,
    #[serde(default)]
    pub stats: Vec<String>,
    #[serde(default)]
    pub notifications: Vec<String>,
    #[serde(default)]
    pub send: Vec<String>,
    #[serde(default)]
    pub cancel: Vec<String>,
    #[serde(default)]
    pub clear: Vec<String>,
    #[serde(default)]
    pub close: Vec<String>,
    #[serde(default)]
    pub next_agent: Vec<String>,
    #[serde(default)]
    pub previous_agent: Vec<String>,
    #[serde(default)]
    pub next_session: Vec<String>,
    #[serde(default)]
    pub previous_session: Vec<String>,
    #[serde(default)]
    pub dictate: Vec<String>,
    #[serde(default)]
    pub jump: Vec<String>,
    #[serde(default)]
    pub choose: Vec<String>,
    #[serde(default)]
    pub open: Vec<String>,
    #[serde(default)]
    pub close_named: Vec<String>,
    #[serde(default)]
    pub btw: Vec<String>,
}

fn default_voice_model() -> String {
    "~/.cache/whisper/ggml-base.en.bin".to_string()
}
fn default_voice_hotkey() -> String {
    "M-v".to_string()
}
fn default_voice_language() -> String {
    "en".to_string()
}
fn default_voice_always_on_hotkey() -> String {
    "M-V".to_string()
}
fn default_voice_oneshot_hotkey() -> String {
    "M-b".to_string()
}
fn default_voice_hold_enabled() -> bool {
    true
}
fn default_voice_hold_key() -> String {
    "F1".to_string()
}
fn default_vad_threshold() -> f32 {
    0.012
}
fn default_vad_silence_ms() -> u32 {
    700
}
fn default_vad_min_speech_ms() -> u32 {
    300
}

#[derive(Debug, Deserialize, Clone)]
pub struct SoundConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub waiting: Option<String>,
    #[serde(default)]
    pub done: Option<String>,
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            waiting: None,
            done: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PanelConfig {
    #[serde(default = "default_panel_enabled")]
    pub enabled: bool,
    #[serde(default = "default_panel_width")]
    pub width: u16,
    #[serde(default = "default_panel_position")]
    pub position: String,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            enabled: default_panel_enabled(),
            width: default_panel_width(),
            position: default_panel_position(),
        }
    }
}

fn default_panel_enabled() -> bool {
    true
}
fn default_panel_width() -> u16 {
    40
}
fn default_panel_position() -> String {
    "right".to_string()
}

#[derive(Debug, Deserialize)]
pub struct DockConfig {
    #[serde(default = "default_dock_width")]
    pub width: u8,
    #[serde(default = "default_dock_height")]
    pub height: u8,
    #[serde(default = "default_dock_position")]
    pub position: String,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            width: default_dock_width(),
            height: default_dock_height(),
            position: default_dock_position(),
        }
    }
}

fn default_dock_width() -> u8 {
    20
}
fn default_dock_height() -> u8 {
    70
}
fn default_dock_position() -> String {
    "right".to_string()
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
    #[serde(default)]
    pub legacy: bool,
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
            legacy: false,
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
    #[serde(default = "default_dock_key")]
    pub dock: String,
    #[serde(default = "default_close_key")]
    pub close: String,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            picker: default_picker_key(),
            jump: default_jump_key(),
            stats: default_stats_key(),
            dock: default_dock_key(),
            close: default_close_key(),
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
fn default_dock_key() -> String {
    "M-d".to_string()
}
fn default_close_key() -> String {
    "M-q".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub path: String,
    pub cmd: Option<String>,
    pub worktree: Option<WorktreeConfig>,
    #[serde(default)]
    pub voice: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorktreeConfig {
    pub base: String,
    #[serde(default)]
    pub prefix: Option<String>,
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

        config.voice.model = expand_tilde(&config.voice.model)
            .to_string_lossy()
            .into_owned();

        if let Some(ref mut path) = config.sound.waiting {
            *path = expand_tilde(path).to_string_lossy().into_owned();
        }
        if let Some(ref mut path) = config.sound.done {
            *path = expand_tilde(path).to_string_lossy().into_owned();
        }

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
