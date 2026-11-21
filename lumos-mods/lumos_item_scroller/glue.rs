// Lumos Item Scroller: between Horizon's `Game` and the rules in `lumos_item_scroller`.
//
// The press, release, key and render hooks only read the game (`&Game`), so what they ask for
// waits in a queue; the client tick (and the wheel and drag hooks, which lend the game mutably)
// sends it, at most the configured clicks a tick. Every click is an ordinary container click
// through `Game::menu_ops`: predicted and sent as `ContainerClick`s on a server (the integrated
// one too), run by the menu engine on a local world. Clicks go out only while the inventory screen
// they were asked for is still the open one: the queue is dropped when it closes or changes, and
// `send` checks again before every batch.
use crate::game::lumos_item_scroller as m;
use crate::game::lumos_item_scroller_keys as keys;
use crate::game::{Game, Screen};
use crate::mod_api::{NativeMod, NativeScreenView, NativeWorldId};
use horizon_client::hud::HudBatch;
use horizon_client::items::ItemStack;
use horizon_client::menu::Input;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Instant;

/// The merchant screen is drawn 50 px left of the 176-wide origin it shares (trading.rs).
const MERCHANT_SHIFT: f32 = -50.0;
const GOLD: u32 = 0xFFFF_C83C;
const PURPLE: u32 = 0xFFC8_64FF;

/// Something the mod is to do, worked out from the slots when its turn comes.
#[derive(Clone, Debug)]
enum Work {
    Batches(VecDeque<m::Batch>),
    /// A shift-click of the slot, when it holds something.
    Shift(usize),
    /// Item Scroller's stack moves (`m::StackMove`).
    Stacks { slot: usize, matching: bool, to_other: bool, first: bool },
    /// One item out of the slot, where a shift-click would send it.
    PushOne(usize),
    /// One item into the slot from the other side.
    PullOne(usize),
    LeaveOne(usize),
    Throw { slot: usize, all: bool },
    DropLeaveOne(usize),
    Vertical { slot: usize, up: bool, amount: m::VAmount },
    VerticalMatching { slot: usize, up: bool },
    /// Every stack like this slot's in its inventory, thrown.
    DropMatching(usize),
    /// Every stack like `like` in `side`'s inventory, thrown (after the hand's was dropped).
    DropLike { side: usize, like: ItemStack },
    /// One craft or trade from the result slot to the inventory.
    TakeOne(usize),
    /// One trade's payment into the payment slots (offer index).
    Pay(usize),
    ClearPayment,
    /// Select merchant offer `index` (vanilla fills the payment with all it can).
    Select(usize),
    ClearWrong(m::Recipe),
    ClearGrid,
    Fill { recipe: m::Recipe, mode: m::FillMode, rounds: u32 },
    Craft(m::CraftJob),
    CraftStack(m::CraftStack),
    Trade(m::TradeMax),
    ThrowResults(String),
    MoveResults(String),
    Sort(usize),
}
impl Work {
    /// A job that waits for the server between its steps.
    fn job(&self) -> bool {
        matches!(self, Work::Fill { .. } | Work::Craft(_) | Work::CraftStack(_) | Work::Trade(_))
    }
}

/// A click-drag move going on.
#[derive(Clone, Debug)]
struct Drag {
    action: m::Action,
    /// The button the press came with (before macOS's Ctrl+click is read as a left click).
    button: i32,
    last: Option<usize>,
    visited: Vec<usize>,
}

/// After a Shift+click lets go, while Shift stays held: each stack hovered on the clicked slot's
/// side is shift-clicked too, once (no button held: the pointer is followed every frame).
#[derive(Clone, Debug)]
struct Hover {
    first: usize,
    /// The slots of the clicked slot's side, read at the first frame.
    side: Option<Vec<usize>>,
    visited: Vec<usize>,
    /// The pointer at the last frame.
    at: [f32; 2],
}

