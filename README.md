# hive

A tmux session manager with transparent notification overlay for Claude Code instances.

Manages multiple Claude Code workers from one place — session creation, git worktrees, fuzzy picker, and non-intrusive X11 notifications when instances finish or need input.

## Features

- **Session picker** with fuzzy search (nucleo) inside a tmux popup
- **Git worktree** creation and cleanup per project
- **Notification overlay** — transparent, click-through X11 window positioned in the top-right of your terminal
- **Idle detection** — detects when Claude Code finishes a task (no activity for 45s)
- **Notification persistence** — notifications stay until you visit the tmux window or delete them manually

## Install

```bash
cargo install --path .
```

### Dependencies

- tmux (>= 3.2 for display-popup)
- libcairo (for notification text rendering)
- X11 with a compositor (picom/compton) for transparency

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

The overlay monitors `/tmp/claude-status-*` files written by Claude Code hooks. Configure these hooks in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "echo working > /tmp/claude-status-$(tmux display-message -p -t $TMUX_PANE '#S:#I')" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "echo done > /tmp/claude-status-$(tmux display-message -p -t $TMUX_PANE '#S:#I')" }] }],
    "Notification": [{ "matcher": "permission_prompt|elicitation_dialog", "hooks": [{ "type": "command", "command": "echo waiting > /tmp/claude-status-$(tmux display-message -p -t $TMUX_PANE '#S:#I')" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "echo working > /tmp/claude-status-$(tmux display-message -p -t $TMUX_PANE '#S:#I')" }] }]
  }
}
```

The `-t $TMUX_PANE` is important — it ensures the status file is written for the correct window regardless of which window you're currently viewing.

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
| `stop` | Stop the overlay process |
