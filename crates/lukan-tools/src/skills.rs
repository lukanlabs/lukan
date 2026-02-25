//! Skill discovery and loading.
//!
//! Skills live in `.lukan/skills/<folder>/SKILL.md` with YAML frontmatter:
//! ```text
//! ---
//! name: Git Commits
//! description: How to create proper git commits
//! ---
//! (instructions…)
//! ```

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use regex::Regex;
use serde_json::json;

use crate::{Tool, ToolContext};

// ── Types ────────────────────────────────────────────────────────────────

/// Metadata about a discovered skill.
pub struct SkillInfo {
    /// Human-readable name from frontmatter (e.g. "Git Commits")
    pub name: String,
    /// One-line description from frontmatter
    pub description: String,
    /// Directory name under `.lukan/skills/` (e.g. "git-commits")
    pub folder: String,
    /// Full path to the SKILL.md file
    pub path: PathBuf,
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Parse YAML frontmatter from SKILL.md content.
/// Returns `(name, description)` if both fields are present.
fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let re = Regex::new(r"(?s)^---\s*\n(.*?)\n---").ok()?;
    let caps = re.captures(content)?;
    let yaml = caps.get(1)?.as_str();

    let name_re = Regex::new(r"(?m)^name:[ \t]*(.+)$").ok()?;
    let desc_re = Regex::new(r"(?m)^description:[ \t]*(.+)$").ok()?;

    let name = name_re.captures(yaml)?.get(1)?.as_str().trim().to_string();
    let desc = desc_re.captures(yaml)?.get(1)?.as_str().trim().to_string();

    if name.is_empty() || desc.is_empty() {
        return None;
    }
    Some((name, desc))
}

// ── Public API ───────────────────────────────────────────────────────────

/// Discover all skills under `cwd/.lukan/skills/`.
pub async fn discover_skills(cwd: &Path) -> Vec<SkillInfo> {
    let skills_dir = cwd.join(".lukan").join("skills");
    let mut entries = match tokio::fs::read_dir(&skills_dir).await {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut skills = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if let Ok(content) = tokio::fs::read_to_string(&skill_md).await
            && let Some((name, description)) = parse_frontmatter(&content)
        {
            let folder = entry.file_name().to_string_lossy().to_string();
            skills.push(SkillInfo {
                name,
                description,
                folder,
                path: skill_md,
            });
        }
    }
    skills.sort_by(|a, b| a.folder.cmp(&b.folder));
    skills
}

/// Load the full content of a skill's SKILL.md by folder name.
pub async fn load_skill_content(cwd: &Path, folder: &str) -> Option<String> {
    let skill_path = cwd
        .join(".lukan")
        .join("skills")
        .join(folder)
        .join("SKILL.md");
    tokio::fs::read_to_string(&skill_path).await.ok()
}

// ── LoadSkill Tool ───────────────────────────────────────────────────────

pub struct LoadSkillTool;

#[async_trait]
impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "LoadSkill"
    }

    fn description(&self) -> &str {
        "Load a skill's instructions from .lukan/skills/. Skills contain project-specific \
         instructions for tasks like git commits, deployments, code patterns, etc. You MUST \
         call this tool BEFORE performing any task that matches an available skill — the skill \
         may override default behavior. Each skill only needs to be loaded once per session — \
         check the \"Already loaded\" list before calling."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill folder name to load (e.g. 'git-commits')"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let folder = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: name"))?;

        match load_skill_content(&ctx.cwd, folder).await {
            Some(content) => Ok(ToolResult::success(format!(
                "[Skill loaded: {folder}]\n\n{content}"
            ))),
            None => Ok(ToolResult::error(format!(
                "Skill \"{folder}\" not found in .lukan/skills/."
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_frontmatter() {
        let content =
            "---\nname: Git Commits\ndescription: How to create proper git commits\n---\nBody here";
        let (name, desc) = parse_frontmatter(content).unwrap();
        assert_eq!(name, "Git Commits");
        assert_eq!(desc, "How to create proper git commits");
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(parse_frontmatter("---\nname: Foo\n---\n").is_none());
        assert!(parse_frontmatter("---\ndescription: Bar\n---\n").is_none());
        assert!(parse_frontmatter("no frontmatter here").is_none());
    }

    #[test]
    fn rejects_empty_values() {
        assert!(parse_frontmatter("---\nname: \ndescription: Bar\n---\n").is_none());
    }

    #[tokio::test]
    async fn discover_empty_dir() {
        let tmp = std::env::temp_dir().join("lukan-skills-test-empty");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        let skills = discover_skills(&tmp).await;
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn discover_finds_valid_skill() {
        let tmp = std::env::temp_dir().join("lukan-skills-test-discover");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        let skill_dir = tmp.join(".lukan").join("skills").join("test-skill");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Test Skill\ndescription: A test skill\n---\nInstructions here",
        )
        .await
        .unwrap();

        let skills = discover_skills(&tmp).await;
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "Test Skill");
        assert_eq!(skills[0].folder, "test-skill");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn load_skill_content_works() {
        let tmp = std::env::temp_dir().join("lukan-skills-test-load");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        let skill_dir = tmp.join(".lukan").join("skills").join("my-skill");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        let content = "---\nname: My Skill\ndescription: Desc\n---\nDo stuff";
        tokio::fs::write(skill_dir.join("SKILL.md"), content)
            .await
            .unwrap();

        let loaded = load_skill_content(&tmp, "my-skill").await;
        assert_eq!(loaded.unwrap(), content);

        assert!(load_skill_content(&tmp, "nonexistent").await.is_none());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
