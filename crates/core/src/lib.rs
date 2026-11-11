//! Platform-independent simulation for freeflake.
//!
//! Platform layers describe the desktop with a [`Scene`] every frame; the
//! [`World`] advances snowfall, piles, snowballs and the advent tree, and the
//! renderer draws whatever the world exposes.

pub mod drift;
pub mod flake;
pub mod geom;
pub mod scene;
pub mod settings;
pub mod world;

pub use drift::{COLUMN_WIDTH, Drift};
pub use flake::{Flake, FlakeKind};
pub use geom::Rect;
pub use scene::{Obstacle, Scene, SurfaceId};
pub use settings::{Intensity, Settings};
pub use world::World;
