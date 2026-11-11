//! Falling particles: snowflakes and clumps of snow knocked off piles.

use glam::Vec2;

/// What kind of particle this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlakeKind {
    /// A snowflake drifting down at terminal velocity.
    Snow,
    /// A clump of packed snow falling under gravity.
    Chunk,
}

#[derive(Clone, Copy, Debug)]
pub struct Flake {
    pub pos: Vec2,
    pub vel: Vec2,
    pub radius: f32,
    /// 0 is far away (small, faint, never settles), 1 is close to the glass.
    pub depth: f32,
    /// Terminal falling speed, points per second.
    pub fall: f32,
    /// Sway frequency, radians per second.
    pub sway: f32,
    pub phase: f32,
    pub kind: FlakeKind,
    pub age: f32,
}

impl Flake {
    /// Whether the particle can settle on surfaces.
    pub fn settles(&self) -> bool {
        self.kind == FlakeKind::Chunk || self.depth >= 0.4
    }

    /// Opacity in `[0, 1]`.
    pub fn alpha(&self) -> f32 {
        let base = match self.kind {
            FlakeKind::Snow => 0.45 + 0.55 * self.depth,
            FlakeKind::Chunk => 1.0,
        };
        base * (self.age * 3.0).min(1.0)
    }

    /// Volume of snow this particle adds to a pile, in point².
    pub fn volume(&self) -> f32 {
        std::f32::consts::PI * self.radius * self.radius * crate::drift::PACKING
    }
}
