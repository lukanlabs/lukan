//! Integration tests for the bwrap sandbox module.

use lukan_tools::sandbox::{
    BwrapConfig, build_bwrap_args, diagnose_bwrap, has_apparmor_profile, is_bwrap_available,
    resolve_sensitive_files,
};
use std::fs;

// ── Unit-style tests ─────────────────────────────────────────────────────

#[test]
fn test_is_bwrap_available_returns_bool() {
    // Should not panic, just return a bool
    let result = is_bwrap_available();
    assert!(result || !result);
}

#[test]
fn test_build_bwrap_args_correct_mount_order() {
    let config = BwrapConfig {
        allowed_dirs: vec![],
        sensitive_patterns: vec![],
        cwd: "/home/user/project".to_string(),
    };
    let args = build_bwrap_args(&config);

    // First element must be "bwrap"
    assert_eq!(args[0], "bwrap");

    // Find positions of key flags to verify order
    let find_flag = |flag: &str| -> Option<usize> { args.iter().position(|a| a == flag) };

    let ro_bind_pos = find_flag("--ro-bind").expect("--ro-bind must be present");
    let dev_pos = find_flag("--dev").expect("--dev must be present");
    let proc_pos = find_flag("--proc").expect("--proc must be present");
    let tmpfs_pos = find_flag("--tmpfs").expect("--tmpfs must be present");
    let new_session_pos = find_flag("--new-session").expect("--new-session must be present");
    let die_with_parent_pos =
        find_flag("--die-with-parent").expect("--die-with-parent must be present");
    let chdir_pos = find_flag("--chdir").expect("--chdir must be present");

    // Verify order: ro-bind < dev < proc < tmpfs < new-session < die-with-parent < chdir
    assert!(ro_bind_pos < dev_pos, "ro-bind must come before dev");
    assert!(dev_pos < proc_pos, "dev must come before proc");
    assert!(proc_pos < tmpfs_pos, "proc must come before tmpfs");
    assert!(
        tmpfs_pos < new_session_pos,
        "tmpfs must come before new-session"
    );
    assert!(
        new_session_pos < die_with_parent_pos,
        "new-session must come before die-with-parent"
    );
    assert!(
        die_with_parent_pos < chdir_pos,
        "die-with-parent must come before chdir"
    );
}

#[test]
fn test_build_bwrap_args_includes_all_required_flags() {
    let config = BwrapConfig {
        allowed_dirs: vec![],
        sensitive_patterns: vec![],
        cwd: "/tmp/test".to_string(),
    };
    let args = build_bwrap_args(&config);

    assert!(args.contains(&"bwrap".to_string()));
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
        allowed_dirs: vec!["/nonexistent/path/xyz/abc/123".to_string()],
        sensitive_patterns: vec![],
        cwd: "/tmp".to_string(),
    };
    let args = build_bwrap_args(&config);

    // Count --bind flags (should be 0 since the dir doesn't exist)
    let bind_count = args.iter().filter(|a| a.as_str() == "--bind").count();
    assert_eq!(bind_count, 0, "Should not have --bind for nonexistent dirs");
}

#[test]
fn test_build_bwrap_args_includes_existing_dirs() {
    let config = BwrapConfig {
        allowed_dirs: vec!["/tmp".to_string()],
        sensitive_patterns: vec![],
        cwd: "/tmp".to_string(),
    };
    let args = build_bwrap_args(&config);

    // /tmp exists, so --bind should be present
    assert!(
        args.contains(&"--bind".to_string()),
        "Should have --bind for /tmp"
    );
}

#[test]
fn test_resolve_sensitive_files_finds_matching_files() {
    // Create temp directory with test sensitive files
    let tmp = std::env::temp_dir().join("lukan_sandbox_test_resolve");
    let _ = fs::create_dir_all(&tmp);

    let env_file = tmp.join(".env");
    let env_local = tmp.join(".env.local");
    let key_file = tmp.join("server.key");
    let normal_file = tmp.join("readme.txt");

    let _ = fs::write(&env_file, "SECRET=value");
    let _ = fs::write(&env_local, "LOCAL_SECRET=value");
    let _ = fs::write(&key_file, "private key data");
    let _ = fs::write(&normal_file, "hello");

    let patterns = vec![".env*".to_string(), "*.key".to_string()];
    let dirs = vec![tmp.to_string_lossy().to_string()];
    let files = resolve_sensitive_files(&patterns, &dirs);

    // Should find .env, .env.local, and server.key
    assert!(
        files.iter().any(|f| f.contains(".env.local")),
        "Should find .env.local"
    );
    assert!(
        files.iter().any(|f| f.ends_with(".env")),
        "Should find .env"
    );
    assert!(
        files.iter().any(|f| f.contains("server.key")),
        "Should find server.key"
    );
    // Should NOT find readme.txt
    assert!(
        !files.iter().any(|f| f.contains("readme.txt")),
        "Should not find readme.txt"
    );

    // Cleanup
    let _ = fs::remove_file(&env_file);
    let _ = fs::remove_file(&env_local);
    let _ = fs::remove_file(&key_file);
    let _ = fs::remove_file(&normal_file);
    let _ = fs::remove_dir(&tmp);
}

#[test]
fn test_resolve_sensitive_files_returns_empty_for_no_matches() {
    let patterns = vec!["*.nonexistent_extension_xyz123".to_string()];
    let dirs = vec!["/tmp".to_string()];
    let files = resolve_sensitive_files(&patterns, &dirs);
    // Very unlikely to match anything
    assert!(
        files.is_empty(),
        "Should return empty for patterns with no matches"
    );
}

