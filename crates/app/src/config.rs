//! Loading and saving settings.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use freeflake_core::Settings;
use freeflake_core::advent::AdventState;

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

fn advent_path() -> Option<PathBuf> {
    dir().map(|d| d.join("advent.json"))
}

pub fn load_advent() -> AdventState {
    advent_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_advent(state: &AdventState) -> Result<()> {
    let path = advent_path().context("no config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(state)?)
        .with_context(|| format!("writing {}", path.display()))
}

/// Today's date; `FREEFLAKE_DATE=2026-12-24` overrides it for testing.
pub fn today() -> chrono::NaiveDate {
    std::env::var("FREEFLAKE_DATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| chrono::Local::now().date_naive())
}
