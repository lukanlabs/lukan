mod test_helpers;

use lukan_tools::create_default_registry;
use serde_json::json;
use test_helpers::{make_tool_context, mark_file_as_read};

fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lukan-write-file-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn write_file_validation_rejects_empty_content() {
    let dir = test_dir("empty-content");
    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();

    let result = registry
        .execute(
            "WriteFile",
            json!({
                "file_path": dir.join("new.txt").to_string_lossy(),
                "content": ""
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Content is empty"));
}

#[tokio::test]
async fn write_file_validation_requires_existing_file_to_be_read_first() {
    let dir = test_dir("requires-read-first");
    let file_path = dir.join("existing.txt");
    tokio::fs::write(&file_path, "old\n").await.unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();

    let result = registry
        .execute(
            "WriteFile",
            json!({
                "file_path": file_path.to_string_lossy(),
                "content": "new\n"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("has not been read yet"));
}

#[tokio::test]
async fn write_file_validation_allows_existing_file_after_read() {
    let dir = test_dir("after-read");
    let file_path = dir.join("existing.txt");
    tokio::fs::write(&file_path, "old\n").await.unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;
    let registry = create_default_registry();

    let result = registry
        .execute(
            "WriteFile",
            json!({
                "file_path": file_path.to_string_lossy(),
                "content": "new\n"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(updated, "new\n");
}
