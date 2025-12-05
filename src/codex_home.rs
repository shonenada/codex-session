use anyhow::{Context, Result};
use dirs::home_dir;
use std::path::PathBuf;

/// Resolve the Codex home directory following the same semantics as the upstream CLI.
pub fn resolve(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_dir {
        return canonicalize_existing(path);
    }

    if let Ok(env_val) = std::env::var("CODEX_HOME") {
        if !env_val.trim().is_empty() {
            return canonicalize_existing(PathBuf::from(env_val));
        }
    }

    let mut default =
        home_dir().context("Could not determine the current user's home directory")?;
    default.push(".codex");
    Ok(default)
}

fn canonicalize_existing(path: PathBuf) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("{path:?} does not exist"))?;
    Ok(canonical)
}
