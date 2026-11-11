//! Converts the simulation state into GPU-ready geometry.

use bytemuck::{Pod, Zeroable};
use freeflake_core::{COLUMN_WIDTH, Drift, FlakeKind, Snowball, World};
use glam::Vec2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MeshVertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Disc {
    pub center: [f32; 2],
    pub radius: f32,
    /// Strength of the dark outline, 0..1.
    pub outline: f32,
    /// Straight (not premultiplied) colour.
    pub color: [f32; 4],
    /// Extra edge softness in radii, 0 for crisp discs.
    pub softness: f32,
}

const SNOW_TOP: [f32; 3] = [0.98, 0.99, 1.0];
const SNOW_BASE: [f32; 3] = [0.84, 0.89, 0.97];
const SNOW_RIM: [f32; 4] = [0.36, 0.45, 0.6, 0.45];

/// Everything to draw this frame, in desktop points.
#[derive(Default)]
pub struct DrawList {
    /// Triangles drawn below the discs.
    pub mesh: Vec<MeshVertex>,
    pub discs: Vec<Disc>,
    /// Triangles drawn above the discs.
    pub mesh_top: Vec<MeshVertex>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.mesh.clear();
        self.discs.clear();
        self.mesh_top.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.mesh.is_empty() && self.discs.is_empty() && self.mesh_top.is_empty()
    }

    /// Rebuilds the list from the world.
    pub fn build(&mut self, world: &World) {
        self.clear();
        for d in world.drifts() {
            self.add_drift(d);
        }
        for f in world.flakes() {
            let (outline, softness, color) = match f.kind {
                FlakeKind::Snow => (0.6 * f.depth, 0.35, [1.0, 1.0, 1.0, f.alpha()]),
                FlakeKind::Chunk => (0.8, 0.1, [0.95, 0.97, 1.0, f.alpha()]),
            };
            self.discs.push(Disc {
                center: f.pos.to_array(),
                radius: f.radius,
                outline,
                color,
                softness,
            });
        }
        let balls = world.snowballs();
        for b in balls {
            self.add_ball(b);
        }
        for stack in world.snowmen() {
            let stack: Vec<&Snowball> = stack.iter().map(|&i| &balls[i]).collect();
            self.add_snowman(&stack);
        }
    }

    fn add_ball(&mut self, b: &Snowball) {
        let r = b.radius;
        let c = b.pos;
        // Shadowed body, then a soft highlight towards the top-left.
        self.discs.push(Disc {
            center: c.to_array(),
            radius: r,
            outline: 0.9,
            color: [0.78, 0.84, 0.93, 1.0],
            softness: 0.0,
        });
        self.discs.push(Disc {
            center: (c + Vec2::new(-0.14, -0.16) * r).to_array(),
            radius: r * 0.82,
            outline: 0.0,
            color: [0.99, 0.995, 1.0, 1.0],
            softness: 0.45,
        });
    }

    fn add_snowman(&mut self, stack: &[&Snowball]) {
        let head = stack[stack.len() - 1];
        let body = stack[stack.len() - 2];
        let (c, r) = (head.pos, head.radius);
        let coal = [0.12, 0.12, 0.14, 1.0];
        let dot = |center: Vec2, radius: f32| Disc {
            center: center.to_array(),
            radius,
            outline: 0.0,
            color: coal,
            softness: 0.1,
        };
        // Eyes.
        for side in [-1.0, 1.0] {
            self.discs.push(dot(
                c + Vec2::new(side * 0.33 * r, -0.2 * r),
                (0.11 * r).max(1.0),
            ));
        }
        // Smile.
        for k in 0..5 {
            let a = std::f32::consts::PI * (0.25 + 0.125 * k as f32);
            let p = c + Vec2::new(a.cos() * 0.5 * r, a.sin() * 0.42 * r + 0.08 * r);
            self.discs.push(dot(p, (0.06 * r).max(0.7)));
        }
        // Buttons on every ball below the head except the bottom one, or on
        // the body of a two-ball snowman.
        let with_buttons = if stack.len() > 2 {
            &stack[1..stack.len() - 1]
        } else {
            &stack[..1]
        };
        for b in with_buttons {
            for k in -1..=1 {
                let p = b.pos + Vec2::new(0.0, k as f32 * 0.32 * b.radius);
                self.discs.push(dot(p, (0.08 * b.radius).max(1.0)));
            }
        }
        // Carrot nose.
        let orange = [0.95, 0.5, 0.12, 1.0];
        let nose = c + Vec2::new(0.0, 0.05 * r);
        self.tri_top(
            nose + Vec2::new(0.0, -0.1 * r),
            nose + Vec2::new(0.0, 0.1 * r),
            nose + Vec2::new(0.6 * r, 0.04 * r),
            orange,
        );
        // Twig arms.
        let brown = [0.36, 0.24, 0.15, 1.0];
        for side in [-1.0f32, 1.0] {
            let from = body.pos + Vec2::new(side * body.radius * 0.8, -0.2 * body.radius);
            let to = from + Vec2::new(side * body.radius * 0.9, -body.radius * 0.6);
            self.line_top(from, to, (0.07 * body.radius).max(1.2), brown);
            let mid = from.lerp(to, 0.65);
            self.line_top(
                mid,
                mid + Vec2::new(side * 0.2, -0.35) * body.radius,
                (0.05 * body.radius).max(1.0),
                brown,
            );
        }
        // Top hat on proper (three-ball) snowmen.
        if stack.len() >= 3 {
            let black = [0.08, 0.08, 0.1, 1.0];
            let brim_y = c.y - r * 0.78;
            self.rect_top(c.x - r * 0.75, brim_y - r * 0.12, r * 1.5, r * 0.14, black);
            self.rect_top(c.x - r * 0.48, brim_y - r * 0.85, r * 0.96, r * 0.75, black);
            self.rect_top(
                c.x - r * 0.48,
                brim_y - r * 0.3,
                r * 0.96,
                r * 0.16,
                [0.75, 0.12, 0.15, 1.0],
            );
        }
    }

    fn tri_top(&mut self, a: Vec2, b: Vec2, c: Vec2, color: [f32; 4]) {
        for p in [a, b, c] {
            self.mesh_top.push(MeshVertex {
                pos: p.to_array(),
                color,
            });
        }
    }

    fn line_top(&mut self, from: Vec2, to: Vec2, width: f32, color: [f32; 4]) {
        let n = (to - from).perp().normalize_or_zero() * width * 0.5;
        self.tri_top(from - n, from + n, to + n, color);
        self.tri_top(from - n, to + n, to - n, color);
    }

    fn rect_top(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let (a, b, c, d) = (
            Vec2::new(x, y),
            Vec2::new(x + w, y),
            Vec2::new(x + w, y + h),
            Vec2::new(x, y + h),
        );
        self.tri_top(a, b, c, color);
        self.tri_top(a, c, d, color);
    }

    fn add_drift(&mut self, d: &Drift) {
        let h = d.heights();
        let exposed = d.exposed();
        let n = h.len();
        let mut i = 0;
        while i < n {
            if !exposed[i] || h[i] < 0.15 {
                i += 1;
                continue;
            }
            let start = i;
            while i < n && exposed[i] && h[i] >= 0.15 {
                i += 1;
            }
            self.add_run(d, start, i);
        }
    }

    /// Adds the visible pile over columns `start..end`.
    fn add_run(&mut self, d: &Drift, start: usize, end: usize) {
        let h = d.heights();
        let smooth = |i: usize| {
            let l = h[i.saturating_sub(1)];
            let r = h[(i + 1).min(h.len() - 1)];
            (l + 2.0 * h[i] + r) * 0.25
        };
        // Contour: left edge, every column centre, right edge.
        let mut top: Vec<(f32, f32)> = Vec::with_capacity(end - start + 2);
        let left = d.x() + start as f32 * COLUMN_WIDTH;
        let right = d.x() + end as f32 * COLUMN_WIDTH;
        top.push((left, smooth(start) * 0.55));
        for i in start..end {
            top.push((d.column_center(i), smooth(i)));
        }
        top.push((right, smooth(end - 1) * 0.55));

        let base = d.y();
        // Rim first, one point taller, then the fill on top of it.
        for (lift, top_color, base_color) in [
            (1.0, SNOW_RIM, SNOW_RIM),
            (0.0, rgba(SNOW_TOP, 1.0), rgba(SNOW_BASE, 1.0)),
        ] {
            for w in top.windows(2) {
                let (x0, h0) = w[0];
                let (x1, h1) = w[1];
                if h0 + h1 <= 0.0 {
                    continue;
                }
                let t0 = base - h0 - lift * (h0 > 0.05) as u8 as f32;
                let t1 = base - h1 - lift * (h1 > 0.05) as u8 as f32;
                let a = MeshVertex {
                    pos: [x0, t0],
                    color: top_color,
                };
                let b = MeshVertex {
                    pos: [x1, t1],
                    color: top_color,
                };
                let c = MeshVertex {
                    pos: [x1, base],
                    color: base_color,
                };
                let e = MeshVertex {
                    pos: [x0, base],
                    color: base_color,
                };
                self.mesh.extend_from_slice(&[a, b, c, a, c, e]);
            }
        }
    }
}

fn rgba(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}
