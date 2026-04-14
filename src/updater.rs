use std::process::Command;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn current_version() -> &'static str {
    CURRENT_VERSION
}

/// Check for updates and install if available.
/// Returns a status message.
const REPO_RAW_BASE: &str = "https://raw.githubusercontent.com/bergerg/c4/main/";

pub fn check_and_update() -> String {
    match do_update() {
        Ok(msg) => msg,
        Err(e) => format!("Update failed: {}", e),
    }
}

/// Fetches the remote Cargo.toml and returns the version string if a newer version is available.
fn fetch_remote_version() -> Result<Option<String>, String> {
    let cargo_url = format!("{}Cargo.toml", REPO_RAW_BASE);

    let output = Command::new("curl")
        .args(["-sSf", "--max-time", "10", &cargo_url])
        .output()
        .map_err(|e| format!("curl failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("Failed to fetch remote version: {}", stderr.trim()));
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let remote_version = content
        .lines()
        .find(|l| l.starts_with("version"))
        .and_then(|l| l.split('"').nth(1))
        .ok_or_else(|| "Cannot parse version from remote Cargo.toml".to_string())?
        .to_string();

    if is_newer(&remote_version, CURRENT_VERSION) {
        Ok(Some(remote_version))
    } else {
        Ok(None)
    }
}

fn do_update() -> Result<String, String> {
    let script_url = format!("{}install.sh", REPO_RAW_BASE);

    // Lightweight version check — fetches only Cargo.toml, no clone
    let new_version = fetch_remote_version()?;
    let Some(new_version) = new_version else {
        return Ok(format!("Already up to date (v{})", CURRENT_VERSION));
    };

    // Run the install script with --update flag
    let cmd = format!("curl -sSf '{}' | bash -s -- --update", script_url);
    let output = Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| format!("Failed to run update script: {}", e))?;

    if output.status.success() {
        Ok(format!(
            "Updated v{} -> v{}. Restart c4 to use new version.",
            CURRENT_VERSION, new_version
        ))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        Err(msg.trim().to_string())
    }
}

fn is_newer(remote: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let r = parse(remote);
    let c = parse(current);
    r > c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_patch_bump() {
        assert!(is_newer("1.0.1", "1.0.0"));
    }

    #[test]
    fn is_newer_minor_bump() {
        assert!(is_newer("1.1.0", "1.0.9"));
    }

    #[test]
    fn is_newer_major_bump() {
        assert!(is_newer("2.0.0", "1.9.9"));
    }

    #[test]
    fn is_newer_same_version_is_false() {
        assert!(!is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn is_newer_older_remote_is_false() {
        assert!(!is_newer("0.9.9", "1.0.0"));
    }

    #[test]
    fn is_newer_older_minor_is_false() {
        assert!(!is_newer("1.0.0", "1.1.0"));
    }

    #[test]
    fn current_version_is_not_empty() {
        assert!(!current_version().is_empty());
    }

    #[test]
    fn repo_raw_base_points_to_install_sh() {
        assert!(format!("{}install.sh", REPO_RAW_BASE).contains("bergerg/c4"));
    }
}
