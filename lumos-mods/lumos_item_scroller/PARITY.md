# Lumos Item Scroller: parity with Item Scroller

What Item Scroller (masa, Fabric, `LTS/1.21.11` of sakura-ryoko/itemscroller, the line that ships for
1.21.11) does, with its default keys, and where this mod stands. Sources: its Modrinth/CurseForge
description, its `Hotkeys.java` / `Configs.java` defaults, its `en_us.json` option texts, and its input
handling, read for behaviour only. No Java code or assets are used here; this is original Rust.

Status words: **done** (was there before this rework and kept), **added** (added in this rework),
**changed** (existed, now follows Item Scroller), **removed** (a Lumos extra Item Scroller doesn't
have), **not possible** (and why), **not done** (possible, left out, and why).

The modifier keys are fixed as Item Scroller's defaults; the plain keys (Q, W, S, A, C, T, M, R, I,
Space, the middle button) are rebindable on the mod's Options > Mods page (`keys.rs`), and in
Options > Controls with Lumos Controls on. "Left" is the left mouse button. On macOS Alt is Option.

## Moving items with clicks and drags (hand empty)

| Item Scroller hotkey (id) | Default | What it does | Here |
|---|---|---|---|
| `keyDragMoveOne` | Ctrl + Left (click, drag) | One item from each slot clicked or dragged over goes to the other inventory (once per slot per drag) | **added** |
| `keyDragMoveMatching` | Alt + Left (click, drag) | Every stack matching the clicked/dragged-over stack, in its inventory, goes to the other inventory | **added** (was Ctrl+Shift+click: **changed**) |
| `keyDragMoveStacks` | Shift + Left (click, drag) | Each slot clicked or dragged over is shift-clicked | **changed**: the press is now taken too (as Item Scroller), so Horizon's Mouse Tweaks doesn't also act |
| `keyDragMoveLeaveOne` | Shift + Right (click, drag) | All but one item of each slot goes to the other inventory | **added** |
| `keyMoveEverything` | Alt + Shift + Left (click) | Every stack of the clicked slot's inventory goes to the other inventory | **added** (also Shift + Space + Left, the older default, as `move_everything_old`) |
| `keyDragDropSingle` | Q + Left (click, drag) | Drops one item from each slot | **added** |
| `keyDragDropStacks` | Shift + Q + Left (click, drag) | Drops each whole stack | **added** |
| `keyDragDropLeaveOne` | Shift + Q + Right (click, drag) | Drops all but one item of each stack | **added** |
| `wsMoveUpSingle` / `wsMoveDownSingle` | W + Left / S + Left | One item to the furthest slot above / below that takes it | **added** |
| `wsMoveUpStacks` / `wsMoveDownStacks` | Shift + W + Left / Shift + S + Left | The stack to the furthest slots above / below | **added** |
| `wsMoveUpMatching` / `wsMoveDownMatching` | Alt + W + Left / Alt + S + Left | Every matching stack of the inventory up / down | **added** |
| `wsMoveUpLeaveOne` / `wsMoveDownLeaveOne` | W + Right / S + Right | All but one up / down | **added** |
| `enableShiftPlaceItems` | Shift + Left on an empty slot with a stack in hand | Puts the stack down, then brings every matching stack from the other inventory into this one | **added** |
| `enableShiftDropItems` | Shift + Left outside the window with a stack in hand | Drops it and every matching stack of the inventory it came from | **added** |
| `dropAllMatching` (`enableDropkeyDropMatching`) | Ctrl + Shift + Q over a stack | Drops every matching stack of that inventory | **done** |

Modifiers are matched exactly, as Item Scroller's keybinds (no extra keys): Ctrl + Shift + Left is
none of these and stays vanilla's shift-click.

### macOS and Ctrl + click

