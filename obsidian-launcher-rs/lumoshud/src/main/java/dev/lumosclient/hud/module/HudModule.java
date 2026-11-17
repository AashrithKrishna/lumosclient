package dev.lumosclient.hud.module;

import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import dev.lumosclient.hud.LumosHud;
import dev.lumosclient.hud.core.Anchor;
import dev.lumosclient.hud.core.Colors;
import dev.lumosclient.hud.core.Layout;
import dev.lumosclient.hud.core.Theme;
import dev.lumosclient.hud.Draw;
import java.util.LinkedHashMap;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.Font;
import net.minecraft.client.gui.GuiGraphics;

/**
 * One HUD element. Settings live in the module's object of lumoshud.json (see HudConfig);
 * this class only knows their defaults. Sizes and drawing are in unscaled GUI pixels at (0, 0):
 * the caller moves and scales the pose.
 */
public abstract class HudModule {
    public static final int PAD_X = 4, PAD_Y = 4;

    public final String id;
    public final String name;
    private final boolean defEnabled;
    private final Anchor defAnchor;
    private final double defX, defY;
    /** Module-specific options and their defaults (Boolean, Number or String). */
    public final Map<String, Object> options = new LinkedHashMap<>();
    private boolean defBackground = true;

    protected HudModule(String id, String name, boolean enabled, Anchor anchor, double x, double y) {
        this.id = id;
        this.name = name;
        this.defEnabled = enabled;
        this.defAnchor = anchor;
        this.defX = x;
        this.defY = y;
    }

    protected HudModule option(String key, Object def) {
        options.put(key, def);
        return this;
    }

    protected HudModule noBackgroundByDefault() {
        defBackground = false;
        return this;
    }

    /** What a frame needs. {@code preview} = the editor: draw sample values when there is nothing to show. */
    public record Ctx(Minecraft mc, GuiGraphics g, Font font, boolean preview, long now) {}

    /** Whether there is anything to draw now (the editor always draws). */
    public boolean visible(Ctx c) {
        return true;
    }

    /** Unscaled width and height. */
    public abstract int[] size(Ctx c);

    public abstract void draw(Ctx c, int w, int h);

    // ── settings ──

    protected JsonObject json() {
        return LumosHud.config().module(id);
    }

    public boolean enabled() { return bool("enabled", defEnabled); }
    public void setEnabled(boolean on) { json().addProperty("enabled", on); }
    public Anchor anchor() { return Anchor.parse(str("anchor", null), defAnchor); }
    public double x() { return num("x", defX); }
    public double y() { return num("y", defY); }
    public double scale() { return Layout.clampScale(num("scale", 1.0)); }
    public int textColor() { return Colors.opaque(Colors.parse(str("textColor", null), 0xFFFFFFFF)); }
    public boolean background() { return bool("background", defBackground); }
    public int backgroundColor() {
        int rgb = Colors.parse(str("backgroundColor", null), 0xFF000000);
        return Colors.withOpacity(rgb, (int) Math.round(num("backgroundOpacity", theme().panelOpacity())));
    }
    public boolean shadow() { return bool("shadow", true); }

    public Theme theme() { return LumosHud.config().theme(); }
    /** Lunar style (rounded panels, icons) rather than Minimal (flat boxes, text only). */
    public boolean lunar() { return theme().lunar(); }
    /** Whether the module has an icon the player can switch off ("icon"). */
    public boolean hasIcon() { return menuIcon() != null; }
    /** The module's icon setting; drawn only in the Lunar style. */
    public boolean iconOn() { return bool("icon", true); }
    public void setIcon(boolean on) { json().addProperty("icon", on); }
    /** Whether to draw the icon now. */
    public boolean showIcon() { return lunar() && iconOn() && hasIcon(); }
    /** The icon in the editor's module list (null: none). */
    public char[][] menuIcon() { return null; }
    /** The text colour, dimmed: labels and units next to a value. */
    public int dimColor() { return Colors.lerp(textColor(), 0xFF000000, 0.32f); }

    public void place(Anchor a, double x, double y) {
        JsonObject o = json();
        o.addProperty("anchor", a.id());
        o.addProperty("x", x);
        o.addProperty("y", y);
    }

    public void setScale(double s) {
        json().addProperty("scale", Layout.clampScale(s));
    }

    public boolean bool(String k, boolean d) {
        Object def = options.get(k);
        JsonPrimitive p = prim(k);
        if (p != null && p.isBoolean()) return p.getAsBoolean();
        return def instanceof Boolean b ? b : d;
    }

    public double num(String k, double d) {
        Object def = options.get(k);
        JsonPrimitive p = prim(k);
        if (p != null && p.isNumber()) return p.getAsDouble();
        return def instanceof Number n ? n.doubleValue() : d;
    }

    public String str(String k, String d) {
        Object def = options.get(k);
        JsonPrimitive p = prim(k);
        if (p != null && p.isString()) return p.getAsString();
        return def instanceof String s ? s : d;
    }

    private JsonPrimitive prim(String k) {
        var e = json().get(k);
        return e != null && e.isJsonPrimitive() ? e.getAsJsonPrimitive() : null;
    }

    // ── drawing helpers ──

    /** The module's box: a rounded panel in the Lunar style, a flat one in Minimal. */
    public void box(Ctx c, int w, int h) {
        box(c, 0, 0, w, h, backgroundColor());
    }

    public void box(Ctx c, int x, int y, int w, int h, int color) {
        if (!background()) return;
        if (lunar()) Draw.panel(c.g(), x, y, w, h, theme().radius(), color);
        else c.g().fill(x, y, x + w, y + h, color);
    }

    /** The module's icon at (x, y) in its text colour and the theme's accent. */
    protected void icon(Ctx c, char[][] grid, int x, int y) {
        Draw.icon(c.g(), grid, x, y, textColor(), theme().accent());
    }

    public void text(Ctx c, String s, int x, int y, int color) {
        c.g().drawString(c.font(), s, x, y, Colors.opaque(color), shadow());
    }
}
