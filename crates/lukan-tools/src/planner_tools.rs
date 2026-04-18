//! Planner-mode tools: SubmitPlan and PlannerQuestion.
//!
//! These tools are **intercepted** by the agent loop before execution.
//! The `execute()` implementations return errors — they should never
//! actually be called because the agent loop handles them specially.

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::{Value, json};

use crate::{Tool, ToolContext};

// ── SubmitPlanTool ───────────────────────────────────────────────────────

pub struct SubmitPlanTool;

#[async_trait]
impl Tool for SubmitPlanTool {
    fn name(&self) -> &str {
        "SubmitPlan"
    }

    fn description(&self) -> &str {
        "Submit a structured plan with title, full markdown content, and ordered implementation tasks. Each task has a title and detailed markdown description. The user will review and accept/reject the plan."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short plan title"
                },
                "plan": {
                    "type": "string",
                    "description": "Full markdown plan content"
                },
                "tasks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Short task title for tracking"
                            },
                            "detail": {
                                "type": "string",
                                "description": "Detailed markdown: what to do, files to modify, dependencies, code examples"
                            }
                        },
                        "required": ["title", "detail"]
                    },
                    "minItems": 1,
                    "description": "Ordered implementation tasks"
                }
            },
            "required": ["title", "plan", "tasks"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn search_hint(&self) -> Option<&str> {
        Some("submit a structured implementation plan")
    }

    fn activity_label(&self, _input: &Value) -> Option<String> {
        Some("Submitting plan".to_string())
    }

    async fn execute(&self, _input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // This tool is intercepted by the agent loop before execution.
        Ok(ToolResult::error(
            "SubmitPlan should be intercepted by the agent loop, not executed directly.",
        ))
    }
}

/// Normalize SubmitPlan input: accept `description` as alias for `detail`.
pub fn normalize_submit_plan_input(input: &mut Value) {
    if let Some(tasks) = input.get_mut("tasks").and_then(|v| v.as_array_mut()) {
        for task in tasks {
            if let Some(obj) = task.as_object_mut()
                && !obj.contains_key("detail")
                && let Some(desc) = obj.remove("description")
            {
                obj.insert("detail".to_string(), desc);
            }
        }
    }
}

// ── PlannerQuestionTool ──────────────────────────────────────────────────

pub struct PlannerQuestionTool;

#[async_trait]
impl Tool for PlannerQuestionTool {
    fn name(&self) -> &str {
        "PlannerQuestion"
    }

    fn description(&self) -> &str {
        "Ask the user 1-4 clarifying questions with structured options. Use when you need user input before designing the plan."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "header": {
                                "type": "string",
                                "description": "Short tab label (max ~12 chars)"
                            },
                            "question": {
                                "type": "string",
                                "description": "The full question text"
                            },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" }
                                    },
                                    "required": ["label"]
                                },
                                "minItems": 2,
                                "maxItems": 6,
                                "description": "Suggested answers"
                            },
                            "multiSelect": {
                                "type": "boolean",
                                "description": "Allow multiple selections"
                            }
                        },
                        "required": ["header", "question", "options"]
                    },
                    "minItems": 1,
                    "maxItems": 4,
                    "description": "Questions to ask the user"
                }
            },
            "required": ["questions"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn search_hint(&self) -> Option<&str> {
        Some("ask the user clarifying planner questions")
    }

    fn activity_label(&self, _input: &Value) -> Option<String> {
        Some("Asking planner question".to_string())
    }

    async fn execute(&self, _input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // This tool is intercepted by the agent loop before execution.
        Ok(ToolResult::error(
            "PlannerQuestion should be intercepted by the agent loop, not executed directly.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_submit_plan_input tests ────────────────────────────

    #[test]
    fn normalize_renames_description_to_detail() {
        let mut input = json!({
            "title": "Plan",
            "plan": "Full plan content",
            "tasks": [
                {
                    "title": "Task 1",
                    "description": "Do something"
                }
            ]
        });
        normalize_submit_plan_input(&mut input);
        let tasks = input["tasks"].as_array().unwrap();
        assert_eq!(tasks[0]["detail"], "Do something");
        assert!(tasks[0].get("description").is_none());
    }

    #[test]
    fn normalize_keeps_detail_if_already_present() {
        let mut input = json!({
            "title": "Plan",
            "plan": "Content",
            "tasks": [
                {
                    "title": "Task 1",
                    "detail": "Already correct"
                }
            ]
        });
        normalize_submit_plan_input(&mut input);
        let tasks = input["tasks"].as_array().unwrap();
        assert_eq!(tasks[0]["detail"], "Already correct");
    }

    #[test]
    fn normalize_does_not_override_existing_detail_with_description() {
        let mut input = json!({
            "title": "Plan",
            "plan": "Content",
            "tasks": [
                {
                    "title": "Task 1",
                    "detail": "Existing detail",
                    "description": "Should be ignored"
                }
            ]
        });
        normalize_submit_plan_input(&mut input);
        let tasks = input["tasks"].as_array().unwrap();
        assert_eq!(tasks[0]["detail"], "Existing detail");
    }

    #[test]
    fn normalize_handles_multiple_tasks() {
        let mut input = json!({
            "title": "Plan",
            "plan": "Content",
            "tasks": [
                { "title": "T1", "description": "D1" },
                { "title": "T2", "detail": "D2" },
                { "title": "T3", "description": "D3" }
            ]
        });
        normalize_submit_plan_input(&mut input);
        let tasks = input["tasks"].as_array().unwrap();
        assert_eq!(tasks[0]["detail"], "D1");
        assert_eq!(tasks[1]["detail"], "D2");
        assert_eq!(tasks[2]["detail"], "D3");
    }

    #[test]
    fn normalize_handles_no_tasks_key() {
        let mut input = json!({ "title": "Plan", "plan": "Content" });
        normalize_submit_plan_input(&mut input); // Should not panic
    }

    #[test]
    fn normalize_handles_empty_tasks_array() {
        let mut input = json!({
            "title": "Plan",
            "plan": "Content",
            "tasks": []
        });
        normalize_submit_plan_input(&mut input); // Should not panic
    }

    // ── Tool metadata tests ──────────────────────────────────────────

    #[test]
    fn submit_plan_tool_name() {
        use crate::Tool;
        assert_eq!(Tool::name(&SubmitPlanTool), "SubmitPlan");
    }

    #[test]
    fn planner_question_tool_name() {
        use crate::Tool;
        assert_eq!(Tool::name(&PlannerQuestionTool), "PlannerQuestion");
    }

    #[test]
    fn submit_plan_schema_requires_title_plan_tasks() {
        let schema = SubmitPlanTool.input_schema();
        let required = schema["required"].as_array().unwrap();
        let required_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_strs.contains(&"title"));
        assert!(required_strs.contains(&"plan"));
        assert!(required_strs.contains(&"tasks"));
    }

    #[test]
    fn planner_question_schema_requires_questions() {
        let schema = PlannerQuestionTool.input_schema();
        let required = schema["required"].as_array().unwrap();
        let required_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_strs.contains(&"questions"));
    }
}
