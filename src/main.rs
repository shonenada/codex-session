mod cli;
mod codex_home;
mod session_store;
mod tui;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use chrono_humanize::HumanTime;
use clap::Parser;
use cli::{Cli, Command, DeleteArgs, InfoArgs, ListArgs, ResumeArgs};
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Table};
use dialoguer::{Confirm, FuzzySelect};
use owo_colors::OwoColorize;
use session_store::{
    ListOptions, SessionDetail, SessionSummary, list_sessions, load_session_detail,
    resolve_session_path,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tui::{TuiOutcome, run as run_tui};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let codex_home = codex_home::resolve(cli.codex_home)?;

    match cli.command {
        Some(Command::List(args)) => run_list(&codex_home, args)?,
        Some(Command::Resume(args)) => run_resume(&codex_home, args, &cli.codex_bin)?,
        Some(Command::Info(args)) => run_info(&codex_home, args)?,
        Some(Command::Delete(args)) => run_delete(&codex_home, args)?,
        None => run_interactive(&codex_home, &cli.codex_bin)?,
    }

    Ok(())
}

fn run_interactive(codex_home: &Path, codex_bin: &str) -> Result<()> {
    let opts = ListOptions {
        limit: 500,
        cursor: None,
        providers: Vec::new(),
        show_all: true,
        cwd_filter: None,
    };
    let list = list_sessions(codex_home, &opts)?;
    if let Some(outcome) = run_tui(list.sessions)? {
        match outcome {
            TuiOutcome::Resume(summary) => {
                println!("Resuming session {}", summary.id.cyan());
                resume_session(codex_bin, &summary.id)?;
            }
            TuiOutcome::Jump(summary) => {
                if let Some(cwd) = summary.cwd.as_ref() {
                    std::env::set_current_dir(cwd)
                        .with_context(|| format!("failed to cd to {}", cwd.display()))?;
                    println!("Changed directory to {}", cwd.display());
                } else {
                    println!("No CWD recorded; staying in current directory");
                }
                println!("Resuming session {}", summary.id.cyan());
                resume_session(codex_bin, &summary.id)?;
            }
        }
    }
    Ok(())
}

fn resolve_scope(all: bool, cwd: Option<PathBuf>) -> (bool, Option<PathBuf>) {
    if let Some(dir) = cwd {
        (false, Some(dir))
    } else if all {
        (true, None)
    } else {
        (true, None)
    }
}

