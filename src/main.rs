mod config;
mod dock;
mod hooks;
mod jump;
mod picker;
mod session;
mod stats;
mod status;
mod tmux;
#[cfg(feature = "voice")]
mod voice;
mod watcher;
mod worktree;

use anyhow::{Context, Result};
use clap::Parser;
use std::process::Command;

#[derive(Parser)]
#[command(name = "hive", about = "Tmux session manager with notifications")]
enum Cli {
    Launch,
    /// Re-read config.toml and re-apply it to the running tmux server,
    /// watcher, panel hook, and Claude hooks — without killing tmux.
    Reload,
    Pick,
    Jump,
    Stats,
    Watch,
    Stop,
    Dock,
    /// Embedded always-on dock pane (run inside a tmux split)
    Panel,
    /// Add a panel pane to the given window if it does not already have one
    /// (called by tmux hooks; target is `session:window` or `session`)
    AddPanel {
        target: String,
    },
    /// Re-apply the configured panel width to every HIVE_PANEL pane
    /// (called by the tmux client-attached hook so panels get resized once
    /// the real client geometry is known, instead of inheriting tmux's
    /// detached default-size scaling).
    ResizePanels,
    /// Focus the panel pane in the current window (toggles back via last-pane)
    FocusPanel,
    /// Close the current tmux session and switch to the next one
    CloseSession,
    /// If the given window contains only an orphan panel pane, kill the window
    /// (called by tmux hooks; target is `session:window`)
    PrunePanel {
        target: String,
    },
    /// Update the agent status for the current tmux pane (used by Claude hooks)
    Status {
        /// Status to record: working, waiting, or done
        state: String,
    },
    /// List available audio input devices (requires --features voice)
    #[cfg(feature = "voice")]
    ListDevices,
    /// Voice control daemon (requires --features voice at build time)
    #[cfg(feature = "voice")]
    Voice,
    /// Toggle the voice daemon's recording state (sends SIGUSR1)
    #[cfg(feature = "voice")]
    VoiceTrigger,
    /// Toggle always-on (VAD) listening mode (sends SIGUSR2)
    #[cfg(feature = "voice")]
    VoiceToggleAlwaysOn,
    /// Capture one VAD-bounded utterance, then auto-cut on silence
    #[cfg(feature = "voice")]
    VoiceTriggerOneshot,
}

const PID_FILE: &str = "/tmp/hive-watcher.pid";
const NOTIF_FILE: &str = "/tmp/hive-notifications.json";
// Always declared (even without the voice feature) so dock.rs compiles
// unconditionally.
pub const VOICE_RECORDING_FLAG: &str = "/tmp/hive-voice-recording";
pub const VOICE_LAST_COMMAND_FILE: &str = "/tmp/hive-voice-last";
pub const VOICE_TRANSCRIBING_FLAG: &str = "/tmp/hive-voice-transcribing";
const BOOTSTRAP_SESSION: &str = "__hive_bootstrap__";

