use std::path::{Path, PathBuf};

use anyhow::Result;

use lukan_agent::subagent_worktrees::{create_worktree, find_git_root};

pub fn resolve_main_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = find_git_root(start)?;
    loop {
        let next = current.parent().and_then(find_git_root);
        match next {
            Some(parent_root) if parent_root != current => current = parent_root,
            _ => return Some(current),
        }
    }
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

fn current_default_branch(repo_root: &Path) -> String {
    let remote_head = std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .output();
    if let Ok(out) = remote_head
        && out.status.success()
    {
        let full = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(branch) = full.rsplit('/').next()
            && !branch.is_empty()
        {
            return branch.to_string();
        }
    }
    "main".to_string()
}

pub fn create_session_worktree(cwd: &Path, slug: &str) -> Result<PathBuf> {
    let repo_root = resolve_main_repo_root(cwd)
        .ok_or_else(|| anyhow::anyhow!("Session worktree mode requires a git repository."))?;

    let default_branch = current_default_branch(&repo_root);
    let origin_ref = format!("origin/{default_branch}");
    let (has_origin_ref, _) = git_output(&repo_root, &["rev-parse", "--verify", &origin_ref])?;
    let base_ref = if has_origin_ref {
        origin_ref
    } else {
        let fetch_ok = std::process::Command::new("git")
            .args(["fetch", "origin", &default_branch])
            .current_dir(&repo_root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if fetch_ok {
            format!("origin/{default_branch}")
        } else {
            "HEAD".to_string()
        }
    };

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
