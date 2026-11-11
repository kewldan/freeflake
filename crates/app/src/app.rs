//! The event loop: overlay windows, frame pacing and the tray.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use freeflake_core::{Scene, Settings, World};
use freeflake_platform::{Desktop, Layer, Screen};
use freeflake_render::{DrawList, Gpu, Renderer, Target, wgpu};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use crate::config;
use crate::settings_window::{Request, SettingsWindow};
use crate::tray::{Action, Tray};
use crate::tree_window::TreeWindow;

/// How often to look for added or removed screens.
const SCREEN_POLL: Duration = Duration::from_secs(2);
/// How long after the last fast motion the frame rate may drop.
const CALM_AFTER: Duration = Duration::from_millis(800);
/// Frame interval when nothing is moving.
const IDLE_INTERVAL: Duration = Duration::from_millis(250);

/// Per-phase frame timings, logged when `FREEFLAKE_PROFILE` is set.
#[derive(Default)]
struct Profile {
    enabled: bool,
    frames: u32,
    phases: [f64; 5],
    lively: u32,
    moved: u32,
    since: Option<Instant>,
}

impl Profile {
    const NAMES: [&str; 5] = ["desktop", "tree", "step", "overlay", "tree-render"];

    fn add(&mut self, phase: usize, from: Instant) -> Instant {
        let now = Instant::now();
        self.phases[phase] += (now - from).as_secs_f64();
        now
    }

    fn frame(&mut self) {
        if !self.enabled {
            return;
        }
        self.frames += 1;
        let since = *self.since.get_or_insert_with(Instant::now);
        if since.elapsed() > Duration::from_secs(5) {
            let report: Vec<String> = Self::NAMES
                .iter()
                .zip(self.phases)
                .map(|(n, t)| format!("{n} {:.2}ms", t * 1000.0 / self.frames as f64))
                .collect();
            log::info!(
                "{} fps, {}; lively {} moved {}",
                self.frames / 5,
                report.join(", "),
                self.lively,
                self.moved
            );
            *self = Self {
                enabled: true,
                ..Self::default()
            };
        }
    }
}

struct Overlay {
    window: Arc<Window>,
    target: Target,
    screen: Screen,
    catches_clicks: bool,
}

pub struct App {
    settings: Settings,
    world: World,
    desktop: Box<dyn Desktop>,
    instance: wgpu::Instance,
    renderer: Option<Renderer>,
    overlays: Vec<Overlay>,
    screens: Vec<Screen>,
    scene: Scene,
    draw: DrawList,
    last_obstacles: Vec<freeflake_core::Obstacle>,
    last_activity: Instant,
    /// Areas where falling snow is hidden (windows, when snow over windows
    /// is off).
    hidden: Vec<freeflake_core::Rect>,
    tray: Option<Tray>,
    tree: Option<TreeWindow>,
    settings_window: Option<SettingsWindow>,
    autostart: bool,
    date_checked: Instant,
    profile: Profile,
    last_tick: Instant,
    next_frame: Instant,
    screens_checked: Instant,
    error: Option<anyhow::Error>,
}

impl App {
    pub fn new(settings: Settings, event_loop: &EventLoop<()>) -> Result<Self> {
        let display = Box::new(event_loop.owned_display_handle());
        #[allow(unused_mut)]
        let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(display);
        #[cfg(target_os = "windows")]
        {
            // Only DirectComposition swapchains support per-pixel transparency.
            desc.backends = wgpu::Backends::DX12;
            desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        }
        let instance = wgpu::Instance::new(desc);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let now = Instant::now();
        Ok(Self {
            world: World::new(settings.clone(), seed),
            settings,
            desktop: freeflake_platform::desktop()?,
            instance,
            renderer: None,
            overlays: Vec::new(),
            screens: Vec::new(),
            scene: Scene::default(),
            draw: DrawList::default(),
            last_obstacles: Vec::new(),
            last_activity: now,
            hidden: Vec::new(),
            tray: None,
            tree: None,
            settings_window: None,
            autostart: crate::autostart::is_enabled(),
            date_checked: now,
            profile: Profile {
                enabled: std::env::var_os("FREEFLAKE_PROFILE").is_some(),
                ..Profile::default()
            },
            last_tick: now,
            next_frame: now,
            screens_checked: now,
            error: None,
        })
    }

    /// The error that stopped the event loop, if any.
    pub fn into_result(self) -> Result<()> {
        self.error.map_or(Ok(()), Err)
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        log::error!("{error:#}");
        self.error = Some(error);
        event_loop.exit();
    }

