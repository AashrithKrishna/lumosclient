# Horizon 1.21.11 Rust Port — Phase 1 Audit & Blueprint

Branch: `port-1.21.11` (created from `main`)

## 1. Verified `1.21.11` Metadata & Checksums

Read from `~/Library/Application Support/minecraft/versions/1.21.11/1.21.11.json` and `1.21.11.jar`'s `version.json`:

| Field | `26.3` (Current Pin) | `1.21.11` (Target) |
| --- | --- | --- |
| `VERSION_ID` / `VANILLA_VERSION` | `"26.3"` | `"1.21.11"` |
| `PROTOCOL_VERSION` | `777` | `774` |
| `DATA_VERSION` (`world_version`) | `5023` | `4671` |
| `RESOURCE_PACK_FORMAT` | `(97, 1)` | `(75, 0)` |
| `DATA_PACK_FORMAT` | `(121, 0)` | `(94, 1)` |
| `JAVA_VERSION` | `25` | `21` |
| `BUILD_TIME` | `"Tue Sep 15 11:20:48 UTC 2026"` | `"Tue Dec 09 12:20:42 UTC 2025"` |
| `manifest_sha1` | `11f1aefe473eefba0af62e3286c33b692807b2eb` | `b8e7e6ddb3ca94298c38391fbda1455e29b42949` |
| `client_sha1` | `e877b6a07acd633fb3bb475002175cec036e7b87` | `ba2df812c2d12e0219c489c4cd9a5e1f0760f5bd` |
| `server_sha1` | `33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c` | `64bb6d763bed0a9f1d632ec347938594144943ed` |
| Official Mappings | Unobfuscated (`26.1+`) | `client_mappings` (`031a68be...`) & `server_mappings` (`5621e925...`) |

**Key finding:** Because Mojang ships official ProGuard mappings (`client.txt` and `server.txt`) in `1.21.11.json`, and Horizon's Java exporters already use Mojang's official names, `1.21.11` jars can be remapped to official Mojang names during `horizon-prepare` / `tools/lab.py` rather than rewriting all exporters to obfuscated names.

---

## 2. File-by-File Port Checklist

### A. Pin & Resource Preparation (`research-lock.json`, `crates/horizon-prepare/`)
- **`research-lock.json`**: Update `minecraft` block to `1.21.11` (and record `client_mappings` / `server_mappings` hashes).
- **`crates/horizon-prepare/src/artifacts.rs`**:
  - Add ProGuard-to-official remapping step (for obfuscated `< 26.1` jars using `client_mappings` / `server_mappings`) before compiling or running exporters.
  - Verify `1.21.11` `server.jar` unpack (`net.minecraft.data.Main --reports`).
- **`crates/horizon-prepare/src/exporters.rs`** (`18` Java files in `mods/horizon/src/test/java/dev/horizonlab/parity/`):
  - **Server (`15` + `PrepareWorker`):** `VanillaHeightmapOracle`, `VanillaBlockLandingOracle`, `VanillaEntityAttributeOracle`, `VanillaPlayerHealthOracle`, `VanillaDamageSourceOracle`, `VanillaCollisionOracle`, `VanillaFaceVisibilityOracle`, `VanillaGameplayFactsExporter`, `VanillaEntityTypesExporter`, `VanillaBlockBehaviourExporter`, `VanillaDataFixTablesExporter`, `VanillaCommandParseOracle`, `VanillaCreativeTabsExporter`, `VanillaSimpleBlockOracle`, `VanillaStructureBlockExporter`.
  - **Client (`3` + `PrepareWorker`):** `VanillaBlockStateBindingOracle`, `VanillaEntityModelExporter`, `VanillaMobDataExporter`.
  - Patch references to `26.1–26.3`-only classes/registries (e.g., `world_clock`, `block_transformer`, `worldgen/block_state_provider`).

### B. Regenerated Tables (`tools/port_regen.py`)
1. `crates/horizon-core/src/version.rs` (`tools/gen_version_consts.py`)
2. `crates/horizon-protocol/src/ids.rs` (`tools/generate_packet_ids.py`)
3. `crates/horizon-client/src/lan/tracking.rs` (`tools/generate_entity_tracking.py`)
4. `crates/horizon-client/src/lan/command_order.rs` (`tools/generate_lan_command_order.py`)
5. `crates/horizon-client/src/lan/command_suggestions.rs` (`VanillaCommandSuggestionsExporter.java` + `tools/generate_command_suggestions.py`)
6. `crates/horizon-command/src/vanilla_order.rs` (`VanillaCommandParseOracle.java` + `tools/generate_command_order.py`)

