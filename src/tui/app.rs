use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::session::discovery;
use crate::session::{Session, SessionStatus};

const MAX_LOG_ENTRIES: usize = 500;

#[derive(Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Thread-safe log buffer shared between the app and background threads (hotkey, watcher).
#[derive(Clone)]
pub struct LogBuffer(Arc<Mutex<VecDeque<LogEntry>>>);

impl LogBuffer {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(VecDeque::new())))
    }

    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        let mut buf = self.0.lock().unwrap();
        if buf.len() >= MAX_LOG_ENTRIES {
            buf.pop_front();
        }
        buf.push_back(LogEntry {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            message: message.into(),
        });
    }

    pub fn info(&self, message: impl Into<String>) {
        self.log(LogLevel::Info, message);
    }

    #[allow(dead_code)]
    pub fn warn(&self, message: impl Into<String>) {
        self.log(LogLevel::Warn, message);
    }

    pub fn error(&self, message: impl Into<String>) {
        self.log(LogLevel::Error, message);
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.0.lock().unwrap().iter().cloned().collect()
    }
}

pub struct LogViewer {
    pub scroll: usize,
    pub copied: bool,
}

pub struct ConfigEditor {
    pub selected: usize,
    pub editing: bool,
    pub edit_buf: String,
    pub error: Option<String>,
    pub success: Option<String>,
    pub fields: Vec<(&'static str, &'static str, String)>,
    pub updating: bool,
}

pub struct FocusPicker {
    pub candidates: Vec<(String, String)>, // (terminal_id, terminal_name)
    pub project_name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Status,
    Project,
    Task,
    Cost,
    Messages,
    Context,
    Started,
}

impl SortColumn {
    pub const ALL: &[SortColumn] = &[
        SortColumn::Status,
        SortColumn::Project,
        SortColumn::Task,
        SortColumn::Cost,
        SortColumn::Messages,
        SortColumn::Context,
        SortColumn::Started,
    ];

    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            SortColumn::Status => "Status",
            SortColumn::Project => "Project",
            SortColumn::Task => "Task",
            SortColumn::Cost => "Cost",
            SortColumn::Messages => "Msgs",
            SortColumn::Context => "Ctx",
            SortColumn::Started => "Started",
        }
    }

    pub fn next(&self) -> SortColumn {
        let idx = SortColumn::ALL.iter().position(|c| c == self).unwrap_or(0);
        SortColumn::ALL[(idx + 1) % SortColumn::ALL.len()]
    }

    pub fn prev(&self) -> SortColumn {
        let idx = SortColumn::ALL.iter().position(|c| c == self).unwrap_or(0);
        SortColumn::ALL[(idx + SortColumn::ALL.len() - 1) % SortColumn::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub status_message_at: Option<std::time::Instant>,
    pub hotkey_display: Option<String>,
    /// When multiple terminals match, show a picker.
    pub focus_picker: Option<FocusPicker>,
    pub picker: Option<DirPicker>,
    pub logs: LogBuffer,
    pub log_viewer: Option<LogViewer>,
    pub config: Config,
    pub config_editor: Option<ConfigEditor>,
    /// True when Space (leader key) was pressed, waiting for second key.
    pub leader_active: bool,
    pub sort_column: SortColumn,
    pub sort_dir: SortDir,
    pub page: usize,
    /// Set by the UI each frame based on available table height.
    pub page_size: usize,
    /// When true, keyboard input goes to the search box.
    pub searching: bool,
    pub search_query: String,
    /// Indices into `sessions` that match the current search.
    pub filtered_indices: Vec<usize>,
}

pub struct DirPicker {
    pub query: String,
    pub all_dirs: Vec<PathBuf>,
    pub filtered: Vec<PathBuf>,
    pub selected: usize,
}

impl DirPicker {
    pub fn new() -> Self {
        let all_dirs = collect_project_dirs();
        let filtered = all_dirs.clone();
        Self {
            query: String::new(),
            all_dirs,
            filtered,
            selected: 0,
        }
    }

    pub fn update_filter(&mut self) {
        let q = self.query.to_lowercase();
        if q.is_empty() {
            self.filtered = self.all_dirs.clone();
        } else {
            self.filtered = self
                .all_dirs
                .iter()
                .filter(|p| fuzzy_match(&p.display().to_string().to_lowercase(), &q))
                .cloned()
                .collect();
        }
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.filtered.len() - 1);
        }
    }

    pub fn selected_dir(&self) -> Option<&PathBuf> {
        self.filtered.get(self.selected)
    }
}

