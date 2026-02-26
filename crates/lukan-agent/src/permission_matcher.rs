//! Permission matching for tool execution.
//!
//! Evaluates whether a tool call should be allowed, denied, or require
//! user approval based on the current permission mode and configured
//! deny/ask/allow lists.

use globset::GlobBuilder;
use lukan_core::config::types::{PermissionMode, PermissionsConfig};

/// Verdict for a tool invocation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolVerdict {
    /// Execute immediately
    Allow,
    /// Ask the user before executing
    Ask,
    /// Block execution entirely
    Deny,
}

/// Tools that are always safe (read-only or low-risk)
const SAFE_TOOLS: &[&str] = &[
    "ReadFile",
    "Grep",
    "Glob",
    "WebFetch",
    "Explore",
    "TaskAdd",
    "TaskList",
    "TaskUpdate",
    "PlannerQuestion",
    "SubmitPlan",
    "LoadSkill",
];

/// Browser tools — only treated as safe when browser mode is enabled
const BROWSER_TOOLS: &[&str] = &[
    "BrowserNavigate",
    "BrowserSnapshot",
    "BrowserScreenshot",
    "BrowserClick",
    "BrowserType",
    "BrowserEvaluate",
    "BrowserTabs",
    "BrowserNewTab",
    "BrowserSwitchTab",
    "BrowserSavePDF",
];

/// Tools allowed in planner mode (read-only exploration + planner-specific)
const PLANNER_WHITELIST: &[&str] = &[
    "ReadFile",
    "Grep",
    "Glob",
    "WebFetch",
    "Explore",
    "TaskAdd",
    "TaskList",
    "TaskUpdate",
    "PlannerQuestion",
    "SubmitPlan",
    "LoadSkill",
];

/// Permission matcher: evaluates tool calls against mode + config rules
pub struct PermissionMatcher {
    mode: PermissionMode,
    deny: Vec<PatternRule>,
    ask: Vec<PatternRule>,
    allow: Vec<PatternRule>,
    /// When true, browser tools are treated as safe (auto-allow)
    browser_tools: bool,
}

/// A parsed permission pattern rule
#[derive(Debug)]
struct PatternRule {
    tool_name: String,
    /// Optional argument pattern (e.g. `git:*` for Bash, `**/.env` for file tools)
    arg_pattern: Option<String>,
}

impl PatternRule {
    fn parse(pattern: &str) -> Self {
        // Format: "ToolName" or "ToolName(arg_pattern)"
        if let Some(paren_start) = pattern.find('(')
            && let Some(paren_end) = pattern.rfind(')')
        {
            let tool_name = pattern[..paren_start].to_string();
            let arg_pattern = pattern[paren_start + 1..paren_end].to_string();
            return Self {
                tool_name,
                arg_pattern: Some(arg_pattern),
            };
        }
        Self {
            tool_name: pattern.to_string(),
            arg_pattern: None,
        }
    }

    /// Check if this rule matches a given tool name and input
    fn matches(&self, tool_name: &str, tool_input: &serde_json::Value) -> bool {
        if self.tool_name != tool_name {
            return false;
        }

        let Some(ref arg_pattern) = self.arg_pattern else {
            // Tool-name-only rule: matches any invocation
            return true;
        };

        match tool_name {
            "Bash" => {
                // For Bash: match against the `command` field
                let command = tool_input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match_bash_pattern(arg_pattern, command)
            }
            "ReadFile" | "WriteFile" | "EditFile" => {
                // For file tools: match against `file_path` with glob
                let file_path = tool_input
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match_glob_pattern(arg_pattern, file_path)
            }
            "Grep" | "Glob" => {
                // For search tools: match against `path` or `pattern`
                let path = tool_input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match_glob_pattern(arg_pattern, path)
            }
            _ => {
                // For other tools: no argument matching, treat as tool-name-only
                true
            }
        }
    }
}

