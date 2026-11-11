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
use crate::tray::{Action, Tray};

/// How often to look for added or removed screens.
const SCREEN_POLL: Duration = Duration::from_secs(2);
/// Frame interval when nothing is moving.
const IDLE_INTERVAL: Duration = Duration::from_millis(250);

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
    tray: Option<Tray>,
    last_tick: Instant,
    next_frame: Instant,
    screens_checked: Instant,
    error: Option<anyhow::Error>,
}

impl App {
    pub fn new(settings: Settings, event_loop: &EventLoop<()>) -> Result<Self> {
        let display = Box::new(event_loop.owned_display_handle());
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(display));
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
            tray: None,
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
        log::info!("{} overlay(s) ready", self.overlays.len());
        Ok(())
    }

    fn apply_settings(&mut self, settings: Settings) {
        if settings == self.settings {
            return;
        }
        self.settings = settings.clone();
        self.world.set_settings(settings);
        if let Err(e) = config::save_settings(&self.settings) {
            log::warn!("could not save settings: {e:#}");
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
                Action::OpenSettings => log::info!("settings window is not implemented yet"),
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

        let pointer = self.desktop.pointer();
        self.scene.screens.clear();
        self.scene
            .screens
            .extend(self.screens.iter().map(|s| s.frame));
        self.desktop.obstacles(&mut self.scene.obstacles);
        self.scene.cursor = pointer.pos;
        self.scene.grab = pointer.grab;
        self.scene.mouse_down = pointer.down;
        self.scene.quality = self.desktop.quality();

        self.world.step(dt, &self.scene);
        self.update_click_catching();

        if let Some(renderer) = &mut self.renderer {
            self.draw.build(&self.world);
            renderer.upload(&self.draw);
            for o in &mut self.overlays {
                o.target.set_viewport(o.screen.frame, o.screen.scale);
                renderer.render(&mut o.target);
            }
        }

        let fps = if self.scene.quality < 1.0 {
            self.settings.low_power_fps
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
        if self.overlays.is_empty()
            && let Err(e) = self.rebuild_overlays(event_loop)
        {
            self.fail(event_loop, e);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
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