#[derive(Default)]
struct State {
    config: Option<m::Config>,
    /// The stored recipes of the world they were read for.
    book: Option<(String, m::Book)>,
    favourites: Option<m::Favourites>,
    trade_scroll: Option<m::TradeScroll>,
    wheel: m::Wheel,
    limiter: m::Limiter,
    queue: VecDeque<Work>,
    /// The screen the queue was made in (`screen_key`).
    screen: String,
    /// A press the mod took: its release is the mod's too.
    swallow: Option<i32>,
    drag: Option<Drag>,
    hover: Option<Hover>,
    /// The slot of the last press with an empty hand: where a stack in the hand came from.
    source: Option<usize>,
    /// The mass craft key started a run that hasn't been let go of.
    mass_latch: bool,
    /// The villager whose trade list position was put back, and the position last seen.
    merchant: Option<(String, usize)>,
    notice: Option<(String, Instant)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn config(game: &Game<'_>) -> m::Config {
    with(|s| {
        s.config
            .get_or_insert_with(|| m::Config::parse(&game.native_setting(m::ID, m::CONFIG_KEY).unwrap_or_default()))
            .clone()
    })
}

fn store_config(game: &Game<'_>, cfg: m::Config) {
    game.native_set_setting(m::ID, m::CONFIG_KEY, &cfg.serialize());
    with(|s| s.config = Some(cfg));
}

fn notice(text: impl Into<String>) {
    with(|s| s.notice = Some((text.into(), Instant::now())));
}

/// `sp-<save folder>`, `mp-<address>` or `global`: the recipes are kept per world and server.
fn world_key(game: &Game<'_>) -> String {
    if config(game).recipes_global {
        return "global".into();
    }
    match game.native_client_state().and_then(|c| c.world) {
        Some(NativeWorldId::Singleplayer(folder)) => format!("sp-{}", m::folder_name(&folder)),
        Some(NativeWorldId::Server(address)) => format!("mp-{}", m::folder_name(&address)),
        None => "default".into(),
    }
}

fn book(game: &Game<'_>) -> m::Book {
    let key = world_key(game);
    if let Some(b) = with(|s| s.book.as_ref().filter(|(k, _)| *k == key).map(|(_, b)| b.clone())) {
        return b;
    }
    let b = m::Book::parse(&game.native_setting(m::ID, &format!("recipes-{key}")).unwrap_or_default());
    with(|s| s.book = Some((key, b.clone())));
    b
}

fn store_book(game: &Game<'_>, b: m::Book) {
    let key = world_key(game);
    game.native_set_setting(m::ID, &format!("recipes-{key}"), &b.to_text());
    with(|s| s.book = Some((key, b)));
}

fn favourites(game: &Game<'_>) -> m::Favourites {
    with(|s| {
        s.favourites
            .get_or_insert_with(|| m::Favourites::parse(&game.native_setting(m::ID, m::FAVOURITES_KEY).unwrap_or_default()))
            .clone()
    })
}

fn trade_scroll(game: &Game<'_>) -> m::TradeScroll {
    with(|s| {
        s.trade_scroll
            .get_or_insert_with(|| m::TradeScroll::parse(&game.native_setting(m::ID, m::TRADE_SCROLL_KEY).unwrap_or_default()))
            .clone()
    })
}

// ── the screen ──

/// The open inventory screen, as far as the queue cares: the server's window and menu type, the
/// container and the workstation. A change drops the queue.
fn screen_key(game: &Game<'_>) -> String {
    let (window, menu) = game.remote_menu();
    format!("{window}|{menu}|{:?}|{}|{}", game.container, game.station.is_some(), game.craft_size)
}

/// The mod's clicks may go out: an inventory screen (not creative's) is open.
fn clicks_allowed(game: &Game<'_>) -> bool {
    game.screen == Screen::Inventory && !game.creative_inventory_open() && !game.creative_tab_open()
}

/// The mod is on for the open screen: its switch, its own on/off key and the blacklist.
fn active(game: &Game<'_>, cfg: &m::Config) -> bool {
    game.native_on(m::ID) && cfg.enabled && clicks_allowed(game) && !cfg.blacklisted(&game.remote_menu().1)
}

fn enqueue(game: &Game<'_>, work: Work) {
    let key = screen_key(game);
    with(|s| {
        if s.queue.is_empty() || s.screen != key {
            s.queue.clear();
            s.screen = key;
        }
        // Wheel turns can pile up: keep them to a few.
        if s.queue.len() < 32 {
            s.queue.push_back(work);
        }
    });
}

fn busy() -> bool {
    with(|s| s.queue.iter().any(Work::job))
}

/// The GUI's size as the screen hooks see it, for the hooks that are not handed a batch.
fn gui_batch(game: &Game<'_>) -> Option<HudBatch> {
    let r = game.renderer.as_ref()?;
    Some(HudBatch::new([r.config.width, r.config.height]))
}

/// A server's menus with plain click rules (the ones Horizon's Mouse Tweaks runs in, as these
/// moves are made of the same clicks).
fn plain_menu_type(name: &str) -> bool {
    name == "inventory"
        || name.starts_with("minecraft:generic_9x")
        || matches!(
            name,
            "minecraft:shulker_box"
                | "minecraft:hopper"
                | "minecraft:generic_3x3"
                | "minecraft:furnace"
                | "minecraft:blast_furnace"
                | "minecraft:smoker"
                | "minecraft:crafting"
        )
}

/// The open screen is a container, the inventory or a crafting table with plain click rules.
fn plain_menu(game: &Game<'_>) -> bool {
    use horizon_client::containers::BlockEntity;
    if game.screen != Screen::Inventory
        || game.station.is_some()
        || game.mount_active()
        || game.creative_inventory_open()
        || game.creative_tab_open()
    {
        return false;
    }
    if game.remote.is_some() {
        game.remote_menu_type().is_none_or(plain_menu_type)
    } else {
        matches!(game.container_kind(), None | Some(BlockEntity::Chest(_)) | Some(BlockEntity::Furnace(_)))
    }
}

/// The crafting grid's side when the inventory (2x2) or a crafting table (3x3) is open: Item
/// Scroller's crafting screens.
fn crafting(game: &Game<'_>) -> Option<usize> {
    if !plain_menu(game) {
        return None;
    }
    let grid = if game.remote.is_some() {
        matches!(game.remote_menu_type(), None | Some("minecraft:crafting"))
    } else {
        game.container_kind().is_none()
    };
    (grid && matches!(game.craft_size, 2 | 3)).then_some(game.craft_size)
}

/// Item Scroller's recipe view: only in a crafting screen, only while its key (A) is held.
fn view_open(game: &Game<'_>, cfg: &m::Config) -> bool {
    cfg.crafting && crafting(game).is_some() && !game.recipe_search_focused() && keys::held(game, "recipe_view")
}

fn view_layout(game: &Game<'_>, batch: &HudBatch) -> m::ViewLayout {
    let (ox, _) = game.inventory_origin(batch);
    m::ViewLayout::new(ox, batch.height)
}

/// The recipe (its index in the book) under the pointer in the recipe view.
fn view_entry_under(game: &Game<'_>, at: [f32; 2]) -> Option<usize> {
    let batch = gui_batch(game)?;
    let i = view_layout(game, &batch).at(at)?;
    Some(book(game).selected / m::PAGE * m::PAGE + i)
}

/// The merchant offer under the pointer.
fn offer_under(game: &Game<'_>, at: [f32; 2]) -> Option<usize> {
    let merchant = game.station.as_ref()?.merchant.as_ref()?;
    let batch = gui_batch(game)?;
    let (ox, oy) = game.inventory_origin(&batch);
    let (x, y) = (at[0] - ox, at[1] - oy);
    (0..7).find_map(|i| {
        let (rx, ry) = (5.0 + MERCHANT_SHIFT, 18.0 + 20.0 * i as f32);
        let index = i + merchant.scroll;
        (x >= rx && x < rx + 89.0 && y >= ry && y < ry + 20.0 && index < merchant.session.offers.len()).then_some(index)
    })
}

/// The open merchant as favourites know it: the villager and each offer's items.
fn villager(game: &Game<'_>) -> Option<(String, Vec<String>)> {
    let merchant = game.station.as_ref()?.merchant.as_ref()?;
    let items: Vec<String> = merchant
        .session
        .offers
        .iter()
        .map(|o| m::trade_items(&o.cost_a(game.items()).item, o.cost_b().as_ref().map(|s| &*s.item), &o.result.item))
        .collect();
    if items.is_empty() {
        return None;
    }
    let who = if !merchant.session.profession.is_empty() {
        merchant.session.profession.clone()
    } else {
        merchant.name.clone().unwrap_or_else(|| "merchant".into())
    };
    Some((m::villager_id(&who, &items), items))
}

/// What a slot holds, read without lending the game (the player's, the grid's, a container's).
fn peek<'a>(game: &'a Game<'_>, slot: usize) -> Option<&'a ItemStack> {
    match slot {
        0..36 => game.inventory.get(slot)?.as_ref(),
        100..109 => game.craft_grid.get(slot - 100)?.as_ref(),
        300..500 => {
            let mut n = slot - 300;
            for (pos, size) in game.container_parts() {
                if n < size {
                    return game.world.block_entities.get(&pos)?.slots().get(n)?.as_ref();
                }
                n -= size;
            }
            None
        }
        _ => None,
    }
}

// ── slots ──

/// The open menu's slots and what they hold, with matching stacks numbered alike.
struct Snapshot {
    slots: Vec<usize>,
    stacks: Vec<(usize, Option<m::Stack>)>,
    reps: Vec<ItemStack>,
}
impl Snapshot {
    fn take(game: &mut Game<'_>) -> Snapshot {
        let slots = game.slots();
        let mut reps: Vec<ItemStack> = Vec::new();
        let mut stacks = Vec::with_capacity(slots.len());
        for &s in &slots {
            let st = game.slot_view(s).map(|st| pure(game, &st, &mut reps));
            stacks.push((s, st));
        }
        Snapshot { slots, stacks, reps }
    }
    fn get(&self, slot: usize) -> Option<m::Stack> {
        self.stacks.iter().find(|(s, _)| *s == slot).and_then(|(_, st)| st.clone())
    }
    /// `like` as the rules see it, numbered with the slots' stacks.
    fn like(&mut self, game: &Game<'_>, like: &ItemStack) -> m::Stack {
        pure(game, like, &mut self.reps)
    }
}

fn pure(game: &Game<'_>, st: &ItemStack, reps: &mut Vec<ItemStack>) -> m::Stack {
    let class = match reps.iter().position(|r| r.same_item_same_components(st)) {
        Some(i) => i,
        None => {
            reps.push(st.clone());
            reps.len() - 1
        }
    } as u32;
    m::Stack {
        item: st.item.to_string(),
        count: st.count,
        max: game.max_stack(st),
        class,
        plain: st.extra.is_none() && st.damage == 0,
    }
}

fn stack_of(game: &Game<'_>, st: &Option<ItemStack>) -> Option<m::Stack> {
    let mut reps = Vec::new();
    st.as_ref().map(|s| pure(game, s, &mut reps))
}

/// Where vanilla's quick move would put `slot`'s items, tried on a copy of the menu.
fn landing(game: &mut Game<'_>, slot: usize) -> Vec<usize> {
    let slots = game.slots();
    let mut landing = Vec::new();
    game.menu_session(|menu, env, to_menu| {
        let from = to_menu(slot)?;
        landing = menu
            .quick_move_landing(env, from)
            .into_iter()
            .filter_map(|i| slots.iter().copied().find(|&g| to_menu(g) == Some(i)))
            .collect();
        Some(false)
    });
    landing
}

fn takes_one(game: &mut Game<'_>, slot: usize, stack: &ItemStack) -> bool {
    let mut takes = false;
    game.menu_session(|menu, env, to_menu| {
        takes = to_menu(slot).is_some_and(|i| menu.takes_one(env, i, stack));
        Some(false)
    });
    takes
}

fn is_result(game: &mut Game<'_>, slot: usize) -> bool {
    if slot == m::CRAFT_RESULT || slot == m::TRADE_RESULT {
        return true;
    }
    let mut result = false;
    game.menu_session(|menu, _, to_menu| {
        result = to_menu(slot).is_some_and(|i| menu.is_result(i));
        Some(false)
    });
    result
}

/// The clicks go out; false when the open menu took none of them, or when the screen the queue
/// was made for is no longer open (then the queue goes).
fn send(game: &mut Game<'_>, batch: &m::Batch) -> bool {
    if !clicks_allowed(game) || with(|s| s.screen != screen_key(game)) {
        with(|s| s.queue.clear());
        return false;
    }
    let ops: Vec<(Option<usize>, i32, Input)> = batch
        .iter()
        .map(|o| {
            let input = match o.kind {
                m::Kind::Pickup => Input::Pickup,
                m::Kind::QuickMove => Input::QuickMove,
                m::Kind::Throw => Input::Throw,
                m::Kind::QuickCraft => Input::QuickCraft,
            };
            (o.slot, o.button, input)
        })
        .collect();
    if ops.is_empty() {
        return true;
    }
    if !game.menu_ops(&ops) {
        return false;
    }
    if game.remote.is_none()
        && let Some(key) = game.container
    {
        game.container_changed(key);
    }
    true
}

/// `MerchantMenu.tryMoveItems` for offer `index`: the select trade packet on a server.
fn select_trade(game: &mut Game<'_>, index: usize) {
    if !clicks_allowed(game) || game.station.as_ref().is_none_or(|s| s.merchant.is_none()) {
        return;
    }
    if game.remote.is_some() {
        if let Some(mer) = game.station.as_mut().and_then(|s| s.merchant.as_mut()) {
            mer.session.selection_hint = index;
        }
        game.send(horizon_protocol::packets::play::Serverbound::SelectTrade(
            horizon_protocol::packets::play::SelectTrade { item: index as i32 },
        ));
        return;
    }
    game.menu_session(|menu, env, _| {
        menu.merchant_select(env, index);
        Some(true)
    });
}

fn craft_view(game: &mut Game<'_>) -> m::CraftView {
    let n = game.craft_size;
    let grid = (0..n * n).map(|i| stack_of(game, &game.craft_grid[i])).collect();
    let result = game.craft_result();
    let result = stack_of(game, &result);
    let inventory = (0..36).map(|i| (i, stack_of(game, &game.inventory[i]))).collect();
    let carried = stack_of(game, &game.carried);
    m::CraftView { size: n, grid, result, inventory, carried, result_clicks: game.remote.is_some() }
}

fn trade_view(game: &mut Game<'_>, index: usize) -> Option<m::TradeView> {
    let offer = game.station.as_ref()?.merchant.as_ref()?.session.offers.get(index)?.clone();
    let result = game.slot_view(m::TRADE_RESULT);
    let payment: Vec<Option<ItemStack>> = (0..2).map(|i| game.slot_view(m::PAYMENT + i)).collect();
    Some(m::TradeView {
        offer_result: offer.result.item.to_string(),
        out_of_stock: offer.out_of_stock(),
        result: stack_of(game, &result),
        payment: payment.iter().map(|p| stack_of(game, p)).collect(),
        carried: game.carried.is_some(),
    })
}

/// One trade's payment for offer `index`, from the player's inventory.
fn payment(game: &mut Game<'_>, index: usize) -> Option<Vec<m::Batch>> {
    let offer = game.station.as_ref()?.merchant.as_ref()?.session.offers.get(index)?.clone();
    let costs = [Some((offer.base_cost_a.clone(), offer.cost_a_count(game.items()))), offer.cost_b.clone().map(|c| {
        let n = c.stack.count;
        (c, n)
    })];
    let mut pay = Vec::new();
    for (k, cost) in costs.into_iter().enumerate() {
        let Some((cost, need)) = cost else {
            continue;
        };
        let slot = m::PAYMENT + k;
        let held = game.slot_view(slot);
        let sources = (0..36)
            .filter_map(|i| game.inventory[i].as_ref().filter(|s| cost.test(s)).map(|s| (i, s.count)))
            .collect();
        pay.push(m::Payment {
            slot,
            held: held.as_ref().map_or(0, |s| s.count),
            need,
            fits: held.as_ref().is_none_or(|s| cost.test(s)),
            sources,
        });
    }
    m::plan_payment(&pay)
}

/// A wheel, click or key move as clicks, from the slots as they are now.
fn resolve(game: &mut Game<'_>, work: &Work) -> Option<VecDeque<m::Batch>> {
    let mut snap = Snapshot::take(game);
    let table = game.craft_size == 3 && crafting(game).is_some();
    let batches: Vec<m::Batch> = match work {
        Work::Shift(slot) => {
            snap.get(*slot)?;
            vec![vec![m::quick_move(*slot)]]
        }
        &Work::Stacks { slot, matching, to_other, first } => {
            let results: Vec<usize> = snap.slots.clone().into_iter().filter(|&s| !m::movable(s) || (m::is_container(s) && is_result(game, s))).collect();
            let get = |s: usize| snap.get(s);
            let mv = m::StackMove { slot, slots: &snap.slots, table, matching, to_other, first };
            m::move_stacks(&mv, &get, &|s| results.contains(&s))
        }
        Work::DropMatching(slot) => {
            let like = snap.get(*slot)?;
            m::drop_matching(&like, *slot, &snap.slots, &|s| snap.get(s))
        }
        Work::DropLike { side, like } => {
            let like = snap.like(game, like);
            m::drop_matching(&like, *side, &snap.slots, &|s| snap.get(s))
        }
        &Work::PushOne(slot) => {
            let st = snap.get(slot)?;
            let target = landing(game, slot).first().copied().or_else(|| {
                let other = m::other_side(slot, &snap.slots, table);
                m::first_with_room(&st, &other, &|s| snap.get(s))
            })?;
            vec![m::move_one(slot, st.count, target)]
        }
        &Work::PullOne(slot) => {
            let st = snap.get(slot)?;
            let held = game.slot_view(slot)?;
            if st.count >= st.max || !takes_one(game, slot, &held) {
                return None;
            }
            let other = m::other_side(slot, &snap.slots, table);
            let mut source = None;
            for s in m::matching(&st, &other, &|s| snap.get(s)) {
                if !is_result(game, s) {
                    source = Some(s);
                    break;
                }
            }
            let source = source?;
            vec![m::move_one(source, snap.get(source)?.count, slot)]
        }
        &Work::LeaveOne(slot) => vec![m::leave_one(slot, &snap.slots, table, &|s| snap.get(s))?],
        &Work::Throw { slot, all } => {
            snap.get(slot)?;
            vec![vec![if all { m::throw_stack(slot) } else { m::throw_one(slot) }]]
        }
        &Work::DropLeaveOne(slot) => vec![m::drop_leave_one(slot, snap.get(slot)?.count)?],
        &Work::Vertical { slot, up, amount } => {
            let ys: Vec<(usize, f32)> = snap.slots.iter().map(|&s| (s, game.slot_position(s).1)).collect();
            let y = |s: usize| ys.iter().find(|(i, _)| *i == s).map_or(0.0, |(_, y)| *y);
            let among: Vec<usize> = snap.slots.iter().copied().filter(|&s| m::is_player(s) || m::is_container(s)).collect();
            vec![m::vertical(slot, &among, up, amount, &y, &|s| snap.get(s))?]
        }
        &Work::VerticalMatching { slot, up } => {
            let ys: Vec<(usize, f32)> = snap.slots.iter().map(|&s| (s, game.slot_position(s).1)).collect();
            let y = |s: usize| ys.iter().find(|(i, _)| *i == s).map_or(0.0, |(_, y)| *y);
            // Each matching stack is its own move, planned when its turn comes.
            let each = m::vertical_matching(slot, &snap.slots, up, &y, &|s| snap.get(s));
            with(|s| {
                for &slot in each.iter().rev() {
                    s.queue.push_front(Work::Vertical { slot, up, amount: m::VAmount::Stacks });
                }
            });
            return None;
        }
        &Work::TakeOne(slot) => {
            let st = snap.get(slot)?;
            let order: Vec<usize> = (9..36).chain(0..9).collect();
            let (targets, placed) = m::spread(&st, st.count, &order, false, &|s| snap.get(s));
            if placed < st.count || game.remote.is_none() && slot == m::CRAFT_RESULT {
                return None;
            }
            let mut b = vec![m::pickup(slot)];
            b.extend(targets.into_iter().map(m::pickup));
            vec![b]
        }
        &Work::Pay(index) => payment(game, index)?,
        Work::ClearPayment => (0..2).map(|k| m::PAYMENT + k).filter(|&s| snap.get(s).is_some()).map(|s| vec![m::quick_move(s)]).collect(),
        Work::ClearWrong(recipe) => {
            let v = craft_view(game);
            m::clear_wrong(v.size, &v.grid, recipe)
        }
        Work::ClearGrid => m::clear_grid(&craft_view(game).grid),
        Work::ThrowResults(item) => m::throw_results(item, &snap.slots, &|s| snap.get(s)),
        Work::MoveResults(item) => m::move_results(item, &snap.slots, &|s| snap.get(s)),
        &Work::Sort(slot) => {
            let part: Vec<usize> = if m::is_player(slot) {
                (9..36).collect()
            } else {
                snap.slots.iter().copied().filter(|&s| m::is_container(s)).collect()
            };
            let stacks: Vec<(usize, Option<m::Stack>)> = part.into_iter().map(|s| (s, snap.get(s))).collect();
            m::plan_sort(&stacks)
        }
        _ => return None,
    };
    Some(batches.into())
}

/// Sends what the queue holds while the tick's clicks last. `tick`: a new tick, when the
/// step-by-step jobs (filling, crafting, trading) take their next step.
fn run(game: &mut Game<'_>, tick: bool) {
    let mut stepped = false;
    loop {
        if !with(|s| s.limiter.ready()) {
            return;
        }
        let Some(work) = with(|s| s.queue.pop_front()) else {
            return;
        };
        if !clicks_allowed(game) || with(|s| s.screen != screen_key(game)) {
            with(|s| s.queue.clear());
            return;
        }
        let hand_ok = matches!(work, Work::Batches(_) | Work::Craft(_) | Work::CraftStack(_) | Work::Trade(_));
        if game.carried.is_some() && !hand_ok {
            // The player holds something: the moves that need an empty hand are dropped.
            continue;
        }
        if work.job() {
            if !tick || stepped {
                with(|s| s.queue.push_front(work));
                return;
            }
            stepped = true;
        }
        match work {
            Work::Batches(mut batches) => {
                let Some(batch) = batches.pop_front() else {
                    continue;
                };
                if !send(game, &batch) {
                    with(|s| s.queue.clear());
                    return;
                }
                with(|s| {
                    s.limiter.spend(batch.len());
                    if !batches.is_empty() {
                        s.queue.push_front(Work::Batches(batches));
                    }
                });
            }
            Work::Select(index) => {
                select_trade(game, index);
                with(|s| s.limiter.spend(1));
            }
            Work::Fill { recipe, mode, rounds } => {
                let v = craft_view(game);
                match m::plan_fill(v.size, &v.grid, &recipe, &v.inventory, v.carried.is_some(), mode) {
                    Ok(plan) if plan.clearing => {
                        if rounds < 3 {
                            with(|s| {
                                s.queue.push_front(Work::Fill { recipe, mode, rounds: rounds + 1 });
                                s.queue.push_front(Work::Batches(plan.batches.into()));
                            });
                        }
                    }
                    Ok(plan) => with(|s| s.queue.push_front(Work::Batches(plan.batches.into()))),
                    Err(e) => fill_error(e),
                }
            }
            Work::Craft(mut job) => {
                if crafting(game).is_none() {
                    continue;
                }
                let v = craft_view(game);
                match job.step(&v) {
                    m::Step::Send(batches) => with(|s| {
                        s.queue.push_front(Work::Craft(job));
                        s.queue.push_front(Work::Batches(batches.into()));
                    }),
                    m::Step::Wait | m::Step::Select(_) => {
                        with(|s| s.queue.push_front(Work::Craft(job)));
                        return;
                    }
                    m::Step::Done(why) => {
                        if job.crafted > 0 || !job.hold {
                            notice(format!("Crafted {} times: {why}", job.crafted));
                        }
                    }
                }
            }
            Work::CraftStack(mut job) => {
                let result = game.craft_result();
                let (result, carried) = (stack_of(game, &result), stack_of(game, &game.carried));
                match job.step(result.as_ref(), carried.as_ref()) {
                    m::Step::Send(batches) => with(|s| {
                        s.queue.push_front(Work::CraftStack(job));
                        s.queue.push_front(Work::Batches(batches.into()));
                    }),
                    m::Step::Wait | m::Step::Select(_) => {
                        with(|s| s.queue.push_front(Work::CraftStack(job)));
                        return;
                    }
                    m::Step::Done(_) => {}
                }
            }
            Work::Trade(mut job) => {
                let Some(v) = trade_view(game, job.index) else {
                    continue;
                };
                match job.step(&v) {
                    m::Step::Send(batches) => with(|s| {
                        s.queue.push_front(Work::Trade(job));
                        s.queue.push_front(Work::Batches(batches.into()));
                    }),
                    m::Step::Select(index) => {
                        select_trade(game, index);
                        with(|s| {
                            s.limiter.spend(1);
                            s.queue.push_front(Work::Trade(job));
                        });
                        return;
                    }
                    m::Step::Wait => {
                        with(|s| s.queue.push_front(Work::Trade(job)));
                        return;
                    }
                    m::Step::Done(why) => {
                        // As Item Scroller: what's left in the payment slots goes back.
                        with(|s| s.queue.push_front(Work::ClearPayment));
                        if job.rounds > 0 || why != "can't pay for it" {
                            notice(format!("Traded {} times: {why}", job.rounds));
                        } else {
                            notice("Can't pay for that trade");
                        }
                    }
                }
            }
            other => {
                if let Some(batches) = resolve(game, &other)
                    && !batches.is_empty()
                {
                    with(|s| s.queue.push_front(Work::Batches(batches)));
                }
            }
        }
    }
}

fn fill_error(e: m::FillError) {
    match e {
        m::FillError::Missing(item) => notice(format!("Not enough {}", item.trim_start_matches("minecraft:"))),
        m::FillError::TooBig => notice("That recipe needs a crafting table"),
        m::FillError::HandFull => notice("Put down the stack in your hand first"),
    }
}

// ── hooks ──

/// The client tick: the queue goes out at the configured rate, and goes when the screen does;
/// mass crafting follows its key; the trade list's position is put back and kept.
pub(super) fn on_tick(game: &mut Game<'_>) {
    if !game.native_on(m::ID) {
        with(|s| {
            s.queue.clear();
            s.drag = None;
            s.hover = None;
        });
        return;
    }
    let cfg = config(game);
    let key = screen_key(game);
    let open = clicks_allowed(game) && cfg.enabled;
    with(|s| {
        if !open || (!s.queue.is_empty() && s.screen != key) {
            s.queue.clear();
            s.drag = None;
            s.hover = None;
            s.swallow = None;
            s.wheel = m::Wheel::default();
        }
        if !open {
            s.source = None;
            s.mass_latch = false;
        }
    });
    if !open {
        with(|s| s.merchant = None);
        return;
    }
    if active(game, &cfg) {
        remember_trade_scroll(game, &cfg);
        mass_craft(game, &cfg);
    }
    with(|s| s.limiter.tick(cfg.per_tick()));
    run(game, true);
}

/// Item Scroller's mass craft: while Ctrl+Alt+C is held (or its hold switch is on), the selected
/// recipe is filled and crafted again and again.
fn mass_craft(game: &mut Game<'_>, cfg: &m::Config) {
    let held = keys::held(game, "mass_craft") && game.has_control_down() && game.has_alt_down() && !game.has_shift_down();
    let on = cfg.crafting && crafting(game).is_some() && (held || cfg.mass_craft_hold);
    let running = with(|s| s.queue.iter().any(|w| matches!(w, Work::Craft(j) if j.hold)));
    if !on {
        with(|s| {
            s.mass_latch = false;
            for w in s.queue.iter_mut() {
                if let Work::Craft(j) = w
                    && j.hold
                {
                    j.stop();
                }
            }
        });
        return;
    }
    if running || with(|s| s.mass_latch) || busy() || game.carried.is_some() {
        return;
    }
    let Some(recipe) = book(game).selected_recipe().cloned().filter(|r| r.result.is_some()) else {
        return;
    };
    with(|s| s.mass_latch = true);
    enqueue(game, Work::Craft(m::CraftJob::mass(recipe, cfg.mass_craft_drops)));
}

/// `villagerTradeListRememberScrollPosition`: the list opens where it was left, per villager.
fn remember_trade_scroll(game: &mut Game<'_>, cfg: &m::Config) {
    if !cfg.villager || !cfg.remember_scroll {
        return;
    }
    let Some((vid, items)) = villager(game) else {
        with(|s| s.merchant = None);
        return;
    };
    let Some(scroll) = game.station.as_ref().and_then(|s| s.merchant.as_ref()).map(|m| m.scroll) else {
        return;
    };
    let seen = with(|s| s.merchant.clone());
    match seen {
        Some((v, last)) if v == vid => {
            if scroll != last {
                with(|s| s.merchant = Some((vid.clone(), scroll)));
                let mut t = trade_scroll(game);
                t.set(&vid, scroll);
                game.native_set_setting(m::ID, m::TRADE_SCROLL_KEY, &t.to_text());
                with(|s| s.trade_scroll = Some(t));
            }
        }
        _ => {
            let to = trade_scroll(game).get(&vid).map_or(0, |n| m::clamp_trade_scroll(n, items.len()));
            if let Some(merchant) = game.station.as_mut().and_then(|s| s.merchant.as_mut()) {
                merchant.scroll = to;
            }
            with(|s| s.merchant = Some((vid, to)));
        }
    }
}

/// The wheel over a stack: one item, Shift a stack, Alt every matching stack, Alt+Shift
/// everything; over the crafting result and the trade result, Item Scroller's crafting and
/// trading; over the recipe view, the selection.
pub(super) fn on_scroll(game: &mut Game<'_>, screen: &str, scroll: [f64; 2], modifiers: u32, canceled: &mut bool) {
    if !game.native_on(m::ID) || screen != "inventory" || scroll[1] == 0.0 {
        return;
    }
    let cfg = config(game);
    if !active(game, &cfg) || game.carried.is_some() {
        return;
    }
    if view_open(game, &cfg) {
        *canceled = true;
        let mut b = book(game);
        b.selected = m::recipe_scroll(b.selected, scroll[1]);
        store_book(game, b);
        return;
    }
    let Some(slot) = game.inventory_target().0 else {
        with(|s| s.wheel = m::Wheel::default());
        return;
    };
    let amount = m::amount(modifiers);
    let slots = game.slots();
    let ys: Vec<(usize, f32)> = slots.iter().map(|&s| (s, game.slot_position(s).1)).collect();
    let y = |s: usize| ys.iter().find(|(i, _)| *i == s).map_or(0.0, |(_, y)| *y);
    let above = m::inventory_above(slot, &slots, &y);
    if slot == m::CRAFT_RESULT && cfg.crafting && crafting(game).is_some() {
        *canceled = true;
        let steps = with(|s| s.wheel.steps(scroll[1]));
        if steps != 0 && !busy() {
            craft_scroll(game, &cfg, m::moves_out(f64::from(steps), amount, &cfg, above), amount, steps.unsigned_abs());
            run(game, false);
        }
        return;
    }
    if slot == m::TRADE_RESULT && cfg.scroll_villager && game.station.as_ref().is_some_and(|s| s.merchant.is_some()) {
        *canceled = true;
        let steps = with(|s| s.wheel.steps(scroll[1]));
        if steps != 0 && !busy() {
            trade_scroll_move(game, m::moves_out(f64::from(steps), amount, &cfg, above), amount == m::Amount::Stack);
            run(game, false);
        }
        return;
    }
    if !plain_menu(game) || !m::movable(slot) {
        return;
    }
    let on = match amount {
        m::Amount::One => cfg.scroll_single,
        m::Amount::Stack => cfg.scroll_stacks,
        m::Amount::Matching => cfg.scroll_matching,
        m::Amount::Everything => cfg.scroll_everything,
    };
    if !on {
        return;
    }
    let Some(stack) = game.slot_view(slot) else {
        return;
    };
    // A bundle keeps its own wheel (picking the item it gives).
    if stack.item.ends_with("bundle") {
        return;
    }
    // Item Scroller's wheel wins over Horizon's Mouse Tweaks wheel while it is on: cancelled here,
    // Mouse Tweaks never hears this turn.
    *canceled = true;
    if busy() {
        return;
    }
    let steps = with(|s| s.wheel.steps(scroll[1]));
    if steps == 0 {
        return;
    }
    let out = m::moves_out(f64::from(steps), amount, &cfg, above);
    for _ in 0..steps.unsigned_abs().min(8) {
        let work = match (amount, out) {
            (m::Amount::One, true) => Work::PushOne(slot),
            (m::Amount::One, false) => Work::PullOne(slot),
            (m::Amount::Stack, to_other) => Work::Stacks { slot, matching: true, to_other, first: true },
            (m::Amount::Matching, to_other) => Work::Stacks { slot, matching: true, to_other, first: false },
            (m::Amount::Everything, to_other) => Work::Stacks { slot, matching: false, to_other, first: false },
        };
        enqueue(game, work);
        if matches!(amount, m::Amount::Matching | m::Amount::Everything) {
            break;
        }
    }
    run(game, false);
}

/// The wheel over the crafting result (`enableCraftingFeatures`): toward the inventory it takes
/// what the selected recipe makes (one craft, Shift a shift-click, Alt+Shift everything) or, over
/// an empty result, clears the grid; the other way it fills the grid (Shift: stacks).
fn craft_scroll(game: &mut Game<'_>, _cfg: &m::Config, out: bool, amount: m::Amount, steps: u32) {
    let recipe = book(game).selected_recipe().cloned();
    let result = game.craft_result();
    if out {
        let Some(result) = result else {
            enqueue(game, Work::ClearGrid);
            return;
        };
        let wanted = recipe.as_ref().and_then(|r| r.result.as_ref()).is_none_or(|(item, _)| *item == *result.item);
        if !wanted {
            return;
        }
        match amount {
            m::Amount::Everything => {
                if let Some(r) = recipe {
                    enqueue(game, Work::Craft(m::CraftJob::everything(r)));
                }
            }
            m::Amount::Stack => enqueue(game, Work::Batches(VecDeque::from([vec![m::quick_move(m::CRAFT_RESULT)]]))),
            _ => {
                if game.remote.is_some() {
                    for _ in 0..steps.min(8) {
                        enqueue(game, Work::TakeOne(m::CRAFT_RESULT));
                    }
                }
            }
        }
    } else if let Some(recipe) = recipe {
        let mode = if amount == m::Amount::Stack { m::FillMode::Max } else { m::FillMode::One };
        enqueue(game, Work::Fill { recipe, mode, rounds: 0 });
    }
}

/// The wheel over the trade result (`enableScrollingVillager`).
fn trade_scroll_move(game: &mut Game<'_>, out: bool, stacks: bool) {
    let Some(index) = game.station.as_ref().and_then(|s| s.merchant.as_ref()).map(|m| m.session.selection_hint) else {
        return;
    };
    let has_result = game.slot_view(m::TRADE_RESULT).is_some();
    let work = match (stacks, out, has_result) {
        (true, false, _) => Work::Select(index),
        (true, true, true) => Work::Batches(VecDeque::from([vec![m::quick_move(m::TRADE_RESULT)]])),
        (true, true, false) => Work::ClearPayment,
        (false, false, _) => Work::Pay(index),
        (false, true, true) => Work::TakeOne(m::TRADE_RESULT),
        (false, true, false) => return,
    };
    enqueue(game, work);
}

/// The work a click (or a slot dragged over) of `action` asks for.
fn action_work(action: m::Action, slot: usize) -> Work {
    match action {
        m::Action::MoveOne => Work::PushOne(slot),
        m::Action::LeaveOne => Work::LeaveOne(slot),
        m::Action::Stacks => Work::Shift(slot),
        m::Action::Matching => Work::Stacks { slot, matching: true, to_other: true, first: false },
        m::Action::Everything => Work::Stacks { slot, matching: false, to_other: true, first: false },
        m::Action::DropOne => Work::Throw { slot, all: false },
        m::Action::DropStacks => Work::Throw { slot, all: true },
        m::Action::DropLeaveOne => Work::DropLeaveOne(slot),
        m::Action::Vertical { up, amount: m::VAmount::Matching } => Work::VerticalMatching { slot, up },
        m::Action::Vertical { up, amount } => Work::Vertical { slot, up, amount },
    }
}

fn action_on(action: m::Action, cfg: &m::Config) -> bool {
    match action {
        m::Action::MoveOne => cfg.drag_one,
        m::Action::LeaveOne => cfg.drag_leave_one,
        m::Action::Stacks => cfg.drag_stacks,
        m::Action::Matching => cfg.drag_matching,
        m::Action::Everything => cfg.move_everything,
        m::Action::DropOne | m::Action::DropStacks | m::Action::DropLeaveOne => cfg.drag_drop,
        m::Action::Vertical { .. } => cfg.ws_moves,
    }
}

/// macOS turns Control-click into a right click when Horizon's "Ctrl+click emulates right click"
/// is on: Item Scroller's Ctrl+click is read from that right click (Item Scroller has no
/// Ctrl+right binding of its own).
fn real_button(game: &Game<'_>, button: i32, modifiers: u32) -> i32 {
    if cfg!(target_os = "macos")
        && game.settings.ctrl_click_emulates_right_click
        && button == 1
        && modifiers & m::MODS == m::CTRL
    {
        0
    } else {
        button
    }
}

/// Presses: the merchant's offers, the recipe view and storing recipes, right-click on the
/// crafting result, Shift+click with a stack in hand, and Item Scroller's click and drag moves.
pub(super) fn on_press(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, modifiers: u32, canceled: &mut bool) {
    with(|s| {
        s.drag = None;
        s.hover = None;
    });
    if !game.native_on(m::ID) || screen.id != "inventory" {
        return;
    }
    let cfg = config(game);
    if !active(game, &cfg) {
        return;
    }
    let raw = button;
    let mut take = || {
        *canceled = true;
        with(|s| s.swallow = Some(raw));
    };
    let mods = modifiers & m::MODS;
    let button = real_button(game, button, modifiers);
    if game.station.as_ref().is_some_and(|s| s.merchant.is_some()) {
        if cfg.villager
            && let Some(index) = offer_under(game, screen.mouse)
        {
            if button == 2 {
                toggle_favourite(game, index, keys::held(game, "villager_global"));
                take();
            } else if button == 1 && game.carried.is_none() {
                if !busy() {
                    enqueue(game, Work::Trade(m::TradeMax::new(index)));
                }
                take();
            }
        }
        return;
    }
    if cfg.crafting
        && crafting(game).is_some()
        && view_open(game, &cfg)
    {
        if let Some(i) = view_entry_under(game, screen.mouse) {
            take();
            recipe_click(game, i, button, mods);
            return;
        }
        if game.inventory_target().0 == Some(m::CRAFT_RESULT) && keys::button_is(game, "store_recipe", button) {
            take();
            store_recipe(game);
            return;
        }
    }
    let (slot, outside) = game.inventory_target();
    if cfg.right_click_stack
        && button == 1
        && mods & m::SHIFT == 0
        && slot == Some(m::CRAFT_RESULT)
        && crafting(game).is_some()
        && game.remote.is_some()
    {
        let result = game.craft_result();
        let fits = match (&game.carried, &result) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(c), Some(r)) => c.same_item_same_components(r),
        };
        if fits {
            if !busy() {
                enqueue(game, Work::CraftStack(m::CraftStack::default()));
            }
            take();
        }
        return;
    }
    if !plain_menu(game) {
        return;
    }
    if let Some(carried) = game.carried.as_ref() {
        if button == 0 && mods == m::SHIFT {
            if cfg.shift_place
                && let Some(slot) = slot.filter(|&s| m::movable(s) && peek(game, s).is_none())
            {
                // Put the stack down, then bring every matching stack from the other side.
                enqueue(game, Work::Batches(VecDeque::from([vec![m::pickup(slot)]])));
                enqueue(game, Work::Stacks { slot, matching: true, to_other: false, first: false });
                take();
            } else if cfg.shift_drop
                && slot.is_none()
                && outside
                && let Some(side) = with(|s| s.source)
            {
                enqueue(game, Work::Batches(VecDeque::from([vec![m::drop_hand()]])));
                enqueue(game, Work::DropLike { side, like: carried.clone() });
                take();
            }
        }
        return;
    }
    let Some(slot) = slot.filter(|&s| m::movable(s)) else {
        return;
    };
    // Where a stack picked up next comes from (Shift+click outside drops its matching stacks).
    with(|s| s.source = Some(slot));
    let Some(action) = m::click_action(button, mods, keys::held_keys(game)).filter(|&a| action_on(a, &cfg)) else {
        return;
    };
    // Item Scroller takes the press (vanilla's click and Horizon's Mouse Tweaks don't see it).
    take();
    if busy() {
        return;
    }
    enqueue(game, action_work(action, slot));
    if action.drags() {
        with(|s| s.drag = Some(Drag { action, button: raw, last: Some(slot), visited: vec![slot] }));
    }
}

