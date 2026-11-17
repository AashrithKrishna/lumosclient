import { test, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { route, elyHasJoined, resetLimits, ELY_HAS_JOINED } from "../src/index.js";

class FakeKV {
  constructor() {
    this.map = new Map();
    this.ttl = new Map();
  }
  async get(k, type) {
    const v = this.map.get(k);
    if (v === undefined) return null;
    return type === "json" ? JSON.parse(v) : v;
  }
  async put(k, v, opts) {
    this.map.set(k, v);
    if (opts && opts.expirationTtl) this.ttl.set(k, opts.expirationTtl);
  }
  async delete(k) {
    this.map.delete(k);
  }
}

const BASE = "https://capes.example.workers.dev";
const req = (path, init) => new Request(BASE + path, init);
const post = (body, ip = "1.2.3.4") =>
  req("/v1/select", { method: "POST", body: JSON.stringify(body), headers: { "cf-connecting-ip": ip } });

// Ely.by stub: "Steve" joined whatever serverId was registered in `joined`.
function elyStub(joined) {
  return async (url) => {
    const u = new URL(url);
    assert.equal(u.origin + u.pathname, ELY_HAS_JOINED);
    const name = u.searchParams.get("username");
    const sid = u.searchParams.get("serverId");
    if (joined.get(sid)?.toLowerCase() === name.toLowerCase()) {
      return new Response(JSON.stringify({ id: "a".repeat(32), name: joined.get(sid), properties: [] }), { status: 200 });
    }
    return new Response(null, { status: 204 });
  };
}

let env;
beforeEach(() => {
  env = { CAPES: new FakeKV() };
  resetLimits();
});

test("catalog lists every cape with a URL", async () => {
  const r = await route(req("/v1/capes"), env);
  const list = await r.json();
  assert.ok(list.length >= 10);
  assert.ok(list.every((c) => c.id && c.name && c.url === `${BASE}/capes/${c.id}.png`));
});

test("unknown player has no cape", async () => {
  const r = await route(req("/v1/player/Nobody"), env);
  assert.deepEqual(await r.json(), { cape: null, url: null });
  assert.match(r.headers.get("cache-control"), /max-age/);
});

test("bad names are rejected", async () => {
  assert.equal((await route(req("/v1/player/a%20b"), env)).status, 400);
  assert.equal((await route(req("/v1/player/%E0%A4%A"), env)).status, 400);
});

test("full flow: challenge, join, select, lookup (case-insensitive)", async () => {
  const { serverId } = await (await route(req("/v1/challenge"), env)).json();
  assert.match(serverId, /^[0-9a-f]{32}$/);
  assert.equal(env.CAPES.ttl.get(`ch:${serverId}`), 300);
  const joined = new Map([[serverId, "Steve"]]);
  const r = await route(post({ username: "steve", serverId, cape: "aurora" }), env, elyStub(joined));
  assert.equal(r.status, 200);
  assert.deepEqual(await r.json(), { ok: true, name: "Steve", cape: "aurora" });
  const p = await (await route(req("/v1/player/STEVE"), env)).json();
  assert.deepEqual(p, { cape: "aurora", url: `${BASE}/capes/aurora.png` });
  // The challenge can't be used twice.
  const again = await route(post({ username: "Steve", serverId, cape: "galaxy" }), env, elyStub(joined));
  assert.equal(again.status, 403);
});

test("removing the cape", async () => {
  await env.CAPES.put("p:steve", JSON.stringify({ cape: "ocean" }));
  const { serverId } = await (await route(req("/v1/challenge"), env)).json();
  const r = await route(post({ username: "Steve", serverId, cape: null }), env, elyStub(new Map([[serverId, "Steve"]])));
  assert.equal(r.status, 200);
  assert.deepEqual(await (await route(req("/v1/player/steve"), env)).json(), { cape: null, url: null });
});

test("someone else can't set your cape", async () => {
  const { serverId } = await (await route(req("/v1/challenge"), env)).json();
  // Alex joined, but the request claims Steve.
  const r = await route(post({ username: "Steve", serverId, cape: "galaxy" }), env, elyStub(new Map([[serverId, "Alex"]])));
  assert.equal(r.status, 403);
  assert.equal(await env.CAPES.get("p:steve"), null);
});

test("unknown challenge, unknown cape and bad JSON are rejected", async () => {
  const fake = "0".repeat(32);
  assert.equal((await route(post({ username: "Steve", serverId: fake, cape: "galaxy" }), env, elyStub(new Map()))).status, 403);
  const { serverId } = await (await route(req("/v1/challenge"), env)).json();
  assert.equal((await route(post({ username: "Steve", serverId, cape: "minecon" }), env, elyStub(new Map()))).status, 400);
  const bad = req("/v1/select", { method: "POST", body: "{" });
  assert.equal((await route(bad, env)).status, 400);
});

test("selects are rate-limited per address", async () => {
  let last;
  for (let i = 0; i < 11; i++) {
    last = await route(post({ username: "Steve", serverId: "0".repeat(32), cape: "galaxy" }, "9.9.9.9"), env, elyStub(new Map()));
  }
  assert.equal(last.status, 429);
});

test("hasJoined checks the returned name", async () => {
  const stub = async () => new Response(JSON.stringify({ id: "b".repeat(32), name: "Other" }), { status: 200 });
  assert.equal(await elyHasJoined("Steve", "x", stub), null);
  const ok = async () => new Response(JSON.stringify({ id: "b".repeat(32), name: "steve" }), { status: 200 });
  assert.equal(await elyHasJoined("Steve", "x", ok), "steve");
});

test("unknown routes 404", async () => {
  assert.equal((await route(req("/v2/nope"), env)).status, 404);
});
