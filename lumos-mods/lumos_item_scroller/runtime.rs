// Lumos Item Scroller: the rules (std only). Which keys make which move, which slots a click, a
// drag or a wheel turn moves and the clicks that move them, the rate they go out at, the stored
// recipes and the recipe view, the crafting and trading steps that wait for the server, sorting,
// and the settings, recipes and favourites as text. See PARITY.md for what Item Scroller does.
//
// Slot ids are Horizon's inventory screen ids: 0..9 the hotbar, 9..36 the rest of the player's
// inventory, 100..109 the crafting grid, 200 its result, 300..500 an open container, 500..505 the
// armour and off hand, 600.. a workstation's inputs and 650 its result (a merchant's trade).

use std::collections::{BTreeMap, BTreeSet};

pub const ID: &str = "lumos_item_scroller";
pub const NAME: &str = "Item Scroller";
pub const VERSION: &str = "2.0.0";
pub const DESCRIPTION: &str = "Item Scroller's inventory controls: the wheel moves one item, Shift a \
stack, Alt every matching stack, Alt+Shift everything; Ctrl+click moves one item, Alt+click every \
matching stack, Shift+drag stacks, Shift+right-drag all but one, Q+drag drops, W/S+click move up or \
down; hold A in a crafting screen for the stored recipes (middle-click the result to store), \
Ctrl+C crafts everything, Ctrl+Alt+C mass crafts; villager favourites (middle-click) and \
right-click to trade everything.";

/// The settings, one file in the mod's data folder.
pub const CONFIG_KEY: &str = "config";
/// Favourite trades, every world's.
pub const FAVOURITES_KEY: &str = "favourites";
/// The trade list's scroll position per villager.
pub const TRADE_SCROLL_KEY: &str = "trade-scroll";

/// GLFW modifier bits.
pub const SHIFT: u32 = 1;
pub const CTRL: u32 = 2;
pub const ALT: u32 = 4;
/// The bits the hotkeys look at (Super is left alone).
pub const MODS: u32 = SHIFT | CTRL | ALT;

/// Hotkeys: name, label, default key. Item Scroller's defaults; the modifiers each needs are in
/// `KEY_MODS` and fixed, the key itself is rebindable. The bindings are read in keys.rs.
pub const HOTKEYS: [(&str, &str, &str); 17] = [
    ("drop_key", "Drag drop (hold, click/drag: one; Shift: stacks; Shift+right: all but one)", "key.keyboard.q"),
    ("move_up", "Move up (hold, click/drag; Shift: stacks; Alt: matching; right: all but one)", "key.keyboard.w"),
    ("move_down", "Move down (hold, click/drag; Shift: stacks; Alt: matching; right: all but one)", "key.keyboard.s"),
    ("move_everything_old", "Shift + this + click: move everything (older default)", "key.keyboard.space"),
    ("drop_all", "Ctrl+Shift + this: drop all matching", "key.keyboard.q"),
    ("recipe_view", "Hold: recipe view (crafting screens)", "key.keyboard.a"),
    ("store_recipe", "With the recipe view: over the crafting result, store", "key.mouse.middle"),
    ("craft_everything", "Ctrl + this: craft everything", "key.keyboard.c"),
    ("mass_craft", "Ctrl+Alt + this (held): mass craft", "key.keyboard.c"),
    ("mass_craft_toggle", "Mass craft without holding on/off", "key.keyboard.unknown"),
    ("throw_results", "Ctrl + this: throw crafted results", "key.keyboard.t"),
    ("move_results", "Ctrl + this: move crafted results", "key.keyboard.m"),
    ("trade_favorites", "Trade all favourite trades", "key.keyboard.unknown"),
    ("sort", "Sort the hovered inventory (when on)", "key.keyboard.r"),
    ("toggle_mod", "Item Scroller on/off", "key.keyboard.unknown"),
    ("slot_debug", "Ctrl+Alt+Shift + this: log the hovered slot", "key.keyboard.i"),
    ("villager_global", "Held with middle-click: global favourite", "key.keyboard.left.shift"),
];

/// The modifiers a key hotkey needs, exactly (Item Scroller's keybinds take no extra keys).
pub fn key_mods(id: &str) -> u32 {
    match id {
        "drop_all" => CTRL | SHIFT,
        "craft_everything" | "throw_results" | "move_results" => CTRL,
        "mass_craft" => CTRL | ALT,
        "slot_debug" => CTRL | ALT | SHIFT,
        _ => 0,
    }
}

pub const GRID: usize = 100;
pub const CRAFT_RESULT: usize = 200;
pub const CONTAINER: usize = 300;
pub const PAYMENT: usize = 600;
pub const TRADE_RESULT: usize = 650;
/// Ticks a step waits for the server before it gives up.
pub const TIMEOUT: u32 = 20;

// ── clicks ──

/// `ClickType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pickup,
    QuickMove,
    Throw,
    QuickCraft,
}

/// One container click: the slot (None outside the window), the button and the click type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    pub slot: Option<usize>,
    pub button: i32,
    pub kind: Kind,
}

/// Clicks that go out together (a hand never stays full between two batches).
pub type Batch = Vec<Op>;

pub fn pickup(slot: usize) -> Op {
    Op { slot: Some(slot), button: 0, kind: Kind::Pickup }
}
/// A right click: one item from the hand into the slot (or half the slot into an empty hand).
pub fn pickup_one(slot: usize) -> Op {
    Op { slot: Some(slot), button: 1, kind: Kind::Pickup }
}
pub fn quick_move(slot: usize) -> Op {
    Op { slot: Some(slot), button: 0, kind: Kind::QuickMove }
}
/// THROW: one item.
pub fn throw_one(slot: usize) -> Op {
    Op { slot: Some(slot), button: 0, kind: Kind::Throw }
}
/// THROW with the control button: the whole stack.
pub fn throw_stack(slot: usize) -> Op {
    Op { slot: Some(slot), button: 1, kind: Kind::Throw }
}
/// A click outside the window: the hand's stack is dropped.
pub fn drop_hand() -> Op {
    Op { slot: None, button: 0, kind: Kind::Pickup }
}
/// A quick craft drag over `slots`: with the right button one item into each, with the left the
/// hand split evenly. A drag over one slot is a plain click on it (vanilla).
pub fn drag(slots: &[usize], right: bool) -> Batch {
    let kind = i32::from(right) << 2;
    let mut ops = vec![Op { slot: None, button: kind, kind: Kind::QuickCraft }];
    ops.extend(slots.iter().map(|&s| Op { slot: Some(s), button: kind | 1, kind: Kind::QuickCraft }));
    ops.push(Op { slot: None, button: kind | 2, kind: Kind::QuickCraft });
    ops
}

// ── slots ──

/// A stack as the rules see it. `class` is the same for stacks of the same item with the same
/// components; `plain` is a stack with no components of its own and no damage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    pub item: String,
    pub count: u32,
    pub max: u32,
    pub class: u32,
    pub plain: bool,
}

pub fn is_player(slot: usize) -> bool {
    slot < 36
}
pub fn is_grid(slot: usize) -> bool {
    (GRID..GRID + 9).contains(&slot)
}
pub fn is_container(slot: usize) -> bool {
    (CONTAINER..500).contains(&slot)
}
/// A slot the moves work on: the player's, a container's or the crafting grid's.
pub fn movable(slot: usize) -> bool {
    is_player(slot) || is_container(slot) || is_grid(slot)
}

/// The slots `slot` belongs with: an open container's, the grid's, or the player's (the hotbar and
/// the rest apart when nothing else is open, as vanilla's quick move moves between them).
pub fn same_side(slot: usize, slots: &[usize]) -> Vec<usize> {
    let container = slots.iter().any(|&s| is_container(s));
    let keep = |f: &dyn Fn(usize) -> bool| slots.iter().copied().filter(|&s| f(s)).collect();
    match slot {
        0..9 if !container => keep(&|s| s < 9),
        9..36 if !container => keep(&|s| (9..36).contains(&s)),
        0..36 => keep(&|s| s < 36),
        _ if is_grid(slot) => keep(&|s| is_grid(s)),
        _ if is_container(slot) => keep(&|s| is_container(s)),
        _ => vec![slot],
    }
}

/// The slots across from `slot`: the container's or a crafting table's grid face the player's
/// inventory (the hotbar first, from the right), the inventory faces them; with neither, the hotbar
/// and the rest trade.
pub fn other_side(slot: usize, slots: &[usize], table: bool) -> Vec<usize> {
    let own: Vec<usize> =
        slots.iter().copied().filter(|&s| is_container(s) || (table && is_grid(s))).collect();
    let player = || (0..9).rev().chain((9..36).rev());
    match slot {
        _ if is_grid(slot) || is_container(slot) => player().collect(),
        0..36 if !own.is_empty() => own,
        0..9 => (9..36).collect(),
        9..36 => (0..9).collect(),
        _ => Vec::new(),
    }
}

/// The slots among `among` holding the same item with the same components as `like`.
pub fn matching(like: &Stack, among: &[usize], stack: &dyn Fn(usize) -> Option<Stack>) -> Vec<usize> {
    among.iter().copied().filter(|&s| stack(s).is_some_and(|t| t.class == like.class)).collect()
}

/// Item Scroller's stack moves, as shift-clicks. `to_other`: from `slot`'s side to the other one,
/// the side's stacks from the last slot back and `slot` itself last; else from the other side into
/// `slot`'s. `matching`: only stacks like `slot`'s; `first`: one stack only. `skip`: slots never
/// moved (result slots).
pub struct StackMove<'a> {
    pub slot: usize,
    pub slots: &'a [usize],
    pub table: bool,
    pub matching: bool,
    pub to_other: bool,
    pub first: bool,
}
pub fn move_stacks(m: &StackMove<'_>, stack: &dyn Fn(usize) -> Option<Stack>, skip: &dyn Fn(usize) -> bool) -> Vec<Batch> {
    let Some(like) = stack(m.slot) else {
        return Vec::new();
    };
    let mut pool: Vec<usize> = if m.to_other {
        let mut side = same_side(m.slot, m.slots);
        side.reverse();
        side
    } else {
        other_side(m.slot, m.slots, m.table)
    };
    pool.retain(|&s| s != m.slot && !skip(s));
    let mut out: Vec<Batch> = pool
        .into_iter()
        .filter(|&s| stack(s).is_some_and(|t| !m.matching || t.class == like.class))
        .map(|s| vec![quick_move(s)])
        .collect();
    if m.first {
        out.truncate(1);
    }
    if m.to_other && (out.is_empty() || !m.first) {
        out.push(vec![quick_move(m.slot)]);
    }
    out
}

