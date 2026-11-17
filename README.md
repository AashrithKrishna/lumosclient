<p align="center">
  <img src="obsidian-launcher-rs/assets/lumos_logo_128.png" alt="Lumos Client" width="96">
</p>

<h1 align="center">Lumos Client</h1>

<p align="center">
  <b>A fast, good-looking Minecraft: Java Edition launcher for macOS, Windows and Linux.</b><br>
  Pick a version, press Launch. Lumos downloads the game, Java and your mods for you.
</p>

<p align="center">
  <a href="https://github.com/AashrithKrishna/lumosclient/releases/latest"><b>⬇ Download Lumos Client</b></a>
</p>

## Download

Get the latest version from the [Releases page](https://github.com/AashrithKrishna/lumosclient/releases/latest):

| Computer | File | How to install |
|---|---|---|
| **Mac** (Apple silicon or Intel, macOS 13+) | `Lumos-macOS-universal.dmg` | Open it and drag **Lumos** into Applications. The first time, right-click Lumos and choose **Open**. |
| **Windows** 10 or 11 (64-bit) | `Lumos-Windows-x64.zip` | Unzip it anywhere and double-click **Lumos.exe**. If Windows shows "Windows protected your PC", click **More info**, then **Run anyway**. |
| **Linux** (x86_64, glibc 2.35+: Ubuntu 22.04, Fedora 36, Debian 12 or newer) | `Lumos-Linux-x86_64.tar.gz` | Extract it anywhere and run `./lumos-launcher`; `./install-desktop-entry.sh` adds Lumos to your applications menu. Needs GTK 3 and WebKitGTK 4.1 (`libwebkit2gtk-4.1-0` on Ubuntu/Debian). |

These are the only official Lumos Client downloads. Lumos isn't signed with a paid certificate yet, which is why your computer asks before opening it.

## What you get

- **Every version, ready in one click:** vanilla and Fabric releases, with Java downloaded automatically.
- **Instances:** separate setups, each with its own version, mods, resource packs, shaders and worlds.
- **Mods & packs per instance:** browse Modrinth inside the launcher and pick which instance to install into. Dependencies come along automatically. You can also add your own `.jar` and `.zip` files, or switch any of them off.
- **Skins and capes with Ely.by:** sign in with a free Ely.by account and choose a skin or cape from the catalog inside the launcher. Other Ely.by players see them in game.
- **LumosHUD:** FPS, CPS, coordinates and keystrokes in game (Fabric 1.21.x).
- **Horizon 26.3 (Rust):** an optional engine that plays Minecraft 26.3 without Java. Lumos sets it up the first time you choose it, and installs Horizon's own mods, shaders and packs, including its native Rust mods as ready-made builds (on Apple silicon Macs and Windows x64).
- **Boost FPS:** one click per Fabric instance adds the best performance mods for its version and tuned Java settings.
- **Servers:** save servers, see their ping, join in one click.

## What you need

- Nothing to buy to play: sign in offline, or with a free [Ely.by](https://ely.by) account for skins.
- Lumos doesn't sign in with Microsoft accounts yet, so it can't join servers that require a paid Minecraft account (online-mode servers such as Hypixel). It plays singleplayer, LAN, and servers that allow offline or Ely.by players.
- An internet connection the first time you play a version.

Your instances and settings are stored in `~/LumosClient` on Mac and Linux and in `%USERPROFILE%\LumosClient` on Windows.

## Build it yourself

- **All platforms:** run the **lumos release** workflow under Actions with the version from `obsidian-launcher-rs/Cargo.toml`. It builds the universal Mac app, the Linux and Windows downloads, and attaches them to a draft release.
- **Mac:** `./obsidian-launcher-rs/build_app.sh` creates `dist/Lumos.app` and `dist/Lumos.dmg` for this Mac's own processor.
- **Linux:** build `lumos-launcher`, `horizon-client` and `horizon-prepare` on Linux, then `./obsidian-launcher-rs/package_linux.sh <folder with them>` creates `dist/Lumos-Linux-x86_64.tar.gz`.
- **Windows:** run the **lumos windows** workflow under Actions, then `./obsidian-launcher-rs/package_windows.sh <downloaded programs folder>` to create `dist/Lumos-Windows-x64.zip`.

The launcher's code is in [`obsidian-launcher-rs/`](obsidian-launcher-rs).

## Credits and licences

Lumos Client includes the **Horizon** engine by the Horizon Team. Horizon is released under the [GNU LGPL, version 3 or later](LICENSE), and its older code under the [MIT licence](LICENSES/MIT.txt). Lumos is not made or endorsed by the Horizon Team. Horizon's own project page is in [HORIZON.md](HORIZON.md), and its upstream repository is [horizon-rust/horizon](https://github.com/horizon-rust/horizon).

Lumos bundles [authlib-injector](https://github.com/yushijinhun/authlib-injector) (AGPL-3.0), unmodified, to show Ely.by skins in game. Other dependencies keep their own licences; see the [notices](THIRD_PARTY_NOTICES.md).

<sub>NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT. Minecraft is a trademark of Mojang Synergies AB. Lumos doesn't include any Minecraft files: it downloads them from Mojang for each player.</sub>
