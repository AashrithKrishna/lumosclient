package dev.lumosclient.hud.core;

import java.util.Locale;

/** Text the modules show, kept free of Minecraft classes so it is unit-tested. */
public final class Fmt {
    private Fmt() {}

    private static final String[] NAMES = {"South", "West", "North", "East"};
    private static final String[] AXES = {"+Z", "-X", "-Z", "+X"};
    private static final String[] EIGHT = {"S", "SW", "W", "NW", "N", "NE", "E", "SE"};

    /** Minecraft yaw (0 = south, 90 = west) → 0..3 for South, West, North, East. */
    public static int quadrant(float yaw) {
        float y = ((yaw % 360f) + 360f) % 360f;
        return (int) Math.floor((y + 45f) / 90f) % 4;
    }

    public static String facing(float yaw, boolean axis) {
        int q = quadrant(yaw);
        return axis ? NAMES[q] + " (" + AXES[q] + ")" : NAMES[q];
    }

    /** "N", "NE", … for compact displays. */
    public static String compass(float yaw) {
        float y = ((yaw % 360f) + 360f) % 360f;
        return EIGHT[(int) Math.floor((y + 22.5f) / 45f) % 8];
    }

    /** Effect time left: "1:05", "12:00", "1:02:03", or "∞" for infinite. */
    public static String duration(int ticks) {
        if (ticks < 0) return "∞";
        int secs = ticks / 20;
        int h = secs / 3600, m = (secs / 60) % 60, s = secs % 60;
        return h > 0 ? String.format(Locale.ROOT, "%d:%02d:%02d", h, m, s) : String.format(Locale.ROOT, "%d:%02d", m, s);
    }

    public static String roman(int n) {
        if (n <= 0 || n > 3999) return Integer.toString(n);
        int[] v = {1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1};
        String[] r = {"M", "CM", "D", "CD", "C", "XC", "L", "XL", "X", "IX", "V", "IV", "I"};
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < v.length; i++) {
            while (n >= v[i]) { n -= v[i]; sb.append(r[i]); }
        }
        return sb.toString();
    }

    public static String decimals(double v, int places) {
        places = Math.max(0, Math.min(3, places));
        return String.format(Locale.ROOT, "%." + places + "f", v);
    }

    /** "minecraft:dark_forest" → "Dark Forest" (used when there is no translation). */
    public static String prettyId(String id) {
        String path = id.contains(":") ? id.substring(id.indexOf(':') + 1) : id;
        path = path.substring(path.lastIndexOf('/') + 1);
        StringBuilder sb = new StringBuilder();
        for (String w : path.split("_")) {
            if (w.isEmpty()) continue;
            if (sb.length() > 0) sb.append(' ');
            sb.append(Character.toUpperCase(w.charAt(0))).append(w.substring(1));
        }
        return sb.toString();
    }
}
