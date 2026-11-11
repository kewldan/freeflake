//! Converts the simulation state into GPU-ready geometry.

use bytemuck::{Pod, Zeroable};
use freeflake_core::{COLUMN_WIDTH, Drift, FlakeKind, World};

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
    pub mesh: Vec<MeshVertex>,
    pub discs: Vec<Disc>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.mesh.clear();
        self.discs.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.mesh.is_empty() && self.discs.is_empty()
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
