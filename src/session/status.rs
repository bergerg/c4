use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::parser::ParsedSession;
use super::SessionStatus;

/// Detect whether an alive session is waiting for input or actively thinking.
///
/// Heuristic:
/// - If last message role is "assistant" -> likely waiting for user input
/// - If last message role is "user" -> likely thinking/processing
/// - Also check JSONL file recency: if modified very recently, likely active
pub fn detect_status(parsed: &ParsedSession, jsonl_path: &Option<PathBuf>) -> SessionStatus {
    let recently_modified = jsonl_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .is_some_and(|mtime| {
            SystemTime::now()
                .duration_since(mtime)
                .unwrap_or(Duration::MAX)
                < Duration::from_secs(5)
        });

    match parsed.last_message_role.as_deref() {
        Some("user") => SessionStatus::Thinking,
        Some("assistant") if recently_modified => SessionStatus::Thinking,
        Some("assistant") => {
            match parsed.last_stop_reason.as_deref() {
                Some("tool_use") => SessionStatus::WaitingForApproval,
                _ => SessionStatus::Idle,
            }
        }
        _ => SessionStatus::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ContextUsage, TokenUsage};
    use crate::session::parser::ParsedSession;

    fn make_parsed(last_role: Option<&str>, last_stop_reason: Option<&str>) -> ParsedSession {
        ParsedSession {
            message_count: 1,
            first_message_at: None,
            last_message_at: None,
            first_user_message: None,
            last_message_preview: None,
            last_message_role: last_role.map(|s| s.to_string()),
            last_stop_reason: last_stop_reason.map(|s| s.to_string()),
            model: None,
            git_branch: None,
            total_usage: TokenUsage::default(),
            context_usage: ContextUsage::default(),
            active_agents: 0,
            active_bg_jobs: 0,
        }
    }

    #[test]
    fn user_role_is_thinking() {
        let parsed = make_parsed(Some("user"), None);
        assert_eq!(detect_status(&parsed, &None), SessionStatus::Thinking);
    }

    #[test]
    fn assistant_end_turn_is_idle() {
        let parsed = make_parsed(Some("assistant"), Some("end_turn"));
        assert_eq!(detect_status(&parsed, &None), SessionStatus::Idle);
    }

    #[test]
    fn assistant_tool_use_is_waiting_for_approval() {
        let parsed = make_parsed(Some("assistant"), Some("tool_use"));
        assert_eq!(detect_status(&parsed, &None), SessionStatus::WaitingForApproval);
    }

    #[test]
    fn assistant_no_stop_reason_is_idle() {
        let parsed = make_parsed(Some("assistant"), None);
        assert_eq!(detect_status(&parsed, &None), SessionStatus::Idle);
    }

    #[test]
    fn no_role_is_idle() {
        let parsed = make_parsed(None, None);
        assert_eq!(detect_status(&parsed, &None), SessionStatus::Idle);
    }

    #[test]
    fn assistant_role_with_recently_modified_file_is_thinking() {
        use std::fs;
        let path = std::env::temp_dir().join("c4_status_test.jsonl");
        fs::write(&path, "").unwrap();
        let parsed = make_parsed(Some("assistant"), Some("end_turn"));
        assert_eq!(detect_status(&parsed, &Some(path.clone())), SessionStatus::Thinking);
        fs::remove_file(path).ok();
    }
}
