#!/usr/bin/env python3
"""Downloads the Minecraft version a Horizon commit pins, from Mojang, into a folder laid out
like the official launcher's (versions/, libraries/, assets/), plus its server jar, so
horizon-prepare can import it and compile that commit's exporters in CI. Nothing here is
published: only Horizon's own compiled exporter classes leave the job.

    python3 tools/lumos_native/fetch_mc.py --lock src/research-lock.json --out mc/
"""
import argparse
import concurrent.futures as cf
import hashlib
import json
import os
import sys
import urllib.request

UA = {"User-Agent": "LumosClient-native-builds"}
MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"


def get(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=300) as r:
        return r.read()


def save(url, dest, sha1=None):
    if os.path.isfile(dest) and (not sha1 or hashlib.sha1(open(dest, "rb").read()).hexdigest() == sha1):
        return
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    for attempt in range(5):
        try:
            data = get(url)
            if sha1 and hashlib.sha1(data).hexdigest() != sha1:
                raise ValueError("sha1 mismatch")
            with open(dest + ".part", "wb") as fh:
                fh.write(data)
            os.replace(dest + ".part", dest)
            return
        except Exception as e:
            if attempt == 4:
                raise SystemExit(f"{url}: {e}")


def lock_version(path):
    """The pinned version and the exact version-JSON hash this commit accepts (Mojang has
    republished 26.3's JSON; an older commit only knows the one it pinned)."""
    mc = json.load(open(path)).get("minecraft") or {}
    if not mc.get("version"):
        sys.exit("research-lock.json names no Minecraft version")
    return mc["version"], mc.get("manifest_sha1")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--lock", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    version, pinned_sha1 = lock_version(a.lock)
    vj_bytes = None
    if pinned_sha1:
        try:
            vj_bytes = get(f"https://piston-meta.mojang.com/v1/packages/{pinned_sha1}/{version}.json")
            if hashlib.sha1(vj_bytes).hexdigest() != pinned_sha1:
                vj_bytes = None
        except Exception:
            vj_bytes = None
    if vj_bytes is None:
        m = json.loads(get(MANIFEST))
        entry = next(v for v in m["versions"] if v["id"] == version)
        vj_bytes = get(entry["url"])
    vj = json.loads(vj_bytes)
    vdir = os.path.join(a.out, "versions", version)
    os.makedirs(vdir, exist_ok=True)
    open(os.path.join(vdir, f"{version}.json"), "wb").write(vj_bytes)
    jobs = [(vj["downloads"]["client"]["url"], os.path.join(vdir, f"{version}.jar"), vj["downloads"]["client"]["sha1"]),
            (vj["downloads"]["server"]["url"], os.path.join(a.out, "server.jar"), vj["downloads"]["server"]["sha1"])]
    for lib in vj.get("libraries", []):
        art = lib.get("downloads", {}).get("artifact")
        if art and art.get("path"):
            jobs.append((art["url"], os.path.join(a.out, "libraries", art["path"]), art.get("sha1")))
    ai = vj["assetIndex"]
    idx_bytes = get(ai["url"])
    os.makedirs(os.path.join(a.out, "assets", "indexes"), exist_ok=True)
    open(os.path.join(a.out, "assets", "indexes", f"{ai['id']}.json"), "wb").write(idx_bytes)
    for obj in json.loads(idx_bytes)["objects"].values():
        h = obj["hash"]
        jobs.append((f"https://resources.download.minecraft.net/{h[:2]}/{h}", os.path.join(a.out, "assets", "objects", h[:2], h), h))
    with cf.ThreadPoolExecutor(16) as ex:
        list(ex.map(lambda j: save(*j), jobs))
    print(version)


if __name__ == "__main__":
    main()
