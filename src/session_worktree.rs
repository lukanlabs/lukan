use std::path::{Path, PathBuf};

use anyhow::Result;
use lukan_agent::git_worktree_roots::{find_canonical_git_root, resolve_base_ref};
use lukan_agent::subagent_worktrees::{
    create_worktree_from_base, existing_worktree_head, worktree_path_for,
};

pub fn resolve_main_repo_root(start: &Path) -> Option<PathBuf> {
    find_canonical_git_root(start)
}

pub fn create_session_worktree(cwd: &Path, slug: &str) -> Result<PathBuf> {
    let repo_root = resolve_main_repo_root(cwd)
        .ok_or_else(|| anyhow::anyhow!("Session worktree mode requires a git repository."))?;

    let base_ref = resolve_base_ref(&repo_root)?;

    let target_path = worktree_path_for(&repo_root, slug);
    if existing_worktree_head(&target_path).is_some() {
        return Ok(target_path);
    }

    let (worktree_path, branch, _) = create_worktree_from_base(&repo_root, slug, &base_ref)?;
    let reset = std::process::Command::new("git")
        .args(["reset", "--hard", &base_ref])
        .current_dir(&worktree_path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .status()?;
    if !reset.success() {
        anyhow::bail!("Failed to reset session worktree branch {branch} to base {base_ref}");
    }

    Ok(worktree_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_main_repo_root_returns_git_root_for_plain_repo() {
        let cwd = std::env::current_dir().unwrap();
        let root = resolve_main_repo_root(&cwd).unwrap();
        assert!(root.ends_with("lukan"));
    }
}
