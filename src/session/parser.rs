use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::{ContextUsage, TokenUsage};

#[derive(Debug)]
pub struct ParsedSession {
    pub message_count: u32,
    pub first_message_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub first_user_message: Option<String>,
    pub last_message_preview: Option<String>,
    pub last_message_role: Option<String>,
    pub model: Option<String>,
    pub git_branch: Option<String>,
    pub total_usage: TokenUsage,
    pub context_usage: ContextUsage,
    pub active_agents: u32,
    pub active_bg_jobs: u32,
}

#[derive(Deserialize)]
struct JournalEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<Message>,
    timestamp: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
}

#[derive(Deserialize)]
struct Message {
    role: Option<String>,
    content: Option<serde_json::Value>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
}

pub fn parse_session_jsonl(path: &Path) -> Result<ParsedSession> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut message_count: u32 = 0;
    let mut first_message_at: Option<DateTime<Utc>> = None;
    let mut first_user_message: Option<String> = None;
    let mut last_message_at: Option<DateTime<Utc>> = None;
    let mut last_message_preview: Option<String> = None;
    let mut last_message_role: Option<String> = None;
    let mut model: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut total_usage = TokenUsage::default();
    let mut last_input_tokens: u64 = 0;
    // Track pending Agent/background-Bash tool calls
    let mut pending_agents: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut pending_bg_jobs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let entry: JournalEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let entry_type = match &entry.entry_type {
            Some(t) => t.as_str(),
            None => continue,
        };

        if entry_type != "user" && entry_type != "assistant" {
            continue;
        }

        message_count += 1;

        if let Some(branch) = &entry.git_branch {
            git_branch = Some(branch.clone());
        }

        if let Some(ts_str) = &entry.timestamp {
            if let Ok(ts) = ts_str.parse::<DateTime<Utc>>() {
                if first_message_at.is_none() {
                    first_message_at = Some(ts);
                }
                last_message_at = Some(ts);
            }
        }

        if let Some(msg) = &entry.message {
            last_message_role = msg.role.clone();

            // Capture first real user message as task description
            // Skip system-injected messages (XML tags like <local-command-caveat>)
            if first_user_message.is_none() && entry_type == "user" {
                if let Some(content) = &msg.content {
                    if let Some(preview) = extract_preview(content) {
                        if !preview.trim_start().starts_with('<') {
                            first_user_message = Some(preview);
                        }
                    }
                }
            }

            // Extract preview
            if let Some(content) = &msg.content {
                last_message_preview = extract_preview(content);
            }

            // Track model
            if let Some(m) = &msg.model {
                model = Some(m.clone());
            }

            // Track Agent/background tool calls
            if let Some(content) = &msg.content {
                track_tools(content, entry_type, &mut pending_agents, &mut pending_bg_jobs);
            }

            // Accumulate usage from assistant messages
            if entry_type == "assistant" {
                if let Some(usage) = &msg.usage {
                    total_usage.output_tokens += usage.output_tokens.unwrap_or(0);
                    total_usage.input_tokens += usage.input_tokens.unwrap_or(0);
                    total_usage.cache_read_tokens +=
                        usage.cache_read_input_tokens.unwrap_or(0);
                    total_usage.cache_creation_tokens +=
                        usage.cache_creation_input_tokens.unwrap_or(0);

                    // The input_tokens of the latest assistant message approximates current context
                    let total_input = usage.input_tokens.unwrap_or(0)
                        + usage.cache_read_input_tokens.unwrap_or(0)
                        + usage.cache_creation_input_tokens.unwrap_or(0);
                    if total_input > 0 {
                        last_input_tokens = total_input;
                    }
                }
            }
        }
    }

    let max_tokens = match model.as_deref() {
        Some(m) if m.contains("opus") => 1_000_000,
        _ => 200_000,
    };

    let context_usage = ContextUsage {
        current_tokens: last_input_tokens,
        max_tokens,
    };

    Ok(ParsedSession {
        message_count,
        first_message_at,
        first_user_message,
        last_message_at,
        last_message_preview,
        last_message_role,
        model,
        git_branch,
        total_usage,
        context_usage,
        active_agents: pending_agents.len() as u32,
        active_bg_jobs: pending_bg_jobs.len() as u32,
    })
}

/// Track Agent tool_use calls and tool_result completions in the content array.
fn track_tools(
    content: &serde_json::Value,
    entry_type: &str,
    pending_agents: &mut std::collections::HashSet<String>,
    pending_bg_jobs: &mut std::collections::HashSet<String>,
) {
    let arr = match content.as_array() {
        Some(a) => a,
        None => return,
    };

    for item in arr {
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if entry_type == "assistant" && item_type == "tool_use" {
            let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");
            if id.is_empty() {
                continue;
            }
            if name == "Agent" {
                pending_agents.insert(id.to_string());
            } else if name == "Bash" {
                let is_bg = item
                    .pointer("/input/run_in_background")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if is_bg {
                    pending_bg_jobs.insert(id.to_string());
                }
            }
        }

        if entry_type == "user" && item_type == "tool_result" {
            let tool_use_id = item.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("");
            pending_agents.remove(tool_use_id);
            pending_bg_jobs.remove(tool_use_id);
        }
    }
}