/// Simple fuzzy match: all characters in needle appear in haystack in order.
fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    let mut haystack_chars = haystack.chars();
    for nc in needle.chars() {
        loop {
            match haystack_chars.next() {
                Some(hc) if hc == nc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

/// Collect known project directories from ~/.claude/projects/ and ~/projects/.
fn collect_project_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // From ~/.claude/projects/ - decode the path encoding
    let claude_projects = dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("projects");

    if let Ok(entries) = std::fs::read_dir(&claude_projects) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Decode: leading dash + dashes are path separators, but also dots were dashes
            // The encoded form is the path with / and . replaced by -
            // We can reconstruct by checking if the path exists
            let candidate = PathBuf::from(name.replace('-', "/"));
            // Try the literal decoded path first
            if candidate.is_dir() && seen.insert(candidate.clone()) {
                dirs.push(candidate);
                continue;
            }
            // Also try common patterns: the name often starts with -Users-<user>-projects-<name>
            // Extract just the last component and check ~/projects/<name>
            if let Some(home) = dirs::home_dir() {
                let parts: Vec<&str> = name.split('-').collect();
                // Find "projects" in the parts and take everything after
                if let Some(pos) = parts.iter().position(|&p| p == "projects") {
                    let project_name = parts[pos + 1..].join("-");
                    if !project_name.is_empty() {
                        let p = home.join("projects").join(&project_name);
                        if p.is_dir() && seen.insert(p.clone()) {
                            dirs.push(p);
                        }
                    }
                }
            }
        }
    }

    // Also scan ~/projects/ directly for any we missed
    if let Some(home) = dirs::home_dir() {
        let projects_dir = home.join("projects");
        if let Ok(entries) = std::fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && seen.insert(path.clone()) {
                    dirs.push(path);
                }
            }
        }
    }

    dirs.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .cmp(b.file_name().unwrap_or_default())
    });
    dirs
}

/// Query iTerm2 for all session TTYs and return them as a set.
fn get_iterm_ttys() -> std::collections::HashSet<String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .args(["-e", r#"
tell application "iTerm2"
    set output to ""
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                set output to output & (tty of s) & linefeed
            end repeat
        end repeat
    end repeat
    return output
end tell"#])
        .output()
        .ok();

    let mut ttys = std::collections::HashSet::new();
    if let Some(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let tty = line.trim();
            if !tty.is_empty() {
                ttys.insert(tty.to_string());
            }
        }
    }
    ttys
}

