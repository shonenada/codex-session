use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Inspect Codex session history",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Override the location of the Codex home directory.
    #[arg(long = "codex-home", value_name = "DIR", global = true)]
    pub codex_home: Option<PathBuf>,

    /// Path to the codex binary to run when resuming a session.
    #[arg(
        long = "codex-bin",
        value_name = "PATH",
        default_value = "codex",
        global = true
    )]
    pub codex_bin: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List recorded sessions and show their metadata.
    #[command(alias = "ls")]
    List(ListArgs),

    /// Spawn `codex resume` for a recorded session.
    Resume(ResumeArgs),

    /// Show details about a session.
    Info(InfoArgs),

    /// Delete a recorded session.
    Delete(DeleteArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ListArgs {
    /// Include sessions from every project directory.
    #[arg(long, short = 'a', default_value_t = false)]
    pub all: bool,

    /// Restrict the listing to sessions recorded under this directory.
    #[arg(long = "cwd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Maximum number of sessions to display.
    #[arg(long, default_value_t = 20)]
    pub limit: usize,

    /// Pagination cursor token returned by a previous invocation.
    #[arg(long, value_name = "TOKEN")]
    pub cursor: Option<String>,

    /// Filter sessions by provider id (comma separated list).
    #[arg(long = "provider", value_name = "PROVIDER", value_delimiter = ',', action = ArgAction::Append)]
    pub providers: Vec<String>,

    /// Emit machine-readable JSON instead of a table.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

impl Default for ListArgs {
    fn default() -> Self {
        Self {
            all: false,
            cwd: None,
            limit: 20,
            cursor: None,
            providers: Vec::new(),
            json: false,
        }
    }
}

#[derive(Debug, Args, Clone)]
pub struct ResumeArgs {
    /// Optional session id or path to resume.
    #[arg(value_name = "SESSION_ID_OR_PATH")]
    pub session: Option<String>,

    /// Automatically resume the most recent session.
    #[arg(long, default_value_t = false)]
    pub last: bool,

    /// Include sessions from every project directory when prompting.
    #[arg(long, default_value_t = false)]
    pub all: bool,

    /// Restrict prompting to sessions recorded under this directory.
    #[arg(long = "cwd", value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Show at most this many sessions in the picker.
    #[arg(long, default_value_t = 25)]
    pub limit: usize,

    /// Print the command but do not execute it.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

#[derive(Debug, Args, Clone)]
pub struct InfoArgs {
    /// Session id or path to show.
    #[arg(value_name = "SESSION_ID_OR_PATH")]
    pub session: String,
}

#[derive(Debug, Args, Clone)]
pub struct DeleteArgs {
    /// Session id or path to delete.
    #[arg(value_name = "SESSION_ID_OR_PATH")]
    pub session: String,

    /// Skip the confirmation prompt.
    #[arg(long, short = 'y', default_value_t = false)]
    pub yes: bool,
}
