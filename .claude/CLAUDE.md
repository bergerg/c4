# C4 - Claude Code Command Center

macOS-only TUI dashboard for monitoring Claude Code sessions. Requires iTerm2 — the app exits immediately on any other terminal.

## Build & Run

```bash
cargo check             # fast validation (no binary)
cargo build --release   # release binary → target/release/c4
make install            # build + install to ~/.local/bin/c4
cargo test              # run tests
```

## Module Map

```
src/
  main.rs               # entry point, event loop, key handling, AppleScript focus
  config.rs             # Config struct, ~/.config/c4/config.toml load/save
  session/
    mod.rs              # Session, SessionStatus, TokenUsage, ContextUsage types
    discovery.rs        # discover_sessions() — the core data pipeline (see below)
    parser.rs           # parse_session_jsonl() — reads JSONL conversation files
    status.rs           # detect_status() — WAITING / THINKING heuristic
    cost.rs             # per-model pricing constants
  tui/
    app.rs              # App state, all action methods (focus, launch, close, etc.)
    ui.rs               # ratatui rendering
    hotkey.rs           # global hotkey via rdev
  monitor/
    watcher.rs          # notify file watcher on ~/.claude/
    hotkey.rs           # hotkey thread lifecycle
  updater.rs            # install.sh --update one-liner, version parsing
```

## Data Pipeline (discovery.rs)

`discover_sessions()` merges three sources:

| Source | Location | Contains |
|--------|----------|----------|
| PID files | `~/.claude/sessions/<pid>.json` | pid, sessionId, cwd, startedAt |
| Session index | `~/.claude/projects/<project>/sessions-index.json` | message count, git branch, summary |
| JSONL files | `~/.claude/projects/<project>/<sessionId>.jsonl` | full conversation, tokens, model |

Key quirks to know:
- **`/clear` handling**: creates a new JSONL with a new sessionId, but the PID file keeps the old sessionId. `discovery.rs` detects this and transfers the PID to the newer JSONL.
- **Ephemeral sessions**: live in `~/.local/share/c4/ephemeral/`. They appear in the dashboard while alive and are pruned immediately on exit.
- **Project dir encoding**: Claude encodes paths by replacing `/` and `.` with `-`. `decode_project_dir()` reverses this with filesystem probing.

## Version Bumps

Update version in `Cargo.toml`, then always stage `Cargo.lock` in the same commit. Never bump one without the other.

## Key Constraints

- iTerm2 only — session focusing uses AppleScript against iTerm2's API
- macOS only — uses `libc::kill`, `ps`, `pbcopy`, AppleScript
- No async — uses `std::sync::mpsc` channels; rdev hotkey listener runs on its own thread
