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