fn extract_preview(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(s) => Some(truncate(s, 80)),
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    return Some(truncate(text, 80));
                }
            }
            // Check for tool_use
            for item in arr {
                if item.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let name = item
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown");
                    return Some(format!("[tool: {}]", name));
                }
            }
            None
        }
        _ => None,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    // Take first line
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() <= max {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..max - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_tmp_jsonl(suffix: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("c4_test_{}.jsonl", suffix));
        fs::write(&path, content).unwrap();
        path
    }

    // --- truncate ---

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 80), "hello");
    }

    #[test]
    fn truncate_long_string_gets_ellipsis() {
        let s = "a".repeat(100);
        let result = truncate(&s, 80);
        assert_eq!(result.len(), 80);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_uses_first_line_only() {
        assert_eq!(truncate("first\nsecond", 80), "first");
    }

    #[test]
    fn truncate_trims_surrounding_whitespace() {
        assert_eq!(truncate("  hello  ", 80), "hello");
    }

    // --- extract_preview ---

    #[test]
    fn extract_preview_string_value() {
        let val = serde_json::json!("simple text");
        assert_eq!(extract_preview(&val), Some("simple text".to_string()));
    }

    #[test]
    fn extract_preview_array_with_text_block() {
        let val = serde_json::json!([{"type": "text", "text": "block text"}]);
        assert_eq!(extract_preview(&val), Some("block text".to_string()));
    }

    #[test]
    fn extract_preview_array_with_tool_use() {
        let val = serde_json::json!([{"type": "tool_use", "name": "Bash"}]);
        assert_eq!(extract_preview(&val), Some("[tool: Bash]".to_string()));
    }

    #[test]
    fn extract_preview_null_returns_none() {
        let val = serde_json::json!(null);
        assert_eq!(extract_preview(&val), None);
    }

    // --- parse_session_jsonl ---

    #[test]
    fn parse_empty_file_returns_zero_messages() {
        let path = write_tmp_jsonl("empty", "");
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.message_count, 0);
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_non_message_entries_are_ignored() {
        let content = r#"{"type":"summary","message":null}"#;
        let path = write_tmp_jsonl("summary_only", content);
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.message_count, 0);
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_counts_user_and_assistant_messages() {
        let content = concat!(
            "{\"type\":\"user\",\"timestamp\":\"2024-01-01T00:00:00Z\",",
            "\"message\":{\"role\":\"user\",\"content\":\"Hello\"}}\n",
            "{\"type\":\"assistant\",\"timestamp\":\"2024-01-01T00:00:01Z\",",
            "\"message\":{\"role\":\"assistant\",\"content\":\"Hi\",",
            "\"model\":\"claude-sonnet-4-5\",",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":5,",
            "\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}\n"
        );
        let path = write_tmp_jsonl("two_messages", content);
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.message_count, 2);
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_extracts_model_from_assistant_message() {
        let content = concat!(
            "{\"type\":\"assistant\",\"timestamp\":\"2024-01-01T00:00:00Z\",",
            "\"message\":{\"role\":\"assistant\",\"content\":\"Hi\",",
            "\"model\":\"claude-sonnet-4-5\",",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":5,",
            "\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}\n"
        );
        let path = write_tmp_jsonl("model_extraction", content);
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("claude-sonnet-4-5"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_extracts_git_branch() {
        let content = concat!(
            "{\"type\":\"user\",\"timestamp\":\"2024-01-01T00:00:00Z\",",
            "\"gitBranch\":\"main\",",
            "\"message\":{\"role\":\"user\",\"content\":\"Hello\"}}\n"
        );
        let path = write_tmp_jsonl("git_branch", content);
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.git_branch.as_deref(), Some("main"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_accumulates_token_usage() {
        let content = concat!(
            "{\"type\":\"assistant\",\"timestamp\":\"2024-01-01T00:00:00Z\",",
            "\"message\":{\"role\":\"assistant\",\"content\":\"first\",",
            "\"usage\":{\"input_tokens\":100,\"output_tokens\":50,",
            "\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}\n",
            "{\"type\":\"assistant\",\"timestamp\":\"2024-01-01T00:00:01Z\",",
            "\"message\":{\"role\":\"assistant\",\"content\":\"second\",",
            "\"usage\":{\"input_tokens\":200,\"output_tokens\":80,",
            "\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}\n"
        );
        let path = write_tmp_jsonl("usage_accumulate", content);
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.total_usage.input_tokens, 300);
        assert_eq!(parsed.total_usage.output_tokens, 130);
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_captures_first_user_message() {
        let content = concat!(
            "{\"type\":\"user\",\"timestamp\":\"2024-01-01T00:00:00Z\",",
            "\"message\":{\"role\":\"user\",\"content\":\"First task\"}}\n",
            "{\"type\":\"user\",\"timestamp\":\"2024-01-01T00:00:02Z\",",
            "\"message\":{\"role\":\"user\",\"content\":\"Second task\"}}\n"
        );
        let path = write_tmp_jsonl("first_user_msg", content);
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.first_user_message.as_deref(), Some("First task"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_sets_last_message_role() {
        let content = concat!(
            "{\"type\":\"user\",\"timestamp\":\"2024-01-01T00:00:00Z\",",
            "\"message\":{\"role\":\"user\",\"content\":\"Hello\"}}\n",
            "{\"type\":\"assistant\",\"timestamp\":\"2024-01-01T00:00:01Z\",",
            "\"message\":{\"role\":\"assistant\",\"content\":\"Hi\",",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":5,",
            "\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0}}}\n"
        );
        let path = write_tmp_jsonl("last_role", content);
        let parsed = parse_session_jsonl(&path).unwrap();
        assert_eq!(parsed.last_message_role.as_deref(), Some("assistant"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn parse_nonexistent_file_returns_error() {
        let path = std::path::Path::new("/nonexistent/path/session.jsonl");
        assert!(parse_session_jsonl(path).is_err());
    }
}
