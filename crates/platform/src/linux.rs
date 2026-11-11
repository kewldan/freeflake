//! Linux on X11 (and XWayland): RandR for monitors, EWMH for the window
//! stack, XFixes for click-through input regions.
//!
//! On Wayland sessions the app runs through XWayland. The overlay then works
//! as on X11, while the window list only covers X11 clients; native Wayland
//! windows are invisible to us, so snow settles on the screen bottom and
//! panels there.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use freeflake_core::{Obstacle, Rect};
use glam::Vec2;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::SK;
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, EventMask,
    KeyButMask, StackMode, Window,
};
use x11rb::rust_connection::RustConnection;

use crate::{Desktop, Layer, Pointer, Screen};

const FAST_POLL: Duration = Duration::from_millis(16);
const SLOW_POLL: Duration = Duration::from_millis(66);
const FAST_LINGER: Duration = Duration::from_millis(600);
const MIN_SIZE: u32 = 24;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_CLIENT_LIST_STACKING,
        _NET_CURRENT_DESKTOP,
        _NET_WM_DESKTOP,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_BELOW,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_PID,
        _NET_FRAME_EXTENTS,
        _NET_WORKAREA,
        RESOURCE_MANAGER,
    }
}

pub struct X11Desktop {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    windows: Arc<Mutex<Vec<Obstacle>>>,
    stop: Arc<AtomicBool>,
    scale: Arc<AtomicU32>,
    /// Desktop-level windows, lowered again whenever something raises them.
    desktop_windows: Vec<Window>,
}

impl X11Desktop {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None).context("cannot connect to X server")?;
        let root = conn.setup().roots[screen_num].root;
        let atoms = Atoms::new(&conn)?.reply()?;
        let scale = Arc::new(AtomicU32::new(xft_scale(&conn, root, &atoms).to_bits()));
        let windows = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (windows, stop, scale) = (windows.clone(), stop.clone(), scale.clone());
            thread::Builder::new()
                .name("window-watcher".into())
                .spawn(move || {
                    if let Err(e) = watch_windows(&windows, &stop, &scale) {
                        log::error!("window watcher stopped: {e:#}");
                    }
                })?;
        }
        Ok(Self {
            conn,
            root,
            atoms,
            windows,
            stop,
            scale,
            desktop_windows: Vec::new(),
        })
    }

    fn scale(&self) -> f32 {
        f32::from_bits(self.scale.load(Ordering::Relaxed))
    }

    fn xid(&self, window: &dyn HasWindowHandle) -> Result<Window> {
        let handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        match handle.as_raw() {
            RawWindowHandle::Xlib(h) => Ok(h.window as Window),
            RawWindowHandle::Xcb(h) => Ok(h.window.get()),
            _ => bail!("not an X11 window"),
        }
    }

    /// Asks the window manager to add EWMH states to a mapped window.
    fn add_states(&self, win: Window, states: &[Atom]) -> Result<()> {
        for pair in states.chunks(2) {
            let data = [1, pair[0], pair.get(1).copied().unwrap_or(0), 1, 0];
            let event = ClientMessageEvent::new(32, win, self.atoms._NET_WM_STATE, data);
            self.conn.send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )?;
        }
        Ok(())
    }
}

impl Drop for X11Desktop {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// `Xft.dpi` from the X resource database, as a scale factor.
fn xft_scale(conn: &RustConnection, root: Window, atoms: &Atoms) -> f32 {
    let Ok(Ok(reply)) = conn
        .get_property(
            false,
            root,
            atoms.RESOURCE_MANAGER,
            AtomEnum::STRING,
            0,
            1 << 16,
        )
        .map(|c| c.reply())
    else {
        return 1.0;
    };
    String::from_utf8_lossy(&reply.value)
        .lines()
        .find_map(|l| l.strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|dpi| (dpi / 96.0).max(1.0))
        .unwrap_or(1.0)
}

fn cardinals(conn: &RustConnection, win: Window, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
    conn.get_property(false, win, prop, ty, 0, 4096)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|v| v.collect()))
        .unwrap_or_default()
}

fn watch_windows(
    shared: &Mutex<Vec<Obstacle>>,
    stop: &AtomicBool,
    scale: &AtomicU32,
) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen_num].root;
    let atoms = Atoms::new(&conn)?.reply()?;
    let pid = std::process::id();
    let mut last_change = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let list = read_windows(
            &conn,
            root,
            &atoms,
            pid,
            f32::from_bits(scale.load(Ordering::Relaxed)),
        )
        .unwrap_or_default();
        {
            let mut current = shared.lock().unwrap_or_else(|e| e.into_inner());
            if *current != list {
                last_change = Instant::now();
                *current = list;
            }
        }
        thread::sleep(if last_change.elapsed() < FAST_LINGER {
            FAST_POLL
        } else {
            SLOW_POLL
        });
    }
    Ok(())
}

