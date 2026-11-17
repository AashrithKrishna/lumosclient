#!/usr/bin/env python3
"""Merges every built group's lumos-native-*.json into native-index.json, which Lumos reads
to install native mods: for each mod, the builds that have it, per platform.

    python3 tools/lumos_native/index.py --dir parts/ --out native-index.json
"""
import argparse
import datetime
import glob
import json
import os


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dir", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    builds = []
    for f in sorted(glob.glob(os.path.join(a.dir, "lumos-native-*.json"))):
        b = json.load(open(f))
        if b.get("schema") == 1 and b.get("asset") and b.get("sha256"):
            builds.append(b)
    out = {"schema": 1, "generated": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
           "builds": builds}
    with open(a.out, "w") as fh:
        json.dump(out, fh, indent=1)
    print(f"{len(builds)} builds, {len({m['id'] for b in builds for m in b['mods']})} mods")


if __name__ == "__main__":
    main()
