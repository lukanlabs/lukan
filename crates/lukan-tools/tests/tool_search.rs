mod test_helpers;

use lukan_tools::create_default_registry;
use lukan_tools::tool_search::search_deferred_tools;
use serde_json::json;
use test_helpers::make_tool_context;

#[test]
fn default_registry_exposes_tool_search_and_reflects_runtime_deferred_tools() {
    let registry = create_default_registry();

    let tool_search = registry.get("ToolSearch").expect("ToolSearch should exist");
    assert!(tool_search.is_read_only());
    assert!(tool_search.is_concurrency_safe());

    let deferred = registry.deferred_definitions();
    let web_search_registered = registry.get("WebSearch").is_some();

    assert!(deferred.iter().any(|d| d.name == "WebFetch"));
    assert!(
        !registry
            .default_definitions()
            .iter()
            .any(|d| d.name == "WebFetch")
    );
    assert_eq!(
        deferred.iter().any(|d| d.name == "WebSearch"),
        web_search_registered
    );
    assert!(
        !registry
            .default_definitions()
            .iter()
            .any(|d| d.name == "WebSearch")
    );
}

#[test]
fn search_deferred_tools_matches_current_runtime_registration() {
    let registry = create_default_registry();
    let fetch_results = search_deferred_tools(&registry, "fetch url page", 5);
    assert!(fetch_results.iter().any(|r| r.name == "WebFetch"));

    let results = search_deferred_tools(&registry, "web search information", 5);
    let web_search_registered = registry.get("WebSearch").is_some();

    if web_search_registered {
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "WebSearch");
    } else {
        assert!(results.is_empty());
    }
}

#[tokio::test]
async fn tool_search_returns_runtime_matching_deferred_tools_only() {
    let registry = create_default_registry();
    let tool = registry.get("ToolSearch").unwrap();
    let ctx = make_tool_context(&std::env::temp_dir());

    let fetch_result = tool
        .execute(
            json!({"query": "fetch a webpage by url", "max_results": 5}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(!fetch_result.is_error);
    assert!(fetch_result.content.contains("WebFetch"));

    let result = tool
        .execute(json!({"query": "search the web", "max_results": 5}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    if registry.get("WebSearch").is_some() {
        assert!(result.content.contains("WebSearch"));
    } else {
        assert_eq!(result.content, "No matching deferred tools found.");
    }
}

#[tokio::test]
async fn tool_search_returns_no_matches_message_when_empty() {
    let registry = create_default_registry();
    let tool = registry.get("ToolSearch").unwrap();
    let ctx = make_tool_context(&std::env::temp_dir());

    let result = tool
        .execute(
            json!({"query": "totally-unknown-specialized-capability"}),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(result.content, "No matching deferred tools found.");
}
