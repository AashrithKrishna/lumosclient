// Lumos Controls: the rules, with no game type in them.
//
// Inserted into `crates/horizon-client-render/src/options_ui.rs` as `pub mod lumos_controls`
// (the options screens live in that crate, and Key Binds is one of them); `glue.rs` reads the
// options screen's settings and calls in here. It names nothing outside `std`, so it also
// compiles alone: `rustc --edition 2024 --test runtime.rs` runs the tests below.
//
// The mod keeps no keys of its own. Every key it shows is a native mod's `NativeMod::keys`
// entry, and every change is the `key.<id>.<name>` line of `horizon-mods.txt` that the mod's own
// page in Options > Mods writes.
use std::collections::BTreeMap;

pub const ID: &str = "lumos_controls";
pub const NAME: &str = "Lumos Controls";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "Every native mod's keys in Options > Controls > Key Binds, \
under the mod's name as Java shows mod keys: rebind them there, Escape unbinds, Reset goes back \
to the mod's default, and a key another binding also uses shows in red.";

/// The key name of no key.
pub const UNBOUND: &str = "key.keyboard.unknown";

/// Added to a key's index in its Reset button's act, so the button can be told from the key's
/// own button (`Act::NativeKey(mod, key)`). No mod lists this many keys.
pub const RESET: usize = 1 << 24;

/// Each line of the list is as tall as Key Binds' rows.
pub const LINE: f32 = 20.0;

/// One enabled native mod with keys: its row in Options > Mods (`mods.installed`), its id, its
/// title (`Name 1.0`) and its keys (name, label, default).
pub struct ModKeys<'a> {
    pub index: usize,
    pub id: &'a str,
    pub title: &'a str,
    pub keys: &'a [[String; 3]],
}

/// A line of the mods' part of Key Binds.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// A mod's name, as Key Binds' category headers.
    Header(String),
    /// One key: `Act::NativeKey(index, key)` rebinds it, as on the mod's page.
    Key {
        index: usize,
        key: usize,
        label: String,
        bound: String,
        /// Bound to its default (Reset greyed out).
        default: bool,
        /// Another binding (vanilla's, Horizon's, this or another mod's) has the same key.
        clash: bool,
    },
}

/// A mod's title as a header: `Name 1.0` without the version, as Java names a mod's keys.
pub fn header(title: &str) -> String {
    let title = title.trim();
    match title.rsplit_once(' ') {
        Some((name, version))
            if !name.trim().is_empty()
                && version.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit()) =>
        {
            name.trim().to_owned()
        }
        _ => title.to_owned(),
    }
}

/// The key the player chose for `name` of mod `id` (`key.<id>.<name>`), else `default`: what
/// `native_mod_key` hands the mod.
pub fn bound<'a>(
    chosen: &'a BTreeMap<String, BTreeMap<String, String>>,
    id: &str,
    name: &str,
    default: &'a str,
) -> &'a str {
    chosen
        .get(id)
        .and_then(|keys| keys.get(name))
        .map_or(default, String::as_str)
}

