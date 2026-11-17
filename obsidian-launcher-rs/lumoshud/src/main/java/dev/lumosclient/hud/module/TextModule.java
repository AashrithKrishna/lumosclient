package dev.lumosclient.hud.module;

import dev.lumosclient.hud.core.Anchor;
import dev.lumosclient.hud.core.Icons;

/**
 * A one-line module. Minimal style: "FPS: 144" centred in a flat box. Lunar style: a rounded
 * panel with the module's icon on the left, then the value with its label or unit dimmed:
 * "144 FPS", "Biome Plains".
 */
public abstract class TextModule extends HudModule {
    /** Lunar panel: side padding, gap after the icon, height (the icon and text are centred in it). */
    public static final int L_PAD = 5, L_GAP = 4, L_H = 17;

    private final String label;
    private final char[][] icon;
    private boolean labelAfter;

    protected TextModule(String id, String name, String label, char[][] icon, boolean enabled, Anchor anchor, double x, double y) {
        super(id, name, enabled, anchor, x, y);
        this.label = label;
        this.icon = icon;
        if (label != null) option("showLabel", true);
    }

    /** The label is a unit after the value in the Lunar style ("144 FPS"), not a prefix. */
    protected TextModule labelAfter() {
        labelAfter = true;
        return this;
    }

    /** The value, or null when there is nothing to show (e.g. ping in singleplayer). */
    protected abstract String value(Ctx c);

    /** Shown in the editor when {@link #value} is null. */
    protected abstract String sample();

    /** The value as the Lunar style shows it (e.g. without brackets). */
    protected String lunarValue(String v) {
        return v;
    }

    /** The icon now (some follow the game: the compass, ping bars, sun or moon). */
    protected char[][] icon(Ctx c) {
        return icon;
    }

    @Override
    public char[][] menuIcon() {
        return icon;
    }

    private String raw(Ctx c) {
        String v = value(c);
        return v == null && c.preview() ? sample() : v;
    }

    /** {dimmed prefix, value, dimmed suffix}; null when there is nothing to draw. */
    protected final String[] parts(Ctx c) {
        String v = raw(c);
        if (v == null) return null;
        boolean lbl = label != null && bool("showLabel", true);
        if (!lunar()) return new String[] {null, lbl ? label + ": " + v : v, null};
        v = lunarValue(v);
        if (!lbl) return new String[] {null, v, null};
        return labelAfter ? new String[] {null, v, label} : new String[] {label, v, null};
    }

    private int textWidth(Ctx c, String[] p) {
        int w = c.font().width(p[1]);
        if (p[0] != null) w += c.font().width(p[0] + " ");
        if (p[2] != null) w += c.font().width(" " + p[2]);
        return w;
    }

    @Override
    public boolean visible(Ctx c) {
        return c.preview() || value(c) != null;
    }

    @Override
    public int[] size(Ctx c) {
        String[] p = parts(c);
        if (!lunar()) {
            int w = p == null ? 40 : c.font().width(p[1]);
            return new int[] {Math.max(w + PAD_X * 2, 28), c.font().lineHeight - 1 + PAD_Y * 2};
        }
        int w = p == null ? 30 : textWidth(c, p);
        int iw = showIcon() ? Icons.SIZE + L_GAP : 0;
        return new int[] {Math.max(L_PAD * 2 + iw + w, 24), L_H};
    }

    @Override
    public void draw(Ctx c, int w, int h) {
        String[] p = parts(c);
        if (p == null) return;
        box(c, w, h);
        if (!lunar()) {
            text(c, p[1], (w - c.font().width(p[1])) / 2, PAD_Y, textColor());
            return;
        }
        int iw = showIcon() ? Icons.SIZE + L_GAP : 0;
        // Centred in the panel (it is only wider than the content when the text is very short).
        int x = (w - iw - textWidth(c, p)) / 2;
        if (iw > 0) {
            icon(c, icon(c), x, (h - Icons.SIZE) / 2);
            x += iw;
        }
        int ty = (h - 7) / 2;
        if (p[0] != null) {
            text(c, p[0], x, ty, dimColor());
            x += c.font().width(p[0] + " ");
        }
        text(c, p[1], x, ty, textColor());
        x += c.font().width(p[1]);
        if (p[2] != null) text(c, " " + p[2], x, ty, dimColor());
    }
}
