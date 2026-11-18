#!/usr/bin/env python3
"""Makes every hook of every staged native mod do nothing while that mod is switched off.

    python3 tools/lumos_native/gate.py --work work/

Lumos's all-in-one game compiles every native mod in, and an instance turns on only the mods the
player installed (`mod.<id>:true` in horizon-mixins.txt, every other built-in one `false`). Horizon
leaves it to each mod's own hooks to ask `native_on`, and many never do. So before the mods are
applied, this rewrites each injection's helper (work/mods/<id>/...) to begin with

    if !::horizon_core::lumos_mod_on!("<id>") { return <what the hook does when it isn't there>; }

(engine-patches/04-lumos-mod-switches.patch adds the switch; horizon-mixins puts every helper's
body at its call site, so the check runs at every injection site). What a missing hook does
depends on its kind:

  HEAD, BEFORE_CALL, AFTER_CALL, AFTER_ASSIGN     nothing (`()`, or `Continue(())` when cancellable)
  RETURN, MODIFY_ARG, MODIFY_CONSTANT,
  MODIFY_VARIABLE, MODIFY_EXPRESSION_VALUE        hands back the value it was given (`$value`)
  MODIFY_ARGS                                     hands back the arguments (`$args`)
  WRAP_OPERATION                                  calls the wrapped operation with the arguments
  OVERWRITE                                       becomes a cancellable HEAD that runs the mod's body
                                                  (Break) when on and the original body
                                                  (Continue) when off; only when the target's
                                                  result type can be spelled in the helper
                                                  (no `Self`), else left as it is and reported

The hooks a mod lists itself with (`native_mods`, `native_settings`, `native_settings_click`)
check `lumos_mod_shown!` instead: a mod the player turns off in the game stays listed (and can be
turned on again); a mod the instance doesn't have (off when the game started) is listed nowhere.

Writes work/gates.json: {mod id: {"gated": n, "ungated": [why, ...]}}. package.py reads it to say
which mods can be switched off.
"""
import argparse
import json
import os
import re
import sys
import tomllib

LISTING = {"native_mods", "native_settings", "native_settings_click"}
NOTHING = {"HEAD", "BEFORE_CALL", "AFTER_CALL", "AFTER_ASSIGN"}
VALUE = {"RETURN", "MODIFY_ARG", "MODIFY_CONSTANT", "MODIFY_VARIABLE", "MODIFY_EXPRESSION_VALUE"}


def skip_trivia(s, i):
    """Index of the next character that isn't whitespace or a comment."""
    n = len(s)
    while i < n:
        if s[i].isspace():
            i += 1
        elif s.startswith("//", i):
            j = s.find("\n", i)
            i = n if j < 0 else j + 1
        elif s.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if s.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif s.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        else:
            break
    return i


def tokens(s):
    """(index, char) of each code character outside comments, strings and char literals, with
    literals reported as one 'L' token at their start."""
    i, n = 0, len(s)
    while i < n:
        j = skip_trivia(s, i)
        if j != i:
            i = j
            continue
        c = s[i]
        m = re.match(r'(b?r)(#*)"', s[i:])
        if m:
            end = s.find('"' + m.group(2), i + len(m.group(0)))
            yield i, "L"
            i = (n if end < 0 else end) + 1 + len(m.group(2))
            continue
        if c == '"' or (c == "b" and s.startswith('b"', i)):
            i += 1 if c == '"' else 2
            while i < n and s[i] != '"':
                i += 2 if s[i] == "\\" else 1
            yield i, "L"
            i += 1
            continue
        if c == "'":
            # A char literal ('a', '\n', '\u{..}') or a lifetime ('a).
            if i + 2 < n and s[i + 1] == "\\":
                end = s.find("'", i + 2)
                yield i, "L"
                i = end + 1
                continue
            if i + 2 < n and s[i + 2] == "'":
                yield i, "L"
                i += 3
                continue
            yield i, c
            i += 1
            continue
        yield i, c
        i += 1