fn ensure_tmux_server_alive() -> Result<bool> {
    let _ = Command::new("tmux").arg("start-server").status();
    if !tmux::list_sessions().is_empty() {
        return Ok(false);
    }
    if tmux::has_session(BOOTSTRAP_SESSION) {
        return Ok(true);
    }
    let tmp = std::env::temp_dir();
    let path = tmp.to_string_lossy();
    tmux::create_blank_session(BOOTSTRAP_SESSION, &path)
        .context("failed to create bootstrap tmux session")?;
    Ok(true)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli {
        Cli::Stop => return stop(),
        Cli::Status { state } => return status_cmd(state),
        #[cfg(feature = "voice")]
        Cli::ListDevices => {
            voice::capture::list_input_devices();
            return Ok(());
        }
        #[cfg(feature = "voice")]
        Cli::VoiceTrigger => return voice::trigger(),
        #[cfg(feature = "voice")]
        Cli::VoiceToggleAlwaysOn => return voice::trigger_always_on(),
        #[cfg(feature = "voice")]
        Cli::VoiceTriggerOneshot => return voice::trigger_oneshot(),
        _ => {}
    }

    let config = config::Config::load()?;

    match cli {
        Cli::Launch => launch(config),
        Cli::Reload => reload(config),
        Cli::Pick => picker::run(config),
        Cli::Jump => jump::run(),
        Cli::Stats => stats::run(),
        Cli::Watch => watcher::run(config),
        Cli::Dock => dock::run(config),
        Cli::Panel => dock::run_panel(config),
        Cli::AddPanel { target } => add_panel_cmd(config, &target),
        Cli::ResizePanels => resize_panels_cmd(config),
        Cli::FocusPanel => tmux::focus_panel_toggle(),
        Cli::CloseSession => tmux::close_current_session().map(|_| ()),
        Cli::PrunePanel { target } => tmux::prune_orphan_panel(&target),
        #[cfg(feature = "voice")]
        Cli::Voice => voice::run(config),
        Cli::Stop | Cli::Status { .. } => unreachable!(),
        #[cfg(feature = "voice")]
        Cli::ListDevices => unreachable!(),
        #[cfg(feature = "voice")]
        Cli::VoiceTrigger => unreachable!(),
        #[cfg(feature = "voice")]
        Cli::VoiceToggleAlwaysOn => unreachable!(),
        #[cfg(feature = "voice")]
        Cli::VoiceTriggerOneshot => unreachable!(),
    }
}

fn add_panel_cmd(config: config::Config, target: &str) -> Result<()> {
    if !config.panel.enabled {
        return Ok(());
    }
    let exe = std::env::current_exe().context("could not determine own binary path")?;
    tmux::ensure_panel_in_window(
        target,
        config.panel.width,
        &config.panel.position,
        &exe.to_string_lossy(),
    )
}

fn resize_panels_cmd(config: config::Config) -> Result<()> {
    if !config.panel.enabled {
        return Ok(());
    }
    let exe = std::env::current_exe().context("could not determine own binary path")?;
    tmux::ensure_panels_in_all_windows(
        config.panel.width,
        &config.panel.position,
        &exe.to_string_lossy(),
    );
    Ok(())
}

fn status_cmd(state: &str) -> Result<()> {
    if !matches!(state, "working" | "waiting" | "done" | "idle") {
        anyhow::bail!("invalid status: {state} (expected working|waiting|done|idle)");
    }
    let pane = current_pane().context("could not determine tmux pane")?;
    status::write(&pane, state)?;
    Ok(())
}

fn current_pane() -> Result<String> {
    let mut cmd = Command::new("tmux");
    cmd.arg("display-message").arg("-p");
    if let Ok(pane) = std::env::var("TMUX_PANE") {
        cmd.arg("-t").arg(pane);
    }
    cmd.arg("#S:#I");
    let out = cmd.output().context("running tmux display-message")?;
    if !out.status.success() {
        anyhow::bail!("tmux display-message failed");
    }
    let pane = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if pane.is_empty() {
        anyhow::bail!("tmux returned empty pane id");
    }
    Ok(pane)
}

