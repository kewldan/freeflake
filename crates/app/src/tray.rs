//! Menu-bar / system-tray icon and its menu.

use anyhow::Result;
use freeflake_core::{Intensity, Settings};
use glam::Vec2;
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::i18n::{S, t};

/// What the user picked in the menu.
pub enum Action {
    Settings(Settings),
    Shovel,
    OpenSettings,
    Quit,
}

const WIND_PRESETS: [(S, f32); 5] = [
    (S::WindLeftStrong, -0.8),
    (S::WindLeft, -0.3),
    (S::WindCalm, 0.0),
    (S::WindRight, 0.3),
    (S::WindRightStrong, 0.8),
];

pub struct Tray {
    _icon: TrayIcon,
    pause: CheckMenuItem,
    intensity: Vec<(Intensity, CheckMenuItem)>,
    wind: Vec<(f32, CheckMenuItem)>,
    accumulate: CheckMenuItem,
    mitten: CheckMenuItem,
    tree: CheckMenuItem,
    shovel: MenuId,
    settings: MenuId,
    quit: MenuId,
}

impl Tray {
    pub fn new(settings: &Settings) -> Result<Self> {
        let pause = CheckMenuItem::new(t(S::Pause), true, settings.paused, None);
        let snowfall = Submenu::new(t(S::Snowfall), true);
        let intensity: Vec<_> = Intensity::ALL
            .into_iter()
            .map(|i| {
                let label = match i {
                    Intensity::Light => t(S::Light),
                    Intensity::Steady => t(S::Steady),
                    Intensity::Blizzard => t(S::Blizzard),
                };
                (
                    i,
                    CheckMenuItem::new(label, true, settings.intensity == i, None),
                )
            })
            .collect();
        for (_, item) in &intensity {
            snowfall.append(item)?;
        }
        let wind_menu = Submenu::new(t(S::Wind), true);
        let wind: Vec<_> = WIND_PRESETS
            .into_iter()
            .map(|(label, x)| (x, CheckMenuItem::new(t(label), true, false, None)))
            .collect();
        for (_, item) in &wind {
            wind_menu.append(item)?;
        }
        let accumulate = CheckMenuItem::new(t(S::Accumulate), true, settings.accumulate, None);
        let mitten = CheckMenuItem::new(t(S::Mitten), true, settings.mitten, None);
        let tree = CheckMenuItem::new(t(S::Tree), true, settings.show_tree, None);
        let shovel = MenuItem::new(t(S::Shovel), true, None);
        let settings_item = MenuItem::new(t(S::Settings), true, None);
        let quit = MenuItem::new(t(S::Quit), true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &pause,
            &PredefinedMenuItem::separator(),
            &snowfall,
            &wind_menu,
            &accumulate,
            &mitten,
            &shovel,
            &PredefinedMenuItem::separator(),
            &tree,
            &settings_item,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("freeflake");
        // macOS tints a black template icon to fit the menu bar; elsewhere
        // the icon is drawn white.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(snowflake_icon(0));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(snowflake_icon(255));
        let icon = builder.build()?;

        let this = Self {
            _icon: icon,
            pause,
            intensity,
            wind,
            accumulate,
            mitten,
            tree,
            shovel: shovel.id().clone(),
            settings: settings_item.id().clone(),
            quit: quit.id().clone(),
        };
        this.sync(settings);
        Ok(this)
    }

    /// Updates check marks to match `settings`.
    pub fn sync(&self, settings: &Settings) {
        self.pause.set_checked(settings.paused);
        for (i, item) in &self.intensity {
            item.set_checked(*i == settings.intensity);
        }
        let nearest = WIND_PRESETS.iter().map(|(_, x)| *x).min_by(|a, b| {
            (a - settings.wind.x)
                .abs()
                .total_cmp(&(b - settings.wind.x).abs())
        });
        for (x, item) in &self.wind {
            item.set_checked(Some(*x) == nearest);
        }
        self.accumulate.set_checked(settings.accumulate);
        self.mitten.set_checked(settings.mitten);
        self.tree.set_checked(settings.show_tree);
    }

    /// Drains pending menu events.
    pub fn poll(&self, settings: &Settings) -> Vec<Action> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id();
            let mut s = settings.clone();
            if id == self.pause.id() {
                s.paused = !s.paused;
            } else if id == self.accumulate.id() {
                s.accumulate = !s.accumulate;
            } else if id == self.mitten.id() {
                s.mitten = !s.mitten;
            } else if id == self.tree.id() {
                s.show_tree = !s.show_tree;
            } else if let Some((i, _)) = self.intensity.iter().find(|(_, m)| m.id() == id) {
                s.intensity = *i;
            } else if let Some((x, _)) = self.wind.iter().find(|(_, m)| m.id() == id) {
                s.wind = Vec2::new(*x, 0.0);
            } else if id == &self.shovel {
                actions.push(Action::Shovel);
                continue;
            } else if id == &self.settings {
                actions.push(Action::OpenSettings);
                continue;
            } else if id == &self.quit {
                actions.push(Action::Quit);
                continue;
            } else {
                continue;
            }
            self.sync(&s);
            actions.push(Action::Settings(s));
        }
        actions
    }
}

/// Draws a six-armed snowflake as a template icon.
fn snowflake_icon(shade: u8) -> Icon {
    const N: usize = 44;
    let mut rgba = vec![0u8; N * N * 4];
    let c = Vec2::splat(N as f32 / 2.0);
    let mut segments = Vec::new();
    for k in 0..6 {
        let a = k as f32 * std::f32::consts::TAU / 6.0 - std::f32::consts::FRAC_PI_2;
        let dir = Vec2::new(a.cos(), a.sin());
        let tip = c + dir * 19.0;
        segments.push((c, tip));
        for (at, len) in [(0.5, 6.5), (0.78, 4.5)] {
            let base = c + dir * 19.0 * at;
            for side in [-1.0f32, 1.0] {
                let b = a + side * std::f32::consts::FRAC_PI_3;
                segments.push((base, base + Vec2::new(b.cos(), b.sin()) * len));
            }
        }
    }
    for y in 0..N {
        for x in 0..N {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let d = segments
                .iter()
                .map(|&(a, b)| distance_to_segment(p, a, b))
                .fold(f32::MAX, f32::min);
            let alpha = (2.2 - d).clamp(0.0, 1.0);
            let px = &mut rgba[(y * N + x) * 4..][..4];
            px.copy_from_slice(&[shade, shade, shade, (alpha * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, N as u32, N as u32).expect("valid icon")
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}
