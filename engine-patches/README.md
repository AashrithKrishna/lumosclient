# Engine patches

Lumos's own changes to the Horizon engine that the native builds compile. Each `NN-*.patch` is a
plain `git diff` against the commit in `base-commit.txt` (Horizon 9f42064c, the combined game's
commit), and each applies on top of the ones before it. `tools/lumos_native/stage.py` applies them
in name order with `git apply`, right after it extracts that commit's source and before any mod;
a patch that doesn't apply stops the build, so the game never ships without one silently. A group
built at another commit gets none.

Every patch keeps the picture and the game as they were. The checks, for each one: the replay
screenshots of the benchmark views (below) are identical byte for byte with and without it, the
touched crates' tests pass as before (`horizon-client`'s 1,417; `horizon-client-render`'s 2,252, whose
2 crash-report failures happen without the patches too, in the `fast` profile, which has no line
tables), and the same mods apply on top: 89, and the same 57 left out, with and without them.

| Patch | What it speeds up | Measured gain (Apple M4, 2940x1846) |
| --- | --- | --- |
| `01-game-dir-cache.patch` | `Game::game_dir` (the game folder, the working directory) is kept once known instead of asking the OS each time. Native mods ask for their folders (`native_data_dir`, `native_config_dir`) several times a frame, and Horizon itself once a frame (`shader_packs_dir`); on macOS each `getcwd` opens the folder. Only the launch options change the working directory, before the first frame, and they forget the kept one then. | Main-thread CPU: `getcwd` took 13% of a modded frame (village view) and 4% of a vanilla one; it no longer shows in the profile (the mods' per-frame hooks, `extras_frame`, went from 16.5% to 4.5% of the main thread). In the game with mods: CPU per frame 3.35 to 3.63 ms before, 2.69 to 2.96 ms after (-13 to -26%; 02 and 03 change only GPU work) |
| `02-lean-cloud-shaders.patch` | The clouds' vertex shader passes their overlay (4, which every cloud vertex has) as a constant, so the compiler drops what unlit sky geometry never uses (the lightmap fetch, dynamic and coloured lights), and their fragment shader is `fs_entity` with that constant (`fs_cloud`): the other overlays' branches and the discard go at compile time. At the default 128-chunk cloud distance the clouds are about 38,000 quads every frame. | GPU per frame: open view 3.02 to 2.74 ms (-9%), forest -7%, village -4%, at the wall -4% |
| `03-terrain-without-unused-extras.patch` | The solid terrain is also built without the code of the emissive overlays (OptiFine `_e` textures, native mods' `emissive/`), RGSS sampling (Texture Filtering's RGSS) and native coloured lights (new pipeline constants in `terrain.wgsl`), and a frame that uses none of them and has HDR output off draws with that pipeline (`SolidPipeline::get_for`). Branches never taken still slow Apple GPU shaders; the frames that use any of the three keep the full shaders. | GPU per frame on top of 02: open view 2.71 to 2.48 ms (-9%) |
| `04-lumos-mod-switches.patch` | Not a speed-up: the switch every native mod's hooks check in the all-in-one game. The switches are an allow-list: a built-in mod runs only while its line in the game folder's `horizon-mods.txt` says `mod.<id>:true`; one whose line says `false`, or that has no line at all, is off (a missing line used to mean on, and a built-in mod an instance's file didn't name ran unasked). `horizon_core::lumos_mods` holds the mods turned on (the settings' `ModList` hands its `enabled` set over when it loads, saves or toggles, and `sync_native_switches`, which also sets the trait mods' `mod_api` switches from it; before the settings load they are read from `horizon-mods.txt` in the game folder, `--game-dir` (relative to where the game started; the launch options fix it before they move there) else the working directory, so start-up hooks follow them too) and the `lumos_mod_on!`/`lumos_mod_shown!` checks, two atomic loads each, that `tools/lumos_native/gate.py` puts first in every hook. The mods off when the game started (the ones the instance doesn't have) are listed nowhere: Options > Mods leaves them out (`list_native_mods`), and with it Key Binds (Lumos Controls), Mod Menu and the crash report's Native Mods line; a mod the player turns off during the session stays listed. Lumos's launcher writes `:true` for every native mod the player installed and `:false` for the build's others before each launch, keeping a `:false` the player chose for an installed one (`obsidian-launcher-rs/src/native_builds.rs`). `horizon-threads` and `horizon-mods` depend on `horizon-core` for the hooks mods put in them. | none (each hook: two atomic loads) |
| `05-carried-item-before-click.patch` | Not a speed-up: a fix. On a server, a number key and a click in the same tick reached the server in the wrong order: the tick sends the selected slot (`remote_send_carried`) before the clicks, but the number keys are read after that (`pick_tick_targets`), so the attack, jab, use or block break went out with the old slot and the new slot followed a tick later. Vanilla's `MultiPlayerGameMode` calls `ensureHasSentCarriedItem` before each of those (`attack`, `piercingAttack`, `startDestroyBlock`, `continueDestroyBlock`, `useItemOn`, `useItem`, `releaseUsingItem`); the remote clicks now do too, so attribute swapping (a swap and a hit in one tick) hits with the new item on the server, as on vanilla. Singleplayer goes the same way (the window is a client of its integrated server thread); a LAN host's own clicks send nothing and are unchanged. | none |
| `06-attribute-swap.patch` | Not a speed-up: a fix, the server half of attribute swapping (05 is the client half). The server (the integrated one too, so singleplayer) put a player's main-hand attribute modifiers on as soon as anything ran as them (`as_guest`), so the SetCarriedItem and the attack handled before the player's tick gave the hit the new item's ATTACK_SPEED and ATTACK_DAMAGE with the charge built up under the old one: a sword charged for 20 ticks and swapped to a mace hit at (20 + 0.5) / 33.3 = 0.62 strength (weak, no crit), and a jab's minimum-charge check (`cannotAttackWithItem`) used the new item's speed. Vanilla only changes them in `ServerPlayer.tick` (`detectEquipmentUpdates`), with the attack strength reset (`Player.tick`'s `lastItemInMainHand`): the hit is the new item's (its enchantments, a mace's smash, an axe's shield disabling) at the old item's full charge, damage and speed. The modifiers now follow the main hand as of the player's last tick (`GuestPlay::ticked_main`, taken with the reset in `guest_main_hand_tick`). Test: `lan_actions::attribute_swap_tests` (needs `HORIZON_TEST_RESOURCES`). | none |

All three together, the replay of each view (terrain, sky, clouds and water; GPU milliseconds per
frame, median of 300 frames, mean of 6 interleaved runs each):

| View | Before | After | GPU time | FPS where the GPU is the limit |
| --- | --- | --- | --- | --- |
| open terrain, render distance 12 | 2.58 ms | 2.32 ms | -10% | +11% |
| forest | 2.86 ms | 2.31 ms | -19% | +24% |
| village | 2.08 ms | 1.83 ms | -12% | +13% |
| at a wall | 2.01 ms | 1.79 ms | -11% | +12% |

In the game itself (the combined build with Lumos's mods, built locally from this repo without
and with the patches, the same scratch world and settings):

| View | Before | After | FPS (median frame) | Average FPS | CPU per frame | World pass (GPU) |
| --- | --- | --- | --- | --- | --- | --- |
| open terrain | 3.61 ms | 3.27 ms | 277 to 306 (+10%) | 256 to 295 (+15%) | 3.47 to 2.93 ms | 3.06 to 2.63 ms |
| forest | 3.61 ms | 3.38 ms | 277 to 296 (+7%) | 270 to 287 (+6%) | 3.35 to 2.83 ms | 3.32 to 3.00 ms |
| village | 3.57 ms | 3.09 ms | 280 to 324 (+16%) | 276 to 317 (+15%) | 3.44 to 2.96 ms | 2.41 to 2.10 ms |
| at a wall | 3.77 ms | 2.81 ms | 265 to 356 (+34%) | 262 to 349 (+33%) | 3.63 to 2.69 ms | 2.10 to 1.71 ms |

(Two runs of each build, alternated, 8 s per view; `max fps 260` is unlimited.) With the mods the
game is mostly limited by the main thread's CPU, which is why 01 matters most there; the forest
view is limited by the GPU.

## Measuring

How the numbers above were taken, so the next change can be checked the same way.

- **Settings**: the player's own `horizon-graphics.txt` and `options.txt` (render distance 12, FOV
  110, fancy, clouds at 128 chunks, entity, particle and hidden-leaf culling and the block-entity
  caches on), VSync off, in a scratch game home and a fresh creative world (seed 1234, time and
  weather frozen at noon and clear). `--offscreen --width 1470 --height 923` renders 2940x1846 on
  a Retina Mac, as the player's window.