/// Every stack of `slot`'s inventory (the player's whole inventory, or the container) holding the
/// same as `like`, thrown.
pub fn drop_matching(like: &Stack, slot: usize, slots: &[usize], stack: &dyn Fn(usize) -> Option<Stack>) -> Vec<Batch> {
    let side: Vec<usize> = if is_player(slot) {
        slots.iter().copied().filter(|&s| is_player(s)).collect()
    } else {
        same_side(slot, slots)
    };
    matching(like, &side, stack).into_iter().map(|s| vec![throw_stack(s)]).collect()
}

/// One item of `from` to `to` (an empty slot or one holding the same, with room): the stack
/// picked up, one put down, the rest put back.
pub fn move_one(from: usize, from_count: u32, to: usize) -> Batch {
    let mut b = vec![pickup(from)];
    b.extend(drag(&[to], true));
    if from_count > 1 {
        b.push(pickup(from));
    }
    b
}

/// The first of `among` that takes one more of `like`: a stack of it with room, else an empty slot.
pub fn first_with_room(like: &Stack, among: &[usize], stack: &dyn Fn(usize) -> Option<Stack>) -> Option<usize> {
    among
        .iter()
        .copied()
        .find(|&s| stack(s).is_some_and(|t| t.class == like.class && t.count < t.max))
        .or_else(|| among.iter().copied().find(|&s| stack(s).is_none()))
}

/// Where `amount` of `like` goes among `among`, in order (`among`'s own order: stacks of it with
/// room first unless `in_order`), and how much fits.
pub fn spread(like: &Stack, amount: u32, among: &[usize], in_order: bool, stack: &dyn Fn(usize) -> Option<Stack>) -> (Vec<usize>, u32) {
    let room = |s: usize| match stack(s) {
        Some(t) if t.class == like.class => t.max.saturating_sub(t.count),
        Some(_) => 0,
        None => like.max,
    };
    let order: Vec<usize> = if in_order {
        among.to_vec()
    } else {
        let partial = among.iter().copied().filter(|&s| stack(s).is_some());
        partial.chain(among.iter().copied().filter(|&s| stack(s).is_none())).collect()
    };
    let mut left = amount;
    let mut targets = Vec::new();
    for s in order {
        if left == 0 {
            break;
        }
        let r = room(s);
        if r > 0 {
            targets.push(s);
            left -= r.min(left);
        }
    }
    (targets, amount - left)
}

/// The stack of `from` (`count`) picked up, `keep` items put back (0 or 1), the hand put down on
/// `targets` in turn, and what's left back in `from`.
pub fn carry(from: usize, keep: u32, targets: &[usize], placed: u32, count: u32) -> Batch {
    let mut b = vec![pickup(from)];
    if keep > 0 {
        b.push(pickup_one(from));
    }
    b.extend(targets.iter().map(|&t| pickup(t)));
    if placed + keep < count {
        b.push(pickup(from));
    }
    b
}

/// All but one item of `slot` to the other side (Item Scroller's leave-one move).
pub fn leave_one(slot: usize, slots: &[usize], table: bool, stack: &dyn Fn(usize) -> Option<Stack>) -> Option<Batch> {
    let st = stack(slot)?;
    if st.count < 2 {
        return None;
    }
    let other = other_side(slot, slots, table);
    let (targets, placed) = spread(&st, st.count - 1, &other, false, stack);
    if targets.is_empty() {
        return None;
    }
    Some(carry(slot, 1, &targets, placed, st.count))
}

/// The drops of Item Scroller's drop drag.
pub fn drop_leave_one(slot: usize, count: u32) -> Option<Batch> {
    (count > 1).then(|| vec![pickup(slot), pickup_one(slot), drop_hand()])
}

// ── up and down (W / S) ──

/// The slots strictly above (`up`) or below `slot` on the screen that take `slot`'s stack (empty,
/// or the same with room), the furthest first. `y` is a slot's height on the screen.
pub fn vertical_targets(
    slot: usize,
    among: &[usize],
    up: bool,
    y: &dyn Fn(usize) -> f32,
    stack: &dyn Fn(usize) -> Option<Stack>,
) -> Vec<usize> {
    let Some(st) = stack(slot) else {
        return Vec::new();
    };
    let y0 = y(slot);
    let mut v: Vec<usize> = among
        .iter()
        .copied()
        .filter(|&s| s != slot && y(s) != y0 && (y(s) < y0) == up)
        .filter(|&s| match stack(s) {
            None => true,
            Some(t) => t.class == st.class && t.count < t.max,
        })
        .collect();
    v.sort_by(|&a, &b| {
        let (ya, yb) = (y(a), y(b));
        let o = if up { ya.total_cmp(&yb) } else { yb.total_cmp(&ya) };
        o.then(a.cmp(&b))
    });
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAmount {
    One,
    LeaveOne,
    Stacks,
    Matching,
}

/// One up/down move of `slot` (`Matching` is a `Stacks` move of each matching stack, made by the
/// caller).
pub fn vertical(
    slot: usize,
    among: &[usize],
    up: bool,
    amount: VAmount,
    y: &dyn Fn(usize) -> f32,
    stack: &dyn Fn(usize) -> Option<Stack>,
) -> Option<Batch> {
    let st = stack(slot)?;
    let targets = vertical_targets(slot, among, up, y, stack);
    match amount {
        VAmount::One => Some(move_one(slot, st.count, *targets.first()?)),
        VAmount::Stacks | VAmount::Matching | VAmount::LeaveOne => {
            let keep = u32::from(amount == VAmount::LeaveOne);
            if st.count <= keep {
                return None;
            }
            let (t, placed) = spread(&st, st.count - keep, &targets, true, stack);
            (!t.is_empty()).then(|| carry(slot, keep, &t, placed, st.count))
        }
    }
}

/// The matching stacks a vertical "matching" move takes, the ones nearest the far end last.
pub fn vertical_matching(slot: usize, slots: &[usize], up: bool, y: &dyn Fn(usize) -> f32, stack: &dyn Fn(usize) -> Option<Stack>) -> Vec<usize> {
    let Some(like) = stack(slot) else {
        return Vec::new();
    };
    let mut v = matching(&like, &same_side(slot, slots), stack);
    v.sort_by(|&a, &b| if up { y(a).total_cmp(&y(b)) } else { y(b).total_cmp(&y(a)) });
    v
}

// ── what a click is ──

/// The keys held besides the modifiers, by their bindings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub drop: bool,
    pub up: bool,
    pub down: bool,
    pub space: bool,
}

/// Item Scroller's click and drag moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    MoveOne,
    LeaveOne,
    Stacks,
    Matching,
    Everything,
    DropOne,
    DropStacks,
    DropLeaveOne,
    Vertical { up: bool, amount: VAmount },
}
impl Action {
    /// A move that goes on over the slots dragged across (all but "move everything").
    pub fn drags(self) -> bool {
        self != Action::Everything
    }
    /// A move that takes one item, done once per slot per drag (Item Scroller's rule).
    pub fn once_per_slot(self) -> bool {
        matches!(self, Action::MoveOne | Action::DropOne | Action::Vertical { amount: VAmount::One, .. })
    }
}

/// The move a press of `button` (0 left, 1 right) with `mods` and the `held` keys makes, by Item
/// Scroller's default bindings (modifiers matched exactly).
pub fn click_action(button: i32, mods: u32, held: Held) -> Option<Action> {
    let mods = mods & MODS;
    let vert = |up: bool| -> Option<Action> {
        let amount = match (button, mods) {
            (0, 0) => VAmount::One,
            (0, SHIFT) => VAmount::Stacks,
            (0, ALT) => VAmount::Matching,
            (1, 0) => VAmount::LeaveOne,
            _ => return None,
        };
        Some(Action::Vertical { up, amount })
    };
    if held.drop {
        return match (button, mods) {
            (0, 0) => Some(Action::DropOne),
            (0, SHIFT) => Some(Action::DropStacks),
            (1, SHIFT) => Some(Action::DropLeaveOne),
            _ => None,
        };
    }
    if held.up != held.down {
        return vert(held.up);
    }
    match (button, mods) {
        (0, m) if m == SHIFT | ALT => Some(Action::Everything),
        (0, SHIFT) if held.space => Some(Action::Everything),
        (0, CTRL) => Some(Action::MoveOne),
        (0, ALT) => Some(Action::Matching),
        (0, SHIFT) => Some(Action::Stacks),
        (1, SHIFT) => Some(Action::LeaveOne),
        _ => None,
    }
}

/// The Shift+hover move's next slot: a hovered slot on the clicked slot's side not moved yet (so a
/// stack never goes back and forth).
pub fn hover_pick(hovered: Option<usize>, side: &[usize], visited: &[usize]) -> Option<usize> {
    hovered.filter(|s| movable(*s) && side.contains(s) && !visited.contains(s))
}

/// The points a drag from `from` to `to` passes over, `step` apart, `to` included (Item Scroller
/// walks the line so a fast drag misses no slot).
pub fn line_points(from: [f32; 2], to: [f32; 2], step: f32) -> Vec<[f32; 2]> {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let n = ((dx.abs().max(dy.abs()) / step.max(0.5)).ceil() as usize).clamp(1, 256);
    (1..=n).map(|i| {
        let t = i as f32 / n as f32;
        [from[0] + dx * t, from[1] + dy * t]
    }).collect()
}

// ── the wheel ──

/// How much a wheel turn moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Amount {
    One,
    Stack,
    Matching,
    Everything,
}

/// Item Scroller's wheel modifiers: Shift a stack, Alt every matching stack, Alt+Shift everything;
/// anything else is one item.
pub fn amount(modifiers: u32) -> Amount {
    match modifiers & MODS {
        m if m == SHIFT | ALT => Amount::Everything,
        ALT => Amount::Matching,
        SHIFT => Amount::Stack,
        _ => Amount::One,
    }
}

/// Whether a wheel turn of `dy` (up is positive) moves items to the other inventory. Up does; with
/// the slot-position option, up moves toward the other inventory when that is above (`above`).
pub fn moves_out(dy: f64, amount: Amount, cfg: &Config, above: bool) -> bool {
    let up = dy > 0.0;
    let mut out = if cfg.position_aware { above == up } else { up };
    let reverse = if amount == Amount::One { cfg.reverse_single } else { cfg.reverse_stacks };
    if reverse {
        out = !out;
    }
    out
}

/// Some slot of another inventory is above `slot` on the screen.
pub fn inventory_above(slot: usize, slots: &[usize], y: &dyn Fn(usize) -> f32) -> bool {
    let side = same_side(slot, slots);
    slots.iter().any(|&s| !side.contains(&s) && y(s) < y(slot))
}

/// Wheel steps from scroll deltas, a trackpad's fractions kept until they make a whole step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wheel {
    pub rest: f64,
}
impl Wheel {
    pub fn steps(&mut self, dy: f64) -> i32 {
        if self.rest != 0.0 && self.rest.signum() != dy.signum() {
            self.rest = 0.0;
        }
        self.rest += dy;
        let steps = self.rest.trunc();
        self.rest -= steps;
        steps as i32
    }
}

// ── rate ──

