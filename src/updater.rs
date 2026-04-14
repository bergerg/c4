use std::process::Command;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn current_version() -> &'static str {
    CURRENT_VERSION
}

/// Check for updates and install if available.
/// Returns a status message.
pub fn check_and_update(repo_url: &str) -> String {
    match do_update(repo_url) {
        Ok(msg) => msg,
        Err(e) => format!("Update failed: {}", e),
    }
}

/// Returns a unique temp directory path for the update process.
/// Uses PID + nanosecond timestamp to avoid predictable names in world-writable /tmp.
fn update_tmpdir() -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("c4-update-{}-{}", std::process::id(), nanos))
}

fn do_update(repo_url: &str) -> Result<String, String> {
    // Clone to temp dir
    // SECURITY NOTE: This updater clones from a user-configurable URL with no
    // signature or checksum verification. Only use trusted repo_url values.
    let tmpdir = update_tmpdir();
    let _ = std::fs::remove_dir_all(&tmpdir);

    let status = Command::new("git")
        .args(["clone", "--depth", "1", repo_url, &tmpdir.display().to_string()])
        .output()
        .map_err(|e| format!("git clone failed: {}", e))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr).to_string();
        let _ = std::fs::remove_dir_all(&tmpdir);
        return Err(format!("git clone failed: {}", stderr.trim()));
    }

    // Read remote version from Cargo.toml
    let cargo_toml = std::fs::read_to_string(tmpdir.join("Cargo.toml"))
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&tmpdir);
            format!("Cannot read Cargo.toml: {}", e)
        })?;

    let remote_version = cargo_toml
        .lines()
        .find(|l| l.starts_with("version"))
        .and_then(|l| l.split('"').nth(1))
        .ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&tmpdir);
            "Cannot parse version from remote Cargo.toml".to_string()
        })?
        .to_string();

    if remote_version == CURRENT_VERSION {
        let _ = std::fs::remove_dir_all(&tmpdir);
        return Ok(format!("Already up to date (v{})", CURRENT_VERSION));
    }

    // Compare versions (simple semver: higher = newer)
    if !is_newer(&remote_version, CURRENT_VERSION) {
        let _ = std::fs::remove_dir_all(&tmpdir);
        return Ok(format!(
            "Already up to date (v{}, remote v{})",
            CURRENT_VERSION, remote_version
        ));
    }

    // Build
    let build = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&tmpdir)
        .output()
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&tmpdir);
            format!("cargo build failed: {}", e)
        })?;

    if !build.status.success() {
        let stderr = String::from_utf8_lossy(&build.stderr).to_string();
        let _ = std::fs::remove_dir_all(&tmpdir);
        return Err(format!("Build failed: {}", stderr.trim().lines().last().unwrap_or("")));
    }

    // Replace the current binary
    let new_binary = tmpdir.join("target/release/c4");
    let current_binary = std::env::current_exe().map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmpdir);
        format!("Cannot find current exe: {}", e)
    })?;

    // Copy new to temp file, then atomically rename (atomic on POSIX)
    let tmp_binary = current_binary.with_extension("tmp");
    std::fs::copy(&new_binary, &tmp_binary).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmpdir);
        format!("Cannot write temporary binary: {}. Try: sudo c4 or check permissions.", e)
    })?;
    std::fs::rename(&tmp_binary, &current_binary).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmpdir);
        let _ = std::fs::remove_file(&tmp_binary);
        format!("Cannot replace binary: {}. Try: sudo c4 or check permissions.", e)
    })?;

    let _ = std::fs::remove_dir_all(&tmpdir);

    Ok(format!(
        "Updated v{} -> v{}. Restart c4 to use new version.",
        CURRENT_VERSION, remote_version
    ))
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
    fn random_update_tmpdir_is_unique() {
        let a = update_tmpdir();
        let b = update_tmpdir();
        assert_ne!(a, b, "two calls must return different paths");
    }

    #[test]
    fn random_update_tmpdir_not_predictable_from_pid() {
        let pid = std::process::id();
        let predictable = std::env::temp_dir().join(format!("c4-update-{}", pid));
        let actual = update_tmpdir();
        assert_ne!(actual, predictable, "dir must not be guessable from PID alone");
    }
}
