//! Snowballs: rolled from piles while the grab modifier is held, thrown,
//! stacked into snowmen.

use glam::Vec2;

use crate::drift::{DriftField, PACKING, Spill};
use crate::scene::{Scene, SurfaceId};

/// Most snowballs alive at once.
pub const MAX_BALLS: usize = 40;
const START_RADIUS: f32 = 4.0;
const MAX_RADIUS: f32 = 44.0;
const GRAVITY: f32 = 1500.0;
/// Fraction of the radius a ball sinks into what it rests on.
const SINK: f32 = 0.12;
/// Snow volume a rolling ball can pick up per second, point².
const ROLL_RATE: f32 = 900.0;
/// Fastest throw, points per second.
const MAX_THROW: f32 = 2500.0;
/// Upper ball may be offset from the one below by this fraction of its radius.
const BALANCE: f32 = 0.55;

/// What a resting ball sits on, with its horizontal offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Support {
    Surface { id: SurfaceId, dx: f32 },
    Ball { id: u32, dx: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BallState {
    /// Dragged by the cursor.
    Held,
    Flying,
    Resting(Support),
}

#[derive(Clone, Copy, Debug)]
pub struct Snowball {
    pub id: u32,
    pub pos: Vec2,
    pub vel: Vec2,
    pub radius: f32,
    pub state: BallState,
}

impl Snowball {
    fn volume(&self) -> f32 {
        std::f32::consts::PI * self.radius * self.radius * PACKING
    }

    fn contains(&self, p: Vec2, slack: f32) -> bool {
        self.pos.distance(p) <= self.radius + slack
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snowballs {
    balls: Vec<Snowball>,
    next_id: u32,
    held: Option<u32>,
    grab_offset: Vec2,
    was_down: bool,
}

impl Snowballs {
    pub fn balls(&self) -> &[Snowball] {
        &self.balls
    }

    pub fn is_holding(&self) -> bool {
        self.held.is_some()
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.balls.iter().position(|b| b.id == id)
    }

    /// The ball under `p`, topmost first.
    fn ball_at(&self, p: Vec2) -> Option<usize> {
        self.balls.iter().rposition(|b| b.contains(p, 3.0))
    }

    /// Whether the overlay should catch clicks at `p`.
    pub fn wants_pointer(&self, p: Vec2, drifts: &DriftField) -> bool {
        self.held.is_some()
            || self.ball_at(p).is_some()
            || (self.balls.len() < MAX_BALLS && drifts.hit_pile(p, 6.0))
    }

    pub fn update(&mut self, dt: f32, scene: &Scene, cursor_vel: Vec2, drifts: &mut DriftField) {
        self.handle_pointer(dt, scene, cursor_vel, drifts);
        self.fly(dt, scene, drifts);
        self.settle(drifts);
    }

    fn handle_pointer(
        &mut self,
        dt: f32,
        scene: &Scene,
        cursor_vel: Vec2,
        drifts: &mut DriftField,
    ) {
        let down = scene.mouse_down;
        let pressed = down && !self.was_down && scene.grab;
        self.was_down = down;
        let Some(cursor) = scene.cursor else {
            self.release(Vec2::ZERO);
            return;
        };
        if pressed && self.held.is_none() {
            if let Some(i) = self.ball_at(cursor) {
                let b = &mut self.balls[i];
                b.state = BallState::Held;
                self.grab_offset = b.pos - cursor;
                self.held = Some(b.id);
            } else if self.balls.len() < MAX_BALLS && drifts.hit_pile(cursor, 6.0) {
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1);
                let mut ball = Snowball {
                    id,
                    pos: cursor,
                    vel: Vec2::ZERO,
                    radius: START_RADIUS,
                    state: BallState::Held,
                };
                let wanted = ball.volume();
                let got = drifts.take_snow(cursor, 8.0, wanted);
                ball.radius = radius_for(got.max(wanted * 0.3));
                self.balls.push(ball);
                self.grab_offset = Vec2::ZERO;
                self.held = Some(id);
            }
        }
        let Some(i) = self.held.and_then(|id| self.index(id)) else {
            self.held = None;
            return;
        };
        if !down {
            self.release(cursor_vel);
            return;
        }
        let b = &mut self.balls[i];
        let target = cursor + self.grab_offset;
        if dt > 0.0 {
            b.vel = (target - b.pos) / dt;
        }
        b.pos = target;
        // Rolling over a pile picks up snow.
        if b.vel.length() > 20.0 && b.radius < MAX_RADIUS {
            let got = drifts.take_snow(b.pos, b.radius + 1.5, ROLL_RATE * dt);
            if got > 0.0 {
                let v = b.volume() + got;
                b.radius = radius_for(v).min(MAX_RADIUS);
            }
        }
    }

    fn release(&mut self, vel: Vec2) {
        if let Some(i) = self.held.take().and_then(|id| self.index(id)) {
            let b = &mut self.balls[i];
            b.state = BallState::Flying;
            b.vel = vel.clamp_length_max(MAX_THROW);
        }
    }

    fn fly(&mut self, dt: f32, scene: &Scene, drifts: &DriftField) {
        let bottom = scene.bounds().bottom();
        for i in 0..self.balls.len() {
            if self.balls[i].state != BallState::Flying {
                continue;
            }
            let mut b = self.balls[i];
            b.vel.y += GRAVITY * dt;
            b.vel.x *= 1.0 - (0.4 * dt).min(1.0);
            let next = b.pos + b.vel * dt;
            let mut state = BallState::Flying;
            // Land on another ball.
            if b.vel.y > 0.0 {
                for o in &self.balls {
                    if o.id == b.id || matches!(o.state, BallState::Flying | BallState::Held) {
                        continue;
                    }
                    if next.distance(o.pos) >= b.radius + o.radius || next.y > o.pos.y {
                        continue;
                    }
                    let dx = next.x - o.pos.x;
                    if dx.abs() <= o.radius * BALANCE {
                        state = BallState::Resting(Support::Ball { id: o.id, dx });
                    } else {
                        // Rolls off the side.
                        b.vel.x = dx.signum() * b.vel.x.abs().max(90.0);
                        b.vel.y *= 0.3;
                    }
                    break;
                }
            }
            if state == BallState::Flying
                && let Some(d) = drifts.find_ledge(next.x, b.pos.y + b.radius, next.y + b.radius)
            {
                state = BallState::Resting(Support::Surface {
                    id: d.id(),
                    dx: next.x - d.x(),
                });
            }
            b.pos = next;
            b.state = state;
            if state != BallState::Flying {
                b.vel = Vec2::ZERO;
            }
            self.balls[i] = b;
        }
        self.balls.retain(|b| b.pos.y - b.radius < bottom + 50.0);
    }

    /// Places resting balls on their supports; drops those whose support
    /// vanished.
    fn settle(&mut self, drifts: &DriftField) {
        // Stacks are shallow; a few passes resolve chains bottom-up.
        for _ in 0..6 {
            let mut changed = false;
            for i in 0..self.balls.len() {
                let BallState::Resting(support) = self.balls[i].state else {
                    continue;
                };
                let r = self.balls[i].radius;
                let placed = match support {
                    Support::Surface { id, dx } => drifts.get(id).and_then(|d| {
                        let x = d.x() + dx;
                        let col = d.column_at(x)?;
                        d.exposed()[col].then(|| Vec2::new(x, d.base_y(col) - r * 0.97))
                    }),
                    Support::Ball { id, dx } => self.index(id).and_then(|j| {
                        let base = self.balls[j];
                        (base.state != BallState::Flying).then(|| {
                            Vec2::new(
                                base.pos.x + dx,
                                base.pos.y - (base.radius + r) * (1.0 - SINK),
                            )
                        })
                    }),
                };
                let b = &mut self.balls[i];
                match placed {
                    Some(p) => {
                        if p != b.pos {
                            b.pos = p;
                            changed = true;
                        }
                    }
                    None => {
                        b.state = BallState::Flying;
                        b.vel = Vec2::ZERO;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Breaks every ball into falling snow.
    pub fn clear(&mut self, spills: &mut Vec<Spill>) {
        for b in self.balls.drain(..) {
            spills.push(Spill {
                pos: b.pos,
                vel: Vec2::new(0.0, 40.0),
                volume: b.volume(),
            });
        }
        self.held = None;
    }

    /// Stacks of two or more balls, bottom first, each smaller than the one
    /// below: snowmen.
    pub fn snowmen(&self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        for (i, b) in self.balls.iter().enumerate() {
            if !matches!(b.state, BallState::Resting(Support::Surface { .. })) {
                continue;
            }
            let mut stack = vec![i];
            loop {
                let top = &self.balls[*stack.last().expect("non-empty")];
                let next = self.balls.iter().position(|o| {
                    matches!(o.state, BallState::Resting(Support::Ball { id, .. }) if id == top.id)
                        && o.radius <= top.radius * 1.05
                });
                match next {
                    Some(j) if stack.len() < 4 => stack.push(j),
                    _ => break,
                }
            }
            if stack.len() >= 2 {
                out.push(stack);
            }
        }
        out
    }
}

fn radius_for(volume: f32) -> f32 {
    (volume / (std::f32::consts::PI * PACKING)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::scene::Obstacle;
    use crate::settings::Settings;

    fn scene() -> Scene {
        Scene {
            screens: vec![Rect::new(0.0, 0.0, 1000.0, 800.0)],
            obstacles: vec![Obstacle {
                id: 1,
                rect: Rect::new(100.0, 400.0, 600.0, 300.0),
                holds_snow: true,
                corner_radius: 0.0,
            }],
            ..Scene::default()
        }
    }

    fn snowy_field(scene: &Scene) -> DriftField {
        let mut f = DriftField::default();
        f.sync(scene, &Settings::default(), 1.0 / 60.0, &mut Vec::new());
        for i in 0..200 {
            f.deposit(0, i, 30.0);
        }
        f
    }

    fn frame(
        balls: &mut Snowballs,
        scene: &mut Scene,
        f: &mut DriftField,
        cursor: Vec2,
        down: bool,
    ) {
        scene.cursor = Some(cursor);
        scene.mouse_down = down;
        scene.grab = true;
        balls.update(1.0 / 60.0, scene, Vec2::ZERO, f);
    }

    #[test]
    fn rolling_grows_a_ball_from_the_pile() {
        let mut scene = scene();
        let mut f = snowy_field(&scene);
        let before = f.total_volume();
        let mut balls = Snowballs::default();
        let mut p = Vec2::new(150.0, 396.0);
        frame(&mut balls, &mut scene, &mut f, p, true);
        assert_eq!(balls.balls().len(), 1);
        let r0 = balls.balls()[0].radius;
        for _ in 0..60 {
            p.x += 6.0;
            frame(&mut balls, &mut scene, &mut f, p, true);
        }
        assert!(balls.balls()[0].radius > r0 + 3.0);
        assert!(f.total_volume() < before);
    }

    #[test]
    fn released_ball_lands_on_window_and_stacks() {
        let mut scene = scene();
        let mut f = snowy_field(&scene);
        let mut balls = Snowballs::default();
        // Make two balls and drop them at the same x.
        for size in [12.0, 8.0] {
            let p = Vec2::new(300.0, 396.0);
            frame(&mut balls, &mut scene, &mut f, p, true);
            let last = balls.balls.len() - 1;
            balls.balls[last].radius = size;
            frame(
                &mut balls,
                &mut scene,
                &mut f,
                Vec2::new(400.0, 200.0),
                true,
            );
            frame(
                &mut balls,
                &mut scene,
                &mut f,
                Vec2::new(400.0, 200.0),
                false,
            );
            for _ in 0..120 {
                frame(
                    &mut balls,
                    &mut scene,
                    &mut f,
                    Vec2::new(900.0, 100.0),
                    false,
                );
            }
        }
        let b = balls.balls();
        assert!(
            matches!(b[0].state, BallState::Resting(Support::Surface { .. })),
            "{:?}",
            b[0].state
        );
        assert!(
            matches!(b[1].state, BallState::Resting(Support::Ball { .. })),
            "{:?}",
            b[1].state
        );
        assert_eq!(balls.snowmen().len(), 1);
    }

    #[test]
    fn ball_falls_when_window_closes() {
        let mut scene = scene();
        let mut f = snowy_field(&scene);
        let mut balls = Snowballs::default();
        frame(
            &mut balls,
            &mut scene,
            &mut f,
            Vec2::new(300.0, 396.0),
            true,
        );
        frame(
            &mut balls,
            &mut scene,
            &mut f,
            Vec2::new(300.0, 300.0),
            false,
        );
        for _ in 0..60 {
            frame(
                &mut balls,
                &mut scene,
                &mut f,
                Vec2::new(900.0, 100.0),
                false,
            );
        }
        assert!(matches!(balls.balls()[0].state, BallState::Resting(_)));
        scene.obstacles.clear();
        f.sync(&scene, &Settings::default(), 1.0 / 60.0, &mut Vec::new());
        frame(
            &mut balls,
            &mut scene,
            &mut f,
            Vec2::new(900.0, 100.0),
            false,
        );
        assert_eq!(balls.balls()[0].state, BallState::Flying);
    }
}