fn toggle_favourite(game: &Game<'_>, index: usize, global: bool) {
    let Some((vid, items)) = villager(game) else {
        return;
    };
    let Some(trade) = items.get(index) else {
        return;
    };
    let mut f = favourites(game);
    let now = f.toggle(m::Favourites::key((!global).then_some(vid.as_str()), trade));
    game.native_set_setting(m::ID, m::FAVOURITES_KEY, &f.to_text());
    with(|s| s.favourites = Some(f));
    notice(match (now, global) {
        (true, false) => "Favourite trade",
        (true, true) => "Favourite trade (every villager)",
        (false, _) => "No longer a favourite",
    });
}

/// A click on recipe `i` in the recipe view: left selects it (clearing what isn't in it from the
/// grid) or, when selected, fills one set (Shift: stacks); right does the same, then crafts (one
/// set to the inventory, Shift all, with the drop key held thrown); middle clears the grid.
fn recipe_click(game: &Game<'_>, i: usize, button: i32, mods: u32) {
    if button == 2 {
        if !busy() {
            enqueue(game, Work::ClearGrid);
        }
        return;
    }
    if button != 0 && button != 1 {
        return;
    }
    let mut b = book(game);
    let changed = b.selected != i;
    b.selected = i;
    store_book(game, b.clone());
    let Some(recipe) = b.get(i).cloned() else {
        return;
    };
    if busy() || game.carried.is_some() {
        return;
    }
    let shift = mods & m::SHIFT != 0;
    let mode = if shift { m::FillMode::Max } else { m::FillMode::One };
    if button == 0 {
        if changed {
            enqueue(game, Work::ClearWrong(recipe));
        } else {
            enqueue(game, Work::Fill { recipe, mode, rounds: 0 });
        }
        return;
    }
    if changed {
        enqueue(game, Work::ClearWrong(recipe.clone()));
    }
    let take = match (keys::held(game, "drop_key"), shift) {
        (true, true) => m::Take::DropAll,
        (true, false) => m::Take::DropOne,
        (false, true) => m::Take::Shift,
        (false, false) => m::Take::One,
    };
    enqueue(game, Work::Craft(m::CraftJob::new(recipe, mode, take, 1)));
}

