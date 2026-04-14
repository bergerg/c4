# C4 - Claude Code Command Center

A terminal dashboard for monitoring and managing all your Claude Code sessions from a single pane.

C4 reads Claude Code's session data from `~/.claude/` and presents a live overview of every running (and recently exited) session, including status, token usage, estimated cost, context window fill, and more.

> **Requires iTerm2.** macOS only.

## Features

- **Session dashboard** — lists all Claude Code sessions with project name, git branch, status (WAITING / THINKING / DEAD), context usage percentage, estimated cost, and message count.
- **Live refresh** — watches `~/.claude/` for file changes and auto-refreshes. Manual refresh with `Space r`.
- **Detail panel** — shows the selected session's duration, model, PID, last message preview, full cost breakdown (input / output / cache read / cache write tokens), and a context window gauge.
- **Focus switching** — press Enter to bring the selected session's iTerm2 tab into focus. When multiple terminals share the same session, a numbered picker lets you choose. Uses session ID caching for near-instant repeat focuses.
- **Global hotkey** — a system-wide keyboard shortcut (default: `Option+Shift+=`) that brings C4's terminal into focus from any app. Requires macOS Accessibility permission on first use.
- **New session launcher** — `Space n` opens a fuzzy directory picker. Type to filter, select a directory, and C4 opens a new iTerm2 tab with `claude` running in that directory.
- **Close session** — `Space x` terminates the selected Claude Code session.
- **Search** — press `/` to filter sessions by name. Press Esc to clear.
- **Sort** — press `s` / `S` to cycle sort columns forward/backward; `o` to toggle ascending/descending order.
- **Pagination** — Left / Right arrows page through sessions when the list is long.
- **Log viewer** — `Space l` (or `l`) to view C4's internal log. Navigate with j/k, copy a line with `y`.
- **Configuration** — `Space c` to open the settings editor. Includes a built-in updater. Changes are saved to `~/.config/c4/config.toml`.
- **Debug mode** — run `c4 --debug` to print session data to stdout and exit.

## Requirements

- macOS with iTerm2
- Claude Code CLI installed and on PATH
- Rust toolchain (for building from source; auto-installed if missing)

## Installation

### One-liner (recommended)

```bash
curl -sSf https://raw.githubusercontent.com/bergerg/c4/main/install.sh | bash
```

This clones the repo to a temp directory, builds a release binary, and installs it to `~/.local/bin/c4`. Rust is installed automatically if not present.

To install to a custom location:

```bash
curl -sSf https://raw.githubusercontent.com/bergerg/c4/main/install.sh | PREFIX=$HOME/.local bash
```

### From source

```bash
git clone https://github.com/bergerg/c4.git && cd c4
make install
```

Installs to `~/.local/bin/c4` by default. Override with:

```bash
make install PREFIX=/usr/local
```

### Manual

```bash
cargo build --release
cp target/release/c4 ~/.local/bin/
```

## Uninstall

If installed via the one-liner:

```bash
curl -sSf https://raw.githubusercontent.com/bergerg/c4/main/install.sh | bash -s -- --uninstall
```

If you cloned the repo:

```bash
make uninstall
```

Both remove the binary from `~/.local/bin/c4` and the config directory `~/.config/c4/`.

## Update

From inside C4: press `Space c` to open Settings, select `>> Check for Updates`, and press Enter. C4 clones the latest source, builds it, and replaces its own binary in place. Restart C4 after updating.

## Usage

```
c4                              # start the dashboard
c4 --debug                      # print session info to stdout and exit
c4 --version                    # print version and exit
c4 --hotkey "ctrl+shift+c"      # override the global hotkey for this run
c4 --no-hotkey                  # disable the global hotkey for this run
```

## Keyboard Controls

### Dashboard

| Key          | Action                                |
|--------------|---------------------------------------|
| j / Down     | Select next session                   |
| k / Up       | Select previous session               |
| Enter        | Focus the selected session's terminal |
| Right        | Next page                             |
| Left         | Previous page                         |
| /            | Search / filter sessions              |
| Esc          | Clear search                          |
| s            | Cycle sort column forward             |
| S            | Cycle sort column backward            |
| o            | Toggle sort direction                 |
| Space        | Activate leader key (see below)       |
| l            | Open log viewer                       |
| q            | Quit                                  |