class Helper:
    """A helper file: one `fn name(params) -> R { body }`."""

    def __init__(self, text):
        self.text = text
        toks = list(tokens(text))
        idx = [p for p, _ in toks]
        chars = [c for _, c in toks]
        if "fn" not in "".join(chars):
            raise ValueError("no function")
        # Walk the tokens to find the name, the parameter list's parentheses and the body's braces.
        k = 0
        while not (chars[k] == "f" and "".join(chars[k:k + 2]) == "fn" and (k == 0 or not (chars[k - 1].isalnum() or chars[k - 1] == "_"))):
            k += 1
        name_start = k + 2
        while chars[name_start].isspace():
            name_start += 1
        k = name_start
        while chars[k].isalnum() or chars[k] == "_":
            k += 1
        self.name = "".join(chars[name_start:k])
        while chars[k] != "(":
            k += 1
        self.params_open = idx[k]
        depth = 0
        while True:
            if chars[k] in "([":
                depth += 1
            elif chars[k] in ")]":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        self.params_close = idx[k]
        k += 1
        depth = 0
        while not (chars[k] == "{" and depth == 0):
            if chars[k] in "([":
                depth += 1
            elif chars[k] in ")]":
                depth -= 1
            k += 1
        self.body_open = idx[k]
        last = max(j for j, c in enumerate(chars) if c == "}")
        self.body_close = idx[last]
        between = text[self.params_close + 1:self.body_open].strip()
        self.result = between[2:].strip() if between.startswith("->") else "()"
        self.params_text = text[self.params_open + 1:self.params_close]

    def param_names(self):
        names = []
        # Split at top-level commas (types may hold commas inside <> () []).
        parts, depth, start = [], 0, 0
        t = self.params_text
        for p, c in tokens(t):
            if c in "<([":
                depth += 1
            elif c in ">)]":
                # `->` inside `impl FnOnce(A) -> B` is not a closing angle bracket.
                if not (c == ">" and p > 0 and t[p - 1] == "-"):
                    depth -= 1
            elif c == "," and depth == 0:
                parts.append(t[start:p])
                start = p + 1
        parts.append(t[start:])
        for part in parts:
            part = part.strip()
            if not part:
                continue
            pat = part.split(":", 1)[0].strip()
            pat = re.sub(r"^mut\s+", "", pat)
            names.append(pat)
        return names

    def with_gate(self, statement):
        """The helper with `statement` first in its body."""
        at = self.body_open + 1
        # Inner attributes (`#![...]`) must stay first.
        rest = self.text[at:]
        m = re.match(r"(\s*#!\[[^\]]*\]\s*)*", rest)
        at += m.end() if m else 0
        return self.text[:at] + "\n    " + statement + "\n" + self.text[at:]


def target_result(src, target, name):
    """The result types of every `fn name` the target path could mean, from the staged source."""
    return {s[3] for s in target_sigs(src, target, name)}


def target_sigs(src, target, name):
    """Every `fn name` the target path could mean: (file, start, end of its result type's text,
    that text without spaces, or "()")."""
    segs = [s for s in target.split("::")[1:-1] if s and s[0].islower() and "<" not in s]
    ends = ("/" + "/".join(segs) + ".rs", "/" + "/".join(segs) + "/mod.rs") if segs else None
    found = []
    for top in ("crates",):
        for root, _, files in os.walk(os.path.join(src, top)):
            if "/target" in root or "/tests" in root:
                continue
            for f in files:
                if not f.endswith(".rs"):
                    continue
                path = os.path.join(root, f)
                rel = os.path.relpath(path, src).replace(os.sep, "/")
                if ends and not rel.endswith(ends):
                    continue
                text = open(path, encoding="utf-8", errors="ignore").read()
                for m in re.finditer(r"\bfn\s+" + re.escape(name) + r"\s*(<[^>{]*>)?\s*\(", text):
                    # The signature: up to the body's `{` (or `;`).
                    depth, k = 0, m.end() - 1
                    while k < len(text):
                        c = text[k]
                        if c in "([":
                            depth += 1
                        elif c in ")]":
                            depth -= 1
                        elif c in "{;" and depth == 0:
                            break
                        k += 1
                    sig = text[m.end() - 1:k]
                    close = 0
                    depth = 0
                    for j, c in enumerate(sig):
                        if c == "(":
                            depth += 1
                        elif c == ")":
                            depth -= 1
                            if depth == 0:
                                close = j
                                break
                    rest = sig[close + 1:]
                    after = re.sub(r"\bwhere\b.*", "", rest, flags=re.S)
                    arrow = after.find("->")
                    start = m.end() - 1 + close + 1 + (arrow + 2 if arrow >= 0 else 0)
                    end = m.end() - 1 + close + 1 + len(after.rstrip())
                    norm = re.sub(r"\s+", "", after.strip()[2:]) if arrow >= 0 else "()"
                    found.append((path, start, end, norm))
    return found