/// The recipe in the grid goes to the selected recipe slot; an empty result clears that slot.
fn store_recipe(game: &Game<'_>) {
    let Some(n) = crafting(game) else {
        return;
    };
    let mut b = book(game);
    let i = b.selected;
    let result = game.craft_result();
    let Some(result) = result else {
        b.set(i, None);
        store_book(game, b);
        notice(format!("Recipe {} cleared", i + 1));
        return;
    };
    let grid: Vec<Option<String>> = (0..n * n).map(|c| game.craft_grid[c].as_ref().map(|s| s.item.to_string())).collect();
    let Some(recipe) = m::Recipe::from_grid(n, &grid, Some((result.item.to_string(), result.count))) else {
        return;
    };
    b.set(i, Some(recipe));
    store_book(game, b);
    notice(format!("Recipe {} stored", i + 1));
}

/// The release of a press the mod took is the mod's too; a drag ends.
pub(super) fn on_release(game: &Game<'_>, button: i32, canceled: &mut bool) {
    let shift_only = game.has_shift_down() && !game.has_control_down() && !game.has_alt_down();
    let at = [game.cursor.0, game.cursor.1];
    with(|s| {
        // A Shift+click (or Shift+drag) of stacks goes on over what's hovered while Shift stays held.
        s.hover = s
            .drag
            .take()
            .filter(|d| d.action == m::Action::Stacks && d.button == button && shift_only && game.carried.is_none())
            .and_then(|d| Some(Hover { first: *d.visited.first()?, side: None, visited: d.visited, at }));
        if s.swallow == Some(button) {
            s.swallow = None;
            *canceled = true;
        }
    });
}