    /// (Re)creates one overlay per screen.
    fn rebuild_overlays(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        self.overlays.clear();
        self.screens = self.desktop.screens();
        for screen in self.screens.clone() {
            let frame = screen.frame;
            let attrs = Window::default_attributes()
                .with_title("freeflake")
                .with_transparent(true)
                .with_decorations(false)
                .with_resizable(false)
                .with_active(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_position(LogicalPosition::new(frame.x as f64, frame.y as f64))
                .with_inner_size(LogicalSize::new(frame.w as f64, frame.h as f64));
            #[cfg(target_os = "linux")]
            let attrs = {
                use winit::platform::x11::WindowAttributesExtX11;
                // Unmanaged by the window manager: no decorations, no focus,
                // always above.
                attrs.with_override_redirect(true)
            };
            let window = Arc::new(event_loop.create_window(attrs)?);
            self.desktop
                .configure_window(window.as_ref(), Layer::Overlay, frame)?;
            window.set_cursor_hittest(false).ok();

            let surface = self
                .instance
                .create_surface(window.clone())
                .context("creating overlay surface")?;
            if self.renderer.is_none() {
                let gpu = Gpu::new(self.instance.clone(), &surface)?;
                self.renderer = Some(Renderer::new(gpu, &surface)?);
            }
            let renderer = self.renderer.as_ref().expect("renderer exists");
            let target = renderer.create_target(surface, frame, screen.scale)?;
            self.overlays.push(Overlay {
                window,
                target,
                screen,
                catches_clicks: false,
            });
        }
        let state = self
            .tree
            .take()
            .map(|t| t.state().clone())
            .unwrap_or_else(config::load_advent);
        if let (Some(renderer), Some(primary)) = (&self.renderer, self.screens.first()) {
            let mut tree = TreeWindow::create(
                event_loop,
                self.desktop.as_mut(),
                &self.instance,
                renderer,
                primary,
                state,
            )?;
            tree.set_visible(self.settings.show_tree);
            self.tree = Some(tree);
        }
        log::info!("{} overlay(s) ready", self.overlays.len());
        Ok(())
    }

    fn apply_settings(&mut self, settings: Settings) {
        if settings == self.settings {
            return;
        }
        self.settings = settings.clone();
        self.world.set_settings(settings);
        if let Some(tree) = &mut self.tree {
            tree.set_visible(self.settings.show_tree);
        }
        if let Err(e) = config::save_settings(&self.settings) {
            log::warn!("could not save settings: {e:#}");
        }
    }

    fn open_settings(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.settings_window {
            w.focus();
            return;
        }
        let Some(renderer) = &self.renderer else {
            return;
        };
        match SettingsWindow::open(event_loop, renderer.gpu()) {
            Ok(w) => self.settings_window = Some(w),
            Err(e) => log::error!("settings window: {e:#}"),
        }
    }

    fn settings_window_event(&mut self, event: WindowEvent) {
        let (Some(w), Some(renderer)) = (&mut self.settings_window, &self.renderer) else {
            return;
        };
        let gpu = renderer.gpu().clone();
        if matches!(event, WindowEvent::RedrawRequested) {
            let mut settings = self.settings.clone();
            let requests = w.redraw(&gpu, &mut settings, self.autostart);
            self.apply_settings(settings.sanitized());
            if let Some(tray) = &self.tray {
                tray.sync(&self.settings);
            }
            for r in requests {
                match r {
                    Request::Shovel => self.world.shovel(),
                    Request::SetAutostart(on) => match crate::autostart::set_enabled(on) {
                        Ok(()) => self.autostart = on,
                        Err(e) => log::warn!("autostart: {e:#}"),
                    },
                }
            }
        } else if !w.on_event(&gpu, &event) {
            self.settings_window = None;
        }
    }

    fn handle_tray(&mut self, event_loop: &ActiveEventLoop) {
        let Some(tray) = &self.tray else {
            return;
        };
        for action in tray.poll(&self.settings) {
            match action {
                Action::Settings(s) => self.apply_settings(s),
                Action::Shovel => self.world.shovel(),
                Action::OpenSettings => self.open_settings(event_loop),
                Action::Quit => event_loop.exit(),
            }
        }
    }

    /// While the grab modifier is held over snow or a snowball, the overlay
    /// under the cursor takes clicks so they do not reach the app below.
    fn update_click_catching(&mut self) {
        let catch_at = self.scene.cursor.filter(|&p| {
            self.world.is_holding() || (self.scene.grab && self.world.wants_pointer(p))
        });
        for o in &mut self.overlays {
            let catch = catch_at.is_some_and(|p| o.screen.frame.contains(p));
            if catch != o.catches_clicks {
                o.catches_clicks = catch;
                if let Err(e) = self.desktop.set_click_through(o.window.as_ref(), !catch) {
                    log::warn!("could not toggle click-through: {e:#}");
                }
            }
        }
    }

    fn tick(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;

        self.handle_tray(event_loop);

        if now - self.screens_checked > SCREEN_POLL {
            self.screens_checked = now;
            if self.desktop.screens() != self.screens {
                log::info!("screen layout changed");
                self.rebuild_overlays(event_loop)?;
            }
        }

        let t = Instant::now();
        let pointer = self.desktop.pointer();
        self.scene.screens.clear();
        self.scene
            .screens
            .extend(self.screens.iter().map(|s| s.frame));
        self.desktop.obstacles(&mut self.scene.obstacles);
        let t = self.profile.add(0, t);
        if now - self.date_checked > Duration::from_secs(30) {
            self.date_checked = now;
            if let Some(tree) = &mut self.tree {
                tree.refresh_date();
            }
        }
        if let Some(tree) = &mut self.tree {
            let occluded = pointer
                .pos
                .is_some_and(|p| self.scene.occluded(p, usize::MAX));
            if tree.update(dt, pointer, occluded, self.desktop.as_mut())
                && let Err(e) = config::save_advent(tree.state())
            {
                log::warn!("could not save the tree: {e:#}");
            }
            self.scene.obstacles.extend(tree.ledges());
        }
        self.scene.cursor = pointer.pos;
        self.scene.grab = pointer.grab;
        self.scene.mouse_down = pointer.down;
        self.scene.quality = self.desktop.quality();

        let t = self.profile.add(1, t);
        self.world.step(dt, &self.scene);
        let t = self.profile.add(2, t);
        self.update_click_catching();

        if let Some(renderer) = &mut self.renderer {
            self.hidden.clear();
            if !self.settings.snow_over_windows {
                self.hidden.extend(
                    self.scene
                        .obstacles
                        .iter()
                        .filter(|o| !crate::tree_window::is_tree_ledge(o.id))
                        .map(|o| o.rect),
                );
            }
            self.draw.build(&self.world, &self.hidden);
            for o in &mut self.overlays {
                o.target.set_viewport(o.screen.frame, o.screen.scale);
                renderer.render(&mut o.target, &self.draw);
            }
            let t = self.profile.add(3, t);
            if let Some(tree) = &mut self.tree {
                let covered = tree.covered_by(&self.scene.obstacles);
                tree.render(renderer, covered);
            }
            self.profile.add(4, t);
            self.profile.frame();
        }

        let lively = self.world.is_lively();
        let moved = self.scene.obstacles != self.last_obstacles;
        if lively || moved {
            self.last_activity = now;
        }
        if self.profile.enabled {
            self.profile.lively += lively as u32;
            self.profile.moved += moved as u32;
        }
        std::mem::swap(&mut self.last_obstacles, &mut self.scene.obstacles);
        self.scene.obstacles.clone_from(&self.last_obstacles);
        let calm = self.settings.save_power && now - self.last_activity > CALM_AFTER;
        let fps = if self.scene.quality < 1.0 {
            self.settings.low_power_fps
        } else if calm {
            self.settings.calm_fps
        } else {
            self.settings.fps
        };
        let interval = if self.settings.paused && self.world.flakes().is_empty() {
            IDLE_INTERVAL
        } else {
            Duration::from_secs_f64(1.0 / fps as f64)
        };
        self.next_frame = (self.next_frame + interval).max(now + interval / 2);
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if cause == StartCause::Init {
            match Tray::new(&self.settings) {
                Ok(tray) => self.tray = Some(tray),
                Err(e) => log::error!("tray icon unavailable: {e:#}"),
            }
        }
        let _ = event_loop;
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.overlays.is_empty() {
            if let Err(e) = self.rebuild_overlays(event_loop) {
                self.fail(event_loop, e);
                return;
            }
            if std::env::args().any(|a| a == "--settings") {
                self.open_settings(event_loop);
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.settings_window.as_ref().is_some_and(|w| w.id() == id) {
            self.settings_window_event(event);
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(o) = self.overlays.iter_mut().find(|o| o.window.id() == id) {
                    o.screen.scale = scale_factor as f32;
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if Instant::now() >= self.next_frame
            && let Err(e) = self.tick(event_loop)
        {
            self.fail(event_loop, e);
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
