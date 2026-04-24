mod test_helpers;

use lukan_tools::create_default_registry;
use serde_json::json;
use test_helpers::make_tool_context;

#[tokio::test]
async fn web_fetch_validation_rejects_invalid_url() {
    let dir = std::env::temp_dir();
    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();

    let result = registry
        .execute("WebFetch", json!({"url": "not-a-url"}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Invalid URL"));
}

#[tokio::test]
async fn web_fetch_validation_rejects_private_host() {
    let dir = std::env::temp_dir();
    let ctx = make_tool_context(&dir);
    let registry = create_default_registry();

    let result = registry
        .execute("WebFetch", json!({"url": "http://127.0.0.1/test"}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(
        result
            .content
            .contains("Access to private/local addresses is blocked")
    );
}