/// Match a Bash command pattern like `git:*` or `npm:install`
///
/// Pattern syntax: `prefix:suffix`
/// - `git:*` matches any command starting with `git`
/// - `git:push` matches commands starting with `git push`
/// - `npm:*` matches any command starting with `npm`
fn match_bash_pattern(pattern: &str, command: &str) -> bool {
    if let Some((prefix, suffix)) = pattern.split_once(':') {
        let cmd_parts: Vec<&str> = command.split_whitespace().collect();
        let first_word = cmd_parts.first().copied().unwrap_or("");

        if first_word != prefix {
            return false;
        }

        if suffix == "*" {
            return true;
        }

        // Check if the second word matches the suffix
        let second_word = cmd_parts.get(1).copied().unwrap_or("");
        second_word == suffix
    } else {
        // No colon: match as prefix of the entire command
        command.starts_with(pattern)
    }
}

/// Match a file path against a glob pattern
fn match_glob_pattern(pattern: &str, path: &str) -> bool {
    if let Ok(glob) = GlobBuilder::new(pattern).literal_separator(false).build() {
        glob.compile_matcher().is_match(path)
    } else {
        // Fallback: simple string contains
        path.contains(pattern)
    }
}

impl PermissionMatcher {
    /// Create a new matcher from mode and config
    pub fn new(mode: PermissionMode, config: &PermissionsConfig) -> Self {
        Self {
            mode,
            deny: config.deny.iter().map(|p| PatternRule::parse(p)).collect(),
            ask: config.ask.iter().map(|p| PatternRule::parse(p)).collect(),
            allow: config.allow.iter().map(|p| PatternRule::parse(p)).collect(),
            browser_tools: false,
        }
    }

    /// Determine the verdict for a tool call
    pub fn verdict(&self, tool_name: &str, tool_input: &serde_json::Value) -> ToolVerdict {
        // 1. ALL modes: check deny list first
        if self.deny.iter().any(|r| r.matches(tool_name, tool_input)) {
            return ToolVerdict::Deny;
        }

        match self.mode {
            // 2. Planner: only allow read-only tools
            PermissionMode::Planner => {
                if PLANNER_WHITELIST.contains(&tool_name) {
                    ToolVerdict::Allow
                } else {
                    ToolVerdict::Deny
                }
            }

            // 3. Skip: allow everything (not denied)
            PermissionMode::Skip => ToolVerdict::Allow,

            // 4. Manual: ask for everything (not denied)
            PermissionMode::Manual => ToolVerdict::Ask,

            // 5. Auto: ask list → Ask; allow list → Allow; safe → Allow; default → Ask
            PermissionMode::Auto => {
                if self.ask.iter().any(|r| r.matches(tool_name, tool_input)) {
                    return ToolVerdict::Ask;
                }
                if self.allow.iter().any(|r| r.matches(tool_name, tool_input)) {
                    return ToolVerdict::Allow;
                }
                if SAFE_TOOLS.contains(&tool_name) {
                    return ToolVerdict::Allow;
                }
                if self.browser_tools && BROWSER_TOOLS.contains(&tool_name) {
                    return ToolVerdict::Allow;
                }
                ToolVerdict::Ask
            }
        }
    }

    /// Get the current permission mode
    pub fn mode(&self) -> &PermissionMode {
        &self.mode
    }

    /// Update the permission mode at runtime
    pub fn set_mode(&mut self, mode: PermissionMode) {
        self.mode = mode;
    }

    /// Enable browser tools as safe (auto-allow without asking)
    pub fn enable_browser_tools(&mut self) {
        self.browser_tools = true;
    }

    /// Hot-add a parsed allow rule so the matcher immediately recognizes it
    pub fn add_allow_rule(&mut self, pattern: &str) {
        self.allow.push(PatternRule::parse(pattern));
    }
}