/// Shared between `launch` and `reload`: re-apply everything that depends on
/// config — Claude hooks, panel hooks, panel sweep, and tmux key bindings —
/// against the running tmux server. Idempotent, so it's safe to run multiple
/// times during a single launch (e.g. once before the picker so sessions
/// created by the picker pick up the panel hook, and once after).
fn apply_config(config: &config::Config, exe_str: &str) -> Result<()> {
    if let Err(e) = hooks::ensure_installed(exe_str) {
        eprintln!("hive: could not verify claude hooks: {e:#}");
    }

    if config.panel.enabled {
        if let Err(e) = tmux::install_panel_hook(exe_str) {
            eprintln!("hive: could not install panel hook: {e:#}");
        }
        tmux::ensure_panels_in_all_windows(
            config.panel.width,
            &config.panel.position,
            exe_str,
        );
    }

    install_bindings(config, exe_str)?;

    #[cfg(feature = "voice")]
    if config.voice.enabled {
        if let Err(e) = ensure_voice_daemon(exe_str) {
            eprintln!("hive: could not start voice daemon: {e:#}");
        }
        let trigger_cmd = format!("{exe_str} voice-trigger");
        if let Err(e) = tmux::install_global_binding(&config.voice.hotkey, &trigger_cmd) {
            eprintln!("hive: could not install voice keybinding: {e:#}");
        }
        let always_on_cmd = format!("{exe_str} voice-toggle-always-on");
        if let Err(e) =
            tmux::install_global_binding(&config.voice.always_on_hotkey, &always_on_cmd)
        {
            eprintln!("hive: could not install always-on keybinding: {e:#}");
        }
        let oneshot_cmd = format!("{exe_str} voice-trigger-oneshot");
        if let Err(e) =
            tmux::install_global_binding(&config.voice.oneshot_hotkey, &oneshot_cmd)
        {
            eprintln!("hive: could not install oneshot keybinding: {e:#}");
        }
    }

    Ok(())
}

#[cfg(feature = "voice")]
fn ensure_voice_daemon(exe: &str) -> Result<()> {
    if let Ok(pid_str) = std::fs::read_to_string(voice::PID_FILE) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            // signal 0 = liveness check
            if unsafe { libc::kill(pid, 0) } == 0 {
                return Ok(());
            }
        }
        let _ = std::fs::remove_file(voice::PID_FILE);
    }
    Command::new(exe)
        .arg("voice")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn voice daemon")?;
    Ok(())
}

fn launch(config: config::Config) -> Result<()> {
    let exe = std::env::current_exe().context("could not determine own binary path")?;
    let exe_str = exe.to_string_lossy().into_owned();

    let bootstrap_created = ensure_tmux_server_alive()?;

    // Tell tmux that detached sessions should assume the size of the real
    // terminal hive was launched from. Otherwise splits performed before the
    // client attaches inherit the 80x24 default-size and get proportionally
    // rescaled on attach, which is what makes the panel pane appear absurdly
    // wide on first launch.
    tmux::sync_default_size_to_terminal();

    spawn_watcher(&exe)?;

    // Install hooks/bindings up front so sessions and windows created during
    // the picker (e.g. via `create_project_session`) pick up the panel hook.
    apply_config(&config, &exe_str)?;

    let initial_session = picker::run_and_return(&config)?;

    let Some(name) = initial_session else {
        if bootstrap_created {
            let _ = tmux::kill_session(BOOTSTRAP_SESSION);
        }
        return Ok(());
    };

    if bootstrap_created && name != BOOTSTRAP_SESSION {
        let _ = tmux::kill_session(BOOTSTRAP_SESSION);
    }

    // Re-run the same flow `hive reload` uses now that the picker has had a
    // chance to create new sessions. The panel sizing here is still wrong
    // because we're detached — the `client-attached` / `client-resized` hooks
    // installed by `install_panel_hook` re-run `resize-panels` once the real
    // client geometry kicks in.
    apply_config(&config, &exe_str)?;

    let status = Command::new("tmux")
        .args(["new-session", "-A", "-s", &name])
        .status()
        .context("failed to start tmux")?;

    if !status.success() {
        anyhow::bail!("tmux exited with {}", status);
    }

    Ok(())
}

fn reload(config: config::Config) -> Result<()> {
    let exe = std::env::current_exe().context("could not determine own binary path")?;
    let exe_str = exe.to_string_lossy().into_owned();

    let _ = Command::new("tmux").arg("start-server").status();

    // Restart watcher so notifications.* / projects changes take effect.
    stop_watcher_quiet();
    spawn_watcher(&exe)?;

    apply_config(&config, &exe_str)?;

    println!("Reloaded hive config.");
    Ok(())
}