/// A click-drag move goes on over every slot the pointer passes while its keys stay held.
pub(super) fn on_drag(game: &mut Game<'_>, screen: &str, mouse: [f32; 2], button: i32, delta: [f32; 2], modifiers: u32, canceled: &mut bool) {
    let Some(mut d) = with(|s| s.drag.clone()) else {
        return;
    };
    let real = real_button(game, button, modifiers);
    let still = game.native_on(m::ID)
        && screen == "inventory"
        && button == d.button
        && game.carried.is_none()
        && m::click_action(real, modifiers, keys::held_keys(game)) == Some(d.action);
    if !still {
        with(|s| s.drag = None);
        return;
    }
    // The drag is Item Scroller's: neither vanilla's nor Mouse Tweaks' drag hears it.
    *canceled = true;
    let Some(batch) = gui_batch(game) else {
        return;
    };
    let from = [mouse[0] - delta[0], mouse[1] - delta[1]];
    for p in m::line_points(from, mouse, 4.0) {
        let slot = game.slot_at(&batch, p[0], p[1]).filter(|&s| m::movable(s));
        if let Some(slot) = slot
            && d.last != Some(slot)
            && !(d.action.once_per_slot() && d.visited.contains(&slot))
        {
            enqueue(game, action_work(d.action, slot));
            d.visited.push(slot);
        }
        d.last = slot;
    }
    with(|s| s.drag = Some(d));
    run(game, false);
}

