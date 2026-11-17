# Lumos Capes server

A tiny Cloudflare Worker that hands out the free Lumos Client capes and remembers which
cape each Ely.by player picked. It runs on Cloudflare's free plan.

| Request | Answer |
| --- | --- |
| `GET /v1/capes` | `[{id, name, url}]`, the cape catalog |
| `GET /v1/player/<name>` | `{cape, url}` for an Ely.by name (any case); `cape` is `null` when none |
| `GET /v1/challenge` | `{serverId}`, a one-time id that is valid for 5 minutes |
| `POST /v1/select` | body `{username, serverId, cape}`; `cape: null` removes the cape |
| `GET /capes/<id>.png` | the cape texture (static file) |

How a player proves the account is theirs: the launcher asks for a challenge, "joins" it on
Ely.by with the player's own sign-in (`POST https://authserver.ely.by/session/join`), and
sends the choice. The worker then asks Ely.by `GET /session/hasJoined?username=..&serverId=..`,
the same check a Minecraft server does. Only a 200 answer naming that player counts.
Nobody's password or token ever reaches this server.

## Deploy (first time, about 10 minutes)

You need [Node.js](https://nodejs.org) 18 or newer and a free Cloudflare account.

1. Make a free account at <https://dash.cloudflare.com/sign-up> and confirm your email.
2. Open a terminal in this folder:
   ```sh
   cd capes-server
   ```
3. Sign Wrangler (Cloudflare's command-line tool) in. A browser window opens; press **Allow**.
   ```sh
   npx wrangler login
   ```
4. Create the storage for players' choices:
   ```sh
   npx wrangler kv namespace create CAPES
   ```
   It prints something like `id = "0f2ac74b498b48028cb68387df4c7c8f"`. Open `wrangler.toml`
   and put that id in place of `REPLACE_WITH_YOUR_KV_NAMESPACE_ID`.
5. Publish:
   ```sh
   npx wrangler deploy
   ```
   The last lines show your address, e.g. `https://lumos-capes.<your-subdomain>.workers.dev`.
   (The first time, Wrangler may ask you to pick a free `workers.dev` subdomain.)
6. Check it: open `https://lumos-capes.<your-subdomain>.workers.dev/v1/capes` in a browser.
   You should see the list of capes.
7. Tell the launcher where the server is: in
   `obsidian-launcher-rs/src/capes.rs` set
   ```rust
   pub const LUMOS_CAPES_URL: &str = "https://lumos-capes.<your-subdomain>.workers.dev";
   ```
   (no trailing slash) and rebuild the launcher. The launcher passes this address to the
   in-game LumosCapes mod, so nothing else needs changing.

To update later (new capes, code changes): `npx wrangler deploy` again.

## Free-plan limits

Workers' free plan allows 100,000 requests a day. KV's free plan allows 100,000 reads and
1,000 writes a day; each cape change uses about 3 writes (challenge, its removal, the choice),
so roughly 300 cape changes a day. Player lookups are reads and are cached for 60 seconds.
Cape changes are limited to 10 a minute per address (`[[ratelimits]]` in `wrangler.toml`).

## Adding or changing capes

The capes are drawn by `tools/gen_capes.py` (needs Python 3 and Pillow: `pip install pillow`):

```sh
python3 tools/gen_capes.py   # rewrites public/capes/*.png and src/catalog.js
npx wrangler deploy
```

Each PNG is a standard 64x32 cape texture: the visible side at (1,1) 10x16, the inner side
at (12,1), 1-pixel edges around them, and an elytra design at (22,0).

## Tests

```sh
npm test
```

Runs the worker against an in-memory KV and a fake Ely.by (Node's built-in test runner, no
installs needed). `npx wrangler dev` runs the worker locally on <http://localhost:8787>.
