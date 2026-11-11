# freeflake

Free, open-source snow for your desktop. Snow falls over everything, piles up
on windows, the Dock and the bottom of the screen, and an advent Christmas tree
gets new ornaments every day of December.

- Three snowfall modes (light, steady, blizzard) and adjustable wind
- Snow settles on window tops, the Dock/taskbar and screen bottoms
- Shake a window and the snow flies off; close it and the snow falls
- Brush snow away with the cursor (the "mitten") or clear it all (the "shovel")
- Clicks pass through; it never gets in the way
- Every monitor, low CPU, lighter snow on battery saver or when hot
- Reads only window positions, never their contents. No network, no account.

Status: early development. macOS is tested; Windows and Linux (X11, Wayland
through XWayland) build but need testing on real machines.

## Install

macOS and Linux with Homebrew:

```bash
brew install kewldan/tap/freeflake
```

Or download a build from [Releases](https://github.com/kewldan/freeflake/releases):
`.app.zip` for macOS, `.zip` for Windows, `.tar.gz` for Linux. Builds are not
signed yet; on macOS, if Gatekeeper refuses to open the app, run
`xattr -dr com.apple.quarantine freeflake.app`.

## Use

- The snowflake in the menu bar / tray toggles snowfall modes, wind, piles,
  the mitten, the shovel and the tree. "Settings…" has everything else.
- Hold Option (Alt) and drag along a pile to roll a snowball, let go to throw
  it, stack balls into a snowman.
- From December 1, open the day's gift under the tree and drag the ornaments
  onto the branches.

## Build

```bash
cargo run --release              # the app
cargo run --release -- --settings
cargo test --workspace
cargo run --release -p freeflake-render --example snapshot -- out.png
cargo run --release -p freeflake-render --example tree -- tree.png 24
FREEFLAKE_DATE=2026-12-24 cargo run --release   # pretend it is Christmas Eve
FREEFLAKE_PROFILE=1 RUST_LOG=info cargo run --release   # frame timings
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

MIT