/// Every frame: the Shift+hover move after a Shift+click (see `Hover`).
pub(super) fn on_frame(game: &mut Game<'_>) {
    let Some(mut h) = with(|s| s.hover.take()) else {
        return;
    };
    let cfg = config(game);
    let still = active(game, &cfg)
        && cfg.drag_stacks
        && plain_menu(game)
        && game.carried.is_none()
        && game.has_shift_down()
        && !game.has_control_down()
        && !game.has_alt_down()
        && m::click_action(0, m::SHIFT, keys::held_keys(game)) == Some(m::Action::Stacks);
    if !still {
        return;
    }
    let Some(batch) = gui_batch(game) else {
        return;
    };
    let first = h.first;
    let side = h.side.get_or_insert_with(|| m::same_side(first, &game.slots())).clone();
    let now = [game.cursor.0, game.cursor.1];
    if now != h.at {
        for p in m::line_points(h.at, now, 4.0) {
            let hovered = game.slot_at(&batch, p[0], p[1]);
            if let Some(slot) = m::hover_pick(hovered, &side, &h.visited) {
                enqueue(game, Work::Shift(slot));
                h.visited.push(slot);
            }
        }
        h.at = now;
    }
    with(|s| s.hover = Some(h));
    run(game, false);
}

/// Keys: the recipe view's selection keys, Ctrl+Shift+Q, the crafting keys, trading favourites,
/// sorting, the on/off key and the slot log.
pub(super) fn on_key(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, repeat: bool, modifiers: u32, canceled: &mut bool) {
    if !game.native_on(m::ID) || screen.id != "inventory" || game.recipe_search_focused() {
        return;
    }
    let mut cfg = config(game);
    if !repeat && keys::pressed(game, "toggle_mod", key, modifiers) {
        cfg.enabled = !cfg.enabled;
        notice(if cfg.enabled { "Item Scroller on" } else { "Item Scroller off" });
        store_config(game, cfg);
        *canceled = true;
        return;
    }
    if !active(game, &cfg) {
        return;
    }
    if view_open(game, &cfg) {
        let mut b = book(game);
        if let Some(n) = m::recipe_nav(key, modifiers & m::SHIFT != 0, b.selected) {
            b.selected = n;
            store_book(game, b);
            *canceled = true;
            return;
        }
    }
    if repeat {
        return;
    }
    let hovered = game.inventory_target().0;
    if keys::pressed(game, "drop_all", key, modifiers) && cfg.drop_all && plain_menu(game) && game.carried.is_none() {
        if let Some(slot) = hovered.filter(|&s| m::movable(s)) {
            if !busy() {
                enqueue(game, Work::DropMatching(slot));
            }
            *canceled = true;
        }
        return;
    }
    if keys::pressed(game, "slot_debug", key, modifiers) {
        log::info!("Item Scroller: screen {} slot {hovered:?} holds {:?}", screen_key(game), hovered.and_then(|s| peek(game, s)));
        *canceled = true;
        return;
    }
    if keys::pressed(game, "mass_craft_toggle", key, modifiers) {
        cfg.mass_craft_hold = !cfg.mass_craft_hold;
        notice(if cfg.mass_craft_hold { "Mass craft: on without holding" } else { "Mass craft: hold the key" });
        store_config(game, cfg);
        *canceled = true;
        return;
    }
    if keys::pressed(game, "sort", key, modifiers) && cfg.sort && plain_menu(game) && game.carried.is_none() {
        if let Some(slot) = hovered.filter(|&s| m::is_player(s) || m::is_container(s)) {
            if !busy() {
                enqueue(game, Work::Sort(slot));
            }
            *canceled = true;
        }
        return;
    }
    if keys::pressed(game, "trade_favorites", key, modifiers) && cfg.villager {
        if let Some((vid, items)) = villager(game) {
            if !busy() && game.carried.is_none() {
                for i in favourites(game).of(&vid, &items).0 {
                    enqueue(game, Work::Trade(m::TradeMax::new(i)));
                }
            }
            *canceled = true;
        }
        return;
    }
    if !cfg.crafting || crafting(game).is_none() {
        return;
    }
    let recipe = book(game).selected_recipe().cloned().filter(|r| r.result.is_some());
    if keys::pressed(game, "mass_craft", key, modifiers) {
        // The tick runs it while the keys are held.
        *canceled = true;
    } else if keys::pressed(game, "craft_everything", key, modifiers) {
        *canceled = true;
        if let Some(r) = recipe
            && !busy()
            && game.carried.is_none()
        {
            enqueue(game, Work::Craft(m::CraftJob::everything(r)));
        }
    } else if keys::pressed(game, "throw_results", key, modifiers) {
        *canceled = true;
        if let Some((item, _)) = recipe.and_then(|r| r.result)
            && !busy()
        {
            enqueue(game, Work::ThrowResults(item));
        }
    } else if keys::pressed(game, "move_results", key, modifiers) {
        *canceled = true;
        if let Some((item, _)) = recipe.and_then(|r| r.result)
            && !busy()
        {
            enqueue(game, Work::MoveResults(item));
        }
    }
}

