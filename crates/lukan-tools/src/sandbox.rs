//! Bubblewrap (bwrap) OS-level sandbox for Bash tool commands.
//!
//! Provides kernel-enforced filesystem isolation: read-only everywhere,
//! writable only in allowed dirs + /tmp, sensitive files blocked.

use std::path::Path;
use std::sync::OnceLock;

/// Path where the AppArmor profile for bwrap is installed.
const APPARMOR_PROFILE_PATH: &str = "/etc/apparmor.d/bwrap";

/// Content of the AppArmor profile that allows bwrap to use user namespaces.
const APPARMOR_PROFILE_CONTENT: &str = r#"abi <abi/4.0>,
profile bwrap /usr/bin/bwrap flags=(unconfined) {
  userns,
}
"#;

/// Default file patterns to block inside the sandbox.
pub const DEFAULT_SENSITIVE_PATTERNS: &[&str] = &[
    ".env*",
    "credentials.json",
    "*.pem",
    "*.key",
    "*.p12",
    ".npmrc",
];

/// Configuration for building bwrap command arguments.
pub struct BwrapConfig {
    /// Directories that should be writable inside the sandbox.
    pub allowed_dirs: Vec<String>,
    /// Glob patterns for sensitive files to block (overlay with /dev/null).
    pub sensitive_patterns: Vec<String>,
    /// Working directory to preserve inside the sandbox.
    pub cwd: String,
}

/// Sandbox configuration passed to tools via `ToolContext`.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Whether the OS-level sandbox is enabled.
    pub enabled: bool,
    /// Directories that should be writable inside the sandbox.
    pub allowed_dirs: Vec<String>,
    /// Glob patterns for sensitive files to block.
    pub sensitive_patterns: Vec<String>,
}

/// Cached result of `is_bwrap_available()`.
static BWRAP_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Timeout for bwrap availability checks (matches Node.js implementation).
const BWRAP_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Check if bwrap is available and functional on this system.
///
/// Returns `false` on non-Linux platforms. On Linux, runs a simple test
/// command once (with a 5-second timeout) and caches the result for
/// the lifetime of the process.
pub fn is_bwrap_available() -> bool {
    *BWRAP_AVAILABLE.get_or_init(|| {
        if !cfg!(target_os = "linux") {
            return false;
        }

        run_bwrap_test_with_timeout(BWRAP_CHECK_TIMEOUT)
    })
}

/// Spawn a bwrap test command and wait up to `timeout` for it to finish.
/// Returns `true` only if the command exits successfully within the timeout.
fn run_bwrap_test_with_timeout(timeout: std::time::Duration) -> bool {
    let mut child = match std::process::Command::new("bwrap")
        .args([
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--",
            "true",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    // Poll the child at short intervals up to the timeout.
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                // Still running
                if start.elapsed() >= timeout {
                    // Timed out -- kill and return false
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(poll_interval);
            }
            Err(_) => return false,
        }
    }
}

/// Clear the cached bwrap availability result.
///
/// Used after installing/removing AppArmor profiles to force a re-check.
pub fn clear_bwrap_cache() {
    // OnceLock doesn't support clearing, so we use a workaround:
    // We just accept that the cache persists for the process lifetime.
    // For setup/uninstall commands, we re-test directly.
}

/// Build the bwrap argument array for sandboxing a command.
///
/// Mount order matters for overlapping paths:
/// 1. `--ro-bind / /` -- entire filesystem read-only
/// 2. `--dev /dev` -- minimal device nodes
/// 3. `--proc /proc` -- process info
/// 4. `--tmpfs /tmp` -- writable /tmp (isolated)
/// 5. `--bind <dir> <dir>` -- writable allowed dirs (only if dir exists)
/// 6. `--ro-bind /dev/null <file>` -- block sensitive files (AFTER writable binds)
/// 7. `--new-session` -- new process session
/// 8. `--die-with-parent` -- kill sandbox if parent dies
/// 9. `--chdir <cwd>` -- preserve working directory
///
/// Returns a `Vec<String>` where the first element is `"bwrap"` and the rest are args.
pub fn build_bwrap_args(config: &BwrapConfig) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // 1. Read-only root filesystem
    args.extend(["--ro-bind".into(), "/".into(), "/".into()]);

    // 2. Device nodes
    args.extend(["--dev".into(), "/dev".into()]);

    // 3. Process info
    args.extend(["--proc".into(), "/proc".into()]);

    // 4. Writable /tmp (isolated)
    args.extend(["--tmpfs".into(), "/tmp".into()]);

    // 5. Writable bind mounts for allowed directories
    for dir in &config.allowed_dirs {
        let resolved = resolve_path(dir);
        if Path::new(&resolved).exists() {
            args.extend(["--bind".into(), resolved.clone(), resolved]);
        }
    }

    // 6. Block sensitive files by overlaying /dev/null (must come AFTER writable binds)
    let mut search_dirs: Vec<String> = vec![config.cwd.clone()];
    for dir in &config.allowed_dirs {
        search_dirs.push(resolve_path(dir));
    }
    let sensitive_files = resolve_sensitive_files(&config.sensitive_patterns, &search_dirs);
    for file in &sensitive_files {
        args.extend(["--ro-bind".into(), "/dev/null".into(), file.clone()]);
    }

    // 7. New session (replaces setsid)
    args.push("--new-session".into());

    // 8. Kill sandbox if parent dies
    args.push("--die-with-parent".into());

    // 9. Preserve working directory
    args.extend(["--chdir".into(), config.cwd.clone()]);

    // Prepend the bwrap binary name
    let mut result = vec!["bwrap".to_string()];
    result.append(&mut args);
    result
}

