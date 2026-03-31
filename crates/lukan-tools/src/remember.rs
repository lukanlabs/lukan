use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use crate::{Tool, ToolContext};

// ── Data Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MemoryFrontmatter {
    pub tags: Vec<String>,
    pub summary: String,
    pub memory_type: String,
    pub importance: String,
    pub related: Vec<String>,
    pub created: String,
    pub index: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryFile {
    pub filename: String,
    pub path: PathBuf,
    pub frontmatter: MemoryFrontmatter,
}

// ── Frontmatter Parsing ─────────────────────────────────────────────

/// Parse YAML frontmatter from a markdown file.
/// Expects `---\n...\n---` at the start of the file.
pub fn parse_memory_frontmatter(content: &str) -> Option<MemoryFrontmatter> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("\n---")?;
    let yaml_block = &rest[..end];

    let mut tags = Vec::new();
    let mut summary = String::new();
    let mut memory_type = String::from("context");
    let mut importance = String::from("medium");
    let mut related = Vec::new();
    let mut created = String::new();
    let mut index = Vec::new();
    let mut in_index = false;

    for line in yaml_block.lines() {
        let trimmed = line.trim();

        // Handle multi-line index entries
        if in_index {
            if let Some(val) = trimmed.strip_prefix("- ") {
                index.push(val.trim_matches('"').trim_matches('\'').to_string());
                continue;
            } else if let Some(val) = trimmed.strip_prefix('-') {
                let val = val.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    index.push(val.to_string());
                }
                continue;
            } else {
                in_index = false;
            }
        }

        if let Some(val) = trimmed.strip_prefix("tags:") {
            tags = parse_bracket_list(val);
        } else if let Some(val) = trimmed.strip_prefix("summary:") {
            summary = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = trimmed.strip_prefix("type:") {
            memory_type = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("importance:") {
            importance = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("related:") {
            related = parse_bracket_list(val);
        } else if let Some(val) = trimmed.strip_prefix("created:") {
            created = val.trim().to_string();
        } else if trimmed.starts_with("index:") {
            in_index = true;
        }
    }

    if summary.is_empty() {
        return None;
    }

    Some(MemoryFrontmatter {
        tags,
        summary,
        memory_type,
        importance,
        related,
        created,
        index,
    })
}

/// Parse `[item1, item2, item3]` into a Vec<String>.
fn parse_bracket_list(val: &str) -> Vec<String> {
    let val = val.trim();
    let inner = val
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(val);
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ── Discovery ───────────────────────────────────────────────────────

/// Discover all structured memory files in `.lukan/memories/`.
/// Excludes `MEMORY.md` (behavior profile, not a structured memory).
pub async fn discover_memory_files(cwd: &Path) -> Vec<MemoryFile> {
    let memories_dir = cwd.join(".lukan").join("memories");
    let mut files = Vec::new();

    let mut entries = match tokio::fs::read_dir(&memories_dir).await {
        Ok(e) => e,
        Err(_) => return files,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip behavior profile, hidden files, non-markdown
        if filename == "MEMORY.md" || filename.starts_with('.') || !filename.ends_with(".md") {
            continue;
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(frontmatter) = parse_memory_frontmatter(&content) {
            files.push(MemoryFile {
                filename,
                path,
                frontmatter,
            });
        }
    }

    // Sort by importance (high first), then by filename
    files.sort_by(|a, b| {
        let imp_order = |s: &str| match s {
            "high" => 0,
            "medium" => 1,
            "low" => 2,
            _ => 3,
        };
        imp_order(&a.frontmatter.importance)
            .cmp(&imp_order(&b.frontmatter.importance))
            .then_with(|| a.filename.cmp(&b.filename))
    });

    files
}

// ── Search ──────────────────────────────────────────────────────────

/// Search memory files by query. Matches against tags, summary, related, and type.
/// Returns files sorted by relevance (number of field matches).
pub async fn search_memory_files(cwd: &Path, query: &str) -> Vec<MemoryFile> {
    let all = discover_memory_files(cwd).await;
    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<(usize, MemoryFile)> = all
        .into_iter()
        .filter_map(|mf| {
            let mut score = 0usize;
            let fm = &mf.frontmatter;

            for term in &query_terms {
                // Tags (highest weight)
                if fm.tags.iter().any(|t| t.to_lowercase().contains(term)) {
                    score += 3;
                }
                // Summary
                if fm.summary.to_lowercase().contains(term) {
                    score += 2;
                }
                // Related
                if fm.related.iter().any(|r| r.to_lowercase().contains(term)) {
                    score += 1;
                }
                // Type
                if fm.memory_type.to_lowercase().contains(term) {
                    score += 1;
                }
                // Index entries
                if fm.index.iter().any(|i| i.to_lowercase().contains(term)) {
                    score += 1;
                }
            }

            if score > 0 { Some((score, mf)) } else { None }
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, mf)| mf).collect()
}

// ── Prompt Helpers ──────────────────────────────────────────────────

/// Generate a compact summary listing for injection into the system prompt.
/// Caps at 20 entries.
pub async fn get_memory_summaries_for_prompt(cwd: &Path) -> Option<String> {
    let files = discover_memory_files(cwd).await;
    if files.is_empty() {
        return None;
    }

    let mut section = String::from(
        "## Project Memories\nUse the Remember tool to recall details before important decisions.\n\n",
    );

    let cap = files.len().min(20);
    for mf in &files[..cap] {
        section.push_str(&format!(
            "- **{}** [{}|{}]: {}\n",
            mf.filename,
            mf.frontmatter.memory_type,
            mf.frontmatter.importance,
            mf.frontmatter.summary,
        ));
    }

    if files.len() > 20 {
        section.push_str(&format!(
            "\n({} more memories available via Remember tool)\n",
            files.len() - 20
        ));
    }

    Some(section)
}

/// Format all frontmatters for the LLM memory update prompt.
pub fn format_frontmatters_for_llm(files: &[MemoryFile]) -> String {
    if files.is_empty() {
        return String::from("(no existing memory files)");
    }
    files
        .iter()
        .map(|mf| {
            format!(
                "[{}] tags: [{}] | type: {} | importance: {} | summary: {}",
                mf.filename,
                mf.frontmatter.tags.join(", "),
                mf.frontmatter.memory_type,
                mf.frontmatter.importance,
                mf.frontmatter.summary,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check if MEMORY.md (behavior profile) exists.
pub async fn has_behavior_profile(cwd: &Path) -> bool {
    tokio::fs::metadata(cwd.join(".lukan").join("memories").join("MEMORY.md"))
        .await
        .is_ok()
}

/// Read MEMORY.md (behavior profile) content.
pub async fn read_behavior_profile(cwd: &Path) -> Option<String> {
    tokio::fs::read_to_string(cwd.join(".lukan").join("memories").join("MEMORY.md"))
        .await
        .ok()
        .filter(|s| !s.trim().is_empty())
}

// ── Remember Tool ───────────────────────────────────────────────────

pub struct RememberTool;

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "Remember"
    }

    fn description(&self) -> &str {
        "Recall relevant project memories, past decisions, and lessons learned. \
         Use this before important actions to check for relevant context. \
         Returns matching memory summaries and index entries. \
         Use ReadFiles with line ranges to load full details if needed."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to recall (e.g. 'grep tool decisions', 'auth flow', 'error handling patterns')"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: query"))?;

        let matches = search_memory_files(&ctx.cwd, query).await;

        if matches.is_empty() {
            return Ok(ToolResult::success("No matching memories found."));
        }

        let mut output = format!("Found {} matching memories:\n", matches.len());

        for mf in &matches {
            output.push_str(&format!(
                "\n--- {} [{}|{}] ---\n",
                mf.filename, mf.frontmatter.memory_type, mf.frontmatter.importance,
            ));
            output.push_str(&format!("Summary: {}\n", mf.frontmatter.summary));
            output.push_str(&format!("Tags: {}\n", mf.frontmatter.tags.join(", ")));

            if !mf.frontmatter.index.is_empty() {
                output.push_str("Index:\n");
                for entry in &mf.frontmatter.index {
                    output.push_str(&format!("  - {entry}\n"));
                }
            }

            output.push_str(&format!("Path: {}\n", mf.path.display()));
        }

        Ok(ToolResult::success(output))
    }
}
