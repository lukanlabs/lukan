use std::path::PathBuf;

use async_trait::async_trait;
use lukan_core::models::checkpoints::{FileOperation, FileSnapshot};
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use similar::{ChangeTag, TextDiff};

use crate::{Tool, ToolContext, format_stats};

pub struct EditFileTool;

struct SingleEdit<'a> {
    old_text: &'a str,
    new_text: &'a str,
    replace_all: bool,
}

/// Apply a single edit to `content` in-memory. Returns the new content or an error string.
fn apply_edit(content: &str, edit: &SingleEdit<'_>, file_path_str: &str) -> Result<String, String> {
    let count = content.matches(edit.old_text).count();

    if count == 0 {
        return Err(format!(
            "old_text not found in {file_path_str}. Make sure it matches exactly (including whitespace).\nold_text: {:?}",
            edit.old_text
        ));
    }

    if !edit.replace_all && count > 1 {
        return Err(format!(
            "old_text found {count} times in {file_path_str}. Use replace_all: true or provide more context to make it unique."
        ));
    }

    if edit.replace_all {
        Ok(content.replace(edit.old_text, edit.new_text))
    } else {
        Ok(content.replacen(edit.old_text, edit.new_text, 1))
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "EditFile"
    }

    fn description(&self) -> &str {
        "Perform exact string replacements in files. Supports a single edit (old_text/new_text) or \
        multiple atomic edits via the `edits` array. All edits in `edits` are validated before \
        writing — if any fails, the file is not modified. The old_text must be unique unless \
        replace_all is true."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "The exact text to find and replace (single-edit mode)"
                },
                "new_text": {
                    "type": "string",
                    "description": "The replacement text (single-edit mode)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false, single-edit mode)",
                    "default": false
                },
                "edits": {
                    "type": "array",
                    "description": "Multiple edits applied atomically in order. If any edit fails, no changes are written.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_text": {
                                "type": "string",
                                "description": "The exact text to find and replace"
                            },
                            "new_text": {
                                "type": "string",
                                "description": "The replacement text"
                            },
                            "replace_all": {
                                "type": "boolean",
                                "description": "Replace all occurrences (default: false)",
                                "default": false
                            }
                        },
                        "required": ["old_text", "new_text"]
                    }
                }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let file_path_str = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: file_path"))?;

        let path = PathBuf::from(file_path_str);
        let path = if path.is_absolute() {
            path
        } else {
            ctx.cwd.join(&path)
        };

        if let Err(msg) = ctx.check_path_allowed(&path) {
            return Ok(ToolResult::error(msg));
        }

        if let Err(msg) = ctx.check_sensitive(&path) {
            return Ok(ToolResult::error(msg));
        }

        // Must have been read first
        if !ctx.read_files.lock().await.contains(&path) {
            return Ok(ToolResult::error(format!(
                "File has not been read yet. Use ReadFile first: {file_path_str}"
            )));
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read file: {e}"))),
        };

        // Build the list of edits — either from `edits` array or single old_text/new_text
        let edits_value = input.get("edits");
        let new_content = if let Some(arr) = edits_value.and_then(|v| v.as_array()) {
            // Multi-edit mode: validate and apply all edits atomically
            let mut edits: Vec<SingleEdit<'_>> = Vec::with_capacity(arr.len());
            for (i, item) in arr.iter().enumerate() {
                let old_text = item
                    .get("old_text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("edits[{i}] missing old_text"))?;
                let new_text = item
                    .get("new_text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("edits[{i}] missing new_text"))?;
                let replace_all = item
                    .get("replace_all")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                edits.push(SingleEdit {
                    old_text,
                    new_text,
                    replace_all,
                });
            }

            // Apply sequentially to an in-memory copy — atomic: all-or-nothing
            let mut working = content.clone();
            for (i, edit) in edits.iter().enumerate() {
                match apply_edit(&working, edit, file_path_str) {
                    Ok(result) => working = result,
                    Err(msg) => {
                        return Ok(ToolResult::error(format!("edits[{i}] failed: {msg}")));
                    }
                }
            }
            working
        } else {
            // Single-edit mode: old_text and new_text required at top level
            let old_text = input
                .get("old_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing required field: old_text (or provide edits array)")
                })?;
            let new_text = input
                .get("new_text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing required field: new_text (or provide edits array)")
                })?;
            let replace_all = input
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            match apply_edit(
                &content,
                &SingleEdit {
                    old_text,
                    new_text,
                    replace_all,
                },
                file_path_str,
            ) {
                Ok(result) => result,
                Err(msg) => return Ok(ToolResult::error(msg)),
            }
        };

        // Write the file
        tokio::fs::write(&path, &new_content).await?;

        // Generate diff — 3 lines of context around each change (like git)
        let diff = TextDiff::from_lines(&content, &new_content);
        let mut diff_str = format!("--- {file_path_str}\n");
        let mut added = 0usize;
        let mut removed = 0usize;
        for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
            diff_str.push_str(&format!("{}\n", hunk.header()));
            for change in hunk.iter_changes() {
                let sign = match change.tag() {
                    ChangeTag::Delete => {
                        removed += 1;
                        "-"
                    }
                    ChangeTag::Insert => {
                        added += 1;
                        "+"
                    }
                    ChangeTag::Equal => " ",
                };
                diff_str.push_str(&format!("{sign}{}", change.value()));
            }
        }

        let stats = format_stats(added, removed);
        let msg = stats;

        let snapshot = FileSnapshot {
            path: file_path_str.to_string(),
            operation: FileOperation::Modified,
            before: Some(content.clone()),
            after: Some(new_content),
            diff: Some(diff_str.clone()),
            additions: added as u32,
            deletions: removed as u32,
        };

        Ok(ToolResult::success(msg)
            .with_diff(diff_str)
            .with_snapshot(snapshot))
    }
}
