//! User-facing strings in Russian and English.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Ru,
    En,
}

fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        let forced = std::env::var("FREEFLAKE_LANG").ok();
        let locale = forced.or_else(sys_locale::get_locale).unwrap_or_default();
        if locale.to_lowercase().starts_with("ru") {
            Lang::Ru
        } else {
            Lang::En
        }
    })
}

/// Declares string keys with their translations.
macro_rules! strings {
    ($($key:ident => $ru:literal, $en:literal;)*) => {
        #[allow(non_camel_case_types, dead_code)]
        #[derive(Clone, Copy)]
        pub enum S { $($key,)* }

        pub fn t(s: S) -> &'static str {
            match (s, lang()) {
                $((S::$key, Lang::Ru) => $ru, (S::$key, Lang::En) => $en,)*
            }
        }
    };
}

strings! {
    Pause => "Пауза", "Pause";
    Snowfall => "Снегопад", "Snowfall";
    Light => "Лёгкий", "Light";
    Steady => "Ровный", "Steady";
    Blizzard => "Метель", "Blizzard";
    Wind => "Ветер", "Wind";
    WindLeftStrong => "Сильно влево", "Strong to the left";
    WindLeft => "Влево", "To the left";
    WindCalm => "Штиль", "Calm";
    WindRight => "Вправо", "To the right";
    WindRightStrong => "Сильно вправо", "Strong to the right";
    Accumulate => "Сугробы", "Snow piles";
    SnowOverWindows => "Снег поверх окон", "Snow over windows";
    Mitten => "Варежка (смахивать курсором)", "Mitten (brush with cursor)";
    Shovel => "Лопата: убрать весь снег", "Shovel: clear all snow";
    Tree => "Ёлка", "Christmas tree";
    Settings => "Настройки…", "Settings…";
    Quit => "Выход", "Quit";
    SettingsTitle => "настройки", "settings";
    WindHint => "Перетащите точку: в сторону, вверх или вниз. Двойной щелчок — штиль.", "Drag the dot: sideways, up or down. Double-click for calm.";
    MaxWindowPile => "Сугроб на окне, пт", "Pile on windows, pt";
    MaxGroundPile => "Сугроб внизу экрана, пт", "Pile at the bottom, pt";
    Melt => "Подтаивание", "Slow melting";
    SnowballHint => "Зажмите Option (Alt) и ведите по сугробу, чтобы скатать снежок. Отпустите — бросок. Поставьте шары друг на друга — получится снеговик.", "Hold Option (Alt) and drag along a pile to roll a snowball. Let go to throw it. Stack balls to make a snowman.";
    ShowTree => "Показывать ёлку на рабочем столе", "Show the tree on the desktop";
    TreeHint => "С 1 декабря под ёлкой каждый день появляется подарок. Открывайте и вешайте игрушки на ветки.", "From December 1 a gift appears under the tree every day. Open it and hang the ornaments.";
    Performance => "Производительность", "Performance";
    Fps => "Кадров в секунду", "Frames per second";
    LowPowerFps => "В режиме энергосбережения", "On battery saver";
    Autostart => "Запускать при входе в систему", "Start at login";
    About => "freeflake на GitHub", "freeflake on GitHub";
}
