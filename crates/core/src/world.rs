//! The simulation state and its time step.

use glam::Vec2;

use crate::drift::{CHUNK_VOLUME, Drift, DriftField, PACKING, Spill};
use crate::flake::{Flake, FlakeKind};
use crate::scene::Scene;
use crate::settings::Settings;
use crate::snowball::{Snowball, Snowballs};

/// Most snowflakes alive at once at full quality.
const MAX_SNOW: usize = 4000;
/// Most chunks alive at once.
const MAX_CHUNKS: usize = 1500;
/// Gravity for chunks, points per second².
const GRAVITY: f32 = 900.0;
/// Wind speed at strength 1, points per second.
const WIND_SPEED: Vec2 = Vec2::new(150.0, 70.0);
/// Mitten radius in points.
const MITTEN_RADIUS: f32 = 14.0;
/// Cursor speed below which the mitten does nothing.
const MITTEN_MIN_SPEED: f32 = 40.0;

pub struct World {
    settings: Settings,
    flakes: Vec<Flake>,
    drifts: DriftField,
    rng: fastrand::Rng,
    time: f32,
    spawn_debt: f32,
    last_cursor: Option<Vec2>,
    cursor_vel: Vec2,
    spills: Vec<Spill>,
    balls: Snowballs,
}

impl World {
    pub fn new(settings: Settings, seed: u64) -> Self {
        Self {
            settings,
            flakes: Vec::with_capacity(MAX_SNOW),
            drifts: DriftField::default(),
            rng: fastrand::Rng::with_seed(seed),
            time: 0.0,
            spawn_debt: 0.0,
            last_cursor: None,
            cursor_vel: Vec2::ZERO,
            spills: Vec::new(),
            balls: Snowballs::default(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    pub fn flakes(&self) -> &[Flake] {
        &self.flakes
    }

    pub fn snowballs(&self) -> &[Snowball] {
        self.balls.balls()
    }

    /// Stacks of snowball indices (bottom first) that form snowmen.
    pub fn snowmen(&self) -> Vec<Vec<usize>> {
        self.balls.snowmen()
    }

    /// Whether the overlay should take clicks at `p` while the grab modifier
    /// is held.
    pub fn wants_pointer(&self, p: Vec2) -> bool {
        self.settings.accumulate && self.balls.wants_pointer(p, &self.drifts)
    }

    /// A snowball is being dragged.
    pub fn is_holding(&self) -> bool {
        self.balls.is_holding()
    }

    pub fn drifts(&self) -> &[Drift] {
        self.drifts.drifts()
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    /// Whether anything is visible or moving.
    pub fn is_idle(&self) -> bool {
        self.flakes.is_empty() && self.drifts.total_volume() < 0.01
    }

    /// Current wind including gusts, in points per second.
    pub fn wind(&self) -> Vec2 {
        let t = self.time;
        let g = self.settings.intensity.gustiness();
        let gust = 1.0 + g * (0.6 * (0.31 * t).sin() + 0.4 * (0.83 * t + 1.7).sin());
        let swirl = Vec2::new(0.0, g * 12.0 * (0.53 * t + 0.4).sin());
        self.settings.wind * WIND_SPEED * gust + swirl
    }

    /// Throws every pile off its surface.
    pub fn shovel(&mut self) {
        self.drifts.clear(&mut self.rng, &mut self.spills);
        self.balls.clear(&mut self.spills);
        self.flush_spills();
    }

    /// Advances the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32, scene: &Scene) {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }
        self.time += dt;
        self.track_cursor(dt, scene.cursor);

        if self.settings.accumulate {
            self.drifts
                .sync(scene, &self.settings, dt, &mut self.spills);
        } else {
            self.drifts.clear(&mut self.rng, &mut self.spills);
            self.balls.clear(&mut self.spills);
        }
        if self.settings.accumulate {
            self.balls
                .update(dt, scene, self.cursor_vel, &mut self.drifts);
        }
        if !self.settings.paused {
            self.spawn(dt, scene);
        }
        self.move_flakes(dt, scene);
        if self.settings.accumulate {
            self.drifts
                .relax(dt, self.settings.melt, &mut self.rng, &mut self.spills);
        }
        if let Some(cursor) = scene.cursor
            && self.settings.mitten
            && !scene.grab
            && self.cursor_vel.length() > MITTEN_MIN_SPEED
        {
            self.drifts.brush(
                cursor,
                self.cursor_vel,
                MITTEN_RADIUS,
                &mut self.rng,
                &mut self.spills,
            );
        }
        self.flush_spills();
    }

    fn track_cursor(&mut self, dt: f32, cursor: Option<Vec2>) {
        match (cursor, self.last_cursor) {
            (Some(c), Some(l)) => {
                let v = (c - l) / dt;
                self.cursor_vel = self.cursor_vel.lerp(v, 0.6);
            }
            _ => self.cursor_vel = Vec2::ZERO,
        }
        self.last_cursor = cursor;
    }

    fn spawn(&mut self, dt: f32, scene: &Scene) {
        let total_width: f32 = scene.screens.iter().map(|s| s.w).sum();
        if total_width <= 0.0 {
            return;
        }
        let intensity = self.settings.intensity;
        let quality = scene.quality.clamp(0.1, 1.0);
        let cap = (MAX_SNOW as f32 * quality) as usize;
        let wind = self.wind();
        let (fall_min, fall_max) = intensity.fall_speed();
        let (r_min, r_max) = intensity.radius();
        // Flakes that blow sideways need to start upwind of the screen.
        let mean_fall = (fall_min + fall_max) * 0.5;

        self.spawn_debt += intensity.spawn_rate() * total_width / 1000.0 * quality * dt;
        let snow_count = self
            .flakes
            .iter()
            .filter(|f| f.kind == FlakeKind::Snow)
            .count();
        let mut budget = cap.saturating_sub(snow_count);
        while self.spawn_debt >= 1.0 {
            self.spawn_debt -= 1.0;
            if budget == 0 {
                continue;
            }
            budget -= 1;
            let pick = self.rng.f32() * total_width;
            let mut acc = 0.0;
            let screen = scene
                .screens
                .iter()
                .find(|s| {
                    acc += s.w;
                    pick < acc
                })
                .unwrap_or(&scene.screens[0]);
            let drift_dist = (wind.x * 0.7 * screen.h / mean_fall).clamp(-screen.w, screen.w);
            let x0 = screen.x - drift_dist.max(0.0);
            let span = screen.w + drift_dist.abs();
            let depth = self.rng.f32().powf(0.7);
            let size = 0.55 + 0.45 * depth;
            self.flakes.push(Flake {
                pos: Vec2::new(
                    x0 + self.rng.f32() * span,
                    screen.y - 6.0 - self.rng.f32() * 30.0,
                ),
                vel: Vec2::ZERO,
                radius: lerp(r_min, r_max, self.rng.f32()) * size,
                depth,
                fall: lerp(fall_min, fall_max, self.rng.f32()) * (0.55 + 0.45 * depth),
                sway: lerp(0.6, 1.8, self.rng.f32()),
                phase: self.rng.f32() * std::f32::consts::TAU,
                kind: FlakeKind::Snow,
                age: 0.0,
            });
        }
        // Never bank more than a frame's worth while capped.
        self.spawn_debt = self.spawn_debt.min(1.0);
    }

    fn move_flakes(&mut self, dt: f32, scene: &Scene) {
        let wind = self.wind();
        let bounds = scene.bounds().expand(200.0);
        let time = self.time;
        let accumulate = self.settings.accumulate;
        let cursor = scene.cursor.filter(|_| self.settings.mitten);
        let cursor_vel = self.cursor_vel;
        let follow = (2.5 * dt).min(1.0);
        let drifts = &mut self.drifts;

        self.flakes.retain_mut(|f| {
            let prev = f.pos;
            f.age += dt;
            match f.kind {
                FlakeKind::Snow => {
                    let near = 0.4 + 0.6 * f.depth;
                    let target = Vec2::new(
                        wind.x * near + (time * f.sway + f.phase).sin() * 14.0 * near,
                        f.fall + wind.y * near,
                    );
                    f.vel += (target - f.vel) * follow;
                }
                FlakeKind::Chunk => {
                    f.vel.y += GRAVITY * dt;
                    f.vel.x += (wind.x * 0.3 - f.vel.x) * (0.8 * dt).min(1.0);
                }
            }
            if let Some(c) = cursor {
                let d = f.pos - c;
                let dist = d.length();
                if dist < 40.0 && cursor_vel.length() > MITTEN_MIN_SPEED {
                    f.vel += cursor_vel * 0.05 * (1.0 - dist / 40.0);
                }
            }
            f.pos += f.vel * dt;

            if accumulate
                && f.settles()
                && let Some((di, col)) = drifts.find_landing(prev, f.pos)
            {
                drifts.deposit(di, col, f.volume());
                return false;
            }
            f.pos.y < bounds.bottom() && f.pos.x > bounds.x && f.pos.x < bounds.right()
        });
    }

    fn flush_spills(&mut self) {
        let chunks = self
            .flakes
            .iter()
            .filter(|f| f.kind == FlakeKind::Chunk)
            .count();
        let mut budget = MAX_CHUNKS.saturating_sub(chunks);
        for s in self.spills.drain(..) {
            let n = ((s.volume / CHUNK_VOLUME).ceil() as usize).clamp(1, 4);
            let volume = s.volume / n as f32;
            let radius = (volume / (std::f32::consts::PI * PACKING))
                .sqrt()
                .clamp(0.8, 3.5);
            for _ in 0..n {
                if budget == 0 {
                    return;
                }
                budget -= 1;
                let jitter = Vec2::new(self.rng.f32() - 0.5, self.rng.f32() - 0.5);
                self.flakes.push(Flake {
                    pos: s.pos + jitter * 3.0,
                    vel: s.vel + jitter * 30.0,
                    radius,
                    depth: 1.0,
                    fall: 0.0,
                    sway: 0.0,
                    phase: 0.0,
                    kind: FlakeKind::Chunk,
                    age: 1.0,
                });
            }
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::scene::{Obstacle, SurfaceId};

    fn scene() -> Scene {
        Scene {
            screens: vec![Rect::new(0.0, 0.0, 1200.0, 800.0)],
            obstacles: vec![Obstacle {
                id: 7,
                rect: Rect::new(200.0, 300.0, 600.0, 300.0),
                holds_snow: true,
                corner_radius: 0.0,
            }],
            ..Scene::default()
        }
    }

    fn run(world: &mut World, scene: &Scene, seconds: f32) {
        let steps = (seconds * 60.0) as usize;
        for _ in 0..steps {
            world.step(1.0 / 60.0, scene);
        }
    }

    fn window_volume(world: &World) -> f32 {
        world
            .drifts()
            .iter()
            .find(|d| d.id() == SurfaceId::Window(7))
            .map(Drift::volume)
            .unwrap_or(0.0)
    }

    #[test]
    fn snow_falls_and_piles_up_on_window() {
        let mut world = World::new(Settings::default(), 42);
        let scene = scene();
        run(&mut world, &scene, 30.0);
        assert!(!world.flakes().is_empty());
        assert!(window_volume(&world) > 50.0, "{}", window_volume(&world));
    }

    #[test]
    fn paused_world_stops_spawning() {
        let settings = Settings {
            paused: true,
            ..Settings::default()
        };
        let mut world = World::new(settings, 1);
        run(&mut world, &scene(), 5.0);
        assert!(world.flakes().is_empty());
    }

    #[test]
    fn shovel_empties_piles() {
        let mut world = World::new(Settings::default(), 3);
        let scene = scene();
        run(&mut world, &scene, 30.0);
        assert!(window_volume(&world) > 0.0);
        world.shovel();
        assert_eq!(window_volume(&world), 0.0);
        assert!(world.flakes().iter().any(|f| f.kind == FlakeKind::Chunk));
    }

    #[test]
    fn flake_count_is_capped() {
        let settings = Settings {
            intensity: crate::settings::Intensity::Blizzard,
            ..Settings::default()
        };
        let mut world = World::new(settings, 5);
        let mut scene = scene();
        scene.screens = vec![Rect::new(0.0, 0.0, 8000.0, 3000.0)];
        scene.quality = 0.25;
        run(&mut world, &scene, 20.0);
        let snow = world
            .flakes()
            .iter()
            .filter(|f| f.kind == FlakeKind::Snow)
            .count();
        assert!(snow <= MAX_SNOW / 4, "{snow}");
    }
}
