//! Basic geometry in desktop coordinates.
//!
//! The whole simulation lives in one global coordinate space measured in
//! logical points: the origin is the top-left corner of the primary screen,
//! `x` grows to the right and `y` grows downwards.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Axis-aligned rectangle in desktop points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn origin(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.w, self.h)
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// Half-open containment: left/top edges are inside, right/bottom are not.
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    pub fn expand(&self, by: f32) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.w + 2.0 * by,
            self.h + 2.0 * by,
        )
    }
}

/// Subtracts the half-open interval `[a, b)` from a sorted list of disjoint
/// intervals.
pub(crate) fn subtract_interval(intervals: &mut Vec<(f32, f32)>, a: f32, b: f32) {
    if a >= b {
        return;
    }
    let mut out = Vec::with_capacity(intervals.len() + 1);
    for &(x0, x1) in intervals.iter() {
        if b <= x0 || a >= x1 {
            out.push((x0, x1));
            continue;
        }
        if a > x0 {
            out.push((x0, a));
        }
        if b < x1 {
            out.push((b, x1));
        }
    }
    *intervals = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Vec2::new(0.0, 0.0)));
        assert!(!r.contains(Vec2::new(10.0, 5.0)));
        assert!(!r.contains(Vec2::new(5.0, 10.0)));
    }

    #[test]
    fn subtract_splits_interval() {
        let mut v = vec![(0.0, 10.0)];
        subtract_interval(&mut v, 3.0, 5.0);
        assert_eq!(v, vec![(0.0, 3.0), (5.0, 10.0)]);
        subtract_interval(&mut v, -1.0, 4.0);
        assert_eq!(v, vec![(5.0, 10.0)]);
        subtract_interval(&mut v, 0.0, 100.0);
        assert!(v.is_empty());
    }
}
