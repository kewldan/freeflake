//! A snapshot of the desktop that platform layers feed into the simulation.

use glam::Vec2;

use crate::geom::Rect;

/// Identifies a surface snow can pile up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceId {
    /// The top edge of an on-screen window (or the Dock / taskbar).
    Window(u64),
    /// The bottom edge of a screen.
    Ground(u32),
}

/// Something on screen that hides what is behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    /// Stable id of the window for as long as it exists.
    pub id: u64,
    pub rect: Rect,
    /// Whether snow settles on the top edge.
    pub holds_snow: bool,
}

/// Everything the simulation needs to know about the desktop for one step.
#[derive(Clone, Debug)]
pub struct Scene {
    /// Full frames of all screens.
    pub screens: Vec<Rect>,
    /// On-screen windows ordered front to back.
    pub obstacles: Vec<Obstacle>,
    /// Global cursor position, if known.
    pub cursor: Option<Vec2>,
    /// The "grab" modifier (Option / Alt) is held: the overlay takes clicks.
    pub grab: bool,
    /// Primary mouse button is down.
    pub mouse_down: bool,
    /// Quality factor in `(0, 1]`; lowered on battery saver or heat.
    pub quality: f32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            screens: Vec::new(),
            obstacles: Vec::new(),
            cursor: None,
            grab: false,
            mouse_down: false,
            quality: 1.0,
        }
    }
}

impl Scene {
    /// Bounding box of all screens.
    pub fn bounds(&self) -> Rect {
        self.screens
            .iter()
            .copied()
            .reduce(|a, b| a.union(&b))
            .unwrap_or_default()
    }

    pub fn on_any_screen(&self, p: Vec2) -> bool {
        self.screens.iter().any(|s| s.contains(p))
    }

    /// Whether `p` is hidden by one of the first `front` obstacles.
    pub fn occluded(&self, p: Vec2, front: usize) -> bool {
        self.obstacles[..front.min(self.obstacles.len())]
            .iter()
            .any(|o| o.rect.contains(p))
    }
}