/// Get the TTY for a given PID, normalized to /dev/ttysNNN form.
fn get_pid_tty(pid: u32) -> Option<String> {
    use std::process::Command;

    let output = Command::new("ps")
        .args(["-o", "tty=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tty.is_empty() || tty == "??" {
        return None;
    }
    if tty.starts_with("/dev/") {
        Some(tty)
    } else {
        Some(format!("/dev/{}", tty))
    }
}

/// Tag each alive session with whether it's running inside iTerm2.
fn tag_iterm_sessions(sessions: &mut [Session]) {
    let iterm_ttys = get_iterm_ttys();
    for session in sessions.iter_mut() {
        if session.status == SessionStatus::Dead {
            session.in_iterm = false;
            continue;
        }
        session.in_iterm = get_pid_tty(session.pid)
            .map(|tty| iterm_ttys.contains(&tty))
            .unwrap_or(false);
    }
}

impl App {
    pub fn new(logs: LogBuffer, config: Config) -> Self {
        let mut sessions = discovery::discover_sessions().unwrap_or_default();
        tag_iterm_sessions(&mut sessions);
        logs.info(format!("Started. Found {} sessions.", sessions.len()));
        Self {
            sessions,
            selected: 0,
            should_quit: false,
            status_message: None,
            status_message_at: None,
            hotkey_display: None,
            focus_picker: None,
            picker: None,
            logs,
            log_viewer: None,
            config,
            config_editor: None,
            leader_active: false,
            sort_column: SortColumn::Status,
            sort_dir: SortDir::Asc,
            page: 0,
            page_size: 20,
            searching: false,
            search_query: String::new(),
            filtered_indices: Vec::new(),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_message_at = Some(std::time::Instant::now());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
        self.status_message_at = None;
    }

    /// Clear status message if it's been showing for more than 10 seconds.
    pub fn expire_status(&mut self) {
        if let Some(at) = self.status_message_at {
            if at.elapsed() >= std::time::Duration::from_secs(10) {
                self.clear_status();
            }
        }
    }

    pub fn cycle_sort_next(&mut self) {
        let new_col = self.sort_column.next();
        if new_col == self.sort_column {
            self.toggle_sort_dir();
        } else {
            self.sort_column = new_col;
            self.sort_dir = SortDir::Asc;
        }
        self.apply_sort();
    }

    pub fn cycle_sort_prev(&mut self) {
        let new_col = self.sort_column.prev();
        if new_col == self.sort_column {
            self.toggle_sort_dir();
        } else {
            self.sort_column = new_col;
            self.sort_dir = SortDir::Asc;
        }
        self.apply_sort();
    }

    pub fn toggle_sort_dir(&mut self) {
        self.sort_dir = match self.sort_dir {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::Asc,
        };
        self.apply_sort();
    }

    pub fn apply_sort(&mut self) {
        let prev_id = self.selected_session().map(|s| s.session_id.clone());

        let dir = self.sort_dir;
        self.sessions.sort_by(|a, b| {
            let cmp = match self.sort_column {
                SortColumn::Status => {
                    fn rank(s: &SessionStatus) -> u8 {
                        match s { SessionStatus::WaitingForInput => 0, SessionStatus::Thinking => 1, SessionStatus::Dead => 2 }
                    }
                    rank(&a.status).cmp(&rank(&b.status))
                }
                SortColumn::Project => a.project_name.to_lowercase().cmp(&b.project_name.to_lowercase()),
                SortColumn::Task => {
                    let at = a.summary.as_deref().unwrap_or("");
                    let bt = b.summary.as_deref().unwrap_or("");
                    at.to_lowercase().cmp(&bt.to_lowercase())
                }
                SortColumn::Cost => {
                    let ac = a.cost.estimated_cost_usd(a.model.as_deref());
                    let bc = b.cost.estimated_cost_usd(b.model.as_deref());
                    ac.partial_cmp(&bc).unwrap_or(std::cmp::Ordering::Equal)
                }
                SortColumn::Messages => a.message_count.cmp(&b.message_count),
                SortColumn::Context => {
                    a.context_usage.percentage().partial_cmp(&b.context_usage.percentage())
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                SortColumn::Started => a.started_at.cmp(&b.started_at),
            };
            match dir {
                SortDir::Asc => cmp,
                SortDir::Desc => cmp.reverse(),
            }
        });

        // Restore selection
        if let Some(id) = prev_id {
            if let Some(idx) = self.sessions.iter().position(|s| s.session_id == id) {
                self.selected = idx;
            }
        }
    }

    pub fn refresh(&mut self) {
        let prev_selected_id = self
            .sessions
            .get(self.selected)
            .map(|s| s.session_id.clone());

        let mut fresh = discovery::discover_sessions().unwrap_or_default();
        tag_iterm_sessions(&mut fresh);

        let fresh_map: std::collections::HashMap<String, Session> = fresh
            .into_iter()
            .map(|s| (s.session_id.clone(), s))
            .collect();

        // Update existing sessions in-place (preserve order), remove gone ones
        self.sessions.retain_mut(|s| {
            if let Some(updated) = fresh_map.get(&s.session_id) {
                *s = updated.clone();
                true
            } else {
                if s.is_ephemeral {
                    cleanup_ephemeral_dirs(s.cwd.clone());
                }
                false
            }
        });

        // Append new sessions not already in the list
        let existing_ids: std::collections::HashSet<String> =
            self.sessions.iter().map(|s| s.session_id.clone()).collect();
        self.sessions.extend(
            fresh_map
                .into_values()
                .filter(|s| !existing_ids.contains(&s.session_id)),
        );

        self.logs.info(format!("Refreshed. {} sessions.", self.sessions.len()));

        // Re-apply current sort (preserves user's chosen order)
        self.apply_sort();

        // Restore selection
        if let Some(id) = prev_selected_id {
            if let Some(idx) = self.sessions.iter().position(|s| s.session_id == id) {
                self.selected = idx;
                return;
            }
        }
        if self.selected >= self.sessions.len() && !self.sessions.is_empty() {
            self.selected = self.sessions.len() - 1;
        }
    }

    /// Number of visible items (filtered or all).
    pub fn visible_count(&self) -> usize {
        if self.search_query.is_empty() {
            self.sessions.len()
        } else {
            self.filtered_indices.len()
        }
    }

    /// Get the session index in `self.sessions` for a given position in the visible list.
    pub fn visible_session_index(&self, visible_pos: usize) -> Option<usize> {
        if self.search_query.is_empty() {
            if visible_pos < self.sessions.len() { Some(visible_pos) } else { None }
        } else {
            self.filtered_indices.get(visible_pos).copied()
        }
    }

    pub fn total_pages(&self) -> usize {
        let count = self.visible_count();
        if self.page_size == 0 || count == 0 {
            return 1;
        }
        (count + self.page_size - 1) / self.page_size
    }

    pub fn clamp_page(&mut self) {
        let max = self.total_pages().saturating_sub(1);
        if self.page > max {
            self.page = max;
        }
    }

    /// Range into the visible list for the current page.
    pub fn page_range(&self) -> std::ops::Range<usize> {
        let count = self.visible_count();
        let start = self.page * self.page_size;
        let end = (start + self.page_size).min(count);
        start..end
    }

    pub fn next_page(&mut self) {
        if self.page < self.total_pages().saturating_sub(1) {
            self.page += 1;
            self.selected = self.page * self.page_size;
        }
    }

    pub fn prev_page(&mut self) {
        if self.page > 0 {
            self.page -= 1;
            self.selected = self.page * self.page_size;
        }
    }

    pub fn next(&mut self) {
        let count = self.visible_count();
        if count == 0 { return; }
        let range = self.page_range();
        if self.selected + 1 < range.end {
            self.selected += 1;
        } else if self.page < self.total_pages().saturating_sub(1) {
            self.next_page();
        } else {
            self.page = 0;
            self.selected = 0;
        }
    }

    pub fn previous(&mut self) {
        let count = self.visible_count();
        if count == 0 { return; }
        let range = self.page_range();
        if self.selected > range.start {
            self.selected -= 1;
        } else if self.page > 0 {
            self.prev_page();
            let range = self.page_range();
            self.selected = range.end.saturating_sub(1);
        } else {
            self.page = self.total_pages().saturating_sub(1);
            self.selected = self.visible_count().saturating_sub(1);
        }
    }

    pub fn selected_session(&self) -> Option<&Session> {
        let idx = self.visible_session_index(self.selected)?;
        self.sessions.get(idx)
    }

    pub fn update_search_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_indices.clear();
        } else {
            let q = self.search_query.to_lowercase();
            self.filtered_indices = self
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    fuzzy_match(&s.project_name.to_lowercase(), &q)
                        || s.summary
                            .as_deref()
                            .is_some_and(|t| fuzzy_match(&t.to_lowercase(), &q))
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.page = 0;
        self.selected = 0;
    }

    pub fn start_search(&mut self) {
        self.searching = true;
    }

    pub fn stop_search(&mut self) {
        self.searching = false;
        // Keep the filter active, just exit input mode
    }

    pub fn clear_search(&mut self) {
        self.searching = false;
        self.search_query.clear();
        self.filtered_indices.clear();
        self.page = 0;
        self.selected = 0;
    }

    pub fn toggle_log_viewer(&mut self) {
        if self.log_viewer.is_some() {
            self.log_viewer = None;
        } else {
            let count = self.logs.entries().len();
            self.log_viewer = Some(LogViewer {
                scroll: count.saturating_sub(1),
                copied: false,
            });
        }
    }

    pub fn open_config_editor(&mut self) {
        let mut fields = self.config.fields();
        fields.push(("_version", "Version", crate::updater::current_version().to_string()));
        fields.push(("_update", ">> Check for Updates", "".to_string()));
        self.config_editor = Some(ConfigEditor {
            selected: 0,
            editing: false,
            edit_buf: String::new(),
            error: None,
            success: None,
            fields,
            updating: false,
        });
    }

    pub fn close_config_editor(&mut self) {
        self.config_editor = None;
    }

    pub fn config_start_edit(&mut self) {
        if let Some(ce) = &mut self.config_editor {
            let key = ce.fields[ce.selected].0;

            // Read-only fields
            if key == "_version" {
                return;
            }

            // Toggle fields: cycle through options instead of text edit
            if key == "view_mode" {
                let new_val = if ce.fields[ce.selected].2 == "compact" {
                    "detailed"
                } else {
                    "compact"
                };
                let _ = self.config.set_field(key, new_val);
                self.config.save();
                self.logs.info(format!("View: {}", new_val));
                // Refresh fields
                let mut fields = self.config.fields();
                fields.push(("_version", "Version", crate::updater::current_version().to_string()));
                fields.push(("_update", ">> Check for Updates", "".to_string()));
                ce.fields = fields;
                return;
            }

            // Update action
            if key == "_update" {
                if ce.updating {
                    return;
                }
                ce.updating = true;
                ce.error = None;
                ce.success = None;
                let repo_url = self.config.repo_url.clone();
                let logs = self.logs.clone();

                // Run update in background thread
                let result = crate::updater::check_and_update(&repo_url);
                logs.info(format!("Update: {}", result));
                if let Some(ce) = &mut self.config_editor {
                    if result.starts_with("Update failed") || result.starts_with("Cannot") {
                        ce.error = Some(result);
                    } else {
                        ce.success = Some(result);
                    }
                    ce.updating = false;
                }
                return;
            }

            ce.editing = true;
            ce.edit_buf = ce.fields[ce.selected].2.clone();
            ce.error = None;
            ce.success = None;
        }
    }

    pub fn config_confirm_edit(&mut self) {
        let (key, new_val) = match &self.config_editor {
            Some(ce) if ce.editing => (
                ce.fields[ce.selected].0,
                ce.edit_buf.clone(),
            ),
            _ => return,
        };

        match self.config.set_field(key, &new_val) {
            Ok(()) => {
                self.config.save();
                self.logs.info(format!("Config: {} = {}", key, new_val));
                let mut fields = self.config.fields();
                fields.push(("_version", "Version", crate::updater::current_version().to_string()));
                fields.push(("_update", ">> Check for Updates", "".to_string()));
                if let Some(ce) = &mut self.config_editor {
                    ce.fields = fields;
                    ce.editing = false;
                    ce.error = None;
                }
                // Update hotkey display if hotkey changed
                if key == "hotkey" {
                    self.hotkey_display = Some(new_val);
                    self.set_status("Hotkey changed. Restart c4 to apply.");
                }
            }
            Err(e) => {
                if let Some(ce) = &mut self.config_editor {
                    ce.error = Some(e);
                }
            }
        }
    }

    pub fn config_cancel_edit(&mut self) {
        if let Some(ce) = &mut self.config_editor {
            ce.editing = false;
            ce.error = None;
        }
    }

    pub fn open_picker(&mut self) {
        self.picker = Some(DirPicker::new());
    }

    pub fn launch_ephemeral_session(&mut self) -> Option<String> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let tmp_dir = std::path::PathBuf::from(format!("/tmp/c4/ephemeral-{}", ts));
        if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
            return Some(format!("Failed to create ephemeral dir: {}", e));
        }
        match open_terminal_with_claude(&tmp_dir) {
            Ok(()) => {
                self.logs.info(format!("Ephemeral session launched in {}", tmp_dir.display()));
                None
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp_dir);
                Some(e)
            }
        }
    }

    pub fn close_picker(&mut self) {
        self.picker = None;
    }

    /// Launch claude in a new terminal tab at the selected directory.
    pub fn launch_session(&mut self) -> Option<String> {
        let dir = match self.picker.as_ref().and_then(|p| p.selected_dir()) {
            Some(d) => d.clone(),
            None => return None,
        };
        self.picker = None;

        match open_terminal_with_claude(&dir) {
            Ok(()) => {
                let msg = format!("Launched claude in {}", dir.display());
                self.logs.info(&msg);
                self.set_status(msg);
                None
            }
            Err(e) => {
                self.logs.error(format!("Launch failed: {}", e));
                Some(e)
            }
        }
    }

    pub fn close_session(&mut self) {
        let session = match self.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };

        if session.status == SessionStatus::Dead {
            self.set_status("Session is already dead");
            return;
        }

        let name = session.project_name.clone();

        // Kill the process
        unsafe { libc::kill(session.pid as i32, libc::SIGTERM); }

        // Close the terminal pane
        let cwd = session.cwd.display().to_string();
        let _ = close_terminal_by_cwd(&cwd);

        if let Some(real_idx) = self.visible_session_index(self.selected) {
            if session.message_count == 0 {
                self.sessions.remove(real_idx);
                self.update_search_filter();
                if self.selected >= self.visible_count() && self.visible_count() > 0 {
                    self.selected = self.visible_count() - 1;
                }
            } else {
                if let Some(s) = self.sessions.get_mut(real_idx) {
                    s.status = SessionStatus::Dead;
                    s.in_iterm = false;
                }
            }
        }

        self.logs.info(format!("Terminated session: {}", name));
        self.set_status(format!("Terminated {}", name));
    }

    /// Resume a dead session in a new terminal tab.
    fn resume_dead_session(&mut self, session: &Session) {
        let cwd = session.cwd.display().to_string();
        let session_id = session.session_id.clone();
        let name = session.project_name.clone();

        match resume_in_new_tab(&cwd, &session_id) {
            Ok(()) => {
                self.logs.info(format!("Resumed {} in new tab", name));
                self.set_status(format!("Resumed {}", name));
                self.refresh();
            }
            Err(e) => {
                self.logs.error(format!("Resume failed: {}", e));
                self.set_status(e);
            }
        }
    }

    pub fn focus_session(&mut self) {
        let session = match self.selected_session() {
            Some(s) => s.clone(),
            None => return,
        };

        if session.status == SessionStatus::Dead {
            self.resume_dead_session(&session);
            return;
        }

        if !session.in_iterm {
            self.set_status("Cannot focus: session is not running in iTerm2");
            return;
        }

        let candidates = match find_terminal_candidates(session.pid) {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => {
                self.set_status("Terminal not found");
                return;
            }
            Err(e) => {
                self.logs.error(format!("Focus failed: {}", e));
                self.set_status(e);
                return;
            }
        };

        if candidates.len() == 1 {
            match focus_terminal_by_id(&candidates[0].0) {
                Ok(_) => {
                    self.logs.info(format!("Focused {}", session.project_name));
                    self.clear_status();
                }
                Err(e) => {
                    self.logs.error(format!("Focus failed: {}", e));
                    self.set_status(e);
                }
            }
        } else {
            // Multiple matches: show picker
            self.focus_picker = Some(FocusPicker {
                candidates,
                project_name: session.project_name.clone(),
            });
        }
    }
}

