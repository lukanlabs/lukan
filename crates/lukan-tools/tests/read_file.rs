mod test_helpers;

use lukan_tools::create_default_registry;
use serde_json::json;
use test_helpers::{make_restricted_tool_context, make_tool_context};

fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lukan-read-file-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn read_file_returns_numbered_lines() {
    let dir = test_dir("numbered-lines");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "first\nsecond\nthird\n")
        .await
        .unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("1\tfirst"));
    assert!(result.content.contains("2\tsecond"));
    assert!(result.content.contains("3\tthird"));
}

#[tokio::test]
async fn read_file_honors_offset_and_limit() {
    let dir = test_dir("offset-limit");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "one\ntwo\nthree\nfour\nfive\n")
        .await
        .unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "offset": 2,
                "limit": 2
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("2\ttwo"));
    assert!(result.content.contains("3\tthree"));
    assert!(!result.content.contains("1\tone"));
    assert!(!result.content.contains("4\tfour"));
    assert!(
        result.content.contains("... (3 more lines, 5 total)")
            || result.content.contains("... (2 more lines, 5 total)")
    );
}

#[tokio::test]
async fn read_file_returns_empty_marker_for_empty_file() {
    let dir = test_dir("empty-file");
    let file_path = dir.join("empty.txt");
    tokio::fs::write(&file_path, "").await.unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(result.content, "(empty file)");
}

#[tokio::test]
async fn read_file_returns_already_in_context_stub_when_unchanged() {
    let dir = test_dir("already-in-context");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "alpha\nbeta\n").await.unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let first = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();
    assert!(!first.is_error);

    let second = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(!second.is_error);
    assert!(second.content.contains("file already in context"));
    assert!(second.content.contains("not modified since last read"));
}

#[tokio::test]
async fn read_file_fails_outside_allowed_paths() {
    let dir = test_dir("allowed-root");
    let outside_dir = test_dir("outside-root");
    let file_path = outside_dir.join("sample.txt");
    tokio::fs::write(&file_path, "secret\n").await.unwrap();

    let ctx = make_restricted_tool_context(&dir, vec![dir.clone()]);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("outside allowed directories"));
}

#[tokio::test]
async fn read_file_fails_for_sensitive_paths() {
    let dir = test_dir("sensitive-path");
    let file_path = dir.join(".env");
    tokio::fs::write(&file_path, "TOKEN=abc\n").await.unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Access denied"));
}

#[tokio::test]
async fn read_file_returns_image_payload_for_png() {
    let dir = test_dir("png-image");
    let file_path = dir.join("pixel.png");
    let png_bytes: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xB5,
        0x1C, 0x0C, 0x02, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC,
        0xFF, 0x1F, 0x00, 0x03, 0x03, 0x02, 0x00, 0xEF, 0xBF, 0x55, 0x2D, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    tokio::fs::write(&file_path, png_bytes).await.unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("Image file:"));
    assert!(result.image.is_some());
    assert!(result.image.unwrap().starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn read_file_auto_tails_background_log_without_explicit_offset() {
    let dir = test_dir("bg-log-tail");
    let file_path = dir.join("lukan-bg-123.log");
    let content = (1..=60)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    tokio::fs::write(&file_path, format!("{content}\n"))
        .await
        .unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("background process log"));
    assert!(result.content.contains("line 60"));
    assert!(result.content.contains("11\tline 11"));
    assert!(!result.content.contains("1\tline 10"));
}

#[tokio::test]
async fn read_file_blocks_special_device_paths_in_validation() {
    let dir = test_dir("device-path");
    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();

    let result = registry
        .execute("ReadFiles", json!({"file_path": "/dev/zero"}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("special device path"));
}

#[tokio::test]
async fn read_file_returns_error_for_missing_file() {
    let dir = test_dir("missing-file");
    let file_path = dir.join("missing.txt");

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("ReadFiles").unwrap();

    let result = tool
        .execute(json!({"file_path": file_path.to_string_lossy()}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("No such file or directory"));
}
