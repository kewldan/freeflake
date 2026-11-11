//! freeflake: snow on your desktop.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod autostart;
mod config;
mod i18n;
mod settings_window;
mod tray;
mod tree_window;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,freeflake=info"),
    )
    .init();

    let mut builder = EventLoop::builder();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        // No Dock icon, no app menu: freeflake lives in the menu bar.
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        // Overlays need X11 features (override-redirect, input shapes); on
        // Wayland sessions this runs through XWayland.
        builder.with_x11();
    }
    let event_loop = builder.build()?;
    let mut app = app::App::new(config::load_settings(), &event_loop)?;
    event_loop.run_app(&mut app)?;
    app.into_result()
}
