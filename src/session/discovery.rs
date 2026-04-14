use anyhow::Result;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::parser;
use super::status;
use super::{Session, SessionStatus};

#[derive(Deserialize)]
struct SessionFile {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    #[serde(rename = "startedAt")]
    started_at: u64,
    #[allow(dead_code)]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct SessionsIndex {
    #[allow(dead_code)]
    version: Option<u32>,
    entries: Vec<SessionIndexEntry>,
}

#[derive(Deserialize, Clone)]
struct SessionIndexEntry {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "fullPath")]
    full_path: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(rename = "messageCount")]
    message_count: Option<u32>,
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    summary: Option<String>,
    modified: Option<String>,
}

/// All known info about a session gathered from various sources before
/// we build the final Session struct.
struct SessionInfo {
    session_id: String,
    jsonl_path: PathBuf,
    /// From PID file, if available.
    pid: u32,
    cwd: String,
    started_at_ms: Option<u64>,
    /// From index, if available.
    index_entry: Option<SessionIndexEntry>,
}

fn claude_dir() -> PathBuf {
    dirs::home_dir()
        .expect("no home dir")
        .join(".claude")
}

/// Check if a PID is alive and actively running a Claude Code session.
fn is_pid_alive(pid: u32) -> bool {
    if unsafe { libc::kill(pid as i32, 0) } != 0 {
        return false;
    }
    let output = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok();
    let comm = output
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    comm.contains("claude") || comm.contains("node")
}

fn project_name_from_cwd(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| cwd.to_string())
}

pub(crate) fn cwd_to_project_dir(cwd: &str) -> String {
    cwd.replace('/', "-").replace('.', "-")
}

pub(crate) fn is_ephemeral_cwd(cwd: &str) -> bool {
    cwd.starts_with("/tmp/c4/ephemeral-")
        || cwd.starts_with("/private/tmp/c4/ephemeral-")
}

/// Best-effort decode of an encoded project dir name back to a real path.
/// Claude encodes paths by replacing / and . with -, so the encoding is lossy.
/// We try all possible separator combinations recursively and return the first
/// that resolves to an existing directory.
fn decode_project_dir(name: &str) -> String {
    let segments: Vec<&str> = name.split('-').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return name.to_string();
    }

    // Try to find a valid path by recursively choosing / or . between segments
    fn try_decode(segments: &[&str], idx: usize, current: &str) -> Option<String> {
        if idx == segments.len() {
            return if Path::new(current).exists() {
                Some(current.to_string())
            } else {
                None
            };
        }
        if idx == 0 {
            return try_decode(segments, 1, &format!("/{}", segments[0]));
        }
        // Try / separator first (more common)
        let slash = format!("{}/{}", current, segments[idx]);
        if let Some(result) = try_decode(segments, idx + 1, &slash) {
            return Some(result);
        }
        // Try . separator (e.g. gal.berger)
        let dot = format!("{}.{}", current, segments[idx]);
        if let Some(result) = try_decode(segments, idx + 1, &dot) {
            return Some(result);
        }
        None
    }

    if let Some(path) = try_decode(&segments, 0, "") {
        return path;
    }

    // Fallback: last segment as project name
    segments.last().unwrap_or(&name.as_ref()).to_string()
}