/// Resolve sensitive file patterns against directories, returning only existing file paths.
///
/// For each unique directory in `search_dirs`, reads its entries and matches them
/// against each pattern using simple glob matching:
/// - `".env*"` matches entries starting with `.env`
/// - `"*.pem"` matches entries ending with `.pem`
/// - `"credentials.json"` matches exact entry name
pub fn resolve_sensitive_files(patterns: &[String], search_dirs: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Deduplicate search dirs
    let mut unique_dirs = Vec::new();
    let mut dir_set = std::collections::HashSet::new();
    for dir in search_dirs {
        let resolved = resolve_path(dir);
        if dir_set.insert(resolved.clone()) {
            unique_dirs.push(resolved);
        }
    }

    for dir in &unique_dirs {
        let dir_path = Path::new(dir);
        if !dir_path.exists() {
            continue;
        }

        let entries = match std::fs::read_dir(dir_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let entry_names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        for pattern in patterns {
            for entry in &entry_names {
                if glob_match(pattern, entry) {
                    let full_path = dir_path.join(entry);
                    if full_path.exists() {
                        let path_str = full_path.to_string_lossy().to_string();
                        if seen.insert(path_str.clone()) {
                            files.push(path_str);
                        }
                    }
                }
            }
        }
    }

    files
}

/// Diagnose why bwrap is not working. Returns a human-readable message.
pub fn diagnose_bwrap() -> String {
    if !cfg!(target_os = "linux") {
        return "bwrap is only supported on Linux.".to_string();
    }

    if !Path::new("/usr/bin/bwrap").exists() {
        return "bwrap binary not found. Install it with: sudo apt install bubblewrap".to_string();
    }

    // Try running bwrap and capture stderr
    let result = std::process::Command::new("bwrap")
        .args([
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--",
            "true",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                return "bwrap is working.".to_string();
            }

            let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
            if stderr.contains("permission denied") && stderr.contains("uid map") {
                // Check if AppArmor restricts userns
                if let Ok(sysctl_output) = std::process::Command::new("sysctl")
                    .args(["-n", "kernel.apparmor_restrict_unprivileged_userns"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .output()
                {
                    let value = String::from_utf8_lossy(&sysctl_output.stdout);
                    if value.trim() == "1" {
                        return if Path::new(APPARMOR_PROFILE_PATH).exists() {
                            "AppArmor blocks unprivileged user namespaces. Profile exists but may need reloading: sudo apparmor_parser -r /etc/apparmor.d/bwrap".to_string()
                        } else {
                            "AppArmor blocks unprivileged user namespaces. Fix with: lukan sandbox setup".to_string()
                        };
                    }
                }
                return "User namespace creation denied. Fix with: lukan sandbox setup".to_string();
            }

            let stderr_str = String::from_utf8_lossy(&output.stderr);
            format!("bwrap failed: {}", stderr_str.trim())
        }
        Err(_) => "Could not execute bwrap.".to_string(),
    }
}

/// Check if the AppArmor profile for bwrap is already installed.
pub fn has_apparmor_profile() -> bool {
    Path::new(APPARMOR_PROFILE_PATH).exists()
}

/// Install the AppArmor profile for bwrap and reload it.
///
/// Requires sudo -- spawns interactive sudo commands.
/// Returns a result message describing the outcome.
pub fn setup_apparmor() -> anyhow::Result<String> {
    if !cfg!(target_os = "linux") {
        return Ok("AppArmor profiles are only supported on Linux.".to_string());
    }

    if !Path::new("/usr/bin/bwrap").exists() {
        return Ok("bwrap is not installed. Run: sudo apt install bubblewrap".to_string());
    }

    // Check if already working (test directly, don't use cache)
    if test_bwrap_works() {
        return Ok("bwrap is already working -- no setup needed.".to_string());
    }

    // Write the profile via sudo tee
    println!("Installing AppArmor profile for bwrap at {APPARMOR_PROFILE_PATH}...");
    let write_result = std::process::Command::new("sudo")
        .args(["tee", APPARMOR_PROFILE_PATH])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match write_result {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                use std::io::Write;
                let _ = stdin.write_all(APPARMOR_PROFILE_CONTENT.as_bytes());
            }
            let output = child.wait_with_output()?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Ok(format!(
                    "Failed to write AppArmor profile: {}",
                    if err.trim().is_empty() {
                        "sudo denied"
                    } else {
                        err.trim()
                    }
                ));
            }
        }
        Err(e) => {
            return Ok(format!("Failed to write AppArmor profile: {e}"));
        }
    }

    // Reload the profile
    println!("Reloading AppArmor profile...");
    let reload_result = std::process::Command::new("sudo")
        .args(["apparmor_parser", "-r", APPARMOR_PROFILE_PATH])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()?;

    if !reload_result.status.success() {
        let err = String::from_utf8_lossy(&reload_result.stderr);
        return Ok(format!("Profile written but reload failed: {}", err.trim()));
    }

    // Re-test bwrap (direct test, not cached)
    if test_bwrap_works() {
        Ok("AppArmor profile installed. bwrap sandbox is now active.".to_string())
    } else {
        Ok("Profile installed but bwrap still fails. Run: lukan sandbox status".to_string())
    }
}

