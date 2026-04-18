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
        // Truncate old_text in error message to avoid flooding the screen
        let preview: String = edit.old_text.chars().take(200).collect();
        let truncated = if edit.old_text.len() > 200 {
            format!("{preview}... ({} chars total)", edit.old_text.len())
        } else {
            preview
        };

        // Try to find the region in the file that best matches old_text
        // so the model can see exactly what differs.
        let old_lines: Vec<&str> = edit.old_text.lines().collect();
        let first_line = old_lines.first().map(|l| l.trim()).unwrap_or("");
        let hint = if !first_line.is_empty() {
            let file_lines: Vec<&str> = content.lines().collect();
            if let Some(idx) = file_lines.iter().position(|l| l.trim() == first_line) {
                // Extract the same number of lines from the file as old_text has
                let end = (idx + old_lines.len()).min(file_lines.len());
                let actual: String = file_lines[idx..end].join("\n");
                let expected: String = old_lines.join("\n");
                if actual != expected {
                    // Show what the model sent vs what the file actually has
                    let actual_preview: String = actual.chars().take(300).collect();
                    let expected_preview: String = expected.chars().take(300).collect();
                    format!(
                        "\nFirst line matched at line {}, but the full block differs.\nYou sent:\n{:?}\nFile has:\n{:?}\nUse the file's actual content as old_text.",
                        idx + 1,
                        expected_preview,
                        actual_preview
                    )
                } else {
                    // Lines match individually but full text doesn't — likely trailing newline/whitespace
                    "\nHint: lines match but whitespace differs. Check trailing newlines or spaces."
                        .to_string()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        return Err(format!(
            "old_text not found in {file_path_str}. Make sure it matches exactly (including whitespace).\nold_text: {:?}{hint}",
            truncated
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

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn search_hint(&self) -> Option<&str> {
        Some("edit existing files by exact string replacement")
    }

    fn activity_label(&self, _input: &serde_json::Value) -> Option<String> {
        Some("Editing file".to_string())
    }

    fn validate_input(&self, input: &serde_json::Value, ctx: &ToolContext) -> Result<(), String> {
        let file_path_str = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing required field: file_path".to_string())?;

        let path = PathBuf::from(file_path_str);
        let path = if path.is_absolute() {
            path
        } else {
            ctx.cwd.join(&path)
        };

        if let Some(read_files) = ctx.read_files.try_lock().ok() {
            if !read_files.contains_key(&path) {
                return Err(format!(
                    "File has not been read yet. Use ReadFiles first: {file_path_str}"
                ));
            }
        }

        let edits_value = input.get("edits");
        if edits_value.is_none() {
            let old_text = input.get("old_text").and_then(|v| v.as_str()).ok_or_else(|| {
                "Missing required field: old_text (or provide edits array)".to_string()
            })?;
            let _new_text = input.get("new_text").and_then(|v| v.as_str()).ok_or_else(|| {
                "Missing required field: new_text (or provide edits array)".to_string()
            })?;
            if old_text.is_empty() {
                return Err("old_text cannot be empty.".to_string());
            }
        }

        Ok(())
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
        if !ctx.read_files.lock().await.contains_key(&path) {
            return Ok(ToolResult::error(format!(
                "File has not been read yet. Use ReadFiles first: {file_path_str}"
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

#[cfg(test)]
mod tests {
    use super::*;

    fn single_edit<'a>(old: &'a str, new: &'a str, replace_all: bool) -> SingleEdit<'a> {
        SingleEdit {
            old_text: old,
            new_text: new,
            replace_all,
        }
    }

    #[test]
    fn apply_edit_simple_replacement() {
        let content = "hello world";
        let edit = single_edit("hello", "goodbye", false);
        let result = apply_edit(content, &edit, "test.rs").unwrap();
        assert_eq!(result, "goodbye world");
    }

    #[test]
    fn apply_edit_not_found() {
        let content = "hello world";
        let edit = single_edit("missing", "replacement", false);
        let err = apply_edit(content, &edit, "test.rs").unwrap_err();
        assert!(err.contains("old_text not found"));
        assert!(err.contains("test.rs"));
    }

    #[test]
    fn apply_edit_duplicate_without_replace_all() {
        let content = "foo bar foo baz";
        let edit = single_edit("foo", "qux", false);
        let err = apply_edit(content, &edit, "test.rs").unwrap_err();
        assert!(err.contains("found 2 times"));
        assert!(err.contains("replace_all: true"));
    }

    #[test]
    fn apply_edit_replace_all_replaces_all_occurrences() {
        let content = "foo bar foo baz foo";
        let edit = single_edit("foo", "qux", true);
        let result = apply_edit(content, &edit, "test.rs").unwrap();
        assert_eq!(result, "qux bar qux baz qux");
    }

    #[test]
    fn apply_edit_replace_all_single_occurrence() {
        let content = "hello world";
        let edit = single_edit("hello", "goodbye", true);
        let result = apply_edit(content, &edit, "test.rs").unwrap();
        assert_eq!(result, "goodbye world");
    }

    #[test]
    fn apply_edit_multiline_content() {
        let content = "line 1\nline 2\nline 3\n";
        let edit = single_edit("line 2", "replaced line", false);
        let result = apply_edit(content, &edit, "test.rs").unwrap();
        assert_eq!(result, "line 1\nreplaced line\nline 3\n");
    }

    #[test]
    fn apply_edit_preserves_whitespace() {
        let content = "    indented code\n        more indented\n";
        let edit = single_edit("    indented code", "    new code", false);
        let result = apply_edit(content, &edit, "test.rs").unwrap();
        assert_eq!(result, "    new code\n        more indented\n");
    }

    #[test]
    fn apply_edit_empty_new_text_deletes() {
        let content = "before middle after";
        let edit = single_edit("middle ", "", false);
        let result = apply_edit(content, &edit, "test.rs").unwrap();
        assert_eq!(result, "before after");
    }

    #[test]
    fn apply_edit_empty_old_text_not_found_in_empty_content() {
        // Empty old_text matches everywhere (0-length match appears content.len()+1 times)
        // but since content is non-empty, "" matches many times
        let content = "abc";
        let edit = single_edit("", "x", false);
        // "" matches 4 times in "abc" (before a, before b, before c, at end)
        let err = apply_edit(content, &edit, "test.rs").unwrap_err();
        assert!(err.contains("found") && err.contains("times"));
    }

    #[test]
    fn apply_edit_replaces_only_first_when_not_replace_all() {
        // Even with replace_all=false, if there's exactly 1 match it works
        let content = "unique text here";
        let edit = single_edit("unique text", "changed text", false);
        let result = apply_edit(content, &edit, "test.rs").unwrap();
        assert_eq!(result, "changed text here");
    }

    // ── Tool metadata tests ──────────────────────────────────────────

    #[test]
    fn edit_file_tool_name() {
        let tool = EditFileTool;
        assert_eq!(Tool::name(&tool), "EditFile");
    }

    #[test]
    fn edit_file_tool_description_not_empty() {
        let tool = EditFileTool;
        assert!(!Tool::description(&tool).is_empty());
    }

    #[test]
    fn edit_file_tool_schema_has_file_path() {
        let tool = EditFileTool;
        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap();
        assert!(props.get("file_path").is_some());
        assert!(props.get("old_text").is_some());
        assert!(props.get("new_text").is_some());
        assert!(props.get("replace_all").is_some());
        assert!(props.get("edits").is_some());
    }

    #[test]
    fn edit_file_tool_required_fields() {
        let tool = EditFileTool;
        let schema = tool.input_schema();
        let required = schema.get("required").unwrap().as_array().unwrap();
        let required_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_strs.contains(&"file_path"));
    }
}
