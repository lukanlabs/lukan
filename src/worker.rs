use anyhow::Result;
use clap::Subcommand;

use lukan_core::workers::{WorkerCreateInput, WorkerManager};

#[derive(Subcommand)]
pub enum WorkerCommands {
    /// List all workers
    List,
    /// Add a new worker interactively
    Add,
    /// Remove a worker by ID
    Remove { id: String },
    /// Pause (disable) a worker
    Pause { id: String },
    /// Resume (enable) a worker
    Resume { id: String },
    /// Show recent runs for a worker
    Runs { id: String },
}

pub async fn handle_worker_command(command: WorkerCommands) -> Result<()> {
    match command {
        WorkerCommands::List => {
            let workers = WorkerManager::list().await?;
            if workers.is_empty() {
                println!("No workers configured.");
                println!("Use `lukan worker add` to create one.");
                return Ok(());
            }

            println!(
                "\n  {:<8} {:<20} {:<12} {:<10} {:<20}",
                "ID", "Name", "Schedule", "Enabled", "Last Run"
            );
            println!("  {}", "-".repeat(72));
            for w in &workers {
                let last_run = w.last_run_status.as_deref().unwrap_or("never");
                let enabled = if w.enabled { "yes" } else { "no" };
                println!(
                    "  {:<8} {:<20} {:<12} {:<10} {:<20}",
                    w.id, w.name, w.schedule, enabled, last_run
                );
            }
            println!();
        }

        WorkerCommands::Add => {
            let name: String = dialoguer::Input::new()
                .with_prompt("Worker name")
                .interact_text()?;

            let schedule: String = dialoguer::Input::new()
                .with_prompt("Schedule (e.g. every:5m, */10 * * * *)")
                .interact_text()?;

            // Validate schedule upfront
            lukan_core::workers::schedule::parse_schedule_ms(&schedule)?;

            let prompt: String = dialoguer::Input::new()
                .with_prompt("Prompt (task for the agent)")
                .interact_text()?;

            let input = WorkerCreateInput {
                name: name.clone(),
                schedule: schedule.clone(),
                prompt,
                tools: None,
                provider: None,
                model: None,
                enabled: Some(true),
                notify: None,
            };

            let worker = WorkerManager::create(input).await?;
            println!("\n  Created worker '{}' (ID: {})", worker.name, worker.id);
            println!("  Schedule: {schedule}");
            println!("  Enabled: true\n");
        }

        WorkerCommands::Remove { id } => {
            if WorkerManager::delete(&id).await? {
                println!("  Deleted worker {id}");
            } else {
                println!("  Worker not found: {id}");
            }
        }

        WorkerCommands::Pause { id } => {
            let patch = lukan_core::workers::WorkerUpdateInput {
                enabled: Some(false),
                name: None,
                schedule: None,
                prompt: None,
                tools: None,
                provider: None,
                model: None,
                notify: None,
            };
            match WorkerManager::update(&id, patch).await? {
                Some(w) => println!("  Paused worker '{}' ({})", w.name, w.id),
                None => println!("  Worker not found: {id}"),
            }
        }

        WorkerCommands::Resume { id } => {
            let patch = lukan_core::workers::WorkerUpdateInput {
                enabled: Some(true),
                name: None,
                schedule: None,
                prompt: None,
                tools: None,
                provider: None,
                model: None,
                notify: None,
            };
            match WorkerManager::update(&id, patch).await? {
                Some(w) => println!("  Resumed worker '{}' ({})", w.name, w.id),
                None => println!("  Worker not found: {id}"),
            }
        }

        WorkerCommands::Runs { id } => {
            let worker = WorkerManager::get(&id).await?;
            match worker {
                Some(w) => {
                    println!("\n  Recent runs for '{}' ({}):\n", w.name, w.id);
                }
                None => {
                    println!("  Worker not found: {id}");
                    return Ok(());
                }
            }

            let runs = WorkerManager::get_runs(&id, 10).await?;
            if runs.is_empty() {
                println!("  No runs yet.");
            } else {
                println!(
                    "  {:<8} {:<10} {:<24} {:<24} {:<6}",
                    "Run ID", "Status", "Started", "Completed", "Turns"
                );
                println!("  {}", "-".repeat(74));
                for r in &runs {
                    let completed = r.completed_at.as_deref().unwrap_or("-");
                    println!(
                        "  {:<8} {:<10} {:<24} {:<24} {:<6}",
                        r.id, r.status, r.started_at, completed, r.turns
                    );
                }
            }
            println!();
        }
    }

    Ok(())
}
