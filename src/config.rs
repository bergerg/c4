use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_refresh_secs")]
    pub refresh_interval_secs: u64,
    #[serde(default = "default_projects_dir")]
    pub projects_dir: String,
    #[serde(default = "default_repo_url")]
    pub repo_url: String,
    #[serde(default = "default_view_mode")]
    pub view_mode: String,
}

fn default_repo_url() -> String {
    "https://github.com/YOUR_USER/c4.git".into()
}

fn default_view_mode() -> String {
    "compact".into()
}

fn default_hotkey() -> String {
    "cmd+option+ctrl+=".into()
}

fn default_refresh_secs() -> u64 {
    3
}

fn default_projects_dir() -> String {
    dirs::home_dir()
        .unwrap_or_default()
        .join("projects")
        .display()
        .to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            refresh_interval_secs: default_refresh_secs(),
            projects_dir: default_projects_dir(),
            repo_url: default_repo_url(),
            view_mode: default_view_mode(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("c4")
            .join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if let Ok(data) = fs::read_to_string(&path) {
            toml::from_str(&data).unwrap_or_default()
        } else {
            let cfg = Self::default();
            cfg.save();
            cfg
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(data) = toml::to_string_pretty(self) {
            let _ = fs::write(&path, data);
        }
    }

    /// All configurable fields as (key, display_name, current_value).
    pub fn fields(&self) -> Vec<(&'static str, &'static str, String)> {
        vec![
            ("hotkey", "Global Hotkey", self.hotkey.clone()),
            (
                "refresh_interval_secs",
                "Refresh Interval (secs)",
                self.refresh_interval_secs.to_string(),
            ),
            ("projects_dir", "Projects Directory", self.projects_dir.clone()),
            ("view_mode", "View Mode", self.view_mode.clone()),
            ("repo_url", "Update Repo URL", self.repo_url.clone()),
        ]
    }

    pub fn set_field(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "hotkey" => {
                // Validate hotkey
                crate::monitor::hotkey::parse_hotkey(value)?;
                self.hotkey = value.to_string();
                Ok(())
            }
            "refresh_interval_secs" => {
                let v: u64 = value.parse().map_err(|_| "Must be a number".to_string())?;
                if v == 0 {
                    return Err("Must be > 0".into());
                }
                self.refresh_interval_secs = v;
                Ok(())
            }
            "projects_dir" => {
                if !std::path::Path::new(value).is_dir() {
                    return Err(format!("'{}' is not a directory", value));
                }
                self.projects_dir = value.to_string();
                Ok(())
            }
            "view_mode" => {
                match value {
                    "compact" | "detailed" => {
                        self.view_mode = value.to_string();
                        Ok(())
                    }
                    _ => Err("Must be 'compact' or 'detailed'".into()),
                }
            }
            "repo_url" => {
                self.repo_url = value.to_string();
                Ok(())
            }
            _ => Err(format!("Unknown key: {}", key)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkey() {
        assert_eq!(Config::default().hotkey, "cmd+option+ctrl+=");
    }

    #[test]
    fn default_refresh_secs_is_3() {
        assert_eq!(Config::default().refresh_interval_secs, 3);
    }

    #[test]
    fn default_view_mode_is_compact() {
        assert_eq!(Config::default().view_mode, "compact");
    }

    #[test]
    fn fields_returns_five_entries() {
        let cfg = Config::default();
        assert_eq!(cfg.fields().len(), 5);
    }

    #[test]
    fn fields_contains_expected_keys() {
        let cfg = Config::default();
        let keys: Vec<&str> = cfg.fields().iter().map(|(k, _, _)| *k).collect();
        assert!(keys.contains(&"hotkey"));
        assert!(keys.contains(&"refresh_interval_secs"));
        assert!(keys.contains(&"projects_dir"));
        assert!(keys.contains(&"view_mode"));
        assert!(keys.contains(&"repo_url"));
    }

    #[test]
    fn set_field_view_mode_compact_valid() {
        let mut cfg = Config::default();
        cfg.view_mode = "detailed".to_string();
        assert!(cfg.set_field("view_mode", "compact").is_ok());
        assert_eq!(cfg.view_mode, "compact");
    }

    #[test]
    fn set_field_view_mode_detailed_valid() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("view_mode", "detailed").is_ok());
        assert_eq!(cfg.view_mode, "detailed");
    }

    #[test]
    fn set_field_view_mode_invalid_returns_err() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("view_mode", "fancy").is_err());
    }

    #[test]
    fn set_field_refresh_secs_valid() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("refresh_interval_secs", "10").is_ok());
        assert_eq!(cfg.refresh_interval_secs, 10);
    }

    #[test]
    fn set_field_refresh_secs_zero_returns_err() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("refresh_interval_secs", "0").is_err());
    }

    #[test]
    fn set_field_refresh_secs_non_numeric_returns_err() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("refresh_interval_secs", "abc").is_err());
    }

    #[test]
    fn set_field_repo_url_accepts_any_string() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("repo_url", "https://example.com/repo.git").is_ok());
        assert_eq!(cfg.repo_url, "https://example.com/repo.git");
    }

    #[test]
    fn set_field_projects_dir_existing_dir_ok() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("projects_dir", "/tmp").is_ok());
        assert_eq!(cfg.projects_dir, "/tmp");
    }

    #[test]
    fn set_field_projects_dir_nonexistent_returns_err() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("projects_dir", "/nonexistent/path/xyz").is_err());
    }

    #[test]
    fn set_field_hotkey_valid_updates() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("hotkey", "ctrl+shift+a").is_ok());
        assert_eq!(cfg.hotkey, "ctrl+shift+a");
    }

    #[test]
    fn set_field_hotkey_invalid_returns_err() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("hotkey", "just_a_letter").is_err());
    }

    #[test]
    fn set_field_unknown_key_returns_err() {
        let mut cfg = Config::default();
        assert!(cfg.set_field("nonexistent_field", "value").is_err());
    }
}
