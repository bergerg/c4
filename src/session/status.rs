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
        Some("assistant") => SessionStatus::WaitingForInput,
        _ => SessionStatus::WaitingForInput,
    }
}
