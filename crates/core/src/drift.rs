//! Snow piles ("drifts") lying on window tops and screen bottoms.
//!
//! A drift is a height map of fixed-width columns attached to a surface. It
//! follows its window around, loses snow when the window is shaken or closed,
//! relaxes towards the angle of repose and slowly melts.

use glam::Vec2;

use crate::geom::subtract_interval;
use crate::scene::{Scene, SurfaceId};
use crate::settings::Settings;

/// Width of one height-map column, in points.
pub const COLUMN_WIDTH: f32 = 3.0;
/// Fraction of a flake's disc area that turns into packed snow.
pub const PACKING: f32 = 0.45;
/// Snow volume (point²) carried by one falling chunk.
pub const CHUNK_VOLUME: f32 = 9.0;

/// Largest height difference between neighbouring columns at rest.
const REPOSE_STEP: f32 = 1.6;
/// How fast slopes relax, per second.
const RELAX_RATE: f32 = 10.0;
/// Height an edge column holds before snow starts sliding off.
const EDGE_HOLD: f32 = 5.0;
/// Speed of a window (points per second) above which snow is shaken off.
const SHAKE_SPEED: f32 = 2200.0;
/// Constant melt, points per second.
const MELT_BASE: f32 = 0.02;
/// Melt proportional to height, per second.
const MELT_RATE: f32 = 0.004;

/// Snow that leaves a drift and becomes falling particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spill {
    pub pos: Vec2,
    pub vel: Vec2,
    /// Snow volume in point².
    pub volume: f32,
}

/// Snow resting on one surface.
#[derive(Clone, Debug)]
pub struct Drift {
    id: SurfaceId,
    /// Left edge of the surface.
    x: f32,
    /// Base line the snow rests on.
    y: f32,
    heights: Vec<f32>,
    exposed: Vec<bool>,
    max_height: f32,
    /// Number of obstacles in front of this surface.
    front: usize,
    seen: bool,
    /// Volume waiting to slide off the left and right edges.
    edge_volume: [f32; 2],
}

impl Drift {
    fn new(id: SurfaceId, x: f32, y: f32, width: f32, max_height: f32) -> Self {
        let n = column_count(width);
        Self {
            id,
            x,
            y,
            heights: vec![0.0; n],
            exposed: vec![false; n],
            max_height,
            front: 0,
            seen: true,
            edge_volume: [0.0; 2],
        }
    }

    pub fn id(&self) -> SurfaceId {
        self.id
    }

    pub fn x(&self) -> f32 {
        self.x
    }

    pub fn y(&self) -> f32 {
        self.y
    }

    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    pub fn exposed(&self) -> &[bool] {
        &self.exposed
    }

    pub fn width(&self) -> f32 {
        self.heights.len() as f32 * COLUMN_WIDTH
    }

    pub fn right(&self) -> f32 {
        self.x + self.width()
    }

    pub fn column_at(&self, x: f32) -> Option<usize> {
        if x < self.x {
            return None;
        }
        let i = ((x - self.x) / COLUMN_WIDTH) as usize;
        (i < self.heights.len()).then_some(i)
    }

    pub fn column_center(&self, i: usize) -> f32 {
        self.x + (i as f32 + 0.5) * COLUMN_WIDTH
    }

    /// Y coordinate of the snow surface in column `i`.
    pub fn surface_y(&self, i: usize) -> f32 {
        self.y - self.heights[i]
    }

    /// Total snow volume, in point².
    pub fn volume(&self) -> f32 {
        self.heights.iter().sum::<f32>() * COLUMN_WIDTH
    }

    /// Adds `volume` point² of snow around column `i`.
    pub fn deposit(&mut self, i: usize, volume: f32) {
        let h = volume / COLUMN_WIDTH;
        let n = self.heights.len();
        if n == 0 {
            return;
        }
        let i = i.min(n - 1);
        let (mut center, mut side) = (0.6, 0.2);
        if i == 0 || i + 1 == n {
            center += 0.2;
            side = 0.2;
        }
        self.heights[i] += h * center;
        if i > 0 {
            self.heights[i - 1] += h * side;
        }
        if i + 1 < n {
            self.heights[i + 1] += h * side;
        }
    }

