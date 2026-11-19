// Lumos Item Scroller: the rules (std only). Which slots a wheel turn or a mass click moves, the
// clicks that move them, the rate the clicks go out at, how a stored recipe fills the crafting
// grid, the mass-craft and trade-many steps, and the settings, recipe slots and favourites as text.
//
// Slot ids are Horizon's inventory screen ids: 0..9 the hotbar, 9..36 the rest of the player's
// inventory, 100..109 the crafting grid, 200 its result, 300..500 an open container, 500..505 the
// armour and off hand, 600.. a workstation's inputs and 650 its result (a merchant's trade).

use std::collections::BTreeSet;

pub const ID: &str = "lumos_item_scroller";
pub const NAME: &str = "Item Scroller";
pub const VERSION: &str = "1.0.0";
pub const DESCRIPTION: &str = "Move items with the mouse wheel (one, a stack or every matching stack), \
Ctrl+Shift+click and Shift+drag mass moves, Ctrl+Shift+Q drops every matching stack, recipe slots \
beside the crafting grid with mass crafting, and favourite villager trades with Shift+click to trade \
as many times as you can.";

/// The settings, one file in the mod's data folder.
pub const CONFIG_KEY: &str = "config";
/// Favourite trades, every world's.
pub const FAVOURITES_KEY: &str = "favourites";

/// Hotkeys: name, label, default key. The key bindings themselves are read in keys.rs.
pub const HOTKEYS: [(&str, &str, &str); 3] = [
    ("drop_all", "Drop all matching (with Ctrl+Shift)", "key.keyboard.q"),
    ("mass_craft", "Mass craft the selected recipe", "key.keyboard.unknown"),
    ("store_recipe", "Store the grid in the hovered recipe slot", "key.keyboard.unknown"),
];

pub const GRID: usize = 100;
pub const CRAFT_RESULT: usize = 200;
pub const CONTAINER: usize = 300;
pub const TRADE_RESULT: usize = 650;
/// Ticks a step waits for the server before it gives up.
pub const TIMEOUT: u32 = 20;
/// GLFW modifier bits.
pub const SHIFT: u32 = 1;
pub const CTRL: u32 = 2;
pub const ALT: u32 = 4;

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
pub fn quick_move(slot: usize) -> Op {
    Op { slot: Some(slot), button: 0, kind: Kind::QuickMove }
}
/// THROW with the control button: the whole stack.
pub fn throw_stack(slot: usize) -> Op {
    Op { slot: Some(slot), button: 1, kind: Kind::Throw }
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

/// Every slot of `slot`'s side holding what it holds, `slot` itself included, as quick moves.
pub fn move_matching(slot: usize, slots: &[usize], stack: &dyn Fn(usize) -> Option<Stack>) -> Vec<Batch> {
    let Some(like) = stack(slot) else {
        return Vec::new();
    };
    matching(&like, &same_side(slot, slots), stack).into_iter().map(|s| vec![quick_move(s)]).collect()
}

/// Every stack of `slot`'s inventory (the player's whole inventory, or the container) holding what
/// it holds, thrown.
pub fn drop_matching(slot: usize, slots: &[usize], stack: &dyn Fn(usize) -> Option<Stack>) -> Vec<Batch> {
    let Some(like) = stack(slot) else {
        return Vec::new();
    };
    let side: Vec<usize> = if is_player(slot) {
        slots.iter().copied().filter(|&s| is_player(s)).collect()
    } else {
        same_side(slot, slots)
    };
    matching(&like, &side, stack).into_iter().map(|s| vec![throw_stack(s)]).collect()
}

// ── the wheel ──

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Wheel up moves items out of the hovered stack, down brings them in (Item Scroller's way).
    UpMovesOut,
    /// Wheel down moves them out (Horizon's Mouse Tweaks' way).
    DownMovesOut,
    /// Wheel up moves items up the screen: out of a stack of the player's inventory below, into
    /// one of the container (or the inventory above the hotbar) above.
    ByPosition,
}

/// Whether a wheel turn of `dy` (up is positive) over `slot` moves items out of it.
pub fn moves_out(dy: f64, dir: Direction, slot: usize, slots: &[usize]) -> bool {
    let up = dy > 0.0;
    match dir {
        Direction::UpMovesOut => up,
        Direction::DownMovesOut => !up,
        Direction::ByPosition => {
            let container = slots.iter().any(|&s| is_container(s) || is_grid(s));
            let lower = match slot {
                0..9 => true,
                9..36 => container,
                _ => false,
            };
            if lower { up } else { !up }
        }
    }
}

/// How much a wheel turn moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Amount {
    One,
    Stack,
    All,
}

