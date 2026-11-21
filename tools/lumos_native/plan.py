#!/usr/bin/env python3
"""Plans Lumos Client's native-mod builds from Horizon's mod registry.

Every native mod pins the Horizon commit it patches (its version's `api`,
`native-1/<horizon>/<mixins>`). Mods pinned to the same commit are built together into
one game, with the newest horizon-mixins tool any of them pins.

    python3 tools/lumos_native/plan.py --out plan.json [--only fdc87aec,6f8c7a0e] [--matrix]

Writes plan.json: {"groups": [{"commit", "short", "mixins", "mods": [{id, version, name,
url, sha256, size}]}]}. With --matrix, prints the GitHub Actions matrix of commits.
"""
import argparse
import json
import os
import re
import sys
import urllib.request

INDEX = "https://horizon-rust.github.io/horizon-mods/index.json"
UA = {"User-Agent": "LumosClient-native-builds"}
SHA = re.compile(r"[0-9a-f]{40}")
ID = re.compile(r"[a-z0-9_]{1,64}")


def get_json(url, token=None):
    headers = dict(UA)
    if token:
        headers["Authorization"] = f"Bearer {token}"
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=120) as r:
        return json.load(r)


def commit_date(rev, token):
    try:
        c = get_json(f"https://api.github.com/repos/horizon-rust/horizon-mixins/commits/{rev}", token)
        return c["commit"]["committer"]["date"]
    except Exception as e:  # unknown revision: sorts first, so a known newer one wins
        print(f"warning: no date for horizon-mixins {rev}: {e}", file=sys.stderr)
        return ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="plan.json")
    ap.add_argument("--only", default="", help="comma-separated commit prefixes to keep")
    ap.add_argument("--matrix", action="store_true")
    ap.add_argument("--combined", default="", help="one group: every native mod retargeted to this Horizon commit")
    a = ap.parse_args()
    token = os.environ.get("GITHUB_TOKEN")
    idx = get_json(INDEX)
    # Horizon's registry moved its native-v1 mods to an archive (2026-11-18); keep building them
    # from Lumos's frozen copy of those listings. The frozen listing wins over the live one: an id
    # the live index still has now carries releases for newer Horizon commits or the newer mod API
    # (flair, freecam_plus, imfast, living_camera, keystrokes, fps_overlay, right_click_crops in
    # 2026-11), which the pinned game can't build, and would otherwise drop out of the build.
    frozen = os.path.join(os.path.dirname(os.path.abspath(__file__)), "native-v1-index.json")
    if os.path.exists(frozen):
        f = json.load(open(frozen, encoding="utf-8"))
        kept = {p.get("id") for p in f.get("projects", [])}
        idx["projects"] = [p for p in idx.get("projects", []) if p.get("id") not in kept] + f.get("projects", [])
        idx.setdefault("revoked", []).extend(f.get("revoked", []))
    if idx.get("schema") != 1:
        sys.exit(f"unknown registry schema {idx.get('schema')}")
    revoked = {(r.get("id"), r.get("sha256") or None) for r in idx.get("revoked", [])}
    groups = {}
    for p in idx["projects"]:
        if p.get("kind") != "native-mod":
            continue
        v = next((v for v in p.get("versions", []) if v.get("game") == "26.3"), None)
        if not v or not str(v.get("api", "")).startswith("native-1/"):
            continue
        parts = v["api"].split("/")
        if len(parts) < 3:
            continue
        horizon, mixins = parts[1], parts[2]
        # These go into build commands and URLs: only full commit ids are accepted.
        if not (SHA.fullmatch(horizon) and SHA.fullmatch(mixins) and ID.fullmatch(p["id"])):
            print(f"warning: {p.get('id')!r} has a malformed pin; skipped", file=sys.stderr)
            continue
        f = v["file"]
        if (p["id"], None) in revoked or (p["id"], f["sha256"]) in revoked:
            continue
        g = groups.setdefault(horizon, {"commit": horizon, "short": horizon[:8], "mixins_revs": set(), "mods": []})
        g["mixins_revs"].add(mixins)
        g["mods"].append({"id": p["id"], "version": v["version"], "name": p.get("name", p["id"]),
                          "url": f["url"], "sha256": f["sha256"].lower(), "size": f["size"], "mixins": mixins})
    if a.combined:
        # Every mod into one game at one commit, with the newest horizon-mixins any mod pins
        # (stage.py rewrites each mod's declared revision; apply.py keeps those that still apply).
        if not SHA.fullmatch(a.combined):
            sys.exit("--combined needs a full 40-character commit")
        mods = [m for g in groups.values() for m in g["mods"]]
        revs = sorted({m["mixins"] for m in mods}, key=lambda r: commit_date(r, token))
        g = {"commit": a.combined, "short": a.combined[:8], "combined": True, "mixins": revs[-1],
             "mods": sorted(mods, key=lambda m: m["id"])}
        with open(a.out, "w") as fh:
            json.dump({"groups": [g]}, fh, indent=1)
        print(f"combined: {len(mods)} native mods at {a.combined[:8]} (horizon-mixins {revs[-1][:8]})", file=sys.stderr)
        if a.matrix:
            print(json.dumps({"commit": [a.combined]}))
        return
    only = [o.strip() for o in a.only.split(",") if o.strip()]
    # Lumos's own mods (lumos-mods/<id>/) pin a commit too: that commit always gets a group, even
    # when no registry mod pins it any more (stage.py adds them; the registry list may be empty).
    own = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "lumos-mods")
    if os.path.isdir(own):
        for i in sorted(os.listdir(own)):
            m = os.path.join(own, i, "horizon-mixin.toml")
            if not (i.startswith("lumos_") and os.path.isfile(m)):
                continue
            text = open(m, encoding="utf-8").read()
            rev = re.search(r'horizon_revision\s*=\s*"([0-9a-f]{40})"', text)
            mix = re.search(r"horizon-mixins ([0-9a-f]{40})", text)
            if rev and rev.group(1) not in groups:
                groups[rev.group(1)] = {"commit": rev.group(1), "short": rev.group(1)[:8], "mixins_revs": set(), "mods": []}
            if rev and mix:
                groups[rev.group(1)]["mixins_revs"].add(mix.group(1))
    out = []
    for g in sorted(groups.values(), key=lambda g: -len(g["mods"])):
        if only and not any(g["commit"].startswith(o) for o in only):
            continue
        revs = sorted(g.pop("mixins_revs"), key=lambda r: commit_date(r, token)) or ["e59da9094f0c2fdda6c627fe9eff22dec6a906f0"]
        g["mixins"] = revs[-1]
        g["mods"].sort(key=lambda m: m["id"])
        out.append(g)
    with open(a.out, "w") as fh:
        json.dump({"groups": out}, fh, indent=1)
    print(f"{len(out)} groups, {sum(len(g['mods']) for g in out)} native mods", file=sys.stderr)
    if a.matrix:
        print(json.dumps({"commit": [g["commit"] for g in out]}))


if __name__ == "__main__":
    main()
