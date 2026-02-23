use anyhow::Result;

const INSTALL_URL: &str = "https://get.lukan.ai/install.sh";

/// Self-update by downloading and running the install script
pub async fn run_update() -> Result<()> {
    println!("\n  \x1b[36mChecking for updates...\x1b[0m\n");

    let shell = if which_exists("bash") { "bash" } else { "sh" };

    // Build the pipe command: curl -fsSL <url> | bash (or wget equivalent)
    let script = if which_exists("curl") {
        format!("curl -fsSL {INSTALL_URL} | {shell}")
    } else if which_exists("wget") {
        format!("wget -qO- {INSTALL_URL} | {shell}")
    } else {
        anyhow::bail!("Either curl or wget is required for updates");
    };

    let status = tokio::process::Command::new(shell)
        .args(["-c", &script])
        .status()
        .await?;

    if !status.success() {
        anyhow::bail!("Update failed with exit code: {}", status);
    }

    Ok(())
}

fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