    /// Takes snow out of every column and turns it into spills.
    fn spill_all(&mut self, vel: Vec2, spills: &mut Vec<Spill>) {
        let mut acc = SpillAccumulator::default();
        for i in 0..self.heights.len() {
            let h = std::mem::take(&mut self.heights[i]);
            if self.exposed[i] && h > 0.0 {
                let pos = Vec2::new(self.column_center(i), self.y - h * 0.5);
                acc.add(pos, vel, h * COLUMN_WIDTH, spills);
            }
        }
        acc.flush(spills);
        self.edge_volume = [0.0; 2];
    }

    /// Moves the drift to a new surface rectangle. Snow that no longer fits is
    /// spilled.
    fn reshape(&mut self, x: f32, y: f32, width: f32, spills: &mut Vec<Spill>) {
        let n = column_count(width);
        let old_n = self.heights.len();
        if n == old_n {
            self.x = x;
            self.y = y;
            return;
        }
        let new_right = x + n as f32 * COLUMN_WIDTH;
        let left_anchored = (x - self.x).abs() < 0.5;
        let right_anchored = (new_right - self.right()).abs() < 0.5;

        let mut lost = SpillAccumulator::default();
        let new_heights: Vec<f32> = if left_anchored || right_anchored {
            // The window was resized from one edge: snow stays where it is.
            let offset = if left_anchored {
                0
            } else {
                old_n as isize - n as isize
            };
            for (i, &h) in self.heights.iter().enumerate() {
                let j = i as isize - offset;
                if (j < 0 || j >= n as isize) && h > 0.0 && self.exposed[i] {
                    let pos = Vec2::new(self.column_center(i), self.y - h * 0.5);
                    lost.add(pos, Vec2::ZERO, h * COLUMN_WIDTH, spills);
                }
            }
            (0..n)
                .map(|j| {
                    let i = j as isize + offset;
                    if (0..old_n as isize).contains(&i) {
                        self.heights[i as usize]
                    } else {
                        0.0
                    }
                })
                .collect()
        } else {
            // Moved and resized at once (e.g. maximised): stretch the pile.
            (0..n)
                .map(|j| {
                    let t = (j as f32 + 0.5) / n as f32 * old_n as f32;
                    self.heights[(t as usize).min(old_n.saturating_sub(1))]
                })
                .collect()
        };
        lost.flush(spills);
        self.heights = new_heights;
        self.exposed = vec![false; n];
        self.x = x;
        self.y = y;
        self.edge_volume = [0.0; 2];
    }

    fn update_exposure(&mut self, scene: &Scene) {
        let probe_y = self.y - 0.5;
        let mut open = vec![(self.x, self.right())];
        for o in &scene.obstacles[..self.front.min(scene.obstacles.len())] {
            if o.rect.y <= probe_y && probe_y < o.rect.bottom() {
                subtract_interval(&mut open, o.rect.x, o.rect.right());
            }
        }
        let mut k = 0;
        for i in 0..self.heights.len() {
            let cx = self.column_center(i);
            while k < open.len() && open[k].1 <= cx {
                k += 1;
            }
            let in_open = k < open.len() && open[k].0 <= cx;
            self.exposed[i] = in_open && scene.on_any_screen(Vec2::new(cx, probe_y));
        }
    }