/// Open a new iTerm2 tab and resume a session by ID.
fn resume_in_new_tab(cwd: &str, session_id: &str) -> Result<(), String> {
    use std::process::Command;

    let escaped_cwd = cwd.replace('\\', "\\\\").replace('"', "\\\"");
    let escaped_id = session_id.replace('\\', "\\\\").replace('"', "\\\"");
    let cmd = format!("claude --resume {}", escaped_id);

    let script = format!(
        r#"tell application "iTerm2"
    tell current window
        create tab with default profile
        tell current session
            write text "cd '{escaped_cwd}' && {cmd}"
        end tell
    end tell
    activate
    return "ok"
end tell"#,
    );

    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("osascript: {}", e))?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if result == "ok" {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() { "iTerm2 not responding".into() } else { stderr })
    }
}

fn cleanup_ephemeral_dirs(cwd: std::path::PathBuf) {
    std::thread::spawn(move || {
        let _ = std::fs::remove_dir_all(&cwd);
        if let Some(home) = dirs::home_dir() {
            let encoded = crate::session::discovery::cwd_to_project_dir(&cwd.display().to_string());
            let _ = std::fs::remove_dir_all(
                home.join(".claude").join("projects").join(&encoded),
            );
        }
    });
}

fn open_terminal_with_claude(dir: &PathBuf) -> Result<(), String> {
    use std::process::Command;

    let dir_str = dir.display().to_string();
    let escaped_dir = dir_str.replace('\\', "\\\\").replace('"', "\\\"");

    let script = format!(
        r#"tell application "iTerm2"
    tell current window
        create tab with default profile
        tell current session
            write text "cd '{escaped_dir}' && claude"
        end tell
    end tell
    activate
    return "ok"
end tell"#,
    );

    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("osascript: {}", e))?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if result == "ok" {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() { "iTerm2 not responding".into() } else { stderr })
    }
}