/// Reads the window stack. Requests for all windows are sent before any
/// reply is awaited, so the whole list costs a handful of round trips.
fn read_windows(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
    pid: u32,
    scale: f32,
) -> Result<Vec<Obstacle>> {
    let stacking = cardinals(
        conn,
        root,
        atoms._NET_CLIENT_LIST_STACKING,
        AtomEnum::WINDOW,
    );
    let current_desktop = cardinals(conn, root, atoms._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL)
        .first()
        .copied();
    let prop = |w, p: Atom, t: AtomEnum| conn.get_property(false, w, p, t, 0, 64);
    let mut pending = Vec::with_capacity(stacking.len());
    for &win in stacking.iter().rev() {
        pending.push((
            win,
            conn.get_window_attributes(win)?,
            conn.get_geometry(win)?,
            conn.translate_coordinates(win, root, 0, 0)?,
            prop(win, atoms._NET_WM_STATE, AtomEnum::ATOM)?,
            prop(win, atoms._NET_WM_DESKTOP, AtomEnum::CARDINAL)?,
            prop(win, atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM)?,
            prop(win, atoms._NET_FRAME_EXTENTS, AtomEnum::CARDINAL)?,
            prop(win, atoms._NET_WM_PID, AtomEnum::CARDINAL)?,
        ));
    }
    let mut out = Vec::new();
    for (win, attrs, geom, pos, state, desktop, kind, extents, wpid) in pending {
        let (Ok(attrs), Ok(geom), Ok(pos)) = (attrs.reply(), geom.reply(), pos.reply()) else {
            continue;
        };
        let values = |c: x11rb::cookie::Cookie<'_, RustConnection, _>| -> Vec<u32> {
            let r: Result<x11rb::protocol::xproto::GetPropertyReply, _> = c.reply();
            r.ok()
                .and_then(|r| r.value32().map(|v| v.collect()))
                .unwrap_or_default()
        };
        let (state, desktop, kind, extents, wpid) = (
            values(state),
            values(desktop),
            values(kind),
            values(extents),
            values(wpid),
        );
        if attrs.map_state != x11rb::protocol::xproto::MapState::VIEWABLE
            || state.contains(&atoms._NET_WM_STATE_HIDDEN)
            || kind.contains(&atoms._NET_WM_WINDOW_TYPE_DESKTOP)
            || wpid.first() == Some(&pid)
        {
            continue;
        }
        if let (Some(&d), Some(cur)) = (desktop.first(), current_desktop)
            && d != cur
            && d != u32::MAX
        {
            continue;
        }
        if (geom.width as u32) < MIN_SIZE || (geom.height as u32) < MIN_SIZE {
            continue;
        }
        let [left, right, top, bottom] = match extents.as_slice() {
            [l, r, t, b] => [*l as i32, *r as i32, *t as i32, *b as i32],
            _ => [0; 4],
        };
        let x = pos.dst_x as i32 - left;
        let y = pos.dst_y as i32 - top;
        let w = geom.width as i32 + left + right;
        let h = geom.height as i32 + top + bottom;
        out.push(Obstacle {
            id: win as u64,
            rect: Rect::new(
                x as f32 / scale,
                y as f32 / scale,
                w as f32 / scale,
                h as f32 / scale,
            ),
            holds_snow: true,
            corner_radius: 0.0,
        });
    }
    Ok(out)
}

