//! Renders the advent tree on a given date after hanging some ornaments.
//!
//! cargo run --release -p freeflake-render --example tree -- out.png [day] [hang]

use std::fs::File;
use std::io::BufWriter;

use anyhow::Result;
use chrono::NaiveDate;
use freeflake_core::Rect;
use freeflake_core::advent::{AdventState, TREE_SIZE, Tree, on_tree};
use freeflake_render::{DrawList, Gpu, Renderer, wgpu};
use glam::Vec2;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).map(String::as_str).unwrap_or("tree.png");
    let day: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(24);
    let hang: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40);
    let today = NaiveDate::from_ymd_opt(2026, 12, day).unwrap();
    let mut tree = Tree::new(AdventState::default(), today);

    // Open all but the last two gifts.
    while tree.pending_days().len() > 2 {
        let (_, r) = tree.gift_boxes()[0];
        tree.pointer_down(r.center());
    }
    // Hang ornaments at pseudo-random spots on the tree.
    let mut rng = fastrand::Rng::with_seed(4);
    let mut hung = 0;
    while hung < hang && !tree.floor_slots().is_empty() {
        let (_, from) = tree.floor_slots()[0];
        let to = loop {
            let p = Vec2::new(rng.f32() * TREE_SIZE.x, 60.0 + rng.f32() * 360.0);
            if on_tree(p) {
                break p;
            }
        };
        tree.pointer_down(from);
        tree.pointer_move(to);
        tree.pointer_up(to);
        hung += 1;
    }
    tree.advance(1.3);

    let mut list = DrawList::default();
    list.build_tree(&tree);
    let gpu = Gpu::headless()?;
    let renderer = Renderer::with_format(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let view = Rect::new(0.0, 0.0, TREE_SIZE.x, TREE_SIZE.y);
    let image = renderer.render_offscreen(&list, view, 2.0, [0.16, 0.3, 0.48])?;
    let file = BufWriter::new(File::create(out)?);
    let mut enc = png::Encoder::new(file, image.width, image.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&image.pixels)?;
    println!(
        "{out}: {} hung, {} on the floor, {} gifts waiting",
        tree.hung().len(),
        tree.state().floor.len(),
        tree.pending_days().len()
    );
    Ok(())
}
