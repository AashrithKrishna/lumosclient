package dev.lumosclient.hud.core;

import java.util.ArrayList;
import java.util.List;

/** Placement maths for the HUD and its editor: no Minecraft classes, so it is unit-tested. */
public final class Layout {
    private Layout() {}

    public static final double MIN_SCALE = 0.5;
    public static final double MAX_SCALE = 3.0;

    public static double clampScale(double s) {
        if (Double.isNaN(s)) return 1.0;
        return Math.round(Math.max(MIN_SCALE, Math.min(MAX_SCALE, s)) * 100) / 100.0;
    }

    /** The left/top of a module of size (w, h) (scaled) placed at anchor + offset on a screen. */
    public static Rect place(Anchor a, double x, double y, double w, double h, double screenW, double screenH) {
        double left = a.fx * screenW - a.fx * w + x;
        double top = a.fy * screenH - a.fy * h + y;
        // Keep the whole box on screen even after the window shrinks.
        left = Math.max(0, Math.min(screenW - w, left));
        top = Math.max(0, Math.min(screenH - h, top));
        return new Rect(left, top, w, h);
    }

    /** Anchor and offset (x, y) for a box the player dropped at (left, top). */
    public static Placement fromAbsolute(double left, double top, double w, double h, double screenW, double screenH) {
        left = Math.max(0, Math.min(screenW - w, left));
        top = Math.max(0, Math.min(screenH - h, top));
        Anchor a = Anchor.nearest(left + w / 2, top + h / 2, screenW, screenH);
        double x = left - a.fx * screenW + a.fx * w;
        double y = top - a.fy * screenH + a.fy * h;
        return new Placement(a, Math.round(x * 2) / 2.0, Math.round(y * 2) / 2.0);
    }

    public record Placement(Anchor anchor, double x, double y) {}

    /** Where a dragged box snaps, plus the guide lines to draw (screen x for vertical, y for horizontal). */
    public record Snap(double left, double top, List<Double> guidesX, List<Double> guidesY) {}

    /**
     * Snaps a dragged box's edges or centre to the screen's edges and centre and to the other
     * modules' edges and centres, when within {@code threshold} pixels.
     */
    public static Snap snap(Rect moving, List<Rect> others, double screenW, double screenH, double threshold) {
        List<Double> xs = new ArrayList<>(List.of(0.0, screenW / 2, screenW));
        List<Double> ys = new ArrayList<>(List.of(0.0, screenH / 2, screenH));
        for (Rect o : others) {
            xs.add(o.left()); xs.add(o.centerX()); xs.add(o.right());
            ys.add(o.top()); ys.add(o.centerY()); ys.add(o.bottom());
        }
        double[] bx = best(new double[] {moving.left(), moving.centerX(), moving.right()}, xs, threshold);
        double[] by = best(new double[] {moving.top(), moving.centerY(), moving.bottom()}, ys, threshold);
        List<Double> gx = new ArrayList<>(), gy = new ArrayList<>();
        double left = moving.left(), top = moving.top();
        if (bx != null) { left += bx[0]; gx.add(bx[1]); }
        if (by != null) { top += by[0]; gy.add(by[1]); }
        left = Math.max(0, Math.min(screenW - moving.width(), left));
        top = Math.max(0, Math.min(screenH - moving.height(), top));
        return new Snap(left, top, gx, gy);
    }

    /** {shift, line} for the closest (own edge → target line) pair within threshold, or null. */
    private static double[] best(double[] own, List<Double> targets, double threshold) {
        double[] best = null;
        for (double e : own) {
            for (double t : targets) {
                double d = t - e;
                if (Math.abs(d) <= threshold && (best == null || Math.abs(d) < Math.abs(best[0]))) {
                    best = new double[] {d, t};
                }
            }
        }
        return best;
    }
}
