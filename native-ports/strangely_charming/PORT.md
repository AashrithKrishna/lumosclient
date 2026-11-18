# strangely_charming: Lumos port

Started from the registry release **strangely_charming 0.2.11** (game 26.3), `strangely_charming-0.2.11.hnmod`,
SHA-256 `fb5d84f5f8db78cd48226581c3706742e913a34a9047069056a0007f34abee7f`. It was written for Horizon `2ebf00a0281c592f79191e76fce0e7d522c6f425` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MIT` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: Evander. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- Targets moved into `game` submodules: `first_person_arm` (`frame_geometry`), `portal_shape` (`nether_portal`), `use_instrument` (`using_item`), `creative_tab_open` (`creative_screen`), `play_mouse` (`play_input`), `draw_inventory_slots` (`inventory_slots`).
- `animal-templates.rs`: its copy of `Content::read_declaring` fills the new `Content::fluids` field (empty, as Horizon's does).
- `animal-models.rs`: `MobTypes::load_with_models` takes the item defaults now; `evanders_models::load` passes them through. Horizon 9f42064c also added `MobTypes::load_with_defaults` and `load_with_document_and_defaults`, which the client uses at startup, so two more OVERWRITE injections (`animal-models-defaults`, `animal-models-document-defaults`) route them through the mod's loader like the other three.
- `captain-drop.rs`: `dispenser::new_item_motion` is gone; the raid horn now spawns with `add_item_entity(.., None, 10)`, which draws the item constructor's default motion as vanilla's `spawnAtLocation` does.
- `anvil-label.rs`: a block's custom name (`world.block_names`) is a text component (`Tag`) now; read to plain text as Horizon's `block_items.rs` does.
- `job-session.rs`: a mob's `random` is an `EntityRandom` now; copied into the merchant session with `clone_from`, as Horizon's `trading.rs` does.
