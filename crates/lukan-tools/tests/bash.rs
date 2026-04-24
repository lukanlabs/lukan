mod test_helpers;

use lukan_tools::ToolContext;
use lukan_tools::redact_env_vars;
use serde_json::json;
use test_helpers::make_tool_context;
use tokio_util::sync::CancellationToken;

fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lukan-bash-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bash_tool<'a>(registry: &'a lukan_tools::ToolRegistry) -> &'a dyn lukan_tools::Tool {
    registry.get("Bash").unwrap()
}

#[tokio::test]
async fn bash_executes_simple_command_successfully() {
    let dir = test_dir("simple-success");
    let ctx = make_tool_context(&dir);
    let registry = lukan_tools::create_default_registry();

    let result = bash_tool(&registry)
        .execute(json!({"command": "printf 'hello'"}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("hello"));
}

#[tokio::test]
async fn bash_returns_error_result_for_failing_command() {
    let dir = test_dir("failing-command");
    let ctx = make_tool_context(&dir);
    let registry = lukan_tools::create_default_registry();

    let result = bash_tool(&registry)
        .execute(json!({"command": "bash -c 'echo boom >&2; exit 7'"}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("boom") || result.content.contains("exit code"));
}

#[tokio::test]
async fn bash_executes_in_specified_cwd() {
    let dir = test_dir("cwd");
    let marker = dir.join("marker.txt");
    std::fs::write(&marker, "cwd-marker\n").unwrap();

    let ctx = make_tool_context(&dir);
    let registry = lukan_tools::create_default_registry();

    let result = bash_tool(&registry)
        .execute(json!({"command": "pwd && ls"}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains(&dir.to_string_lossy().to_string()));
    assert!(result.content.contains("marker.txt"));
}

#[tokio::test]
async fn bash_returns_timeout_error_when_command_exceeds_timeout() {
    let dir = test_dir("timeout");
    let ctx = make_tool_context(&dir);
    let registry = lukan_tools::create_default_registry();

    let result = bash_tool(&registry)
        .execute(
            json!({
                "command": "sleep 1",
                "timeout": 10
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("timed out"));
}

#[tokio::test]
async fn bash_background_mode_returns_pid_immediately() {
    let dir = test_dir("background-pid");
    let ctx = make_tool_context(&dir);
    let registry = lukan_tools::create_default_registry();

    let result = bash_tool(&registry)
        .execute(
            json!({
                "command": "sleep 0.2; echo done",
                "background": true
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(
        result.content.contains("Background process started") || result.content.contains("PID")
    );
}

#[tokio::test]
async fn bash_wait_pid_returns_output_after_background_process_finishes() {
    let dir = test_dir("wait-pid");
    let ctx = make_tool_context(&dir);
    let registry = lukan_tools::create_default_registry();

    let launch = bash_tool(&registry)
        .execute(
            json!({
                "command": "sleep 0.1; echo bg-finished",
                "background": true
            }),
            &ctx,
        )
        .await
        .unwrap();

    let pid: u32 = launch
        .content
        .split_whitespace()
        .find_map(|part| part.parse::<u32>().ok())
        .expect("background output should include pid");

    let waited = bash_tool(&registry)
        .execute(
            json!({
                "command": "ignored",
                "wait_pid": pid,
                "timeout": 2000
            }),
            &ctx,
        )
        .await
        .unwrap();

    assert!(!waited.is_error);
    assert!(waited.content.contains("bg-finished") || waited.content.contains("finished"));
}

#[tokio::test]
async fn bash_can_be_cancelled_via_cancellation_token() {
    let dir = test_dir("cancel");
    let mut ctx: ToolContext = make_tool_context(&dir);
    let token = CancellationToken::new();
    ctx.cancel = Some(token.clone());
    let registry = lukan_tools::create_default_registry();

    let handle = tokio::spawn(async move {
        bash_tool(&registry)
            .execute(json!({"command": "sleep 5"}), &ctx)
            .await
            .unwrap()
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    token.cancel();
    let result = handle.await.unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Cancelled by user"));
}

#[tokio::test]
async fn bash_receives_extra_env_variables() {
    let dir = test_dir("extra-env");
    let mut ctx = make_tool_context(&dir);
    ctx.extra_env
        .insert("LUKAN_TEST_VAR".into(), "xyz123".into());
    let registry = lukan_tools::create_default_registry();

    let result = bash_tool(&registry)
        .execute(json!({"command": "printf \"$LUKAN_TEST_VAR\""}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("xyz123"));
}

#[test]
fn redact_env_vars_replaces_secret_values_in_output() {
    unsafe {
        std::env::set_var("LUKAN_SECRET_FOR_TEST", "super-secret-value");
    }
    let redacted = redact_env_vars(
        "token=super-secret-value",
        &["LUKAN_SECRET_FOR_TEST".to_string()],
    );
    assert_eq!(redacted, "token=[REDACTED:LUKAN_SECRET_FOR_TEST]");
}
