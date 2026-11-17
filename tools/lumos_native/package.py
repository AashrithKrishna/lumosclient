#!/usr/bin/env python3
"""Packages one built native-mod group for one platform.

    python3 tools/lumos_native/package.py --plan plan.json --commit <sha> --work work/ \
        --bin <folder with horizon-client[.exe], horizon-prepare[.exe] and anything beside them> \
        --exporters <folder with server/, client/, manifest.json> --platform macos-arm64 --out dist/

Makes dist/lumos-native-<short>-<platform>.zip:
  bin/                 the game with the group's mods built in, its preparer (+ DXC on Windows)
  horizon-root/        what that preparer needs: research-lock.json, the parity sources, exporters
  native.json          the commit, the mods built in (id, version, name, can_turn_off) and the ones left out
and dist/lumos-native-<short>-<platform>.json (native.json plus the zip's sha256 and size).
"""
import argparse
import hashlib
import json
import os
import shutil
import zipfile


def add_tree(z, folder, arc):
    for root, _, files in os.walk(folder):
        for f in files:
            full = os.path.join(root, f)
            z.write(full, os.path.join(arc, os.path.relpath(full, folder)).replace(os.sep, "/"))


def main():
    ap = argparse.ArgumentParser()
    for k in ("plan", "commit", "work", "bin", "exporters", "platform", "out"):
        ap.add_argument(f"--{k}", required=True)
    a = ap.parse_args()
    plan = json.load(open(a.plan))
    g = next(g for g in plan["groups"] if g["commit"] == a.commit)
    applied = json.load(open(os.path.join(a.work, "applied.json")))
    by_id = {m["id"]: m for m in g["mods"]}
    mods = []
    for i in applied["applied"]:
        folder = os.path.join(a.work, "mods", i)
        text = ""
        for root, _, files in os.walk(folder):
            for f in files:
                if f.endswith(".rs"):
                    text += open(os.path.join(root, f), encoding="utf-8", errors="ignore").read()
        m = by_id[i]
        # A built-in mod honours horizon-mods.txt only when its own hooks ask native_on().
        mods.append({"id": i, "version": m["version"], "name": m["name"], "sha256": m["sha256"], "can_turn_off": "native_on(" in text})
    native = {"schema": 1, "commit": g["commit"], "short": g["short"], "mixins": g["mixins"], "platform": a.platform,
              "combined": bool(g.get("combined")),
              "mods": mods, "left_out": applied["failed"]}
    os.makedirs(a.out, exist_ok=True)
    name = f"lumos-native-{'all-' if g.get('combined') else ''}{g['short']}-{a.platform}"
    zpath = os.path.join(a.out, name + ".zip")
    src = os.path.join(a.work, "src")
    with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
        for f in sorted(os.listdir(a.bin)):
            full = os.path.join(a.bin, f)
            if os.path.isfile(full):
                info = zipfile.ZipInfo.from_file(full, "bin/" + f)
                info.external_attr = (0o755 << 16) if not f.endswith((".txt", ".json")) else (0o644 << 16)
                info.compress_type = zipfile.ZIP_DEFLATED
                with open(full, "rb") as fh:
                    z.writestr(info, fh.read())
        z.write(os.path.join(src, "research-lock.json"), "horizon-root/research-lock.json")
        add_tree(z, os.path.join(src, "mods/horizon/src/test/java/dev/horizonlab/parity"),
                 "horizon-root/mods/horizon/src/test/java/dev/horizonlab/parity")
        for part in ("server", "client"):
            add_tree(z, os.path.join(a.exporters, part), f"horizon-root/exporters/{part}")
        z.write(os.path.join(a.exporters, "manifest.json"), "horizon-root/exporters/manifest.json")
        z.writestr("native.json", json.dumps(native, indent=1))
    data = open(zpath, "rb").read()
    native["asset"] = name + ".zip"
    native["sha256"] = hashlib.sha256(data).hexdigest()
    native["size"] = len(data)
    with open(os.path.join(a.out, name + ".json"), "w") as fh:
        json.dump(native, fh, indent=1)
    print(zpath)


if __name__ == "__main__":
    main()
