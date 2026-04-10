# hive

A tmux session manager with notification overlay for Claude Code instances.

Think of it as a hive: you (the developer) are the hive mind, and each Claude Code instance is a worker. Workers go off and do their thing in their own tmux window, then communicate back via notifications when they finish a task or need your attention.

Manages multiple Claude Code workers from one place — session creation, git worktrees, fuzzy picker, and non-intrusive X11 notifications when instances finish or need input.

## Features

- **Session picker** with fuzzy search (nucleo) inside a tmux popup
- **Git worktree** creation and cleanup per project
- **Notification overlay** — click-through X11 window positioned in the top-right of your terminal
- **Notification jump list** — tmux popup showing all pending notifications; press Enter to jump straight to the worker that needs you
- **Worker dock** — togglable sidebar popup showing every Claude instance, its state, feature/branch, project, cumulative working time, and most recent task; navigate with `Up`/`Down` (or `Ctrl-k`/`Ctrl-j`) and `Enter` to jump to that worker
- **Stats dashboard** — bar chart of working time per session for today or the last 7 days
- **Idle detection** — detects when Claude Code finishes a task
- **Notification persistence** — notifications stay until you visit the tmux window or delete them manually

## Supported agents

hive is built around a simple interface: an agent calls `hive status working|waiting|done` whenever it changes state, and hive takes care of notifications, the dock, and stats.

| Agent       | Out of the box         | Notes                                                                               |
|-------------|------------------------|-------------------------------------------------------------------------------------|
| Claude Code | Yes — hooks auto-installed | `hive launch` writes the four hook entries into `~/.claude/settings.json` on first run |
| Any other CLI agent | Manual integration | Call `hive status working\|waiting\|done` from any lifecycle hook the tool exposes  |

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
| `Alt-n` | Open notification list (only if notifications exist) |
| `Alt-d` | Toggle worker dock |
| `Alt-g` | Open stats dashboard |

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
| `Up` / `Down` (or `Ctrl-k` / `Ctrl-j`) | Navigate |
| `Enter` | Jump to that session/window |
| `d` | Delete notification |
| `Esc` | Close |

### Worker Dock

A toggle-able side popup showing every Claude instance the watcher has seen, whether or not it's currently active. Sleeping workers (Done/Idle without a pending notification) fade to gray; workers that still need your attention keep their accent color.

| Key | Action |
|-----|--------|
| `Up` / `Down` (or `Ctrl-k` / `Ctrl-j`) | Navigate |
| `Enter` | Jump to that worker's window (and close the dock) |
| `Alt-d` / `Esc` / `q` | Close the dock |

## Configuration

`~/.config/hive/config.toml`

```toml
[keybindings]
picker = "M-s"
jump   = "M-n"
stats  = "M-g"
dock   = "M-d"

[picker]
width  = 60   # percent of terminal
height = 60

[jump]
width  = 60
height = 40

[stats]
width  = 70
height = 50

[dock]
width    = 20
height   = 70
position = "right"   # "left" | "right"

[panel]
enabled  = true
width    = 40        # terminal columns (cells)
position = "right"   # "left" | "right"

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

The dock reads the same state, so a worker with a pending notification stays visually "alive" in the dock (accent color, bright text) until you visit it or dismiss it; once dismissed it grays out.

## Voice control (MVP, opt-in)

hive ships with an experimental voice layer behind the `voice` cargo feature. It runs a local whisper.cpp model — no cloud STT — and is designed as a push-to-talk control surface for everything hive already does (popups, jumping to workers, accepting waiting prompts, dictating into Claude).

### Build

```bash
cargo install --path . --features voice
```

This pulls in `whisper-rs` (which builds whisper.cpp via cmake) and `cpal` for audio capture. The default build is unaffected; if you don't pass `--features voice`, none of this is compiled in.

### Set up

1. Download a ggml whisper model — `base.en` is a good starting point:

   ```bash
   mkdir -p ~/.cache/whisper
   curl -L -o ~/.cache/whisper/ggml-base.en.bin \
     https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
   ```

2. Enable voice in `~/.config/hive/config.toml`:

   ```toml
   [voice]
   enabled  = true
   model    = "~/.cache/whisper/ggml-base.en.bin"
   hotkey   = "M-v"   # PTT toggle (tmux key, no prefix)
   language = "en"
   ```

3. `hive launch` — the daemon spawns automatically and binds the hotkey.

### How it works

- `hive launch` spawns `hive voice` as a background daemon (one process, one model load).
- The daemon writes its PID to `/tmp/hive-voice.pid` and waits on `SIGUSR1`.
- `Alt-v` (or whatever you bound) runs `hive voice-trigger`, which sends `SIGUSR1` to the daemon. First press starts recording, second press stops and transcribes.
- The transcript is parsed against a fixed grammar; the matched intent is dispatched via tmux.
- `hive stop` also tears down the voice daemon.

### Voice grammar (MVP)

| Say | Action |
|-----|--------|
| "picker", "open picker", "sessions" | Open session picker |
| "dock", "workers", "open dock" | Toggle/focus the worker dock |
| "stats", "dashboard" | Open stats dashboard |
| "notifications", "notifs" | Open the notification list |
| "yes" / "accept" / "approve" / "ok" / "1" | Send `1<Enter>` to the most recently waiting Claude pane |
| "jump to <session>" / "go to <session>" / "<session>" | Switch to that tmux session (fuzzy-matched) |
| "tell claude <prompt>" / "dictate <text>" | Type the rest of the phrase into the focused pane and press Enter |
| "cancel" / "stop" / "never mind" | No-op (drop the recording) |

The fuzzy matcher strips spaces and dashes from both query and session names, so "rust foo" matches `rust-foo`.

### Known MVP limitations

- Push-to-talk is toggle-based (press to start, press again to stop) — there is no hold-to-talk.
- Resampling is linear interpolation. Whisper tolerates it, but transcript quality on a 48kHz mic is worse than running whisper at native 16kHz.
- "Accept" targets the most recently `waiting` pane it finds in `$XDG_STATE_HOME/hive/panes/`. If you have multiple workers waiting, it picks the newest one.
- Dictation sends keys to whichever pane tmux currently considers focused — if you're in the panel pane it'll go there.
- No TTS yet; feedback is via `tmux display-message` ("recording", "transcribing", the matched intent label, etc.).

## Subcommands

| Command | Description |
|---------|-------------|
| `launch` | Main entry point — start tmux + overlay + picker |
| `pick` | Session picker (used internally by tmux keybinding) |
| `jump` | Notification list (used internally by tmux keybinding) |
| `dock` | Worker dock TUI (used internally by tmux keybinding) |
| `stats` | Stats dashboard (used internally by tmux keybinding) |
| `watch` | Run the overlay (used internally by launch) |
| `status <state>` | Record agent status for the current tmux pane (used by Claude hooks or other agents) |
| `voice` | Voice control daemon (only built with `--features voice`) |
| `voice-trigger` | Toggle the voice daemon's recording state (only built with `--features voice`) |
| `stop` | Stop the overlay process (and voice daemon if running) |