pub fn amount(modifiers: u32) -> Amount {
    if modifiers & CTRL != 0 {
        Amount::All
    } else if modifiers & SHIFT != 0 {
        Amount::Stack
    } else {
        Amount::One
    }
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
}

/// The recipe slots of one world or server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Book {
    pub slots: Vec<Option<Recipe>>,
}
impl Book {
    pub fn get(&self, i: usize) -> Option<&Recipe> {
        self.slots.get(i).and_then(Option::as_ref)
    }
    pub fn set(&mut self, i: usize, r: Option<Recipe>) {
        if self.slots.len() <= i {
            self.slots.resize(i + 1, None);
        }
        self.slots[i] = r;
        while self.slots.last().is_some_and(Option::is_none) {
            self.slots.pop();
        }
    }
    /// One line a slot: `index|recipe`.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
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
            if let Some((i, rest)) = line.trim().split_once('|')
                && let Ok(i) = i.parse::<usize>()
                && i < 64
                && let Some(r) = Recipe::from_line(rest)
            {
                book.set(i, Some(r));
            }
        }
        book
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
    let wrong: Vec<Batch> = (0..n * n)
        .filter(|&i| match (&grid.get(i).cloned().flatten(), &want[i]) {
            (Some(s), Some(w)) => s.item != *w,
            (Some(_), None) => true,
            _ => false,
        })
        .map(|i| vec![quick_move(GRID + i)])
        .collect();
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

/// What a step needs to know of the crafting screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CraftView {
    pub size: usize,
    pub grid: Vec<Option<Stack>>,
    pub result: Option<Stack>,
    pub inventory: Vec<(usize, Option<Stack>)>,
    pub carried: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Fill,
    AwaitResult,
    AwaitTaken,
    Cleanup,
    Finished,
}

/// Mass crafting: fill the grid with all it takes, shift-click the result, again, until the
/// ingredients run out; then what's left in the grid goes back.
#[derive(Clone, Debug, PartialEq)]
pub struct MassCraft {
    pub recipe: Recipe,
    phase: Phase,
    wait: u32,
    pub crafted: u32,
    before: Vec<Option<Stack>>,
    why: String,
}
impl MassCraft {
    pub fn new(recipe: Recipe) -> Self {
        MassCraft { recipe, phase: Phase::Fill, wait: 0, crafted: 0, before: Vec::new(), why: String::new() }
    }
    fn cleanup(&mut self, why: &str) {
        self.phase = Phase::Cleanup;
        self.why = why.to_owned();
    }
    pub fn step(&mut self, v: &CraftView) -> Step {
        match self.phase {
            Phase::Fill => match plan_fill(v.size, &v.grid, &self.recipe, &v.inventory, v.carried, FillMode::Max) {
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
                    let why = if self.crafted == 0 { format!("not enough {item}") } else { "out of ingredients".into() };
                    self.cleanup(&why);
                    self.step(v)
                }
            },
            Phase::AwaitResult => {
                if v.result.is_some() {
                    self.before = v.grid.clone();
                    self.phase = Phase::AwaitTaken;
                    self.wait = 0;
                    return Step::Send(vec![vec![quick_move(CRAFT_RESULT)]]);
                }
                self.wait += 1;
                if self.wait > TIMEOUT {
                    self.cleanup("the grid makes nothing");
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
                        self.cleanup("stopped");
                    }
                    return Step::Wait;
                }
                self.wait += 1;
                if self.wait > TIMEOUT {
                    self.cleanup("no room for the result");
                    return self.step(v);
                }
                Step::Wait
            }
            Phase::Cleanup => {
                self.phase = Phase::Finished;
                let back: Vec<Batch> =
                    (0..v.grid.len()).filter(|&i| v.grid[i].is_some()).map(|i| vec![quick_move(GRID + i)]).collect();
                if back.is_empty() { Step::Done(self.why.clone()) } else { Step::Send(back) }
            }
            Phase::Finished => Step::Done(self.why.clone()),
        }
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