// ── drawing ──

fn frame(game: &Game<'_>, batch: &mut HudBatch, r: [f32; 4], edge: u32, inside: u32) {
    let [x, y, w, h] = r;
    game.native_fill(batch, [x, y, w, h], edge);
    game.native_fill(batch, [x + 1.0, y + 1.0, w - 2.0, h - 2.0], inside);
}

/// A small star, 5 x 4 pixels.
fn star(game: &Game<'_>, batch: &mut HudBatch, x: f32, y: f32, colour: u32) {
    const ROWS: [&str; 4] = ["..#..", "#####", ".###.", ".#.#."];
    for (r, row) in ROWS.iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            if ch == '#' {
                game.native_fill(batch, [x + c as f32, y + r as f32, 1.0, 1.0], colour);
            }
        }
    }
}

pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !game.native_on(m::ID) || screen.id != "inventory" {
        return;
    }
    let cfg = config(game);
    if !active(game, &cfg) {
        return;
    }
    let (ox, oy) = game.inventory_origin(batch);
    if let Some(merchant) = game.station.as_ref().and_then(|s| s.merchant.as_ref())
        && cfg.villager
        && let Some((vid, items)) = villager(game)
    {
        let (favs, global) = favourites(game).of(&vid, &items);
        for i in 0..7 {
            let index = i + merchant.scroll;
            if favs.contains(&index) {
                let (x, y) = (ox + MERCHANT_SHIFT + 5.0, oy + 18.0 + 20.0 * i as f32);
                if cfg.highlight {
                    game.native_fill(batch, [x, y, 88.0, 20.0], 0x40FF_C83C);
                }
                star(game, batch, x + 81.0, y + 2.0, if global { PURPLE } else { GOLD });
            }
        }
    }
    if view_open(game, &cfg) {
        draw_view(game, batch, screen.mouse);
    }
    if let Some((text, at)) = with(|s| s.notice.clone()) {
        if at.elapsed().as_secs_f32() < 3.0 {
            let w = game.native_text_width(&text);
            let x = ox + (176.0 - w) / 2.0;
            game.native_fill(batch, [x - 3.0, oy - 14.0, w + 6.0, 12.0], 0xA000_0000);
            game.native_text(batch, &text, [x, oy - 12.0], 0xFFFFFF, true);
        } else {
            with(|s| s.notice = None);
        }
    }
}

