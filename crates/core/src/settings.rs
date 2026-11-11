//! User-tunable settings, persisted by the app as TOML.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// How hard it snows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intensity {
    /// A few large, slow flakes.
    Light,
    /// Calm, even snowfall.
    #[default]
    Steady,
    /// Dense, fast and gusty.
    Blizzard,
}

impl Intensity {
    pub const ALL: [Intensity; 3] = [Intensity::Light, Intensity::Steady, Intensity::Blizzard];

    /// New flakes per second per 1000 points of screen width.
    pub fn spawn_rate(self) -> f32 {
        match self {
            Intensity::Light => 14.0,
            Intensity::Steady => 40.0,
            Intensity::Blizzard => 150.0,
        }
    }

    /// Base falling speed range in points per second.
    pub fn fall_speed(self) -> (f32, f32) {
        match self {
            Intensity::Light => (32.0, 60.0),
            Intensity::Steady => (45.0, 85.0),
            Intensity::Blizzard => (90.0, 170.0),
        }
    }

    /// Flake radius range in points.
    pub fn radius(self) -> (f32, f32) {
        match self {
            Intensity::Light => (1.6, 3.6),
            Intensity::Steady => (1.2, 3.2),
            Intensity::Blizzard => (1.0, 2.8),
        }
    }

    /// Relative amplitude of wind gusts.
    pub fn gustiness(self) -> f32 {
        match self {
            Intensity::Light => 0.15,
            Intensity::Steady => 0.3,
            Intensity::Blizzard => 0.8,
        }
    }
}

/// All persisted settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub intensity: Intensity,
    /// Wind direction and strength, each component in `[-1, 1]`.
    /// Positive `x` blows to the right, positive `y` blows downwards.
    pub wind: Vec2,
    pub paused: bool,
    /// Let snow pile up on windows, the Dock/taskbar and the screen bottom.
    pub accumulate: bool,
    /// Brush snow off with the cursor.
    pub mitten: bool,
    /// Slowly melt piles so they never cover everything.
    pub melt: bool,
    /// Maximum pile height on a window, in points.
    pub max_window_drift: f32,
    /// Maximum pile height on the screen bottom, in points.
    pub max_ground_drift: f32,
    /// Show the advent tree on the desktop.
    pub show_tree: bool,
    /// Target frame rate.
    pub fps: u32,
    /// Frame rate on battery saver or when the machine is hot.
    pub low_power_fps: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            intensity: Intensity::default(),
            wind: Vec2::new(0.15, 0.0),
            paused: false,
            accumulate: true,
            mitten: true,
            melt: true,
            max_window_drift: 28.0,
            max_ground_drift: 60.0,
            show_tree: true,
            fps: 60,
            low_power_fps: 30,
        }
    }
}

impl Settings {
    /// Clamps values loaded from a hand-edited file into sane ranges.
    pub fn sanitized(mut self) -> Self {
        self.wind = self.wind.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
        self.max_window_drift = self.max_window_drift.clamp(0.0, 120.0);
        self.max_ground_drift = self.max_ground_drift.clamp(0.0, 300.0);
        self.fps = self.fps.clamp(15, 240);
        self.low_power_fps = self.low_power_fps.clamp(10, self.fps);
        self
    }
}