### C. Protocol & Wire Layout (`crates/horizon-protocol/`)
Already cataloged in `crates/horizon-protocol/src/adapter/{translate.rs,layout_26_2.rs,v1_21_11.rs,v1_21_11_ids.rs}`:
- **Registry counts (`26.3` → `1.21.11`):**
  - Block states: `35,723` → `29,671`
  - Blocks: `1,286` → `1,166`
  - Items: `1,658` → `1,505`
  - Entity types: `161` → `157`
  - Sound events: `1,991` → `1,838`
  - Particle types: `128` → `115`
  - Data components: `122` → `104`
- **Registries absent in `1.21.11`:** `minecraft:world_clock` (26.1), `minecraft:decorated_pot_pattern`, `minecraft:block_transformer`, `minecraft:worldgen/block_state_provider` (26.3).
- **Packet & Codec differences (`1.21.11` vs `26.3`):**
  - `ChunkSection`: `1.21.11` encodes block count only (no fluid count after block count).
  - `SetTime`: `1.21.11` encodes `(game_time, day_time, ticking)` instead of `world_clock` array.
  - `AgeableMob` entity metadata: `1.21.11` lacks the extra boolean inserted after index `16` in `26.1` (`39` entity types).
  - Serverbound `swing` / `interact` / `attack` / `punch`: `1.21.11` uses combined `Interact` (actions `0`/`1`/`2`) and `Swing` rather than `26.1`/`26.3` split packets.
  - `CommonPlayerSpawnInfo` (`i8` game types vs `VarInt`), `UPDATE_ADVANCEMENTS` tab position, `LevelParticles`, `MoveEntity`, `TeleportEntity`, `LightUpdate` bitset (`toLongArray` vs `toByteArray`), `SignUpdate` side (`bool` vs `VarInt`).

### D. Worldgen, DataFixers, Items & Gameplay (`crates/horizon-world/`, `horizon-items/`, `horizon-assets/`, `horizon-client/`)
- **Removed/Changed `26.1–26.3` Features:**
  - `minecraft:block_transformer` (`crates/horizon-items/src/items.rs`, `crates/horizon-assets/src/item_model.rs`, `crates/horizon-client/src/block_transform.rs`, `crates/horizon-client/src/game.rs`): `1.21.11` handles axe/hoe/shovel block transforms directly without a synced `block_transformer` registry/component.
  - `minecraft:world_clock` & `world_clocks.dat` (`crates/horizon-client-core/src/registries.rs`, `crates/horizon-command/src/args.rs`, `crates/horizon-client/src/save.rs`): `1.21.11` stores time directly in `level.dat` (`Time` / `DayTime`).
  - `minecraft:chain` vs `minecraft:iron_chain` (`crates/horizon-world/src/worldgen/rule_test.rs`, `crates/horizon-port-pack/src/carry.rs`): `1.21.11` uses `minecraft:chain`.
  - Worldgen overrides & structure templates (`crates/horizon-world/src/worldgen/overrides.rs`, `structure/template.rs`): `1.21.11` uses `Name`/`Properties` and `configured_carver`/`configured_feature`.
  - DataFixers (`crates/horizon-world/src/datafix/`): target `DATA_VERSION = 4671` instead of `5023`.

---

## 3. Recommended Phase 2+ Execution Order
1. **Phase 2A (Prepare Pipeline):** Add ProGuard `client_mappings` / `server_mappings` remapping to `horizon-prepare` / `tools/lab.py` and verify all `18` exporters compile & export `1.21.11` resources into `.local/horizon-client/1.21.11/`.
2. **Phase 2B (Regen & Protocol):** Pin `1.21.11` in `research-lock.json`, run the 6 generators (`tools/port_regen.py`), and switch `horizon-protocol` packet codecs/adapters to `774`.
3. **Phase 2C (World / Items / Client Compilation):** Update `horizon-world`, `horizon-items`, `horizon-assets`, and `horizon-client` for `1.21.11` deltas (`world_clock`, `block_transformer`, `chain`, `DataVersion 4671`) until `cargo build --release -p horizon-game` passes.
4. **Phase 2D (Smoke & Parity):** Boot `horizon-client --resources .local/horizon-client/1.21.11` and run differential parity harnesses against `1.21.11`.
