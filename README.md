# hive

A tmux session manager with notification overlay for Claude Code instances.

Think of it as a hive: you (the developer) are the hive mind, and each Claude Code instance is a worker. Workers go off and do their thing in their own tmux window, then communicate back via notifications when they finish a task or need your attention.

## Why another one of these?

Several tools cover the same problem space — running multiple Claude Code agents in parallel, each in its own isolated environment, with some way to know when they need you:

- **[workmux](https://github.com/raine/workmux)** — terminal-only, git worktrees + tmux windows, supports multiple agents (claude, gemini, codex, etc.)
- **[cmux](https://github.com/manaflow-ai/cmux)** — native macOS GUI built on Ghostty, with vertical tabs, split panes, an embedded browser and a socket API

Both solve roughly the same thing hive does. I built hive because I wanted something that fits the way I actually work:

- **tmux sessions + windows, not panes.** Panes split the screen and force you to look at multiple things at once. I keep more mental clarity by seeing one thing at a time, in full, and moving between windows.
- **Jump-driven, not layout-driven.** Move between sessions via the fuzzy picker, jump straight to a worker from a notification, or bounce back and forth with `tmux switch-client -l`. The notification overlay tells me *which* worker needs attention; I jump there directly instead of scanning a sidebar.
- **Lightweight and Linux/X11-native.** No GUI app, no Mac dependency — just a small Rust binary, tmux, and a click-through X11 overlay window.

Manages multiple Claude Code workers from one place — session creation, git worktrees, fuzzy picker, and non-intrusive X11 notifications when instances finish or need input.

## Features

- **Session picker** with fuzzy search (nucleo) inside a tmux popup
- **Git worktree** creation and cleanup per project
- **Notification overlay** — click-through X11 window positioned in the top-right of your terminal
- **Notification jump list** — tmux popup showing all pending notifications; press Enter to jump straight to the worker that needs you
- **Idle detection** — detects when Claude Code finishes a task
- **Notification persistence** — notifications stay until you visit the tmux window or delete them manually

## Install

```bash
cargo install --path .
```

### Dependencies

- tmux (>= 3.2 for display-popup)
- libcairo (for notification text rendering)
- X11

## Usage

### Start everything

```bash
hive launch
```

This:
1. Spawns the notification overlay in the background
2. Opens the session picker
3. Starts tmux attached to the chosen session
4. Binds keyboard shortcuts (see below)

### Stop the overlay

```bash
hive stop
```

## Keyboard Shortcuts

These are bound automatically by `launch` (no tmux prefix needed):

| Shortcut | Action |
|----------|--------|
| `Alt-s` | Open session picker |
| `Alt-j` | Open notification list (only if notifications exist) |

These are the defaults — configure them in `config.toml` under `[keybindings]`.

### Session Picker

| Key | Action |
|-----|--------|
| Type | Fuzzy filter sessions |
| `Up` / `Down` | Navigate |
| `Enter` | Open session (creates it if needed) |
| `Ctrl-d` | Kill session (+ remove worktree in worktree mode) |
| `Ctrl-n` | Create new session (prompts for project name and branch) |
| `Ctrl-w` | Toggle between all sessions and worktrees only |
| `Esc` | Close picker |

### Notification List

| Key | Action |
|-----|--------|
| `Up` / `Down` | Navigate |
| `Enter` | Jump to that session/window |
| `d` | Delete notification |
| `Esc` | Close |

## Configuration

`~/.config/hive/config.toml`

```toml
[[project]]
name = "myproject"
path = "~/Projects/myproject"
cmd = "just dev"                  # startup command (default: "git status")

[project.worktree]
base = "~/Projects"               # parent dir for worktrees
copy_dirs = ["vendor"]            # dirs to copy into new worktrees
copy_files = ["config/local.env"] # files to copy into new worktrees
```

### Session layout

Each session gets three windows:
1. `{prefix}-run` — runs the startup command
2. `{prefix}-claude` — runs `claude`
3. `{prefix}-sh` — runs `git status`

Where `{prefix}` is the first character of the session name.

## How notifications work

Claude Code hooks call `hive status <state>` (one of `working`, `waiting`, `done`). The CLI resolves the current tmux pane and writes a small JSON file to `$XDG_STATE_HOME/hive/panes/<pane>.json`. The overlay watches that directory and turns state changes into notifications.

`hive launch` installs the hooks automatically the first time it runs — it reads `~/.claude/settings.json`, leaves any existing hooks alone, and appends the four hive entries (`PostToolUse`, `Stop`, `Notification`, `UserPromptSubmit`) only if they aren't already present. To install them by hand, the entries look like:

```json
{
  "hooks": {
    "PostToolUse":      [{ "hooks": [{ "type": "command", "command": "hive status working" }] }],
    "Stop":             [{ "hooks": [{ "type": "command", "command": "hive status done" }] }],
    "Notification":     [{ "matcher": "permission_prompt|elicitation_dialog", "hooks": [{ "type": "command", "command": "hive status waiting" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "hive status working" }] }]
  }
}
```

`hive status` reads `$TMUX_PANE` and passes it to `tmux display-message` so the right pane is recorded regardless of which window you're currently viewing.

### Notification lifecycle

1. Claude starts working -> `working` events keep coming
2. Claude finishes -> no more events for 45 seconds -> **"Finished in session"** notification
3. Claude needs input -> **"Waiting for input in session"** notification
4. You navigate to that tmux window -> notification auto-dismissed
5. Or press `d` in the notification list to delete manually

## Subcommands

| Command | Description |
|---------|-------------|
| `launch` | Main entry point — start tmux + overlay + picker |
| `pick` | Session picker (used internally by tmux keybinding) |
| `jump` | Notification list (used internally by tmux keybinding) |
| `watch` | Run the overlay (used internally by launch) |
| `status <state>` | Record agent status for the current tmux pane (used by Claude hooks) |
| `stop` | Stop the overlay process |
