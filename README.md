# C4 - Claude Code Command Center

A terminal dashboard for monitoring and managing all your Claude Code sessions from a single pane.

C4 reads Claude Code's session data from `~/.claude/` and presents a live overview of every running (and recently exited) session, including status, token usage, estimated cost, context window fill, and more. It also lets you jump to any session's terminal, launch new sessions, and configure settings -- all without leaving the TUI.

## Features

- **Session dashboard** -- lists all Claude Code sessions with project name, git branch, status (WAITING / THINKING / DEAD), context usage percentage, estimated cost, and message count.
- **Live refresh** -- watches `~/.claude/` for file changes and auto-refreshes every few seconds. Manual refresh with `r`.
- **Detail panel** -- shows the selected session's duration, model, PID, last message preview, full cost breakdown (input / output / cache read / cache write tokens), and a context window gauge.
- **Focus switching** -- press Enter to focus the terminal tab or split pane running the selected session. Supports Ghostty, iTerm2, and Terminal.app. Uses caching for near-instant repeat focuses.
- **Global hotkey** -- a system-wide keyboard shortcut (default: Option+Shift+=) that brings C4's terminal into focus from any app. Requires macOS Accessibility permission on first use.
- **New session launcher** -- press `n` to open a fuzzy directory picker. Type to filter, select a directory, and C4 opens a new terminal tab with `claude` running in that directory.
- **Log viewer** -- press `l` to view C4's internal log. Navigate with j/k, copy a line with `y`.
- **Configuration** -- press `c` to open the settings editor. Changes are saved to `~/.config/c4/config.toml` and persist across restarts.
- **Debug mode** -- run `c4 --debug` to print session data to stdout and exit (useful for scripting).

## Requirements

- macOS (uses AppleScript for terminal focus switching)
- Claude Code CLI installed and on PATH
- Rust toolchain (for building from source)

## Installation

### From source

```
git clone <repo-url> && cd c4
make install
```

This builds a release binary and installs it to `/usr/local/bin/c4`. If Rust is not installed, the Makefile installs it automatically.

To install to a different location:

```
make install PREFIX=$HOME/.local
```

### One-liner install

```
curl -sSf https://raw.githubusercontent.com/<user>/c4/main/install.sh | bash
```

### Manual

```
cargo build --release
cp target/release/c4 ~/.local/bin/
```

### Uninstall

If you cloned the repo:

```
make uninstall
```

If you installed via the one-liner:

```
curl -sSf https://raw.githubusercontent.com/<user>/c4/main/install.sh | bash -s -- --uninstall
```

Both remove the binary from `~/.local/bin/c4` and the config directory `~/.config/c4/`.

## Usage

```
c4                              # start the dashboard
c4 --debug                      # print session info to stdout and exit
c4 --hotkey "ctrl+shift+c"      # override the global hotkey
c4 --no-hotkey                  # disable the global hotkey
```

## Keyboard Controls

### Dashboard

| Key       | Action                              |
|-----------|-------------------------------------|
| j / Down  | Select next session                 |
| k / Up    | Select previous session             |
| Enter     | Focus the selected session terminal |
| n         | New session (directory picker)       |
| l         | Open log viewer                     |
| c         | Open settings                       |
| r         | Manual refresh                      |
| q         | Quit                                |

### Directory Picker (n)

| Key       | Action                |
|-----------|-----------------------|
| Type      | Fuzzy filter          |
| Down      | Select next           |
| Up        | Select previous       |
| Enter     | Launch claude session |
| Esc       | Cancel                |

### Log Viewer (l)

| Key       | Action                  |
|-----------|-------------------------|
| j / Down  | Scroll down             |
| k / Up    | Scroll up               |
| g         | Jump to top             |
| G         | Jump to bottom          |
| y         | Copy selected line      |
| PageUp    | Scroll up 20 lines     |
| PageDown  | Scroll down 20 lines   |
| l / Esc   | Close                   |

### Settings (c)

| Key       | Action                              |
|-----------|-------------------------------------|
| j / Down  | Select next setting                 |
| k / Up    | Select previous setting             |
| Enter     | Edit selected setting               |
| Enter     | Confirm edit (while editing)        |
| Esc       | Cancel edit / close settings        |

## Configuration

Settings are stored in `~/.config/c4/config.toml`. You can edit this file directly or use the in-app settings editor (`c`).

```toml
hotkey = "option+shift+="
refresh_interval_secs = 3
projects_dir = "/Users/you/projects"
```

### hotkey

The global keyboard shortcut that brings C4 into focus from anywhere. Format: modifier keys joined with `+`, followed by the key.

Supported modifiers: `ctrl`, `shift`, `alt` / `option`, `cmd` / `command`.

Supported keys: `a`-`z`, `0`-`9`, `f1`-`f12`, `space`, `enter`, `tab`, `` ` ``, `-`, `=`, `[`, `]`, `\`, `;`, `'`, `,`, `.`, `/`.

Examples:
- `option+shift+=`
- `ctrl+shift+c`
- `cmd+shift+space`

The global hotkey requires macOS Accessibility permission. On first use, macOS will prompt you to grant permission to your terminal app in System Settings > Privacy & Security > Accessibility.

Changes to the hotkey require restarting C4 to take effect.

### refresh_interval_secs

How often C4 polls for session status changes, in seconds. The file watcher also triggers immediate refreshes when `~/.claude/` changes, so this is a fallback interval. Default: 3.

### projects_dir

The directory scanned by the new session picker (`n`) for project directories. C4 also scans `~/.claude/projects/` for previously used directories. Default: `~/projects`.

## How It Works

C4 reads Claude Code's file-based session data:

| Source | Data |
|--------|------|
| `~/.claude/sessions/{PID}.json` | Active session registry: PID, session ID, working directory, start time |
| `~/.claude/projects/{project}/{uuid}.jsonl` | Full conversation history: messages, tool calls, token usage per turn |
| Session JSONL `usage` fields | Input/output/cache tokens for cost estimation and context tracking |

Session status is determined by:
- **PID liveness** -- `kill -0 PID` to check if the process is running
- **Last message role** -- if the last message is from the assistant and the JSONL hasn't been written to in 5+ seconds, the session is WAITING for user input
- **Recency** -- if the JSONL was just modified and the last message is from the user, the session is THINKING

Cost estimation uses per-model pricing:
- Opus: $15 / $75 / $1.50 / $18.75 per million tokens (input / output / cache read / cache write)
- Sonnet: $3 / $15 / $0.30 / $3.75
- Haiku: $0.80 / $4 / $0.08 / $1.00

Context usage is estimated from the total input tokens (input + cache read + cache creation) of the most recent assistant turn.

## Terminal Support

Focus switching and new session launching use AppleScript and support:

- **Ghostty** -- matches terminals by working directory across all windows, tabs, and split panes. Uses Ghostty's `focus` command for precise pane targeting.
- **iTerm2** -- matches sessions by `session.path` variable. Selects the tab and activates the window.
- **Terminal.app** -- matches tabs by command history. Basic tab selection.

C4 auto-detects which terminal apps are running and only queries those.

## Building for Distribution

The release binary is a single ~1.5 MB file with no runtime dependencies.

```
cargo build --release
# Binary at target/release/c4
```

For universal binaries (arm64 + x86_64):

```
rustup target add x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create \
  target/aarch64-apple-darwin/release/c4 \
  target/x86_64-apple-darwin/release/c4 \
  -output c4
```

## License

MIT
