# freeflake

Free, open-source desktop snow: an alternative to the paid Flake
(tryflake.ru), plus snowballs/snowmen and an advent Christmas tree. Rust
workspace, MIT. macOS 14+, Windows 10 22H2+/11, Linux X11 and Wayland through
XWayland. Russian and English UI. Product priorities: feels alive and
charming, never gets in the way, light on CPU and battery.

Do not copy code from Snowhush (PolyForm Noncommercial since 0.6.2) or other
non-MIT snow apps; ideas are fine, code is written from scratch.

## Build, run, test

```sh
cargo run --release                         # the app (menu bar / tray)
cargo run --release -- --settings           # also open the settings window
cargo test --workspace                      # core unit tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo check --workspace --target x86_64-pc-windows-msvc      # cross-check
cargo check --workspace --target x86_64-unknown-linux-gnu
FREEFLAKE_DATE=2026-12-24 cargo run --release               # fake today's date
FREEFLAKE_PROFILE=1 RUST_LOG=info cargo run --release       # frame timings every 5 s
FREEFLAKE_LANG=ru|en                                        # force UI language
cargo run --release -p freeflake-render --example snapshot -- out.png [seconds] [light|steady|blizzard]
FREEFLAKE_FRAMES=dir:90 cargo run ... --example snapshot ...  # animation frames (docs/snow.gif)
cargo run --release -p freeflake-render --example tree -- tree.png [day] [ornaments]
python3 design/make_icon.py                 # docs/icon.png + packaging/macos/AppIcon.icns
```

- Verify visual changes with the offscreen examples (`snapshot`, `tree`):
  they need no display and never capture the user's screen. If you do run
  the real app and take a screenshot, capture only freeflake's own window
  (`screencapture -l <window id>`), never the whole screen.
- Releases: push a `vX.Y.Z` tag; `.github/workflows/release.yml` builds
  macOS (tar.gz + .app.zip, arm64 and x86_64), Windows (zip) and Linux
  (tar.gz, x86_64 and aarch64), uploads them with `SHA256SUMS`. The Homebrew
  formula lives in `kewldan/homebrew-tap` (`Formula/freeflake.rb`, template in
  `packaging/homebrew/`); the workflow updates it only if the
  `HOMEBREW_TAP_TOKEN` secret exists, otherwise update it by hand.
- Version lives once in `[workspace.package]` of the root `Cargo.toml`.

## Architecture

- `crates/core` — no OS or GPU code, unit-tested.
  - `geom` (`Rect`, interval subtraction), `settings` (`Settings`,
    `Intensity`, persisted as TOML), `scene` (`Scene`, `Obstacle`,
    `SurfaceId`): what the platform reports each frame.
  - `flake` — falling particles: `Snow` (terminal velocity, sway, depth) and
    `Chunk` (packed snow under gravity, from spills).
  - `drift` — `DriftField`: per-surface height maps of 3-pt columns. Follows
    windows, spills snow when a window is shaken/closed/shrunk, relaxes to
    the angle of repose, melts, mitten (`brush`), shovel (`clear`).
  - `snowball` — rolling, throwing, resting on surfaces or other balls,
    snowman detection (stacks of ≥2 decreasing balls).
  - `advent` — `Tree`: calendar rules, gift table, floor/hung ornaments,
    pointer interaction, branch ledges. Tree-local coordinates
    (`TREE_SIZE`).
  - `world` — `World::step(dt, &Scene)` ties it all together.
- `crates/render` — wgpu. `DrawList` (mesh under discs, instanced discs, mesh
  on top) built from the world (`draw_list.rs`) or the tree (`tree.rs`).
  `Renderer` holds pipelines; each window has a `Target` with its own
  surface, viewport uniform and geometry buffers. `render_offscreen` for
  examples/tests. `shader.wgsl` writes premultiplied alpha.
- `crates/platform` — `Desktop` trait: `screens`, `obstacles` (front to
  back, own windows excluded), `pointer`, `quality`, `configure_window`
  (overlay or desktop layer), `set_click_through`. `macos.rs` (AppKit +
  CGWindowList), `windows.rs` (Win32 + DWM), `linux.rs` (x11rb: RandR,
  EWMH, XFixes). The window list is read on a background "window-watcher"
  thread with adaptive polling (16 ms while windows move, 66 ms when still).
- `crates/app` — `app.rs` (winit `ApplicationHandler`, one overlay per screen,
  frame pacing, click catching), `tree_window.rs`, `settings_window.rs`
  (egui on the shared device), `tray.rs` (tray-icon/muda; ksni on Linux),
  `config.rs` (settings.toml, advent.json, `today()`), `autostart.rs`,
  `i18n.rs` (`strings!` macro, ru/en).

## Invariants and gotchas

- One global coordinate space: desktop points, origin at the top-left of the
  primary screen, y down. Platforms convert (Cocoa is bottom-left; Windows and
  X11 report physical pixels, divided by the primary screen's scale).
- `Scene::obstacles` is ordered front to back. A drift's `front` is the number
  of obstacles in front of it; ground drifts use all of them. Tree ledges are
  appended after the real windows (they sit behind everything).
- Our own windows must never appear in the obstacle list (filtered by pid).
- Overlays are click-through by default. They catch clicks only while the grab
  modifier (Option/Alt) is held over snow or a snowball, or a ball is held;
  the tree window catches clicks only over something interactive and not
  covered by another window. Pointer state is polled, not taken from winit
  events.
- Snow that leaves a pile becomes `Spill`s, then `Chunk` flakes: volume is
  conserved (tests check it). Keep it that way when adding mechanics.
- `Settings::sanitized` clamps anything loaded from disk or edited in the UI.
- On Windows, wgpu must use DX12 with `DxgiFromVisual` (DirectComposition),
  otherwise the overlay is opaque. On Linux the app forces winit's X11
  backend; overlays are override-redirect.
- egui needs the `default_fonts` feature; its font has no ⌥ glyph.
- Commits: Conventional Commits, no AI attribution, author kewldan, work on
  `main`.
