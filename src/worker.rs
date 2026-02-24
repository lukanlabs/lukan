use anyhow::{Result, bail};
use clap::Subcommand;
use dialoguer::Select;

use lukan_core::workers::{WorkerCreateInput, WorkerManager};

#[derive(Subcommand)]
pub enum WorkerCommands {
    /// List all workers
    List,
    /// Add a new worker interactively
    Add,
    /// Remove a worker by ID (interactive if omitted)
    Remove { id: Option<String> },
    /// Pause (disable) a worker (interactive if omitted)
    Pause { id: Option<String> },
    /// Resume (enable) a worker (interactive if omitted)
    Resume { id: Option<String> },
    /// Browse worker runs and view output (interactive)
    Runs { id: Option<String> },
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
            let id = match id {
                Some(id) => id,
                None => pick_worker("Select worker to remove").await?,
            };
            if WorkerManager::delete(&id).await? {
                println!("  Deleted worker {id}");
            } else {
                println!("  Worker not found: {id}");
            }
        }

        WorkerCommands::Pause { id } => {
            let id = match id {
                Some(id) => id,
                None => pick_worker("Select worker to pause").await?,
            };
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
            let id = match id {
                Some(id) => id,
                None => pick_worker("Select worker to resume").await?,
            };
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
            interactive_runs(id).await?;
        }
    }

    Ok(())
}

/// Interactive worker picker — returns the selected worker ID
async fn pick_worker(prompt: &str) -> Result<String> {
    let workers = WorkerManager::list().await?;
    if workers.is_empty() {
        bail!("No workers configured. Use `lukan worker add` to create one.");
    }

    let items: Vec<String> = workers
        .iter()
        .map(|w| {
            let status = if w.enabled { "enabled" } else { "disabled" };
            let last = w.last_run_status.as_deref().unwrap_or("never run");
            format!("{} [{}] ({})", w.name, status, last)
        })
        .collect();

    let selection = Select::new()
        .with_prompt(prompt)
        .items(&items)
        .default(0)
        .interact()?;

    Ok(workers[selection].id.clone())
}

/// Interactive run browser: pick worker → pick run → view output
async fn interactive_runs(id: Option<String>) -> Result<()> {
    let worker_id = match id {
        Some(id) => id,
        None => pick_worker("Select worker to view runs").await?,
    };

    let worker = WorkerManager::get(&worker_id).await?;
    let worker_name = match worker {
        Some(ref w) => w.name.clone(),
        None => {
            println!("  Worker not found: {worker_id}");
            return Ok(());
        }
    };

    loop {
        let runs = WorkerManager::get_runs(&worker_id, 20).await?;
        if runs.is_empty() {
            println!("\n  No runs yet for '{worker_name}'.");
            return Ok(());
        }

        let items: Vec<String> = runs
            .iter()
            .map(|r| {
                let completed = r.completed_at.as_deref().unwrap_or("running");
                let turns = r.turns;
                let tokens = r.token_usage.input + r.token_usage.output;
                format!(
                    "{} │ {} │ {} │ {}t │ {}tok",
                    r.id, r.status, completed, turns, tokens
                )
            })
            .collect();

        println!();
        let selection = Select::new()
            .with_prompt(format!("Runs for '{}' (ESC to go back)", worker_name))
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(idx) = selection else {
            return Ok(());
        };

        let run = &runs[idx];
        print_run_detail(run, &worker_name);
    }
}

fn print_run_detail(run: &lukan_core::workers::WorkerRun, worker_name: &str) {
    println!("\n  ╭─ Run {} for '{}' ─────────────────", run.id, worker_name);
    println!("  │ Status:    {}", run.status);
    println!("  │ Started:   {}", run.started_at);
    println!(
        "  │ Completed: {}",
        run.completed_at.as_deref().unwrap_or("-")
    );
    println!("  │ Turns:     {}", run.turns);
    println!(
        "  │ Tokens:    {} in / {} out",
        run.token_usage.input, run.token_usage.output
    );
    if let Some(ref err) = run.error {
        println!("  │ Error:     {err}");
    }
    println!("  ╰────────────────────────────────────");

    if !run.output.is_empty() {
        println!();
        println!("  Output:");
        println!("  ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄");
        for line in run.output.lines() {
            println!("  {line}");
        }
        println!("  ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄");
    }
    println!();
}
