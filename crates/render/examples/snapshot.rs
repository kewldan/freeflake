//! Simulates a desktop with a few windows and saves a PNG of the result.
//!
//! cargo run --release -p freeflake-render --example snapshot -- out.png [seconds] [intensity]

use std::fs::File;
use std::io::BufWriter;

use anyhow::Result;
use freeflake_core::{Intensity, Obstacle, Rect, Scene, Settings, World};
use freeflake_render::{DrawList, Gpu, MeshVertex, Renderer, wgpu};
use glam::Vec2;

fn quad(list: &mut Vec<MeshVertex>, r: Rect, color: [f32; 4]) {
    let v = |x, y| MeshVertex { pos: [x, y], color };
    let (a, b, c, d) = (
        v(r.x, r.y),
        v(r.right(), r.y),
        v(r.right(), r.bottom()),
        v(r.x, r.bottom()),
    );
    list.extend_from_slice(&[a, b, c, a, c, d]);
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).map(String::as_str).unwrap_or("snapshot.png");
    let seconds: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let intensity = match args.get(3).map(String::as_str) {
        Some("light") => Intensity::Light,
        Some("blizzard") => Intensity::Blizzard,
        _ => Intensity::Steady,
    };

    let screen = Rect::new(0.0, 0.0, 1440.0, 900.0);
    let windows = [
        (
            1,
            Rect::new(160.0, 260.0, 520.0, 380.0),
            [0.97, 0.97, 0.98, 1.0],
        ),
        (
            2,
            Rect::new(560.0, 420.0, 640.0, 360.0),
            [0.16, 0.17, 0.2, 1.0],
        ),
        (
            3,
            Rect::new(1000.0, 140.0, 360.0, 220.0),
            [0.93, 0.95, 1.0, 1.0],
        ),
    ];
    let dock = Rect::new(380.0, 832.0, 680.0, 64.0);
    // Front to back: the dark window overlaps the light one.
    let mut obstacles: Vec<Obstacle> = vec![
        Obstacle {
            id: 100,
            rect: dock,
            holds_snow: true,
        },
        Obstacle {
            id: 2,
            rect: windows[1].1,
            holds_snow: true,
        },
        Obstacle {
            id: 3,
            rect: windows[2].1,
            holds_snow: true,
        },
        Obstacle {
            id: 1,
            rect: windows[0].1,
            holds_snow: true,
        },
    ];
    obstacles.dedup_by_key(|o| o.id);
    let scene = Scene {
        screens: vec![screen],
        obstacles,
        cursor: Some(Vec2::new(-500.0, -500.0)),
        ..Scene::default()
    };
    let settings = Settings {
        intensity,
        ..Settings::default()
    };
    let mut world = World::new(settings, 7);
    let steps = (seconds * 60.0) as usize;
    let start = std::time::Instant::now();
    for _ in 0..steps {
        world.step(1.0 / 60.0, &scene);
    }
    let sim = start.elapsed();

    // Roll three balls along the light window's pile and stack them on the
    // small window: a snowman.
    let mut scene = scene;
    let frame = |world: &mut World, scene: &mut Scene, p: Vec2, down: bool| {
        scene.cursor = Some(p);
        scene.grab = true;
        scene.mouse_down = down;
        world.step(1.0 / 60.0, scene);
    };
    for len in [420.0, 300.0, 200.0] {
        let start = Vec2::new(180.0, 256.0);
        frame(&mut world, &mut scene, start, true);
        let steps = (len / 4.0) as usize;
        for i in 0..steps {
            let x = 180.0 + (i as f32 * 4.0) % 480.0;
            frame(&mut world, &mut scene, Vec2::new(x, 256.0), true);
        }
        let drop = Vec2::new(1180.0, 40.0);
        for _ in 0..30 {
            frame(&mut world, &mut scene, drop, true);
        }
        frame(&mut world, &mut scene, drop, false);
        for _ in 0..90 {
            frame(&mut world, &mut scene, Vec2::new(-500.0, -500.0), false);
        }
    }
    scene.grab = false;
    for _ in 0..120 {
        world.step(1.0 / 60.0, &scene);
    }
    println!(
        "balls: {:?}",
        world
            .snowballs()
            .iter()
            .map(|b| (b.radius.round(), b.state))
            .collect::<Vec<_>>()
    );

    let mut draw = DrawList::default();
    let t = std::time::Instant::now();
    draw.build(&world);
    let build = t.elapsed();
    let mut background = Vec::new();
    for id in [1, 3, 2] {
        let (_, r, c) = windows.iter().find(|w| w.0 == id).unwrap();
        quad(&mut background, *r, *c);
        quad(
            &mut background,
            Rect::new(r.x, r.y, r.w, 28.0),
            [c[0] * 0.92, c[1] * 0.92, c[2] * 0.93, 1.0],
        );
    }
    quad(&mut background, dock, [0.55, 0.6, 0.7, 0.55]);
    draw.mesh.splice(0..0, background);

    let gpu = Gpu::headless()?;
    let renderer = Renderer::with_format(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let image = renderer.render_offscreen(&draw, screen, 2.0, [0.18, 0.33, 0.52])?;

    let file = BufWriter::new(File::create(out)?);
    let mut enc = png::Encoder::new(file, image.width, image.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&image.pixels)?;
    println!(
        "{out}: {} flakes, {} drift vertices; sim {:.2} ms/step, draw list {:.2} ms",
        world.flakes().len(),
        draw.mesh.len(),
        sim.as_secs_f64() * 1000.0 / steps as f64,
        build.as_secs_f64() * 1000.0,
    );
    Ok(())
}
