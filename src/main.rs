mod config;
mod monitor;
mod session;
mod tui;
mod updater;

use std::io;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use monitor::hotkey;
use monitor::watcher;
use tui::app::{App, LogBuffer};
use tui::ui;

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("c4 {}", updater::current_version());
        return Ok(());
    }

    if std::env::args().any(|a| a == "--debug") {
        let sessions = session::discovery::discover_sessions()?;
        for s in &sessions {
            println!(
                "PID={} project={} branch={} status={} model={} msgs={} ctx={:.1}% cost=${:.4} jsonl={}",
                s.pid,
                s.project_name,
                s.git_branch.as_deref().unwrap_or("-"),
                s.status.label(),
                s.model.as_deref().unwrap_or("?"),
                s.message_count,
                s.context_usage.percentage(),
                s.cost.estimated_cost_usd(s.model.as_deref()),
                s.jsonl_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "NONE".into()),
            );
        }
        if sessions.is_empty() {
            println!("No sessions found.");
        }
        return Ok(());
    }

    ensure_ephemeral_base_trusted();

    let cfg = config::Config::load();

    // Hotkey: CLI flag overrides config, --no-hotkey disables
    let no_hotkey = std::env::args().any(|a| a == "--no-hotkey");
    let hotkey_combo = if no_hotkey {
        None
    } else {
        Some(parse_hotkey_arg().unwrap_or(cfg.hotkey.clone()))
    };

    let logs = LogBuffer::new();
    let ccc_title = format!("c4-{}", std::process::id());

    // Register global hotkey if configured
    if let Some(combo) = &hotkey_combo {
        match hotkey::parse_hotkey(combo) {
            Ok(hk) => {
                let title = ccc_title.clone();
                let callback = Arc::new(move || {
                    focus_own_terminal(&title);
                });
                hotkey::start_hotkey_listener(hk, callback);
                logs.info(format!("Global hotkey registered: {}", combo));
            }
            Err(e) => {
                eprintln!("Invalid hotkey '{}': {}", combo, e);
                std::process::exit(1);
            }
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        crossterm::terminal::SetTitle(&ccc_title),
        EnterAlternateScreen
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, hotkey_combo.as_deref(), logs, cfg);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Returns Some(combo) for a hotkey, or None if `--no-hotkey` is passed.
fn parse_hotkey_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--no-hotkey") {
        return None;
    }
    for (i, arg) in args.iter().enumerate() {
        if arg == "--hotkey" {
            return args.get(i + 1).cloned();
        }
        if let Some(val) = arg.strip_prefix("--hotkey=") {
            return Some(val.to_string());
        }
    }
    None
}