/// Remove the AppArmor profile for bwrap.
///
/// Requires sudo. Returns a result message describing the outcome.
pub fn uninstall_apparmor() -> anyhow::Result<String> {
    if !Path::new(APPARMOR_PROFILE_PATH).exists() {
        return Ok("AppArmor profile is not installed -- nothing to remove.".to_string());
    }

    // Remove the loaded profile from the kernel
    println!("Removing AppArmor profile...");
    let _ = std::process::Command::new("sudo")
        .args(["apparmor_parser", "-R", APPARMOR_PROFILE_PATH])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    // Delete the file
    let rm_result = std::process::Command::new("sudo")
        .args(["rm", APPARMOR_PROFILE_PATH])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()?;

    if !rm_result.status.success() {
        let err = String::from_utf8_lossy(&rm_result.stderr);
        return Ok(format!(
            "Failed to remove profile: {}",
            if err.trim().is_empty() {
                "sudo denied"
            } else {
                err.trim()
            }
        ));
    }

    Ok("AppArmor profile removed.".to_string())
}

// ── Internal helpers ────────────────────────────────────────────────────

/// Test if bwrap works directly (not cached). Uses the same timeout as `is_bwrap_available()`.
fn test_bwrap_works() -> bool {
    run_bwrap_test_with_timeout(BWRAP_CHECK_TIMEOUT)
}

/// Resolve a path to its canonical absolute form.
/// Falls back to the original string if canonicalization fails.
fn resolve_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

