# pickupnotifier: Lumos port

Started from the registry release **pickupnotifier 1.1.0** (game 26.3), `pickupnotifier-1.1.0.hnmod`,
SHA-256 `2b38c66503114deb8f58c62c7c1f64ca31568c947c1b32b91395400027bd2af2`. It was written for Horizon `fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca` with horizon-mixins `5a64af938ec86b00d53d16a408aabc0038fb3e57`.

Retargeted to Horizon `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0` (horizon-mixins `e59da9094f0c2fdda6c627fe9eff22dec6a906f0`),
the commit of Lumos Client's combined game.

Licence: `MPL-2.0` (as declared in Horizon's mod registry; the release carries no licence file of its own).
Authors: DeportedToHell. All the mod's code is theirs; the changes below are Lumos's.

## Changes

- `[compatibility] horizon_revision` set to 9f42064c.
- `glue.rs`: an experience orb's `value` is an `i32` now; converted to the `u32` the entry counts in.
