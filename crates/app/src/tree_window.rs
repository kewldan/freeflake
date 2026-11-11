//! The advent tree's own window, sitting on the desktop below other windows.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use freeflake_core::advent::{AdventState, TREE_SIZE, Tree};
use freeflake_core::{Obstacle, Rect};
use freeflake_platform::{Desktop, Layer, Pointer, Screen};
use freeflake_render::{DrawList, Renderer, Target, wgpu};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

/// Frame interval for the tree when nobody touches it: the lights blink
/// slowly enough for 20 fps.
const IDLE_FRAME: Duration = Duration::from_millis(50);

/// Obstacle ids for the tree's branches; far above any real window number.
const LEDGE_ID_BASE: u64 = 1 << 48;

pub struct TreeWindow {
    window: Arc<Window>,
    target: Target,
    frame: Rect,
    tree: Tree,
    list: DrawList,
    catches_clicks: bool,
    was_down: bool,
    visible: bool,
    /// Render on the next frame regardless of the idle rate.
    urgent: bool,
    last_render: Instant,
}

impl TreeWindow {
    pub fn create(
        event_loop: &ActiveEventLoop,
        desktop: &mut dyn Desktop,
        instance: &wgpu::Instance,
        renderer: &Renderer,
        screen: &Screen,
        state: AdventState,
    ) -> Result<Self> {
        let vis = screen.visible;
        let frame = Rect::new(
            vis.right() - TREE_SIZE.x - 24.0,
            vis.bottom() - TREE_SIZE.y - 4.0,
            TREE_SIZE.x,
            TREE_SIZE.y,
        );
        let attrs = Window::default_attributes()
            .with_title("freeflake tree")
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_active(false)
            .with_position(LogicalPosition::new(frame.x as f64, frame.y as f64))
            .with_inner_size(LogicalSize::new(frame.w as f64, frame.h as f64));
        let window = Arc::new(event_loop.create_window(attrs)?);
        desktop.configure_window(window.as_ref(), Layer::Desktop, frame)?;
        let surface = instance
            .create_surface(window.clone())
            .context("creating tree surface")?;
        let local = Rect::new(0.0, 0.0, frame.w, frame.h);
        let target = renderer.create_target(surface, local, screen.scale)?;
        Ok(Self {
            window,
            target,
            frame,
            tree: Tree::new(state, crate::config::today()),
            list: DrawList::default(),
            catches_clicks: false,
            was_down: false,
            visible: true,
            urgent: true,
            last_render: Instant::now(),
        })
    }

    pub fn state(&self) -> &AdventState {
        self.tree.state()
    }

    pub fn set_visible(&mut self, visible: bool) {
        if visible != self.visible {
            self.visible = visible;
            self.urgent = true;
            self.window.set_visible(visible);
        }
    }

    pub fn refresh_date(&mut self) {
        self.tree.set_today(crate::config::today());
    }

    /// Branch ledges as snow-holding obstacles in desktop points.
    pub fn ledges(&self) -> impl Iterator<Item = Obstacle> + '_ {
        Tree::ledges()
            .into_iter()
            .enumerate()
            .filter(|_| self.visible)
            .map(|(i, r)| Obstacle {
                id: LEDGE_ID_BASE + i as u64,
                rect: Rect::new(r.x + self.frame.x, r.y + self.frame.y, r.w, r.h),
                holds_snow: true,
            })
    }

    /// Handles the pointer. `occluded` tells whether another window covers
    /// the cursor. Returns true when the advent state changed.
    pub fn update(
        &mut self,
        dt: f32,
        pointer: Pointer,
        occluded: bool,
        desktop: &mut dyn Desktop,
    ) -> bool {
        self.tree.advance(dt);
        if !self.visible {
            return false;
        }
        let local = pointer.pos.map(|p| p - self.frame.origin());
        let bounds = Rect::new(0.0, 0.0, self.frame.w, self.frame.h);
        let inside = local.filter(|&l| bounds.contains(l) && !occluded);
        let dragging = self.tree.dragged().is_some();
        match (inside, local) {
            (Some(l), _) => self.tree.pointer_move(l),
            (None, Some(l)) if dragging => self.tree.pointer_move(l),
            _ => self.tree.pointer_leave(),
        }
        if inside.is_some() || dragging {
            self.urgent = true;
        }
        let catch = dragging || inside.is_some_and(|l| self.tree.interactive_at(l));
        if catch != self.catches_clicks {
            self.catches_clicks = catch;
            if let Err(e) = desktop.set_click_through(self.window.as_ref(), !catch) {
                log::warn!("tree click-through: {e:#}");
            }
        }
        let mut changed = false;
        if let Some(l) = local {
            if pointer.down && !self.was_down && catch {
                changed |= self.tree.pointer_down(l);
            }
            if !pointer.down && self.was_down {
                changed |= self.tree.pointer_up(l);
            }
        }
        self.was_down = pointer.down;
        changed
    }

    pub fn render(&mut self, renderer: &Renderer) {
        if !self.visible || (!self.urgent && self.last_render.elapsed() < IDLE_FRAME) {
            return;
        }
        self.urgent = false;
        self.last_render = Instant::now();
        self.list.build_tree(&self.tree);
        renderer.render(&mut self.target, &self.list);
    }
}
