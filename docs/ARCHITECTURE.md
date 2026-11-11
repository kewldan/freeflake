# Architecture

freeflake is a Cargo workspace with a platform-independent core and thin
native layers.

```
crates/
  core/      simulation: flakes, piles, snowballs, advent tree. No OS or GPU code.
  render/    wgpu renderer: one Renderer, one Target per overlay window.
  platform/  native desktop integration behind the `Desktop` trait.
  app/       the `freeflake` binary: winit event loop, tray, settings, persistence.
```

## Data flow per frame

1. `platform` reports screens, on-screen windows (front to back), cursor and
   the grab modifier (Option / Alt). Coordinates are global desktop points,
   origin at the top-left of the primary screen.
2. `core::World::step` advances the simulation against that `Scene`.
3. `render::DrawList::build` turns the world into triangles and instanced
   discs; the renderer uploads them once and draws into every overlay window.

## Overlay windows

One borderless transparent window per screen, above everything, ignoring
mouse events. While the grab modifier is held over snow or the tree, the
window under the cursor starts taking clicks so snowballs and ornaments can be
dragged.

The advent tree lives in a separate window at desktop level, below normal
windows.

## Platforms

| | Overlay | Window list | Notes |
|---|---|---|---|
| macOS 14+ | NSWindow at screen-saver level | `CGWindowListCopyWindowInfo` (bounds need no permission) | |
| Windows 10 22H2+ / 11 | layered topmost tool window, DirectComposition swapchain | `EnumWindows` + DWM frame bounds | untested |
| Linux X11 | override-redirect ARGB window, empty XFixes input region | `_NET_CLIENT_LIST_STACKING` | untested |
| Linux Wayland | through XWayland, same as X11 | X11 clients only | untested |

## Testing

- `cargo test` covers the simulation.
- `cargo run --release -p freeflake-render --example snapshot -- out.png`
  simulates a fake desktop and renders it offscreen, without a display.
