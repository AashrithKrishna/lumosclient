package dev.lumosclient.hud.core;

/** A box on screen in GUI pixels (already scaled). */
public record Rect(double left, double top, double width, double height) {
    public double right() { return left + width; }
    public double bottom() { return top + height; }
    public double centerX() { return left + width / 2; }
    public double centerY() { return top + height / 2; }

    public boolean contains(double x, double y) {
        return x >= left && x < right() && y >= top && y < bottom();
    }

    public Rect at(double l, double t) {
        return new Rect(l, t, width, height);
    }
}
