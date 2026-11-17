package dev.lumosclient.hud.core;

import java.util.Locale;

/**
 * The screen point a module is placed relative to. A module's offset (x, y) is measured from
 * this point of the screen to the same point of the module's box, so a module anchored to the
 * right edge stays there at every window size and GUI scale.
 */
public enum Anchor {
    TOP_LEFT(0, 0), TOP_CENTER(1, 0), TOP_RIGHT(2, 0),
    MIDDLE_LEFT(0, 1), CENTER(1, 1), MIDDLE_RIGHT(2, 1),
    BOTTOM_LEFT(0, 2), BOTTOM_CENTER(1, 2), BOTTOM_RIGHT(2, 2);

    /** 0 = left/top, 0.5 = centre, 1 = right/bottom. */
    public final double fx, fy;

    Anchor(int col, int row) {
        this.fx = col / 2.0;
        this.fy = row / 2.0;
    }

    /** The JSON name: "top_left", "bottom_center", … */
    public String id() {
        return name().toLowerCase(Locale.ROOT);
    }

    public static Anchor parse(String s, Anchor fallback) {
        if (s == null) return fallback;
        try {
            return valueOf(s.trim().toUpperCase(Locale.ROOT).replace('-', '_'));
        } catch (IllegalArgumentException e) {
            return fallback;
        }
    }

    /** The anchor of the screen third (horizontally and vertically) a point falls in. */
    public static Anchor nearest(double cx, double cy, double screenW, double screenH) {
        int col = cx < screenW / 3 ? 0 : cx > screenW * 2 / 3 ? 2 : 1;
        int row = cy < screenH / 3 ? 0 : cy > screenH * 2 / 3 ? 2 : 1;
        return values()[row * 3 + col];
    }
}
