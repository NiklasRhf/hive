# hive

A tmux session manager with notification overlay for Claude Code instances.

Manages multiple Claude Code workers from one place — session creation, git worktrees, fuzzy picker, and non-intrusive X11 notifications when instances finish or need input.

## Features

- **Session picker** with fuzzy search (nucleo) inside a tmux popup
- **Git worktree** creation and cleanup per project
- **Notification overlay** — click-through X11 window positioned in the top-right of your terminal
- **Idle detection** — detects when Claude Code finishes a task (no activity for 45s)
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
