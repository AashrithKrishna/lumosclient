#!/usr/bin/env python3
"""Stages one native-mod group: Horizon's source at the pinned commit and the group's mods.

    python3 tools/lumos_native/stage.py --plan plan.json --commit <sha> --work work/

Makes work/src (the commit's source, symlinks removed, `.horizon-revision` written, as
horizon-mixins expects, and Lumos's engine patches applied: engine-patches/README.md) and
work/mods/<id>/ (each mod's mixins folder, downloaded from the
registry and checked against its SHA-256 and size). With the engine patches applied, gate.py then
starts every hook of every mod with its mod's switch (work/gates.json). Prints the group's
horizon-mixins rev.
"""
import argparse
import hashlib
import io
import json
import os
import shutil
import sys
import tarfile
import urllib.request
import zipfile

UA = {"User-Agent": "LumosClient-native-builds"}


def fetch(url, limit):
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=300) as r:
        data = r.read(limit + 1)
    if len(data) > limit:
        raise SystemExit(f"{url} is larger than expected")
    return data


def apply_engine_patches(src, commit):
    """Lumos's own engine changes (engine-patches/*.patch, plain git diffs against the commit named
    in engine-patches/base-commit.txt), applied in name order before any mod. A patch that doesn't
    apply stops the build: the game must never ship without one silently."""
    here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "engine-patches")
    base = os.path.join(here, "base-commit.txt")
    if not os.path.isfile(base):
        return False
    pinned = open(base, encoding="utf-8").read().split()[0]
    if pinned != commit:
        print(f"engine patches are for {pinned[:8]}, not {commit[:8]}: none applied", file=sys.stderr)
        return False
    import subprocess
    for name in sorted(f for f in os.listdir(here) if f.endswith(".patch")):
        patch = os.path.abspath(os.path.join(here, name))
        # Windows checkouts can turn the patch into CRLF lines; the source is LF, so normalise a copy.
        data = open(patch, "rb").read()
        if b"\r\n" in data:
            patch = os.path.join(src, ".." , f".lf-{name}")
            open(patch, "wb").write(data.replace(b"\r\n", b"\n"))
            patch = os.path.abspath(patch)
        # The source sits inside the Lumos checkout in CI: keep git from treating it as part of
        # that repository (git apply would then resolve paths from the checkout's root).
        env = dict(os.environ, GIT_CEILING_DIRECTORIES=os.path.dirname(os.path.abspath(src)))
        r = subprocess.run(["git", "apply", "--whitespace=nowarn", patch], cwd=src, env=env,
                           capture_output=True, text=True)
        if r.returncode != 0:
            sys.exit(f"engine patch {name} doesn't apply to {commit[:8]}: {(r.stderr or r.stdout).strip()}")
        print(f"engine patch: {name}", file=sys.stderr)
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--plan", required=True)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--work", required=True)
    a = ap.parse_args()
    plan = json.load(open(a.plan))
    g = next((g for g in plan["groups"] if g["commit"] == a.commit), None)
    if not g:
        sys.exit(f"no group for {a.commit}")
    src = os.path.join(a.work, "src")
    mods = os.path.join(a.work, "mods")
    shutil.rmtree(src, ignore_errors=True)
    shutil.rmtree(mods, ignore_errors=True)
    os.makedirs(src)
    os.makedirs(mods)

    # Source: GitHub's archive of the commit (what Horizon's launcher fetches too).
    data = fetch(f"https://codeload.github.com/horizon-rust/horizon/tar.gz/{g['commit']}", 2 << 30)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        top = None
        for m in tar.getmembers():
            parts = m.name.split("/", 1)
            top = top or parts[0]
            if len(parts) < 2 or not parts[1] or m.issym() or m.islnk():
                continue  # horizon-mixins refuses symlinks
            rel = os.path.normpath(parts[1])
            if rel.startswith("..") or os.path.isabs(rel):
                sys.exit(f"unsafe path in archive: {m.name}")
            dest = os.path.join(src, rel)
            if m.isdir():
                os.makedirs(dest, exist_ok=True)
            elif m.isfile():
                os.makedirs(os.path.dirname(dest), exist_ok=True)
                with tar.extractfile(m) as fh, open(dest, "wb") as out:
                    shutil.copyfileobj(fh, out)
                if m.mode & 0o111:
                    os.chmod(dest, 0o755)
    with open(os.path.join(src, ".horizon-revision"), "w") as fh:
        fh.write(g["commit"] + "\n")
    patched = apply_engine_patches(src, g["commit"])

    for m in g["mods"]:
        blob = fetch(m["url"], int(m["size"]))
        if len(blob) != int(m["size"]) or hashlib.sha256(blob).hexdigest() != m["sha256"]:
            sys.exit(f"{m['id']}: download doesn't match the registry")
        z = zipfile.ZipFile(io.BytesIO(blob))
        prefix = f"mixins/{m['id']}/"
        names = [n for n in z.namelist() if n.startswith(prefix) and not n.endswith("/")]
        if not names:
            print(f"warning: {m['id']} has no {prefix} folder; skipped", file=sys.stderr)
            continue
        for n in names:
            rel = os.path.normpath(n[len(prefix):])
            if rel.startswith("..") or os.path.isabs(rel):
                sys.exit(f"unsafe path in {m['id']}: {n}")
            dest = os.path.join(mods, m["id"], rel)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, "wb") as out:
                out.write(z.read(n))
    # Ports: registry mods patched by Lumos to build against the combined game's Horizon commit
    # (native-ports/<id>/ is a full mixins/<id>/ folder, with the mod's licence kept). They replace
    # the registry copy in the combined game only.
    ports = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "native-ports")
    if g.get("combined") and os.path.isdir(ports):
        for i in sorted(os.listdir(ports)):
            manifest = os.path.join(ports, i, "horizon-mixin.toml")
            if os.path.isfile(manifest) and g["commit"] in open(manifest, encoding="utf-8").read():
                shutil.rmtree(os.path.join(mods, i), ignore_errors=True)
                shutil.copytree(os.path.join(ports, i), os.path.join(mods, i))
                print(f"port: {i}", file=sys.stderr)
    # Lumos's own native mods (lumos-mods/<id>/ is already a mixins/<id>/ folder) ride in the
    # combined game, and in the group game of the commit they were written for.
    own = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "lumos-mods")
    if os.path.isdir(own):
        for i in sorted(os.listdir(own)):
            manifest = os.path.join(own, i, "horizon-mixin.toml")
            if i.startswith("lumos_") and os.path.isfile(manifest):
                if g.get("combined") or g["commit"] in open(manifest, encoding="utf-8").read():
                    shutil.copytree(os.path.join(own, i), os.path.join(mods, i), dirs_exist_ok=True)
    if g.get("combined"):
        # Retarget: each mod declares the commit it was made for; the combined game is one commit.
        import re
        for root, _, files in os.walk(mods):
            for f in files:
                if f == "horizon-mixin.toml":
                    path = os.path.join(root, f)
                    text = open(path, encoding="utf-8").read()
                    text = re.sub(r'(horizon_revision\s*=\s*")[0-9a-f]{40}(")', lambda m: m.group(1) + g["commit"] + m.group(2), text)
                    open(path, "w", encoding="utf-8").write(text)
    if patched:
        # Every hook of every mod checks its mod's switch first (gate.py; the switch is engine patch 04).
        import subprocess
        subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "gate.py"),
                        "--work", a.work], check=True)
    print(g["mixins"])


if __name__ == "__main__":
    main()