/// Clicks per tick, the choices on the settings page.
pub const RATES: [u32; 6] = [1, 2, 4, 8, 16, 32];

/// The clicks a tick may still send. A batch goes out whole; what it overspends comes off the
/// next ticks, so the average stays at the rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limiter {
    credit: i64,
}
impl Limiter {
    /// A new tick: `per_tick` more, never more than one tick's worth saved.
    pub fn tick(&mut self, per_tick: u32) {
        self.credit = (self.credit + i64::from(per_tick)).min(i64::from(per_tick));
    }
    pub fn ready(&self) -> bool {
        self.credit > 0
    }
    pub fn spend(&mut self, clicks: usize) {
        self.credit -= clicks as i64;
    }
}

// ── recipes ──

/// A crafting pattern as stored: `w` x `h` cells (item ids), and the result it showed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recipe {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Option<String>>,
    pub result: Option<(String, u32)>,
}
impl Recipe {
    /// The grid's (`n` x `n`) pattern trimmed to the cells in use; None for an empty grid.
    pub fn from_grid(n: usize, grid: &[Option<String>], result: Option<(String, u32)>) -> Option<Recipe> {
        let used = |r: usize, c: usize| grid.get(r * n + c).is_some_and(Option::is_some);
        let rows: Vec<usize> = (0..n).filter(|&r| (0..n).any(|c| used(r, c))).collect();
        let cols: Vec<usize> = (0..n).filter(|&c| (0..n).any(|r| used(r, c))).collect();
        let (&r0, &r1) = (rows.first()?, rows.last()?);
        let (&c0, &c1) = (cols.first()?, cols.last()?);
        let (w, h) = (c1 - c0 + 1, r1 - r0 + 1);
        let mut cells = Vec::with_capacity(w * h);
        for r in r0..=r1 {
            for c in c0..=c1 {
                cells.push(grid[r * n + c].clone());
            }
        }
        Some(Recipe { w, h, cells, result })
    }
    /// What each cell of an `n` x `n` grid should hold, the pattern at its top left.
    pub fn on_grid(&self, n: usize) -> Option<Vec<Option<String>>> {
        if self.w > n || self.h > n || self.cells.len() != self.w * self.h {
            return None;
        }
        let mut want = vec![None; n * n];
        for r in 0..self.h {
            for c in 0..self.w {
                want[r * n + c] = self.cells[r * self.w + c].clone();
            }
        }
        Some(want)
    }
    /// `w|h|a,b,-,c|result|count`.
    pub fn to_line(&self) -> String {
        let cells: Vec<&str> = self.cells.iter().map(|c| c.as_deref().unwrap_or("-")).collect();
        let (item, count) = match &self.result {
            Some((i, c)) => (i.as_str(), *c),
            None => ("-", 0),
        };
        format!("{}|{}|{}|{}|{}", self.w, self.h, cells.join(","), item, count)
    }
    pub fn from_line(line: &str) -> Option<Recipe> {
        let mut parts = line.split('|');
        let w: usize = parts.next()?.parse().ok()?;
        let h: usize = parts.next()?.parse().ok()?;
        let cells: Vec<Option<String>> =
            parts.next()?.split(',').map(|c| (c != "-" && !c.is_empty()).then(|| c.to_owned())).collect();
        let item = parts.next().unwrap_or("-");
        let count: u32 = parts.next().and_then(|c| c.parse().ok()).unwrap_or(0);
        if w == 0 || h == 0 || w > 3 || h > 3 || cells.len() != w * h || cells.iter().all(Option::is_none) {
            return None;
        }
        let result = (item != "-" && count > 0).then(|| (item.to_owned(), count));
        Some(Recipe { w, h, cells, result })
    }
    /// The item the recipe view shows for it.
    pub fn icon(&self) -> Option<(String, u32)> {
        self.result.clone().or_else(|| self.cells.iter().flatten().next().map(|c| (c.clone(), 1)))
    }
}

/// Recipes a page of the recipe view, and pages (Item Scroller: 18 and 8).
pub const PAGE: usize = 18;
pub const PAGES: usize = 8;
pub const MAX_RECIPES: usize = PAGE * PAGES;

/// The stored recipes of one world or server, and the selected one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Book {
    pub slots: Vec<Option<Recipe>>,
    pub selected: usize,
}
impl Book {
    pub fn get(&self, i: usize) -> Option<&Recipe> {
        self.slots.get(i).and_then(Option::as_ref)
    }
    pub fn selected_recipe(&self) -> Option<&Recipe> {
        self.get(self.selected)
    }
    pub fn set(&mut self, i: usize, r: Option<Recipe>) {
        if i >= MAX_RECIPES {
            return;
        }
        if self.slots.len() <= i {
            self.slots.resize(i + 1, None);
        }
        self.slots[i] = r;
        while self.slots.last().is_some_and(Option::is_none) {
            self.slots.pop();
        }
    }
    /// `selected=n`, then one line a slot: `index|recipe`.
    pub fn to_text(&self) -> String {
        let mut out = format!("selected={}\n", self.selected);
        for (i, r) in self.slots.iter().enumerate() {
            if let Some(r) = r {
                out.push_str(&format!("{i}|{}\n", r.to_line()));
            }
        }
        out
    }
    pub fn parse(text: &str) -> Book {
        let mut book = Book::default();
        for line in text.lines() {
            let line = line.trim();
            if let Some(n) = line.strip_prefix("selected=") {
                book.selected = n.parse::<usize>().unwrap_or(0).min(MAX_RECIPES - 1);
            } else if let Some((i, rest)) = line.split_once('|')
                && let Ok(i) = i.parse::<usize>()
                && i < MAX_RECIPES
                && let Some(r) = Recipe::from_line(rest)
            {
                book.set(i, Some(r));
            }
        }
        book
    }
}

/// The recipe view's keys while it is open: 1-9 pick a recipe, up/down the previous/next, left/
/// right half a page (with Shift a page). The new selection, or None.
pub fn recipe_nav(key: &str, shift: bool, cur: usize) -> Option<usize> {
    let jump = if shift { PAGE } else { PAGE / 2 };
    let last = MAX_RECIPES - 1;
    if let Some(d) = key.strip_prefix("key.keyboard.")
        && d.len() == 1
        && let Some(n) = d.chars().next().and_then(|c| c.to_digit(10))
        && n >= 1
    {
        return Some(n as usize - 1);
    }
    match key {
        "key.keyboard.up" if cur > 0 => Some(cur - 1),
        "key.keyboard.down" if cur < last => Some(cur + 1),
        "key.keyboard.left" if cur >= jump => Some(cur - jump),
        "key.keyboard.right" if cur + jump <= last => Some(cur + jump),
        _ => None,
    }
}

/// The wheel over the recipe view: down the next recipe, up the previous.
pub fn recipe_scroll(cur: usize, dy: f64) -> usize {
    if dy < 0.0 { (cur + 1).min(MAX_RECIPES - 1) } else { cur.saturating_sub(1) }
}

