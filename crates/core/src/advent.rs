//! The advent tree: a gift for every day of December, ornaments to hang.
//!
//! Everything here works in tree-local points: the tree window is
//! [`TREE_SIZE`] points large with the origin at its top-left corner.

use std::collections::BTreeSet;

use chrono::{Datelike, NaiveDate};
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::geom::Rect;

/// Size of the tree window, in points.
pub const TREE_SIZE: Vec2 = Vec2::new(360.0, 540.0);
/// Floor line the tree and gifts stand on.
pub const FLOOR_Y: f32 = 470.0;
const CENTER_X: f32 = TREE_SIZE.x / 2.0;
/// How close to an ornament a click has to be to pick it up.
const PICK_RADIUS: f32 = 14.0;
/// Visible gifts at most; older ones queue behind.
const MAX_GIFTS_SHOWN: usize = 6;
/// Ornaments shown on the floor at most.
const MAX_FLOOR: usize = 14;

/// One triangular layer of the tree.
#[derive(Clone, Copy, Debug)]
pub struct Tier {
    pub apex: Vec2,
    pub bottom: f32,
    pub half_width: f32,
}

impl Tier {
    pub fn contains(&self, p: Vec2) -> bool {
        if p.y < self.apex.y || p.y > self.bottom {
            return false;
        }
        let t = (p.y - self.apex.y) / (self.bottom - self.apex.y);
        (p.x - self.apex.x).abs() <= self.half_width * t
    }

    /// Half width of the tier at height `y`.
    pub fn half_width_at(&self, y: f32) -> f32 {
        let t = ((y - self.apex.y) / (self.bottom - self.apex.y)).clamp(0.0, 1.0);
        self.half_width * t
    }
}

/// Tiers from the top down.
pub const TIERS: [Tier; 4] = [
    Tier {
        apex: Vec2::new(CENTER_X, 52.0),
        bottom: 160.0,
        half_width: 68.0,
    },
    Tier {
        apex: Vec2::new(CENTER_X, 105.0),
        bottom: 245.0,
        half_width: 104.0,
    },
    Tier {
        apex: Vec2::new(CENTER_X, 175.0),
        bottom: 338.0,
        half_width: 138.0,
    },
    Tier {
        apex: Vec2::new(CENTER_X, 255.0),
        bottom: 432.0,
        half_width: 168.0,
    },
];

/// Trunk rectangle.
pub const TRUNK: Rect = Rect::new(CENTER_X - 17.0, 430.0, 34.0, FLOOR_Y - 430.0);

/// Spot where the topper star sits.
pub const TOPPER_SPOT: Vec2 = Vec2::new(CENTER_X, 44.0);

pub fn on_tree(p: Vec2) -> bool {
    TIERS.iter().any(|t| t.contains(p))
}

/// Ornament palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hue {
    Red,
    Gold,
    Blue,
    Silver,
    Purple,
    Green,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "hue")]
pub enum Ornament {
    Ball(Hue),
    Bell,
    Star,
    Icicle,
    Candy,
    Snowflake,
    Bow,
    Pinecone,
    /// A string of lights; drapes across the tree at the height it is dropped.
    Garland,
    /// The star on top of the tree.
    Topper,
}

impl Ornament {
    /// Rough size for picking and layout.
    pub fn radius(self) -> f32 {
        match self {
            Ornament::Garland => 13.0,
            Ornament::Topper => 18.0,
            Ornament::Icicle => 11.0,
            _ => 9.0,
        }
    }
}