#[test]
fn test_resolve_sensitive_files_deduplicates_dirs() {
    let tmp = std::env::temp_dir().join("lukan_sandbox_test_dedup");
    let _ = fs::create_dir_all(&tmp);
    let test_file = tmp.join(".env");
    let _ = fs::write(&test_file, "TEST=1");

    let dir_str = tmp.to_string_lossy().to_string();
    let patterns = vec![".env*".to_string()];
    // Same directory repeated
    let dirs = vec![dir_str.clone(), dir_str.clone(), dir_str];
    let files = resolve_sensitive_files(&patterns, &dirs);

    // Should not have duplicates
    let count = files.iter().filter(|f| f.ends_with(".env")).count();
    assert_eq!(count, 1, "Should deduplicate results");

    // Cleanup
    let _ = fs::remove_file(&test_file);
    let _ = fs::remove_dir(&tmp);
}

#[test]
fn test_diagnose_bwrap_returns_nonempty_string() {
    let msg = diagnose_bwrap();
    assert!(
        !msg.is_empty(),
        "Diagnosis should return a non-empty string"
    );
}

#[test]
fn test_has_apparmor_profile_returns_bool() {
    let result = has_apparmor_profile();
    // Just verify it doesn't panic
    assert!(result || !result);
}

// ── Conditional integration tests (only run when bwrap is available) ──────

#[test]
fn test_bwrap_simple_command_execution() {
    if !is_bwrap_available() {
        eprintln!("Skipping: bwrap not available");
        return;
    }

    let config = BwrapConfig {
        allowed_dirs: vec![],
        sensitive_patterns: vec![],
        cwd: "/tmp".to_string(),
    };
    let args = build_bwrap_args(&config);

    // Run "echo hello" inside bwrap
    let output = std::process::Command::new(&args[0])
        .args(&args[1..])
        .arg("--")
        .arg("bash")
        .arg("-c")
        .arg("echo hello")
        .output()
        .expect("Failed to run bwrap");

    assert!(output.status.success(), "bwrap echo should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim() == "hello", "Expected 'hello', got: {stdout}");
}

#[test]
fn test_bwrap_read_only_filesystem() {
    if !is_bwrap_available() {
        eprintln!("Skipping: bwrap not available");
        return;
    }

    let config = BwrapConfig {
        allowed_dirs: vec![],
        sensitive_patterns: vec![],
        cwd: "/tmp".to_string(),
    };
    let args = build_bwrap_args(&config);

    // Attempt to write to /usr (should fail -- read-only)
    let output = std::process::Command::new(&args[0])
        .args(&args[1..])
        .arg("--")
        .arg("bash")
        .arg("-c")
        .arg("touch /usr/test_file_should_fail 2>&1")
        .output()
        .expect("Failed to run bwrap");

    assert!(
        !output.status.success() || {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            stderr.contains("Read-only")
                || stderr.contains("Permission denied")
                || stdout.contains("Read-only")
                || stdout.contains("Permission denied")
        },
        "Write to read-only path should fail"
    );
}

#[test]
fn test_bwrap_writable_allowed_dirs() {
    if !is_bwrap_available() {
        eprintln!("Skipping: bwrap not available");
        return;
    }

    // Create a temp directory that will be writable
    let tmp_dir = std::env::temp_dir().join("lukan_bwrap_writable_test");
    let _ = fs::create_dir_all(&tmp_dir);

    let config = BwrapConfig {
        allowed_dirs: vec![tmp_dir.to_string_lossy().to_string()],
        sensitive_patterns: vec![],
        cwd: "/tmp".to_string(),
    };
    let args = build_bwrap_args(&config);

    // Write should succeed in allowed dir
    let test_file = tmp_dir.join("bwrap_write_test.txt");
    let cmd = format!("echo 'test' > {}", test_file.display());
    let output = std::process::Command::new(&args[0])
        .args(&args[1..])
        .arg("--")
        .arg("bash")
        .arg("-c")
        .arg(&cmd)
        .output()
        .expect("Failed to run bwrap");

    assert!(
        output.status.success(),
        "Write to allowed dir should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the file was actually created
    assert!(test_file.exists(), "File should exist after bwrap write");

    // Cleanup
    let _ = fs::remove_file(&test_file);
    let _ = fs::remove_dir(&tmp_dir);
}

#[test]
fn test_bwrap_sensitive_file_blocking() {
    if !is_bwrap_available() {
        eprintln!("Skipping: bwrap not available");
        return;
    }

    // Create a temp directory with a sensitive file
    let tmp_dir = std::env::temp_dir().join("lukan_bwrap_sensitive_test");
    let _ = fs::create_dir_all(&tmp_dir);
    let env_file = tmp_dir.join(".env");
    fs::write(&env_file, "SECRET=supersecret").expect("Failed to write test .env");

    let config = BwrapConfig {
        allowed_dirs: vec![tmp_dir.to_string_lossy().to_string()],
        sensitive_patterns: vec![".env*".to_string()],
        cwd: tmp_dir.to_string_lossy().to_string(),
    };
    let args = build_bwrap_args(&config);

    // Read the sensitive file inside bwrap -- should be empty (overlaid with /dev/null)
    let cmd = format!("cat {}", env_file.display());
    let output = std::process::Command::new(&args[0])
        .args(&args[1..])
        .arg("--")
        .arg("bash")
        .arg("-c")
        .arg(&cmd)
        .output()
        .expect("Failed to run bwrap");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("supersecret"),
        "Sensitive file content should be blocked. Got: {stdout}"
    );

    // Cleanup
    let _ = fs::remove_file(&env_file);
    let _ = fs::remove_dir(&tmp_dir);
}