/// Find all candidate iTerm2 sessions that could be running the given PID.
/// Returns (session_id, session_name) pairs matching the PID's TTY.
fn find_terminal_candidates(pid: u32) -> Result<Vec<(String, String)>, String> {
    use std::process::Command;

    // Get the PID's TTY
    let tty_output = Command::new("ps")
        .args(["-o", "tty=", "-p", &pid.to_string()])
        .output()
        .map_err(|e| format!("ps failed: {}", e))?;
    let target_tty = String::from_utf8_lossy(&tty_output.stdout).trim().to_string();
    if target_tty.is_empty() || target_tty == "??" {
        return Err(format!("No TTY for PID {}", pid));
    }
    // Normalize: ps gives "ttysNNN", iTerm2 gives "/dev/ttysNNN"
    let target_tty_full = if target_tty.starts_with("/dev/") {
        target_tty.clone()
    } else {
        format!("/dev/{}", target_tty)
    };

    // Get all iTerm2 sessions with their tty and name
    let as_output = Command::new("osascript")
        .args(["-e", r#"
tell application "iTerm2"
    set output to ""
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                set output to output & (id of s) & "|" & (tty of s) & "|" & (name of s) & linefeed
            end repeat
        end repeat
    end repeat
    return output
end tell"#])
        .output()
        .map_err(|e| format!("osascript failed: {}", e))?;

    let sessions_str = String::from_utf8_lossy(&as_output.stdout);
    let mut candidates = Vec::new();

    for line in sessions_str.lines() {
        let parts: Vec<&str> = line.splitn(3, '|').collect();
        if parts.len() < 3 {
            continue;
        }
        if parts[1] == target_tty_full {
            candidates.push((parts[0].to_string(), parts[2].to_string()));
        }
    }

    Ok(candidates)
}

/// Focus a specific iTerm2 session by its ID.
pub fn focus_terminal_by_id(session_id: &str) -> Result<(), String> {
    use std::process::Command;

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

    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("focus failed: {}", e))?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if result == "ok" {
        Ok(())
    } else {
        Err("Session not found in iTerm2".into())
    }
}



fn close_terminal_by_cwd(cwd: &str) -> Result<(), String> {
    use std::process::Command;

    let escaped_cwd = cwd.replace('\\', "\\\\").replace('"', "\\\"");

    let script = format!(
        r#"tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                try
                    if (variable named "session.path" in s) is "{escaped_cwd}" then
                        tell s to close
                        return "ok"
                    end if
                end try
            end repeat
        end repeat
    end repeat
end tell"#,
    );

    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("osascript: {}", e))?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if result == "ok" {
        Ok(())
    } else {
        Err("Session not found in iTerm2".into())
    }
}
