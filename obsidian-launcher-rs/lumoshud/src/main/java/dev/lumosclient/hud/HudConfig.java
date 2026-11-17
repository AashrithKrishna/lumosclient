package dev.lumosclient.hud;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.lumosclient.hud.core.Theme;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;

/**
 * config/lumoshud.json in the instance folder. The launcher's HUD page edits the same file
 * (obsidian-launcher-rs/src/hud_config.rs), so both sides keep keys they don't know.
 *
 * <pre>
 * {
 *   "version": 1,
 *   "editorKey": "key.keyboard.right.shift",   // Minecraft key name (as options.txt), opens the editor
 *   "snapping": true,                          // editor snaps to screen edges and other modules
 *   "theme": {                                 // how modules are drawn (core/Theme.java)
 *     "style": "lunar",                        // lunar (rounded panels, icons) | minimal (flat, text only)
 *     "accent": "#FF6B2C",                     // pressed keys, icon highlights, the editor
 *     "cornerRadius": 3,                       // 0 – 6
 *     "panelOpacity": 45                       // percent; the default for modules' backgroundOpacity
 *   },
 *   "modules": {
 *     "&lt;id&gt;": {                                 // fps, cps, keystrokes, coordinates, direction, biome,
 *                                                // armor, potions, ping, clock, server, day, speed,
 *                                                // reach, combo, itemcounter, memory, sprint
 *       "enabled": true,
 *       "anchor": "top_left",                    // top_left|top_center|top_right|middle_left|center|
 *                                                // middle_right|bottom_left|bottom_center|bottom_right
 *       "x": 4, "y": 4,                          // GUI pixels from the anchor point of the screen to
 *                                                // the same point of the module's box
 *       "scale": 1.0,                            // 0.5 – 3.0
 *       "textColor": "#FFFFFF",                  // #RRGGBB or #AARRGGBB
 *       "background": true,
 *       "backgroundColor": "#000000",
 *       "backgroundOpacity": 45,                 // percent, 0 – 100 (theme.panelOpacity when missing)
 *       "shadow": true,                          // text shadow
 *       "icon": true,                            // Lunar style: the module's icon
 *       ...                                      // module options, e.g. "showLabel", "format24h"
 *     }
 *   }
 * }
 * </pre>
 * Anything missing takes its default, so an empty file (or none) is the default layout.
 */
public final class HudConfig {
    public static final int VERSION = 1;
    public static final String DEFAULT_EDITOR_KEY = "key.keyboard.right.shift";
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();

    private final Path path;
    private JsonObject root = new JsonObject();
    private long loadedMtime = -1;

    public HudConfig(Path path) {
        this.path = path;
    }

    public Path path() {
        return path;
    }

    public synchronized void load() {
        root = new JsonObject();
        try {
            if (Files.isRegularFile(path)) {
                JsonElement e = JsonParser.parseString(Files.readString(path, StandardCharsets.UTF_8));
                if (e.isJsonObject()) root = e.getAsJsonObject();
                loadedMtime = Files.getLastModifiedTime(path).toMillis();
            }
        } catch (Exception e) {
            LumosHud.LOG.warn("[LumosHUD] Couldn't read {}: {}", path, e.toString());
        }
    }

    /** Re-reads the file when something else (the launcher) changed it. */
    public synchronized boolean reloadIfChanged() {
        try {
            if (!Files.isRegularFile(path)) return false;
            long m = Files.getLastModifiedTime(path).toMillis();
            if (m == loadedMtime) return false;
            load();
            return true;
        } catch (IOException e) {
            return false;
        }
    }

    public synchronized void save() {
        root.addProperty("version", VERSION);
        try {
            Files.createDirectories(path.getParent());
            Path tmp = path.resolveSibling(path.getFileName() + ".tmp");
            Files.writeString(tmp, GSON.toJson(root) + "\n", StandardCharsets.UTF_8);
            Files.move(tmp, path, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            loadedMtime = Files.getLastModifiedTime(path).toMillis();
        } catch (IOException e) {
            LumosHud.LOG.warn("[LumosHUD] Couldn't save {}: {}", path, e.toString());
        }
    }

    /** Forgets every module's settings and position (the editor's Reset). */
    public synchronized void resetModules() {
        root.remove("modules");
    }

    public synchronized String editorKey() {
        return str(root, "editorKey", DEFAULT_EDITOR_KEY);
    }

    public synchronized boolean snapping() {
        return bool(root, "snapping", true);
    }

    public synchronized void setSnapping(boolean on) {
        root.addProperty("snapping", on);
    }

    private JsonObject cachedThemeSrc;
    private String cachedThemeJson;
    private Theme cachedTheme = Theme.DEFAULT;

    /** The "theme" object, read once per change (modules ask for it every frame). */
    public synchronized Theme theme() {
        JsonObject t = root.has("theme") && root.get("theme").isJsonObject() ? root.getAsJsonObject("theme") : null;
        if (t == null) return Theme.DEFAULT;
        String key = t.toString();
        if (t != cachedThemeSrc || !key.equals(cachedThemeJson)) {
            cachedThemeSrc = t;
            cachedThemeJson = key;
            cachedTheme = Theme.of(str(t, "style", null), str(t, "accent", null),
                num(t, "cornerRadius", Double.NaN), num(t, "panelOpacity", Double.NaN));
        }
        return cachedTheme;
    }

    /** Sets one "theme" key (the editor's Style button). */
    public synchronized void setTheme(String key, String value) {
        JsonObject t = root.has("theme") && root.get("theme").isJsonObject() ? root.getAsJsonObject("theme") : null;
        if (t == null) {
            t = new JsonObject();
            root.add("theme", t);
        }
        t.addProperty(key, value);
    }

    /** The module's own object, created when missing. */
    public synchronized JsonObject module(String id) {
        JsonObject mods = root.has("modules") && root.get("modules").isJsonObject() ? root.getAsJsonObject("modules") : null;
        if (mods == null) {
            mods = new JsonObject();
            root.add("modules", mods);
        }
        JsonElement m = mods.get(id);
        if (m == null || !m.isJsonObject()) {
            m = new JsonObject();
            mods.add(id, m);
        }
        return m.getAsJsonObject();
    }

    static boolean bool(JsonObject o, String k, boolean d) {
        try {
            JsonElement e = o.get(k);
            return e != null && e.isJsonPrimitive() && e.getAsJsonPrimitive().isBoolean() ? e.getAsBoolean() : d;
        } catch (Exception ex) {
            return d;
        }
    }

    static double num(JsonObject o, String k, double d) {
        try {
            JsonElement e = o.get(k);
            return e != null && e.isJsonPrimitive() && e.getAsJsonPrimitive().isNumber() ? e.getAsDouble() : d;
        } catch (Exception ex) {
            return d;
        }
    }

    static String str(JsonObject o, String k, String d) {
        try {
            JsonElement e = o.get(k);
            return e != null && e.isJsonPrimitive() && e.getAsJsonPrimitive().isString() ? e.getAsString() : d;
        } catch (Exception ex) {
            return d;
        }
    }
}
