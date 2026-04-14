pub mod cost;
pub mod discovery;
pub mod parser;
pub mod status;

use chrono::{DateTime, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Session {
    pub pid: u32,
    pub session_id: String,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
    pub git_branch: Option<String>,
    pub summary: Option<String>,
    pub project_name: String,
    pub status: SessionStatus,
    pub message_count: u32,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_message_preview: Option<String>,
    pub model: Option<String>,
    pub cost: TokenUsage,
    pub context_usage: ContextUsage,
    pub jsonl_path: Option<PathBuf>,
    pub active_agents: u32,
    pub active_bg_jobs: u32,
    /// Whether this session is running inside iTerm2.
    pub in_iterm: bool,
    /// Whether this session is ephemeral (runs in a temporary directory).
    pub is_ephemeral: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    WaitingForInput,
    Thinking,
    Dead,
}

impl SessionStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::WaitingForInput => "WAITING",
            Self::Thinking => "THINKING",
            Self::Dead => "DEAD",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl TokenUsage {
    pub fn estimated_cost_usd(&self, model: Option<&str>) -> f64 {
        // Pricing per million tokens (as of 2025)
        let (input_price, output_price, cache_read_price, cache_write_price) = match model {
            Some(m) if m.contains("opus") => (15.0, 75.0, 1.5, 18.75),
            Some(m) if m.contains("haiku") => (0.80, 4.0, 0.08, 1.0),
            _ => (3.0, 15.0, 0.30, 3.75), // sonnet default
        };
        let m = 1_000_000.0;
        (self.input_tokens as f64 / m * input_price)
            + (self.output_tokens as f64 / m * output_price)
            + (self.cache_read_tokens as f64 / m * cache_read_price)
            + (self.cache_creation_tokens as f64 / m * cache_write_price)
    }
}

#[derive(Debug, Clone)]
pub struct ContextUsage {
    pub current_tokens: u64,
    pub max_tokens: u64,
}

impl Default for ContextUsage {
    fn default() -> Self {
        Self {
            current_tokens: 0,
            max_tokens: 200_000,
        }
    }
}

impl ContextUsage {
    pub fn percentage(&self) -> f32 {
        if self.max_tokens == 0 {
            return 0.0;
        }
        (self.current_tokens as f32 / self.max_tokens as f32) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_status_label_waiting() {
        assert_eq!(SessionStatus::WaitingForInput.label(), "WAITING");
    }

    #[test]
    fn session_status_label_thinking() {
        assert_eq!(SessionStatus::Thinking.label(), "THINKING");
    }

    #[test]
    fn session_status_label_dead() {
        assert_eq!(SessionStatus::Dead.label(), "DEAD");
    }

    #[test]
    fn token_usage_cost_zero_tokens() {
        let usage = TokenUsage::default();
        assert_eq!(usage.estimated_cost_usd(None), 0.0);
    }

    #[test]
    fn token_usage_cost_sonnet_input_only() {
        let usage = TokenUsage { input_tokens: 1_000_000, ..Default::default() };
        let cost = usage.estimated_cost_usd(None);
        assert!((cost - 3.0).abs() < 1e-9);
    }

    #[test]
    fn token_usage_cost_opus_input_only() {
        let usage = TokenUsage { input_tokens: 1_000_000, ..Default::default() };
        let cost = usage.estimated_cost_usd(Some("claude-opus-4"));
        assert!((cost - 15.0).abs() < 1e-9);
    }

    #[test]
    fn token_usage_cost_haiku_input_only() {
        let usage = TokenUsage { input_tokens: 1_000_000, ..Default::default() };
        let cost = usage.estimated_cost_usd(Some("claude-haiku-3-5"));
        assert!((cost - 0.80).abs() < 1e-9);
    }

    #[test]
    fn token_usage_cost_sonnet_input_and_output() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Default::default()
        };
        let cost = usage.estimated_cost_usd(None); // 3.0 + 15.0
        assert!((cost - 18.0).abs() < 1e-9);
    }

    #[test]
    fn token_usage_cost_cache_tokens() {
        let usage = TokenUsage {
            cache_read_tokens: 1_000_000,
            cache_creation_tokens: 1_000_000,
            ..Default::default()
        };
        let cost = usage.estimated_cost_usd(None); // 0.30 + 3.75
        assert!((cost - 4.05).abs() < 1e-9);
    }

    #[test]
    fn context_usage_percentage_zero_current() {
        let ctx = ContextUsage::default();
        assert_eq!(ctx.percentage(), 0.0);
    }

    #[test]
    fn context_usage_percentage_half() {
        let ctx = ContextUsage { current_tokens: 100_000, max_tokens: 200_000 };
        assert!((ctx.percentage() - 50.0).abs() < 1e-3);
    }

    #[test]
    fn context_usage_percentage_full() {
        let ctx = ContextUsage { current_tokens: 200_000, max_tokens: 200_000 };
        assert!((ctx.percentage() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn context_usage_percentage_zero_max_returns_zero() {
        let ctx = ContextUsage { current_tokens: 100, max_tokens: 0 };
        assert_eq!(ctx.percentage(), 0.0);
    }
}
