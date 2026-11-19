// Lumos Item Scroller: between Horizon's `Game` and the rules in `lumos_item_scroller`.
//
// The press, release, key and render hooks only read the game (`&Game`), so what they ask for
// waits in a queue; the client tick (and the wheel and drag hooks, which lend the game mutably)
// sends it, at most the configured clicks a tick. Every click is an ordinary container click
// through `Game::menu_ops`: predicted and sent as `ContainerClick`s on a server (the integrated
// one too), run by the menu engine on a local world.
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

/// Something the mod is to do, worked out from the slots when its turn comes.
#[derive(Clone, Debug)]
enum Work {
    Batches(VecDeque<m::Batch>),
    /// Every stack like this slot's on its side, quick-moved.
    MoveMatching(usize),
    /// Every stack like this slot's in its inventory, thrown.
    DropMatching(usize),
    /// One item out of the slot, where a shift-click would send it.
    PushOne(usize),
    /// One item into the slot from the other side.
    PullOne(usize),
    /// One stack like the slot's from the other side, quick-moved.
    PullStack(usize),
    /// Every stack like the slot's on the other side, quick-moved.
    PullAll(usize),
    /// Recipe slot `i` into the grid; `rounds` of clearing done.
    Fill { recipe: m::Recipe, mode: m::FillMode, rounds: u32 },
    Craft(m::MassCraft),
    Trade(m::TradeMax),
}

#[derive(Default)]
struct State {
    config: Option<m::Config>,
    /// The recipe slots of the world they were read for.
    book: Option<(String, m::Book)>,
    favourites: Option<m::Favourites>,
    selected: Option<usize>,
    wheel: m::Wheel,
    limiter: m::Limiter,
    queue: VecDeque<Work>,
    /// The server's window id the queue was made in.
    window: i32,
    /// A press the mod took: its release is the mod's too.
    swallow: Option<i32>,
    /// A Shift+drag with an empty hand: the slots it has moved.
    drag: Option<Vec<usize>>,
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

fn notice(text: impl Into<String>) {
    with(|s| s.notice = Some((text.into(), Instant::now())));
}

/// `sp-<save folder>` or `mp-<address>`: the recipe slots are kept per world and server.
fn world_key(game: &Game<'_>) -> String {
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

fn enqueue(game: &Game<'_>, work: Work) {
    let window = game.remote_menu().0;
    with(|s| {
        if s.queue.is_empty() {
            s.window = window;
        }
        // Wheel turns can pile up: keep them to a few.
        if s.queue.len() < 16 {
            s.queue.push_back(work);
        }
    });
}

fn busy() -> bool {
    with(|s| s.queue.iter().any(|w| matches!(w, Work::Fill { .. } | Work::Craft(_) | Work::Trade(_))))
}

// ── the screen ──

/// The GUI's size as the screen hooks see it, for the hooks that are not handed a batch.
fn gui_batch(game: &Game<'_>) -> Option<HudBatch> {
    let r = game.renderer.as_ref()?;
    Some(HudBatch::new([r.config.width, r.config.height]))
}

/// A server's menus with plain click rules (the ones Horizon's Mouse Tweaks runs in, as wheel and
/// mass moves are made of the same clicks).
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

/// The crafting grid's side when the inventory (2x2) or a crafting table (3x3) is open.
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

/// The panel `[x, y, w, h]` and whether the recipe book is open beside it.
fn panel(game: &Game<'_>, batch: &HudBatch) -> ([f32; 4], bool) {
    let (ox, oy) = game.inventory_origin(batch);
    let book = game.recipe_book_shift(batch.width) > 0.0;
    ([ox, oy, 176.0, game.screen_height()], book)
}

fn recipe_slot_under(game: &Game<'_>, cfg: &m::Config, at: [f32; 2]) -> Option<usize> {
    let batch = gui_batch(game)?;
    let (rect, book) = panel(game, &batch);
    m::recipe_slot_at(rect, book, cfg.recipe_slots, at)
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
        (x >= rx && x < rx + 88.0 && y >= ry && y < ry + 20.0 && index < merchant.session.offers.len()).then_some(index)
    })
}

