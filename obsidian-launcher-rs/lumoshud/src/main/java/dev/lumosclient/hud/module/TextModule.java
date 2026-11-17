package dev.lumosclient.hud.module;

import dev.lumosclient.hud.core.Anchor;

/** A one-line module: "FPS: 144" in a box. */
public abstract class TextModule extends HudModule {
    private final String label;

    protected TextModule(String id, String name, String label, boolean enabled, Anchor anchor, double x, double y) {
        super(id, name, enabled, anchor, x, y);
        this.label = label;
        if (label != null) option("showLabel", true);
    }

    /** The value, or null when there is nothing to show (e.g. ping in singleplayer). */
    protected abstract String value(Ctx c);

    /** Shown in the editor when {@link #value} is null. */
    protected abstract String sample();

    protected final String line(Ctx c) {
        String v = value(c);
        if (v == null) {
            if (!c.preview()) return null;
            v = sample();
        }
        return label != null && bool("showLabel", true) ? label + ": " + v : v;
    }

    @Override
    public boolean visible(Ctx c) {
        return c.preview() || value(c) != null;
    }

    @Override
    public int[] size(Ctx c) {
        String s = line(c);
        int w = s == null ? 40 : c.font().width(s);
        return new int[] {Math.max(w + PAD_X * 2, 28), c.font().lineHeight - 1 + PAD_Y * 2};
    }

    @Override
    public void draw(Ctx c, int w, int h) {
        String s = line(c);
        if (s == null) return;
        box(c, w, h);
        text(c, s, (w - c.font().width(s)) / 2, PAD_Y, textColor());
    }
}