fn run_list(codex_home: &Path, args: ListArgs) -> Result<()> {
    let (show_all, cwd_filter) = resolve_scope(args.all, args.cwd.clone());

    let opts = ListOptions {
        limit: args.limit.max(1),
        cursor: args.cursor.clone(),
        providers: args.providers.clone(),
        show_all,
        cwd_filter,
    };

    let list = list_sessions(codex_home, &opts)?;

    if args.json {
        let payload = serde_json::json!({
            "sessions": list.sessions,
            "next_cursor": list.next_cursor,
            "scanned_files": list.scanned_files,
            "reached_scan_cap": list.reached_scan_cap,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    if list.sessions.is_empty() {
        println!("{}", "No Codex sessions were found.".yellow());
        println!(
            "Use {} to focus on a directory (e.g. {}).",
            "--cwd".green(),
            "codex-session list --cwd ~/Projects/app".cyan()
        );
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Updated", "Branch", "CWD", "Conversation"]);

    for summary in &list.sessions {
        let updated = summary
            .updated_at
            .map(|dt| format_relative(dt))
            .unwrap_or_else(|| "unknown".to_string());
        let preview = summary
            .preview
            .as_deref()
            .unwrap_or("(no user message yet)");
        let cwd = summary
            .cwd
            .as_ref()
            .map(|path| shorten_path(path, 28))
            .unwrap_or_else(|| "(unknown)".into());
        table.add_row(vec![
            Cell::new(updated),
            Cell::new(summary.git_branch.as_deref().unwrap_or("-")),
            Cell::new(cwd),
            Cell::new(truncate_preview(preview)),
        ]);
    }

    println!("{}", table);
    println!(
        "Scanned {} files{}.",
        list.scanned_files,
        if list.reached_scan_cap {
            " (hit scan cap)"
        } else {
            ""
        }
    );

    if let Some(cursor) = list.next_cursor {
        println!(
            "More sessions available. Continue with {}",
            format!("--cursor {cursor}").green()
        );
    }

    if let Some(first) = list.sessions.first() {
        println!(
            "To resume, run {} ({}).",
            first.resume_hint(),
            first
                .cwd
                .as_ref()
                .map(|p| shorten_path(p, 32))
                .unwrap_or_else(|| "unknown location".into())
        );
    }

    Ok(())
}

fn run_resume(codex_home: &Path, args: ResumeArgs, codex_bin: &str) -> Result<()> {
    let summary = if let Some(query) = args.session.as_deref() {
        let path = resolve_session_path(codex_home, query)?;
        load_session_detail(codex_home, &path)?.summary
    } else if args.last {
        let opts = build_resume_list_opts(&args)?;
        let list = list_sessions(codex_home, &opts)?;
        list.sessions
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No recorded sessions found"))?
    } else {
        prompt_for_session(codex_home, &args)?
    };

    if args.dry_run {
        println!(
            "{}",
            format!("{} {} {}", codex_bin, "resume", summary.id).cyan()
        );
        return Ok(());
    }

    println!("Resuming session {}", summary.id.cyan());
    resume_session(codex_bin, &summary.id)
}

fn build_resume_list_opts(args: &ResumeArgs) -> Result<ListOptions> {
    let (show_all, cwd_filter) = resolve_scope(args.all, args.cwd.clone());

    Ok(ListOptions {
        limit: args.limit.max(1),
        cursor: None,
        providers: Vec::new(),
        show_all,
        cwd_filter,
    })
}

fn prompt_for_session(codex_home: &Path, args: &ResumeArgs) -> Result<SessionSummary> {
    let opts = build_resume_list_opts(args)?;
    let list = list_sessions(codex_home, &opts)?;
    if list.sessions.is_empty() {
        bail!("No recorded sessions available to resume");
    }

    let items: Vec<String> = list
        .sessions
        .iter()
        .map(|summary| {
            format!(
                "{:<18} {:<20} {:<28} {}",
                summary
                    .updated_at
                    .map(|dt| format_relative(dt))
                    .unwrap_or_else(|| "unknown".into()),
                summary.git_branch.as_deref().unwrap_or("-"),
                summary
                    .cwd
                    .as_ref()
                    .map(|path| shorten_path(path, 28))
                    .unwrap_or_else(|| "(unknown)".into()),
                truncate_preview(
                    summary
                        .preview
                        .as_deref()
                        .unwrap_or("(no user message yet)")
                )
            )
        })
        .collect();

    let selection = FuzzySelect::new()
        .with_prompt("Pick a session to resume")
        .items(&items)
        .default(0)
        .interact()?;

    Ok(list.sessions[selection].clone())
}

fn run_info(codex_home: &Path, args: InfoArgs) -> Result<()> {
    let path = resolve_session_path(codex_home, &args.session)?;
    let detail = load_session_detail(codex_home, &path)?;
    print_detail(&detail);
    Ok(())
}

fn print_detail(detail: &SessionDetail) {
    println!("Session : {}", detail.summary.id.green());
    println!("Path    : {}", detail.summary.path.display());
    if let Some(cwd) = detail.summary.cwd.as_ref() {
        println!("CWD     : {}", cwd.display());
    }
    if let Some(provider) = detail.summary.provider.as_ref() {
        println!("Provider: {provider}");
    }
    if let Some(branch) = detail.git_branch.as_ref() {
        println!("Git     : {branch}");
    }
    if let Some(created) = detail.summary.created_at {
        println!("Started : {}", format_relative(created));
    }
    if let Some(updated) = detail.summary.updated_at {
        println!("Updated : {}", format_relative(updated));
    }
    if let Some(source) = detail.source.as_ref() {
        println!("Source  : {source:?}");
    }
    if let Some(instructions) = detail.instructions.as_ref() {
        println!("Notes   : {}", truncate_preview(instructions));
    }
    println!("Resume  : {}", detail.summary.resume_hint());
}

fn run_delete(codex_home: &Path, args: DeleteArgs) -> Result<()> {
    let path = resolve_session_path(codex_home, &args.session)?;
    let detail = load_session_detail(codex_home, &path)?;
    if !args.yes {
        println!(
            "Delete session {} recorded at {}?",
            detail.summary.id.red(),
            path.display()
        );
        if !Confirm::new()
            .with_prompt("This cannot be undone. Continue?")
            .default(false)
            .interact()?
        {
            println!("Aborted");
            return Ok(());
        }
    }
    fs::remove_file(&path)?;
    println!("Removed session {}", detail.summary.id.red());
    Ok(())
}

pub(crate) fn truncate_preview(text: &str) -> String {
    const MAX: usize = 80;
    if text.chars().count() <= MAX {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(MAX).collect();
        format!("{truncated}…")
    }
}

pub(crate) fn format_relative(dt: DateTime<Utc>) -> String {
    let now = Utc::now();
    let ht = HumanTime::from(dt - now);
    format!("{} ({})", ht, dt.format("%Y-%m-%d %H:%M"))
}

pub(crate) fn shorten_path(path: &Path, max_chars: usize) -> String {
    let text = path.display().to_string();
    truncate_left(&text, max_chars)
}

pub(crate) fn truncate_left(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text.to_string()
    } else {
        let keep = max_chars.saturating_sub(1).max(1);
        let tail_start = chars.len().saturating_sub(keep);
        let tail: String = chars[tail_start..].iter().collect();
        format!("…{tail}")
    }
}

fn resume_session(codex_bin: &str, session_id: &str) -> Result<()> {
    let status = ProcessCommand::new(codex_bin)
        .arg("resume")
        .arg(session_id)
        .status()
        .with_context(|| format!("failed to spawn {codex_bin}"))?;
    if !status.success() {
        bail!("codex exited with status {status}");
    }
    Ok(())
}