- **Views**: open terrain from (-519.5, 110, 54.5) facing south 15 degrees down; the forest and
  river from (-552.5, 80, 22.5) facing west; the plains village from (-960.5, 90, 140.5); and a
  stone wall filled at (-532..-507, 69..88, 57), looked at from 3 blocks.
- **Replay** (GPU, exact): `horizon-ctl render-capture on`, then `render-capture DIR` at each
  view, and Horizon's `scene_replay` example on the captures, alternating the two builds six times
  per view (GPU time varies by a tenth between runs of the same build; one run proves nothing).
  `--screenshot` saves the frame: compare the decoded RGBA bytes of both builds' screenshots, at
  `--yaw-offsets` 0, 90, 180 and 270 of each view. (Pillow's `ImageChops.difference(...).getbbox()`
  only looks at alpha on RGBA images: compare the bytes.)
- **In game**: `horizon-ctl bench 8` at each view after 8 s to settle, with `gpu-passes on`;
  `cpu.p50_ms`, `gpu.p50_ms` and `p50_ms` (the frame) are the medians of every frame. Wait 40 s
  after the world opens: the vignette's fade-out costs ~0.5 ms of GPU a frame until then.
- **Profiles**: macOS `sample PID 6` on the running game; the main thread's call tree shows where
  the CPU goes (the getcwd calls above came from it).
