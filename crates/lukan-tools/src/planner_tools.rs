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

    async fn execute(&self, _input: Value, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // This tool is intercepted by the agent loop before execution.
        Ok(ToolResult::error(
            "PlannerQuestion should be intercepted by the agent loop, not executed directly.",
        ))
    }
}
