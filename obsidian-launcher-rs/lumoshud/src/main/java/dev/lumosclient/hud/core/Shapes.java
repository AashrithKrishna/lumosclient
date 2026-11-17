package dev.lumosclient.hud.core;

import java.util.ArrayList;
import java.util.List;

/** Rounded panels as a few axis-aligned rectangles (GuiGraphics.fill), no Minecraft classes. */
public final class Shapes {
    private Shapes() {}

    public static final int FILL = 0, BORDER = 1;

    /** {x0, y0, x1, y1, FILL|BORDER}; together they cover the rounded w×h shape exactly once. */
    public record Run(int x0, int y0, int x1, int y1, int kind) {}

    /** How far row {@code y} (0 = top) of a top corner is cut in, for radius r. */
    static int inset(int y, int r) {
        if (r <= 0 || y >= r) return 0;
        if (r == 1) return 1;
        double dy = r - y - 0.5;
        return r - (int) Math.floor(Math.sqrt(r * r - dy * dy) + 0.25);
    }

    static int rowInset(int y, int h, int r) {
        if (y < r) return inset(y, r);
        if (y >= h - r) return inset(h - 1 - y, r);
        return 0;
    }

    /**
     * A rounded w×h panel with radius r: the inside, and (when {@code border}) a 1-pixel ring
     * that follows the same curve. Rows with the same spans are merged.
     */
    public static List<Run> panel(int w, int h, int r, boolean border) {
        List<Run> out = new ArrayList<>();
        if (w <= 0 || h <= 0) return out;
        r = Math.max(0, Math.min(r, Math.min(w, h) / 2));
        int ir = Math.max(0, r - 1);
        int startY = 0;
        int[] prev = null;
        for (int y = 0; y <= h; y++) {
            int[] cur = null;
            if (y < h) {
                int o = rowInset(y, h, r);
                int i = -1; // inner inset, -1 = the whole row is ring
                if (border && y >= 1 && y < h - 1 && w > 2) {
                    i = Math.max(o + 1, 1 + rowInset(y - 1, h - 2, Math.min(ir, Math.min(w - 2, h - 2) / 2)));
                    if (i >= w - i) i = -1;
                }
                cur = border ? new int[] {o, i} : new int[] {o, -2};
            }
            if (prev != null && (cur == null || cur[0] != prev[0] || cur[1] != prev[1])) {
                emit(out, w, startY, y, prev[0], prev[1]);
                startY = y;
            }
            if (prev == null) startY = y;
            prev = cur;
        }
        return out;
    }

    private static void emit(List<Run> out, int w, int y0, int y1, int o, int i) {
        if (i == -2) { out.add(new Run(o, y0, w - o, y1, FILL)); return; }
        if (i == -1) { out.add(new Run(o, y0, w - o, y1, BORDER)); return; }
        out.add(new Run(o, y0, i, y1, BORDER));
        out.add(new Run(i, y0, w - i, y1, FILL));
        out.add(new Run(w - i, y0, w - o, y1, BORDER));
    }

    /** Moves {@code cur} toward {@code target} (0..1): rises over {@code riseMs}, falls over {@code fallMs}. */
    public static float approach(float cur, float target, long dtMs, float riseMs, float fallMs) {
        if (dtMs <= 0) return cur;
        float span = target > cur ? riseMs : fallMs;
        if (span <= 0) return target;
        float step = dtMs / span;
        return target > cur ? Math.min(target, cur + step) : Math.max(target, cur - step);
    }
}
