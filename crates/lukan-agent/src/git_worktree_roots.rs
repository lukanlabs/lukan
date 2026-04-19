use std::path::{Path, PathBuf};

use anyhow::Result;

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

pub fn find_canonical_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = find_git_root(start)?;
    loop {
        let parent = current.parent().and_then(find_git_root);
        match parent {
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

pub fn default_branch(repo_root: &Path) -> String {
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

pub fn resolve_base_ref(repo_root: &Path) -> Result<String> {
    let default_branch = default_branch(repo_root);
    let origin_ref = format!("origin/{default_branch}");
    let (has_origin_ref, _) = git_output(repo_root, &["rev-parse", "--verify", &origin_ref])?;
    if has_origin_ref {
        return Ok(origin_ref);
    }

    let fetch_ok = std::process::Command::new("git")
        .args(["fetch", "origin", &default_branch])
        .current_dir(repo_root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if fetch_ok {
        Ok(format!("origin/{default_branch}"))
    } else {
        Ok("HEAD".to_string())
    }
}