- Other sessions compiling on the same Mac move every number: measure with the load average
  low, and alternate the builds.

## Tried and left out

- **The far-tile sampling path** (`sample_nearest`'s `tiled` branch) left out the same way as 03's
  extras: another 7% of GPU time, but not exact. A section's interpolated UV can round to just
  above 1.0, which the full shader already treats as a tile's, and up to 117 pixels a view came
  out one level (1/255) different. Left out.
- **The fade dither** (`faded`) left out when no section or tile fades: no measurable gain.
- **Occlusion culling forced on** (`HORIZON_GPU_CULL=all`, Horizon's own two-phase test): at this
  resolution its extra passes cost more than the hidden sections save (open view +0.3 ms, at the
  wall +0.3 to +3 ms: the box test shades every box's pixels at 2940x1846). Horizon's default
  (on Metal only past 1,600 sections in view) stays.
- **Separate hand and screen-effect passes**: a game with native mods draws the first-person hand
  in a pass of its own (0.15 to 0.28 ms of GPU), because a mod may draw in `native_world_drawn`
  between them; the vignette while it fades (0.5 ms). Merging them needs every HUD pipeline in a
  depth-compatible variant; not done.

## Seen, not engine work

- In the game with mods a full-screen "Screen effects" pass runs every frame (0.37 ms of GPU at
  this size) where the vanilla game draws it only while the vignette fades: some mod's full-screen
  GUI overlay, found by its blit at (0, 0) (`screen_effects.rs`).
- Waymap reads its three settings files from disk every frame (`waymap::load_settings` calls
  `native_setting` before checking `settings_loaded`): about 2% of the main thread.
