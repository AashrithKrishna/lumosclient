# Native mod ports

Registry native mods patched by Lumos so they build against the Horizon commit of Lumos's combined
("all-in-one") game, `9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`. Each `<id>/` folder is the mod's
`mixins/<id>/` folder from its registry release, changed only where Horizon's code moved, with
`[compatibility] horizon_revision` set to that commit. `tools/lumos_native/stage.py` uses a port in
place of the registry copy for the combined game only; the per-commit group games keep the originals.

Every port keeps its original licence and authors. `PORT.md` in each folder names the registry
version it started from and lists what was changed and why.
