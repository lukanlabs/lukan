use std::path::{Path, PathBuf};

use anyhow::Result;

use lukan_agent::git_worktree_roots::{find_canonical_git_root, resolve_base_ref};
use lukan_agent::subagent_worktrees::create_worktree;

pub fn resolve_main_repo_root(start: &Path) -> Option<PathBuf> {
    find_canonical_git_root(start)
}

fn git_output(repo_root: &Path, args: &[&str]) -> Result<(bool, String)> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .output()?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

pub fn create_session_worktree(cwd: &Path, slug: &str) -> Result<PathBuf> {
    let repo_root = resolve_main_repo_root(cwd)
        .ok_or_else(|| anyhow::anyhow!("Session worktree mode requires a git repository."))?;

    let base_ref = resolve_base_ref(&repo_root)?;

    let (worktree_path, branch, _) = create_worktree(&repo_root, slug)?;
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