/// Where the recipe view goes: the space left of the window (`gui_left` wide, `height` high),
/// two columns of nine, each entry its number then its item, scaled down to fit; the selected
/// recipe's grid left of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewLayout {
    /// An item's size.
    pub cell: f32,
    /// The entries' left edge and top.
    pub x: f32,
    pub y: f32,
    /// An entry's width (its number and item) and height.
    pub col_w: f32,
    pub row_h: f32,
}
pub const VIEW_COLUMNS: usize = 2;
pub const VIEW_ROWS: usize = 9;
impl ViewLayout {
    pub fn new(gui_left: f32, height: f32) -> ViewLayout {
        let num = 14.0;
        let by_height = height / (VIEW_ROWS as f32 + 1.2) - 2.0;
        let by_width = (gui_left - VIEW_COLUMNS as f32 * (num + 4.0)) / (VIEW_COLUMNS as f32 + 3.0 + 0.8);
        let cell = by_height.min(by_width).clamp(6.0, 16.0);
        let col_w = cell + num + 4.0;
        let x = (gui_left - VIEW_COLUMNS as f32 * col_w - 2.0).max(0.0);
        ViewLayout { cell, x, y: cell + 4.0, col_w, row_h: cell + 2.0 }
    }
    /// Entry `i` of the page: its item's `[x, y, size, size]`.
    pub fn entry(&self, i: usize) -> [f32; 4] {
        let (col, row) = (i / VIEW_ROWS, i % VIEW_ROWS);
        [self.x + col as f32 * self.col_w + self.col_w - self.cell - 2.0, self.y + row as f32 * self.row_h, self.cell, self.cell]
    }
    /// The page entry at `at`.
    pub fn at(&self, at: [f32; 2]) -> Option<usize> {
        (0..PAGE).find(|&i| {
            let [x, y, w, h] = self.entry(i);
            at[0] >= x && at[0] < x + w && at[1] >= y && at[1] < y + h
        })
    }
    /// The selected recipe's grid: its top left and a cell's size.
    pub fn grid(&self) -> ([f32; 2], f32) {
        let size = (self.cell + 1.0).min(17.0);
        ([(self.x - 3.0 * size - 2.0).max(0.0), self.y + 3.0 * self.row_h], size)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMode {
    /// One item in each cell: one craft's worth.
    One,
    /// Everything there is, split evenly, up to a stack a cell.
    Max,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillError {
    /// The pattern is bigger than the grid.
    TooBig,
    /// The hand holds something.
    HandFull,
    /// Not enough of this item.
    Missing(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FillPlan {
    /// The batches only take wrong items out of the grid; plan again after them.
    pub clearing: bool,
    pub batches: Vec<Batch>,
}

/// The cells of an `n` x `n` grid holding what `recipe` doesn't put there, shift-clicked out.
pub fn clear_wrong(n: usize, grid: &[Option<Stack>], recipe: &Recipe) -> Vec<Batch> {
    let Some(want) = recipe.on_grid(n) else {
        return Vec::new();
    };
    (0..n * n)
        .filter(|&i| match (&grid.get(i).cloned().flatten(), &want[i]) {
            (Some(s), Some(w)) => s.item != *w,
            (Some(_), None) => true,
            _ => false,
        })
        .map(|i| vec![quick_move(GRID + i)])
        .collect()
}

/// Every cell of the grid, shift-clicked out.
pub fn clear_grid(grid: &[Option<Stack>]) -> Vec<Batch> {
    (0..grid.len()).filter(|&i| grid[i].is_some()).map(|i| vec![quick_move(GRID + i)]).collect()
}

/// The clicks that fill an `n` x `n` grid (`grid`, cell i is slot 100 + i) with `recipe` from the
/// player's inventory (`inventory`: slot, stack). Items that don't belong in a cell go back to the
/// inventory first. Only plain stacks are used, so the cells always stack.
pub fn plan_fill(
    n: usize,
    grid: &[Option<Stack>],
    recipe: &Recipe,
    inventory: &[(usize, Option<Stack>)],
    carried: bool,
    mode: FillMode,
) -> Result<FillPlan, FillError> {
    let want = recipe.on_grid(n).ok_or(FillError::TooBig)?;
    if carried {
        return Err(FillError::HandFull);
    }
    let wrong = clear_wrong(n, grid, recipe);
    if !wrong.is_empty() {
        return Ok(FillPlan { clearing: true, batches: wrong });
    }
    let items: BTreeSet<&String> = want.iter().flatten().collect();
    // Check everything first: nothing moves when one item is short.
    for item in &items {
        let empty = (0..n * n).filter(|&i| want[i].as_ref() == Some(*item) && grid[i].is_none()).count() as u32;
        let have: u32 = inventory.iter().filter_map(|(_, s)| s.as_ref()).filter(|s| s.plain && &s.item == *item).map(|s| s.count).sum();
        if have < empty {
            return Err(FillError::Missing((*item).clone()));
        }
    }
    let mut batches = Vec::new();
    for item in items {
        let cells: Vec<usize> = (0..n * n).filter(|&i| want[i].as_ref() == Some(item)).collect();
        let mut counts: Vec<u32> = cells.iter().map(|&i| grid[i].as_ref().map_or(0, |s| s.count)).collect();
        let mut sources: Vec<(usize, u32, u32)> = inventory
            .iter()
            .filter_map(|(slot, s)| s.as_ref().filter(|s| s.plain && &s.item == item).map(|s| (*slot, s.count, s.max)))
            .collect();
        // The main inventory before the hotbar, small stacks first.
        sources.sort_by_key(|&(slot, count, _)| (slot < 9, count, slot));
        for (slot, count, max) in sources {
            let mut order: Vec<usize> = (0..cells.len()).collect();
            order.sort_by_key(|&k| (counts[k], k));
            let (targets, right): (Vec<usize>, bool) = match mode {
                FillMode::One => (order.into_iter().filter(|&k| counts[k] == 0).collect(), true),
                FillMode::Max => (order.into_iter().filter(|&k| counts[k] < max).collect(), false),
            };
            let k = targets.len().min(count as usize);
            if k == 0 {
                break;
            }
            let targets = &targets[..k];
            let mut left = count;
            for &t in targets {
                let put = if right { 1 } else { (count / k as u32).min(max - counts[t]) };
                counts[t] += put;
                left -= put;
            }
            let mut b = vec![pickup(slot)];
            b.extend(drag(&targets.iter().map(|&t| GRID + cells[t]).collect::<Vec<_>>(), right));
            if left > 0 {
                b.push(pickup(slot));
            }
            batches.push(b);
        }
    }
    Ok(FillPlan { clearing: false, batches })
}

/// Every stack of the result `item` among `slots` (plain item ids), thrown (Ctrl+T) or, from the
/// player's inventory, shift-clicked (Ctrl+M).
pub fn throw_results(item: &str, slots: &[usize], stack: &dyn Fn(usize) -> Option<Stack>) -> Vec<Batch> {
    slots
        .iter()
        .copied()
        .filter(|&s| (is_player(s) || is_container(s)) && stack(s).is_some_and(|t| t.item == item))
        .map(|s| vec![throw_stack(s)])
        .collect()
}
pub fn move_results(item: &str, slots: &[usize], stack: &dyn Fn(usize) -> Option<Stack>) -> Vec<Batch> {
    slots
        .iter()
        .copied()
        .filter(|&s| is_player(s) && stack(s).is_some_and(|t| t.item == item))
        .map(|s| vec![quick_move(s)])
        .collect()
}

/// What a step needs to know of the crafting screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CraftView {
    pub size: usize,
    pub grid: Vec<Option<Stack>>,
    pub result: Option<Stack>,
    pub inventory: Vec<(usize, Option<Stack>)>,
    pub carried: Option<Stack>,
    /// The result slot takes plain and throw clicks (a server; Horizon's local world takes only
    /// shift-clicks there).
    pub result_clicks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Send these.
    Send(Vec<Batch>),
    /// Select merchant offer `index` (the select trade packet).
    Select(usize),
    /// Nothing this tick.
    Wait,
    /// Finished, and why.
    Done(String),
}

/// What happens to a craft's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// Shift-clicked: as many as the grid makes, to the inventory.
    Shift,
    /// One craft picked up and put in the inventory.
    One,
    /// One craft thrown.
    DropOne,
    /// Thrown with the control button: everything the grid makes.
    DropAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Fill,
    AwaitResult,
    AwaitTaken,
    Cleanup,
    Finished,
}

/// Crafting with a stored recipe: fill the grid, take the result, again, `rounds` times (0: until
/// the ingredients run out or it is stopped). Each step waits for the server's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct CraftJob {
    pub recipe: Recipe,
    pub fill: FillMode,
    pub take: Take,
    pub rounds: u32,
    /// What's left in the grid goes back at the end.
    pub cleanup: bool,
    /// Runs while the mass craft key is held (or its toggle is on).
    pub hold: bool,
    phase: Phase,
    wait: u32,
    pub crafted: u32,
    before: Vec<Option<Stack>>,
    why: String,
}
impl CraftJob {
    pub fn new(recipe: Recipe, fill: FillMode, take: Take, rounds: u32) -> Self {
        CraftJob {
            recipe,
            fill,
            take,
            rounds,
            cleanup: false,
            hold: false,
            phase: Phase::Fill,
            wait: 0,
            crafted: 0,
            before: Vec::new(),
            why: String::new(),
        }
    }
    /// Item Scroller's mass craft (held key): fill with stacks, take, again, while held.
    pub fn mass(recipe: Recipe, drop: bool) -> Self {
        let mut j = CraftJob::new(recipe, FillMode::Max, if drop { Take::DropAll } else { Take::Shift }, 0);
        j.hold = true;
        j
    }
    /// `craftEverything`: fill with stacks and shift-click until the ingredients run out.
    pub fn everything(recipe: Recipe) -> Self {
        CraftJob::new(recipe, FillMode::Max, Take::Shift, 0)
    }
    /// The key was let go: no more rounds.
    pub fn stop(&mut self) {
        if matches!(self.phase, Phase::Fill | Phase::AwaitResult) {
            self.finish("stopped");
        }
    }
    fn finish(&mut self, why: &str) {
        self.why = why.to_owned();
        self.phase = if self.cleanup { Phase::Cleanup } else { Phase::Finished };
    }
    fn result_matches(&self, r: &Stack) -> bool {
        self.recipe.result.as_ref().is_none_or(|(item, _)| *item == r.item)
    }
    fn take_ops(&self, v: &CraftView, r: &Stack) -> Option<Batch> {
        let take = if v.result_clicks { self.take } else { Take::Shift };
        Some(match take {
            Take::Shift => vec![quick_move(CRAFT_RESULT)],
            Take::DropOne => vec![throw_one(CRAFT_RESULT)],
            Take::DropAll => vec![throw_stack(CRAFT_RESULT)],
            Take::One => {
                let mut order: Vec<usize> = (9..36).chain(0..9).collect();
                order.retain(|s| v.inventory.iter().any(|(i, _)| i == s));
                let get = |s: usize| v.inventory.iter().find(|(i, _)| *i == s).and_then(|(_, t)| t.clone());
                let fits = |s: usize| get(s).is_some_and(|t| t.item == r.item && t.plain && t.count + r.count <= t.max);
                let t = order.iter().copied().find(|&s| fits(s)).or_else(|| order.iter().copied().find(|&s| get(s).is_none()))?;
                vec![pickup(CRAFT_RESULT), pickup(t)]
            }
        })
    }
    pub fn step(&mut self, v: &CraftView) -> Step {
        match self.phase {
            Phase::Fill => {
                if self.rounds > 0 && self.crafted >= self.rounds {
                    self.finish("done");
                    return self.step(v);
                }
                // The grid may already hold a full set (a click on the selected recipe filled it).
                if v.result.as_ref().is_some_and(|r| self.result_matches(r)) && self.crafted == 0 && self.fill == FillMode::One {
                    self.phase = Phase::AwaitResult;
                    return self.step(v);
                }
                match plan_fill(v.size, &v.grid, &self.recipe, &v.inventory, v.carried.is_some(), self.fill) {
                    Ok(plan) if plan.clearing => Step::Send(plan.batches),
                    Ok(plan) => {
                        self.phase = Phase::AwaitResult;
                        self.wait = 0;
                        Step::Send(plan.batches)
                    }
                    Err(FillError::HandFull) => {
                        self.wait += 1;
                        if self.wait > TIMEOUT {
                            self.phase = Phase::Finished;
                            return Step::Done("the hand is full".into());
                        }
                        Step::Wait
                    }
                    Err(FillError::TooBig) => {
                        self.phase = Phase::Finished;
                        Step::Done("the recipe doesn't fit this grid".into())
                    }
                    Err(FillError::Missing(item)) => {
                        // What's in the grid may still make one more.
                        if v.result.as_ref().is_some_and(|r| self.result_matches(r)) {
                            self.phase = Phase::AwaitResult;
                            return self.step(v);
                        }
                        let why = if self.crafted == 0 { format!("not enough {item}") } else { "out of ingredients".into() };
                        self.finish(&why);
                        self.step(v)
                    }
                }
            }
            Phase::AwaitResult => {
                if let Some(r) = v.result.as_ref().filter(|r| self.result_matches(r)) {
                    let Some(ops) = self.take_ops(v, r) else {
                        self.finish("no room for the result");
                        return self.step(v);
                    };
                    self.before = v.grid.clone();
                    self.phase = Phase::AwaitTaken;
                    self.wait = 0;
                    return Step::Send(vec![ops]);
                }
                self.wait += 1;
                if self.wait > TIMEOUT {
                    self.finish("the grid makes nothing");
                    return self.step(v);
                }
                Step::Wait
            }
            Phase::AwaitTaken => {
                if v.grid != self.before {
                    self.crafted += 1;
                    self.phase = Phase::Fill;
                    self.wait = 0;
                    if self.crafted >= 4096 {
                        self.finish("stopped");
                    }
                    return Step::Wait;
                }
                self.wait += 1;
                if self.wait > TIMEOUT {
                    self.finish("no room for the result");
                    return self.step(v);
                }
                Step::Wait
            }
            Phase::Cleanup => {
                self.phase = Phase::Finished;
                let back = clear_grid(&v.grid);
                if back.is_empty() { Step::Done(self.why.clone()) } else { Step::Send(back) }
            }
            Phase::Finished => Step::Done(self.why.clone()),
        }
    }
}

/// Right-click on the crafting result: one craft into the hand at a time, until the hand holds a
/// full stack or the grid makes no more (`enableRightClickCraftingOneStack`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CraftStack {
    last: u32,
    sent: bool,
    wait: u32,
}
impl CraftStack {
    pub fn step(&mut self, result: Option<&Stack>, carried: Option<&Stack>) -> Step {
        let held = carried.map_or(0, |c| c.count);
        if self.sent {
            if held > self.last {
                self.sent = false;
                self.last = held;
                self.wait = 0;
            } else {
                self.wait += 1;
                if self.wait > TIMEOUT {
                    return Step::Done("done".into());
                }
                return Step::Wait;
            }
        }
        let Some(r) = result else {
            return Step::Done("done".into());
        };
        if let Some(c) = carried
            && (c.item != r.item || c.class != r.class || c.count + r.count > c.max)
        {
            return Step::Done("done".into());
        }
        self.sent = true;
        self.last = held;
        Step::Send(vec![vec![pickup_one(CRAFT_RESULT)]])
    }
}

// ── trades ──

/// What a step needs to know of the merchant screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TradeView {
    /// The offer's result item and whether it is out of stock.
    pub offer_result: String,
    pub out_of_stock: bool,
    /// The result slot and the two payment slots.
    pub result: Option<Stack>,
    pub payment: Vec<Option<Stack>>,
    pub carried: bool,
}

