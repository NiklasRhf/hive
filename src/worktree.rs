use crate::config::ProjectConfig;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::Command;

fn git(repo: &str, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .context("failed to run git")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn detect_base_branch(repo: &str) -> String {
    git(repo, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        .ok()
        .and_then(|s| s.strip_prefix("refs/remotes/origin/").map(String::from))
        .unwrap_or_else(|| "main".to_string())
}

pub struct Progress {
    pub ratio: f64,
}

pub fn create(
    project: &ProjectConfig,
    branch: &str,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf> {
    let wt = project
        .worktree
        .as_ref()
        .context("project has no worktree config")?;
    let session_name = format!("{}-{branch}", project.name);
    let wt_path = PathBuf::from(&wt.base).join(&session_name);

    if wt_path.is_dir() {
        return Ok(wt_path);
    }

    let total_steps = 2 + wt.copy_dirs.len() + wt.copy_files.len();
    let mut step = 0;

    let mut advance = || {
        step += 1;
        on_progress(Progress {
            ratio: step as f64 / total_steps as f64,
        });
    };

    let source = &project.path;
    let base_branch = detect_base_branch(source);

    advance();
    let _ = git(source, &["fetch", "origin", &base_branch]);

    advance();
    let origin_ref = format!("origin/{base_branch}");
    let wt_str = wt_path.to_string_lossy();

    let ok = Command::new("git")
        .args([
            "-C",
            source,
            "worktree",
            "add",
            &wt_str,
            "-b",
            branch,
            &origin_ref,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if !ok.is_ok_and(|s| s.success()) {
        let status = Command::new("git")
            .args(["-C", source, "worktree", "add", &wt_str, branch])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("git worktree add failed")?;
        if !status.success() {
            bail!("failed to create worktree at {}", wt_path.display());
        }
    }

    for dir in &wt.copy_dirs {
        advance();
        let src = PathBuf::from(source).join(dir);
        let dst = wt_path.join(dir);
        if src.is_dir() {
            copy_dir_recursive(&src, &dst).with_context(|| format!("copying dir {dir}"))?;
        }
    }

    for file in &wt.copy_files {
        advance();
        let src = PathBuf::from(source).join(file);
        let dst = wt_path.join(file);
        if src.is_file() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst).with_context(|| format!("copying file {file}"))?;
        }
    }

    Ok(wt_path)
}

pub fn remove(project: &ProjectConfig, session_name: &str) -> Result<()> {
    let wt = project
        .worktree
        .as_ref()
        .context("project has no worktree config")?;
    let wt_path = PathBuf::from(&wt.base).join(session_name);

    if !wt_path.is_dir() {
        return Ok(());
    }

    let wt_str = wt_path.to_string_lossy();
    let ok = Command::new("git")
        .args([
            "-C",
            &project.path,
            "worktree",
            "remove",
            "--force",
            &wt_str,
        ])
        .status();

    if !ok.is_ok_and(|s| s.success()) {
        std::fs::remove_dir_all(&wt_path)
            .with_context(|| format!("removing worktree dir {}", wt_path.display()))?;
    }

    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