### Leader key (Space, then…)

| Key | Action                        |
|-----|-------------------------------|
| n   | New session (directory picker) |
| x   | Close selected session        |
| r   | Manual refresh                |
| l   | Open log viewer               |
| c   | Open settings / updater       |

### Directory Picker (Space n)

| Key       | Action                 |
|-----------|------------------------|
| Type      | Fuzzy filter           |
| Down      | Select next            |
| Up        | Select previous        |
| Enter     | Launch claude session  |
| Esc       | Cancel                 |

### Focus Picker (Enter, when multiple terminals match)

| Key    | Action                             |
|--------|------------------------------------|
| 1–9    | Focus the numbered terminal        |
| Esc    | Cancel                             |

### Log Viewer (l or Space l)

| Key       | Action               |
|-----------|----------------------|
| j / Down  | Scroll down          |
| k / Up    | Scroll up            |
| g         | Jump to top          |
| G         | Jump to bottom       |
| PageUp    | Scroll up 20 lines   |
| PageDown  | Scroll down 20 lines |
| y         | Copy selected line   |
| l / Esc   | Close                |

### Settings (Space c)

| Key       | Action                       |
|-----------|------------------------------|
| j / Down  | Select next field            |
| k / Up    | Select previous field        |
| Enter     | Edit field / confirm edit    |
| Esc       | Cancel edit / close settings |

## Configuration

Settings are stored in `~/.config/c4/config.toml`. Edit directly or use the in-app settings editor (`Space c`).

```toml
hotkey = "option+shift+="
refresh_interval_secs = 3
projects_dir = "/Users/you/projects"
```

### hotkey

The global keyboard shortcut that brings C4 into focus from any app.

Format: modifier keys joined with `+`, followed by the key. Supported modifiers: `ctrl`, `shift`, `alt` / `option`, `cmd` / `command`. Supported keys: `a`–`z`, `0`–`9`, `f1`–`f12`, `space`, `enter`, `tab`, `` ` ``, `-`, `=`, `[`, `]`, `\`, `;`, `'`, `,`, `.`, `/`.

Examples: `option+shift+=`, `ctrl+shift+c`, `cmd+shift+space`

Requires macOS Accessibility permission. On first use, macOS will prompt you to grant permission to your terminal app in System Settings > Privacy & Security > Accessibility. Changes require restarting C4.

### refresh_interval_secs

Fallback polling interval in seconds. The file watcher triggers immediate refreshes when `~/.claude/` changes, so this is rarely needed. Default: 3.

### projects_dir

Directory scanned by the new session picker (`Space n`) for project directories. C4 also includes previously used directories from `~/.claude/projects/`. Default: `~/projects`.

## How It Works

C4 reads Claude Code's file-based session data:

| Source | Data |
|--------|------|
| `~/.claude/sessions/{PID}.json` | Active session registry: PID, session ID, working directory, start time |
| `~/.claude/projects/{project}/{uuid}.jsonl` | Full conversation history: messages, tool calls, token usage per turn |

Session status is determined by:
- **PID liveness** — `kill -0 PID` to check if the process is running
- **Last message role** — assistant message + no recent writes = WAITING for user input
- **Recency** — JSONL just modified + last message from user = THINKING

Cost estimation uses per-model pricing:
- Opus: $15 / $75 / $1.50 / $18.75 per million tokens (input / output / cache read / cache write)
- Sonnet: $3 / $15 / $0.30 / $3.75
- Haiku: $0.80 / $4 / $0.08 / $1.00

Context usage is estimated from the total input tokens of the most recent assistant turn.

> [!WARNING]
> **Cost and context figures are estimates only.** C4 derives token counts and costs from local session files and the numbers will not match your actual usage. Do not use C4 for billing decisions. For authoritative usage and billing data, visit your [Claude profile page](https://claude.ai/settings/billing).

## Building for Distribution

The release binary is a single statically-linked file with no runtime dependencies.

```bash
cargo build --release
# Binary at target/release/c4
```

For a universal binary (arm64 + x86_64):

```bash
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
