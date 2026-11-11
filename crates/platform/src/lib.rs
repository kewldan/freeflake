//! Native desktop integration: what is on screen, where the cursor is, and
//! how to turn a plain window into a click-through overlay.
//!
//! Every platform implements [`Desktop`]. Coordinates are global desktop
//! points with the origin at the top-left of the primary screen.

use anyhow::Result;
use freeflake_core::{Obstacle, Rect};
use glam::Vec2;
use raw_window_handle::HasWindowHandle;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// A physical display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// Full frame.
    pub frame: Rect,
    /// Frame minus the menu bar, Dock or taskbar.
    pub visible: Rect,
    /// Physical pixels per point.
    pub scale: f32,
}

/// Cursor and modifier state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pointer {
    pub pos: Option<Vec2>,
    /// The grab modifier (Option on macOS, Alt elsewhere) is held.
    pub grab: bool,
    /// The primary button is down.
    pub down: bool,
}

/// Where an overlay window sits in the window stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Above everything, including the Dock and full-screen apps.
    Overlay,
    /// On the desktop, below normal windows.
    Desktop,
}

/// Queries the desktop. Must be used on the main thread.
pub trait Desktop {
    fn screens(&mut self) -> Vec<Screen>;

    /// Fills `out` with on-screen windows, front to back. Our own windows are
    /// excluded.
    fn obstacles(&mut self, out: &mut Vec<Obstacle>);

    fn pointer(&mut self) -> Pointer;

    /// Rendering quality in `(0, 1]`: lower on battery saver or when hot.
    fn quality(&mut self) -> f32;

    /// Turns `window` into a borderless, transparent window at `layer`
    /// covering `frame`, visible on every virtual desktop.
    fn configure_window(
        &mut self,
        window: &dyn HasWindowHandle,
        layer: Layer,
        frame: Rect,
    ) -> Result<()>;

    /// Lets clicks pass through `window` (or catches them).
    fn set_click_through(
        &mut self,
        window: &dyn HasWindowHandle,
        click_through: bool,
    ) -> Result<()>;
}

/// Returns the implementation for the current platform.
pub fn desktop() -> Result<Box<dyn Desktop>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacDesktop::new()?))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WinDesktop::new()?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::X11Desktop::new()?))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        anyhow::bail!("this platform is not supported yet")
    }
}
