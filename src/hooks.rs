use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::PathBuf;

const HIVE_HOOK_MARKER: &str = "hive status ";

fn settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

fn hook_command(exe: &str, status: &str) -> String {
    format!("{exe} status {status}")
}

fn make_hook_entry(exe: &str, status: &str, matcher: Option<&str>) -> Value {
    let mut entry = serde_json::Map::new();
    if let Some(m) = matcher {
        entry.insert("matcher".to_string(), Value::String(m.to_string()));
    }
    entry.insert(
        "hooks".to_string(),
        json!([{ "type": "command", "command": hook_command(exe, status) }]),
    );
    Value::Object(entry)
}

/// Returns true if a hive entry exists for this event. If `desired_cmd` is set,
/// rewrites any matching command in-place so an older `hive status …` entry gets
/// upgraded to the absolute-path form.
fn event_already_installed(event_arr: &mut Value, desired_cmd: Option<&str>) -> bool {
    let Some(arr) = event_arr.as_array_mut() else {
        return false;
    };
    let mut found = false;
    for entry in arr {
        let Some(hooks) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
            continue;
        };
        for h in hooks {
            if let Some(cmd_val) = h.get_mut("command") {
                if let Some(cmd) = cmd_val.as_str() {
                    if cmd.contains(HIVE_HOOK_MARKER) {
                        found = true;
                        if let Some(want) = desired_cmd {
                            if cmd != want {
                                *cmd_val = Value::String(want.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    found
}

pub fn ensure_installed(exe: &str) -> Result<()> {
    let Some(path) = settings_path() else {
        return Ok(());
    };

    let mut settings: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content)
                .with_context(|| format!("parsing {}", path.display()))?
        }
    } else {
        json!({})
    };

    let events: [(&str, &str, Option<&str>); 5] = [
        ("PostToolUse", "working", None),
        ("Stop", "done", None),
        (
            "Notification",
            "waiting",
            Some("permission_prompt|elicitation_dialog"),
        ),
        ("UserPromptSubmit", "working", None),
        ("SessionStart", "idle", None),
    ];

    let root = settings
        .as_object_mut()
        .context("~/.claude/settings.json root is not an object")?;
    let hooks_obj = root
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));
    let hooks_map = hooks_obj
        .as_object_mut()
        .context("~/.claude/settings.json `hooks` is not an object")?;

    let mut added = Vec::new();
    let mut changed = false;
    for (event, status, matcher) in events {
        let event_entry = hooks_map
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        let desired = hook_command(exe, status);
        let before = serde_json::to_string(event_entry).unwrap_or_default();
        if event_already_installed(event_entry, Some(&desired)) {
            let after = serde_json::to_string(event_entry).unwrap_or_default();
            if before != after {
                changed = true;
            }
            continue;
        }
        if let Some(arr) = event_entry.as_array_mut() {
            arr.push(make_hook_entry(exe, status, matcher));
            added.push(event);
            changed = true;
        }
    }

    if changed {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        if added.is_empty() {
            eprintln!(
                "hive: updated claude notification hook commands in {}",
                path.display()
            );
        } else {
            eprintln!(
                "hive: installed claude notification hooks ({}) in {}",
                added.join(", "),
                path.display()
            );
        }
    }

    Ok(())
}
