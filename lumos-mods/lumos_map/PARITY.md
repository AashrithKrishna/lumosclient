# Lumos Map: feature parity with Xaero's Minimap and Xaero's World Map

What this is: a list of what Xaero's Minimap and Xaero's World Map do, taken only from their public
pages, a comparison with Lumos Map as it is on `port-1.21.11` (commit `1ac5567656`, Lumos Map 3.0.0,
Horizon `9f42064c`), and an ordered plan to close the gaps.

## The legal rule for this work

Xaero's mods are All Rights Reserved. This spec describes **behaviour** in our own words, from
public descriptions only (sources below). Nobody working on Lumos Map opens, decompiles or copies
their jars, code, textures, icons, frame art, translation files or config files, and nobody copies
their wording beyond a setting's name where we need it to say what we mean. Lumos Map keeps its own
look (its own frames, symbols, colours and layout) and its own file formats. Where public sources
don't say how something works, this spec says so and gives a Lumos design of our own; it never
guesses at their code.

## Sources

| Tag | Source | What it is |
|---|---|---|
| MR-MM | https://modrinth.com/mod/xaeros-minimap | Minimap's Modrinth description |
| CF-MM | https://www.curseforge.com/minecraft/mc-mods/xaeros-minimap | Minimap's CurseForge description |
| MR-WM | https://modrinth.com/mod/xaeros-world-map | World Map's Modrinth description |
| CF-WM | https://www.curseforge.com/minecraft/mc-mods/xaeros-world-map | World Map's CurseForge description and FAQ |
| CL-MM | https://chocolateminecraft.com/update.php?mod_id=0 | Minimap's official changelog (2019 to 2026) |
| CL-WM | https://chocolateminecraft.com/update.php?mod_id=2 | World Map's official changelog (2016 to 2026) |
| VE | https://wiki.vanillaeuropa.com/Xaero's_MiniMap | A server wiki: recommended settings, how cave mode feels in play |
| HP | https://www.minecraft-hosting.pro/article/183-xaeros-minimap-mod | A hosting company's guide listing the settings screens |

VE and HP are third-party guides: good for setting names and how things behave in play, weaker for
defaults. Changelog entries are cited as `CL-MM 22.6.0` (version) so they can be checked. Where two
sources disagree it is noted in the row.