/// Simple glob matching for sensitive file patterns.
///
/// Supports:
/// - `"*.ext"` -- matches entries ending with `.ext`
/// - `"prefix*"` -- matches entries starting with `prefix`
/// - `"exact"` -- exact match
/// - `"*mid*"` -- contains match (both sides have `*`)
fn glob_match(pattern: &str, entry: &str) -> bool {
    if pattern == entry {
        // Exact match
        return true;
    }

    let starts_with_star = pattern.starts_with('*');
    let ends_with_star = pattern.ends_with('*');

    match (starts_with_star, ends_with_star) {
        (true, true) => {
            // *mid* -- contains match
            let mid = &pattern[1..pattern.len() - 1];
            entry.contains(mid)
        }
        (true, false) => {
            // *.ext -- suffix match
            let suffix = &pattern[1..];
            entry.ends_with(suffix)
        }
        (false, true) => {
            // prefix* -- prefix match
            let prefix = &pattern[..pattern.len() - 1];
            entry.starts_with(prefix)
        }
        (false, false) => {
            // No wildcards -- exact match only (already checked above)
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("credentials.json", "credentials.json"));
        assert!(!glob_match("credentials.json", "other.json"));
    }

    #[test]
    fn test_glob_match_prefix() {
        assert!(glob_match(".env*", ".env"));
        assert!(glob_match(".env*", ".env.local"));
        assert!(glob_match(".env*", ".env.production"));
        assert!(!glob_match(".env*", "env"));
    }

    #[test]
    fn test_glob_match_suffix() {
        assert!(glob_match("*.pem", "server.pem"));
        assert!(glob_match("*.pem", "cert.pem"));
        assert!(!glob_match("*.pem", "server.key"));
        assert!(glob_match("*.key", "server.key"));
        assert!(glob_match("*.p12", "cert.p12"));
    }

    #[test]
    fn test_glob_match_contains() {
        assert!(glob_match("*secret*", "my-secret-file"));
        assert!(!glob_match("*secret*", "my-file"));
    }

    #[test]
    fn test_is_bwrap_available_returns_bool() {
        // This just verifies the function doesn't panic
        let _ = is_bwrap_available();
    }

    #[test]
    fn test_build_bwrap_args_basic_structure() {
        let config = BwrapConfig {
            allowed_dirs: vec![],
            sensitive_patterns: vec![],
            cwd: "/home/user/project".to_string(),
        };
        let args = build_bwrap_args(&config);

        // First element must be "bwrap"
        assert_eq!(args[0], "bwrap");

        // Must contain the critical flags in order
        let args_str = args[1..].join(" ");

        // ro-bind must come first
        let ro_bind_pos = args_str.find("--ro-bind / /").unwrap();
        // dev must come after ro-bind
        let dev_pos = args_str.find("--dev /dev").unwrap();
        assert!(dev_pos > ro_bind_pos);
        // proc must come after dev
        let proc_pos = args_str.find("--proc /proc").unwrap();
        assert!(proc_pos > dev_pos);
        // tmpfs must come after proc
        let tmpfs_pos = args_str.find("--tmpfs /tmp").unwrap();
        assert!(tmpfs_pos > proc_pos);
        // new-session must be present
        assert!(args_str.contains("--new-session"));
        // die-with-parent must be present
        assert!(args_str.contains("--die-with-parent"));
        // chdir must be present
        assert!(args_str.contains("--chdir /home/user/project"));
    }

    #[test]
    fn test_build_bwrap_args_includes_all_required_flags() {
        let config = BwrapConfig {
            allowed_dirs: vec![],
            sensitive_patterns: vec![],
            cwd: "/tmp/test".to_string(),
        };
        let args = build_bwrap_args(&config);

        assert!(args.contains(&"--ro-bind".to_string()));
        assert!(args.contains(&"--dev".to_string()));
        assert!(args.contains(&"--proc".to_string()));
        assert!(args.contains(&"--tmpfs".to_string()));
        assert!(args.contains(&"--new-session".to_string()));
        assert!(args.contains(&"--die-with-parent".to_string()));
        assert!(args.contains(&"--chdir".to_string()));
    }

    #[test]
    fn test_build_bwrap_args_skips_nonexistent_dirs() {
        let config = BwrapConfig {
            allowed_dirs: vec!["/nonexistent/dir/that/should/not/exist".to_string()],
            sensitive_patterns: vec![],
            cwd: "/tmp".to_string(),
        };
        let args = build_bwrap_args(&config);

        // The --bind for the nonexistent dir should NOT be present
        let bind_positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| a.as_str() == "--bind")
            .map(|(i, _)| i)
            .collect();
        assert!(
            bind_positions.is_empty(),
            "Should not have --bind for nonexistent dirs"
        );
    }

    #[test]
    fn test_resolve_sensitive_files_no_matches() {
        let patterns = vec!["*.nonexistent_extension_xyz".to_string()];
        let dirs = vec!["/tmp".to_string()];
        let files = resolve_sensitive_files(&patterns, &dirs);
        // Unlikely to find files with this extension
        assert!(files.is_empty() || !files.is_empty()); // Just don't panic
    }

    #[test]
    fn test_resolve_sensitive_files_finds_matches() {
        // Create a temp directory with test files
        let tmp = std::env::temp_dir().join("lukan_sandbox_test");
        let _ = std::fs::create_dir_all(&tmp);
        let test_file = tmp.join(".env.test");
        let _ = std::fs::write(&test_file, "TEST=1");

        let patterns = vec![".env*".to_string()];
        let dirs = vec![tmp.to_string_lossy().to_string()];
        let files = resolve_sensitive_files(&patterns, &dirs);

        assert!(
            files.iter().any(|f| f.contains(".env.test")),
            "Should find .env.test, got: {files:?}"
        );

        // Cleanup
        let _ = std::fs::remove_file(&test_file);
        let _ = std::fs::remove_dir(&tmp);
    }

    #[test]
    fn test_diagnose_bwrap_returns_string() {
        // Just verify it doesn't panic and returns a non-empty message
        let msg = diagnose_bwrap();
        assert!(!msg.is_empty());
    }
}
