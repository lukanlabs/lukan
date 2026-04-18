mod test_helpers;

use lukan_tools::create_default_registry;
use serde_json::json;
use std::time::{Duration, SystemTime};
use test_helpers::{make_restricted_tool_context, make_tool_context};

fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lukan-glob-tool-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_mtime(path: &std::path::Path, secs_since_epoch: u64) {
    let filetime = std::process::Command::new("touch")
        .arg("-t")
        .arg(format_timestamp(secs_since_epoch))
        .arg(path)
        .status()
        .expect("touch command should run");
    assert!(filetime.success());
}

fn format_timestamp(secs_since_epoch: u64) -> String {
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(secs_since_epoch);
    let datetime: chrono::DateTime<chrono::Utc> = time.into();
    datetime.format("%Y%m%d%H%M.%S").to_string()
}

#[tokio::test]
async fn glob_finds_matching_files_under_cwd_by_default() {
    let dir = test_dir("default-cwd");
    std::fs::create_dir_all(dir.join("src/nested")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("src/nested/lib.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(dir.join("README.md"), "readme\n").unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(json!({"pattern": "src/**/*.rs"}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("src/main.rs"));
    assert!(result.content.contains("src/nested/lib.rs"));
    assert!(!result.content.contains("README.md"));
}

#[tokio::test]
async fn glob_supports_relative_base_path() {
    let dir = test_dir("relative-base");
    std::fs::create_dir_all(dir.join("src/nested")).unwrap();
    std::fs::write(dir.join("src/nested/file.rs"), "pub fn g() {}\n").unwrap();
    std::fs::write(dir.join("other.rs"), "fn other() {}\n").unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(json!({"pattern": "**/*.rs", "path": "src"}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("src/nested/file.rs"));
    assert!(!result.content.contains("other.rs"));
}

#[tokio::test]
async fn glob_returns_error_for_invalid_pattern() {
    let dir = test_dir("invalid-pattern");
    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(json!({"pattern": "["}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Invalid glob pattern"));
}

#[tokio::test]
async fn glob_fails_when_base_path_is_outside_allowed_paths() {
    let dir = test_dir("allowed-root");
    let outside_dir = test_dir("outside-root");
    std::fs::write(outside_dir.join("file.rs"), "fn x() {}\n").unwrap();

    let ctx = make_restricted_tool_context(&dir, vec![dir.clone()]);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(
            json!({
                "pattern": "**/*.rs",
                "path": outside_dir.to_string_lossy()
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("outside allowed directories"));
}

#[tokio::test]
async fn glob_skips_sensitive_paths() {
    let dir = test_dir("sensitive-skip");
    std::fs::create_dir_all(dir.join("visible")).unwrap();
    std::fs::create_dir_all(dir.join(".ssh")).unwrap();
    std::fs::write(dir.join("visible/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join(".ssh/secret.rs"), "secret\n").unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(json!({"pattern": "**/*.rs"}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("visible/main.rs"));
    assert!(!result.content.contains(".ssh/secret.rs"));
}

#[tokio::test]
async fn glob_returns_results_sorted_by_newest_first() {
    let dir = test_dir("mtime-order");
    let older = dir.join("older.rs");
    let newer = dir.join("newer.rs");
    std::fs::write(&older, "old\n").unwrap();
    std::fs::write(&newer, "new\n").unwrap();

    set_mtime(&older, 1_700_000_000);
    set_mtime(&newer, 1_800_000_000);

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(json!({"pattern": "**/*.rs"}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    let newer_index = result.content.find("newer.rs").unwrap();
    let older_index = result.content.find("older.rs").unwrap();
    assert!(newer_index < older_index, "newer file should appear first");
}

#[tokio::test]
async fn glob_respects_max_results_and_reports_truncation() {
    let dir = test_dir("max-results");
    std::fs::write(dir.join("a.rs"), "a\n").unwrap();
    std::fs::write(dir.join("b.rs"), "b\n").unwrap();
    std::fs::write(dir.join("c.rs"), "c\n").unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(json!({"pattern": "**/*.rs", "max_results": 2}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("showing 2"));
}

#[tokio::test]
async fn glob_returns_error_when_base_directory_does_not_exist() {
    let dir = test_dir("missing-base");
    let missing = dir.join("does-not-exist");
    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();

    let result = registry
        .execute(
            "Glob",
            json!({"pattern": "**/*.rs", "path": missing.to_string_lossy()}),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Base directory does not exist"));
}

#[tokio::test]
async fn glob_returns_no_files_matched_when_empty() {
    let dir = test_dir("no-matches");
    std::fs::write(dir.join("README.md"), "hi\n").unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("Glob").unwrap();

    let result = tool
        .execute(json!({"pattern": "**/*.rs"}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(result.content, "No files matched.");
}
