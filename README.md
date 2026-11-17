<p align="center">
  <img src="obsidian-launcher-rs/assets/lumos_logo_128.png" alt="Lumos Client" width="96">
</p>

<h1 align="center">Lumos Client</h1>

<p align="center">
  <b>A fast, good-looking Minecraft: Java Edition launcher for macOS and Windows.</b><br>
  Pick a version, press Launch. Lumos downloads the game, Java and your mods for you.
</p>

<p align="center">
  <a href="https://github.com/AashrithKrishna/lumosclient/releases/latest"><b>⬇ Download Lumos Client</b></a>
</p>

## Download

Get the latest version from the [Releases page](https://github.com/AashrithKrishna/lumosclient/releases/latest):

| Computer | File | How to install |
|---|---|---|
| **Mac** (Apple silicon or Intel, macOS 13+) | `Lumos.dmg` | Open it and drag **Lumos** into Applications. The first time, right-click Lumos and choose **Open**. |
| **Windows** 10 or 11 (64-bit) | `Lumos-Windows-x64.zip` | Unzip it anywhere and double-click **Lumos.exe**. If Windows shows "Windows protected your PC", click **More info**, then **Run anyway**. |

These are the only official Lumos Client downloads. Lumos isn't signed with a paid certificate yet, which is why your computer asks before opening it.

## What you get

- **Every version, ready in one click:** vanilla and Fabric releases, with Java downloaded automatically.
- **Instances:** separate setups, each with its own version, mods, resource packs, shaders and worlds.
- **Mods & packs per instance:** browse Modrinth inside the launcher and pick which instance to install into. Dependencies come along automatically. You can also add your own `.jar` and `.zip` files, or switch any of them off.
- **Skins and capes with Ely.by:** sign in with a free Ely.by account and choose a skin or cape from the catalog inside the launcher. Other Ely.by players see them in game.
- **LumosHUD:** FPS, CPS, coordinates and keystrokes in game (Fabric 1.21.11).
- **Horizon 26.3 (Rust):** an optional engine that plays Minecraft 26.3 without Java. Lumos sets it up the first time you choose it.
- **Servers:** save servers, see their ping, join in one click.

## What you need

- Your own copy of **Minecraft: Java Edition** to play online. Offline play works without an account.
- An internet connection the first time you play a version.

Your instances and settings are stored in `~/LumosClient` on a Mac and in `%USERPROFILE%\LumosClient` on Windows.

## Build it yourself

- **Mac:** `./obsidian-launcher-rs/build_app.sh` creates `dist/Lumos.app` and `dist/Lumos.dmg`.
- **Windows:** run the **lumos windows** workflow under Actions, then `./obsidian-launcher-rs/package_windows.sh <downloaded programs folder>` to create `dist/Lumos-Windows-x64.zip`.

The launcher's code is in [`obsidian-launcher-rs/`](obsidian-launcher-rs).

## Credits and licences

Lumos Client includes the **Horizon** engine by the Horizon Team. Horizon is released under the [GNU LGPL, version 3 or later](LICENSE), and its older code under the [MIT licence](LICENSES/MIT.txt). Lumos is not made or endorsed by the Horizon Team. Horizon's own project page is in [HORIZON.md](HORIZON.md), and its upstream repository is [horizon-rust/horizon](https://github.com/horizon-rust/horizon).

Lumos bundles [authlib-injector](https://github.com/yushijinhun/authlib-injector) (AGPL-3.0), unmodified, to show Ely.by skins in game. Other dependencies keep their own licences; see the [notices](THIRD_PARTY_NOTICES.md).

<sub>NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT. Minecraft is a trademark of Mojang Synergies AB. Lumos doesn't include any Minecraft files: it downloads them from Mojang for each player.</sub>
