//! Starting freeflake at login.

use anyhow::{Context, Result};
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

fn launcher() -> Result<AutoLaunch> {
    let exe = std::env::current_exe().context("cannot find own executable")?;
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name("freeflake")
        .set_app_path(&exe.to_string_lossy());
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    Ok(builder.build()?)
}

pub fn is_enabled() -> bool {
    launcher()
        .and_then(|l| Ok(l.is_enabled()?))
        .unwrap_or(false)
}

pub fn set_enabled(on: bool) -> Result<()> {
    let l = launcher()?;
    if on {
        l.enable()?
    } else {
        l.disable()?
    }
    Ok(())
}