/// The favourite key of offer `index` of the open merchant.
fn offer_key(game: &Game<'_>, index: usize) -> Option<String> {
    let merchant = game.station.as_ref()?.merchant.as_ref()?;
    let offer = merchant.session.offers.get(index)?;
    let profession = if !merchant.session.profession.is_empty() {
        merchant.session.profession.clone()
    } else {
        merchant.name.clone().unwrap_or_else(|| "merchant".into())
    };
    let a = offer.cost_a(game.items());
    let b = offer.cost_b();
    Some(m::favourite_key(&profession, &a.item, b.as_ref().map(|s| &*s.item), &offer.result.item))
}

// ── slots ──

/// The open menu's slots and what they hold, with matching stacks numbered alike.
struct Snapshot {
    slots: Vec<usize>,
    stacks: Vec<(usize, Option<m::Stack>)>,
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
        Snapshot { slots, stacks }
    }
    fn get(&self, slot: usize) -> Option<m::Stack> {
        self.stacks.iter().find(|(s, _)| *s == slot).and_then(|(_, st)| st.clone())
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

/// The clicks go out; false when the open menu took none of them.
fn send(game: &mut Game<'_>, batch: &m::Batch) -> bool {
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
    m::CraftView { size: n, grid, result, inventory, carried: game.carried.is_some() }
}

fn trade_view(game: &mut Game<'_>, index: usize) -> Option<m::TradeView> {
    let offer = game.station.as_ref()?.merchant.as_ref()?.session.offers.get(index)?.clone();
    let result = game.slot_view(m::TRADE_RESULT);
    let payment: Vec<Option<ItemStack>> = (0..2).map(|i| game.slot_view(m::TRADE_RESULT - 50 + i)).collect();
    Some(m::TradeView {
        offer_result: offer.result.item.to_string(),
        out_of_stock: offer.out_of_stock(),
        result: stack_of(game, &result),
        payment: payment.iter().map(|p| stack_of(game, p)).collect(),
        carried: game.carried.is_some(),
    })
}

/// A wheel or mass action as clicks, from the slots as they are now.
fn resolve(game: &mut Game<'_>, work: &Work) -> Option<VecDeque<m::Batch>> {
    let snap = Snapshot::take(game);
    let get = |s: usize| snap.get(s);
    let table = game.craft_size == 3 && crafting(game).is_some();
    let batches: Vec<m::Batch> = match *work {
        Work::MoveMatching(slot) => m::move_matching(slot, &snap.slots, &get),
        Work::DropMatching(slot) => m::drop_matching(slot, &snap.slots, &get),
        Work::PushOne(slot) => {
            let st = get(slot)?;
            let target = landing(game, slot).first().copied().or_else(|| {
                let other = m::other_side(slot, &snap.slots, table);
                m::first_with_room(&st, &other, &get)
            })?;
            vec![m::move_one(slot, st.count, target)]
        }
        Work::PullOne(slot) => {
            let st = get(slot)?;
            let held = game.slot_view(slot)?;
            if st.count >= st.max || !takes_one(game, slot, &held) {
                return None;
            }
            let other = m::other_side(slot, &snap.slots, table);
            let mut source = None;
            for s in m::matching(&st, &other, &get) {
                if !is_result(game, s) {
                    source = Some(s);
                    break;
                }
            }
            let source = source?;
            vec![m::move_one(source, get(source)?.count, slot)]
        }
        Work::PullStack(slot) | Work::PullAll(slot) => {
            let st = get(slot)?;
            let other = m::other_side(slot, &snap.slots, table);
            let mut found: Vec<usize> = Vec::new();
            for s in m::matching(&st, &other, &get) {
                if !is_result(game, s) {
                    found.push(s);
                }
            }
            if matches!(work, Work::PullStack(_)) {
                found.truncate(1);
            }
            found.into_iter().map(|s| vec![m::quick_move(s)]).collect()
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
        if game.carried.is_some() && !matches!(work, Work::Batches(_) | Work::Craft(_) | Work::Trade(_)) {
            // The player holds something: the wheel and mass moves wait for an empty hand no more.
            continue;
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
            Work::Fill { recipe, mode, rounds } => {
                if !tick || stepped {
                    with(|s| s.queue.push_front(Work::Fill { recipe, mode, rounds }));
                    return;
                }
                stepped = true;
                let v = craft_view(game);
                match m::plan_fill(v.size, &v.grid, &recipe, &v.inventory, v.carried, mode) {
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
                if !tick || stepped {
                    with(|s| s.queue.push_front(Work::Craft(job)));
                    return;
                }
                stepped = true;
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
                    m::Step::Done(why) => notice(format!("Mass craft: {} crafts, {why}", job.crafted)),
                }
            }
            Work::Trade(mut job) => {
                if !tick || stepped {
                    with(|s| s.queue.push_front(Work::Trade(job)));
                    return;
                }
                stepped = true;
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
                    m::Step::Done(why) => notice(format!("Traded {} times: {why}", job.rounds)),
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

/// The client tick: the queue goes out at the configured rate, and goes when the screen does.
pub(super) fn on_tick(game: &mut Game<'_>) {
    if !game.native_on(m::ID) {
        return;
    }
    let window = game.remote_menu().0;
    let open = game.screen == Screen::Inventory;
    let cfg = config(game);
    let stale = with(|s| {
        if !open || (!s.queue.is_empty() && s.window != window) {
            s.queue.clear();
            s.drag = None;
            s.swallow = None;
            s.wheel = m::Wheel::default();
            return true;
        }
        s.limiter.tick(cfg.per_tick());
        false
    });
    if !stale {
        run(game, true);
    }
}

/// The wheel over a stack moves items: one, with Shift the stack, with Ctrl every matching stack.
pub(super) fn on_scroll(game: &mut Game<'_>, screen: &str, scroll: [f64; 2], modifiers: u32, canceled: &mut bool) {
    if !game.native_on(m::ID) || screen != "inventory" || scroll[1] == 0.0 {
        return;
    }
    let cfg = config(game);
    if !cfg.scroll || !plain_menu(game) || game.carried.is_some() {
        return;
    }
    let Some(slot) = game.inventory_target().0.filter(|&s| m::is_player(s) || m::is_container(s) || m::is_grid(s)) else {
        with(|s| s.wheel = m::Wheel::default());
        return;
    };
    let amount = m::amount(modifiers);
    // Horizon's Mouse Tweaks wheel keeps the plain wheel; the Shift and Ctrl ones are this mod's.
    let tweaks = game.settings.extras.mouse_tweaks_wheel && game.tweaks_on();
    if tweaks && amount == m::Amount::One {
        return;
    }
    let Some(stack) = game.slot_view(slot) else {
        return;
    };
    // A bundle keeps its own wheel (picking the item it gives).
    if stack.item.ends_with("bundle") {
        return;
    }
    *canceled = true;
    if busy() {
        return;
    }
    let steps = with(|s| s.wheel.steps(scroll[1]));
    if steps == 0 {
        return;
    }
    let slots = game.slots();
    let out = m::moves_out(f64::from(steps), cfg.direction, slot, &slots);
    for _ in 0..steps.unsigned_abs().min(8) {
        let work = match (amount, out) {
            (m::Amount::One, true) => Work::PushOne(slot),
            (m::Amount::Stack, true) => Work::Batches(VecDeque::from([vec![m::quick_move(slot)]])),
            (m::Amount::All, true) => Work::MoveMatching(slot),
            (m::Amount::One, false) => Work::PullOne(slot),
            (m::Amount::Stack, false) => Work::PullStack(slot),
            (m::Amount::All, false) => Work::PullAll(slot),
        };
        enqueue(game, work);
        if amount == m::Amount::All {
            break;
        }
    }
    run(game, false);
}

/// Presses: favourite and trade-max on the merchant's offers, the recipe slots, Ctrl+Shift+click,
/// and the start of a Shift+drag.
pub(super) fn on_press(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, modifiers: u32, canceled: &mut bool) {
    with(|s| s.drag = None);
    if !game.native_on(m::ID) || screen.id != "inventory" {
        return;
    }
    let cfg = config(game);
    let mut take = || {
        *canceled = true;
        with(|s| s.swallow = Some(button));
    };
    let shift = modifiers & m::SHIFT != 0;
    let ctrl = modifiers & m::CTRL != 0;
    let alt = modifiers & m::ALT != 0;
    if game.station.as_ref().is_some_and(|s| s.merchant.is_some()) {
        let Some(index) = offer_under(game, screen.mouse) else {
            return;
        };
        if cfg.favourites && (button == 2 || (button == 0 && alt)) {
            if let Some(key) = offer_key(game, index) {
                let mut f = favourites(game);
                let now = f.toggle(key);
                game.native_set_setting(m::ID, m::FAVOURITES_KEY, &f.to_text());
                with(|s| s.favourites = Some(f));
                notice(if now { "Favourite trade" } else { "No longer a favourite" });
            }
            take();
        } else if cfg.trade_max && button == 0 && shift && game.carried.is_none() {
            if !busy() {
                enqueue(game, Work::Trade(m::TradeMax::new(index)));
            }
            take();
        }
        return;
    }
    if cfg.recipes
        && let Some(n) = crafting(game)
        && let Some(i) = recipe_slot_under(game, &cfg, screen.mouse)
    {
        take();
        if button == 2 || (button == 0 && alt) {
            store_recipe(game, n, i);
            return;
        }
        if button == 1 && shift {
            let mut b = book(game);
            b.set(i, None);
            store_book(game, b);
            notice("Recipe slot cleared");
            return;
        }
        with(|s| s.selected = Some(i));
        let Some(recipe) = book(game).get(i).cloned() else {
            return;
        };
        if busy() || game.carried.is_some() {
            return;
        }
        match button {
            0 if shift && cfg.mass_craft => enqueue(game, Work::Craft(m::MassCraft::new(recipe))),
            0 => enqueue(game, Work::Fill { recipe, mode: m::FillMode::One, rounds: 0 }),
            1 => enqueue(game, Work::Fill { recipe, mode: m::FillMode::Max, rounds: 0 }),
            _ => {}
        }
        return;
    }
    if !plain_menu(game) || game.carried.is_some() || button != 0 {
        return;
    }
    let Some(slot) = game.inventory_target().0.filter(|&s| m::is_player(s) || m::is_container(s) || m::is_grid(s)) else {
        return;
    };
    if shift && ctrl && cfg.mass_move {
        if !busy() {
            enqueue(game, Work::MoveMatching(slot));
        }
        take();
    } else if shift && !ctrl && cfg.shift_drag && !game.tweaks_on() {
        // Vanilla's press moves this slot; the drag moves the ones entered after it.
        with(|s| s.drag = Some(vec![slot]));
    }
}

fn store_recipe(game: &Game<'_>, n: usize, i: usize) {
    let grid: Vec<Option<String>> = (0..n * n).map(|c| game.craft_grid[c].as_ref().map(|s| s.item.to_string())).collect();
    let result = game.craft_result().map(|r| (r.item.to_string(), r.count));
    let Some(recipe) = m::Recipe::from_grid(n, &grid, result) else {
        notice("Put a recipe in the grid to store it");
        return;
    };
    let mut b = book(game);
    b.set(i, Some(recipe));
    store_book(game, b);
    with(|s| s.selected = Some(i));
    notice("Recipe stored");
}

/// The release of a press the mod took is the mod's too.
pub(super) fn on_release(game: &Game<'_>, button: i32, canceled: &mut bool) {
    let _ = game;
    with(|s| {
        s.drag = None;
        if s.swallow == Some(button) {
            s.swallow = None;
            *canceled = true;
        }
    });
}

/// Shift+drag with an empty hand: each slot entered is shift-clicked (Horizon's Mouse Tweaks does
/// this itself when it is on, and then the mod leaves it to it).
pub(super) fn on_drag(game: &mut Game<'_>, screen: &str, button: i32, modifiers: u32, canceled: &mut bool) {
    let _ = canceled;
    if !game.native_on(m::ID) || screen != "inventory" || button != 0 {
        return;
    }
    if with(|s| s.drag.is_none()) {
        return;
    }
    if modifiers & m::SHIFT == 0 || game.carried.is_some() || game.tweaks_on() || !plain_menu(game) {
        with(|s| s.drag = None);
        return;
    }
    let Some(slot) = game.inventory_target().0.filter(|&s| m::is_player(s) || m::is_container(s) || m::is_grid(s)) else {
        return;
    };
    let new = with(|s| {
        let visited = s.drag.as_mut()?;
        (!visited.contains(&slot)).then(|| visited.push(slot))
    });
    if new.is_some() && game.slot_view(slot).is_some() {
        enqueue(game, Work::Batches(VecDeque::from([vec![m::quick_move(slot)]])));
        run(game, false);
    }
}

/// Ctrl+Shift+drop key over a stack drops every matching stack; the recipe keys.
pub(super) fn on_key(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, repeat: bool, modifiers: u32, canceled: &mut bool) {
    if !game.native_on(m::ID) || screen.id != "inventory" || repeat || game.recipe_search_focused() {
        return;
    }
    let cfg = config(game);
    let both = modifiers & (m::SHIFT | m::CTRL) == m::SHIFT | m::CTRL;
    if cfg.drop_all && both && keys::pressed(game, "drop_all", key) && plain_menu(game) && game.carried.is_none() {
        if let Some(slot) = game.inventory_target().0.filter(|&s| m::is_player(s) || m::is_container(s) || m::is_grid(s)) {
            if !busy() {
                enqueue(game, Work::DropMatching(slot));
            }
            *canceled = true;
        }
        return;
    }
    if !cfg.recipes {
        return;
    }
    let Some(n) = crafting(game) else {
        return;
    };
    if keys::pressed(game, "store_recipe", key) {
        if let Some(i) = recipe_slot_under(game, &cfg, screen.mouse) {
            store_recipe(game, n, i);
            *canceled = true;
        }
    } else if cfg.mass_craft && keys::pressed(game, "mass_craft", key) {
        *canceled = true;
        let selected = with(|s| s.selected);
        let Some(recipe) = selected.and_then(|i| book(game).get(i).cloned()) else {
            notice("Click a recipe slot first");
            return;
        };
        if !busy() && game.carried.is_none() {
            enqueue(game, Work::Craft(m::MassCraft::new(recipe)));
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
fn star(game: &Game<'_>, batch: &mut HudBatch, x: f32, y: f32) {
    const ROWS: [&str; 4] = ["..#..", "#####", ".###.", ".#.#."];
    for (r, row) in ROWS.iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            if ch == '#' {
                game.native_fill(batch, [x + c as f32, y + r as f32, 1.0, 1.0], GOLD);
            }
        }
    }
}

pub(super) fn on_render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !game.native_on(m::ID) || screen.id != "inventory" {
        return;
    }
    let cfg = config(game);
    let (rect, book_open) = panel(game, batch);
    if let Some(merchant) = game.station.as_ref().and_then(|s| s.merchant.as_ref())
        && cfg.favourites
    {
        let (ox, oy) = game.inventory_origin(batch);
        let favs = favourites(game);
        for i in 0..7 {
            let index = i + merchant.scroll;
            if index >= merchant.session.offers.len() {
                break;
            }
            if offer_key(game, index).is_some_and(|k| favs.has(&k)) {
                let (x, y) = (ox + MERCHANT_SHIFT + 5.0, oy + 18.0 + 20.0 * i as f32);
                if cfg.highlight {
                    game.native_fill(batch, [x, y, 88.0, 20.0], 0x40FF_C83C);
                }
                star(game, batch, x + 1.0, y + 1.0);
            }
        }
    }
    if cfg.recipes && crafting(game).is_some() {
        draw_recipes(game, batch, &cfg, rect, book_open, screen.mouse);
    }
    if let Some((text, at)) = with(|s| s.notice.clone()) {
        if at.elapsed().as_secs_f32() < 3.0 {
            let w = game.native_text_width(&text);
            let x = rect[0] + (rect[2] - w) / 2.0;
            game.native_fill(batch, [x - 3.0, rect[1] - 14.0, w + 6.0, 12.0], 0xA000_0000);
            game.native_text(batch, &text, [x, rect[1] - 12.0], 0xFFFFFF, true);
        } else {
            with(|s| s.notice = None);
        }
    }
}

fn draw_recipes(game: &Game<'_>, batch: &mut HudBatch, cfg: &m::Config, rect: [f32; 4], book_open: bool, mouse: [f32; 2]) {
    let b = book(game);
    let selected = with(|s| s.selected);
    let mut hovered = None;
    for i in 0..cfg.recipe_slots {
        let r = m::recipe_slot_rect(rect, book_open, cfg.recipe_slots, i);
        let edge = if selected == Some(i) { GOLD } else { 0xFF37_3737 };
        frame(game, batch, r, edge, 0xFF8B_8B8B);
        if let Some(recipe) = b.get(i) {
            let shown = recipe
                .result
                .clone()
                .or_else(|| recipe.cells.iter().flatten().next().map(|c| (c.clone(), 1)));
            if let Some((item, count)) = shown {
                game.draw_item(batch, &ItemStack::new(&item, count), r[0] + 1.0, r[1] + 1.0);
            }
        }
        if mouse[0] >= r[0] && mouse[0] < r[0] + r[2] && mouse[1] >= r[1] && mouse[1] < r[1] + r[3] {
            hovered = Some((i, r));
        }
    }
    let Some((i, r)) = hovered else {
        return;
    };
    let lines: Vec<&str> = if b.get(i).is_some() {
        vec!["Click: fill once", "Right-click: fill all", "Shift+click: mass craft", "Middle/Alt+click: store grid", "Shift+right-click: clear"]
    } else {
        vec!["Middle/Alt+click: store grid"]
    };
    let text_w = lines.iter().map(|l| game.native_text_width(l)).fold(0.0, f32::max);
    let (pw, ph) = match b.get(i) {
        Some(rc) => (rc.w as f32 * 18.0, rc.h as f32 * 18.0),
        None => (0.0, 0.0),
    };
    let w = text_w.max(pw) + 8.0;
    let h = ph + lines.len() as f32 * 10.0 + 8.0 + if ph > 0.0 { 4.0 } else { 0.0 };
    let x = if r[0] + 22.0 + w < batch.width { r[0] + 22.0 } else { r[0] - w - 4.0 };
    let y = r[1].min(batch.height - h - 2.0).max(2.0);
    frame(game, batch, [x, y, w, h], 0xF050_00FF, 0xF010_0010);
    let mut ty = y + 4.0;
    if let Some(rc) = b.get(i) {
        for row in 0..rc.h {
            for col in 0..rc.w {
                let (cx, cy) = (x + 4.0 + col as f32 * 18.0, ty + row as f32 * 18.0);
                game.native_fill(batch, [cx, cy, 16.0, 16.0], 0x40FF_FFFF);
                if let Some(item) = &rc.cells[row * rc.w + col] {
                    game.native_item(batch, item, [cx, cy], 16.0);
                }
            }
        }
        ty += ph + 4.0;
    }
    for l in lines {
        game.native_text(batch, l, [x + 4.0, ty], 0xFFFFFF, true);
        ty += 10.0;
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
    game.native_set_setting(m::ID, m::CONFIG_KEY, &cfg.serialize());
    with(|s| s.config = Some(cfg));
}