impl Desktop for X11Desktop {
    fn screens(&mut self) -> Vec<Screen> {
        let scale = xft_scale(&self.conn, self.root, &self.atoms);
        self.scale.store(scale.to_bits(), Ordering::Relaxed);
        let workarea = cardinals(
            &self.conn,
            self.root,
            self.atoms._NET_WORKAREA,
            AtomEnum::CARDINAL,
        );
        let work = match workarea.as_slice() {
            [x, y, w, h, ..] => Some(Rect::new(*x as f32, *y as f32, *w as f32, *h as f32)),
            _ => None,
        };
        let monitors = self
            .conn
            .randr_get_monitors(self.root, true)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.monitors)
            .unwrap_or_default();
        let mut screens: Vec<(bool, Screen)> = monitors
            .iter()
            .map(|m| {
                let px = Rect::new(m.x as f32, m.y as f32, m.width as f32, m.height as f32);
                // The work area spans all monitors; clip it to this one.
                let vis = work
                    .filter(|w| w.intersects(&px))
                    .map(|w| {
                        let x = w.x.max(px.x);
                        let y = w.y.max(px.y);
                        Rect::new(
                            x,
                            y,
                            w.right().min(px.right()) - x,
                            w.bottom().min(px.bottom()) - y,
                        )
                    })
                    .unwrap_or(px);
                let pts = |r: Rect| Rect::new(r.x / scale, r.y / scale, r.w / scale, r.h / scale);
                (
                    m.primary,
                    Screen {
                        frame: pts(px),
                        visible: pts(vis),
                        scale,
                    },
                )
            })
            .collect();
        screens.sort_by_key(|(primary, _)| !*primary);
        if screens.is_empty() {
            let s = &self.conn.setup().roots[0];
            let frame = Rect::new(
                0.0,
                0.0,
                s.width_in_pixels as f32 / scale,
                s.height_in_pixels as f32 / scale,
            );
            return vec![Screen {
                frame,
                visible: frame,
                scale,
            }];
        }
        screens.into_iter().map(|(_, s)| s).collect()
    }

    fn obstacles(&mut self, out: &mut Vec<Obstacle>) {
        out.clear();
        out.extend_from_slice(&self.windows.lock().unwrap_or_else(|e| e.into_inner()));
        for &w in &self.desktop_windows {
            let _ = self
                .conn
                .configure_window(w, &ConfigureWindowAux::new().stack_mode(StackMode::BELOW));
        }
        let _ = self.conn.flush();
    }

    fn pointer(&mut self) -> Pointer {
        let Ok(Ok(p)) = self.conn.query_pointer(self.root).map(|c| c.reply()) else {
            return Pointer::default();
        };
        let scale = self.scale();
        Pointer {
            pos: Some(Vec2::new(p.root_x as f32, p.root_y as f32) / scale),
            grab: p.mask.contains(KeyButMask::MOD1),
            down: p.mask.contains(KeyButMask::BUTTON1),
        }
    }

    fn quality(&mut self) -> f32 {
        let profile =
            std::fs::read_to_string("/sys/firmware/acpi/platform_profile").unwrap_or_default();
        if profile.trim() == "low-power" {
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
        let win = self.xid(window)?;
        let scale = self.scale();
        let aux = ConfigureWindowAux::new()
            .x((frame.x * scale).round() as i32)
            .y((frame.y * scale).round() as i32)
            .width((frame.w * scale).round() as u32)
            .height((frame.h * scale).round() as u32)
            .stack_mode(match layer {
                Layer::Overlay => StackMode::ABOVE,
                Layer::Desktop => StackMode::BELOW,
            });
        self.conn.configure_window(win, &aux)?;
        let a = &self.atoms;
        let layer_state = match layer {
            Layer::Overlay => a._NET_WM_STATE_ABOVE,
            Layer::Desktop => a._NET_WM_STATE_BELOW,
        };
        self.add_states(
            win,
            &[
                layer_state,
                a._NET_WM_STATE_STICKY,
                a._NET_WM_STATE_SKIP_TASKBAR,
                a._NET_WM_STATE_SKIP_PAGER,
            ],
        )?;
        if layer == Layer::Desktop {
            self.desktop_windows.push(win);
        }
        self.set_click_through_xid(win, true)?;
        self.conn.flush()?;
        Ok(())
    }

    fn set_click_through(
        &mut self,
        window: &dyn HasWindowHandle,
        click_through: bool,
    ) -> Result<()> {
        let win = self.xid(window)?;
        self.set_click_through_xid(win, click_through)?;
        self.conn.flush()?;
        Ok(())
    }
}

impl X11Desktop {
    /// An empty input region lets clicks fall through; `NONE` restores the
    /// default (the whole window).
    fn set_click_through_xid(&self, win: Window, click_through: bool) -> Result<()> {
        self.conn.xfixes_query_version(5, 0)?.reply()?;
        if click_through {
            let region = self.conn.generate_id()?;
            self.conn.xfixes_create_region(region, &[])?;
            self.conn
                .xfixes_set_window_shape_region(win, SK::INPUT, 0, 0, region)?;
            self.conn.xfixes_destroy_region(region)?;
        } else {
            self.conn
                .xfixes_set_window_shape_region(win, SK::INPUT, 0, 0, x11rb::NONE)?;
        }
        Ok(())
    }
}