/// Generate a broad allow pattern from a tool name and its input.
///
/// For Bash, generates `Bash(prefix:*)` based on the first word of the command.
/// For other tools, generates a tool-name-only pattern.
pub fn generate_allow_pattern(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let first_word = cmd.split_whitespace().next().unwrap_or("");
            format!("Bash({first_word}:*)")
        }
        _ => tool_name.to_string(),
    }
}

/// The planner whitelist, exported for tool definition filtering
pub const PLANNER_TOOL_WHITELIST: &[&str] = PLANNER_WHITELIST;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn default_config() -> PermissionsConfig {
        PermissionsConfig::default()
    }

    #[test]
    fn skip_mode_allows_all() {
        let matcher = PermissionMatcher::new(PermissionMode::Skip, &default_config());
        assert_eq!(
            matcher.verdict("Bash", &json!({"command": "rm -rf /"})),
            ToolVerdict::Allow
        );
    }

    #[test]
    fn manual_mode_asks_all() {
        let matcher = PermissionMatcher::new(PermissionMode::Manual, &default_config());
        assert_eq!(
            matcher.verdict("ReadFile", &json!({"file_path": "test.rs"})),
            ToolVerdict::Ask
        );
    }

    #[test]
    fn auto_mode_safe_tools() {
        let matcher = PermissionMatcher::new(PermissionMode::Auto, &default_config());
        assert_eq!(
            matcher.verdict("ReadFile", &json!({"file_path": "test.rs"})),
            ToolVerdict::Allow
        );
        assert_eq!(
            matcher.verdict("Grep", &json!({"pattern": "foo"})),
            ToolVerdict::Allow
        );
        assert_eq!(
            matcher.verdict("Bash", &json!({"command": "echo hi"})),
            ToolVerdict::Ask
        );
    }

    #[test]
    fn deny_list_overrides_all() {
        let config = PermissionsConfig {
            deny: vec!["Bash".to_string()],
            ..default_config()
        };
        let matcher = PermissionMatcher::new(PermissionMode::Skip, &config);
        assert_eq!(
            matcher.verdict("Bash", &json!({"command": "ls"})),
            ToolVerdict::Deny
        );
    }

    #[test]
    fn planner_mode_whitelist() {
        let matcher = PermissionMatcher::new(PermissionMode::Planner, &default_config());
        assert_eq!(
            matcher.verdict("ReadFile", &json!({"file_path": "test.rs"})),
            ToolVerdict::Allow
        );
        assert_eq!(
            matcher.verdict("Bash", &json!({"command": "ls"})),
            ToolVerdict::Deny
        );
        assert_eq!(
            matcher.verdict("WriteFile", &json!({"file_path": "test.rs"})),
            ToolVerdict::Deny
        );
    }

    #[test]
    fn bash_pattern_matching() {
        let config = PermissionsConfig {
            allow: vec!["Bash(git:*)".to_string()],
            ..default_config()
        };
        let matcher = PermissionMatcher::new(PermissionMode::Auto, &config);
        assert_eq!(
            matcher.verdict("Bash", &json!({"command": "git status"})),
            ToolVerdict::Allow
        );
        assert_eq!(
            matcher.verdict("Bash", &json!({"command": "git push"})),
            ToolVerdict::Allow
        );
        assert_eq!(
            matcher.verdict("Bash", &json!({"command": "rm -rf /"})),
            ToolVerdict::Ask
        );
    }

    #[test]
    fn file_glob_pattern() {
        let config = PermissionsConfig {
            deny: vec!["ReadFile(**/.env)".to_string()],
            ..default_config()
        };
        let matcher = PermissionMatcher::new(PermissionMode::Auto, &config);
        assert_eq!(
            matcher.verdict("ReadFile", &json!({"file_path": "/home/user/.env"})),
            ToolVerdict::Deny
        );
        assert_eq!(
            matcher.verdict("ReadFile", &json!({"file_path": "src/main.rs"})),
            ToolVerdict::Allow
        );
    }
}
