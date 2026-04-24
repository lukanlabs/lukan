mod test_helpers;

use lukan_tools::create_default_registry;
use serde_json::json;
use test_helpers::{make_restricted_tool_context, make_tool_context, mark_file_as_read};

fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lukan-edit-file-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn edit_file_requires_file_to_have_been_read_first() {
    let dir = test_dir("requires-read-first");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "hello world").await.unwrap();

    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_text": "hello",
                "new_text": "goodbye"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("File has not been read yet"));
}

#[tokio::test]
async fn edit_file_succeeds_when_file_was_previously_read() {
    let dir = test_dir("success-after-read");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "hello world").await.unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;

    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_text": "hello",
                "new_text": "goodbye"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(updated, "goodbye world");
}

#[tokio::test]
async fn edit_file_fails_outside_allowed_paths() {
    let dir = test_dir("allowed-root");
    let outside_dir = test_dir("outside-root");
    let file_path = outside_dir.join("sample.txt");
    tokio::fs::write(&file_path, "hello world").await.unwrap();

    let ctx = make_restricted_tool_context(&dir, vec![dir.clone()]);
    mark_file_as_read(&ctx, &file_path).await;

    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_text": "hello",
                "new_text": "goodbye"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("outside allowed directories"));
}

#[tokio::test]
async fn edit_file_fails_for_sensitive_paths() {
    let dir = test_dir("sensitive-path");
    let file_path = dir.join(".env");
    tokio::fs::write(&file_path, "SECRET=123").await.unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;

    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_text": "123",
                "new_text": "456"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Access denied"));
}

#[tokio::test]
async fn multi_edit_applies_all_edits_in_order() {
    let dir = test_dir("multi-edit-success");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "alpha\nbeta\ngamma\n")
        .await
        .unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;

    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "edits": [
                    {"old_text": "alpha", "new_text": "one"},
                    {"old_text": "gamma", "new_text": "three"}
                ]
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(updated, "one\nbeta\nthree\n");
}

#[tokio::test]
async fn multi_edit_fails_atomically_when_one_edit_is_invalid() {
    let dir = test_dir("multi-edit-atomic");
    let file_path = dir.join("sample.txt");
    let original = "alpha\nbeta\ngamma\n";
    tokio::fs::write(&file_path, original).await.unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;

    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "edits": [
                    {"old_text": "alpha", "new_text": "one"},
                    {"old_text": "missing", "new_text": "boom"}
                ]
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("edits[1] failed"));
    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(updated, original);
}

#[tokio::test]
async fn successful_edit_updates_disk_contents() {
    let dir = test_dir("disk-contents");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "before after").await.unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;

    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let _ = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_text": "before",
                "new_text": "during"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let updated = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(updated, "during after");
}

#[tokio::test]
async fn edit_file_validation_rejects_empty_old_text() {
    let dir = test_dir("empty-old-text");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "hello world").await.unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;
    let registry = create_default_registry();

    let result = registry
        .execute(
            "EditFile",
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_text": "",
                "new_text": "goodbye"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("old_text cannot be empty"));
}

#[tokio::test]
async fn successful_edit_returns_diff_output() {
    let dir = test_dir("diff-output");
    let file_path = dir.join("sample.txt");
    tokio::fs::write(&file_path, "before after\n")
        .await
        .unwrap();

    let ctx = make_tool_context(&dir);
    mark_file_as_read(&ctx, &file_path).await;

    let registry = create_default_registry();
    let tool = registry.get("EditFile").unwrap();
    let result = tool
        .execute(
            json!({
                "file_path": file_path.to_string_lossy(),
                "old_text": "before",
                "new_text": "during"
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("Added") || result.content.contains("Removed"));
    let diff = result.diff.expect("expected diff output");
    assert!(diff.contains("---"));
    assert!(diff.contains("@@"));
    assert!(diff.contains("-before after"));
    assert!(diff.contains("+during after"));
}
