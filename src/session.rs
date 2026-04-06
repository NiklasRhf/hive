use crate::config::Config;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    Stopped,
    Worktree,
}

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub name: String,
    pub status: Status,
}

impl SessionEntry {
    pub fn icon(&self) -> &'static str {
        match self.status {
            Status::Running => "●",
            Status::Stopped => "○",
            Status::Worktree => "⎇",
        }
    }
}

pub fn discover(config: &Config) -> Vec<SessionEntry> {
    let mut names = BTreeSet::new();

    for p in &config.projects {
        names.insert(p.name.clone());
    }

    for s in crate::tmux::list_sessions() {
        names.insert(s);
    }

    for p in &config.projects {
        if let Some(wt) = &p.worktree {
            let base = PathBuf::from(&wt.base);
            if !base.is_dir() {
                continue;
            }
            let prefix = format!("{}-", p.name);
            if let Ok(entries) = std::fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with(&prefix) && entry.path().is_dir() {
                        names.insert(name);
                    }
                }
            }
        }
    }

    names
        .into_iter()
        .map(|name| {
            let status = if crate::tmux::has_session(&name) {
                Status::Running
            } else {
                let parent = name.split('-').next().unwrap_or(&name);
                if parent != name
                    && config
                        .find_project(parent)
                        .is_some_and(|p| p.worktree.is_some())
                {
                    Status::Worktree
                } else {
                    Status::Stopped
                }
            };
            SessionEntry { name, status }
        })
        .collect()
}

pub fn resolve_path(name: &str, config: &Config) -> Option<PathBuf> {
    if let Some(p) = config.find_project(name) {
        return Some(PathBuf::from(&p.path));
    }
    let parent = name.split('-').next()?;
    let project = config.find_project(parent)?;
    let wt = project.worktree.as_ref()?;
    let wt_path = PathBuf::from(&wt.base).join(name);
    wt_path.is_dir().then_some(wt_path)
}

pub fn resolve_cmd(name: &str, config: &Config) -> String {
    if let Some(p) = config.find_project(name) {
        if let Some(cmd) = &p.cmd {
            return cmd.clone();
        }
    }
    let parent = name.split('-').next().unwrap_or(name);
    if let Some(p) = config.find_project(parent) {
        if let Some(cmd) = &p.cmd {
            return cmd.clone();
        }
    }
    "git status".to_string()
}
