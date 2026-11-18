#!/usr/bin/env python3
"""Applies a group's native mods to the staged source with horizon-mixins.

    python3 tools/lumos_native/apply.py --tool <horizon-mixins> --work work/

Each mod is tried on its own first; one that doesn't apply is left out with its reason
instead of failing the whole group. The rest are applied together into work/stage (adding
them one at a time if together they conflict). Writes work/applied.json:
{"applied": [ids], "failed": {id: reason}}.
"""
import argparse
import json
import re
import os
import shutil
import subprocess
import sys
import tempfile


def rmtree(path):
    """Removes a folder for real (read-only files too, as Windows checkouts have) or says why not."""
    def force(fn, p, _):
        os.chmod(p, 0o700)
        fn(p)
    if os.path.exists(path):
        shutil.rmtree(path, onerror=force)
    if os.path.exists(path):
        sys.exit(f"couldn't remove {path} (is something still using it?)")


def fingerprint(src, staged):
    """The lines a mod adds: in the files it changed (their size differs from the source's), the
    lines the source doesn't have. Lets a compile error be traced to the mod whose code it is."""
    added = {}
    for top in ("crates", "native-crates"):
        for root, _, files in os.walk(os.path.join(staged, top)):
            for f in files:
                if not f.endswith(".rs"):
                    continue
                new = os.path.join(root, f)
                rel = os.path.relpath(new, staged).replace(os.sep, "/")
                old = os.path.join(src, rel)
                if os.path.exists(old) and os.path.getsize(old) == os.path.getsize(new):
                    continue
                theirs = set()
                if os.path.exists(old):
                    theirs = {l.strip() for l in open(old, encoding="utf-8", errors="ignore")}
                mine = {l.strip() for l in open(new, encoding="utf-8", errors="ignore")} - theirs
                mine = sorted(l for l in mine if len(l) >= 12)
                if mine:
                    added[rel] = mine
    return added


def apply(tool, src, mod_ids, mods_root, out):
    with tempfile.TemporaryDirectory() as tmp:
        sel = os.path.join(tmp, "mods")
        os.makedirs(sel)
        for i in mod_ids:
            shutil.copytree(os.path.join(mods_root, i), os.path.join(sel, i))
        rmtree(out)
        r = subprocess.run([tool, "apply", "--workspace", src, "--mods", sel, "--out", out],
                           capture_output=True, text=True)
        ok = r.returncode == 0
        apply.last_output = (r.stderr or "") + (r.stdout or "")
        reason = (r.stderr or r.stdout).strip().splitlines()
        return ok, " ".join(reason[-3:])[:600]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tool", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--exclude", default="", help="comma-separated mod ids to leave out (the reason file says why)")
    ap.add_argument("--fingerprint-only", action="store_true", help="record each mod's added lines for build.py, apply nothing")
    ap.add_argument("--without-crates", action="store_true",
                    help="leave out mods that bring their own Rust crates ([[crate]]): used when the full build failed")
    a = ap.parse_args()
    src = os.path.join(a.work, "src")
    mods = os.path.join(a.work, "mods")
    stage = os.path.join(a.work, "stage")
    scratch = os.path.join(a.work, "scratch-stage")
    ids = sorted(os.listdir(mods))
    failed, good = {}, []
    reasons = {}
    rf = os.path.join(a.work, "excluded.json")
    if os.path.exists(rf):
        reasons = json.load(open(rf))
    for i in filter(None, a.exclude.split(",")):
        if i in ids:
            ids.remove(i)
            failed[i] = reasons.get(i, "didn't compile with this Horizon version")
    if a.without_crates:
        for i in list(ids):
            tomls = [os.path.join(r, f) for r, _, fs in os.walk(os.path.join(mods, i)) for f in fs if f == "horizon-mixin.toml"]
            if any("[[crate]]" in open(t, encoding="utf-8", errors="ignore").read() for t in tomls):
                ids.remove(i)
                failed[i] = "its own Rust crate didn't compile with this Horizon version"
                print(f"skip {i}: brings its own crate", file=sys.stderr)
    fp_path = os.path.join(a.work, "fingerprints.json")
    prints = json.load(open(fp_path)) if os.path.exists(fp_path) else {}
    if a.fingerprint_only:
        # Only record what each mod adds (build.py traces compile errors with it); applies nothing.
        for i in ids:
            if i not in prints:
                ok, _ = apply(a.tool, src, [i], mods, scratch)
                if ok:
                    prints[i] = fingerprint(src, scratch)
        rmtree(scratch)
        with open(fp_path, "w") as fh:
            json.dump(prints, fh)
        print(f"fingerprinted {len(prints)} mods", file=sys.stderr)
        return
    # Fast path: all of them at once. Trying each mod on its own first costs one full apply per
    # mod (over an hour for ~150 mods), so that only happens when applying them together fails.
    together = list(ids)
    for _ in range(80):
        if not together:
            break
        ok, why = apply(a.tool, src, together, mods, stage)
        if ok:
            with open(os.path.join(a.work, "applied.json"), "w") as fh:
                json.dump({"applied": together, "failed": failed}, fh, indent=1)
            print(f"applied {len(together)}/{len(ids)} together", file=sys.stderr)
            return
        # horizon-mixins names the mod an error belongs to: drop it and try the rest together again.
        named = [m for m in re.findall(r'"mod_id"\s*:\s*"([a-z0-9_]+)"', apply.last_output) if m in together]
        if not named:
            for m in re.findall(r"\bmods[/\\]([a-z0-9_]+)[/\\]", apply.last_output):
                if m in together:
                    named = [m]
                    break
        if not named:
            print(f"applying together failed and no mod is named ({why}); trying each on its own", file=sys.stderr)
            break
        bad = named[0]
        together.remove(bad)
        failed[bad] = why
        print(f"FAIL {bad}: {why}", file=sys.stderr)
    for i in ids:
        ok, why = apply(a.tool, src, [i], mods, scratch)
        (good.append(i) if ok else failed.__setitem__(i, why))
        if ok and i not in prints:
            prints[i] = fingerprint(src, scratch)
        print(f"{'ok  ' if ok else 'FAIL'} {i}" + ("" if ok else f": {why}"), file=sys.stderr)
    with open(fp_path, "w") as fh:
        json.dump(prints, fh)
    rmtree(scratch)
    applied = []
    if good:
        ok, why = apply(a.tool, src, good, mods, stage)
        if ok:
            applied = good
        else:
            print(f"together they conflict ({why}); adding one at a time", file=sys.stderr)
            for i in good:
                ok, why = apply(a.tool, src, applied + [i], mods, scratch)
                if ok:
                    applied.append(i)
                else:
                    failed[i] = f"conflicts with the others: {why}"
            rmtree(scratch)
            if applied:
                ok, why = apply(a.tool, src, applied, mods, stage)
                if not ok:
                    sys.exit(f"final apply failed: {why}")
    if not applied:
        sys.exit("no mod in this group applies")
    with open(os.path.join(a.work, "applied.json"), "w") as fh:
        json.dump({"applied": applied, "failed": failed}, fh, indent=1)
    print(f"applied {len(applied)}/{len(ids)}", file=sys.stderr)


if __name__ == "__main__":
    main()