pub fn discover_sessions() -> Result<Vec<Session>> {
    let sessions_dir = claude_dir().join("sessions");
    let projects_dir = claude_dir().join("projects");

    // 1. Build PID map from session files: sessionId -> SessionFile
    let mut pid_map: HashMap<String, SessionFile> = HashMap::new();
    if sessions_dir.exists() {
        if let Ok(entries) = fs::read_dir(&sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Ok(data) = fs::read_to_string(&path) {
                        if let Ok(sf) = serde_json::from_str::<SessionFile>(&data) {
                            pid_map.insert(sf.session_id.clone(), sf);
                        }
                    }
                }
            }
        }
    }

    // 2. Build index from sessions-index.json files: sessionId -> index entry
    //    Also track which project dirs have an index.
    let mut index_map: HashMap<String, SessionIndexEntry> = HashMap::new();
    let mut indexed_project_dirs: HashSet<PathBuf> = HashSet::new();

    if projects_dir.exists() {
        if let Ok(entries) = fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let dir_path = entry.path();
                let idx_path = dir_path.join("sessions-index.json");
                if idx_path.exists() {
                    indexed_project_dirs.insert(dir_path);
                    if let Ok(data) = fs::read_to_string(&idx_path) {
                        if let Ok(idx) = serde_json::from_str::<SessionsIndex>(&data) {
                            for e in idx.entries {
                                index_map.insert(e.session_id.clone(), e);
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Collect all known sessions into a unified map: sessionId -> SessionInfo
    let mut info_map: HashMap<String, SessionInfo> = HashMap::new();

    // From index entries
    for (sid, entry) in &index_map {
        let jsonl_path = entry
            .full_path
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| p.exists());

        if let Some(jsonl_path) = jsonl_path {
            let sf = pid_map.get(sid);
            info_map.insert(sid.clone(), SessionInfo {
                session_id: sid.clone(),
                jsonl_path,
                pid: sf.map(|s| s.pid).unwrap_or(0),
                cwd: sf
                    .map(|s| s.cwd.clone())
                    .or_else(|| entry.project_path.clone())
                    .unwrap_or_default(),
                started_at_ms: sf.map(|s| s.started_at),
                index_entry: Some(entry.clone()),
            });
        }
    }

    // From JSONL files in project dirs that have NO index (scan directly)
    if projects_dir.exists() {
        if let Ok(entries) = fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let dir_path = entry.path();
                if indexed_project_dirs.contains(&dir_path) {
                    continue; // already covered by index
                }
                if !dir_path.is_dir() {
                    continue;
                }
                if let Ok(files) = fs::read_dir(&dir_path) {
                    for file in files.flatten() {
                        let path = file.path();
                        if path.extension().is_some_and(|e| e == "jsonl") {
                            let sid = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            if info_map.contains_key(&sid) {
                                continue;
                            }
                            let sf = pid_map.get(&sid);
                            let dir_name = dir_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let cwd = sf
                                .map(|s| s.cwd.clone())
                                .unwrap_or_else(|| decode_project_dir(&dir_name));
                            info_map.insert(sid.clone(), SessionInfo {
                                session_id: sid,
                                jsonl_path: path,
                                pid: sf.map(|s| s.pid).unwrap_or(0),
                                cwd,
                                started_at_ms: sf.map(|s| s.started_at),
                                index_entry: None,
                            });
                        }
                    }
                }
            }
        }
    }

    // From PID files not yet in info_map (brand new sessions, no JSONL yet)
    for (sid, sf) in &pid_map {
        if info_map.contains_key(sid) {
            continue;
        }
        // Try to find a JSONL
        let project_key = cwd_to_project_dir(&sf.cwd);
        let jsonl_candidate = projects_dir
            .join(&project_key)
            .join(format!("{}.jsonl", sid));

        // For ephemeral sessions with a live PID, show them immediately even before
        // the JSONL is written. The parser returns None for a missing file, so the
        // session appears as WAITING with 0 messages until the first exchange.
        let jsonl_exists = jsonl_candidate.exists();
        if !jsonl_exists && !is_ephemeral_cwd(&sf.cwd) {
            continue; // non-ephemeral with no JSONL: skip
        }
        if !jsonl_exists && !is_pid_alive(sf.pid) {
            continue; // ephemeral but already dead with no JSONL: skip
        }

        info_map.insert(sid.clone(), SessionInfo {
            session_id: sid.clone(),
            jsonl_path: jsonl_candidate,
            pid: sf.pid,
            cwd: sf.cwd.clone(),
            started_at_ms: Some(sf.started_at),
            index_entry: None,
        });
    }

    // 4. Build Session structs from unified info
    let mut sessions = Vec::new();

    for (_sid, info) in &info_map {
        let parsed = parser::parse_session_jsonl(&info.jsonl_path).ok();

        let alive = info.pid > 0 && is_pid_alive(info.pid);

        let started_at = info
            .started_at_ms
            .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms as i64))
            .or_else(|| {
                info.index_entry
                    .as_ref()
                    .and_then(|e| e.modified.as_deref())
                    .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok())
            })
            .or_else(|| parsed.as_ref().and_then(|p| p.first_message_at))
            .or_else(|| {
                // Last resort: use JSONL file mtime
                fs::metadata(&info.jsonl_path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t))
            })
            .unwrap_or_default();

        let session_status = if !alive {
            SessionStatus::Dead
        } else {
            parsed
                .as_ref()
                .map(|p| status::detect_status(p, &Some(info.jsonl_path.clone())))
                .unwrap_or(SessionStatus::WaitingForInput)
        };

        let message_count = parsed
            .as_ref()
            .map(|p| p.message_count)
            .or_else(|| info.index_entry.as_ref().and_then(|e| e.message_count))
            .unwrap_or(0);

        let cost = parsed
            .as_ref()
            .map(|p| p.total_usage.estimated_cost_usd(parsed.as_ref().and_then(|p| p.model.as_deref())))
            .unwrap_or(0.0);

        // Skip dead sessions with no real usage.
        // Also always skip dead ephemeral sessions regardless of cost — they disappear immediately
        // on exit by design; cleanup is handled in App::refresh().
        if !alive && (message_count == 0 || cost == 0.0 || is_ephemeral_cwd(&info.cwd)) {
            continue;
        }

        sessions.push(Session {
            pid: info.pid,
            session_id: info.session_id.clone(),
            cwd: PathBuf::from(&info.cwd),
            started_at,
            git_branch: parsed
                .as_ref()
                .and_then(|p| p.git_branch.clone())
                .or_else(|| {
                    info.index_entry
                        .as_ref()
                        .and_then(|e| e.git_branch.clone())
                }),
            summary: info
                .index_entry
                .as_ref()
                .and_then(|e| e.summary.clone())
                .or_else(|| {
                    parsed.as_ref().and_then(|p| p.first_user_message.clone())
                }),
            project_name: project_name_from_cwd(&info.cwd),
            status: session_status,
            message_count,
            last_message_at: parsed.as_ref().and_then(|p| p.last_message_at),
            last_message_preview: parsed
                .as_ref()
                .and_then(|p| p.last_message_preview.clone())
                .or_else(|| {
                    info.index_entry.as_ref().and_then(|e| e.summary.clone())
                }),
            model: parsed.as_ref().and_then(|p| p.model.clone()),
            cost: parsed
                .as_ref()
                .map(|p| p.total_usage.clone())
                .unwrap_or_default(),
            context_usage: parsed
                .as_ref()
                .map(|p| p.context_usage.clone())
                .unwrap_or_default(),
            jsonl_path: Some(info.jsonl_path.clone()),
            active_agents: parsed.as_ref().map(|p| p.active_agents).unwrap_or(0),
            active_bg_jobs: parsed.as_ref().map(|p| p.active_bg_jobs).unwrap_or(0),
            in_iterm: false,
            is_ephemeral: is_ephemeral_cwd(&info.cwd),
        });
    }

    // Sort: waiting first, then thinking, then dead — within each group by started_at desc
    fn status_rank(s: &SessionStatus) -> u8 {
        match s {
            SessionStatus::WaitingForInput => 0,
            SessionStatus::Thinking => 1,
            SessionStatus::Dead => 2,
        }
    }
    sessions.sort_by(|a, b| {
        status_rank(&a.status)
            .cmp(&status_rank(&b.status))
            .then(b.started_at.cmp(&a.started_at))
    });

    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ephemeral_cwd_matches_temp_prefix() {
        assert!(is_ephemeral_cwd("/tmp/c4/ephemeral-1744123456"));
        assert!(is_ephemeral_cwd("/tmp/c4/ephemeral-0"));
    }

    #[test]
    fn test_is_ephemeral_cwd_does_not_match_normal_dirs() {
        assert!(!is_ephemeral_cwd("/Users/bergerg/projects/c4"));
        assert!(!is_ephemeral_cwd("/tmp/other-dir"));
        assert!(!is_ephemeral_cwd(""));
    }

    #[test]
    fn test_is_ephemeral_cwd_matches_private_tmp_prefix() {
        assert!(is_ephemeral_cwd("/private/tmp/c4/ephemeral-1744123456"));
        assert!(is_ephemeral_cwd("/private/tmp/c4/ephemeral-0"));
    }
}
