//! Windows: Win32 for monitors, windows and input; DWM for real window
//! frames and cloaked (virtual-desktop) windows.
//!
//! The process is per-monitor DPI aware (winit sets that up), so Win32
//! reports physical pixels. Desktop points are physical pixels divided by the
//! primary monitor's scale; with mixed DPI the secondary monitors are
//! slightly off in size but positions stay consistent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use freeflake_core::{Obstacle, Rect};
use glam::Vec2;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetCursorPos, GetWindowLongPtrW,
    GetWindowThreadProcessId, HWND_BOTTOM, HWND_TOPMOST, IsIconic, IsWindowVisible, LWA_ALPHA,
    MONITORINFOF_PRIMARY, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use crate::{Desktop, Layer, Pointer, Screen};

const FAST_POLL: Duration = Duration::from_millis(16);
const SLOW_POLL: Duration = Duration::from_millis(66);
const FAST_LINGER: Duration = Duration::from_millis(600);
/// Smallest window that counts as an obstacle, in pixels.
const MIN_SIZE: i32 = 24;
/// Windows 11 rounds window corners by 8 pixels.
const WINDOW_RADIUS: f32 = 8.0;
/// Taskbars hold snow like the Dock.
const TASKBAR_CLASSES: [&str; 2] = ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

pub struct WinDesktop {
    windows: Arc<Mutex<Vec<Obstacle>>>,
    stop: Arc<AtomicBool>,
    /// Physical pixels per point on the primary monitor, shared with the
    /// watcher thread as `f32` bits.
    scale: Arc<std::sync::atomic::AtomicU32>,
    desktop_windows: Vec<isize>,
}

impl WinDesktop {
    pub fn new() -> Result<Self> {
        let windows = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let scale = Arc::new(std::sync::atomic::AtomicU32::new(1.0f32.to_bits()));
        let mut this = Self {
            windows: windows.clone(),
            stop: stop.clone(),
            scale: scale.clone(),
            desktop_windows: Vec::new(),
        };
        this.screens();
        thread::Builder::new()
            .name("window-watcher".into())
            .spawn(move || watch_windows(&windows, &stop, &scale))?;
        Ok(this)
    }

    fn scale(&self) -> f32 {
        f32::from_bits(self.scale.load(Ordering::Relaxed))
    }
}

impl Drop for WinDesktop {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn rect_to_points(r: RECT, scale: f32) -> Rect {
    Rect::new(
        r.left as f32 / scale,
        r.top as f32 / scale,
        (r.right - r.left) as f32 / scale,
        (r.bottom - r.top) as f32 / scale,
    )
}

fn hwnd_of(window: &dyn HasWindowHandle) -> Result<HWND> {
    let handle = window
        .window_handle()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        bail!("not a Win32 window");
    };
    Ok(HWND(h.hwnd.get() as *mut _))
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

struct EnumState {
    pid: u32,
    scale: f32,
    out: Vec<Obstacle>,
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam points at the EnumState owned by read_windows.
    let state = unsafe { &mut *(lparam.0 as *mut EnumState) };
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == state.pid {
            return BOOL(1);
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return BOOL(1);
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let class = class_name(hwnd);
        let taskbar = TASKBAR_CLASSES.contains(&class.as_str());
        if !taskbar && (ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_TRANSPARENT.0 != 0) {
            return BOOL(1);
        }
        if matches!(class.as_str(), "Progman" | "WorkerW") {
            return BOOL(1);
        }
        let mut r = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut _,
            size_of::<RECT>() as u32,
        )
        .is_err()
        {
            return BOOL(1);
        }
        if r.right - r.left < MIN_SIZE || r.bottom - r.top < MIN_SIZE {
            return BOOL(1);
        }
        state.out.push(Obstacle {
            id: hwnd.0 as usize as u64,
            rect: rect_to_points(r, state.scale),
            holds_snow: true,
            corner_radius: WINDOW_RADIUS,
        });
    }
    BOOL(1)
}

