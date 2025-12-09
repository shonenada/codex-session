# Codex Session Manager (codex-session)

A lightweight companion CLI for [OpenAI Codex](../codex) that lets you browse, resume, and prune recorded Codex sessions locally. It replicates the native Codex resume picker experience so you can manage history from any terminal.

## Features

- 🚀 Launches into a full-screen TUI by default: select with arrow keys or `j`/`k`, filter live with `/`, resume with `Enter`.
- 🔎 Searches every session under `~/.codex` (or a custom `CODEX_HOME`).
- 🗑️ Deletes the highlighted session via `dd`, with a confirmation dialog.
- 📤 Command mode (`:`) supports `:export <file>` to save the current session's chat history (use `.jsonl` for the raw rollout, `.json` for a structured history list, `.pdf` for a rendered PDF transcript).
- 🧰 Fall back to subcommands (`list`, `resume`, `info`, `delete`) for scripting or automation.

## Getting Started

```bash
cargo run --release
```

Or install a prebuilt binary from GitHub Releases (macOS/Linux):

```bash
curl -sSL https://github.com/shonenada/codex-session/raw/refs/heads/main/scripts/install.sh | bash
# or pin a tag
VERSION=v0.0.2 curl -sSL https://github.com/shonenada/codex-session/raw/refs/heads/main/scripts/install.sh | bash
```

The default command launches the TUI. Use the flags below to fine-tune behavior (e.g. resume non-interactively or target a specific Codex binary):

```bash
# List sessions in table form
cargo run -- list --all

# Resume the most recent rollout directly
cargo run -- resume --last

# Point to a custom Codex binary when resuming
cargo run -- --codex-bin ./codex-dev
```

Environment variables:

- `CODEX_HOME`: override the location of the Codex state directory (defaults to `~/.codex`).

## Keyboard shortcuts (TUI)

| Key            | Action                                |
|----------------|---------------------------------------|
| `↑` / `k`      | Move selection up                     |
| `↓` / `j`      | Move selection down                   |
| `/`            | Start filtering (type to search)      |
| `Enter`        | Resume the highlighted session        |
| `dd`           | Delete highlighted session (confirm)  |
| `:`            | Enter command mode (`:export file`)   |
| `Ctrl+C`       | Quit immediately                      |
| `Esc` / `q`    | Exit current mode / quit               |

### Command mode

Command mode gives you Vim-style control over extra actions. The first supported command is `:export <path>`, which writes the current session history in different formats based on the file extension:

| Extension | Output                                                                 |
|-----------|-------------------------------------------------------------------------|
| `.jsonl`  | Exact copy of the original rollout JSONL file.                          |
| `.json`   | Structured JSON array of `{ role, content }` chat entries.              |
| `.pdf`    | Rendered Markdown transcript saved to a PDF (one page per ~40 lines).   |
| anything else | Markdown transcript (same text shown in the TUI).                  |

Example:

```
:export ~/Desktop/session.json
```

The command status is shown on the bottom status bar after each export.

## Development

This crate reuses the Codex protocol definitions directly. Make sure you have the sibling `codex` repository checked out so the `codex-protocol` path dependency resolves.

```bash
cargo fmt
cargo check
```
