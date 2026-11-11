//! Drawing the advent tree, in tree-local points.

use freeflake_core::Rect;
use freeflake_core::advent::{FLOOR_Y, Hue, Ornament, TIERS, TREE_SIZE, TRUNK, Tree, on_tree};
use glam::Vec2;

use crate::draw_list::{Disc, DrawList, MeshVertex};

type Rgba = [f32; 4];

const NEEDLE_LIGHT: Rgba = [0.22, 0.58, 0.32, 1.0];
const NEEDLE_DARK: Rgba = [0.06, 0.3, 0.17, 1.0];
const GOLD: Rgba = [0.98, 0.78, 0.25, 1.0];
const BULBS: [Rgba; 4] = [
    [1.0, 0.3, 0.3, 1.0],
    [1.0, 0.85, 0.3, 1.0],
    [0.35, 0.65, 1.0, 1.0],
    [0.4, 0.95, 0.5, 1.0],
];
const GIFT_COLORS: [Rgba; 5] = [
    [0.85, 0.2, 0.25, 1.0],
    [0.2, 0.45, 0.85, 1.0],
    [0.95, 0.75, 0.2, 1.0],
    [0.3, 0.65, 0.4, 1.0],
    [0.6, 0.35, 0.8, 1.0],
];

fn hue(h: Hue) -> Rgba {
    match h {
        Hue::Red => [0.9, 0.16, 0.2, 1.0],
        Hue::Gold => [0.96, 0.74, 0.2, 1.0],
        Hue::Blue => [0.22, 0.48, 0.95, 1.0],
        Hue::Silver => [0.82, 0.85, 0.9, 1.0],
        Hue::Purple => [0.62, 0.3, 0.85, 1.0],
        Hue::Green => [0.25, 0.75, 0.45, 1.0],
    }
}

