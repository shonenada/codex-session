# Codex Session Agent Instructions

This project was built to be used inside the OpenAI Codex CLI ecosystem. When operating as an autonomous agent in this repository:

1. **Stay in sync with Codex files** – `codex-session` reads the same rollout files that Codex writes. Do not change on-disk formats without coordinating with upstream `codex/codex-rs`.
2. **Preserve UX parity** – mirror the behavior of Codex' native resume picker (keys, layouts, filters). New features should feel identical unless explicitly experimenting.
3. **Safety first** – session deletion (`dd`) permanently removes rollout JSONL files. Confirm with the user before mass deletion or destructive actions.
4. **Prefer Rust idioms** – follow `cargo fmt`, `cargo clippy`, and keep dependencies minimal. When interfacing with Codex crates, reuse their helpers instead of reimplementing logic.
5. **Document entry points** – update `README.md` whenever CLI flags, keyboard shortcuts, or workflows change so humans and agents have a single source of truth.

If you need details about Codex internals, inspect the sibling `../codex` checkout (especially `codex-rs`).