    fn relax(&mut self, dt: f32, melt: bool, rng: &mut fastrand::Rng, spills: &mut Vec<Spill>) {
        let n = self.heights.len();
        if n == 0 {
            return;
        }
        if melt {
            for h in &mut self.heights {
                if *h > 0.0 {
                    *h = (*h - (MELT_BASE + MELT_RATE * *h) * dt).max(0.0);
                }
            }
        }
        let k = (RELAX_RATE * dt).min(1.0) * 0.5;
        for _ in 0..2 {
            for i in 0..n - 1 {
                let diff = self.heights[i] - self.heights[i + 1];
                if diff.abs() > REPOSE_STEP {
                    let t = (diff.abs() - REPOSE_STEP) * k * diff.signum();
                    self.heights[i] -= t;
                    self.heights[i + 1] += t;
                }
            }
        }
        for h in &mut self.heights {
            *h = h.min(self.max_height);
        }
        // Snow overhanging the ends slides off.
        for side in 0..2 {
            let i = if side == 0 { 0 } else { n - 1 };
            if !self.exposed[i] {
                continue;
            }
            let h = self.heights[i];
            if h > EDGE_HOLD {
                let amount = (h - EDGE_HOLD) * (3.0 * dt).min(1.0);
                self.heights[i] -= amount;
                self.edge_volume[side] += amount * COLUMN_WIDTH;
            }
            if self.edge_volume[side] >= CHUNK_VOLUME {
                self.edge_volume[side] -= CHUNK_VOLUME;
                let dir = if side == 0 { -1.0 } else { 1.0 };
                let x = if side == 0 {
                    self.x - 1.5
                } else {
                    self.right() + 1.5
                };
                spills.push(Spill {
                    pos: Vec2::new(x, self.y - self.heights[i]),
                    vel: Vec2::new(dir * rng.f32() * 25.0, 10.0),
                    volume: CHUNK_VOLUME,
                });
            }
        }
    }
}

/// Groups consecutive small amounts of snow into chunk-sized spills.
#[derive(Default)]
struct SpillAccumulator {
    volume: f32,
    weighted_pos: Vec2,
    vel: Vec2,
}

impl SpillAccumulator {
    fn add(&mut self, pos: Vec2, vel: Vec2, volume: f32, spills: &mut Vec<Spill>) {
        self.volume += volume;
        self.weighted_pos += pos * volume;
        self.vel = vel;
        if self.volume >= CHUNK_VOLUME {
            self.flush(spills);
        }
    }

    fn flush(&mut self, spills: &mut Vec<Spill>) {
        if self.volume > 0.05 {
            spills.push(Spill {
                pos: self.weighted_pos / self.volume,
                vel: self.vel,
                volume: self.volume,
            });
        }
        *self = Self::default();
    }
}

fn column_count(width: f32) -> usize {
    (width / COLUMN_WIDTH).floor().max(0.0) as usize
}

/// All drifts in the world.
#[derive(Clone, Debug, Default)]
pub struct DriftField {
    drifts: Vec<Drift>,
}

impl DriftField {
    pub fn drifts(&self) -> &[Drift] {
        &self.drifts
    }

    pub fn drifts_mut(&mut self) -> &mut [Drift] {
        &mut self.drifts
    }

    pub fn total_volume(&self) -> f32 {
        self.drifts.iter().map(Drift::volume).sum()
    }

