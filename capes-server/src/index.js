// Lumos Client capes server (Cloudflare Worker).
//
//   GET  /v1/capes            catalog [{id, name, url}]
//   GET  /v1/player/<name>    {cape: id|null, url: string|null}   (Ely.by name, any case)
//   GET  /v1/challenge        {serverId}   one-time id, kept in KV for 5 minutes
//   POST /v1/select           {username, serverId, cape}   cape = id, or null to remove
//
// Ownership proof: the launcher "joins" serverId on Ely.by with the player's own session
// (POST authserver.ely.by/session/join), then this worker asks Ely.by whether that player
// joined it (GET /session/hasJoined), exactly like a Minecraft server checks a player.
// The cape PNGs are static assets (public/capes/<id>.png), served by Cloudflare directly.

import CATALOG from "./catalog.js";

export const ELY_HAS_JOINED = "https://authserver.ely.by/session/hasJoined";
const CHALLENGE_TTL = 300; // seconds (KV's minimum TTL is 60)
const NAME_RE = /^[A-Za-z0-9_.-]{1,32}$/;
const SERVER_ID_RE = /^[0-9a-f]{32}$/;
const CAPE_IDS = new Set(CATALOG.map((c) => c.id));

// Per-isolate fallback rate limiter (a Cloudflare isolate is reused for many requests).
// When the SELECT_LIMITER rate-limit binding is configured it is used instead.
const memoryHits = new Map();

export async function allowed(env, key, limit = 10, windowMs = 60_000, now = Date.now()) {
  if (env.SELECT_LIMITER && typeof env.SELECT_LIMITER.limit === "function") {
    const { success } = await env.SELECT_LIMITER.limit({ key });
    return success;
  }
  const hits = (memoryHits.get(key) || []).filter((t) => now - t < windowMs);
  if (hits.length >= limit) {
    memoryHits.set(key, hits);
    return false;
  }
  hits.push(now);
  memoryHits.set(key, hits);
  if (memoryHits.size > 5000) memoryHits.clear();
  return true;
}

export function resetLimits() {
  memoryHits.clear();
}

function json(body, status = 200, extra = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", ...extra },
  });
}

const error = (status, message) => json({ error: message }, status, { "cache-control": "no-store" });

function capeUrl(origin, id) {
  return `${origin}/capes/${id}.png`;
}

function randomHex(bytes = 16) {
  const a = new Uint8Array(bytes);
  crypto.getRandomValues(a);
  return [...a].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function handleCatalog(origin) {
  const list = CATALOG.map((c) => ({ id: c.id, name: c.name, url: capeUrl(origin, c.id) }));
  return json(list, 200, { "cache-control": "public, max-age=300" });
}

export async function handlePlayer(env, origin, rawName) {
  let name;
  try {
    name = decodeURIComponent(rawName);
  } catch {
    return error(400, "bad name");
  }
  if (!NAME_RE.test(name)) return error(400, "bad name");
  const stored = await env.CAPES.get(`p:${name.toLowerCase()}`, "json");
  const cape = stored && CAPE_IDS.has(stored.cape) ? stored.cape : null;
  return json({ cape, url: cape ? capeUrl(origin, cape) : null }, 200, {
    "cache-control": "public, max-age=60",
  });
}

export async function handleChallenge(env, ip) {
  if (!(await allowed(env, `ch:${ip}`, 20))) return error(429, "too many requests, wait a minute");
  const serverId = randomHex(16);
  await env.CAPES.put(`ch:${serverId}`, "1", { expirationTtl: CHALLENGE_TTL });
  return json({ serverId }, 200, { "cache-control": "no-store" });
}

/** Asks Ely.by whether `username` joined `serverId`. Returns the profile name, or null. */
export async function elyHasJoined(username, serverId, fetchFn = fetch) {
  const url = `${ELY_HAS_JOINED}?username=${encodeURIComponent(username)}&serverId=${encodeURIComponent(serverId)}`;
  const resp = await fetchFn(url, { headers: { accept: "application/json" } });
  if (resp.status !== 200) return null;
  let body;
  try {
    body = await resp.json();
  } catch {
    return null;
  }
  if (!body || typeof body.name !== "string" || typeof body.id !== "string") return null;
  return body.name.toLowerCase() === username.toLowerCase() ? body.name : null;
}

export async function handleSelect(request, env, ip, fetchFn = fetch) {
  if (!(await allowed(env, `sel:${ip}`, 10))) return error(429, "too many requests, wait a minute");
  let body;
  try {
    body = await request.json();
  } catch {
    return error(400, "send JSON");
  }
  const { username, serverId } = body || {};
  let cape = body ? body.cape : undefined;
  if (cape === "none" || cape === "") cape = null;
  if (typeof username !== "string" || !NAME_RE.test(username)) return error(400, "bad username");
  if (typeof serverId !== "string" || !SERVER_ID_RE.test(serverId)) return error(400, "bad serverId");
  if (cape !== null && !CAPE_IDS.has(cape)) return error(400, "unknown cape");

  const key = `ch:${serverId}`;
  if (!(await env.CAPES.get(key))) return error(403, "challenge expired, try again");
  const name = await elyHasJoined(username, serverId, fetchFn);
  if (!name) return error(403, "Ely.by could not confirm this account");
  // One use per challenge.
  await env.CAPES.delete(key);

  const pkey = `p:${name.toLowerCase()}`;
  if (cape === null) await env.CAPES.delete(pkey);
  else await env.CAPES.put(pkey, JSON.stringify({ cape, name, updated: Date.now() }));
  return json({ ok: true, name, cape }, 200, { "cache-control": "no-store" });
}

export async function route(request, env, fetchFn = fetch) {
  const url = new URL(request.url);
  const ip = request.headers.get("cf-connecting-ip") || "local";
  const path = url.pathname.replace(/\/+$/, "");
  if (request.method === "GET" && path === "/v1/capes") return handleCatalog(url.origin);
  if (request.method === "GET" && path.startsWith("/v1/player/")) {
    return handlePlayer(env, url.origin, path.slice("/v1/player/".length));
  }
  if (request.method === "GET" && path === "/v1/challenge") return handleChallenge(env, ip);
  if (request.method === "POST" && path === "/v1/select") return handleSelect(request, env, ip, fetchFn);
  if (request.method === "GET" && (path === "" || path === "/")) {
    return new Response("Lumos Client capes server. See /v1/capes\n", { headers: { "content-type": "text/plain" } });
  }
  return error(404, "not found");
}

export default {
  async fetch(request, env) {
    try {
      return await route(request, env);
    } catch (e) {
      return error(500, "server error");
    }
  },
};