/// Trading offer `index` as many times as the items allow: select it (the payment moves in),
/// shift-click the result (vanilla trades while the payment lasts), select it again.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeMax {
    pub index: usize,
    phase: u8,
    wait: u32,
    pub rounds: u32,
    before: (Option<Stack>, Vec<Option<Stack>>),
}
impl TradeMax {
    pub fn new(index: usize) -> Self {
        TradeMax { index, phase: 0, wait: 0, rounds: 0, before: (None, Vec::new()) }
    }
    pub fn step(&mut self, v: &TradeView) -> Step {
        const SELECT: u8 = 0;
        const AWAIT_RESULT: u8 = 1;
        const AWAIT_TAKEN: u8 = 2;
        const DONE: u8 = 3;
        match self.phase {
            SELECT => {
                if v.out_of_stock {
                    self.phase = DONE;
                    return Step::Done("out of stock".into());
                }
                if v.carried {
                    self.phase = DONE;
                    return Step::Done("the hand is full".into());
                }
                self.phase = AWAIT_RESULT;
                self.wait = 0;
                Step::Select(self.index)
            }
            AWAIT_RESULT => {
                if v.result.as_ref().is_some_and(|r| r.item == v.offer_result) {
                    self.before = (v.result.clone(), v.payment.clone());
                    self.phase = AWAIT_TAKEN;
                    self.wait = 0;
                    return Step::Send(vec![vec![quick_move(TRADE_RESULT)]]);
                }
                self.wait += 1;
                if self.wait > TIMEOUT {
                    self.phase = DONE;
                    return Step::Done(if self.rounds == 0 { "can't pay for it".into() } else { "out of items".into() });
                }
                Step::Wait
            }
            AWAIT_TAKEN => {
                if (v.result.clone(), v.payment.clone()) != self.before {
                    self.rounds += 1;
                    self.phase = if self.rounds >= 256 { DONE } else { SELECT };
                    return Step::Wait;
                }
                self.wait += 1;
                if self.wait > TIMEOUT {
                    self.phase = DONE;
                    return Step::Done("no room in the inventory".into());
                }
                Step::Wait
            }
            _ => Step::Done("done".into()),
        }
    }
}

/// One payment slot to fill: the slot, what it holds, how many the trade asks for, whether what it
/// holds is the right item (or nothing), and the inventory slots holding the right item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payment {
    pub slot: usize,
    pub held: u32,
    pub need: u32,
    pub fits: bool,
    pub sources: Vec<(usize, u32)>,
}

/// Item Scroller's wheel down over the trade result: one trade's payment moved in, item by item
/// (right clicks). None when it can't be paid (nothing moves then).
pub fn plan_payment(pay: &[Payment]) -> Option<Vec<Batch>> {
    let mut used: BTreeMap<usize, u32> = BTreeMap::new();
    let mut out = Vec::new();
    for p in pay {
        if !p.fits {
            return None;
        }
        let mut missing = p.need.saturating_sub(p.held);
        for &(src, count) in &p.sources {
            if missing == 0 {
                break;
            }
            let left = count - used.get(&src).copied().unwrap_or(0);
            let take = left.min(missing);
            if take == 0 {
                continue;
            }
            let mut b = vec![pickup(src)];
            b.extend((0..take).map(|_| pickup_one(p.slot)));
            if left > take {
                b.push(pickup(src));
            }
            out.push(b);
            *used.entry(src).or_default() += take;
            missing -= take;
        }
        if missing > 0 {
            return None;
        }
    }
    Some(out)
}

/// A trade's items, `cost a|cost b|result` (ids).
pub fn trade_items(cost_a: &str, cost_b: Option<&str>, result: &str) -> String {
    let clean = |s: &str| s.replace(['|', '\n'], "");
    format!("{}|{}|{}", clean(cost_a), clean(cost_b.unwrap_or("-")), clean(result))
}

/// A villager as the client can tell it again: its profession (or name) and its first two trades,
/// which never change (Horizon keeps no merchant UUID on the client).
pub fn villager_id(profession: &str, first_trades: &[String]) -> String {
    let mut s = profession.replace(['|', '\n'], "");
    for t in first_trades.iter().take(2) {
        s.push('/');
        s.push_str(&t.replace('|', ","));
    }
    s
}

/// Favourite trades: `v|<villager>|<items>` for one villager, `g|<items>` for every villager.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Favourites {
    pub keys: BTreeSet<String>,
}
impl Favourites {
    pub fn parse(text: &str) -> Favourites {
        let mut keys = BTreeSet::new();
        for l in text.lines().map(str::trim) {
            let n = l.split('|').count();
            if (l.starts_with("v|") && n == 5) || (l.starts_with("g|") && n == 4) {
                keys.insert(l.to_owned());
            } else if n == 4 {
                // The first version's per-profession key: kept as a global favourite.
                let (_, items) = l.split_once('|').unwrap_or_default();
                keys.insert(format!("g|{items}"));
            }
        }
        Favourites { keys }
    }
    pub fn to_text(&self) -> String {
        self.keys.iter().map(|k| format!("{k}\n")).collect()
    }
    pub fn key(villager: Option<&str>, items: &str) -> String {
        match villager {
            Some(v) => format!("v|{}|{items}", v.replace('|', ",")),
            None => format!("g|{items}"),
        }
    }
    /// Marks or unmarks it; true when it is a favourite now.
    pub fn toggle(&mut self, key: String) -> bool {
        if self.keys.remove(&key) {
            false
        } else {
            self.keys.insert(key);
            true
        }
    }
    /// The favourite offers (indices) of a villager whose offers' items are `offers`: its own when
    /// it has any, else the global ones (then `true`).
    pub fn of(&self, villager: &str, offers: &[String]) -> (Vec<usize>, bool) {
        let own: Vec<usize> = (0..offers.len()).filter(|&i| self.keys.contains(&Favourites::key(Some(villager), &offers[i]))).collect();
        if !own.is_empty() {
            return (own, false);
        }
        ((0..offers.len()).filter(|&i| self.keys.contains(&Favourites::key(None, &offers[i]))).collect(), true)
    }
}

/// The trade list's scroll position per villager, the last 256.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TradeScroll {
    pub list: Vec<(String, usize)>,
}
impl TradeScroll {
    pub fn parse(text: &str) -> TradeScroll {
        TradeScroll {
            list: text
                .lines()
                .filter_map(|l| l.rsplit_once('|'))
                .filter_map(|(k, v)| Some((k.to_owned(), v.trim().parse().ok()?)))
                .collect(),
        }
    }
    pub fn to_text(&self) -> String {
        self.list.iter().map(|(k, v)| format!("{k}|{v}\n")).collect()
    }
    pub fn get(&self, k: &str) -> Option<usize> {
        self.list.iter().find(|(x, _)| x == k).map(|(_, v)| *v)
    }
    pub fn set(&mut self, k: &str, v: usize) {
        self.list.retain(|(x, _)| x != k);
        self.list.push((k.to_owned(), v));
        if self.list.len() > 256 {
            self.list.remove(0);
        }
    }
}

/// `MerchantScreen`'s list shows 7 offers: the furthest it scrolls.
pub fn clamp_trade_scroll(scroll: usize, offers: usize) -> usize {
    scroll.min(offers.saturating_sub(7))
}

/// A file-name-safe form of a world or server name.
pub fn folder_name(s: &str) -> String {
    let out: String =
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).take(80).collect();
    if out.is_empty() { "_".into() } else { out }
}

// ── sorting ──

/// The clicks that sort `slots` (one inventory: slot, stack): stacks of the same thing merged,
/// then ordered by item id (stacks of one thing together, the biggest first), the empty slots at
/// the end. Each batch leaves the hand empty.
pub fn plan_sort(slots: &[(usize, Option<Stack>)]) -> Vec<Batch> {
    let mut state: Vec<Option<Stack>> = slots.iter().map(|(_, s)| s.clone()).collect();
    let id = |i: usize| slots[i].0;
    let n = state.len();
    let mut out = Vec::new();
    // Merge partial stacks into the first of their kind.
    for i in 0..n {
        for j in i + 1..n {
            let (Some(a), Some(b)) = (state[i].clone(), state[j].clone()) else {
                continue;
            };
            if a.class != b.class || a.count >= a.max {
                continue;
            }
            let moved = (a.max - a.count).min(b.count);
            let mut batch = vec![pickup(id(j)), pickup(id(i))];
            if moved < b.count {
                batch.push(pickup(id(j)));
            }
            out.push(batch);
            state[i].as_mut().unwrap().count += moved;
            if moved == b.count {
                state[j] = None;
            } else {
                state[j].as_mut().unwrap().count -= moved;
            }
        }
    }
    let key = |s: &Stack| (s.item.clone(), s.class, u32::MAX - s.count);
    let mut want: Vec<Stack> = state.iter().flatten().cloned().collect();
    want.sort_by_key(key);
    for (pos, w) in want.iter().enumerate() {
        if state[pos].as_ref().is_some_and(|s| key(s) == key(w)) {
            continue;
        }
        let Some(j) = (pos + 1..n).find(|&j| state[j].as_ref().is_some_and(|s| key(s) == key(w))) else {
            continue;
        };
        let mut batch = vec![pickup(id(j)), pickup(id(pos))];
        if state[pos].is_some() {
            batch.push(pickup(id(j)));
        }
        out.push(batch);
        state.swap(pos, j);
    }
    out
}