/// Item Scroller's recipe view: the page of stored recipes left of the window, numbered, the
/// selected one framed, its grid beside them, the page and a hint above.
fn draw_view(game: &Game<'_>, batch: &mut HudBatch, mouse: [f32; 2]) {
    let b = book(game);
    let v = view_layout(game, batch);
    let page = b.selected / m::PAGE;
    let first = page * m::PAGE;
    game.native_text(batch, &format!("Page {}/{}", page + 1, m::PAGES), [v.x, 2.0], 0xFFFFFF, true);
    let mut hovered = None;
    for i in 0..m::PAGE {
        let index = first + i;
        let [x, y, w, h] = v.entry(i);
        if index == b.selected {
            frame(game, batch, [x - 1.0, y - 1.0, w + 2.0, h + 2.0], GOLD, 0x8000_0000);
        } else {
            game.native_fill(batch, [x, y, w, h], 0x6000_0000);
        }
        let label = format!("{}", index + 1);
        let tw = game.native_text_width(&label);
        game.native_text(batch, &label, [x - tw - 2.0, y + (h - 8.0) / 2.0], 0xC0C0C0, true);
        if let Some((item, _)) = b.get(index).and_then(m::Recipe::icon) {
            game.native_item(batch, &item, [x, y], w);
        }
        if mouse[0] >= x && mouse[0] < x + w && mouse[1] >= y && mouse[1] < y + h {
            hovered = Some(index);
        }
    }
    // The selected (or hovered) recipe's grid.
    let shown = hovered.unwrap_or(b.selected);
    if let Some(r) = b.get(shown) {
        let ([gx, gy], size) = v.grid();
        for row in 0..3 {
            for col in 0..3 {
                let (cx, cy) = (gx + col as f32 * size, gy + row as f32 * size);
                game.native_fill(batch, [cx, cy, size - 1.0, size - 1.0], 0x8040_4040);
                if row < r.h
                    && col < r.w
                    && let Some(item) = &r.cells[row * r.w + col]
                {
                    game.native_item(batch, item, [cx, cy], size - 1.0);
                }
            }
        }
    }
}

// ── Options > Mods ──

pub(super) fn on_list(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: keys::listed(),
    });
}

pub(super) fn on_settings(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != m::ID {
        return;
    }
    buttons.extend(config(game).labels());
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID {
        return;
    }
    let mut cfg = config(game);
    cfg.click(button);
    store_config(game, cfg);
}
