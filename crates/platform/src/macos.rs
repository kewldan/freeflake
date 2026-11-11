//! macOS: AppKit for screens, cursor and window setup; CoreGraphics for the
//! list of on-screen windows. Window bounds are readable without the Screen
//! Recording permission, so freeflake needs no permissions at all.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use freeflake_core::{Obstacle, Rect};
use glam::Vec2;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSEvent, NSEventModifierFlags, NSScreen, NSScreenSaverWindowLevel, NSView, NSWindow,
    NSWindowCollectionBehavior,
};
use objc2_core_foundation::CFString;
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGDesktopIconWindowLevel, kCGNullWindowID,
    kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerName,
    kCGWindowOwnerPID,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSNumber, NSPoint, NSProcessInfo, NSProcessInfoThermalState, NSRect,
    NSSize, NSString,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::{Desktop, Layer, Pointer, Screen};

/// `kCGDockWindowLevel`.
const DOCK_LEVEL: i64 = 20;
/// Smallest window that counts as an obstacle.
const MIN_SIZE: f64 = 24.0;
/// Window list polling while something moves, and when all is still.
const FAST_POLL: Duration = Duration::from_millis(16);
const SLOW_POLL: Duration = Duration::from_millis(66);
/// How long to keep polling fast after the last change.
const FAST_LINGER: Duration = Duration::from_millis(600);

pub struct MacDesktop {
    mtm: MainThreadMarker,
    /// Height of the primary screen, to flip Cocoa's bottom-left origin.
    primary_height: f64,
    windows: Arc<Mutex<Vec<Obstacle>>>,
    stop: Arc<AtomicBool>,
}

impl MacDesktop {
    pub fn new() -> Result<Self> {
        let mtm = MainThreadMarker::new().context("freeflake must run on the main thread")?;
        let pid = NSProcessInfo::processInfo().processIdentifier() as i64;
        let windows = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let windows = windows.clone();
            let stop = stop.clone();
            thread::Builder::new()
                .name("window-watcher".into())
                .spawn(move || watch_windows(pid, &windows, &stop))?;
        }
        let mut this = Self {
            mtm,
            primary_height: 0.0,
            windows,
            stop,
        };
        this.refresh_primary();
        Ok(this)
    }

    fn refresh_primary(&mut self) {
        let screens = NSScreen::screens(self.mtm);
        if let Some(primary) = screens.iter().next() {
            self.primary_height = primary.frame().size.height;
        }
    }

    /// Cocoa rect (bottom-left origin) to desktop rect (top-left origin).
    fn cocoa_to_desktop(&self, r: NSRect) -> Rect {
        Rect::new(
            r.origin.x as f32,
            (self.primary_height - r.origin.y - r.size.height) as f32,
            r.size.width as f32,
            r.size.height as f32,
        )
    }

    fn desktop_to_cocoa(&self, r: Rect) -> NSRect {
        NSRect::new(
            NSPoint::new(r.x as f64, self.primary_height - r.y as f64 - r.h as f64),
            NSSize::new(r.w as f64, r.h as f64),
        )
    }

    fn ns_window(&self, window: &dyn HasWindowHandle) -> Result<Retained<NSWindow>> {
        let handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            bail!("not an AppKit window");
        };
        // SAFETY: the handle comes from a live window owned by the caller.
        let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
        view.window().context("view is not in a window")
    }
}

impl Drop for MacDesktop {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Polls the window list on a background thread; the query costs about a
/// millisecond, which would otherwise come out of every frame.
fn watch_windows(pid: i64, shared: &Mutex<Vec<Obstacle>>, stop: &AtomicBool) {
    let keys = Keys::new();
    let mut list = Vec::new();
    let mut last_change = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        autoreleasepool(|_| read_windows(pid, &keys, &mut list));
        {
            let mut current = shared.lock().unwrap_or_else(|e| e.into_inner());
            if *current != list {
                last_change = Instant::now();
                std::mem::swap(&mut *current, &mut list);
            }
        }
        let interval = if last_change.elapsed() < FAST_LINGER {
            FAST_POLL
        } else {
            SLOW_POLL
        };
        thread::sleep(interval);
    }
}

/// Dictionary keys, created once.
struct Keys {
    x: Retained<NSString>,
    y: Retained<NSString>,
    width: Retained<NSString>,
    height: Retained<NSString>,
}

impl Keys {
    fn new() -> Self {
        Self {
            x: NSString::from_str("X"),
            y: NSString::from_str("Y"),
            width: NSString::from_str("Width"),
            height: NSString::from_str("Height"),
        }
    }
}

fn cf_key(k: &CFString) -> &NSString {
    // SAFETY: CFString is toll-free bridged to NSString.
    unsafe { &*(k as *const CFString as *const NSString) }
}