def gate_mod(folder, src, catalog):
    manifest = os.path.join(folder, "horizon-mixin.toml")
    text = open(manifest, encoding="utf-8").read()
    data = tomllib.loads(text)
    mod_id = data["mod"]["id"]
    injections = data.get("injection", [])
    # The [[injection]] tables' text, in the same order tomllib reads them.
    heads = [m.start() for m in re.finditer(r"(?m)^[ \t]*\[\[[ \t]*injection[ \t]*\]\][ \t]*$", text)]
    if len(heads) != len(injections):
        return {"gated": 0, "ungated": [f"manifest layout not understood ({len(heads)} tables, {len(injections)} injections)"]}
    bounds = []
    for n, h in enumerate(heads):
        nxt = re.compile(r"(?m)^[ \t]*\[").search(text, h + 1)
        bounds.append((h, nxt.start() if nxt else len(text)))
    out, last = [], 0
    report = {"gated": 0, "ungated": []}
    for n, (inj, (a, b)) in enumerate(zip(injections, bounds)):
        block = text[a:b]
        out.append(text[last:a])
        last = b
        kind = inj["kind"]
        target = inj.get("target") or catalog.get(inj.get("hook", ""), "")
        fn_name = target.split("::")[-1] if target else ""
        crate = target.split("::")[0] if target else "horizon_client"
        root = "crate" if crate == "horizon_core" else "::horizon_core"
        macro = "lumos_mod_shown" if fn_name in LISTING else "lumos_mod_on"
        check = f'!{root}::{macro}!("{mod_id}")'
        helper_rel = inj["helper"]
        try:
            helper = Helper(open(os.path.join(folder, helper_rel), encoding="utf-8").read())
        except (OSError, ValueError, IndexError, StopIteration) as e:
            report["ungated"].append(f"{inj['id']}: helper not read ({e})")
            out.append(block)
            continue
        bindings = inj.get("bindings", [])
        new_block = block
        if kind in NOTHING:
            ret = "::core::ops::ControlFlow::Continue(())" if inj.get("cancellable") else ""
            source = helper.with_gate(f"if {check} {{ return {ret}; }}".replace("return ;", "return;"))
        elif kind in VALUE or kind == "MODIFY_ARGS":
            special = "$args" if kind == "MODIFY_ARGS" else "$value"
            names = [x["name"] for x in bindings if x.get("value") == special]
            if len(names) != 1:
                report["ungated"].append(f"{inj['id']}: {kind} without one {special} binding")
                out.append(block)
                continue
            source = helper.with_gate(f"if {check} {{ return {names[0]}; }}")
        elif kind == "WRAP_OPERATION":
            op = [x["name"] for x in bindings if x.get("value") == "$operation"]
            recv = [x["name"] for x in bindings if x.get("value") == "$receiver"]
            args = sorted(((int(x["value"][4:]), x["name"]) for x in bindings if re.fullmatch(r"\$arg\d+", x.get("value", ""))))
            if len(op) != 1:
                report["ungated"].append(f"{inj['id']}: WRAP_OPERATION without $operation")
                out.append(block)
                continue
            call = ", ".join(recv + [nm for _, nm in args])
            source = helper.with_gate(f"if {check} {{ return {op[0]}({call}); }}")
        elif kind == "OVERWRITE":
            sigs = target_sigs(src, target, fn_name) if target else []
            results = {s[3] for s in sigs}
            mine = re.sub(r"\s+", "", helper.result)
            owner = target.split("::")[-2] if target.count("::") >= 2 else ""
            if len(sigs) > 1 and re.fullmatch(r"[A-Z][A-Za-z0-9_]*(<'_>)?", owner):
                # Several `fn name` in the file: keep the one in `impl <owner>`.
                bare = owner.split("<")[0]

                def in_owner(s):
                    code = open(s[0], encoding="utf-8").read()[:s[1]]
                    impls = list(re.finditer(r"(?m)^\s*impl\b[^{]*\{", code))
                    return bool(impls) and re.search(r"(\bfor\s+|^\s*impl(<[^>]*>)?\s+)" + bare + r"\b", impls[-1].group(0).strip()) is not None
                sigs = [s for s in sigs if in_owner(s)]
                results = {s[3] for s in sigs}
            if (len(sigs) == 1 and results != {mine} and re.fullmatch(r"[A-Z][A-Za-z0-9_]*", owner)
                    and re.sub(r"\bSelf\b", owner, sigs[0][3]) == mine):
                # The target's result says `Self`, which a hook can't: spell its type out there
                # (the same type, so the engine is unchanged), as the mod's own helper does.
                path, start, end, _ = sigs[0]
                code = open(path, encoding="utf-8").read()
                code = code[:start] + re.sub(r"\bSelf\b", owner, code[start:end]) + code[end:]
                open(path, "w", encoding="utf-8").write(code)
                results = {mine}
            if "Self" in mine or results != {mine}:
                report["ungated"].append(
                    f"{inj['id']}: OVERWRITE of {target} (its result type {sorted(results) or '?'} can't be named in a hook)")
                out.append(block)
                continue
            if "cancellable" in block:
                report["ungated"].append(f"{inj['id']}: OVERWRITE that names cancellable")
                out.append(block)
                continue
            names = helper.param_names()
            t = helper.text
            if not re.search(r"\bfn\s+" + re.escape(helper.name) + r"\s*$", t[:helper.params_open]):
                report["ungated"].append(f"{inj['id']}: OVERWRITE helper not understood")
                out.append(block)
                continue
            inner_fn = "fn lumos_overwritten" + t[helper.params_open:helper.body_close + 1]
            source = (
                f"fn {helper.name}({helper.params_text}) -> ::core::ops::ControlFlow<{helper.result}> {{\n"
                f"    if {check} {{ return ::core::ops::ControlFlow::Continue(()); }}\n"
                f"    {inner_fn}\n"
                f"    ::core::ops::ControlFlow::Break(lumos_overwritten({', '.join(names)}))\n"
                f"}}\n")
            new_block = re.sub(r'(?m)^(\s*kind\s*=\s*)"OVERWRITE"', r'\1"HEAD"\ncancellable = true', new_block, count=1)
            if new_block == block:
                report["ungated"].append(f"{inj['id']}: OVERWRITE kind line not found")
                out.append(block)
                continue
        else:
            report["ungated"].append(f"{inj['id']}: kind {kind} not handled")
            out.append(block)
            continue
        base, ext = os.path.splitext(helper_rel)
        gated_rel = f"{base}.lumos-gate-{n}{ext}"
        with open(os.path.join(folder, gated_rel), "w", encoding="utf-8") as fh:
            fh.write(source)
        new_block, count = re.subn(r'(helper\s*=\s*")' + re.escape(helper_rel) + '"', lambda m: m.group(1) + gated_rel + '"', new_block, count=1)
        if count != 1:
            report["ungated"].append(f"{inj['id']}: helper line not found")
            out.append(block)
            continue
        out.append(new_block)
        report["gated"] += 1
    out.append(text[last:])
    with open(manifest, "w", encoding="utf-8") as fh:
        fh.write("".join(out))
    return report


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", required=True)
    a = ap.parse_args()
    src = os.path.join(a.work, "src")
    mods = os.path.join(a.work, "mods")
    catalog = {}
    hooks = os.path.join(src, "native-hooks.json")
    if os.path.exists(hooks):
        d = json.load(open(hooks, encoding="utf-8"))
        catalog = {h["name"]: h["target"] for part in ("hooks", "helpers") for h in d.get(part, []) if "target" in h}
    reports = {}
    for i in sorted(os.listdir(mods)):
        folder = os.path.join(mods, i)
        if not os.path.isfile(os.path.join(folder, "horizon-mixin.toml")):
            continue
        try:
            reports[i] = gate_mod(folder, src, catalog)
        except Exception as e:  # a manifest this doesn't understand: the mod stays as it was
            reports[i] = {"gated": 0, "ungated": [f"not gated: {e}"]}
        if reports[i]["ungated"]:
            print(f"gate: {i}: {len(reports[i]['ungated'])} hook(s) not switchable: {'; '.join(reports[i]['ungated'])}", file=sys.stderr)
    with open(os.path.join(a.work, "gates.json"), "w") as fh:
        json.dump(reports, fh, indent=1)
    total = sum(r["gated"] for r in reports.values())
    print(f"gate: {total} hooks of {len(reports)} mods switch with their mod", file=sys.stderr)


if __name__ == "__main__":
    main()
