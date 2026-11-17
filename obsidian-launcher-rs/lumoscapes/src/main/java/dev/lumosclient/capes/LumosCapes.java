package dev.lumosclient.capes;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.blaze3d.platform.NativeImage;
import java.io.InputStream;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.Locale;
import java.util.Properties;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.regex.Pattern;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.core.ClientAsset;
import net.minecraft.resources.Identifier;
import net.minecraft.world.entity.player.PlayerSkin;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Lumos capes in game. For each player drawn, {@link #apply} looks the player's name up on the
 * Lumos capes server (once, in the background, cached for a few minutes), downloads that cape's
 * texture once, and from then on hands the game a skin whose cape and elytra use it.
 *
 * <p>Nothing here blocks the render thread: lookups and downloads run on two background
 * threads, and only the finished texture is uploaded on the game thread.
 *
 * <p>The server address comes from the {@code lumoscapes.url} system property (LumosClient sets
 * it) or {@code url=} in {@code config/lumoscapes.properties}. Without one the mod does nothing.
 */
public final class LumosCapes {
    private static final Logger LOG = LoggerFactory.getLogger("LumosCapes");
    private static final Pattern CAPE_ID = Pattern.compile("[a-z0-9_]{1,32}");
    private static final Pattern NAME = Pattern.compile("[A-Za-z0-9_.-]{1,32}");
    private static final long FRESH_MS = 5 * 60_000L;
    private static final long RETRY_MS = 60_000L;
    private static final int MAX_TEXTURE_BYTES = 64 * 1024;
    private static final int MAX_PLAYERS = 2000;

    static final String BASE = serverUrl();

    /** What the server said about a player: the cape (or null) and when to ask again. */
    private record Lookup(String cape, long refreshAt, boolean pending) {}

    private static final ConcurrentHashMap<String, Lookup> PLAYERS = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<String, ClientAsset.Texture> TEXTURES = new ConcurrentHashMap<>();
    /** Cape id -> time of the last download attempt that hasn't produced a texture. */
    private static final ConcurrentHashMap<String, Long> TEXTURE_TRIES = new ConcurrentHashMap<>();

    private static final ExecutorService POOL = Executors.newFixedThreadPool(2, r -> {
        Thread t = new Thread(r, "LumosCapes");
        t.setDaemon(true);
        return t;
    });
    private static final HttpClient HTTP = HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(10))
            .followRedirects(HttpClient.Redirect.NORMAL)
            .executor(POOL)
            .build();

    private LumosCapes() {}

    /** The skin to draw for the player called {@code name}: {@code skin} with their Lumos cape, if any. */
    public static PlayerSkin apply(String name, PlayerSkin skin) {
        if (BASE == null || name == null || skin == null || !NAME.matcher(name).matches()) {
            return skin;
        }
        String key = name.toLowerCase(Locale.ROOT);
        long now = System.currentTimeMillis();
        Lookup known = PLAYERS.get(key);
        if (known == null || (!known.pending() && now >= known.refreshAt())) {
            refresh(key, known);
        }
        String cape = known == null ? null : known.cape();
        if (cape == null) {
            return skin;
        }
        ClientAsset.Texture texture = TEXTURES.get(cape);
        if (texture == null) {
            loadTexture(cape, now);
            return skin;
        }
        return new PlayerSkin(skin.body(), texture, texture, skin.model(), skin.secure());
    }

    private static void refresh(String key, Lookup known) {
        Lookup pending = new Lookup(known == null ? null : known.cape(), 0, true);
        boolean mine = known == null ? PLAYERS.putIfAbsent(key, pending) == null : PLAYERS.replace(key, known, pending);
        if (!mine) {
            return; // another frame already asked
        }
        if (PLAYERS.size() > MAX_PLAYERS) {
            PLAYERS.clear();
            PLAYERS.put(key, pending);
        }
        HttpRequest req = HttpRequest.newBuilder(URI.create(BASE + "/v1/player/" + URLEncoder.encode(key, StandardCharsets.UTF_8)))
                .timeout(Duration.ofSeconds(15))
                .header("Accept", "application/json")
                .GET()
                .build();
        HTTP.sendAsync(req, HttpResponse.BodyHandlers.ofString()).whenComplete((resp, err) -> {
            long now = System.currentTimeMillis();
            if (err != null || resp.statusCode() != 200) {
                PLAYERS.put(key, new Lookup(pending.cape(), now + RETRY_MS, false));
                return;
            }
            PLAYERS.put(key, new Lookup(parseCape(resp.body()), now + FRESH_MS, false));
        });
    }

    /** The cape id in a {@code /v1/player} answer, or null. */
    static String parseCape(String body) {
        try {
            JsonElement root = JsonParser.parseString(body);
            if (!root.isJsonObject()) {
                return null;
            }
            JsonElement cape = ((JsonObject) root).get("cape");
            if (cape == null || !cape.isJsonPrimitive() || !cape.getAsJsonPrimitive().isString()) {
                return null;
            }
            String id = cape.getAsString();
            return CAPE_ID.matcher(id).matches() ? id : null;
        } catch (RuntimeException e) {
            return null;
        }
    }

    private static void loadTexture(String cape, long now) {
        Long last = TEXTURE_TRIES.get(cape);
        if (last != null && now - last < RETRY_MS) {
            return;
        }
        boolean mine = last == null ? TEXTURE_TRIES.putIfAbsent(cape, now) == null : TEXTURE_TRIES.replace(cape, last, now);
        if (!mine) {
            return;
        }
        String url = BASE + "/capes/" + cape + ".png";
        HttpRequest req = HttpRequest.newBuilder(URI.create(url)).timeout(Duration.ofSeconds(20)).GET().build();
        HTTP.sendAsync(req, HttpResponse.BodyHandlers.ofByteArray()).whenComplete((resp, err) -> {
            if (err != null || resp.statusCode() != 200 || resp.body().length > MAX_TEXTURE_BYTES) {
                LOG.debug("Couldn't download cape {}", cape, err);
                return; // TEXTURE_TRIES keeps the time, so it's tried again in a minute
            }
            byte[] png = resp.body();
            // Textures are made and uploaded on the game thread.
            Minecraft.getInstance().execute(() -> register(cape, url, png));
        });
    }

    private static void register(String cape, String url, byte[] png) {
        try {
            NativeImage image = NativeImage.read(png);
            Identifier id = Identifier.fromNamespaceAndPath("lumoscapes", "capes/" + cape);
            Minecraft.getInstance().getTextureManager().register(id, new DynamicTexture(() -> "LumosCapes " + cape, image));
            TEXTURES.put(cape, new ClientAsset.DownloadedTexture(id, url));
            TEXTURE_TRIES.remove(cape);
        } catch (Exception e) {
            LOG.warn("Cape {} isn't a usable image", cape, e);
        }
    }

    // ------------------------------------------------------------------ settings

    private static String serverUrl() {
        String url = System.getProperty("lumoscapes.url");
        if (url == null || url.isBlank()) {
            Path file = FabricLoader.getInstance().getConfigDir().resolve("lumoscapes.properties");
            if (Files.isRegularFile(file)) {
                Properties props = new Properties();
                try (InputStream in = Files.newInputStream(file)) {
                    props.load(in);
                    url = props.getProperty("url");
                } catch (Exception e) {
                    LOG.warn("Couldn't read {}", file, e);
                }
            }
        }
        String base = cleanUrl(url);
        if (base == null) {
            LOG.info("No capes server set (lumoscapes.url); Lumos capes are off.");
        } else {
            LOG.info("Lumos capes from {}", base);
        }
        return base;
    }

    /** https anywhere, plain http only on this computer; no trailing slash. Null when unusable. */
    static String cleanUrl(String url) {
        if (url == null) {
            return null;
        }
        String u = url.trim();
        while (u.endsWith("/")) {
            u = u.substring(0, u.length() - 1);
        }
        try {
            URI uri = new URI(u);
            String host = uri.getHost();
            if (host == null || uri.getUserInfo() != null || uri.getQuery() != null || uri.getFragment() != null) {
                return null;
            }
            boolean local = host.equals("localhost") || host.equals("127.0.0.1");
            if ("https".equals(uri.getScheme()) || ("http".equals(uri.getScheme()) && local)) {
                return u;
            }
        } catch (Exception e) {
            return null;
        }
        return null;
    }
}
