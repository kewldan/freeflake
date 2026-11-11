//! Loading and saving settings.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use freeflake_core::Settings;

fn dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "freeflake").map(|d| d.config_dir().to_path_buf())
}

fn settings_path() -> Option<PathBuf> {
    dir().map(|d| d.join("settings.toml"))
}

/// Loads settings, falling back to defaults when the file is missing or bad.
pub fn load_settings() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };
    match fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<Settings>(&text) {
            Ok(s) => s.sanitized(),
            Err(e) => {
                log::warn!("ignoring broken {}: {e}", path.display());
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(settings: &Settings) -> Result<()> {
    let path = settings_path().context("no config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(settings)?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}
