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
import os
import shutil
import subprocess
import sys
import tempfile


def apply(tool, src, mod_ids, mods_root, out):
    with tempfile.TemporaryDirectory() as tmp:
        sel = os.path.join(tmp, "mods")
        os.makedirs(sel)
        for i in mod_ids:
            shutil.copytree(os.path.join(mods_root, i), os.path.join(sel, i))
        shutil.rmtree(out, ignore_errors=True)
        r = subprocess.run([tool, "apply", "--workspace", src, "--mods", sel, "--out", out],
                           capture_output=True, text=True)
        ok = r.returncode == 0
        reason = (r.stderr or r.stdout).strip().splitlines()
        return ok, " ".join(reason[-3:])[:600]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tool", required=True)
    ap.add_argument("--work", required=True)
    a = ap.parse_args()
    src = os.path.join(a.work, "src")
    mods = os.path.join(a.work, "mods")
    stage = os.path.join(a.work, "stage")
    scratch = os.path.join(a.work, "scratch-stage")
    ids = sorted(os.listdir(mods))
    failed, good = {}, []
    for i in ids:
        ok, why = apply(a.tool, src, [i], mods, scratch)
        (good.append(i) if ok else failed.__setitem__(i, why))
        print(f"{'ok  ' if ok else 'FAIL'} {i}" + ("" if ok else f": {why}"), file=sys.stderr)
    shutil.rmtree(scratch, ignore_errors=True)
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
            shutil.rmtree(scratch, ignore_errors=True)
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