fn read_windows(pid: i64, keys: &Keys, out: &mut Vec<Obstacle>) {
    out.clear();
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return;
    };
    // SAFETY: a CFArray of CFDictionary is toll-free bridged to NSArray of
    // NSDictionary.
    let list: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*list as *const _ as *const NSArray<NSDictionary<NSString, AnyObject>>) };
    // SAFETY: the key constants are valid for the life of the process.
    let (k_number, k_layer, k_bounds, k_alpha, k_pid, k_owner) = unsafe {
        (
            cf_key(kCGWindowNumber),
            cf_key(kCGWindowLayer),
            cf_key(kCGWindowBounds),
            cf_key(kCGWindowAlpha),
            cf_key(kCGWindowOwnerPID),
            cf_key(kCGWindowOwnerName),
        )
    };
    for info in list.iter() {
        let number = |k: &NSString| {
            info.objectForKey(k)
                .and_then(|v| v.downcast::<NSNumber>().ok())
        };
        let layer = number(k_layer).map(|n| n.as_i64()).unwrap_or(0);
        // Wallpaper helpers, notification centre, screen savers and the like.
        if !matches!(layer, 0 | 3 | 8 | DOCK_LEVEL | 24 | 25) {
            continue;
        }
        if number(k_pid).map(|n| n.as_i64()) == Some(pid) {
            continue;
        }
        if number(k_alpha).is_some_and(|a| a.as_f64() < 0.01) {
            continue;
        }
        let Some(id) = number(k_number).map(|n| n.as_i64() as u64) else {
            continue;
        };
        let Some(bounds) = info
            .objectForKey(k_bounds)
            .and_then(|v| v.downcast::<NSDictionary>().ok())
        else {
            continue;
        };
        let field = |k: &NSString| {
            bounds
                .objectForKey(k)
                .and_then(|v| v.downcast::<NSNumber>().ok())
                .map(|n| n.as_f64())
                .unwrap_or(0.0)
        };
        let (x, y) = (field(&keys.x), field(&keys.y));
        let (w, h) = (field(&keys.width), field(&keys.height));
        if w < MIN_SIZE || h < MIN_SIZE {
            continue;
        }
        let holds_snow = match layer {
            // Normal, floating and modal windows.
            0 | 3 | 8 => true,
            DOCK_LEVEL => info
                .objectForKey(k_owner)
                .and_then(|v| v.downcast::<NSString>().ok())
                .is_some_and(|s| s.to_string() == "Dock"),
            // The menu bar hides snow without holding it.
            _ => false,
        };
        if layer == DOCK_LEVEL && !holds_snow {
            continue;
        }
        out.push(Obstacle {
            id,
            rect: Rect::new(x as f32, y as f32, w as f32, h as f32),
            holds_snow,
        });
    }
}

impl Desktop for MacDesktop {
    fn screens(&mut self) -> Vec<Screen> {
        self.refresh_primary();
        NSScreen::screens(self.mtm)
            .iter()
            .map(|s| Screen {
                frame: self.cocoa_to_desktop(s.frame()),
                visible: self.cocoa_to_desktop(s.visibleFrame()),
                scale: s.backingScaleFactor() as f32,
            })
            .collect()
    }

    fn obstacles(&mut self, out: &mut Vec<Obstacle>) {
        out.clear();
        let current = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        out.extend_from_slice(&current);
    }

    fn pointer(&mut self) -> Pointer {
        let p = NSEvent::mouseLocation();
        Pointer {
            pos: Some(Vec2::new(p.x as f32, (self.primary_height - p.y) as f32)),
            grab: NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Option),
            down: NSEvent::pressedMouseButtons() & 1 != 0,
        }
    }

    fn quality(&mut self) -> f32 {
        let info = NSProcessInfo::processInfo();
        let thermal = info.thermalState();
        if thermal == NSProcessInfoThermalState::Critical {
            0.25
        } else if thermal == NSProcessInfoThermalState::Serious || info.isLowPowerModeEnabled() {
            0.5
        } else {
            1.0
        }
    }

    fn configure_window(
        &mut self,
        window: &dyn HasWindowHandle,
        layer: Layer,
        frame: Rect,
    ) -> Result<()> {
        let w = self.ns_window(window)?;
        let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary;
        let level = match layer {
            Layer::Overlay => NSScreenSaverWindowLevel,
            Layer::Desktop => (kCGDesktopIconWindowLevel + 1) as isize,
        };
        w.setLevel(level);
        w.setCollectionBehavior(behavior);
        w.setOpaque(false);
        w.setHasShadow(false);
        w.setIgnoresMouseEvents(true);
        w.setFrame_display(self.desktop_to_cocoa(frame), true);
        w.orderFrontRegardless();
        Ok(())
    }

    fn set_click_through(
        &mut self,
        window: &dyn HasWindowHandle,
        click_through: bool,
    ) -> Result<()> {
        self.ns_window(window)?.setIgnoresMouseEvents(click_through);
        Ok(())
    }
}