/// The lines for `mods` (in their Options > Mods order), with what each key is bound to now and
/// whether it clashes with `others` (every vanilla and Horizon binding's key) or with another
/// mod key. `query` is Key Binds' search: a mod whose name has it shows all its keys, else only
/// the keys whose label has it; a mod with none left shows nothing.
pub fn lines(
    mods: &[ModKeys<'_>],
    chosen: &BTreeMap<String, BTreeMap<String, String>>,
    others: &[&str],
    query: &str,
) -> Vec<Line> {
    // Every mod key, searched or not, for the clashes.
    let all: Vec<(usize, usize, &str)> = mods
        .iter()
        .flat_map(|m| {
            m.keys
                .iter()
                .enumerate()
                .map(move |(k, [name, _, default])| (m.index, k, bound(chosen, m.id, name, default)))
        })
        .collect();
    let query = query.trim().to_lowercase();
    let mut out = Vec::new();
    for m in mods {
        let title = header(m.title);
        let whole = query.is_empty() || title.to_lowercase().contains(&query);
        let mut keys = Vec::new();
        for (k, [name, label, default]) in m.keys.iter().enumerate() {
            if !whole && !label.to_lowercase().contains(&query) {
                continue;
            }
            let key = bound(chosen, m.id, name, default);
            let clash = key != UNBOUND
                && !key.is_empty()
                && (others.contains(&key)
                    || all
                        .iter()
                        .any(|&(i, j, other)| (i, j) != (m.index, k) && other == key));
            keys.push(Line::Key {
                index: m.index,
                key: k,
                label: label.clone(),
                bound: key.to_owned(),
                default: key == default.as_str(),
                clash,
            });
        }
        if !keys.is_empty() {
            out.push(Line::Header(title));
            out.extend(keys);
        }
    }
    out
}

/// How much taller the list is with `lines`.
pub fn height(lines: &[Line]) -> f32 {
    lines.len() as f32 * LINE
}

/// A Reset button's act index for key `key`, and back.
pub fn reset_act(key: usize) -> usize {
    key + RESET
}

/// The key a Reset button's act index stands for; None for a key's own button.
pub fn reset_of(act: usize) -> Option<usize> {
    act.checked_sub(RESET)
}

/// Where a key row's parts sit, from the screen's width, as Key Binds lays out its own rows:
/// (label x, key button x, Reset x).
pub fn columns(width: f32) -> (f32, f32, f32) {
    let cx = (width / 2.0).floor();
    let row_left = cx - 170.0;
    let scrollbar_x = row_left + 340.0 + 6.0 + 2.0;
    let reset_x = scrollbar_x - 50.0 - 10.0;
    let change_x = reset_x - 5.0 - 75.0;
    (row_left + 2.0, change_x, reset_x)
}

/// Where the list's last row ends on screen: Key Binds counts its content as the rows' height
/// plus 2 on each side, from `top`, scrolled by `scroll`.
pub fn list_end(top: f32, content: f32, scroll: f32) -> f32 {
    top + content - scroll - 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[(&str, &str, &str)]) -> Vec<[String; 3]> {
        list.iter()
            .map(|(n, l, d)| [n.to_string(), l.to_string(), d.to_string()])
            .collect()
    }

    fn chosen(list: &[(&str, &str, &str)]) -> BTreeMap<String, BTreeMap<String, String>> {
        let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (id, name, key) in list {
            out.entry(id.to_string())
                .or_default()
                .insert(name.to_string(), key.to_string());
        }
        out
    }

    #[test]
    fn a_header_is_the_mods_name_without_its_version() {
        assert_eq!(header("Fullbright Key 1.0.0"), "Fullbright Key");
        assert_eq!(header("LumosHUD v2.1"), "LumosHUD");
        assert_eq!(header("Map"), "Map");
        assert_eq!(header("Better F3"), "Better F3");
        assert_eq!(header("1.0"), "1.0");
    }

    #[test]
    fn a_key_is_the_players_choice_else_its_default() {
        let c = chosen(&[("lumos_map", "waypoint", "key.keyboard.n")]);
        assert_eq!(bound(&c, "lumos_map", "waypoint", "key.keyboard.b"), "key.keyboard.n");
        assert_eq!(bound(&c, "lumos_map", "world_map", "key.keyboard.m"), "key.keyboard.m");
        assert_eq!(bound(&c, "lumos_hud", "editor", "key.keyboard.right.shift"), "key.keyboard.right.shift");
    }

    #[test]
    fn each_mod_gets_a_header_then_its_keys_in_order() {
        let map = keys(&[
            ("waypoint", "Add Waypoint", "key.keyboard.b"),
            ("world_map", "World Map", "key.keyboard.m"),
        ]);
        let fb = keys(&[("toggle", "Toggle Fullbright", "key.keyboard.g")]);
        let mods = [
            ModKeys { index: 3, id: "lumos_map", title: "Map 1.0.0", keys: &map },
            ModKeys { index: 5, id: "lumos_fullbright_key", title: "Fullbright Key 1.0.0", keys: &fb },
        ];
        let c = chosen(&[("lumos_map", "world_map", "key.keyboard.j")]);
        let out = lines(&mods, &c, &[], "");
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], Line::Header("Map".into()));
        assert_eq!(
            out[2],
            Line::Key {
                index: 3,
                key: 1,
                label: "World Map".into(),
                bound: "key.keyboard.j".into(),
                default: false,
                clash: false,
            }
        );
        assert_eq!(out[3], Line::Header("Fullbright Key".into()));
        assert!(matches!(out[4], Line::Key { index: 5, key: 0, default: true, .. }));
        assert_eq!(height(&out), 100.0);
    }

    #[test]
    fn a_key_another_binding_uses_clashes_and_an_unbound_one_never_does() {
        let a = keys(&[("one", "One", "key.keyboard.g"), ("two", "Two", "key.keyboard.unknown")]);
        let b = keys(&[("three", "Three", "key.keyboard.h"), ("four", "Four", "key.keyboard.unknown")]);
        let mods = [
            ModKeys { index: 0, id: "a", title: "A", keys: &a },
            ModKeys { index: 1, id: "b", title: "B", keys: &b },
        ];
        let clashes = |c: &BTreeMap<String, BTreeMap<String, String>>, others: &[&str]| -> Vec<bool> {
            lines(&mods, c, others, "")
                .into_iter()
                .filter_map(|l| match l {
                    Line::Key { clash, .. } => Some(clash),
                    Line::Header(_) => None,
                })
                .collect()
        };
        // Nothing shared: no clash; two unbound keys never clash.
        assert_eq!(clashes(&chosen(&[]), &["key.keyboard.w"]), [false; 4]);
        // With vanilla's (or Horizon's) key.
        assert_eq!(clashes(&chosen(&[]), &["key.keyboard.g"]), [true, false, false, false]);
        // Between two mods: both are red.
        let c = chosen(&[("b", "three", "key.keyboard.g")]);
        assert_eq!(clashes(&c, &[]), [true, false, true, false]);
        // Within one mod.
        let c = chosen(&[("a", "two", "key.keyboard.g")]);
        assert_eq!(clashes(&c, &[]), [true, true, false, false]);
    }

    #[test]
    fn a_search_keeps_a_mod_by_name_or_its_keys_by_label() {
        let map = keys(&[
            ("waypoint", "Add Waypoint", "key.keyboard.b"),
            ("world_map", "World Map", "key.keyboard.m"),
        ]);
        let fl = keys(&[("look", "Free Look", "key.keyboard.left.alt")]);
        let mods = [
            ModKeys { index: 0, id: "lumos_map", title: "Map 1.0.0", keys: &map },
            ModKeys { index: 1, id: "lumos_freelook", title: "Freelook 1.0.0", keys: &fl },
        ];
        let c = chosen(&[]);
        // The mod's name: all of its keys.
        assert_eq!(lines(&mods, &c, &[], "FREELOOK").len(), 2);
        // A key's label: that key, under its mod.
        let out = lines(&mods, &c, &[], "waypoint");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Line::Header("Map".into()));
        // "map" names the mod: both its keys.
        assert_eq!(lines(&mods, &c, &[], "map").len(), 3);
        assert!(lines(&mods, &c, &[], "jump").is_empty());
        // A clash is counted against keys the search hides too.
        let c = chosen(&[("lumos_freelook", "look", "key.keyboard.b")]);
        assert!(matches!(lines(&mods, &c, &[], "free")[1], Line::Key { clash: true, .. }));
    }

    #[test]
    fn reset_buttons_are_told_from_key_buttons() {
        assert_eq!(reset_of(reset_act(0)), Some(0));
        assert_eq!(reset_of(reset_act(7)), Some(7));
        assert_eq!(reset_of(3), None);
    }

    #[test]
    fn rows_sit_where_key_binds_puts_its_own() {
        // A 427-wide GUI (854x480 at scale 2): KeyBindsList's 340-wide rows around the middle.
        let (label, change, reset) = columns(427.0);
        assert_eq!(label, 45.0);
        assert_eq!(reset, 331.0);
        assert_eq!(change, 251.0);
        // The list's end: Key Binds' `content = y + scroll - top + 2`.
        let (top, scroll, y) = (38.0, 15.0, 200.0);
        let content = y + scroll - top + 2.0;
        assert_eq!(list_end(top, content, scroll), y);
    }
}