/// What is inside each day's gift, December 1 to 31.
pub fn gift(day: u32) -> Vec<Ornament> {
    use Hue::*;
    use Ornament::*;
    match day {
        1 => vec![Ball(Red), Ball(Red), Ball(Red)],
        2 => vec![Garland],
        3 => vec![Ball(Gold), Ball(Gold), Bell],
        4 => vec![Candy, Candy, Candy],
        5 => vec![Ball(Blue), Ball(Blue), Snowflake],
        6 => vec![Icicle, Icicle, Icicle],
        7 => vec![Bow, Bow, Ball(Red)],
        8 => vec![Garland],
        9 => vec![Ball(Silver), Ball(Silver), Ball(Silver)],
        10 => vec![Bell, Bell, Star],
        11 => vec![Snowflake, Snowflake, Ball(Purple)],
        12 => vec![Pinecone, Pinecone, Pinecone],
        13 => vec![Ball(Green), Ball(Green), Candy],
        14 => vec![Garland],
        15 => vec![Star, Star, Icicle],
        16 => vec![Ball(Gold), Ball(Gold), Ball(Gold)],
        17 => vec![Bell, Bow, Ball(Red)],
        18 => vec![Snowflake, Snowflake, Snowflake],
        19 => vec![Ball(Blue), Ball(Blue), Pinecone],
        20 => vec![Garland],
        21 => vec![Candy, Candy, Star],
        22 => vec![Ball(Silver), Ball(Silver), Bell],
        23 => vec![Icicle, Icicle, Bow],
        24 => vec![Topper],
        25 => vec![Ball(Gold), Ball(Red), Ball(Blue), Snowflake],
        26 => vec![Ball(Purple), Ball(Purple), Star],
        27 => vec![Bell, Bell, Candy],
        28 => vec![Ball(Green), Pinecone, Icicle],
        29 => vec![Snowflake, Bow, Ball(Silver)],
        30 => vec![Ball(Red), Ball(Gold), Star],
        31 => vec![Garland, Star, Ball(Gold)],
        _ => Vec::new(),
    }
}

/// The festive season a date belongs to, identified by its December's year.
/// December 1 to January 14 is in season.
pub fn season_of(date: NaiveDate) -> Option<i32> {
    match (date.month(), date.day()) {
        (12, _) => Some(date.year()),
        (1, d) if d <= 14 => Some(date.year() - 1),
        _ => None,
    }
}

/// How many gifts can be opened on `date`: the day of December, or all 31
/// in early January.
pub fn days_available(date: NaiveDate) -> u32 {
    match date.month() {
        12 => date.day(),
        1 if date.day() <= 14 => 31,
        _ => 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub ornament: Ornament,
    pub pos: Vec2,
}

/// Persisted progress of one season.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdventState {
    pub season: i32,
    pub opened: BTreeSet<u32>,
    /// Unpacked ornaments lying on the floor.
    pub floor: Vec<Ornament>,
    pub hung: Vec<Placed>,
}

/// What is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    ornament: Ornament,
    pos: Vec2,
    offset: Vec2,
}

/// The interactive tree.
#[derive(Clone, Debug)]
pub struct Tree {
    state: AdventState,
    today: NaiveDate,
    drag: Option<Drag>,
    hover: Option<Vec2>,
    /// Gifts just opened, for a short burst animation: (position, age).
    bursts: Vec<(Vec2, f32)>,
    time: f32,
}

impl Tree {
    /// Restores the tree, starting fresh if the saved season is over.
    pub fn new(mut state: AdventState, today: NaiveDate) -> Self {
        if let Some(season) = season_of(today)
            && season != state.season
        {
            state = AdventState {
                season,
                ..AdventState::default()
            };
        }
        Self {
            state,
            today,
            drag: None,
            hover: None,
            bursts: Vec::new(),
            time: 0.0,
        }
    }

