#!/usr/bin/env python3
"""Builds the staged game, leaving out the mods that break the compile.

    python3 tools/lumos_native/build.py --tool <horizon-mixins> --work work/ --package horizon-client --profile fast

Runs from outside work/stage (Windows can't delete a folder a process is inside, and a retry
re-applies into it). When cargo fails, the errors' files are traced back to the mods that patch
them (or bring the failing crate); those are left out, the rest re-applied, and it builds again.
Mods left out this way are added to work/applied.json's "failed" with the compiler's message.
"""
import argparse
import json
import os
import posixpath
import re
import subprocess
import sys

ROUNDS = 8
ANSI = re.compile(r"(?:\x1b|\^\[)\[[0-9;]*m")


def mod_files(mods_root, mod_id):
    """Paths a mod's manifests name (the files it patches, the crates it brings), with / separators."""
    found = set()
    for root, _, files in os.walk(os.path.join(mods_root, mod_id)):
        for f in files:
            if f.endswith(".toml"):
                text = open(os.path.join(root, f), encoding="utf-8", errors="ignore").read()
                found.update(s.replace("\\", "/").strip("/") for s in re.findall(r'"([^"\s]+/[^"\s]+)"', text))
    return found


def culprits(log, stage, mods_root, applied):
    """Which applied mods the compile errors point at, each with the first error naming them."""
    errors = []  # (message, file)
    lines = log.splitlines()
    for n, line in enumerate(lines):
        if line.startswith("error"):
            msg = line
            for follow in lines[n + 1:n + 4]:
                m = re.search(r"--> (\S+?):(\d+):\d+", follow)
                if m:
                    errors.append((msg, m.group(1).replace("\\", "/"), int(m.group(2))))
                    break
            m = re.search(r"failed to run custom build command for `\S+ v\S+ \(([^)]+)\)`", line)
            if m:
                path = m.group(1).replace("\\", "/")
                rel = path.split("/stage/", 1)[-1]
                errors.append((msg, rel, 0))
    files = {i: mod_files(mods_root, i) for i in applied}
    blame = {}
    for msg, path, line in errors:
        path = posixpath.normpath(path.split("/stage/", 1)[-1])  # e.g. crates/a/../b/src/x.rs
        # A mod's own module named in the error (`crate::<mod id>`) is the plainest clue.
        named = [i for i in applied if re.search(rf"\b{re.escape(i)}\b", msg)]
        if named:
            for i in named:
                blame.setdefault(i, f"didn't compile with this Horizon version: {msg[:300]}")
            continue
        touching = [i for i in applied if any(path == f or path.startswith(f + "/") or f.startswith(path) for f in files[i])]
        if path.startswith("native-crates/"):
            top = path.split("/")[1]
            touching += [i for i in applied if i == top or any(f.startswith(f"native-crates/{top}") or f.split("/")[-1] == top for f in files[i])]
        touching = sorted(set(touching))
        if len(touching) > 1 and line:
            # Several mods patch this file: keep the ones whose id shows up near the error.
            try:
                src = open(os.path.join(stage, path), encoding="utf-8", errors="ignore").read().splitlines()
                near = "\n".join(src[max(0, line - 80):line + 20])
                named = [i for i in touching if i in near]
                if named:
                    touching = named
            except OSError:
                pass
        if len(touching) > 6:
            continue  # too many to guess; another error may narrow it
        for i in touching:
            blame.setdefault(i, f"didn't compile with this Horizon version: {msg[:300]}")
    return blame


def main():
    ap = argparse.ArgumentParser()
    for k in ("tool", "work", "package", "profile"):
        ap.add_argument(f"--{k}", required=True)
    a = ap.parse_args()
    work = os.path.abspath(a.work)
    stage = os.path.join(work, "stage")
    mods_root = os.path.join(work, "mods")
    here = os.path.dirname(os.path.abspath(__file__))
    excluded = {}
    tried_without_crates = False
    for rnd in range(1, ROUNDS + 1):
        cmd = ["cargo", "build", "--locked", "--profile", a.profile, "--manifest-path", os.path.join(stage, "Cargo.toml"),
               "-p", a.package, "--bin", "horizon-client", "-p", "horizon-prepare", "--bin", "horizon-prepare"]
        r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                           env=dict(os.environ, CARGO_TERM_COLOR="never"))
        log = ANSI.sub("", r.stdout + r.stderr)
        print(log[-20000:], file=sys.stderr)
        if r.returncode == 0:
            print(f"built in round {rnd}; left out for compile errors: {sorted(excluded) or 'none'}")
            return
        applied = json.load(open(os.path.join(work, "applied.json")))["applied"]
        blame = {i: why for i, why in culprits(log, stage, mods_root, applied).items() if i not in excluded}
        if not blame:
            if tried_without_crates:
                sys.exit("the build fails and the errors don't point at a mod")
            tried_without_crates = True
            print("::warning::errors don't point at a mod; retrying without mods that bring their own crates", file=sys.stderr)
        else:
            print(f"::warning::round {rnd}: leaving out {', '.join(sorted(blame))}", file=sys.stderr)
            excluded.update(blame)
        json.dump(excluded, open(os.path.join(work, "excluded.json"), "w"), indent=1)
        extra = ["--without-crates"] if tried_without_crates else []
        subprocess.run([sys.executable, os.path.join(here, "apply.py"), "--tool", a.tool, "--work", work,
                        "--exclude", ",".join(sorted(excluded))] + extra, check=True)
    sys.exit(f"still failing after {ROUNDS} rounds")


if __name__ == "__main__":
    main()
