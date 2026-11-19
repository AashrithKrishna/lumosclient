# Lumos World Map: plan and feature parity with Xaero's World Map

`lumos_world_map` is a fullscreen world map for Horizon 26.3 (pinned to `9f42064c`), made to work
with Lumos Map (`lumos_map`, the minimap) the way Xaero's World Map works with Xaero's Minimap.

## The legal rule

Xaero's mods are All Rights Reserved. This plan lists **behaviour** in our own words, taken only from
public descriptions: the World Map's Modrinth and CurseForge pages, its official changelog, and the
third-party guides already cited in `lumos_map/PARITY.md` (tags below are that file's). Nobody
working on this mod downloads, decompiles or copies Xaero's jars, code, textures, icons or text. The
look, file formats and code here are Lumos's own; where public sources don't say how something works,
this plan gives our own design.

Sources: MR-WM (Modrinth page), CF-WM (CurseForge page and FAQ), CL-WM (official changelog),
MR-MM/CL-MM (the minimap's pages, for the parts the two share), VE/HP (third-party guides). See
`lumos_map/PARITY.md`, "Sources", for the links.

## What lumos_map already had (Lumos Map 3.1.0)

Its world map page (M) has: explored chunks saved per world or server in `LMR2` region files, sub-worlds
by spawn (or one map), a region cache round you and the view, cave maps per dimension (Off, Layered,
Full) saved per 16-block band (`LMR3`), the right-click menu (waypoint here, teleport, edit, hide,
delete), other players, footsteps, another dimension's saved map, chunk grid, slime chunks, PNG export
of the view, a waypoint manager (U) with sets, death points, an editor, and a paged settings screen.
It draws both maps from 16 x 16 cells in the one 1024 x 1024 mod image slot, so the world map falls
back to coarse detail when zoomed out, and it keeps zoom in fixed steps.

`lumos_world_map` keeps those **files** (same folders, same formats), so the two mods share one map,
one set of waypoints and one sub-world choice, and adds what a separate, fuller world map needs.

## Architecture

- **Separate, no build dependency.** Each mod builds alone. They meet only on disk (Lumos Map's
  folders and files) and through each other's settings (read by key with `native_setting`). Whether
  the other is here and on: built in (`option_env!("HORIZON_MIXIN_MODS")`), allowed by Lumos's
  switches (`horizon_core::lumos_mods::on`, engine patch 04) and not turned off (`native_on`).
- **Shared formats, copied.** `runtime.rs` carries a copy of Lumos Map's tile/region code (`LMR1-3`),
  waypoint file (`Lumos Map waypoints 2`), `subworlds.txt` and `footsteps.txt`, kept byte for byte.
- **Own side file `LWX1`** (`r.x.z.lwx` beside `r.x.z.lmr`): per column the block seen from above,
  its biome and the block light over it, with a name table per file. Lumos Map ignores it (it only
  reads `.lmr`). Layout in `runtime.rs` ("LWX1").
- **One writer.** With this mod on, only it writes region files; each write merges with the file on
  disk (`merge_region`, `merge_extras`), on one worker thread, so tiles Lumos Map or an earlier
  session saved are never lost. Lumos Map keeps its own scans in memory and reads changed regions back.
- **Drawing.** 128 x 128 textures in Horizon's map slots (256 atlas slots of framed maps, claimed
  under negative ids no map uses), not the mod image slot, so the minimap's cells are never touched.
  Levels of detail 0 to 2 are composed from tiles of regions in memory; level 3 and up from
  per-region 64 x 64 overviews (8 blocks a texel) made off the frame from every region file, so
  zooming out never loads whole regions. Far textures are drawn under near ones while those load.
- **Memory limit.** "Regions kept in memory" (16/32/64/128): least recently wanted regions are
  saved and let go. Near detail is drawn only when the view's regions fit in the limit; else the
  overviews show.
- **Threads.** Colour table, region reads, overview indexing, region writes and PNG encoding are off
  the frame; scanning and texture composing run in the frame hook within time budgets.

## Feature list

Status: **Done**; **Partial** (some of it; why); **Not possible** (needs what Horizon 9f42064c or a
client-only mod can't have); **N/A** (another Java mod or a server mod).

### The fullscreen map

| Xaero's World Map feature (source) | Here | Status |
|---|---|---|
| Open with M, close with M or Escape (MR-WM) | `keys.rs` MAP; M on the map closes it | Done |
| Drag to pan (MR-WM) | Drag past a 3-pixel slop | Done |
| Wheel zoom about the pointer, smooth (MR-WM, CL-WM 1.13/1.42) | Continuous zoom easing to its target, anchored at the pointer; "Smooth zoom" switch; Shift for double steps | Done |
| Zoom buttons / keys (CL-WM 1.14) | Zoom +/- buttons; Page Up/Down on the map | Done |
| Zoom range near unlimited (CL-WM 1.42) | 1/16 to 128 blocks a pixel (settings) | Done |
| Arrow keys pan (CL-WM) | Arrows and WASD | Done |
| Centre on player, follow (CL-WM 1.42) | "On me" button and Home; Follow toggle; "Follow you when opened" | Done |
| Cursor coordinates (CL-WM 1.4.9) | Bottom bar: X Y Z | Done |
| Chunk and region at the cursor | Chunk (and block in it), region | Done |
| Biome at the cursor (CL-WM 1.29) | From the side file, else the live world | Done |
| Block at the cursor | From the side file, else the live world | Done |
| Hide the UI (F1, CL-WM 1.42) | `hide_ui` key (F1 on the map) | Done |
| Controls help (CL-WM 1.14) | Controls page with every key | Done |
| Zoom level shown (CL-WM 1.14) | Title bar | Done |
| Chunk grid | Toolbar and setting | Done |
| Region lines | Setting | Done |

### Mapping and saving

| Feature | Here | Status |
|---|---|---|
| Maps while you play, from loaded chunks; SP and MP (MR-WM) | Scans loaded chunks within a time budget (Light/Normal/Fast) | Done |
| Saved per world / server / dimension (MR-WM) | `lumos-map/<sp-or-mp>/<sub-world>/<dimension>/` | Done |
| Update loaded chunks / map new chunks toggles (CL-WM b1.3) | Two switches | Done |
| Multiworld: several maps per server address (MR-WM, CF-WM) | Sub-worlds in `subworlds.txt` (Lumos Map's) | Done |
| Map selection: by spawn, single, manual confirm (CF-WM) | "Server maps": Ask to confirm / By spawn / Single | Done |
| Map-selection / confirm screen (CF-WM) | Map page: the server's maps with spawns; Use this / New map / Later; mapping waits for the answer | Done |
| Quick confirm key (CF-WM) | `quick_confirm` key (unbound) | Done |
| Server tells the level id (CF-WM "Server" mode) | Needs a server-side mod | N/A |
| Optimised singleplayer generation from region files (MR-WM) | Reading the save's `.mca` files isn't exposed to native mods; maps loaded chunks only | Not possible |
| Resource packs regenerate the map (MR-WM) | New colours when the block sprites change; loaded chunks mapped again; saved tiles keep their colours until seen again | Partial |

### Look

| Feature | Here | Status |
|---|---|---|
| Accurate colours from textures (MR-WM) | Texture averages with biome tints (as Lumos Map) | Done |
| Vanilla map colours (MR-WM) | Switch; optional biome hue on map colours | Done |
| Terrain slopes / shading modes (HP, MR-WM) | Off, 2D (gentle), 3D (strong) | Done |
| Lighting, night (MR-WM, VE) | Darker with the Overworld sky; lit blocks glow (block light from the side file) | Done |
| Biome tints, blending (CL-WM 1.30.3) | Two switches | Done |
| Stained glass (HP) | See-through switch | Done |
| Flowers (VE "Load flowers") | Switch (off: the ground under them) | Done |
| Water depth (MR-WM) | Seabed shows through, darker with depth; switch | Done |
| Terrain depth / height colouring | Not in this version (Lumos Map has it on the minimap) | Partial |
| Look switches baked into tiles (glass, flowers, tints, water) | Loaded chunks mapped again on a switch; saved tiles keep how they were mapped | Partial |

### Caves

| Feature | Here | Status |
|---|---|---|
| Cave mode on the map (CL-WM 1.30) | Cave layers | Done |
| Auto cave mode underground (CL-WM 1.30.5) | A roof over the 3 x 3 columns round you, held a second; shows your layer | Done |
| Cave layers / start Y (CF-WM "Cave Mode Top Y") | 16-block bands stepped with `[` `]` and buttons; title shows "Caves Y top..bottom" | Done |
| Saved per layer (CF-WM `caves/<id>`) | `caves/<band>` (`LMR3`, Lumos Map's) | Done |
| Full cave mode (CF-WM) | `caves/full` read (Lumos Map writes it); Nether surface is under its roof | Partial |
| Depth, legible caves (CL-MM 20.9/23.4) | Settings, or the minimap's when "from the minimap" is on | Done |
| Maps caves while underground | Switch | Done |
| Above / below the Nether roof (CF-WM) | Under the roof only | Partial |
| Cave maps when the server forbids them | No server part | N/A |

### Dimensions

| Feature | Here | Status |
|---|---|---|
| View other dimensions' saved maps (CL-WM 1.37) | Button and N key: Overworld, Nether, End, then any dimension with a saved folder | Done |
| Nether x8 coordinates | View moved to the same place; cursor shows the other dimension's coordinates; your arrow at your converted place | Done |
| Waypoints converted between Overworld and Nether | Switch | Done |

### Waypoints (shared with Lumos Map)

| Feature | Here | Status |
|---|---|---|
| On the map, shared with the minimap (MR-WM) | Lumos Map's `waypoints.txt` and `global-waypoints.txt`, re-read when either mod writes | Done |
| Right-click menu: create at cursor (CL-WM 1.14) | Create (opens the editor), centre here | Done |
| Teleport where allowed (MR-WM) | `/tp @s x y z` when the player has permission level 2+ (and the switch) | Done |
| Edit, delete, disable (CL-WM 1.14, 1.42) | Editor (name, X/Y/Z, colour, set, enabled), delete with confirm, enable/disable | Done |
| Share in chat | Opens chat with "Name: x y z (Dimension)" | Done |
| Waypoint list / menu with filter (CL-WM 1.14) | List beside the map: All / This set / Death points / Hidden, nearest first, click centres | Done |
| Sets (MR-MM) | Set button cycles the current set; "All sets" | Done |
| Death points (MR-MM) | Shown (X mark); made by Lumos Map, or by this mod when Lumos Map is off | Done |
| Temporary waypoints dropped on leaving | When Lumos Map is off | Done |
| Waypoint at the pointer key (CL-WM 1.5) | B on the map | Done |
| Full manager (search, multi-select, symbols, custom colours) | Lumos Map's U (stays there) | Partial |

### Players

| Feature | Here | Status |
|---|---|---|
| Your arrow, colour (CL-WM 1.6.4) | Switch and 4 colours | Done |
| Other players (CF-WM) | Players the client has loaded, with names | Done |
| Tracked players beyond render distance (CF-WM, with a server mod or OPAC) | Needs a server or party mod | N/A |
| Claims, parties (OPAC) | Another Java mod | N/A |

### Toolbar, export, settings, keys

| Feature | Here | Status |
|---|---|---|
| Toolbar: dimension, caves, layer, waypoints, map, centre/follow, zoom, grid, PNG, settings, controls | Tool column | Done |
| Export PNG (MR-WM, CL-WM 1.32) | The view at 1:1 (up to 2048 blocks a side) or the whole map at 1:8 from overviews (up to 128 x 128 regions); `lumos-map/exports/` | Done |
| Settings screen | In the map and on Options > Mods: General, Look, Caves, Waypoints, Players, Performance, Minimap, Keys | Done |
| Keys through `keys.rs` | 14 keys, rebound on Options > Mods (and Controls with Lumos Controls) | Done |
| Server-enforced settings, item requirement, effects (MR-WM) | No server part | N/A |

### Performance

| Feature | Here | Status |
|---|---|---|
| Region caching (CL-WM 1.3) | Region cache with LRU | Done |
| Memory limit (CL-WM 1.41.2) | "Regions kept in memory" | Done |
| LOD for zoomed-out views | Per-region overviews, textures at levels 3-10 | Done |
| Background threads (CL-WM 1.3) | Reads, overviews, writes, PNG off the frame; scanning and composing in budgets on the frame | Partial |
| Map writing pace (CL-MM 20.28, CL-WM 1.11.3) | "Mapping speed" | Done |
| Overview memory | Every region's overview stays (16 KB each); not counted in the limit | Partial |

### Minimap integration

| Feature (Xaero) | Here | Status |
|---|---|---|
| Minimap uses the world map's chunks (MR-MM "Use World Map Chunks") | Lumos Map, with this mod on, stops writing regions and re-reads the ones this mod writes (10 s); its setting "With World Map: use its chunks" | Done |
| Shared cave settings | This map reads Lumos Map's cave depth and legibility ("from the minimap", default on) | Done |
| Minimap's enlarge key opens the world map | Lumos Map's Z opens this map when "The minimap's enlarge key opens this map" is on | Done |
| M on the minimap side opens the world map | Lumos Map's M and its map page hand over to this mod ("Open this map in place of the minimap's") | Done |
| Sub-world choice shared | This mod chooses; Lumos Map follows `subworlds.txt` | Done |
| Minimap shows the world map's cave textures (CL-MM 23.4) | Lumos Map reads saved bands itself | Done (already) |

## Changes in lumos_map

Small and switched by `world_map_on` (this mod built in, allowed, on); with it off or absent Lumos
Map does exactly what it did:
- M and its map page open `lumos_world_map`'s screen (if "Open this map in place of the minimap's").
- Its enlarge key opens the world map instead of enlarging (if "enlarge key opens this map").
- It stops writing explored and cave regions (this mod writes them, merged); with its new setting
  "With World Map: use its chunks" (World map page, on) it re-reads changed region files.
- It follows the sub-world in `subworlds.txt` instead of choosing by spawn.
- It re-reads `waypoints.txt`/`global-waypoints.txt` when their times change (its screen closed).

## Defaults (keys)

M open/close; quick confirm unbound; on the map: Page Up/Down zoom, V cave mode, `[`/`]` layers,
N dimension, Home centre, follow unbound, B waypoint at the pointer, U list, F1 hide bars, export
unbound.

## Not verified

Built and the runtime tests run; nothing has been tried in the game yet.