// ── settings ──

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// `toggleModOnOff`.
    pub enabled: bool,
    pub scroll_single: bool,
    pub scroll_stacks: bool,
    pub scroll_matching: bool,
    pub scroll_everything: bool,
    pub scroll_villager: bool,
    pub reverse_single: bool,
    pub reverse_stacks: bool,
    pub position_aware: bool,
    pub drag_stacks: bool,
    pub drag_one: bool,
    pub drag_matching: bool,
    pub drag_leave_one: bool,
    pub move_everything: bool,
    pub drag_drop: bool,
    pub ws_moves: bool,
    pub shift_place: bool,
    pub shift_drop: bool,
    pub drop_all: bool,
    pub crafting: bool,
    pub right_click_stack: bool,
    pub mass_craft_drops: bool,
    pub mass_craft_hold: bool,
    pub recipes_global: bool,
    pub villager: bool,
    pub highlight: bool,
    pub remember_scroll: bool,
    pub sort: bool,
    /// Index into `RATES`.
    pub rate: usize,
    /// Menu type ids (`minecraft:generic_9x3`, `inventory`) the mod leaves alone.
    pub blacklist: Vec<String>,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            scroll_single: true,
            scroll_stacks: true,
            scroll_matching: true,
            scroll_everything: true,
            scroll_villager: true,
            reverse_single: false,
            reverse_stacks: false,
            position_aware: false,
            drag_stacks: true,
            drag_one: true,
            drag_matching: true,
            drag_leave_one: true,
            move_everything: true,
            drag_drop: true,
            ws_moves: true,
            shift_place: true,
            shift_drop: true,
            drop_all: true,
            crafting: true,
            right_click_stack: true,
            mass_craft_drops: true,
            mass_craft_hold: false,
            recipes_global: false,
            villager: true,
            highlight: true,
            remember_scroll: true,
            sort: false,
            rate: 2,
            blacklist: Vec::new(),
        }
    }
}

/// The on/off settings: key in the file, label, and the field.
type Flag = (&'static str, &'static str, fn(&mut Config) -> &mut bool);
const FLAGS: [Flag; 28] = [
    ("enabled", "Item Scroller", |c| &mut c.enabled),
    ("scroll_single", "Wheel: one item", |c| &mut c.scroll_single),
    ("scroll_stacks", "Shift+wheel: a stack", |c| &mut c.scroll_stacks),
    ("scroll_matching", "Alt+wheel: matching stacks", |c| &mut c.scroll_matching),
    ("scroll_everything", "Alt+Shift+wheel: everything", |c| &mut c.scroll_everything),
    ("scroll_villager", "Wheel over the trade result", |c| &mut c.scroll_villager),
    ("reverse_single", "Reverse the wheel (one item)", |c| &mut c.reverse_single),
    ("reverse_stacks", "Reverse the wheel (stacks)", |c| &mut c.reverse_stacks),
    ("position_aware", "Wheel direction by slot position", |c| &mut c.position_aware),
    ("drag_stacks", "Shift+click/drag: stacks", |c| &mut c.drag_stacks),
    ("drag_one", "Ctrl+click/drag: one item", |c| &mut c.drag_one),
    ("drag_matching", "Alt+click/drag: matching stacks", |c| &mut c.drag_matching),
    ("drag_leave_one", "Shift+right-drag: all but one", |c| &mut c.drag_leave_one),
    ("move_everything", "Alt+Shift+click: everything", |c| &mut c.move_everything),
    ("drag_drop", "Drop key+click/drag: drop", |c| &mut c.drag_drop),
    ("ws_moves", "W/S+click/drag: up/down", |c| &mut c.ws_moves),
    ("shift_place", "Shift+click empty slot: bring matching", |c| &mut c.shift_place),
    ("shift_drop", "Shift+click outside: drop matching", |c| &mut c.shift_drop),
    ("drop_all", "Ctrl+Shift+Q: drop all matching", |c| &mut c.drop_all),
    ("crafting", "Crafting features (hold A)", |c| &mut c.crafting),
    ("right_click_stack", "Right-click result: craft a stack", |c| &mut c.right_click_stack),
    ("mass_craft_drops", "Mass craft drops the results", |c| &mut c.mass_craft_drops),
    ("mass_craft_hold", "Mass craft without holding", |c| &mut c.mass_craft_hold),
    ("recipes_global", "Recipes for every world", |c| &mut c.recipes_global),
    ("villager", "Villager favourites and trading", |c| &mut c.villager),
    ("highlight", "Highlight favourite trades", |c| &mut c.highlight),
    ("remember_scroll", "Remember the trade list position", |c| &mut c.remember_scroll),
    ("sort", "Sort key (R)", |c| &mut c.sort),
];

