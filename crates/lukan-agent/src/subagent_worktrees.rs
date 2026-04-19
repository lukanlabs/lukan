use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::git_worktree_roots::find_canonical_git_root;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCleanupStatus {
    Pending,
    AutoremovedClean,
    PreservedChanges,
    PreservedCleanupFailed,
    RemovedManual,
}

impl WorktreeCleanupStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AutoremovedClean => "autoremoved (clean)",
            Self::PreservedChanges => "preserved (changes detected)",
            Self::PreservedCleanupFailed => "preserved (cleanup failed)",
            Self::RemovedManual => "removed manually",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRecord {
    pub agent_id: String,
    pub task: String,
    pub isolation: String,
    pub worktree_path: PathBuf,
    pub worktree_branch: String,
    pub git_root: PathBuf,
    pub head_commit: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cleanup_status: WorktreeCleanupStatus,
}

pub fn canonical_project_root(start: &Path) -> PathBuf {
    find_canonical_git_root(start).unwrap_or_else(|| start.to_path_buf())
}

pub fn sanitize_branch_fragment(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        let keep = ch.is_ascii_alphanumeric();
        if keep {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };

    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn worktree_path_for(repo_root: &Path, agent_id: &str) -> PathBuf {
    canonical_project_root(repo_root)
        .join(".lukan")
        .join("subagents")
        .join(agent_id)
}

pub fn existing_worktree_head(worktree_path: &Path) -> Option<String> {
    let head = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(worktree_path)
        .output()
        .ok()?;
    if !head.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&head.stdout).trim().to_string())
}

pub fn create_worktree_from_base(
    repo_root: &Path,
    agent_id: &str,
    base_ref: &str,
) -> anyhow::Result<(PathBuf, String, String)> {
    let repo_root = canonical_project_root(repo_root);
    let slug = sanitize_branch_fragment(agent_id);
    let branch = format!("lukan-subagent-{slug}");
    let worktree_root = worktree_path_for(&repo_root, agent_id);
    if let Some(existing_head) = existing_worktree_head(&worktree_root) {
        return Ok((worktree_root, branch, existing_head));
    }

    fs::create_dir_all(worktree_root.parent().unwrap_or(&repo_root))?;
    let add = std::process::Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg("-B")
        .arg(&branch)
        .arg(&worktree_root)
        .arg(base_ref)
        .current_dir(&repo_root)
        .output()?;

    if !add.status.success() {
        let stderr = String::from_utf8_lossy(&add.stderr);
        anyhow::bail!("Failed to create subagent worktree: {}", stderr.trim());
    }

    let head = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(&worktree_root)
        .output()?;
    if !head.status.success() {
        let stderr = String::from_utf8_lossy(&head.stderr);
        anyhow::bail!("Failed to resolve subagent worktree HEAD: {}", stderr.trim());
    }
    let head_commit = String::from_utf8_lossy(&head.stdout).trim().to_string();

    Ok((worktree_root, branch, head_commit))
}

pub fn create_worktree(repo_root: &Path, agent_id: &str) -> anyhow::Result<(PathBuf, String, String)> {
    create_worktree_from_base(repo_root, agent_id, "HEAD")
}

pub fn remove_worktree(worktree_path: &Path, worktree_branch: &str, git_root: &Path) -> bool {
    let remove = std::process::Command::new("git")
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree_path)
        .current_dir(git_root)
        .output();
    match remove {
        Ok(output) if output.status.success() => {}
        _ => return false,
    }

    let _ = std::process::Command::new("git")
        .arg("branch")
        .arg("-D")
        .arg(worktree_branch)
        .current_dir(git_root)
        .output();

    let _ = std::process::Command::new("git")
        .arg("worktree")
        .arg("prune")
        .current_dir(git_root)
        .output();

    true
}

pub fn worktree_has_changes(worktree_path: &Path, head_commit: &str) -> bool {
    let status = std::process::Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(worktree_path)
        .output();
    let Ok(status) = status else {
        return true;
    };
    if !status.status.success() || !String::from_utf8_lossy(&status.stdout).trim().is_empty() {
        return true;
    }

    let rev_list = std::process::Command::new("git")
        .arg("rev-list")
        .arg("--count")
        .arg(format!("{head_commit}..HEAD"))
        .current_dir(worktree_path)
        .output();
    let Ok(rev_list) = rev_list else {
        return true;
    };
    if !rev_list.status.success() {
        return true;
    }

    String::from_utf8_lossy(&rev_list.stdout)
        .trim()
        .parse::<u32>()
        .map(|count| count > 0)
        .unwrap_or(true)
}

fn records_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".lukan")
        .join("subagents")
        .join("worktrees.json")
}

pub fn load_records(project_root: &Path) -> Vec<WorktreeRecord> {
    let path = records_path(&canonical_project_root(project_root));
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_records(project_root: &Path, records: &[WorktreeRecord]) -> anyhow::Result<()> {
    let path = records_path(&canonical_project_root(project_root));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(records)?)?;
    Ok(())
}

pub fn upsert_record(project_root: &Path, record: WorktreeRecord) -> anyhow::Result<()> {
    let mut records = load_records(&canonical_project_root(project_root));
    if let Some(existing) = records.iter_mut().find(|r| r.agent_id == record.agent_id) {
        *existing = record;
    } else {
        records.push(record);
    }
    save_records(&canonical_project_root(project_root), &records)
}

pub fn remove_record(project_root: &Path, agent_id: &str) -> anyhow::Result<()> {
    let mut records = load_records(&canonical_project_root(project_root));
    records.retain(|r| r.agent_id != agent_id);
    save_records(&canonical_project_root(project_root), &records)
}

pub fn cleanup_stale_worktrees(project_root: &Path, older_than_secs: i64) -> anyhow::Result<usize> {
    let mut records = load_records(&canonical_project_root(project_root));
    let cutoff = Utc::now() - chrono::Duration::seconds(older_than_secs);
    let mut removed = 0usize;

    for record in &mut records {
        if record.cleanup_status != WorktreeCleanupStatus::Pending {
            continue;
        }
        if record.started_at >= cutoff {
            continue;
        }
        if worktree_has_changes(&record.worktree_path, &record.head_commit) {
            continue;
        }
        if remove_worktree(&record.worktree_path, &record.worktree_branch, &record.git_root) {
            record.cleanup_status = WorktreeCleanupStatus::RemovedManual;
            removed += 1;
        }
    }

    save_records(&canonical_project_root(project_root), &records)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_branch_fragment_normalizes_values() {
        assert_eq!(sanitize_branch_fragment("Agent 12/test"), "agent-12-test");
        assert_eq!(sanitize_branch_fragment("---abc___"), "abc");
    }

    #[test]
    fn load_records_returns_empty_when_missing() {
        let root = std::env::temp_dir().join("lukan-worktree-missing-records");
        let _ = fs::remove_dir_all(&root);
        assert!(load_records(&root).is_empty());
    }
}