/// Focus C4's own iTerm2 session by matching the unique title we set on startup.
/// Caches the session ID after first lookup for instant subsequent focuses.
fn focus_own_terminal(title: &str) {
    use std::process::Command;
    use std::sync::Mutex;

    static CACHE: std::sync::LazyLock<Mutex<Option<String>>> =
        std::sync::LazyLock::new(|| Mutex::new(None));

    // Fast path: focus by cached session ID
    if let Some(session_id) = CACHE.lock().unwrap().as_ref() {
        let escaped_id = session_id.replace('"', "\\\"");
        let script = format!(
            r#"tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if id of s is "{escaped_id}" then
                    select s
                    tell w to select t
                    activate
                    return "ok"
                end if
            end repeat
        end repeat
    end repeat
end tell"#,
        );
        if let Ok(output) = Command::new("osascript").args(["-e", &script]).output() {
            if String::from_utf8_lossy(&output.stdout).trim() == "ok" {
                return;
            }
        }
        *CACHE.lock().unwrap() = None;
    }

    // Slow path: find session by title, cache ID
    let escaped_title = title.replace('"', "\\\"");
    let script = format!(
        r#"tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if name of s contains "{escaped_title}" then
                    select s
                    tell w to select t
                    activate
                    return id of s
                end if
            end repeat
        end repeat
    end repeat
end tell"#,
    );

    if let Ok(output) = Command::new("osascript").args(["-e", &script]).output() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !result.is_empty() && result != "" {
            *CACHE.lock().unwrap() = Some(result);
        }
    }
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    hotkey_combo: Option<&str>,
    logs: LogBuffer,
    cfg: config::Config,
) -> Result<()> {
    let mut app = App::new(logs, cfg);
    app.hotkey_display = hotkey_combo.map(|s| s.to_string());

    let (watch_tx, watch_rx) = mpsc::channel();
    let _watcher = watcher::start_watcher(watch_tx);

    loop {
        app.expire_status();
        app.poll_update();
        terminal.draw(|f| ui::draw(f, &mut app))?;

        if watch_rx.try_recv().is_ok() {
            while watch_rx.try_recv().is_ok() {}
            app.refresh();
        }

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if app.searching {
                    handle_search_key(&mut app, key.code);
                } else if app.focus_picker.is_some() {
                    handle_focus_picker_key(&mut app, key.code);
                } else if app.config_editor.is_some() {
                    handle_config_key(&mut app, key.code);
                } else if app.picker.is_some() {
                    handle_picker_key(&mut app, key.code);
                } else if app.log_viewer.is_some() {
                    handle_log_key(&mut app, key.code);
                } else if app.leader_active {
                    app.leader_active = false;
                    match key.code {
                        KeyCode::Char('n') => app.open_picker(),
                        KeyCode::Char('x') => app.close_session(),
                        KeyCode::Char('r') => app.refresh(),
                        KeyCode::Char('l') => app.toggle_log_viewer(),
                        KeyCode::Char('c') => app.open_config_editor(),
                        KeyCode::Char('e') => {
                            if let Some(err) = app.launch_ephemeral_session() {
                                app.set_status(err);
                            }
                        }
                        _ => {} // invalid key, just dismiss
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('j') | KeyCode::Down => app.next(),
                        KeyCode::Char('k') | KeyCode::Up => app.previous(),
                        KeyCode::Right => app.next_page(),
                        KeyCode::Left => app.prev_page(),
                        KeyCode::Char('l') => app.toggle_log_viewer(),
                        KeyCode::Char(' ') => { app.leader_active = true; }
                        KeyCode::Enter => app.focus_session(),
                        KeyCode::Char('s') => app.cycle_sort_next(),
                        KeyCode::Char('S') => app.cycle_sort_prev(),
                        KeyCode::Char('o') => app.toggle_sort_dir(),
                        KeyCode::Char('/') => app.start_search(),
                        KeyCode::Char('t') => app.toggle_show_terminated(),
                        KeyCode::Esc => app.clear_search(),
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_picker_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.close_picker(),
        KeyCode::Enter => {
            if let Some(err) = app.launch_session() {
                app.set_status(err);
            }
        }
        KeyCode::Up => {
            if let Some(p) = &mut app.picker {
                p.previous();
            }
        }
        KeyCode::Down => {
            if let Some(p) = &mut app.picker {
                p.next();
            }
        }
        KeyCode::Backspace => {
            if let Some(p) = &mut app.picker {
                p.query.pop();
                p.update_filter();
            }
        }
        KeyCode::Char(c) => {
            if let Some(p) = &mut app.picker {
                p.query.push(c);
                p.update_filter();
            }
        }
        _ => {}
    }
}

fn handle_log_key(app: &mut App, code: KeyCode) {
    // Clear copied indicator on any navigation
    if let Some(v) = &mut app.log_viewer {
        if !matches!(code, KeyCode::Char('y')) {
            v.copied = false;
        }
    }

    match code {
        KeyCode::Esc | KeyCode::Char('l') => app.toggle_log_viewer(),
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(v) = &mut app.log_viewer {
                v.scroll = v.scroll.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(v) = &mut app.log_viewer {
                let count = app.logs.entries().len();
                if v.scroll < count.saturating_sub(1) {
                    v.scroll += 1;
                }
            }
        }
        KeyCode::PageUp => {
            if let Some(v) = &mut app.log_viewer {
                v.scroll = v.scroll.saturating_sub(20);
            }
        }
        KeyCode::PageDown => {
            if let Some(v) = &mut app.log_viewer {
                let count = app.logs.entries().len();
                v.scroll = (v.scroll + 20).min(count.saturating_sub(1));
            }
        }
        KeyCode::Char('G') => {
            if let Some(v) = &mut app.log_viewer {
                let count = app.logs.entries().len();
                v.scroll = count.saturating_sub(1);
            }
        }
        KeyCode::Char('g') => {
            if let Some(v) = &mut app.log_viewer {
                v.scroll = 0;
            }
        }
        KeyCode::Char('y') => {
            let entries = app.logs.entries();
            if let Some(v) = &mut app.log_viewer {
                if let Some(entry) = entries.get(v.scroll) {
                    let text = format!("{} [{}] {}", entry.timestamp, match entry.level {
                        tui::app::LogLevel::Info => "INFO",
                        tui::app::LogLevel::Warn => "WARN",
                        tui::app::LogLevel::Error => "ERROR",
                    }, entry.message);
                    copy_to_clipboard(&text);
                    v.copied = true;
                }
            }
        }
        _ => {}
    }
}

fn handle_config_key(app: &mut App, code: KeyCode) {
    let editing = app.config_editor.as_ref().is_some_and(|ce| ce.editing);

    if editing {
        match code {
            KeyCode::Esc => app.config_cancel_edit(),
            KeyCode::Enter => app.config_confirm_edit(),
            KeyCode::Backspace => {
                if let Some(ce) = &mut app.config_editor {
                    ce.edit_buf.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ce) = &mut app.config_editor {
                    ce.edit_buf.push(c);
                }
            }
            _ => {}
        }
    } else {
        match code {
            KeyCode::Esc | KeyCode::Char('c') => app.close_config_editor(),
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(ce) = &mut app.config_editor {
                    ce.selected = ce.selected.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(ce) = &mut app.config_editor {
                    let max = ce.fields.len().saturating_sub(1);
                    if ce.selected < max {
                        ce.selected += 1;
                    }
                }
            }
            KeyCode::Enter => app.config_start_edit(),
            _ => {}
        }
    }
}

fn handle_focus_picker_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.focus_picker = None;
        }
        KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
            let idx = (c as usize) - ('1' as usize);
            if let Some(fp) = app.focus_picker.take() {
                if let Some((term_id, _name)) = fp.candidates.get(idx) {
                    match tui::app::focus_terminal_by_id(term_id) {
                        Ok(_) => {
                            app.logs.info(format!("Focused {} ({})", fp.project_name, idx + 1));
                            app.clear_status();
                        }
                        Err(e) => {
                            app.set_status(e);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_search_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.clear_search(),
        KeyCode::Enter => app.stop_search(),
        KeyCode::Backspace => {
            app.search_query.pop();
            app.recompute_visible();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.recompute_visible();
        }
        KeyCode::Down | KeyCode::Up => {
            // Allow navigation while searching
            app.stop_search();
        }
        _ => {}
    }
}

/// Returns the user-owned base directory for ephemeral sessions.
/// Uses ~/.local/share/c4/ephemeral/ which is not world-writable, unlike /tmp.
pub fn ephemeral_base_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".local/share/c4/ephemeral")
}

/// Ensure the ephemeral base dir exists and is listed in ~/.claude/settings.json trustedDirectories.
/// This makes Claude Code trust all ephemeral sessions launched under the base dir without
/// showing the "Do you trust this project?" prompt.
fn ensure_ephemeral_base_trusted() {
    let base = ephemeral_base_dir();
    let _ = std::fs::create_dir_all(&base);

    let settings_path = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("settings.json"),
        None => return,
    };

    let mut settings: serde_json::Value = settings_path
        .exists()
        .then(|| std::fs::read_to_string(&settings_path).ok())
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    let trusted_glob = format!("{}/*", base.display());
    let already_trusted = settings["trustedDirectories"]
        .as_array()
        .map(|arr| arr.iter().any(|v| v.as_str() == Some(&trusted_glob)))
        .unwrap_or(false);

    if !already_trusted {
        let mut arr = settings["trustedDirectories"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        arr.push(serde_json::json!(trusted_glob));
        settings["trustedDirectories"] = serde_json::Value::Array(arr);
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(&settings_path, json);
        }
    }
}

fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if let Ok(mut child) = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
    {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}