impl Config {
    pub fn per_tick(&self) -> u32 {
        RATES[self.rate.min(RATES.len() - 1)]
    }
    pub fn blacklisted(&self, menu: &str) -> bool {
        self.blacklist.iter().any(|b| b == menu)
    }
    pub fn serialize(&self) -> String {
        let mut c = self.clone();
        let mut out = String::new();
        for (key, _, f) in FLAGS {
            out.push_str(&format!("{key}={}\n", *f(&mut c)));
        }
        out.push_str(&format!("rate={}\nblacklist={}\n", self.per_tick(), self.blacklist.join(",")));
        out
    }
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let Some((k, v)) = line.trim().split_once('=') else { continue };
            let b = v == "true";
            if let Some((_, _, f)) = FLAGS.iter().find(|f| f.0 == k) {
                *f(&mut c) = b;
                continue;
            }
            match k {
                // The first version's settings.
                "scroll" => c.scroll_single = b,
                "direction" => match v {
                    "down" => (c.reverse_single, c.reverse_stacks) = (true, true),
                    "position" => c.position_aware = true,
                    _ => {}
                },
                "mass_move" => c.drag_matching = b,
                "shift_drag" => c.drag_stacks = b,
                "recipes" => c.crafting = b,
                "favourites" | "trade_max" => c.villager = c.villager && b,
                "rate" => {
                    if let Ok(r) = v.parse::<u32>() {
                        c.rate = RATES.iter().position(|&x| x == r).unwrap_or(2);
                    }
                }
                "blacklist" => {
                    c.blacklist = v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect();
                }
                _ => {}
            }
        }
        c
    }
    /// The settings page's buttons, in order.
    pub fn labels(&self) -> Vec<String> {
        let mut c = self.clone();
        let mut out: Vec<String> =
            FLAGS.iter().map(|(_, label, f)| format!("{label}: {}", if *f(&mut c) { "ON" } else { "OFF" })).collect();
        out.push(format!("Clicks per tick: {}", self.per_tick()));
        out
    }
    pub fn click(&mut self, button: usize) {
        if let Some((_, _, f)) = FLAGS.get(button) {
            let v = f(self);
            *v = !*v;
        } else if button == FLAGS.len() {
            self.rate = (self.rate + 1) % RATES.len();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hover_moves_each_slot_of_the_clicked_side_once() {
        use super::*;
        let slots: Vec<usize> = (0..36).chain(300..327).collect();
        let side = same_side(305, &slots);
        assert_eq!(hover_pick(Some(306), &side, &[305]), Some(306));
        assert_eq!(hover_pick(Some(305), &side, &[305]), None, "already moved");
        assert_eq!(hover_pick(Some(10), &side, &[305]), None, "the other side: never moved back");
        assert_eq!(hover_pick(None, &side, &[305]), None);
        // Inventory alone: the hotbar and the rest are apart.
        let own: Vec<usize> = (0..36).collect();
        let side = same_side(12, &own);
        assert_eq!(hover_pick(Some(20), &side, &[12]), Some(20));
        assert_eq!(hover_pick(Some(3), &side, &[12]), None);
    }

    use super::*;

    fn st(item: &str, count: u32) -> Stack {
        Stack { item: item.into(), count, max: 64, class: item.len() as u32 * 1000 + item.bytes().map(u32::from).sum::<u32>(), plain: true }
    }
    fn table(slots: Vec<(usize, Stack)>) -> impl Fn(usize) -> Option<Stack> {
        move |s| slots.iter().find(|(i, _)| *i == s).map(|(_, t)| t.clone())
    }
    fn chest_slots() -> Vec<usize> {
        (0..36).chain(300..327).collect()
    }
    fn none(_: usize) -> bool {
        false
    }
    /// The screen height of a chest screen's slot.
    fn chest_y(s: usize) -> f32 {
        match s {
            300..327 => 18.0 + ((s - 300) / 9) as f32 * 18.0,
            9..36 => 84.0 + ((s - 9) / 9) as f32 * 18.0,
            _ => 142.0,
        }
    }

    #[test]
    fn item_scrollers_default_clicks() {
        let h = Held::default();
        assert_eq!(click_action(0, CTRL, h), Some(Action::MoveOne));
        assert_eq!(click_action(0, ALT, h), Some(Action::Matching));
        assert_eq!(click_action(0, SHIFT, h), Some(Action::Stacks));
        assert_eq!(click_action(1, SHIFT, h), Some(Action::LeaveOne));
        assert_eq!(click_action(0, SHIFT | ALT, h), Some(Action::Everything));
        assert_eq!(click_action(0, SHIFT, Held { space: true, ..h }), Some(Action::Everything));
        // No extra keys: Ctrl+Shift+click is vanilla's, and so are plain clicks.
        assert_eq!(click_action(0, CTRL | SHIFT, h), None);
        assert_eq!(click_action(0, 0, h), None);
        assert_eq!(click_action(1, 0, h), None);
        // Super (Cmd) is not looked at.
        assert_eq!(click_action(0, CTRL | 8, h), Some(Action::MoveOne));
        let q = Held { drop: true, ..h };
        assert_eq!(click_action(0, 0, q), Some(Action::DropOne));
        assert_eq!(click_action(0, SHIFT, q), Some(Action::DropStacks));
        assert_eq!(click_action(1, SHIFT, q), Some(Action::DropLeaveOne));
        let w = Held { up: true, ..h };
        assert_eq!(click_action(0, 0, w), Some(Action::Vertical { up: true, amount: VAmount::One }));
        assert_eq!(click_action(0, ALT, w), Some(Action::Vertical { up: true, amount: VAmount::Matching }));
        let s = Held { down: true, ..h };
        assert_eq!(click_action(1, 0, s), Some(Action::Vertical { up: false, amount: VAmount::LeaveOne }));
        assert!(Action::MoveOne.once_per_slot() && !Action::Stacks.once_per_slot());
        assert!(!Action::Everything.drags());
    }

    #[test]
    fn a_fast_drag_passes_every_point() {
        let p = line_points([0.0, 0.0], [40.0, 10.0], 4.0);
        assert_eq!(p.len(), 10);
        assert_eq!(*p.last().unwrap(), [40.0, 10.0]);
        assert_eq!(line_points([5.0, 5.0], [5.0, 5.0], 4.0), vec![[5.0, 5.0]]);
    }

    #[test]
    fn sides_follow_the_open_menu() {
        let chest = chest_slots();
        assert_eq!(same_side(4, &chest).len(), 36);
        assert_eq!(same_side(305, &chest), (300..327).collect::<Vec<_>>());
        let own: Vec<usize> = (0..36).chain(100..104).chain([200]).collect();
        assert_eq!(same_side(4, &own), (0..9).collect::<Vec<_>>());
        assert_eq!(other_side(4, &own, false), (9..36).collect::<Vec<_>>());
        assert_eq!(other_side(20, &chest, false), (300..327).collect::<Vec<_>>());
        assert_eq!(other_side(310, &chest, false)[0], 8);
        let crafting: Vec<usize> = (0..36).chain(100..109).chain([200]).collect();
        assert_eq!(other_side(3, &crafting, true), (100..109).collect::<Vec<_>>());
    }

    #[test]
    fn stack_moves_follow_item_scroller() {
        let mut named = st("minecraft:stone", 3);
        named.class = 1;
        let get = table(vec![(1, st("minecraft:stone", 5)), (12, st("minecraft:stone", 64)), (13, named), (14, st("minecraft:dirt", 1)), (301, st("minecraft:stone", 2))]);
        let slots = chest_slots();
        let m = |matching, to_other, first| StackMove { slot: 12, slots: &slots, table: false, matching, to_other, first };
        // Alt: every matching stack of the side, the hovered one last.
        assert_eq!(move_stacks(&m(true, true, false), &get, &none), vec![vec![quick_move(1)], vec![quick_move(12)]]);
        // Shift+wheel up: one matching stack, not the hovered one while there's another.
        assert_eq!(move_stacks(&m(true, true, true), &get, &none), vec![vec![quick_move(1)]]);
        // Alt+Shift: everything of the side.
        assert_eq!(move_stacks(&m(false, true, false), &get, &none).len(), 4);
        // In: the other side's matching stacks, never the hovered one.
        assert_eq!(move_stacks(&m(true, false, false), &get, &none), vec![vec![quick_move(301)]]);
        assert!(move_stacks(&m(true, false, false), &get, &|s| s == 301).is_empty());
        let only = table(vec![(12, st("minecraft:stone", 64))]);
        assert_eq!(move_stacks(&m(true, true, true), &only, &none), vec![vec![quick_move(12)]]);
        let own: Vec<usize> = (0..36).collect();
        assert_eq!(drop_matching(&st("minecraft:stone", 1), 12, &own, &get), vec![vec![throw_stack(1)], vec![throw_stack(12)]]);
    }

    #[test]
    fn leave_one_keeps_one_behind() {
        let slots = chest_slots();
        let get = table(vec![(10, st("minecraft:stone", 40)), (300, st("minecraft:stone", 60)), (301, st("minecraft:dirt", 1))]);
        let b = leave_one(10, &slots, false, &get).unwrap();
        // Pick up 40, put one back, 4 onto the stack in the chest, 35 into the first empty slot.
        assert_eq!(b, vec![pickup(10), pickup_one(10), pickup(300), pickup(302)]);
        let one = table(vec![(10, st("minecraft:stone", 1))]);
        assert_eq!(leave_one(10, &slots, false, &one), None);
        assert_eq!(drop_leave_one(4, 5), Some(vec![pickup(4), pickup_one(4), drop_hand()]));
        assert_eq!(drop_leave_one(4, 1), None);
    }

    #[test]
    fn w_and_s_move_to_the_furthest_slots() {
        let slots = chest_slots();
        let get = table(vec![(20, st("minecraft:stone", 10)), (300, st("minecraft:dirt", 1))]);
        let up = vertical_targets(20, &slots, true, &chest_y, &get);
        // The chest's top row first (300 holds dirt), then on down.
        assert_eq!(up[0], 301);
        assert!(up.iter().all(|&s| chest_y(s) < chest_y(20)));
        let down = vertical_targets(20, &slots, false, &chest_y, &get);
        assert_eq!(chest_y(down[0]), 142.0);
        assert_eq!(vertical(20, &slots, true, VAmount::One, &chest_y, &get), Some(move_one(20, 10, 301)));
        assert_eq!(vertical(20, &slots, true, VAmount::Stacks, &chest_y, &get), Some(vec![pickup(20), pickup(301)]));
        assert_eq!(vertical(20, &slots, true, VAmount::LeaveOne, &chest_y, &get), Some(vec![pickup(20), pickup_one(20), pickup(301)]));
        let get2 = table(vec![(20, st("minecraft:stone", 10)), (30, st("minecraft:stone", 5)), (2, st("minecraft:stone", 1))]);
        assert_eq!(vertical_matching(20, &slots, true, &chest_y, &get2), vec![20, 30, 2]);
    }

    #[test]
    fn the_wheel_follows_the_settings() {
        let mut c = Config::default();
        assert!(moves_out(1.0, Amount::One, &c, false));
        assert!(!moves_out(-1.0, Amount::One, &c, false));
        c.reverse_single = true;
        assert!(!moves_out(1.0, Amount::One, &c, false));
        assert!(moves_out(1.0, Amount::Stack, &c, false));
        c.reverse_single = false;
        c.position_aware = true;
        // The other inventory above: up moves to it; below: down does.
        assert!(moves_out(1.0, Amount::One, &c, true));
        assert!(moves_out(-1.0, Amount::One, &c, false));
        assert_eq!(amount(0), Amount::One);
        assert_eq!(amount(SHIFT), Amount::Stack);
        assert_eq!(amount(ALT), Amount::Matching);
        assert_eq!(amount(ALT | SHIFT), Amount::Everything);
        assert_eq!(amount(CTRL), Amount::One);
        let slots = chest_slots();
        assert!(inventory_above(20, &slots, &chest_y));
        assert!(!inventory_above(305, &slots, &chest_y));
    }

    #[test]
    fn a_trackpad_scrolls_in_whole_steps() {
        let mut w = Wheel::default();
        assert_eq!(w.steps(0.4), 0);
        assert_eq!(w.steps(0.4), 0);
        assert_eq!(w.steps(0.4), 1);
        assert_eq!(w.steps(-0.5), 0);
        assert_eq!(w.steps(-2.0), -2);
    }

    #[test]
    fn one_item_moves_with_the_rest_put_back() {
        assert_eq!(
            move_one(3, 5, 300),
            vec![
                pickup(3),
                Op { slot: None, button: 4, kind: Kind::QuickCraft },
                Op { slot: Some(300), button: 5, kind: Kind::QuickCraft },
                Op { slot: None, button: 6, kind: Kind::QuickCraft },
                pickup(3)
            ]
        );
        assert_eq!(move_one(3, 1, 300).len(), 4);
        let full = st("minecraft:stone", 64);
        let get = table(vec![(300, full), (301, st("minecraft:stone", 10)), (302, st("minecraft:dirt", 1))]);
        let like = st("minecraft:stone", 1);
        assert_eq!(first_with_room(&like, &[300, 301, 303], &get), Some(301));
        assert_eq!(first_with_room(&like, &[300, 302, 303], &get), Some(303));
    }

    #[test]
    fn the_limiter_keeps_the_average_rate() {
        let mut l = Limiter::default();
        assert!(!l.ready());
        l.tick(4);
        assert!(l.ready());
        l.spend(10);
        assert!(!l.ready());
        l.tick(4);
        assert!(!l.ready());
        l.tick(4);
        assert!(l.ready());
        for _ in 0..10 {
            l.tick(4);
        }
        l.spend(4);
        assert!(!l.ready());
    }

    fn planks_grid() -> Vec<Option<String>> {
        let p = Some("minecraft:oak_planks".to_owned());
        vec![None, None, None, p.clone(), p.clone(), None, p.clone(), p.clone(), None]
    }

    #[test]
    fn a_recipe_is_kept_trimmed_and_fits_smaller_grids() {
        let r = Recipe::from_grid(3, &planks_grid(), Some(("minecraft:crafting_table".into(), 1))).unwrap();
        assert_eq!((r.w, r.h), (2, 2));
        assert!(r.on_grid(2).unwrap().iter().all(|c| c.as_deref() == Some("minecraft:oak_planks")));
        assert_eq!(Recipe::from_line(&r.to_line()), Some(r.clone()));
        let mut book = Book::default();
        book.set(100, Some(r.clone()));
        book.selected = 100;
        assert_eq!(Book::parse(&book.to_text()), book);
        assert_eq!(book.selected_recipe(), Some(&r));
        book.set(100, None);
        assert!(book.slots.is_empty());
        book.set(MAX_RECIPES, Some(r));
        assert!(book.slots.is_empty());
        // The first version's file (no selection line) still reads.
        assert_eq!(Book::parse("3|1|1|a|-|0\n").get(3).map(|r| r.w), Some(1));
    }

    #[test]
    fn the_recipe_view_keys_and_layout() {
        assert_eq!(recipe_nav("key.keyboard.1", false, 40), Some(0));
        assert_eq!(recipe_nav("key.keyboard.9", false, 0), Some(8));
        assert_eq!(recipe_nav("key.keyboard.0", false, 0), None);
        assert_eq!(recipe_nav("key.keyboard.down", false, 3), Some(4));
        assert_eq!(recipe_nav("key.keyboard.up", false, 0), None);
        assert_eq!(recipe_nav("key.keyboard.right", false, 3), Some(12));
        assert_eq!(recipe_nav("key.keyboard.right", true, 3), Some(21));
        assert_eq!(recipe_nav("key.keyboard.left", false, 3), None);
        assert_eq!(recipe_nav("key.keyboard.a", false, 3), None);
        assert_eq!(recipe_scroll(3, -1.0), 4);
        assert_eq!(recipe_scroll(0, 1.0), 0);
        let v = ViewLayout::new(200.0, 300.0);
        assert_eq!(v.cell, 16.0);
        let e = v.entry(0);
        assert!(e[0] + e[2] <= 200.0 && e[0] >= 0.0);
        assert_eq!(v.at([e[0] + 1.0, e[1] + 1.0]), Some(0));
        let e = v.entry(9);
        assert!(e[0] > v.entry(0)[0]);
        assert_eq!(v.at([e[0] + 1.0, e[1] + 1.0]), Some(9));
        assert_eq!(v.at([1.0, 1.0]), None);
        // A narrow window shrinks it.
        assert!(ViewLayout::new(80.0, 300.0).cell < 16.0);
        assert!(v.grid().0[0] < v.x);
    }

    fn view_inventory(stacks: Vec<(usize, Stack)>) -> Vec<(usize, Option<Stack>)> {
        (0..36).map(|i| (i, stacks.iter().find(|(s, _)| *s == i).map(|(_, t)| t.clone()))).collect()
    }

    #[test]
    fn filling_once_puts_one_in_each_cell() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let inv = view_inventory(vec![(10, st("minecraft:oak_planks", 3)), (2, st("minecraft:oak_planks", 10))]);
        let plan = plan_fill(2, &[None, None, None, None], &r, &inv, false, FillMode::One).unwrap();
        assert!(!plan.clearing);
        assert_eq!(plan.batches.len(), 2);
        assert_eq!(plan.batches[0][0], pickup(10));
        assert_eq!(plan.batches[0].iter().filter(|o| o.button == 5).count(), 3);
        assert_eq!(*plan.batches[1].last().unwrap(), pickup(2));
    }

    #[test]
    fn filling_to_the_max_splits_evenly() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let inv = view_inventory(vec![(10, st("minecraft:oak_planks", 64)), (11, st("minecraft:oak_planks", 64))]);
        let plan = plan_fill(3, &vec![None; 9], &r, &inv, false, FillMode::Max).unwrap();
        assert_eq!(plan.batches.len(), 2);
        let mut cells: Vec<usize> = plan.batches[0].iter().filter(|o| o.button == 1).filter_map(|o| o.slot).collect();
        cells.sort();
        assert_eq!(cells, vec![100, 101, 103, 104]);
    }

    #[test]
    fn filling_stops_before_anything_moves_when_short() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let inv = view_inventory(vec![(10, st("minecraft:oak_planks", 3))]);
        assert_eq!(plan_fill(2, &vec![None; 4], &r, &inv, false, FillMode::One), Err(FillError::Missing("minecraft:oak_planks".into())));
        assert_eq!(plan_fill(2, &vec![None; 4], &r, &inv, true, FillMode::One), Err(FillError::HandFull));
    }

    #[test]
    fn wrong_items_leave_the_grid_first() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let inv = view_inventory(vec![(10, st("minecraft:oak_planks", 64))]);
        let grid = vec![Some(st("minecraft:dirt", 1)), Some(st("minecraft:oak_planks", 2)), None, None];
        let plan = plan_fill(2, &grid, &r, &inv, false, FillMode::One).unwrap();
        assert!(plan.clearing);
        assert_eq!(plan.batches, vec![vec![quick_move(100)]]);
        assert_eq!(clear_wrong(2, &grid, &r), vec![vec![quick_move(100)]]);
        assert_eq!(clear_grid(&grid).len(), 2);
    }

    fn craft_view(inv: Vec<(usize, Stack)>) -> CraftView {
        CraftView { size: 2, grid: vec![None; 4], result: None, inventory: view_inventory(inv), carried: None, result_clicks: true }
    }

    #[test]
    fn crafting_everything_fills_takes_and_repeats() {
        let r = Recipe::from_grid(3, &planks_grid(), Some(("minecraft:crafting_table".into(), 1))).unwrap();
        let mut job = CraftJob::everything(r);
        let mut v = craft_view(vec![(10, st("minecraft:oak_planks", 8))]);
        assert!(matches!(job.step(&v), Step::Send(b) if b.len() == 1));
        v.grid = vec![Some(st("minecraft:oak_planks", 2)); 4];
        v.inventory = view_inventory(vec![]);
        assert_eq!(job.step(&v), Step::Wait);
        v.result = Some(st("minecraft:crafting_table", 1));
        assert_eq!(job.step(&v), Step::Send(vec![vec![quick_move(CRAFT_RESULT)]]));
        assert_eq!(job.step(&v), Step::Wait);
        v.grid = vec![None; 4];
        v.result = None;
        assert_eq!(job.step(&v), Step::Wait);
        assert_eq!(job.crafted, 1);
        assert!(matches!(job.step(&v), Step::Done(_)));
    }

    #[test]
    fn mass_crafting_drops_and_stops_when_let_go() {
        let r = Recipe::from_grid(3, &planks_grid(), Some(("minecraft:crafting_table".into(), 1))).unwrap();
        let mut job = CraftJob::mass(r.clone(), true);
        let mut v = craft_view(vec![(10, st("minecraft:oak_planks", 64))]);
        v.grid = vec![Some(st("minecraft:oak_planks", 16)); 4];
        v.result = Some(st("minecraft:crafting_table", 1));
        v.inventory = view_inventory(vec![]);
        // The grid is full already: nothing to fill, then the result is thrown.
        assert_eq!(job.step(&v), Step::Send(vec![]));
        assert_eq!(job.step(&v), Step::Send(vec![vec![throw_stack(CRAFT_RESULT)]]));
        // On a local world the result only takes shift-clicks.
        let mut local = CraftJob::mass(r.clone(), true);
        v.result_clicks = false;
        assert_eq!(local.step(&v), Step::Send(vec![]));
        assert_eq!(local.step(&v), Step::Send(vec![vec![quick_move(CRAFT_RESULT)]]));
        let mut held = CraftJob::mass(r, true);
        held.stop();
        assert!(matches!(held.step(&v), Step::Done(_)));
    }

    #[test]
    fn a_wrong_result_is_never_taken() {
        let r = Recipe::from_grid(3, &planks_grid(), Some(("minecraft:crafting_table".into(), 1))).unwrap();
        let mut job = CraftJob::new(r, FillMode::One, Take::One, 1);
        let mut v = craft_view(vec![]);
        v.grid = vec![Some(st("minecraft:oak_planks", 1)); 4];
        v.result = Some(st("minecraft:stick", 4));
        assert_eq!(job.step(&v), Step::Send(vec![]));
        assert_eq!(job.step(&v), Step::Wait);
        v.result = Some(st("minecraft:crafting_table", 1));
        // One craft picked up and put in the first empty main inventory slot.
        assert_eq!(job.step(&v), Step::Send(vec![vec![pickup(CRAFT_RESULT), pickup(9)]]));
    }

    #[test]
    fn right_click_crafts_up_to_a_stack() {
        let mut job = CraftStack::default();
        let r = st("minecraft:stick", 4);
        assert_eq!(job.step(Some(&r), None), Step::Send(vec![vec![pickup_one(CRAFT_RESULT)]]));
        assert_eq!(job.step(Some(&r), None), Step::Wait);
        let held = st("minecraft:stick", 4);
        assert_eq!(job.step(Some(&r), Some(&held)), Step::Send(vec![vec![pickup_one(CRAFT_RESULT)]]));
        let full = st("minecraft:stick", 62);
        let mut job2 = CraftStack::default();
        assert!(matches!(job2.step(Some(&r), Some(&full)), Step::Done(_)));
        assert!(matches!(CraftStack::default().step(None, None), Step::Done(_)));
    }

    #[test]
    fn trading_max_selects_takes_and_repeats() {
        let mut job = TradeMax::new(2);
        let mut v = TradeView { offer_result: "minecraft:emerald".into(), ..Default::default() };
        assert_eq!(job.step(&v), Step::Select(2));
        assert_eq!(job.step(&v), Step::Wait);
        v.payment = vec![Some(st("minecraft:wheat", 40)), None];
        v.result = Some(st("minecraft:emerald", 1));
        assert_eq!(job.step(&v), Step::Send(vec![vec![quick_move(TRADE_RESULT)]]));
        v.payment = vec![Some(st("minecraft:wheat", 0)), None];
        v.result = None;
        assert_eq!(job.step(&v), Step::Wait);
        assert_eq!(job.rounds, 1);
        assert_eq!(job.step(&v), Step::Select(2));
        for _ in 0..TIMEOUT {
            assert_eq!(job.step(&v), Step::Wait);
        }
        assert!(matches!(job.step(&v), Step::Done(_)));
        let mut sold_out = TradeMax::new(0);
        v.out_of_stock = true;
        assert!(matches!(sold_out.step(&v), Step::Done(_)));
    }

    #[test]
    fn one_trades_payment_goes_in_item_by_item() {
        let pay = vec![
            Payment { slot: 600, held: 2, need: 5, fits: true, sources: vec![(10, 2), (11, 10)] },
            Payment { slot: 601, held: 0, need: 1, fits: true, sources: vec![(4, 1)] },
        ];
        let plan = plan_payment(&pay).unwrap();
        assert_eq!(plan[0], vec![pickup(10), pickup_one(600), pickup_one(600)]);
        assert_eq!(plan[1], vec![pickup(11), pickup_one(600), pickup(11)]);
        assert_eq!(plan[2], vec![pickup(4), pickup_one(601)]);
        let short = vec![Payment { slot: 600, held: 0, need: 5, fits: true, sources: vec![(10, 2)] }];
        assert_eq!(plan_payment(&short), None);
        let wrong = vec![Payment { slot: 600, held: 1, need: 5, fits: false, sources: vec![(10, 9)] }];
        assert_eq!(plan_payment(&wrong), None);
    }

    #[test]
    fn favourites_are_per_villager_then_global() {
        let offers = vec![trade_items("minecraft:wheat", None, "minecraft:emerald"), trade_items("minecraft:emerald", None, "minecraft:bread")];
        let v = villager_id("minecraft:farmer", &offers);
        let mut f = Favourites::default();
        assert!(f.toggle(Favourites::key(None, &offers[1])));
        assert_eq!(f.of(&v, &offers), (vec![1], true));
        assert!(f.toggle(Favourites::key(Some(&v), &offers[0])));
        assert_eq!(f.of(&v, &offers), (vec![0], false));
        assert_eq!(Favourites::parse(&f.to_text()), f);
        // The first version's lines become global favourites.
        let old = Favourites::parse("minecraft:farmer|minecraft:wheat|-|minecraft:emerald\n");
        assert_eq!(old.of("x", &offers), (vec![0], true));
        let mut s = TradeScroll::default();
        s.set(&v, 4);
        assert_eq!(TradeScroll::parse(&s.to_text()).get(&v), Some(4));
        assert_eq!(clamp_trade_scroll(9, 10), 3);
        assert_eq!(clamp_trade_scroll(9, 5), 0);
    }

    #[test]
    fn sorting_merges_and_orders() {
        let slots = vec![
            (9, Some(st("minecraft:stone", 10))),
            (10, Some(st("minecraft:dirt", 5))),
            (11, None),
            (12, Some(st("minecraft:stone", 60))),
        ];
        let plan = plan_sort(&slots);
        // 54 stone onto the first stack, then the dirt to the front.
        assert_eq!(plan[0], vec![pickup(12), pickup(9), pickup(12)]);
        assert_eq!(plan[1], vec![pickup(10), pickup(9), pickup(10)]);
        // Sorted already: nothing to do.
        let sorted = vec![(9, Some(st("minecraft:dirt", 5))), (10, Some(st("minecraft:stone", 64))), (11, None)];
        assert!(plan_sort(&sorted).is_empty());
    }

    #[test]
    fn settings_round_trip_and_cycle() {
        let mut c = Config::default();
        assert_eq!(c.per_tick(), 4);
        let n = c.labels().len();
        for b in 0..n {
            c.click(b);
        }
        c.blacklist = vec!["minecraft:generic_9x3".into()];
        assert_eq!(Config::parse(&c.serialize()), c);
        assert!(c.blacklisted("minecraft:generic_9x3"));
        assert_eq!(c.per_tick(), 8);
        assert!(!c.enabled);
        assert_eq!(Config::parse(""), Config::default());
        // The first version's settings carry over.
        let old = Config::parse("scroll=false\ndirection=position\nmass_move=false\nrate=16\n");
        assert!(!old.scroll_single && old.position_aware && !old.drag_matching && old.per_tick() == 16);
        assert_eq!(key_mods("mass_craft"), CTRL | ALT);
        assert_eq!(key_mods("drop_key"), 0);
        assert_eq!(folder_name("play.example.net:25565"), "play.example.net_25565");
    }
}
