//! Prints what freeflake sees: screens and obstacles (rectangles only).
//!
//! cargo run -p freeflake-platform --example desktop

fn main() -> anyhow::Result<()> {
    let mut desktop = freeflake_platform::desktop()?;
    for (i, s) in desktop.screens().iter().enumerate() {
        println!("screen {i}: frame {:?} visible {:?} scale {}", s.frame, s.visible, s.scale);
    }
    // Give the watcher thread a moment to read the window list.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let mut obstacles = Vec::new();
    desktop.obstacles(&mut obstacles);
    for o in &obstacles {
        let r = o.rect;
        println!(
            "{:>16x} {:>7.1},{:>7.1} {:>7.1}x{:<7.1} holds snow: {}",
            o.id, r.x, r.y, r.w, r.h, o.holds_snow
        );
    }
    println!("pointer: {:?}", desktop.pointer());
    Ok(())
}
