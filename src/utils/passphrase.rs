//! Passphrase loading for SQLCipher: env var → .env in dir → secure prompt.

use anyhow::{Context, Result};
use colored::Colorize;
use log::{info, warn};
use std::path::Path;

const ENV_KEY: &str = "NEFAXER_DB_KEY";

fn try_env_then_dotenv(dir: &Path) -> Option<String> {
    if let Ok(s) = std::env::var(ENV_KEY) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    let env_path = dir.join(".env");
    if env_path.is_file() {
        let _ = dotenvy::from_path(&env_path);
        if let Ok(s) = std::env::var(ENV_KEY) {
            let s = s.trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// Read passphrase: env (NEFAXER_DB_KEY) → .env in `dir` → secure prompt.
/// `is_new`: true for creating a new encrypted index (prompt says "New ...", reminds to note it down).
pub fn get_passphrase(dir: &Path, is_new: bool) -> Result<String> {
    info!("Encryption mode (either flag was provided or encrypted index was detected)");
    if let Some(s) = try_env_then_dotenv(dir) {
        info!("Passphrase found in environment");
        return Ok(s);
    }
    let label = format!("[{}]", env!("CARGO_PKG_NAME")).cyan().bold();
    let prompt = if is_new {
        "Create new passphrase: "
    } else {
        "Enter passphrase: "
    };
    let pass =
        rpassword::prompt_password(format!("{} {}", label, prompt)).context("read passphrase")?;
    if is_new {
        warn!("Lost passphrase = lost access");
    }
    Ok(pass.trim().to_string())
}
