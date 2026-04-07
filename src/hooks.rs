use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::PathBuf;

const HIVE_HOOK_MARKER: &str = "hive status ";

fn settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

fn hook_command(status: &str) -> String {
    format!("hive status {status}")
}

fn make_hook_entry(status: &str, matcher: Option<&str>) -> Value {
    let mut entry = serde_json::Map::new();
    if let Some(m) = matcher {
        entry.insert("matcher".to_string(), Value::String(m.to_string()));
    }
    entry.insert(
        "hooks".to_string(),
        json!([{ "type": "command", "command": hook_command(status) }]),
    );
    Value::Object(entry)
}

fn event_already_installed(event_arr: &Value) -> bool {
    let Some(arr) = event_arr.as_array() else {
        return false;
    };
    for entry in arr {
        let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) else {
            continue;
        };
        for h in hooks {
            if let Some(cmd) = h.get("command").and_then(|c| c.as_str()) {
                if cmd.contains(HIVE_HOOK_MARKER) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn ensure_installed() -> Result<()> {
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

    let events: [(&str, &str, Option<&str>); 4] = [
        ("PostToolUse", "working", None),
        ("Stop", "done", None),
        (
            "Notification",
            "waiting",
            Some("permission_prompt|elicitation_dialog"),
        ),
        ("UserPromptSubmit", "working", None),
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
    for (event, status, matcher) in events {
        let event_entry = hooks_map
            .entry(event.to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if event_already_installed(event_entry) {
            continue;
        }
        if let Some(arr) = event_entry.as_array_mut() {
            arr.push(make_hook_entry(status, matcher));
            added.push(event);
        }
    }

    if !added.is_empty() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        eprintln!(
            "hive: installed claude notification hooks ({}) in {}",
            added.join(", "),
            path.display()
        );
    }

    Ok(())
}
