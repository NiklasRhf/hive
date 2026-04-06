mod config;
mod jump;
mod picker;
mod session;
mod stats;
mod tmux;
mod watcher;
mod worktree;

use anyhow::{Context, Result};
use clap::Parser;
use std::process::Command;

#[derive(Parser)]
#[command(name = "hive", about = "Tmux session manager with notifications")]
enum Cli {
    Launch,
    Pick,
    Jump,
    Stats,
    Watch,
    Stop,
}

const PID_FILE: &str = "/tmp/hive-watcher.pid";
const NOTIF_FILE: &str = "/tmp/hive-notifications.json";

fn main() -> Result<()> {
    let cli = Cli::parse();

    if matches!(cli, Cli::Stop) {
        return stop();
    }

    let config = config::Config::load()?;

    match cli {
        Cli::Launch => launch(config),
        Cli::Pick => picker::run(config),
        Cli::Jump => jump::run(),
        Cli::Stats => stats::run(),
        Cli::Watch => watcher::run(config),
        Cli::Stop => unreachable!(),
    }
}

fn launch(config: config::Config) -> Result<()> {
    let exe = std::env::current_exe().context("could not determine own binary path")?;
    let exe_str = exe.to_string_lossy();

    Command::new(&exe)
        .arg("watch")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn watcher")?;

    let initial_session = picker::run_and_return(&config)?;

    let Some(name) = initial_session else {
        return Ok(());
    };

    let pick_cmd = format!(
        "display-popup -E -w {}% -h {}% '{}' pick",
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
        "display-popup -E -w {}% -h {}% '{}' stats",
        config.stats.width, config.stats.height, exe_str
    );

    let status = Command::new("tmux")
        .args([
            "new-session",
            "-A",
            "-s",
            &name,
            ";",
            "bind-key",
            "-n",
            &config.keybindings.picker,
            "run-shell",
            "-b",
            &format!("tmux {pick_cmd}"),
            ";",
            "bind-key",
            "-n",
            &config.keybindings.jump,
            "run-shell",
            "-b",
            &jump_cmd,
            ";",
            "bind-key",
            "-n",
            &config.keybindings.stats,
            "run-shell",
            "-b",
            &format!("tmux {stats_cmd}"),
        ])
        .status()
        .context("failed to start tmux")?;

    if !status.success() {
        anyhow::bail!("tmux exited with {}", status);
    }

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
    Ok(())
}