fn shade(c: Rgba, k: f32) -> Rgba {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

impl DrawList {
    /// Rebuilds the list with the tree scene.
    pub fn build_tree(&mut self, tree: &Tree) {
        self.clear();
        let time = tree.time();
        self.ellipse(
            Vec2::new(TREE_SIZE.x / 2.0, FLOOR_Y + 2.0),
            Vec2::new(150.0, 10.0),
            [0.0, 0.0, 0.0, 0.18],
        );
        self.rect(TRUNK, [0.38, 0.24, 0.14, 1.0], [0.25, 0.15, 0.09, 1.0]);
        for (i, tier) in TIERS.iter().enumerate().rev() {
            self.tier(tier.apex, tier.bottom, tier.half_width, i as u32);
        }
        for p in tree
            .hung()
            .iter()
            .filter(|p| p.ornament == Ornament::Garland)
        {
            self.garland(p.pos.y, time, (p.pos.x * 7.0) as u32);
        }
        for p in tree
            .hung()
            .iter()
            .filter(|p| p.ornament != Ornament::Garland)
        {
            self.ornament(p.ornament, p.pos, 1.0, time);
        }
        for (day, r) in tree.gift_boxes() {
            let hover = tree.hover().is_some_and(|h| r.expand(3.0).contains(h));
            self.gift(r, day, hover, time);
        }
        for (o, pos) in tree.floor_slots() {
            self.ornament(o, pos, 0.9, time);
        }
        for &(pos, age) in tree.bursts() {
            for k in 0..10 {
                let a = k as f32 * std::f32::consts::TAU / 10.0;
                let p = pos + Vec2::new(a.cos(), a.sin()) * (12.0 + age * 70.0);
                self.discs.push(Disc {
                    center: p.to_array(),
                    radius: 2.5 * (1.0 - age),
                    outline: 0.0,
                    color: BULBS[k % BULBS.len()],
                    softness: 0.5,
                });
            }
        }
        if let Some((o, pos)) = tree.dragged() {
            let over = o == Ornament::Topper || on_tree(pos);
            let lift = if over { 1.15 } else { 1.05 };
            self.ornament(o, pos, lift, time);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tri(&mut self, top: bool, a: Vec2, b: Vec2, c: Vec2, ca: Rgba, cb: Rgba, cc: Rgba) {
        let out = if top {
            &mut self.mesh_top
        } else {
            &mut self.mesh
        };
        out.push(MeshVertex {
            pos: a.to_array(),
            color: ca,
        });
        out.push(MeshVertex {
            pos: b.to_array(),
            color: cb,
        });
        out.push(MeshVertex {
            pos: c.to_array(),
            color: cc,
        });
    }

    fn rect(&mut self, r: Rect, top: Rgba, bottom: Rgba) {
        let (a, b) = (Vec2::new(r.x, r.y), Vec2::new(r.right(), r.y));
        let (c, d) = (Vec2::new(r.right(), r.bottom()), Vec2::new(r.x, r.bottom()));
        self.tri(false, a, b, c, top, top, bottom);
        self.tri(false, a, c, d, top, bottom, bottom);
    }

    fn ellipse(&mut self, c: Vec2, r: Vec2, color: Rgba) {
        let n = 24;
        let edge = [color[0], color[1], color[2], 0.0];
        for i in 0..n {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
            let p0 = c + Vec2::new(a0.cos(), a0.sin()) * r;
            let p1 = c + Vec2::new(a1.cos(), a1.sin()) * r;
            self.tri(false, c, p0, p1, color, edge, edge);
        }
    }

    /// One tier with a ragged lower edge.
    fn tier(&mut self, apex: Vec2, bottom: f32, half_width: f32, seed: u32) {
        let teeth = 9 + seed as usize * 2;
        let mut edge = Vec::with_capacity(teeth * 2 + 1);
        for i in 0..=teeth * 2 {
            let t = i as f32 / (teeth * 2) as f32;
            let x = apex.x - half_width + t * half_width * 2.0;
            let dip = if i % 2 == 1 { 9.0 } else { 0.0 };
            let wobble = ((i as f32 + seed as f32 * 3.1) * 2.3).sin() * 2.0;
            edge.push(Vec2::new(x, bottom - dip + wobble));
        }
        for w in edge.windows(2) {
            let side = |p: Vec2| {
                // Darker towards the right and the edge: light from the top-left.
                let k = 1.0 - ((p.x - apex.x) / half_width).clamp(-1.0, 1.0) * 0.18;
                shade(NEEDLE_DARK, k)
            };
            self.tri(
                false,
                apex,
                w[0],
                w[1],
                NEEDLE_LIGHT,
                side(w[0]),
                side(w[1]),
            );
        }
        // Lighter needle strokes.
        for k in 0..teeth {
            let t = (k as f32 + 0.5) / teeth as f32;
            let to = Vec2::new(
                apex.x - half_width * 0.85 + t * half_width * 1.7,
                bottom - 6.0,
            );
            let from = apex.lerp(to, 0.55);
            self.line(false, from, to, 1.2, [0.3, 0.66, 0.4, 0.35]);
        }
    }

    fn line(&mut self, top: bool, from: Vec2, to: Vec2, width: f32, color: Rgba) {
        let n = (to - from).perp().normalize_or_zero() * width * 0.5;
        self.tri(top, from - n, from + n, to + n, color, color, color);
        self.tri(top, from - n, to + n, to - n, color, color, color);
    }

    /// Lights draped across the tree at height `y`.
    fn garland(&mut self, y: f32, time: f32, seed: u32) {
        let half = TIERS.iter().map(|t| t.half_width_at(y)).fold(0.0, f32::max) * 0.95;
        if half < 10.0 {
            return;
        }
        let cx = TREE_SIZE.x / 2.0;
        let tilt = if seed.is_multiple_of(2) { 8.0 } else { -8.0 };
        let point = |t: f32| {
            let x = cx - half + t * half * 2.0;
            Vec2::new(
                x,
                y + (t - 0.5) * tilt + (t * std::f32::consts::PI).sin() * 14.0,
            )
        };
        let n = 24;
        for i in 0..n {
            let (a, b) = (point(i as f32 / n as f32), point((i + 1) as f32 / n as f32));
            self.line(false, a, b, 1.2, [0.1, 0.12, 0.1, 0.8]);
        }
        let bulbs = (half * 2.0 / 17.0) as usize;
        for i in 0..bulbs {
            let p = point((i as f32 + 0.5) / bulbs as f32);
            let phase = (time * 2.2 + i as f32 * 0.9 + seed as f32).sin();
            let mut c = BULBS[(i + seed as usize) % BULBS.len()];
            let glow = 0.55 + 0.45 * phase.max(0.0);
            c = shade(c, 0.6 + 0.4 * glow);
            self.discs.push(Disc {
                center: p.to_array(),
                radius: 6.0,
                outline: 0.0,
                color: [c[0], c[1], c[2], 0.25 * glow],
                softness: 1.0,
            });
            self.discs.push(Disc {
                center: (p + Vec2::new(0.0, 2.0)).to_array(),
                radius: 2.6,
                outline: 0.3,
                color: c,
                softness: 0.15,
            });
        }
    }

    fn star(&mut self, c: Vec2, r: f32, color: Rgba) {
        let inner = r * 0.45;
        let mut pts = Vec::with_capacity(10);
        for k in 0..10 {
            let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
            let rr = if k % 2 == 0 { r } else { inner };
            pts.push(c + Vec2::new(a.cos(), a.sin()) * rr);
        }
        let center = shade(color, 1.1);
        for k in 0..10 {
            let edge = shade(color, if k % 2 == 0 { 0.85 } else { 1.0 });
            self.tri(true, c, pts[k], pts[(k + 1) % 10], center, edge, edge);
        }
    }

    fn ornament(&mut self, o: Ornament, pos: Vec2, scale: f32, time: f32) {
        let s = scale;
        match o {
            Ornament::Ball(h) => {
                let color = hue(h);
                let r = 8.0 * s;
                self.line(
                    true,
                    pos + Vec2::new(0.0, -r - 6.0 * s),
                    pos + Vec2::new(0.0, -r),
                    0.8,
                    [0.2, 0.2, 0.2, 0.7],
                );
                self.discs.push(Disc {
                    center: pos.to_array(),
                    radius: r,
                    outline: 0.5,
                    color: shade(color, 0.75),
                    softness: 0.0,
                });
                self.discs.push(Disc {
                    center: (pos + Vec2::new(-0.18, -0.2) * r).to_array(),
                    radius: r * 0.75,
                    outline: 0.0,
                    color,
                    softness: 0.5,
                });
                self.discs.push(Disc {
                    center: (pos + Vec2::new(-0.35, -0.38) * r).to_array(),
                    radius: r * 0.22,
                    outline: 0.0,
                    color: [1.0, 1.0, 1.0, 0.8],
                    softness: 0.6,
                });
                let cap = Rect::new(pos.x - 2.5 * s, pos.y - r - 2.0 * s, 5.0 * s, 3.5 * s);
                self.tri(
                    true,
                    cap.origin(),
                    Vec2::new(cap.right(), cap.y),
                    Vec2::new(cap.right(), cap.bottom()),
                    [0.75, 0.75, 0.7, 1.0],
                    [0.75, 0.75, 0.7, 1.0],
                    [0.5, 0.5, 0.45, 1.0],
                );
                self.tri(
                    true,
                    cap.origin(),
                    Vec2::new(cap.right(), cap.bottom()),
                    Vec2::new(cap.x, cap.bottom()),
                    [0.75, 0.75, 0.7, 1.0],
                    [0.5, 0.5, 0.45, 1.0],
                    [0.5, 0.5, 0.45, 1.0],
                );
            }
            Ornament::Bell => {
                let (w, h) = (7.0 * s, 10.0 * s);
                let top = pos + Vec2::new(0.0, -h * 0.6);
                let bl = pos + Vec2::new(-w, h * 0.4);
                let br = pos + Vec2::new(w, h * 0.4);
                self.tri(
                    true,
                    top + Vec2::new(-w * 0.45, 0.0),
                    top + Vec2::new(w * 0.45, 0.0),
                    br,
                    GOLD,
                    GOLD,
                    shade(GOLD, 0.7),
                );
                self.tri(
                    true,
                    top + Vec2::new(-w * 0.45, 0.0),
                    br,
                    bl,
                    GOLD,
                    shade(GOLD, 0.7),
                    shade(GOLD, 0.8),
                );
                self.line(true, bl, br, 2.0 * s, shade(GOLD, 0.6));
                self.discs.push(Disc {
                    center: (pos + Vec2::new(0.0, h * 0.55)).to_array(),
                    radius: 2.0 * s,
                    outline: 0.0,
                    color: shade(GOLD, 0.55),
                    softness: 0.1,
                });
                self.discs.push(Disc {
                    center: (top + Vec2::new(0.0, -1.5 * s)).to_array(),
                    radius: 2.0 * s,
                    outline: 0.0,
                    color: [0.8, 0.15, 0.2, 1.0],
                    softness: 0.1,
                });
            }
            Ornament::Star => self.star(pos, 9.0 * s, GOLD),
            Ornament::Topper => {
                let pulse = 0.85 + 0.15 * (time * 1.7).sin();
                self.discs.push(Disc {
                    center: pos.to_array(),
                    radius: 26.0 * s,
                    outline: 0.0,
                    color: [1.0, 0.9, 0.5, 0.35 * pulse],
                    softness: 1.0,
                });
                self.star(pos, 18.0 * s, [1.0, 0.82, 0.28, 1.0]);
            }
            Ornament::Icicle => {
                let ice = [0.85, 0.95, 1.0, 0.95];
                self.tri(
                    true,
                    pos + Vec2::new(-4.0 * s, -9.0 * s),
                    pos + Vec2::new(4.0 * s, -9.0 * s),
                    pos + Vec2::new(0.0, 13.0 * s),
                    ice,
                    shade(ice, 0.8),
                    [0.6, 0.8, 1.0, 0.9],
                );
            }
            Ornament::Candy => {
                let pts: Vec<Vec2> = (0..=10)
                    .map(|k| {
                        let t = k as f32 / 10.0;
                        if t < 0.6 {
                            pos + Vec2::new(2.0, 10.0 - t / 0.6 * 14.0) * s
                        } else {
                            let a = std::f32::consts::PI * ((t - 0.6) / 0.4);
                            pos + Vec2::new(-2.0 + 4.0 * a.cos(), -4.0 - 4.0 * a.sin()) * s
                        }
                    })
                    .collect();
                for (k, w) in pts.windows(2).enumerate() {
                    let c = if k % 2 == 0 {
                        [0.92, 0.15, 0.2, 1.0]
                    } else {
                        [1.0, 1.0, 1.0, 1.0]
                    };
                    self.line(true, w[0], w[1], 3.0 * s, c);
                }
            }
            Ornament::Snowflake => {
                for k in 0..3 {
                    let a = k as f32 * std::f32::consts::PI / 3.0;
                    let d = Vec2::new(a.cos(), a.sin()) * 9.0 * s;
                    self.line(true, pos - d, pos + d, 2.6 * s, [0.55, 0.7, 0.9, 0.7]);
                    self.line(true, pos - d, pos + d, 1.4 * s, [1.0, 1.0, 1.0, 1.0]);
                }
            }
            Ornament::Bow => {
                let red = [0.88, 0.12, 0.2, 1.0];
                for side in [-1.0f32, 1.0] {
                    self.tri(
                        true,
                        pos,
                        pos + Vec2::new(side * 9.0, -6.0) * s,
                        pos + Vec2::new(side * 9.0, 5.0) * s,
                        red,
                        shade(red, 0.8),
                        shade(red, 0.7),
                    );
                    self.line(
                        true,
                        pos,
                        pos + Vec2::new(side * 4.0, 10.0) * s,
                        2.5 * s,
                        shade(red, 0.85),
                    );
                }
                self.discs.push(Disc {
                    center: pos.to_array(),
                    radius: 2.8 * s,
                    outline: 0.0,
                    color: shade(red, 0.75),
                    softness: 0.1,
                });
            }
            Ornament::Pinecone => {
                let brown = [0.5, 0.32, 0.18, 1.0];
                for k in 0..5 {
                    let y = pos.y - 7.0 * s + k as f32 * 3.5 * s;
                    let r = (3.0 + (k as f32 - 2.0).abs() * -0.6 + 2.0) * s;
                    self.discs.push(Disc {
                        center: [pos.x, y],
                        radius: r,
                        outline: 0.2,
                        color: shade(brown, 0.8 + k as f32 * 0.06),
                        softness: 0.1,
                    });
                }
            }
            Ornament::Garland => {
                // A coil of lights, as it lies on the floor or is dragged.
                for k in 0..7 {
                    let a = k as f32 * std::f32::consts::TAU / 7.0 + time * 0.5;
                    let p = pos + Vec2::new(a.cos() * 9.0, a.sin() * 5.0) * s;
                    self.discs.push(Disc {
                        center: p.to_array(),
                        radius: 2.4 * s,
                        outline: 0.3,
                        color: BULBS[k % BULBS.len()],
                        softness: 0.2,
                    });
                }
            }
        }
    }

    fn gift(&mut self, r: Rect, day: u32, hover: bool, time: f32) {
        let mut color = GIFT_COLORS[day as usize % GIFT_COLORS.len()];
        let ribbon_color = if day.is_multiple_of(2) {
            GOLD
        } else {
            [0.95, 0.95, 0.97, 1.0]
        };
        let (dy, lift) = if hover {
            ((time * 8.0).sin() * 1.5 - 2.0, 1.12)
        } else {
            (0.0, 1.0)
        };
        color = shade(color, lift);
        let r = Rect::new(r.x, r.y + dy, r.w, r.h);
        self.rect(r, color, shade(color, 0.7));
        let lid = Rect::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, 6.0);
        self.rect(lid, shade(color, 1.1), shade(color, 0.85));
        let rw = r.w * 0.16;
        self.rect(
            Rect::new(r.center().x - rw / 2.0, lid.y, rw, r.bottom() - lid.y),
            ribbon_color,
            shade(ribbon_color, 0.8),
        );
        self.rect(
            Rect::new(r.x, r.center().y - rw / 2.0, r.w, rw),
            ribbon_color,
            shade(ribbon_color, 0.8),
        );
        let knot = Vec2::new(r.center().x, lid.y - 1.0);
        for side in [-1.0f32, 1.0] {
            self.tri(
                false,
                knot,
                knot + Vec2::new(side * 7.0, -6.0),
                knot + Vec2::new(side * 7.0, 2.0),
                ribbon_color,
                shade(ribbon_color, 0.85),
                shade(ribbon_color, 0.75),
            );
        }
    }
}