/// A favourite offer: the merchant's profession (or its name, when the server doesn't say) and
/// the offer's items. Restocks and price changes keep it.
pub fn favourite_key(profession: &str, cost_a: &str, cost_b: Option<&str>, result: &str) -> String {
    let clean = |s: &str| s.replace(['|', '\n'], "");
    format!("{}|{}|{}|{}", clean(profession), clean(cost_a), clean(cost_b.unwrap_or("-")), clean(result))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Favourites {
    pub keys: BTreeSet<String>,
}
impl Favourites {
    pub fn parse(text: &str) -> Favourites {
        Favourites { keys: text.lines().map(str::trim).filter(|l| l.split('|').count() == 4).map(str::to_owned).collect() }
    }
    pub fn to_text(&self) -> String {
        self.keys.iter().map(|k| format!("{k}\n")).collect()
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
    pub fn has(&self, key: &str) -> bool {
        self.keys.contains(key)
    }
}

/// A file-name-safe form of a world or server name.
pub fn folder_name(s: &str) -> String {
    let out: String =
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).take(80).collect();
    if out.is_empty() { "_".into() } else { out }
}

// ── the recipe slot column ──

/// The book of recipes is 147 wide and sits just left of the shifted panel.
pub const BOOK_WIDTH: f32 = 150.0;

/// Slot `i` of `count` recipe slots, `[x, y, 18, 18]`: columns of 9 left of the panel `rect`
/// (`[x, y, w, h]`), left of the recipe book when it is open, or right of the panel when there is
/// no room on the left.
pub fn recipe_slot_rect(rect: [f32; 4], book: bool, count: usize, i: usize) -> [f32; 4] {
    let cols = count.div_ceil(9).max(1);
    let col = i / 9;
    let row = i % 9;
    let width = cols as f32 * 18.0;
    let left = rect[0] - if book { BOOK_WIDTH } else { 0.0 } - 4.0 - width;
    let x0 = if left >= 2.0 { left } else { rect[0] + rect[2] + 4.0 };
    [x0 + col as f32 * 18.0, rect[1] + 4.0 + row as f32 * 18.0, 18.0, 18.0]
}

pub fn recipe_slot_at(rect: [f32; 4], book: bool, count: usize, at: [f32; 2]) -> Option<usize> {
    (0..count).find(|&i| {
        let [x, y, w, h] = recipe_slot_rect(rect, book, count, i);
        at[0] >= x && at[0] < x + w && at[1] >= y && at[1] < y + h
    })
}

// ── settings ──

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub scroll: bool,
    pub direction: Direction,
    pub mass_move: bool,
    pub shift_drag: bool,
    pub drop_all: bool,
    pub recipes: bool,
    pub recipe_slots: usize,
    pub mass_craft: bool,
    pub favourites: bool,
    pub highlight: bool,
    pub trade_max: bool,
    /// Index into `RATES`.
    pub rate: usize,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            scroll: true,
            direction: Direction::UpMovesOut,
            mass_move: true,
            shift_drag: true,
            drop_all: true,
            recipes: true,
            recipe_slots: 9,
            mass_craft: true,
            favourites: true,
            highlight: true,
            trade_max: true,
            rate: 2,
        }
    }
}
impl Config {
    pub fn per_tick(&self) -> u32 {
        RATES[self.rate.min(RATES.len() - 1)]
    }
    pub fn serialize(&self) -> String {
        let dir = match self.direction {
            Direction::UpMovesOut => "up",
            Direction::DownMovesOut => "down",
            Direction::ByPosition => "position",
        };
        format!(
            "scroll={}\ndirection={dir}\nmass_move={}\nshift_drag={}\ndrop_all={}\nrecipes={}\nrecipe_slots={}\nmass_craft={}\nfavourites={}\nhighlight={}\ntrade_max={}\nrate={}\n",
            self.scroll,
            self.mass_move,
            self.shift_drag,
            self.drop_all,
            self.recipes,
            self.recipe_slots,
            self.mass_craft,
            self.favourites,
            self.highlight,
            self.trade_max,
            self.per_tick(),
        )
    }
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let Some((k, v)) = line.trim().split_once('=') else { continue };
            let b = v == "true";
            match k {
                "scroll" => c.scroll = b,
                "direction" => {
                    c.direction = match v {
                        "down" => Direction::DownMovesOut,
                        "position" => Direction::ByPosition,
                        _ => Direction::UpMovesOut,
                    }
                }
                "mass_move" => c.mass_move = b,
                "shift_drag" => c.shift_drag = b,
                "drop_all" => c.drop_all = b,
                "recipes" => c.recipes = b,
                "recipe_slots" => c.recipe_slots = if v == "18" { 18 } else { 9 },
                "mass_craft" => c.mass_craft = b,
                "favourites" => c.favourites = b,
                "highlight" => c.highlight = b,
                "trade_max" => c.trade_max = b,
                "rate" => {
                    if let Ok(r) = v.parse::<u32>() {
                        c.rate = RATES.iter().position(|&x| x == r).unwrap_or(2);
                    }
                }
                _ => {}
            }
        }
        c
    }
    /// The settings page's buttons, in order.
    pub fn labels(&self) -> Vec<String> {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        let dir = match self.direction {
            Direction::UpMovesOut => "wheel up moves out",
            Direction::DownMovesOut => "wheel down moves out",
            Direction::ByPosition => "by slot position",
        };
        vec![
            format!("Scrolling (wheel, Shift, Ctrl): {}", on(self.scroll)),
            format!("Scroll direction: {dir}"),
            format!("Ctrl+Shift+click moves all matching: {}", on(self.mass_move)),
            format!("Shift+drag moves each slot: {}", on(self.shift_drag)),
            format!("Ctrl+Shift+drop key drops all matching: {}", on(self.drop_all)),
            format!("Recipe slots: {}", on(self.recipes)),
            format!("Recipe slot count: {}", self.recipe_slots),
            format!("Mass crafting: {}", on(self.mass_craft)),
            format!("Favourite trades: {}", on(self.favourites)),
            format!("Highlight favourite trades: {}", on(self.highlight)),
            format!("Shift+click a trade trades max: {}", on(self.trade_max)),
            format!("Clicks per tick: {}", self.per_tick()),
        ]
    }
    pub fn click(&mut self, button: usize) {
        match button {
            0 => self.scroll = !self.scroll,
            1 => {
                self.direction = match self.direction {
                    Direction::UpMovesOut => Direction::DownMovesOut,
                    Direction::DownMovesOut => Direction::ByPosition,
                    Direction::ByPosition => Direction::UpMovesOut,
                }
            }
            2 => self.mass_move = !self.mass_move,
            3 => self.shift_drag = !self.shift_drag,
            4 => self.drop_all = !self.drop_all,
            5 => self.recipes = !self.recipes,
            6 => self.recipe_slots = if self.recipe_slots == 9 { 18 } else { 9 },
            7 => self.mass_craft = !self.mass_craft,
            8 => self.favourites = !self.favourites,
            9 => self.highlight = !self.highlight,
            10 => self.trade_max = !self.trade_max,
            11 => self.rate = (self.rate + 1) % RATES.len(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn sides_follow_the_open_menu() {
        let chest = chest_slots();
        assert_eq!(same_side(4, &chest).len(), 36);
        assert_eq!(same_side(305, &chest), (300..327).collect::<Vec<_>>());
        let own: Vec<usize> = (0..36).chain(100..104).chain([200]).collect();
        assert_eq!(same_side(4, &own), (0..9).collect::<Vec<_>>());
        assert_eq!(same_side(20, &own), (9..36).collect::<Vec<_>>());
        assert_eq!(same_side(101, &own), (100..104).collect::<Vec<_>>());
        assert_eq!(other_side(4, &own, false), (9..36).collect::<Vec<_>>());
        assert_eq!(other_side(20, &chest, false), (300..327).collect::<Vec<_>>());
        assert_eq!(other_side(310, &chest, false)[0], 8);
        let crafting: Vec<usize> = (0..36).chain(100..109).chain([200]).collect();
        assert_eq!(other_side(3, &crafting, true), (100..109).collect::<Vec<_>>());
    }

    #[test]
    fn matching_stacks_need_the_same_components() {
        let mut named = st("minecraft:stone", 3);
        named.class = 1;
        named.plain = false;
        let get = table(vec![(1, st("minecraft:stone", 5)), (12, st("minecraft:stone", 64)), (13, named), (14, st("minecraft:dirt", 1)), (301, st("minecraft:stone", 2))]);
        let slots = chest_slots();
        let moved = move_matching(12, &slots, &get);
        assert_eq!(moved, vec![vec![quick_move(1)], vec![quick_move(12)]]);
        assert_eq!(move_matching(301, &slots, &get), vec![vec![quick_move(301)]]);
        assert!(move_matching(20, &slots, &get).is_empty());
        let own: Vec<usize> = (0..36).collect();
        assert_eq!(drop_matching(12, &own, &get), vec![vec![throw_stack(1)], vec![throw_stack(12)]]);
    }

    #[test]
    fn the_wheel_direction_is_configurable() {
        let chest = chest_slots();
        assert!(moves_out(1.0, Direction::UpMovesOut, 5, &chest));
        assert!(!moves_out(-1.0, Direction::UpMovesOut, 5, &chest));
        assert!(moves_out(-1.0, Direction::DownMovesOut, 5, &chest));
        // By position: up leaves the inventory below for the chest above, and the chest the other way.
        assert!(moves_out(1.0, Direction::ByPosition, 20, &chest));
        assert!(moves_out(-1.0, Direction::ByPosition, 300, &chest));
        let own: Vec<usize> = (0..36).collect();
        assert!(moves_out(-1.0, Direction::ByPosition, 20, &own));
        assert!(moves_out(1.0, Direction::ByPosition, 2, &own));
        assert_eq!(amount(0), Amount::One);
        assert_eq!(amount(SHIFT), Amount::Stack);
        assert_eq!(amount(CTRL | SHIFT), Amount::All);
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
        let mut full = st("minecraft:stone", 64);
        full.count = 64;
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
        // Idle ticks save at most one tick's worth.
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
        let on2 = r.on_grid(2).unwrap();
        assert!(on2.iter().all(|c| c.as_deref() == Some("minecraft:oak_planks")));
        assert_eq!(Recipe::from_line(&r.to_line()), Some(r.clone()));
        let mut book = Book::default();
        book.set(3, Some(r.clone()));
        assert_eq!(Book::parse(&book.to_text()), book);
        book.set(3, None);
        assert!(book.slots.is_empty());
        assert_eq!(Recipe::from_grid(3, &vec![None; 9], None), None);
        let wide = Recipe { w: 3, h: 1, cells: vec![Some("a".into()); 3], result: None };
        assert_eq!(wide.on_grid(2), None);
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
        // Main inventory first: 3 from slot 10 (all used, nothing to put back), then 1 from the hotbar.
        assert_eq!(plan.batches.len(), 2);
        let first = &plan.batches[0];
        assert_eq!(first[0], pickup(10));
        assert_eq!(first.iter().filter(|o| o.button == 5).count(), 3);
        assert_eq!(first.last().unwrap().button, 6);
        let second = &plan.batches[1];
        assert_eq!(second[0], pickup(2));
        assert_eq!(second.iter().filter(|o| o.button == 5).count(), 1);
        assert_eq!(*second.last().unwrap(), pickup(2));
    }

    #[test]
    fn filling_to_the_max_splits_evenly() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let inv = view_inventory(vec![(10, st("minecraft:oak_planks", 64)), (11, st("minecraft:oak_planks", 64))]);
        let plan = plan_fill(3, &vec![None; 9], &r, &inv, false, FillMode::Max).unwrap();
        assert_eq!(plan.batches.len(), 2);
        for b in &plan.batches {
            assert_eq!(b.iter().filter(|o| o.button == 1).count(), 4);
            // 64 over 4 cells is 16 each: nothing comes back.
            assert_eq!(b.last().unwrap().kind, Kind::QuickCraft);
        }
        // The cells are the 2x2 at the top left of the table.
        let cells: Vec<usize> = plan.batches[0].iter().filter(|o| o.button == 1).filter_map(|o| o.slot).collect();
        let mut cells = cells;
        cells.sort();
        assert_eq!(cells, vec![100, 101, 103, 104]);
    }

    #[test]
    fn filling_stops_before_anything_moves_when_short() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let inv = view_inventory(vec![(10, st("minecraft:oak_planks", 3))]);
        assert_eq!(
            plan_fill(2, &vec![None; 4], &r, &inv, false, FillMode::One),
            Err(FillError::Missing("minecraft:oak_planks".into()))
        );
        assert_eq!(plan_fill(2, &vec![None; 4], &r, &inv, true, FillMode::One), Err(FillError::HandFull));
        // A named stack is never used.
        let mut named = st("minecraft:oak_planks", 64);
        named.plain = false;
        let inv = view_inventory(vec![(10, named)]);
        assert!(matches!(plan_fill(2, &vec![None; 4], &r, &inv, false, FillMode::One), Err(FillError::Missing(_))));
    }

    #[test]
    fn wrong_items_leave_the_grid_first() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let inv = view_inventory(vec![(10, st("minecraft:oak_planks", 64))]);
        let grid = vec![Some(st("minecraft:dirt", 1)), Some(st("minecraft:oak_planks", 2)), None, None];
        let plan = plan_fill(2, &grid, &r, &inv, false, FillMode::One).unwrap();
        assert!(plan.clearing);
        assert_eq!(plan.batches, vec![vec![quick_move(100)]]);
        // Cells that already hold some only get what they lack.
        let grid = vec![Some(st("minecraft:oak_planks", 1)), Some(st("minecraft:oak_planks", 2)), None, None];
        let plan = plan_fill(2, &grid, &r, &inv, false, FillMode::One).unwrap();
        assert_eq!(plan.batches[0].iter().filter(|o| o.button == 5).count(), 2);
    }

    #[test]
    fn mass_crafting_fills_takes_and_cleans_up() {
        let r = Recipe::from_grid(3, &planks_grid(), Some(("minecraft:crafting_table".into(), 1))).unwrap();
        let mut job = MassCraft::new(r);
        let mut v = CraftView {
            size: 2,
            grid: vec![None; 4],
            result: None,
            inventory: view_inventory(vec![(10, st("minecraft:oak_planks", 8))]),
            carried: false,
        };
        assert!(matches!(job.step(&v), Step::Send(b) if b.len() == 1));
        // The fill landed (as the client predicts it).
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
        // Nothing left: done, with an empty grid nothing to put back.
        assert!(matches!(job.step(&v), Step::Done(_)));
    }

    #[test]
    fn mass_crafting_gives_up_when_nothing_is_made() {
        let r = Recipe::from_grid(3, &planks_grid(), None).unwrap();
        let mut job = MassCraft::new(r);
        let v = CraftView {
            size: 2,
            grid: vec![Some(st("minecraft:oak_planks", 1)); 4],
            result: None,
            inventory: view_inventory(vec![]),
            carried: false,
        };
        assert_eq!(job.step(&v), Step::Send(vec![]));
        for _ in 0..TIMEOUT {
            assert_eq!(job.step(&v), Step::Wait);
        }
        // Timed out: the grid goes back.
        assert_eq!(job.step(&v), Step::Send((100..104).map(|s| vec![quick_move(s)]).collect()));
        assert!(matches!(job.step(&v), Step::Done(_)));
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
    fn favourites_survive_a_round_trip() {
        let mut f = Favourites::default();
        let k = favourite_key("minecraft:farmer", "minecraft:wheat", None, "minecraft:emerald");
        assert!(f.toggle(k.clone()));
        assert!(f.has(&k));
        let again = Favourites::parse(&f.to_text());
        assert_eq!(again, f);
        assert!(!f.toggle(k.clone()));
        assert!(!f.has(&k));
        assert_eq!(favourite_key("a|b", "x", Some("y"), "z"), "ab|x|y|z");
    }

    #[test]
    fn settings_round_trip_and_cycle() {
        let mut c = Config::default();
        assert_eq!(c.per_tick(), 4);
        for b in 0..12 {
            c.click(b);
        }
        assert_eq!(Config::parse(&c.serialize()), c);
        assert_eq!(c.labels().len(), 12);
        assert_eq!(c.direction, Direction::DownMovesOut);
        assert_eq!(c.per_tick(), 8);
        assert_eq!(Config::parse(""), Config::default());
    }

    #[test]
    fn recipe_slots_sit_left_of_the_panel_or_the_book() {
        let rect = [200.0, 50.0, 176.0, 166.0];
        assert_eq!(recipe_slot_rect(rect, false, 9, 0), [178.0, 54.0, 18.0, 18.0]);
        assert_eq!(recipe_slot_rect(rect, false, 9, 8)[1], 54.0 + 8.0 * 18.0);
        assert_eq!(recipe_slot_rect(rect, false, 18, 9)[0], 200.0 - 4.0 - 36.0 + 18.0);
        // No room left of the book: the right of the panel.
        assert_eq!(recipe_slot_rect(rect, true, 9, 0)[0], 28.0);
        assert_eq!(recipe_slot_rect([100.0, 50.0, 176.0, 166.0], true, 9, 0)[0], 280.0);
        assert_eq!(recipe_slot_at(rect, false, 9, [180.0, 60.0]), Some(0));
        assert_eq!(recipe_slot_at(rect, false, 9, [100.0, 60.0]), None);
        assert_eq!(folder_name("play.example.net:25565"), "play.example.net_25565");
    }
}