fn watch_windows(
    shared: &Mutex<Vec<Obstacle>>,
    stop: &AtomicBool,
    scale: &std::sync::atomic::AtomicU32,
) {
    let pid = unsafe { GetCurrentProcessId() };
    let mut last_change = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let mut state = EnumState {
            pid,
            scale: f32::from_bits(scale.load(Ordering::Relaxed)),
            out: Vec::new(),
        };
        // EnumWindows walks top-level windows front to back.
        let _ = unsafe { EnumWindows(Some(enum_window), LPARAM(&mut state as *mut _ as isize)) };
        {
            let mut current = shared.lock().unwrap_or_else(|e| e.into_inner());
            if *current != state.out {
                last_change = Instant::now();
                *current = state.out;
            }
        }
        thread::sleep(if last_change.elapsed() < FAST_LINGER {
            FAST_POLL
        } else {
            SLOW_POLL
        });
    }
}

struct MonitorList(Vec<(RECT, RECT, f32, bool)>);

unsafe extern "system" fn enum_monitor(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    // SAFETY: lparam points at the MonitorList owned by screens().
    let list = unsafe { &mut *(lparam.0 as *mut MonitorList) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let primary = info.dwFlags & MONITORINFOF_PRIMARY != 0;
            list.0
                .push((info.rcMonitor, info.rcWork, dx as f32 / 96.0, primary));
        }
    }
    BOOL(1)
}

impl Desktop for WinDesktop {
    fn screens(&mut self) -> Vec<Screen> {
        let mut list = MonitorList(Vec::new());
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(enum_monitor),
                LPARAM(&mut list as *mut _ as isize),
            );
        }
        // Primary first, like on macOS.
        list.0.sort_by_key(|m| !m.3);
        if let Some(primary) = list.0.first() {
            self.scale.store(primary.2.to_bits(), Ordering::Relaxed);
        }
        let scale = self.scale();
        list.0
            .iter()
            .map(|&(frame, work, _, _)| Screen {
                frame: rect_to_points(frame, scale),
                visible: rect_to_points(work, scale),
                scale,
            })
            .collect()
    }

    fn obstacles(&mut self, out: &mut Vec<Obstacle>) {
        out.clear();
        let current = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        out.extend_from_slice(&current);
        drop(current);
        // Clicking the desktop can raise our desktop-level windows; push them
        // back down.
        for &h in &self.desktop_windows {
            unsafe {
                let _ = SetWindowPos(
                    HWND(h as *mut _),
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    fn pointer(&mut self) -> Pointer {
        let mut p = POINT::default();
        let pos = unsafe { GetCursorPos(&mut p) }
            .ok()
            .map(|_| Vec2::new(p.x as f32, p.y as f32) / self.scale());
        let down = |vk: u16| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
        Pointer {
            pos,
            grab: down(VK_MENU.0),
            down: down(VK_LBUTTON.0),
        }
    }

    fn quality(&mut self) -> f32 {
        let mut status = SYSTEM_POWER_STATUS::default();
        let saver = unsafe { GetSystemPowerStatus(&mut status) }.is_ok()
            && status.SystemStatusFlag & 1 != 0;
        if saver { 0.5 } else { 1.0 }
    }

    fn configure_window(
        &mut self,
        window: &dyn HasWindowHandle,
        layer: Layer,
        frame: Rect,
    ) -> Result<()> {
        let hwnd = hwnd_of(window)?;
        let scale = self.scale();
        unsafe {
            let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            ex |= WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0;
            if layer == Layer::Overlay {
                ex |= WS_EX_TOPMOST.0;
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
            SetLayeredWindowAttributes(hwnd, Default::default(), 255, LWA_ALPHA)?;
            let after = match layer {
                Layer::Overlay => HWND_TOPMOST,
                Layer::Desktop => HWND_BOTTOM,
            };
            SetWindowPos(
                hwnd,
                Some(after),
                (frame.x * scale).round() as i32,
                (frame.y * scale).round() as i32,
                (frame.w * scale).round() as i32,
                (frame.h * scale).round() as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        if layer == Layer::Desktop {
            self.desktop_windows.push(hwnd.0 as isize);
        }
        Ok(())
    }

    fn set_click_through(
        &mut self,
        window: &dyn HasWindowHandle,
        click_through: bool,
    ) -> Result<()> {
        let hwnd = hwnd_of(window)?;
        unsafe {
            let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if click_through {
                ex |= WS_EX_TRANSPARENT.0;
            } else {
                ex &= !WS_EX_TRANSPARENT.0;
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        }
        Ok(())
    }
}
