use crate::config::Config;
use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub mod capture;
pub mod dispatch;
pub mod hold;
pub mod intent;
pub mod stt;

pub const PID_FILE: &str = "/tmp/hive-voice.pid";
const CMD_FILE: &str = "/tmp/hive-voice-cmd";

const COMMAND_VOCAB: &str = "picker, dock, stats, notifications, choose, select, option, send, cancel, clear, close, open, jump to, go to, next, previous, agent, tell agent";

const MIN_PCM_SAMPLES: usize = 16000 / 4;

static TRIGGER: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigusr1(_: libc::c_int) {
    TRIGGER.store(true, Ordering::SeqCst);
}

extern "C" fn on_sigterm(_: libc::c_int) {
    let _ = std::fs::remove_file(PID_FILE);
    let _ = std::fs::remove_file(crate::VOICE_RECORDING_FLAG);
    let _ = std::fs::remove_file(crate::VOICE_TRANSCRIBING_FLAG);
    std::process::exit(0);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGUSR1, on_sigusr1 as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_sigterm as *const () as libc::sighandler_t);
    }
}

fn check_take() -> bool {
    TRIGGER.swap(false, Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoiceMode {
    PttToggle,
    OneShot,
    AlwaysOn,
}

fn read_cmd() -> VoiceMode {
    match std::fs::read_to_string(CMD_FILE)
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("oneshot") => VoiceMode::OneShot,
        Some("always-on") => VoiceMode::AlwaysOn,
        _ => VoiceMode::PttToggle,
    }
}

struct StateFlag(&'static str);

impl StateFlag {
    fn set(path: &'static str) -> Self {
        let _ = std::fs::write(path, "");
        Self(path)
    }
}

impl Drop for StateFlag {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

pub fn run(config: Config) -> Result<()> {
    if !config.voice.enabled {
        eprintln!("hive voice: disabled in config ([voice] enabled = false)");
        return Ok(());
    }

    install_signal_handlers();
    std::fs::write(PID_FILE, std::process::id().to_string())
        .with_context(|| format!("writing {PID_FILE}"))?;

    eprintln!("hive voice: loading whisper model {}", config.voice.model);
    let stt = stt::Stt::load(&config.voice.model, &config.voice.language)
        .context("failed to load whisper model")?;

    if let Some(ref name) = config.voice.output_device {
        eprintln!("hive voice: setting output device matching {name:?}");
        if let Err(e) = capture::set_pipewire_default_sink(name) {
            eprintln!("hive voice: failed to set output device: {e:#}");
        }
    }

    let input_device = config.voice.input_device.as_deref();
    if let Some(name) = input_device {
        eprintln!("hive voice: using input device matching {name:?}");
    }
    let recorder =
        capture::Recorder::open(input_device).context("failed to open audio input device")?;

    if config.voice.hold_to_talk {
        hold::spawn(&config.voice.hold_key);
    }

    eprintln!("hive voice: ready (PID {})", std::process::id());

    let model_name = std::path::Path::new(&config.voice.model)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.strip_prefix("ggml-").unwrap_or(s))
        .unwrap_or("whisper");
    write_last_command(&format!("{model_name} on {}", current_backend()));

    loop {
        if check_take() {
            match read_cmd() {
                VoiceMode::PttToggle => run_ptt(&config, &stt, &recorder),
                VoiceMode::OneShot => run_oneshot(&config, &stt, &recorder),
                VoiceMode::AlwaysOn => run_always_on(&config, &stt, &recorder),
            }
            continue;
        }
        if hold::HOLD_PRESSED.load(Ordering::SeqCst) {
            run_hold(&config, &stt, &recorder);
            continue;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn drain_while(rec: &capture::Recorder, cont: impl Fn() -> bool) -> Option<Vec<f32>> {
    rec.clear();
    while cont() {
        std::thread::sleep(Duration::from_millis(10));
    }
    Some(capture::resample_to_16k(&rec.drain(), rec.sample_rate))
}

fn capture_session<F>(config: &Config, stt: &stt::Stt, capture: F)
where
    F: FnOnce() -> Option<Vec<f32>>,
{
    let target = capture_focused_pane_id();
    let pcm = {
        let _flag = StateFlag::set(crate::VOICE_RECORDING_FLAG);
        capture()
    };
    match pcm {
        Some(p) if p.len() >= MIN_PCM_SAMPLES => {
            transcribe_and_dispatch(config, stt, &p, target.as_deref())
        }
        Some(_) => write_last_command("too short"),
        None => write_last_command("aborted"),
    }
}

fn run_ptt(config: &Config, stt: &stt::Stt, rec: &capture::Recorder) {
    capture_session(config, stt, || drain_while(rec, || !check_take()));
}

fn run_oneshot(config: &Config, stt: &stt::Stt, rec: &capture::Recorder) {
    let params = vad_params(config);
    capture_session(config, stt, || {
        capture::record_vad_segment(rec, &params, || false).unwrap_or(None)
    });
}

fn run_hold(config: &Config, stt: &stt::Stt, rec: &capture::Recorder) {
    capture_session(config, stt, || {
        drain_while(rec, || hold::HOLD_PRESSED.load(Ordering::SeqCst))
    });
}

// Bypasses capture_session: it would otherwise toggle the recording flag on
// every utterance and make the dot blink. Set once for the whole engagement.
fn run_always_on(config: &Config, stt: &stt::Stt, rec: &capture::Recorder) {
    let _recording = StateFlag::set(crate::VOICE_RECORDING_FLAG);
    let params = vad_params(config);
    eprintln!("hive voice: always-on engaged");
    loop {
        if check_take() {
            break;
        }
        let target = capture_focused_pane_id();
        match capture::record_vad_segment(rec, &params, || TRIGGER.load(Ordering::SeqCst)) {
            Ok(Some(pcm)) if pcm.len() >= MIN_PCM_SAMPLES => {
                transcribe_and_dispatch(config, stt, &pcm, target.as_deref());
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                check_take();
                break;
            }
            Err(e) => {
                eprintln!("hive voice: vad capture error: {e:#}");
                write_last_command("capture failed");
                break;
            }
        }
    }
    eprintln!("hive voice: always-on disengaged");
}

fn vad_params(config: &Config) -> capture::VadParams {
    capture::VadParams {
        threshold: config.voice.vad_threshold,
        silence_ms: config.voice.vad_silence_ms,
        min_speech_ms: config.voice.vad_min_speech_ms,
    }
}

fn transcribe_and_dispatch(
    config: &Config,
    stt: &stt::Stt,
    pcm: &[f32],
    dictation_target: Option<&str>,
) {
    let _flag = StateFlag::set(crate::VOICE_TRANSCRIBING_FLAG);
    let sessions: Vec<String> = crate::session::discover(config)
        .into_iter()
        .map(|e| e.name)
        .collect();
    let aliases = build_alias_map(&sessions, config);
    let prompt = build_initial_prompt(&sessions, &aliases, &config.voice.aliases);
    let text = match stt.transcribe(pcm, &prompt) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("hive voice: transcribe error: {e:#}");
            write_last_command("transcribe failed");
            return;
        }
    };
    eprintln!("hive voice: heard {text:?}");
    let Some(intent) = intent::parse(&text, &sessions, &aliases, &config.voice.aliases) else {
        eprintln!("hive voice: no intent matched: {text:?}");
        write_last_command(&format!("?: {}", text.trim()));
        return;
    };
    let label = intent.label();
    if let Err(e) = dispatch::dispatch(intent, config, dictation_target) {
        eprintln!("hive voice: dispatch error: {e:#}");
        write_last_command("dispatch failed");
    } else if let Some(label) = label {
        write_last_command(&label);
    }
}

use std::collections::HashMap;

fn build_alias_map(sessions: &[String], config: &Config) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for project in &config.projects {
        for alias in &project.voice {
            map.entry(alias.to_lowercase())
                .or_insert_with(|| project.name.clone());
        }
    }
    for session in sessions {
        for project in &config.projects {
            let Some(branch) = session
                .strip_prefix(&project.name)
                .and_then(|rest| rest.strip_prefix('-'))
            else {
                continue;
            };
            if branch.is_empty() {
                break;
            }
            let spoken = branch.replace('-', " ");
            map.entry(spoken).or_insert_with(|| session.clone());
            if branch.contains('-') {
                map.entry(branch.to_string())
                    .or_insert_with(|| session.clone());
            }
            if let Some(prefix) = project
                .worktree
                .as_ref()
                .and_then(|wt| wt.prefix.as_deref())
            {
                if let Some(tail) = branch.strip_prefix(prefix) {
                    if !tail.is_empty() {
                        let short = tail.replace('-', " ");
                        map.entry(short).or_insert_with(|| session.clone());
                        if tail.contains('-') {
                            map.entry(tail.to_string())
                                .or_insert_with(|| session.clone());
                        }
                    }
                }
            }
            break;
        }
    }
    map
}

fn build_initial_prompt(
    sessions: &[String],
    aliases: &HashMap<String, String>,
    cfg: &crate::config::VoiceAliases,
) -> String {
    let mut tokens: Vec<&str> = Vec::new();
    for s in sessions {
        tokens.push(s);
    }
    for alias in aliases.keys() {
        tokens.push(alias);
    }
    let all_custom = [
        &cfg.picker, &cfg.dock, &cfg.stats, &cfg.notifications,
        &cfg.send, &cfg.cancel, &cfg.clear, &cfg.close,
        &cfg.next_agent, &cfg.previous_agent, &cfg.next_session,
        &cfg.previous_session, &cfg.dictate, &cfg.jump,
        &cfg.choose, &cfg.open, &cfg.close_named, &cfg.btw,
    ];
    for list in all_custom {
        for s in list {
            tokens.push(s);
        }
    }
    if tokens.is_empty() {
        COMMAND_VOCAB.to_string()
    } else {
        format!("{COMMAND_VOCAB}, {}", tokens.join(", "))
    }
}

fn signal_with_cmd(cmd: &str) -> Result<()> {
    let pid_str = std::fs::read_to_string(PID_FILE)
        .context("voice daemon is not running (no PID file)")?;
    let pid: i32 = pid_str.trim().parse().context("invalid voice PID file")?;
    std::fs::write(CMD_FILE, cmd).with_context(|| format!("writing {CMD_FILE}"))?;
    let ret = unsafe { libc::kill(pid, libc::SIGUSR1) };
    if ret != 0 {
        let _ = std::fs::remove_file(PID_FILE);
        anyhow::bail!("failed to signal voice daemon (pid {pid})");
    }
    Ok(())
}

pub fn trigger() -> Result<()> {
    signal_with_cmd("ptt")
}

pub fn trigger_always_on() -> Result<()> {
    signal_with_cmd("always-on")
}

pub fn trigger_oneshot() -> Result<()> {
    signal_with_cmd("oneshot")
}

fn capture_focused_pane_id() -> Option<String> {
    let out = std::process::Command::new("tmux")
        .args(["display-message", "-p", "#{pane_id}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn write_last_command(label: &str) {
    let _ = std::fs::write(crate::VOICE_LAST_COMMAND_FILE, label);
}

fn current_backend() -> &'static str {
    if cfg!(feature = "voice-cuda") {
        "cuda"
    } else if cfg!(feature = "voice-vulkan") {
        "vulkan"
    } else if cfg!(feature = "voice-hipblas") {
        "hipblas"
    } else if cfg!(feature = "voice-metal") {
        "metal"
    } else if cfg!(feature = "voice-coreml") {
        "coreml"
    } else if cfg!(feature = "voice-openblas") {
        "cpu+openblas"
    } else {
        "cpu"
    }
}