fn spawn_watcher(exe: &std::path::Path) -> Result<()> {
    Command::new(exe)
        .arg("watch")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn watcher")?;
    Ok(())
}

/// Best-effort: kill the running watcher (if any) and wait briefly for it to
/// release the PID file so the replacement starts cleanly.
fn stop_watcher_quiet() {
    let Ok(pid_str) = std::fs::read_to_string(PID_FILE) else {
        return;
    };
    let Ok(pid) = pid_str.trim().parse::<i32>() else {
        let _ = std::fs::remove_file(PID_FILE);
        return;
    };
    unsafe { libc::kill(pid, libc::SIGTERM) };
    for _ in 0..20 {
        if !std::path::Path::new(PID_FILE).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(PID_FILE);
}

/// Install the four global tmux key bindings (picker / jump / stats / dock).
/// `bind-key -n` is server-global, so re-running this overwrites the old bindings
/// in place — that's how `hive reload` picks up keybinding and popup-size changes.
fn install_bindings(config: &config::Config, exe_str: &str) -> Result<()> {
    let pick_cmd = format!(
        "tmux display-popup -E -w {}% -h {}% '{}' pick",
        config.picker.width, config.picker.height, exe_str
    );
    let jump_cmd = format!(
        "if [ -s {notif} ] && [ \"$(cat {notif})\" != '[]' ]; then tmux display-popup -E -w {jw}% -h {jh}% '{exe}' jump; fi",
        notif = crate::NOTIF_FILE,
        exe = exe_str,
        jw = config.jump.width,
        jh = config.jump.height,
    );
    let stats_cmd = format!(
        "tmux display-popup -E -w {}% -h {}% '{}' stats",
        config.stats.width, config.stats.height, exe_str
    );
    let dock_cmd = if config.panel.enabled {
        // The binding wraps this string in `run-shell -b`, so it must be a
        // plain shell command — not another `run-shell -b …`, which would
        // make /bin/sh try to execute `run-shell` and fail with 127.
        format!("{exe_str} focus-panel")
    } else {
        let dock_pos = match config.dock.position.as_str() {
            "left" => " -x 0",
            "right" => " -x R",
            _ => "",
        };
        format!(
            "tmux display-popup -E -w {}% -h {}%{} '{}' dock",
            config.dock.width, config.dock.height, dock_pos, exe_str
        )
    };

    let close_cmd = format!("{exe_str} close-session");

    tmux::install_global_binding(&config.keybindings.picker, &pick_cmd)?;
    tmux::install_global_binding(&config.keybindings.jump, &jump_cmd)?;
    tmux::install_global_binding(&config.keybindings.stats, &stats_cmd)?;
    tmux::install_global_binding(&config.keybindings.dock, &dock_cmd)?;
    tmux::install_global_binding(&config.keybindings.close, &close_cmd)?;
    Ok(())
}

fn stop() -> Result<()> {
    let pid_str =
        std::fs::read_to_string(PID_FILE).context("watcher is not running (no PID file found)")?;
    let pid: i32 = pid_str.trim().parse().context("invalid PID file")?;

    let ret = unsafe { libc::kill(pid, libc::SIGTERM) };
    if ret != 0 {
        let _ = std::fs::remove_file(PID_FILE);
        anyhow::bail!("failed to kill watcher (pid {})", pid);
    }

    let _ = std::fs::remove_file(PID_FILE);
    println!("Stopped watcher (pid {})", pid);

    #[cfg(feature = "voice")]
    {
        if let Ok(vpid_str) = std::fs::read_to_string(voice::PID_FILE) {
            if let Ok(vpid) = vpid_str.trim().parse::<i32>() {
                let _ = unsafe { libc::kill(vpid, libc::SIGTERM) };
                println!("Stopped voice daemon (pid {vpid})");
            }
            let _ = std::fs::remove_file(voice::PID_FILE);
        }
    }

    Ok(())
}