    pub fn state(&self) -> &AdventState {
        &self.state
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn set_today(&mut self, today: NaiveDate) {
        if today != self.today {
            *self = Self::new(std::mem::take(&mut self.state), today);
        }
    }

    pub fn advance(&mut self, dt: f32) {
        self.time += dt;
        for b in &mut self.bursts {
            b.1 += dt;
        }
        self.bursts.retain(|b| b.1 < 0.8);
    }

    pub fn bursts(&self) -> &[(Vec2, f32)] {
        &self.bursts
    }

    /// Days whose gifts are waiting under the tree, oldest first.
    pub fn pending_days(&self) -> Vec<u32> {
        (1..=days_available(self.today))
            .filter(|d| !self.state.opened.contains(d))
            .collect()
    }

    /// Visible gift boxes with their rectangles.
    pub fn gift_boxes(&self) -> Vec<(u32, Rect)> {
        let days = self.pending_days();
        let shown = &days[..days.len().min(MAX_GIFTS_SHOWN)];
        let n = shown.len() as f32;
        let w = 30.0;
        let gap = 6.0;
        let total = n * w + (n - 1.0).max(0.0) * gap;
        let x0 = CENTER_X - total / 2.0;
        shown
            .iter()
            .enumerate()
            .map(|(i, &d)| {
                let size = if i % 2 == 0 { 26.0 } else { 22.0 };
                let x = x0 + i as f32 * (w + gap) + (w - size) / 2.0;
                (d, Rect::new(x, FLOOR_Y - size + 4.0, size, size))
            })
            .collect()
    }

    /// Where floor ornaments lie.
    pub fn floor_slots(&self) -> Vec<(Ornament, Vec2)> {
        let items = &self.state.floor[..self.state.floor.len().min(MAX_FLOOR)];
        let per_row = 7;
        items
            .iter()
            .enumerate()
            .map(|(i, &o)| {
                let row = (i / per_row) as f32;
                let col = (i % per_row) as f32;
                let x = 40.0 + col * 47.0 + row * 20.0;
                (o, Vec2::new(x, FLOOR_Y + 22.0 + row * 26.0))
            })
            .collect()
    }

    pub fn hung(&self) -> &[Placed] {
        &self.state.hung
    }

    /// The ornament following the cursor, if any.
    pub fn dragged(&self) -> Option<(Ornament, Vec2)> {
        self.drag.map(|d| (d.ornament, d.pos))
    }

    pub fn hover(&self) -> Option<Vec2> {
        self.hover
    }

    /// Whether `p` is something clickable.
    pub fn interactive_at(&self, p: Vec2) -> bool {
        self.drag.is_some()
            || self
                .gift_boxes()
                .iter()
                .any(|(_, r)| r.expand(3.0).contains(p))
            || self.pick_hung(p).is_some()
            || self.pick_floor(p).is_some()
    }

    fn pick_hung(&self, p: Vec2) -> Option<usize> {
        self.state
            .hung
            .iter()
            .rposition(|h| h.pos.distance(p) <= h.ornament.radius().max(PICK_RADIUS))
    }

    fn pick_floor(&self, p: Vec2) -> Option<usize> {
        self.floor_slots()
            .iter()
            .position(|(o, pos)| pos.distance(p) <= o.radius().max(PICK_RADIUS))
    }

    pub fn pointer_move(&mut self, p: Vec2) {
        self.hover = Some(p);
        if let Some(d) = &mut self.drag {
            d.pos = p + d.offset;
        }
    }

    pub fn pointer_leave(&mut self) {
        self.hover = None;
    }

    /// Returns true when persisted state changed.
    pub fn pointer_down(&mut self, p: Vec2) -> bool {
        self.hover = Some(p);
        if let Some((day, rect)) = self
            .gift_boxes()
            .into_iter()
            .find(|(_, r)| r.expand(3.0).contains(p))
        {
            self.state.opened.insert(day);
            self.state.floor.extend(gift(day));
            self.bursts.push((rect.center(), 0.0));
            return true;
        }
        if let Some(i) = self.pick_hung(p) {
            let h = self.state.hung.remove(i);
            self.drag = Some(Drag {
                ornament: h.ornament,
                pos: h.pos,
                offset: h.pos - p,
            });
            return true;
        }
        if let Some(i) = self.pick_floor(p) {
            let (o, pos) = self.floor_slots()[i];
            self.state.floor.remove(i);
            self.drag = Some(Drag {
                ornament: o,
                pos,
                offset: pos - p,
            });
            return true;
        }
        false
    }

    /// Drops whatever is dragged. Returns true when state changed.
    pub fn pointer_up(&mut self, p: Vec2) -> bool {
        let Some(d) = self.drag.take() else {
            return false;
        };
        let pos = p + d.offset;
        let placed = match d.ornament {
            Ornament::Topper if pos.distance(TOPPER_SPOT) < 70.0 || on_tree(pos) => {
                // Only one star on top; a second one goes back to the floor.
                if self
                    .state
                    .hung
                    .iter()
                    .any(|h| h.ornament == Ornament::Topper)
                {
                    None
                } else {
                    Some(TOPPER_SPOT)
                }
            }
            Ornament::Topper => None,
            _ if on_tree(pos) => Some(pos),
            _ => None,
        };
        match placed {
            Some(pos) => self.state.hung.push(Placed {
                ornament: d.ornament,
                pos,
            }),
            None => self.state.floor.push(d.ornament),
        }
        true
    }

    /// Flat ledges where snow can settle, in tree-local points.
    pub fn ledges() -> Vec<Rect> {
        TIERS
            .iter()
            .map(|t| {
                Rect::new(
                    t.apex.x - t.half_width * 0.92,
                    t.bottom - 2.0,
                    t.half_width * 1.84,
                    2.0,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(m: u32, d: u32) -> NaiveDate {
        let y = if m == 1 { 2027 } else { 2026 };
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn calendar_rules() {
        assert_eq!(days_available(date(11, 30)), 0);
        assert_eq!(days_available(date(12, 1)), 1);
        assert_eq!(days_available(date(12, 24)), 24);
        assert_eq!(days_available(date(1, 3)), 31);
        assert_eq!(days_available(date(1, 15)), 0);
        assert_eq!(season_of(date(1, 3)), Some(2026));
        assert_eq!(season_of(date(12, 3)), Some(2026));
        assert!(season_of(date(2, 1)).is_none());
        for d in 1..=31 {
            assert!(!gift(d).is_empty(), "day {d} has no gift");
        }
    }

    #[test]
    fn missed_days_stay_openable() {
        let mut tree = Tree::new(AdventState::default(), date(12, 3));
        assert_eq!(tree.pending_days(), vec![1, 2, 3]);
        let (day, rect) = tree.gift_boxes()[0];
        assert_eq!(day, 1);
        assert!(tree.pointer_down(rect.center()));
        assert_eq!(tree.pending_days(), vec![2, 3]);
        assert_eq!(tree.state().floor.len(), 3);
    }

    #[test]
    fn hang_and_move_ornament() {
        let mut tree = Tree::new(AdventState::default(), date(12, 1));
        let (_, rect) = tree.gift_boxes()[0];
        tree.pointer_down(rect.center());
        let (_, pos) = tree.floor_slots()[0];
        assert!(tree.pointer_down(pos));
        let spot = Vec2::new(CENTER_X, 300.0);
        tree.pointer_move(spot);
        tree.pointer_up(spot);
        assert_eq!(tree.hung().len(), 1);
        assert_eq!(tree.state().floor.len(), 2);
        // Dropping off the tree returns it to the floor.
        let hung = tree.hung()[0].pos;
        tree.pointer_down(hung);
        tree.pointer_up(Vec2::new(5.0, 5.0));
        assert!(tree.hung().is_empty());
        assert_eq!(tree.state().floor.len(), 3);
    }

    #[test]
    fn topper_snaps_to_the_top() {
        let state = AdventState {
            season: 2026,
            floor: vec![Ornament::Topper],
            ..AdventState::default()
        };
        let mut tree = Tree::new(state, date(12, 24));
        let (_, pos) = tree.floor_slots()[0];
        tree.pointer_down(pos);
        tree.pointer_up(Vec2::new(CENTER_X + 10.0, 90.0));
        assert_eq!(tree.hung()[0].pos, TOPPER_SPOT);
    }

    #[test]
    fn new_season_resets_the_tree() {
        let state = AdventState {
            season: 2025,
            opened: [1, 2].into(),
            ..AdventState::default()
        };
        let tree = Tree::new(state, date(12, 2));
        assert!(tree.state().opened.is_empty());
        assert_eq!(tree.state().season, 2026);
    }
}