On macOS the system turns Control-click into a secondary click in most apps. Horizon (9f42064c)
does that only when its "Ctrl+click emulates right click" option is on (`ctrl_click_emulates_right_click`,
off by default). With it off, winit delivers Ctrl + Left as button 0 with the Control bit (2) in the
hook's modifiers, so Ctrl + click moves one item. With it on, the same press arrives as button 1 with
the Control bit; the mod then treats a Ctrl + Right press over a slot as Ctrl + Left (a real Ctrl + right
click can't be told apart there, and Item Scroller has no Ctrl + Right binding).

## The mouse wheel (hand empty, over a stack)

| Item Scroller | Default | Here |
|---|---|---|
| `enableScrollingSingle` | Wheel: one item; up moves it to the other inventory, down brings one in | **done** (Horizon's Mouse Tweaks no longer keeps the plain wheel: Item Scroller's direction wins while this is on) |
| `enableScrollingStacks`, `modifierMoveStack` | Shift + wheel: one matching stack (the hovered one last) | **changed** (was "the hovered stack") |
| `enableScrollingMatchingStacks`, `modifierMoveMatching` | Alt + wheel: every matching stack | **changed** (was Ctrl + wheel) |
| `enableScrollingEverything`, `modifierMoveEverything` | Alt + Shift + wheel: everything | **added** |
| `reverseScrollDirectionSingle` / `...Stacks` | off | **added** (replace the old three-way direction setting; old settings are read) |
| `useSlotPositionAwareScrollDirection` | off: up moves "up the screen" | **changed**: uses the slots' screen positions as Item Scroller does |
| `enableScrollingVillager` | Wheel over the trade result: down fills the payment (Shift: full stacks), up takes a trade (Shift: all you can pay for; over an empty result, Shift + up empties the payment) | **added** |
| Crafting output (`enableCraftingFeatures`) | Wheel over the crafting result: up takes one craft (Shift: shift-click, Alt + Shift: craft everything), up over an empty result clears the grid; down fills the grid with the selected recipe (Shift: stacks) | **added** |

## Crafting

| Item Scroller | Default | Here |
|---|---|---|
| Recipe view (`recipeView`) | Hold **A** in a crafting screen (the inventory's 2x2 or a crafting table): the stored recipes show left of the window, 18 a page (two columns of 9), 8 pages; the selected recipe's grid beside them | **changed**: before, Lumos showed a column of recipe slots all the time. Now only while A is held, only in those two screens |
| Store a recipe (`storeRecipe`) | Middle-click the crafting result while the view is open: stored in the selected slot; on an empty result, clears the selected slot | **changed** |
| Selecting | While the view is open: click a recipe, 1-9, arrows (left/right a half page, with Shift a page), the wheel | **added** |
| Click a recipe | Left: select it (clearing items that aren't in it from the grid), again: fill the grid with one set (Shift: stacks). Right: the same, then craft (one set to the inventory; Shift: shift-click; with the drop key held: drop one / Shift: drop all). Middle: clear the grid | **changed** |
| `enableRightClickCraftingOneStack` | Right-click the crafting result: craft up to a full stack in hand | **added** (servers only: Horizon's local world takes only shift-clicks on a result slot) |
| `craftEverything` | Ctrl + C | Fill with stacks, craft until the ingredients run out | **added** |
| `massCraft` | Ctrl + Alt + C, held | While held: fill, take the result, again | **changed** (was a Lumos hotkey with no default). Results are dropped, as Item Scroller's default (`massCraftSwapsOnly`/recipe book off) mode does; a switch keeps them in the inventory instead |
| `massCraftToggle` / `massCraftHold` | unbound / off | Mass craft keeps going without holding the key | **added** |
| `throwCraftResults` | Ctrl + T | Drops every stack of the selected recipe's result from the inventories | **added** |
| `moveCraftResults` | Ctrl + M | Moves every stack of the result from the player inventory to the other one | **added** |
| `craftingRecipesSaveToFile` / `...IsGlobal` | on / off | Recipes per world or server (or global) in the mod's data folder | **done** / **added** |
| `massCraftUseRecipeBook`, `massCraftSwapsOnly`, `massCraftInterval`, `massCraftIterations`, `recipeBookFailureLimit`, `useRecipeCaching`, `carpetCtrlQCraftingEnabledOnServer`, `clientCraftingFixOn1.12` | | **not done**: alternative packet strategies and Java-side caches. Here every step waits for the server's answer instead, at the click rate |

## Villagers

| Item Scroller | Default | Here |
|---|---|---|
| Favourite a trade | Middle-click a trade button | **changed**: per villager (as Item Scroller); was per profession |
| Global favourite | Shift + middle-click | **added** (used when the villager has no favourites of its own; drawn with a purple star) |
| Trade everything | Right-click a trade button | **changed** (was Shift + click: **removed**) |
| `villagerTradeFavorites` | unbound | Trades every favourite as far as you can pay | **added** |
| Favourites at the top of the list | on | **not possible** here without patching Horizon: its merchant screen sends the list index of the clicked button as the trade id, so reordering the client's list would select the wrong trade. Favourites get a star and a highlight in place instead |
| `villagerTradeListRememberScrollPosition` | on | **added** (per villager, in the mod's data folder) |
| `enableVillagerTradeFeatures` | on | **done** (one switch for these) |

A villager is known by its profession and its first two (novice) trades, which never change: Horizon
9f42064c doesn't keep the merchant's UUID on the client.

## Other

| Item Scroller | Default | Here |
|---|---|---|
| `sortInventory` (`sortInventoryToggle`) | R, off | **added**, off by default: merges and sorts the hovered inventory (main inventory without the hotbar, or the container) by item id |
| Sorting options (category order, top/bottom priority lists, shulker boxes and bundles last, sort method) | | **not done**: lists of item names need a text editor the settings page doesn't have |
| `toggleModOnOff` | unbound | **added** |
| `modMainToggle` | on | **done** (the mod's switch in Options > Mods) |
| `openConfigGui` | I + C | **not possible**: a native mod can't open Options > Mods on its own page at 9f42064c. Use Options > Mods |
| `slotDebug` | Ctrl + Alt + Shift + I | **added** (writes the hovered slot to the log) |
| `rateLimitClickPackets`, `packetRateLimit` | off, 4 | **done**: always on here, 4 clicks a tick by default (1 to 32) |
| `enableItemMovingFallback` | off | **not done**: Horizon's menus all take shift-clicks |
| Screen blacklist (`GUI_BLACKLIST`) | empty | **added**: `blacklist=` in the mod's config file (`horizon-mods/.horizon-mod-data/lumos_item_scroller/config`), menu type ids such as `minecraft:generic_9x3`, comma-separated |
| Debug messages, translation options | | **not done** (not features) |

## Removed Lumos extras

- The always-visible recipe slot column and its click meanings (click fill, right-click fill all,
  Shift + click mass craft, Alt + click store, Shift + right-click clear).
- Shift + click a trade to trade max (Item Scroller uses right-click), Alt + click as a middle click.
- Clearing the grid after a mass craft.

## Horizon's Mouse Tweaks

Horizon has a built-in Mouse Tweaks (`extras/mouse_tweaks.rs`, off by default). Where they overlap,
Item Scroller's behaviour wins while its feature is on: the wheel over a slot (Item Scroller cancels
the wheel before Mouse Tweaks hears it) and Shift + left drag with an empty hand (Item Scroller takes
the press, so Mouse Tweaks never starts its drag and nothing moves twice). With the Item Scroller
feature off, Mouse Tweaks works as before. Mouse Tweaks' right-drag and left-drag with a stack in
hand don't overlap and stay its own.

## Clicks only in open screens

Every click goes through `Game::menu_ops` while an inventory screen is open; the queue is dropped
when the screen closes or another menu opens, and is checked again just before each click.