Legend for the gap columns: **Have** (Lumos does it), **Partial** (some of it, see note), **Missing**,
**N/A** (needs something Lumos can't have, such as a server-side mod or another loader's mods),
**Blocked** (Horizon `9f42064c` can't support it, see [Horizon limits](#horizon-9f42064c-limits)).

---

## 1. Cave mode (priority)

### 1.1 What Xaero's minimap does

**Turning on and off by itself**

- Cave mode switches on by itself underground **and inside buildings** (MR-MM, CF-MM). The test is
  a *solid roof over the player*, not depth: a roof of solid blocks above you turns it on, a clear
  sky turns it off.
- The roof's size is a setting, **Cave Mode Ceiling**: 1x1, 3x3 or 5x5 columns around the player, or
  off (HP; CL-MM 1.16.5, 2019). 1x1 checks only the column you stand in. The bigger sizes need a roof
  over the whole square, so standing under a tree trunk or a narrow overhang does not count (CF-MM
  says 3x3 or more stops it turning on under logs while you cut wood). **Default 3x3** since CL-MM
  20.30.0 (2020-12-27).
- The roof doesn't have to be at one height: a roof made of blocks at different Y levels still
  counts (CL-MM 21.15.0).
- What counts as "solid" follows what the map itself draws as see-through: leaves and glass do not
  make a roof (CL-MM 23.4.4; VE: "any solid roof that blocks the sky, apart from leaves or glass,
  turns it on").
- The player's position is floored, not rounded, when testing (CL-MM 1.16.5).
- Switching has a delay of about a second so walking in and out of a doorway doesn't make the map
  flicker (CL-MM 21.9.0).
- Players can turn automatic mode off (VE turns it off on the surface because any roof flips the
  map) and use a **manual cave mode key**, with a **manual cave mode start Y** setting, instead
  (CL-MM 22.6.0, 2022-04-17; HP: "Manual Cave Mode Start Y / Auto", where Auto finds the start Y
  from the player's position). Default key for manual cave mode: not stated publicly.
- **Cave Mode Zoom In**: entering cave mode can zoom the minimap in by a chosen factor, up to 5x
  (HP; CL-MM 1.16.6). Zoom stepping skips levels that look the same while cave zoom is on
  (CL-MM 22.4.0).

**What slice is shown**

- **Start Y**: automatic (from where the player is) or a typed value with the manual mode (above).
  Public sources don't give the exact offset from the feet or head.
- **Cave Mode Depth**: how far *below* the start Y the map looks for a floor. Range 0 to 64,
  **default 30**; lower is faster, higher costs more in caves (CL-MM 20.9.0, 2020-04-24; HP).
- Nothing public says the minimap draws layers *above* the start Y in its own cave mode. The world
  map's cave menu has a **Cave Mode Top Y** that can be any number; VE uses it set a few blocks above
  the feet to see lava lakes above you before you dig into them.

**How it looks**

- Cave maps always use the shaded-slopes look (CL-MM 1.18.8) and are lit by depth: deeper floor is
  darker (CL-MM 20.0.0 mentions depth-based lighting). A **Legible Cave Maps** option brightens them
  so caves stay readable; since CL-MM 23.4.0 it is separate from the general lighting option, and
  lighting respects the dimension's ambient light.
- Outside cave mode the void (unknown) is a dark colour (CL-MM 22.1.0); an option sets how opaque
  undiscovered areas are, default 50% (CL-MM 26.2.0).
- In cave mode, entities fade by **vertical distance** from you rather than by brightness
  (CL-MM 21.3.0.1); the radar has a "custom Y distance for depth darkening" (CL-MM 22.1.0) and a
  "Min Y Distance For Depth Fade" setting (HP, VE: leave on auto).
- Entity filters can match "above or below ground" and "lit or unlit block" (CL-MM 22.16.0) and a
  predicate for "in the same cave state as me" (CL-MM 23.8.3).
- Ores, chests, spawners: **no public source says either mod highlights them.** Cave maps show the
  blocks' own colours (so an ore shows as its texture colour, like any block). The minimap has a
  **Show Redstone** block-map option (HP), **Show Flowers**, **Display Stained Glass** and **Block
  Transparency** (HP). The **light overlay** colours blocks whose block light is in a range,
  default 0 to 7, to find where mobs can spawn (MR-MM, CF-MM; CL-MM 20.25.0), with block and sky
  light selectable (CL-MM 22.5.0). The fair-play edition hides cave maps and entities entirely
  (MR-MM), which tells us caves are treated as an advantage to keep fair.
- The minimap can show the **world map's cave textures** (CL-MM 23.4.0), and by default it reuses
  the world map's textures (MR-MM: "Use World Map Mod Chunks", HP).

**Server control**: servers can turn cave maps off (config, chat codes, a `no_cave_maps` effect, and
since 2025 a list of dimensions where cave mode is allowed) (MR-MM; CL-MM 25.3.0). Lumos has no
server part; see 9.

### 1.2 What Xaero's world map does

- Cave mode on the full-screen map arrived in World Map 1.30.0 (2023-04-16) (CL-WM 1.30.0; CF-WM).
  Before that, the Nether was drawn as one "under the roof" view; those old maps show as **Full**
  cave mode now (CF-WM).
- **Cave Mode Type**, kept **per dimension**: OFF, **Layered** or **Full** (CF-WM names OFF and Full;
  VE names Layered and recommends it for caving). A **Default Cave Mode Type** sets what a new
  dimension starts with (CF-WM). Public sources don't define the difference exactly. From how VE
  describes using them: *Layered* shows one slice from a chosen top Y down, *Full* shows the whole
  underground under a top Y that is found automatically (CL-WM 1.30.3 says the Full type's top Y
  comes from the solid-block test).
- The **cave icon** at the bottom left of the map opens the cave menu: **Cave Mode Top Y** (any
  number), and in roofed dimensions whether to look **below or above the bedrock roof** (CF-WM).
- **Auto Cave Mode** setting on the world map (CL-WM 1.30.5) and **Display cave mode top Y**
  (VE).
- Cave data is saved, per dimension, in a `caves` folder next to the surface map, one sub-folder per
  layer (CF-WM's instructions for moving old Nether maps into `caves/<id>`). Changing layer reloads
  regions quickly in a sorted order (CL-WM 1.30.0).
- A key to **switch the dimension being viewed** (CL-WM 1.37.0); "Cave Mode Dimensions" checks the
  dimension being viewed, not the one you stand in (CL-WM 1.41.2).

### 1.3 Lumos Map today (cave)

From `glue.rs` (`cave_height`, `follow_cave`, `follow_layer`, `scan`) and `runtime.rs`:

| Part | Lumos now | Status |
|---|---|---|
| Modes | `CAVE_MODES`: Auto (underground), Always, Off; K cycles them | Partial: no manual start Y, no "roof size" |
| Auto trigger | In a dimension with a ceiling, or when **sky light at the head is 0** | Partial: one column, light-based, so it fires under any dark overhang and misses glass/leaf rules; no delay, so it can flicker at cave mouths |
| Start Y | The eye block (`position.y + eye_height`) | Partial: not settable |
| Depth | Scans down to the world's bottom (`while y > min`) | Missing: no depth limit; a deep ravine floor 100 blocks down shows as if it were near |
| Walls | A solid block *at* the start Y is a wall, drawn as its colour times `0x707070` | Partial: no separate "unexplored rock" colour; walls look like dark terrain |
| Depth shading | Only the relief shade from neighbours' heights (`shade`) | Missing: floor doesn't darken with depth below start Y |
| Area | Chunks within the minimap's reach, budget 500 us a frame | Have |
| Saved | No, the cave layer lives in memory and is dropped on leaving it | Missing |
| Rescan | When start Y moves 2 or more, or blocks change in the 5x5 chunks near you | Have |
| Nether | The explored (surface) scan starts at `min_y + 118` and walks down to the first air under the roof | Partial: the 118 is fixed; no "above the bedrock roof" view; no per-dimension choice |
| Cave zoom-in | None | Missing |
| World map layer | `[`/`]` or Down/Up step 8 blocks from the feet; scanned live from loaded chunks only (`slice`) | Partial: not saved, no Layered/Full, unloaded areas blank |
| Minimap uses world map caves | No (separate live layer) | Missing |
| Entities in caves | Mobs more than 8 blocks above or below drawn at alpha 95 | Partial: fixed distance, no above/below mark |
| Light overlay | None (only a light *line* under the minimap) | Missing |
| Ores, chests, spawners | Seen only as their block colour | Same as Xaero (by design, see 1.4) |

### 1.4 Lumos cave mode spec (what to build)

This is our design; where it states a number that the public sources gave, it says so.

**A. When cave mode is on** (minimap)

1. Setting **Cave view**: Auto, Manual, Off (rename "Always" to "Manual"; Manual uses the start Y
   setting below and is switched with the cave key). Default Auto.
2. Setting **Roof size**: 1x1, 3x3, 5x5. Default 3x3 (matches CL-MM 20.30.0).
3. Auto test, once every 4 ticks: for every column in the N x N square centred on the player's
   floored X/Z, walk up from the head block + 1 to the top of the loaded world. The column is
   *roofed* if it meets a block that is "roof-solid": not air, not a fluid, and not see-through on
   the map (leaves, glass, panes, bars, and any block whose averaged top texture is mostly clear;
   `Palette` can carry a `ROOF` flag next to `FLUID`/`AIR`). The roof may be at different heights
   per column. Cave mode turns on when **every** column is roofed.
4. Dimensions with a ceiling (`game.world.shape.has_ceiling`, the Nether) are always "roofed" unless
   the player is above the ceiling (see F).
5. Hysteresis: the new state has to hold for 20 ticks (about a second, CL-MM 21.9.0) before the map
   switches. Keep the old picture during the wait.
6. Optional sky-light test kept as a fast pre-check: if sky light at the head is 15, it's not a
   cave; skip the column walk.

**B. Which slice**

1. **Start Y** = the player's feet block + 2 (the head block's top), unless Manual with a typed
   start Y. Settings: "Start Y: Auto / typed value" (Xaero: "Manual Cave Mode Start Y").
2. **Depth**: 0..64, default 30 (CL-MM 20.9.0). A column is scanned from start Y down to
   `start Y - depth`; nothing found in that range means "deep" (draw the deep colour, below).
3. **Above**: setting **Show above** 0, 2, 4, 8 blocks, default 0. When above > 0, the scan starts at
   `start Y + above` but blocks above the head only count if there is air directly below them
   (lava lakes and caves over you show; the rock you stand under doesn't hide everything). This is
   the job VE uses the world map's top Y for.
4. Rescan rule stays: a new start Y differing by 2 or more, or the world revision changing near you.

**C. How it is drawn**

1. **Floor**: the first block with a map colour going down from start Y in an open column: its own
   colour (texture average, biome tint), with the relief shade.
2. **Depth shading**: multiply the floor colour by `1 - 0.55 * min(1, (startY - y) / depth)`, so the
   floor right under you is full colour and a floor at the depth limit is about half bright.
   **Legible caves** option (default on) caps the darkening at 30% (Xaero's "Legible Cave Maps").
3. **Walls** (columns solid at start Y): a flat dark rock colour, not the block's own colour,
   `0x2A2A2E` with a 1-texel lighter edge (`0x4A4A50`) where a wall meets an open column. This makes
   passages read as shapes, as in Xaero's caves where open space stands out against solid rock.
4. **Deep** (open column, nothing within depth): `0x101014`, darker than walls, so a drop is clear.
5. **Not loaded / never seen**: the minimap's "Unexplored ground" background as now.
6. **Fluids**: water as now (`water_over`); lava stays bright (it is a hazard you want to see), not
   depth-shaded.
7. Cave mode always shades slopes (CL-MM 1.18.8), even if Terrain shading is off.
8. **Cave zoom-in** setting: Off, x1.41, x2 (default Off). On entering cave mode, the minimap's zoom
   steps in by that much and steps back on leaving (CL-MM 1.16.6 style).

**D. Blocks and things in caves.** No public source says Xaero marks ores, chests or spawners, and
the fair-play edition shows caves are a fairness issue. Lumos keeps **no ore or chest highlighting**
(they show only as their own block colours, as now). Two optional, default-off extras that match
documented Xaero options:
- **Show redstone** (HP lists "Show Redstone"): redstone wire and components drawn in their own
  colours even when they are thin (today a thin block can be skipped if it has no map colour).
- **Light overlay** (MR-MM, CF-MM): see 4.6; works in cave mode, and it is how spawners' rooms and
  dark spots are found in Xaero too.

**E. Entities in cave mode**: fade by vertical distance with a setting **Depth fade from** (Auto = 8,
4, 8, 16, 32 blocks); a small up or down tick on dots more than that distance above or below
(Xaero: "Show Entity Depth", VE).

**F. Nether and other roofed dimensions**

1. Replace the hard-coded `min_y + 118` with the dimension's own logical height: start at
   `min_y + logical_height - 1` (the Nether's roof is the top of its logical height) and walk down to
   the first air.
2. **Above the roof**: when the player's feet are above the dimension's logical height (on the
   Nether roof), the minimap shows the surface view of the roof instead (Xaero: "below or above the
   bedrock roof", CF-WM).
3. In the Nether, Auto cave mode uses the player's start Y like the Overworld, so the minimap shows
   the floor around you, not the whole Nether from the roof down.

**G. World map cave layers** (Xaero 1.30 behaviour, our format)

1. Per dimension setting **Cave map**: Off, Layered, Full. Defaults: Overworld Off, Nether Full,
   End Off, other roofed dimensions Full (CF-WM: the type is per dimension; Full is how old Nether
   maps look).
2. **Layered**: the map shows the slice under a **top Y** chosen in the map: `[`/`]` move it by 8
   (today's step) and a typed value; a **Follow me** toggle keeps top Y at the player's start Y.
   **Full**: top Y comes from the roof test; one picture of the floors under the roof.
3. **Saving**: every scan of a cave slice (from the minimap's cave layer as well as the world map)
   is written to `lumos-map/<world>/<dim>/caves/<band>/r.<x>.<z>.lmr`, where `band = topY.div_euclid(16)`
   (layers are kept per 16-block band so walking a few blocks doesn't make a new layer). The tile
   format is the surface one (`LMR2`) plus the band in the header. A band's tile is overwritten when
   rescanned.
4. The world map in cave mode shows the saved band's tiles, so caves you explored stay on the map
   after their chunks unload (today the slice only shows loaded chunks).
5. The minimap's cave layer reads the saved band where it has no live tile yet, so the minimap is
   complete at once on returning to a cave (Xaero's minimap reuses the world map's cave textures,
   CL-MM 23.4.0).
6. Title bar shows "Caves Y 24..-6" when a cave layer is shown (Xaero: "Display cave mode top Y").

**H. Keys**: Cave key (K) cycles Auto, Manual, Off as now; in Manual it also sets start Y to your
current Y when switched on. Shift+K (or a separate unbound "Cave start Y here" key) re-anchors start
Y in Manual.

---

## 2. Minimap rendering

| Feature (Xaero) | How it behaves | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Vanilla style | Keeps the game's own look | - | MR-MM | Have | Lumos keeps its own frame art |
| Rotating square map | Square map turns with the player | on | MR-MM | Have | `rotate`, default on |
| Circle shape | Round map | square | MR-MM, HP | Have | `round` |
| Lock north | North fixed up, an arrow shows your facing | off | MR-MM, HP | Have | `rotate` off |
| Sizes | Several sizes plus "auto" | auto | MR-MM | Have | 64..224 GUI px, or `auto_size` (about a quarter of the GUI's short side, so it follows the GUI scale); off by default |
| Zoom levels | Several; I/O keys | 1x (CL-MM 21.0.0) | MR-MM | Have | 9 steps 0.25..4 blocks/px, = and - |
| Position | Anywhere, 1-pixel precision, "Change Position" screen | top-left | MR-MM | Partial | 4 corners, offsets 0..64 in 7 steps; no drag-to-place |
| Block colours: Accurate | Texture and biome colours | Accurate | MR-MM, HP | Have | `textured` |
| Block colours: Vanilla | Map colour palette | - | MR-MM | Have | `textured` off |
| Biomes in Vanilla mode | Biome tint on the palette | off | MR-MM, HP | Have | `palette_biomes`: grass, leaves and water take the column's biome hue at the palette colour's brightness |
| Terrain depth | Darker the lower the ground relative to you | on | MR-MM, HP | Have | `depth_shade`, minimap only, off by default (keeps 3.1's look); up to 40% darker 48 blocks below you, your height in 4-block steps |
| Terrain slopes | 3D, flat or overhead shading | 3D | HP | Partial | shading on/off only |
| Biome blending | On, can be turned off for speed | on | CL-MM 23.4.4 | Partial | always on, no toggle |
| Lighting | Map darkens at night / by light | on | HP | Partial | `night`: the minimap's Overworld surface times the sky's brightness (time, rain, thunder); off by default; no block light |
| Show flowers | Flowers in their own colours | - | HP | Partial | flowers get their texture average; no toggle |
| Stained glass / transparency | Glass tinted, blocks seen through | - | HP | Have | `glass`: stained glass and panes are passed and tint the ground under them (plain glass has no map colour, so it never hid it); loaded chunks are mapped again on a switch, saved ones keep how they were mapped |
| Short blocks | Slabs/stairs nudged so they don't look like walls | - | HP | Missing | |
| Show redstone | Redstone visible | - | HP | Missing | see 1.4 D |
| Anti-aliasing | Smoother block edges | off | HP | Blocked | nearest-only sampling |
| Opacity | Map see-through | 100% | HP | Have | 5 steps |
| Undiscovered opacity | How dark unknown areas are | 50% | CL-MM 26.2.0 | Have | `BACKGROUNDS` incl. see-through |
| Frame style / colour | Choose frame and its colour | - | HP; CL-MM 22.2.0 | Have | 3 styles, 8 rim colours (`frame_colour`, "Dark" as before) |
| Compass letters | N/E/S/W round the rim, colours | on | HP | Have | off / N only / all; no colour choice |
| Player arrow | Your arrow in the middle, colour | - | HP | Have | |
| Enlarge (Z) | Hold to enlarge; centred, zoom when enlarged, toggle option, keep rotation | Z held | MR-MM, HP, CL-MM 22.13.0/22.17.0 | Missing | |
| Hide under F3 | | - | CL-MM 22.4.0 | Partial | check; corner logic yields to F3 charts |
| Hide when a screen is open | | - | CL-MM 22.2.0 | Partial | not checked: whether `native_hud` runs under open screens; add a setting |
| Chunk grid on minimap | Toggle, line width | off | MR-MM, CL-MM 22.7.0 | Missing | world map only |
| Slime chunks on minimap | Green overlay | off | MR-MM | Missing | world map only |
| Light overlay | Blocks in a light range coloured | 0..7 | MR-MM, CL-MM 20.25.0, 22.5.0 | Missing | |
| World map textures on minimap | Reuses world map data | on | MR-MM, HP | Have | shared `Store` and cells |
| Render layer / over other HUD | | - | CL-MM 1.17.7 | N/A | Horizon orders HUD hooks by priority |

## 3. Minimap info lines

| Feature | Behaviour | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Coordinates | Under the minimap | on | MR-MM | Have | |
| Biome | | off | MR-MM | Have (on) | |
| Light level | Block and sky light separately option | off | MR-MM, CL-MM 22.5.0 | Partial | one value |
| Game time, 12 or 24 h | | off | MR-MM | Partial | 24 h only |
| Camera angles | Yaw and pitch | off | MR-MM, CL-MM 20.5.0 | Partial | facing text only |
| Dimension | | off | CL-MM 21.9.0 | Have | |
| Weather | | off | CL-MM 22.17.0 | Missing | |
| Chunk coordinates | | off | CL-MM 23.2.0 | Missing | |
| Real-world time | | off | CL-MM 23.3.0 | Missing | |
| Overworld coordinates in the Nether | | off | CL-MM 23.3.3 | Missing | the world map's cursor line has it |
| Info display manager | Each line's colour and order | - | CL-MM 23.1.0 | Partial | 4 fixed orders, no colours |
| Text alignment | | - | CL-MM 20.5.0 | Missing | |
| Hide coordinates (streaming) | | off | MR-MM | Missing | |

## 4. Entity radar

| Feature | Behaviour | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Radar on/off and key | Master toggle and a key | on | CL-MM 21.17.0 | Partial | per category only, no key |
| Mobs | Yellow dots; hostile and friendly can differ | yellow | MR-MM | Have | hostile red, passive green |
| Players | White dots or heads | white | MR-MM | Have | white, faces on Tab |
| Items | Red dots | red | MR-MM | Have | off by default |
| Other entities | Purple (arrows, item frames; frames off) | purple | MR-MM, CL-MM 21.7.0 | Have | off by default |
| Icons instead of dots | Heads / model icons; resource-pack icons | "Player List" (icons while Tab held) | MR-MM, CL-MM 20.29.0 | Have | Lumos defaults to the same Tab behaviour |
| Names | Display names: off / player list / always; scale | - | HP, VE | Have | players on, mobs off |
| Name when icon fails | | on | CL-MM 21.14.0 | N/A | Lumos always has a dot |
| Entity count limit | | 1000 | CL-MM 21.15.0 | Partial | fixed 400, no setting |
| Height limit | Hide entities far above/below | - | HP, VE (350) | Missing | |
| Depth fade | Fade by Y distance, custom distance | auto | CL-MM 22.1.0, HP | Partial | fixed 8 blocks |
| Show depth / Y | Above/below marker, entity Y level | - | CL-MM 22.5.0, 23.2.0; HP | Missing | |
| Categories and lists | Custom categories, include/exclude lists, wildcards, predicates (tamed, friend, tracked, above ground, lit) | - | CL-MM 21.17.0, 22.3.0, 22.16.0, 23.8.0 | Missing | big; a simple version: per-mob-type on/off list |
| Render over frame | | off | CL-MM 22.8.0 | Missing | |
| Reverse draw order key | Hold to draw the other order | - | CL-MM 21.19.0 | Missing | |
| Range | | map edge | - | Have | `RANGES` |
| Hide invisible | Config-only | - | CL-MM 22.14.0 | Partial | check `mob.invisible` |
| Party/ally players | OPAC | - | HP | N/A | |

## 5. Waypoints

| Feature | Behaviour | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Count | Effectively unlimited | - | MR-MM | Have | |
| Create | B opens a create screen | B | MR-MM, HP | Have | B opens "New waypoint" box |
| Instant waypoint | `+` adds one at once; on the world map `+` adds a temporary one | + | HP, CL-WM 1.5 | Partial | "B adds instantly" setting, no own key |
| List | U | U | MR-MM | Have | |
| On minimap, in world | Both, hide all or some | on | MR-MM | Have | markers, minimap, world map separately |
| Edge of minimap | Waypoints off the map sit on its rim | - | VE ("ring") | Have | `clamp_to_map` |
| In-world look | Icon scale, up-close scale, opacity, name/distance scale, name above distance | - | HP | Partial | label size, shrink with distance, fade |
| Distance text | km above a threshold, precision, always vs looked-at, angle to show | - | HP, CL-MM 21.21.0 | Partial | m/km or blocks |
| Label on the nearest to the crosshair; sneak shows more | | on | CL-MM 21.16.0 | Partial | looked-at only, no sneak |
| Min/max render distance; local vs global | Local hidden past max, global always | - | MR-MM, CL-MM 20.15.0, 23.8.4 | Partial | one range; "all worlds" waypoints aren't exempt |
| Sets and switch key | Sets, a key cycles the set; render all sets key | unbound | MR-MM, CL-MM 1.17.1 | Partial | sets in the list; no keys |
| Sort | name, distance, angle, newest; new ones to top/bottom | top | CL-MM 1.19.0, 21.21.0 | Partial | no angle sort, no new-to-bottom |
| Share in chat / receive | Share button; received shares can be added | - | MR-MM, CL-MM 1.16.3 | Partial | share yes; "From chat" picks coords, no one-click add from a share line |
| Teleport | `/tp @s x y z`, configurable per server; +0.5 Y option; cross-dimension | `/tp @s x y z` | MR-MM, CL-MM 1.18.1, 22.1.1 | Partial | fixed command, only when ops |
| Temporary | Removed on restart / leaving | - | CL-MM 23.1.0 | Have | |
| Disappear when reached | | off | CL-MM 23.3.0 | Missing | |
| 2D waypoints | Y "~": no height | - | CL-MM 21.13.0 | Missing | |
| Nether/Overworld scaled | x8 / /8 positions | - | CL-MM 1.16, 23.9.0 | Have | `convert` |
| Enable/disable a waypoint | | - | CL-MM 1.16.9 | Have | |
| Hide waypoint coordinates | Streaming | off | MR-MM, HP | Missing | |
| Beams | (not a documented Xaero feature) | - | - | Have (extra) | |
| Third-party waypoints (Waystones) | | - | CL-MM 26.2.0 | N/A | |

## 6. Death points

| Feature | Behaviour | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Auto death point | Skull-shaped waypoint on death | on | MR-MM | Have | Lumos uses its own symbol |
| Keep old ones | Old ones become plain waypoints; setting | on | MR-MM, HP | Partial | keeps 1/3/5/10 death points |
| Delete when reached | Removed when you walk to it | on | CL-MM 22.2.0 | Missing | |
| Drawn on top, always global | | - | CL-MM 22.2.0, 20.15.0 | Partial | |
| Auto sub-world on death | | - | CL-MM 23.5.0 | N/A | |

## 7. World map rendering

| Feature | Behaviour | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Self-writing map | Fills in as you explore; SP, MP, Realms | - | MR-WM | Have | |
| Resource packs | Changing packs regenerates the map | - | MR-WM | Partial | colours recomputed; saved tiles keep old RGB until rescanned |
| Vanilla paper-map look | | off | MR-WM | Have | `textured` off |
| Shading adjustable; lighting; terrain depth | | on | MR-WM, VE | Partial | slope shading on/off |
| Biome blending | toggle | on | CL-WM 1.30.3, VE | Partial | always on |
| Load flowers | | off | VE | Missing | |
| Footsteps | Your recent path | on | MR-WM, CL-WM b1.6 | Missing | |
| Cave mode | see section 1 | - | | Partial | |
| Nether | Cave-style | Full | CF-WM | Partial | under-roof view, fixed height |
| Optimised SP generation | Map of an optimised world builds fast; "Reload/Convert regions" | - | MR-WM, CF-WM | Missing | Lumos only maps loaded chunks |

## 8. World map UI and tools

| Feature | Behaviour | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Open | M | M | MR-WM | Have | |
| Pan, zoom | Drag, wheel; Ctrl for fixed steps; zoom buttons | - | MR-WM, CL-WM 1.13.0, 1.14.0 | Partial | drag, wheel, WASD; no buttons, no Ctrl steps |
| Zoom range | 0.0625x to 50x (newer: near unlimited) | - | CL-WM 1.9.0, 1.13.0, 1.42.0 | Have | 1/8 to 32 blocks a pixel |
| Controls tooltip | Hover lists the keys | - | CL-WM 1.14.0 | Partial | hint line |
| Cursor coordinates | Top of the map | on | CL-WM 1.4.9, VE | Have | bottom bar, with Nether/Overworld |
| Cursor biome | | on | CL-WM 1.29.0, VE | Missing | tiles store no biome |
| Right-click menu | Teleport here, create waypoint, edit/disable/delete waypoint, distance | - | CL-WM 1.14.0, 1.42.0 | Missing | click adds a waypoint |
| Teleport by right-click | `/tp @s x y z`, configurable; tracked players | - | MR-WM, CF-WM | Missing | |
| Waypoints on the map | Shown; toggle button; side menu with filter | on | CL-WM 1.23.1, b1.8, 1.14.0 | Partial | shown, filter All/This set/None; list is a separate page |
| B / + on the map | Add / add temporary at cursor | - | CL-WM 1.5 | Partial | click adds |
| Player arrow | toggle, colour | on | CL-WM 1.6.4, VE | Have | |
| Other players / tracked players | | - | CF-WM | Missing | |
| Chunk grid | | off | CL-WM | Have | |
| Slime chunks | | - | (minimap feature) | Have | singleplayer seed only |
| Export PNG | Whole map; options | - | MR-WM, CL-WM 1.32.0 | Partial | exports the current view |
| Switch viewed dimension | Key and button | - | CL-WM 1.37.0 | Missing | |
| Hide UI (F1) | | F1 | CL-WM 1.42.0 | Missing | |
| Camera follow toggle | | - | CL-WM 1.42.0 | Have | Follow tool |
| Claims / highlights | OPAC | - | MR-WM | N/A | |
| Zoom level display | | on | CL-WM 1.14.0 | Have | in the cursor line |

## 9. Multiplayer, sub-worlds, server control

| Feature | Behaviour | Default | Source | Lumos | Notes |
|---|---|---|---|---|---|
| Maps per server address | Several maps, switched by hand or automatically | - | MR-WM | Have | sub-worlds |
| Selection: Manual | Confirm the map when the world changes | default (newer) | CF-WM | Missing | Lumos switches silently |
| Selection: World Spawn | By spawn point | - | CF-WM | Have | `SPAWN_SLOP` 48 |
| Selection: Single | One map for everything | default when no Server mode | CL-WM 1.11.8 | Missing | |
| Selection: Server | Server tells the level id | - | CF-WM | N/A | needs a server mod |
| Waypoint multiworld | Per sub-world, off by default | off | MR-MM, CL-MM 21.1.0 | Partial | Lumos ties waypoints to the map world |
| Server item requirement, effects, enforced settings | `minimapItemId`, `no_cave_maps`, etc. | - | MR-MM, MR-WM | N/A | no server part; see WP12 for a client-side opt-in |
| Fair-play edition | No radar, no caves | - | MR-MM | Missing | a "fair play" switch is cheap |
| Hide server address | Partial by default | partial | MR-MM | Missing | |

## 10. Keybinds and defaults

Xaero's defaults from public pages, and Lumos's (`keys.rs`).

| Action | Xaero default | Source | Lumos | Lumos default |
|---|---|---|---|---|
| Open world map | M | MR-WM | Have | M |
| Create waypoint | B | MR-MM | Have | B |
| Waypoint list | U | MR-MM | Have | U |
| Minimap settings | Y | MR-MM | Have | unbound |
| Enlarge minimap | Z (hold; toggle option) | MR-MM, CL-MM 22.13.0 | Missing | - |
| Zoom in / out | I / O (Modrinth page); unbound since 21.0.0 (changelog) | MR-MM vs CL-MM 21.0.0 | Have | = / - |
| Instant / temporary waypoint | + | HP, CL-WM 1.5 | Partial | setting only |
| Toggle minimap | unbound | CL-MM 20.14.0 | Have | H |
| Toggle all waypoints | (toggles a saved setting) | CL-MM 20.14.0 | Have | unbound |
| Toggle in-world / on-map waypoints separately | | CL-MM 20.2.0 | Missing | |
| Switch waypoint set | not listed | MR-MM | Missing | |
| Render all sets | | CL-MM 1.17.1 | Missing | |
| Manual cave mode | not listed | CL-MM 22.6.0 | Partial | K cycles modes |
| Toggle entity radar | | CL-MM 21.17.0 | Missing | |
| Reverse entity order (hold) | | CL-MM 21.19.0 | Missing | |
| Toggle chunk grid / slime chunks / light overlay | | MR-MM | Missing | world map buttons only |
| Alternative List Players | | CL-MM 22.15.0 | Partial | Lumos reads vanilla's player list key |
| Switch viewed dimension (map) | | CL-WM 1.37.0 | Missing | |
| Server settings screen | | CL-MM 25.3.0 | N/A | |
| Hide map UI | F1 | CL-WM 1.42.0 | Missing | |

## 11. Performance options

| Feature | Behaviour | Source | Lumos | Notes |
|---|---|---|---|---|
| Map writing pace | Target work time per frame, scaled to render distance | CL-MM 20.28.x, CL-WM 1.11.3 | Have | `BUDGET` 0.7/2.5 ms, `CELL_BUDGET` |
| Full speed while the map is still | | CL-WM 1.14.4 | Partial | bigger budget when open |
| Update loaded chunks / load new chunks toggles | | CL-WM b1.3 | Missing | |
| Region cache limit, unload least-recent | Max loaded regions | CL-WM 1.3, 1.41.2 | Missing | `Store` keeps the whole dimension in memory |
| Separate threads for writing and files | | CL-WM 1.3 | Partial | region load and colour table off-thread; scans on the main thread |
| Biome blending off | | CL-MM 23.4.4 | Missing | |
| Cave depth lower = faster | | CL-MM 20.9.0 | Missing | no depth limit |
| Ignore server heightmaps | | HP | N/A | Lumos scans its own |
| Texture compression | | CL-WM 1.4 | N/A | |

## 12. Integrations

All of these are other Java mods (Open Parties and Claims, Waystones, Better PVP, FTB Ranks,
LuckPerms, AMECS key modifiers): **N/A** on Horizon. What Lumos should do instead:
- Share one source of truth with `lumos_hud` (coordinates, biome, light) so the two don't show the
  same line twice.
- `native-ports/atlas` (MIT, another map) uses Horizon's map slots, not the mod image slot, so both
  can be installed; Lumos should say in its description that two minimaps at once is not useful.
- `lumos_controls` already lists Lumos Map's keys in Controls.

---

## Horizon 9f42064c limits

Read from the pinned Horizon and Lumos code; each one decides how a feature can be done.

1. **One mod image slot, 1024 x 1024** (`MOD_IMAGE_SPRITE`, `MOD_IMAGE_SIZE` in
   `horizon-client-render/src/hud.rs`). Lumos packs 64 x 56 terrain cells of 16 x 16 texels and a
   row of 16 x 16 entity icons into it (`runtime.rs`, "the map texture: cells"). The minimap and the
   world map share the cells and are never shown together. Effects: an **enlarged minimap** and a
   **bigger world map view** fall back to coarser levels of detail; **footsteps, light overlay,
   chunk grid and slime chunks must be drawn as HUD quads/fills, not baked into cells**; persisted
   cave layers are fine (they're data, drawn into cells only when shown). Atlas's port notes that
   Horizon's own map slots can be full in a frame too (`maps.slots.claim` returning `None`).
2. **Nearest sampling for HUD sprites.** The HUD atlas sampler is the default (nearest); only a
   separate "HUD linear" sampler exists for Horizon's own uses. A rotated minimap shows stepped
   texel edges and **anti-aliasing (Xaero option) is Blocked**. Smoother turning would need Horizon
   to expose the linear sampler for `native_sprite`.
3. **Uploads are texture patches** (`map_patches`, `upload` in `glue.rs`): every changed cell costs
   a 1 KiB patch that frame; budgets (`CELL_BUDGET`) keep this in check. Big redraws (layer change,
   resource reload) take several frames.
4. **No `Registrar::register_key_mapping`** at 9f42064c (not in the tree). Keys live in
   `NativeMod::keys` and `keys.rs`; `lumos_controls` mirrors them into Controls. No key modifiers
   (Shift+K style combos must be read by hand from `physical_keys`), no hold/toggle choice from
   Controls.
5. **No server side.** No level ids (Xaero's "Server" map mode), no server-enforced settings, no
   status effects. Client-side fair play only.
6. **World drawing**: in-world waypoints are HUD markers from `project`; `native_world_primitives`
   only gives the beam primitives. 3D icons in the world, as Xaero draws them, aren't possible; the
   HUD markers are the equivalent.
7. **Data the client has**: chunks with block states and biomes, `world.light(x, y, z)` (sky light in
   the high nibble, used by `cave_height`), `world.shape.has_ceiling`, mobs, dropped items, remote
   player names, the singleplayer seed only (`native_world_seed`). A light overlay and roof tests are
   possible; slime chunks in multiplayer are not (no seed). Block states give chests and spawners by
   name if ever wanted (no block-entity API needed).
8. Lumos runs on the main thread inside `native_frame`; scanning more (cave depth, saved bands) must
   stay inside the frame budgets or move to a worker that gets copied chunk sections.

---

## Gap summary: top 10

1. **Cave mode trigger and slice**: one-column sky-light test, no roof size, no hysteresis, no depth
   limit, no manual start Y (1.3).
2. **Cave look**: no depth shading, walls drawn as dark terrain instead of rock, no deep/unknown
   distinction, no legible option, no cave zoom-in.
3. **World map cave layers aren't saved** and show only loaded chunks; no Layered/Full, no
   per-dimension type, Nether roof height hard-coded at 118.
4. **Enlarged minimap (Z)** missing.
5. **World map right-click menu and teleport-from-map** missing; no other players on the map.
6. **Footsteps** (recent path) on the world map missing.
7. **Minimap overlays**: chunk grid, slime chunks and light overlay on the minimap missing.
8. **Entity depth**: no above/below marks, Y display, height limit or depth fade setting; no radar
   key.
9. **Waypoint extras**: disappear-when-reached, delete reached death points, 2D waypoints, set
   switch / render-all keys, configurable teleport command, hide coordinates.
10. **Map selection and memory**: no Manual-confirm or Single mode, no viewed-dimension switch, and
    the whole dimension's tiles stay in memory (no region cache limit).

---

## Implementation plan

Order: cave mode first (user priority), then the most visible gaps. Sizes: S under a day, M one to
three days, L more. Every package keeps the rules in `runtime.rs` with unit tests (they build alone:
`rustc --edition 2024 --test runtime.rs`), the game reads in `glue.rs`, keys only in `keys.rs`, and
new settings in the `settings_table!` (a `SETTINGS_VERSION` bump when defaults change). Test in a
game started from a scratch `--game-dir` with fresh `HORIZON_HOME` etc. (CLAUDE.md), in a creative
singleplayer world.

### WP1 Cave trigger and slice (L)

- `runtime.rs`: `CAVE_MODES` -> Auto/Manual/Off; new settings `cave_roof` (1x1/3x3/5x5, default
  3x3), `cave_depth` (`[8, 16, 30, 48, 64]`, default 30), `cave_above` (0/2/4/8), `cave_start`
  (Auto or a Y kept as text). A pure `fn roofed(columns: &[Option<i32>]) -> bool` and a
  `CaveSwitch { state, pending, since_tick }` with `fn step(&mut self, roofed: bool, tick: u64) ->
  bool` (20-tick hysteresis). Tests: switching only after 20 ticks, flicker suppressed, 3x3 needs all
  9 columns.
- `glue.rs`: `Palette` gets a `ROOF` flag (not air, not fluid, not a see-through block: leaves,
  glass, panes, bars, and top textures mostly clear). `cave_height` becomes `cave_state` doing the
  N x N walk every 4 ticks with the sky-light 15 shortcut; `scan` takes `(start, depth, above)` and
  stops at `start - depth`.
- Nether: replace `min + 118` with the dimension's logical height; above the roof use the surface
  scan.
- `keys.rs`: K cycles Auto/Manual/Off (Manual sets start Y to here); add unbound `cave_here`.
- Verify: dig a 1-wide shaft with an open sky (map stays surface), stand under a 3x3 stone slab
  roof (switches after about a second), under a single log (doesn't with 3x3), under glass (doesn't).
  In a ravine, floor below 30 blocks shows the deep colour. In the Nether, walk onto the roof: surface
  view. Watch the frame time overlay (F3) staying flat while walking in caves.

### WP2 Cave look (M)

- `runtime.rs`: `Col` gets a `kind` (Floor, Wall, Deep); `shade` gains the depth factor
  (`1 - 0.55 * min(1, d/depth)`, capped at 0.3 with Legible on); wall and deep colours and the
  wall edge (a wall texel next to an open one is lighter) in `compose_cell` (needs the neighbours it
  already reads for slopes). Lava exempt from depth shading. Tests on colours.
- Settings: `cave_legible` (on), `cave_zoom` (Off/x1.41/x2).
- `glue.rs`: `follow_cave` applies the cave zoom step on entering/leaving.
- Verify: in a cave system, passages read as shapes against flat rock; a drop is darker than walls;
  lava stays bright; toggling Legible changes darkness at once (cells redrawn via `Look`).

### WP3 Saved cave layers and world map cave types (L)

- `runtime.rs`: band = `top.div_euclid(16)`; `Store` gains an optional band; region file name and
  header carry it (`LMR3`, still reading `LMR2`); per-dimension `CaveType` (Off/Layered/Full) saved
  in the world folder. Tests: encode/decode round trip with a band, band of negative Y.
- `glue.rs`: `Layer` writes its tiles into a `Store` for its band under `<dim>/caves/<band>/`; the
  world map with a cave type shows the band's store through cell source 2; the minimap's cave layer
  falls back to the saved band; title shows the slice's Y range; `[`/`]` step by 8, a "Follow me"
  tool, a typed top Y box (reuse the editor's text box).
- Verify: explore a cave, fly away until its chunks unload, open M: the cave is still on the map at
  that band. Restart the game: still there. Nether opens in Full, Overworld in Off; changing one
  doesn't change the other.

### WP4 Enlarged minimap (M)

- Key `enlarge` (Z, hold; setting "Enlarge key toggles"). Settings: "Enlarged: centred / in place",
  "Zoom when enlarged: same / out x2 / out x4", "Keep rotation when enlarged".
- `glue.rs` `on_hud`: when held, draw at `min(w, h) * 0.8` centred, with the LOD picked by `lod_for`
  under the cell limit (limit 1).
- Verify: hold Z in a large explored area; the map grows, stays crisp, and frame time stays flat.

### WP5 World map right-click menu, teleport, players, footsteps (M)

- `runtime.rs`: `Footsteps` ring buffer (last 1000 points, one per 2 blocks moved, per dimension,
  saved with the world); setting `footsteps` (on).
- `glue.rs` `on_release`: right-click opens a small menu at the pointer: "Waypoint here", "Teleport
  here" (only with `may_teleport`; command setting, default `/tp @s {x} {y} {z}`, Y from the
  tile's height + 1), and on a waypoint "Edit / Hide / Delete / Teleport". Left click keeps adding.
  Draw remote players as dots (and their faces on hover). Footsteps as small fills.
- Verify: right-click a far spot with cheats on: teleported to the ground there. Without cheats the
  teleport rows are absent. Walk around: a dotted trail follows.

### WP6 Minimap overlays: grid, slime, light (M)

- Settings and unbound keys for each. Grid and slime chunks drawn as HUD quads through the same
  `to_map` and clipped to the outline (`Shape::cut`). Light overlay: during the scan record block
  light of the air block above each column's top (`world.light`), store in the live tile (not saved);
  draw texels in range (setting min/max, default 0..7; block or sky) as a translucent quad layer.
  In cave mode use the cave floor.
- Verify: at night in a dark area, unlit floor turns red; placing a torch clears its circle within a
  second (the near rescan).

### WP7 Entity depth and radar keys (M)

- Settings: `depth_fade` (Auto/4/8/16/32), `entity_height_limit` (off/16/32/64), `show_depth`
  (marks), entity Y in the label. Key `radar` toggles all dots. Count limit setting (100..1000).
- `glue.rs` `draw_entities`: up/down ticks, height filter, fade by the setting.
- Verify: spawn mobs on several levels of a cave; the ones above/below get marks and fade beyond the
  distance.

### WP8 Waypoint extras (M)

- `Waypoint` flags: `two_d` (Y "~"), `vanish` (removed when within 4 blocks); death points removed
  when reached (setting, default on); render-all-sets and next-set keys; teleport command setting;
  "Hide coordinates" (list and markers show "hidden"); sneak shows all names; angle sort; "new
  waypoints at the bottom".
- Verify: make a 2D waypoint: distance ignores Y; walk to a vanish waypoint: it goes; die and walk to
  the death point: it goes.

### WP9 Info lines (S)

- Weather, chunk coordinates (`x>>4 z>>4` and in-chunk), Overworld coordinates in the Nether, yaw and
  pitch, 12/24 h, real time, block/sky light; replace `INFO_ORDERS` with a per-line order (8 slots)
  and a colour per line.
- Verify: each line toggles and moves.

### WP10 Map selection and dimensions (M)

- Map mode setting per server: Spawn (now), Manual (on a world change ask "This map / New / other"
  in a small prompt, keep using the last until answered), Single. A viewed-dimension button/key on
  the world map that reads another dimension's saved store (no live scan).
- Verify: on a server with two worlds on one address, Manual asks once per change; Single never
  splits.

### WP11 Memory and speed (M)

- `Store` as a region cache: keep regions within N (setting 64/128/256) of the view or player,
  unload least recently used, reload from disk off-thread as the map pans. Settings: "Update loaded
  chunks", "Map new chunks", "Blend biomes".
- Verify: fly 20 000 blocks in spectator; memory stays bounded (Activity Monitor), the map still
  shows old areas when panned to them.

### WP12 Fair play and privacy (S)

- "Fair play" switch: no entity dots, no cave view. Honour a Lumos server opt-out: if the server's
  MOTD or a chat line contains a documented Lumos code, turn off cave view and radar for that
  server (our own code; we don't copy Xaero's strings). "Hide server address" in the map's world
  name.
- Verify: with the switch on, K does nothing and no dots show.

### WP13 Minimap look extras (S each)

- Terrain depth (darken by height below the player), lighting at night (multiply by sky brightness),
  biomes in palette mode, glass see-through (skip glass and tint the floor), frame colour, drag to
  place the minimap on the settings screen, auto size (GUI scale based).
- Verify by toggling each in a varied area at day and night.

### Status on `feat/map-xaero-parity` (Lumos Map 3.1.0)

Built from this spec only; checked with `cargo check` on the staged game and the `runtime.rs`
tests. Not yet run in the game (see the checklist in the branch's report).

| Package | Done | Left, and why |
|---|---|---|
| WP1 | Roof test (1x1/3x3/5x5, `ROOF` palette flag, sky light 15 short cut, every 200 ms), 20-tick switch, depth 8..64 (30), floors above (0/2/4/8), Manual with start Y, K cycles, `cave_here` key, Nether from the logical top, roof top above it | Start Y is the head's block (feet + 1), the reading of B.1 that keeps "a block at the start is rock". "Show above" counts floors with open space *over* them (lava lakes, caves over you), the reading of B.3 that does what it says it is for |
| WP2 | Rock and edge colours, drop colour, depth factor with Legible cap, lava bright, slopes always shaded in caves, cave zoom-in as an effective zoom (the saved zoom is untouched) | - |
| WP3 | `LMR3` bands under `<dim>/caves/<band>/`, live tiles over saved ones on both maps, Off/Layered/Full per dimension in `<world>/cave-maps.txt`, Nether Full = its old map, "Roof top" for Nether Off, "Y follows me", titles | No typed top Y box (`[`/`]` and follow cover it). Overworld Full is our own design: the first floor under the first solid mass below the surface |
| WP4 | Z enlarges (hold or toggle), zoom out x1/x2/x4, keep rotation; shows under F3 | No "in place" (always centred) |
| WP5 | Right-click menu (waypoint here, teleport here, edit/hide/delete/teleport to a waypoint), other players, footsteps (1000 points, saved) | Teleport command not configurable (no text settings in the table; `tp @s x y z` as the editor's); no tracked players (needs a server part) |
| WP6 | Grid, slime, light overlay (block light up to 0/3/7/11) as HUD quads with keys | Light overlay reads block light only (no sky light choice) |
| WP7 | Radar key and switch, depth fade, height limit, up/down marks, Y in labels, count limit | No reverse-order key, no per-type lists |
| WP8 | 2D and vanishing waypoints, death points gone when reached, next-set and all-sets keys, hide coordinates | No angle sort, no "new at the bottom", no sneak-shows-all, no teleport command setting |
| WP9 | Chunk, other dimension's place, yaw/pitch, weather, real time, 12/24 h | Per-line order and colours (the four fixed orders stay; new lines follow them) |
| WP10 | "Server maps: Single", viewed-dimension button (saved map, read only) | Manual-confirm mode (needs a prompt flow); viewed dimension reads its folder whole |
| WP11 | Region cache (2/4/8 regions round you and the view), update/new chunk switches, blend switch | Cave bands are not cached by region yet; zoomed far out, the view shows at most twice the cache radius |
| WP12 | Fair-play switch | Server opt-out code and hiding the server address |
| WP13 | Settings screen's third column when two don't fit; terrain depth, night lighting, palette biomes, stained glass seen through, frame colour, auto size (each off or as before by default) | Drag to place: the minimap sits in a `Corners` slot shared with other HUD mods (corner and offsets), and a free position would need a new setting kind and a minimap preview on the settings screen (the HUD minimap isn't drawn under the mod screen). Night lighting has no block light. Auto size is off by default (it matches the old size only at a 640x360 GUI). Checked with `cargo check` and the runtime tests, not yet in the game |

### Not doable on 9f42064c

Anti-aliased minimap (needs a linear sampler for `native_sprite`), keys in Controls through
Horizon's own API (needs `Registrar::register_key_mapping`), server map mode and server-enforced
settings (need a server component), integrations with Java mods.