    /// Matches drifts to the current scene: follows moved windows, spills snow
    /// from windows that were closed, shaken or shrunk.
    pub fn sync(&mut self, scene: &Scene, settings: &Settings, dt: f32, spills: &mut Vec<Spill>) {
        for d in &mut self.drifts {
            d.seen = false;
        }
        for (k, o) in scene.obstacles.iter().enumerate() {
            if o.holds_snow && o.rect.w >= COLUMN_WIDTH * 4.0 {
                let id = SurfaceId::Window(o.id);
                self.upsert(
                    id,
                    o.rect.x,
                    o.rect.y,
                    o.rect.w,
                    settings.max_window_drift,
                    k,
                    dt,
                    spills,
                );
            }
        }
        let front = scene.obstacles.len();
        for (i, s) in scene.screens.iter().enumerate() {
            let id = SurfaceId::Ground(i as u32);
            self.upsert(
                id,
                s.x,
                s.bottom(),
                s.w,
                settings.max_ground_drift,
                front,
                dt,
                spills,
            );
        }
        for d in &mut self.drifts {
            if !d.seen {
                d.spill_all(Vec2::new(0.0, 20.0), spills);
            }
        }
        self.drifts.retain(|d| d.seen);
        for d in &mut self.drifts {
            d.update_exposure(scene);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert(
        &mut self,
        id: SurfaceId,
        x: f32,
        y: f32,
        width: f32,
        max_height: f32,
        front: usize,
        dt: f32,
        spills: &mut Vec<Spill>,
    ) {
        let Some(d) = self.drifts.iter_mut().find(|d| d.id == id) else {
            let mut d = Drift::new(id, x, y, width, max_height);
            d.front = front;
            self.drifts.push(d);
            return;
        };
        d.seen = true;
        d.front = front;
        d.max_height = max_height;
        let delta = Vec2::new(x - d.x, y - d.y);
        if delta == Vec2::ZERO && column_count(width) == d.heights.len() {
            return;
        }
        if dt > 0.0 && delta.length() / dt > SHAKE_SPEED {
            // Shaken: half of the snow flies off with the window's momentum.
            let fling = -delta / dt * 0.15;
            let mut acc = SpillAccumulator::default();
            for i in 0..d.heights.len() {
                let h = d.heights[i] * 0.5;
                if h > 0.0 && d.exposed[i] {
                    d.heights[i] -= h;
                    let pos = Vec2::new(d.column_center(i), d.y - d.heights[i] - h * 0.5);
                    acc.add(pos + delta, fling, h * COLUMN_WIDTH, spills);
                }
            }
            acc.flush(spills);
        }
        d.reshape(x, y, width, spills);
    }

    /// Ages all drifts.
    pub fn relax(&mut self, dt: f32, melt: bool, rng: &mut fastrand::Rng, spills: &mut Vec<Spill>) {
        for d in &mut self.drifts {
            d.relax(dt, melt, rng, spills);
        }
    }

    /// Finds the first exposed snow surface crossed by a particle moving from
    /// `from` to `to`. Returns `(drift index, column)`.
    pub fn find_landing(&self, from: Vec2, to: Vec2) -> Option<(usize, usize)> {
        if to.y < from.y {
            return None;
        }
        let mut best: Option<(usize, usize, f32)> = None;
        for (di, d) in self.drifts.iter().enumerate() {
            if to.y < d.y - d.max_height - 1.0 || from.y > d.y {
                continue;
            }
            let Some(col) = d.column_at(to.x) else {
                continue;
            };
            if !d.exposed[col] {
                continue;
            }
            let s = d.surface_y(col);
            if from.y <= s && to.y >= s && best.is_none_or(|b| s < b.2) {
                best = Some((di, col, s));
            }
        }
        best.map(|(di, col, _)| (di, col))
    }

    pub fn deposit(&mut self, drift: usize, col: usize, volume: f32) {
        self.drifts[drift].deposit(col, volume);
    }

    /// Brushes snow away around the cursor (the "mitten").
    pub fn brush(
        &mut self,
        cursor: Vec2,
        cursor_vel: Vec2,
        radius: f32,
        rng: &mut fastrand::Rng,
        spills: &mut Vec<Spill>,
    ) {
        for d in &mut self.drifts {
            if cursor.y < d.y - d.max_height - radius || cursor.y > d.y + radius * 0.5 {
                continue;
            }
            if cursor.x < d.x - radius || cursor.x > d.right() + radius {
                continue;
            }
            let first = ((cursor.x - radius - d.x) / COLUMN_WIDTH).floor().max(0.0) as usize;
            let last = (((cursor.x + radius - d.x) / COLUMN_WIDTH).ceil().max(0.0) as usize)
                .min(d.heights.len());
            let mut acc = SpillAccumulator::default();
            for i in first..last {
                let h = d.heights[i];
                if h < 0.05 || !d.exposed[i] {
                    continue;
                }
                let top = d.y - h;
                let dx = d.column_center(i) - cursor.x;
                let dy = if cursor.y < top {
                    top - cursor.y
                } else if cursor.y > d.y {
                    cursor.y - d.y
                } else {
                    0.0
                };
                let dist = (dx * dx + dy * dy).sqrt();
                if dist >= radius {
                    continue;
                }
                let strength = ((1.0 - dist / radius) * 1.5).min(1.0);
                let taken = h * strength;
                d.heights[i] -= taken;
                let pos = Vec2::new(d.column_center(i), top + taken * 0.5);
                let kick = Vec2::new(
                    cursor_vel.x * 0.45 + (rng.f32() - 0.5) * 30.0,
                    cursor_vel.y.min(0.0) * 0.3 - 50.0 - rng.f32() * 60.0,
                );
                acc.add(pos, kick, taken * COLUMN_WIDTH, spills);
            }
            acc.flush(spills);
        }
    }

    /// Throws all snow off every surface (the "shovel").
    pub fn clear(&mut self, rng: &mut fastrand::Rng, spills: &mut Vec<Spill>) {
        for d in &mut self.drifts {
            let vel = Vec2::new((rng.f32() - 0.5) * 40.0, -20.0);
            d.spill_all(vel, spills);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::scene::Obstacle;

    fn scene_with(obstacles: Vec<Obstacle>) -> Scene {
        Scene {
            screens: vec![Rect::new(0.0, 0.0, 1000.0, 800.0)],
            obstacles,
            ..Scene::default()
        }
    }

    fn window(id: u64, x: f32, y: f32, w: f32, h: f32) -> Obstacle {
        Obstacle {
            id,
            rect: Rect::new(x, y, w, h),
            holds_snow: true,
        }
    }

    fn field_for(scene: &Scene) -> DriftField {
        let mut f = DriftField::default();
        f.sync(scene, &Settings::default(), 1.0 / 60.0, &mut Vec::new());
        f
    }

    #[test]
    fn creates_window_and_ground_drifts() {
        let scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let f = field_for(&scene);
        assert_eq!(f.drifts().len(), 2);
        let w = &f.drifts()[0];
        assert_eq!(w.id(), SurfaceId::Window(1));
        assert_eq!(w.heights().len(), 100);
        assert!(w.exposed().iter().all(|&e| e));
    }

    #[test]
    fn front_window_hides_part_of_back_window() {
        let scene = scene_with(vec![
            window(2, 0.0, 100.0, 150.0, 300.0),
            window(1, 100.0, 200.0, 300.0, 200.0),
        ]);
        let f = field_for(&scene);
        let back = f
            .drifts()
            .iter()
            .find(|d| d.id() == SurfaceId::Window(1))
            .unwrap();
        // Columns under the front window (x < 150) are hidden.
        assert!(!back.exposed()[0]);
        assert!(!back.exposed()[15]);
        assert!(back.exposed()[20]);
        // The ground under both windows is hidden too.
        let ground = f
            .drifts()
            .iter()
            .find(|d| d.id() == SurfaceId::Ground(0))
            .unwrap();
        assert!(ground.exposed()[0]);
        assert!(ground.exposed()[100]);
    }

    #[test]
    fn landing_finds_highest_surface() {
        let scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let f = field_for(&scene);
        let hit = f.find_landing(Vec2::new(150.0, 190.0), Vec2::new(150.0, 205.0));
        assert_eq!(hit.map(|h| h.0), Some(0));
        // Outside the window it falls to the ground.
        assert_eq!(
            f.find_landing(Vec2::new(50.0, 190.0), Vec2::new(50.0, 205.0)),
            None
        );
        let ground = f.find_landing(Vec2::new(50.0, 795.0), Vec2::new(50.0, 805.0));
        assert_eq!(ground.map(|h| h.0), Some(1));
    }

    #[test]
    fn snow_follows_moved_window() {
        let mut scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let mut f = field_for(&scene);
        f.deposit(0, 10, 30.0);
        let before = f.drifts()[0].volume();
        scene.obstacles[0].rect.x += 5.0;
        scene.obstacles[0].rect.y += 3.0;
        let mut spills = Vec::new();
        f.sync(&scene, &Settings::default(), 1.0 / 60.0, &mut spills);
        assert!(spills.is_empty());
        assert_eq!(f.drifts()[0].x(), 105.0);
        assert!((f.drifts()[0].volume() - before).abs() < 1e-3);
    }

    #[test]
    fn closing_window_spills_its_snow() {
        let mut scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let mut f = field_for(&scene);
        for i in 0..100 {
            f.deposit(0, i, 6.0);
        }
        let volume = f.drifts()[0].volume();
        scene.obstacles.clear();
        let mut spills = Vec::new();
        f.sync(&scene, &Settings::default(), 1.0 / 60.0, &mut spills);
        assert_eq!(f.drifts().len(), 1);
        let spilled: f32 = spills.iter().map(|s| s.volume).sum();
        assert!((spilled - volume).abs() < 0.5, "{spilled} vs {volume}");
    }

    #[test]
    fn shaking_window_throws_snow_off() {
        let mut scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let mut f = field_for(&scene);
        for i in 0..100 {
            f.deposit(0, i, 6.0);
        }
        let before = f.drifts()[0].volume();
        scene.obstacles[0].rect.x += 80.0;
        let mut spills = Vec::new();
        f.sync(&scene, &Settings::default(), 1.0 / 60.0, &mut spills);
        assert!(!spills.is_empty());
        assert!(f.drifts()[0].volume() < before * 0.6);
    }

    #[test]
    fn relax_flattens_spikes() {
        let scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let mut f = field_for(&scene);
        f.drifts_mut()[0].heights[50] = 20.0;
        let mut rng = fastrand::Rng::with_seed(1);
        for _ in 0..240 {
            f.relax(1.0 / 60.0, false, &mut rng, &mut Vec::new());
        }
        let h = f.drifts()[0].heights();
        for w in h.windows(2) {
            assert!((w[0] - w[1]).abs() <= REPOSE_STEP + 0.2);
        }
        assert!((f.drifts()[0].volume() - 20.0 * COLUMN_WIDTH).abs() < 0.5);
    }

    #[test]
    fn brush_removes_snow_near_cursor() {
        let scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let mut f = field_for(&scene);
        for i in 0..100 {
            f.deposit(0, i, 15.0);
        }
        let before = f.drifts()[0].volume();
        let mut spills = Vec::new();
        let mut rng = fastrand::Rng::with_seed(1);
        f.brush(
            Vec2::new(250.0, 197.0),
            Vec2::new(300.0, 0.0),
            14.0,
            &mut rng,
            &mut spills,
        );
        let after = f.drifts()[0].volume();
        let spilled: f32 = spills.iter().map(|s| s.volume).sum();
        assert!(after < before);
        assert!((before - after - spilled).abs() < 0.1);
    }

    #[test]
    fn resize_from_right_edge_keeps_left_snow() {
        let mut scene = scene_with(vec![window(1, 100.0, 200.0, 300.0, 200.0)]);
        let mut f = field_for(&scene);
        f.drifts_mut()[0].heights[0] = 4.0;
        f.drifts_mut()[0].heights[99] = 4.0;
        scene.obstacles[0].rect.w = 150.0;
        let mut spills = Vec::new();
        f.sync(&scene, &Settings::default(), 1.0 / 60.0, &mut spills);
        let d = &f.drifts()[0];
        assert_eq!(d.heights().len(), 50);
        assert_eq!(d.heights()[0], 4.0);
        assert_eq!(spills.len(), 1);
    }
}
