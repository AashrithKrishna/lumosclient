package dev.lumosclient.hud.core;

/** "#RRGGBB" / "#AARRGGBB" colours of the config, as the ARGB ints Minecraft draws with. */
public final class Colors {
    private Colors() {}

    public static int parse(String s, int fallback) {
        if (s == null) return fallback;
        String h = s.trim();
        if (h.startsWith("#")) h = h.substring(1);
        try {
            if (h.length() == 6) return 0xFF000000 | Integer.parseInt(h, 16);
            if (h.length() == 8) return (int) Long.parseLong(h, 16);
        } catch (NumberFormatException ignored) {
        }
        return fallback;
    }

    public static String format(int argb) {
        return (argb >>> 24) == 0xFF
            ? String.format("#%06X", argb & 0xFFFFFF)
            : String.format("#%08X", argb);
    }

    /** The colour with its alpha replaced by an opacity in percent (0–100). */
    public static int withOpacity(int rgb, int percent) {
        int a = Math.round(Math.max(0, Math.min(100, percent)) * 255 / 100f);
        return (a << 24) | (rgb & 0xFFFFFF);
    }

    /** Text colour has to be opaque since 1.21.6 (alpha 0 draws nothing). */
    public static int opaque(int argb) {
        return (argb >>> 24) == 0 ? argb | 0xFF000000 : argb;
    }

    /** a → b by t (0..1), every channel including alpha. */
    public static int lerp(int a, int b, float t) {
        t = Math.max(0, Math.min(1, t));
        int out = 0;
        for (int sh = 0; sh <= 24; sh += 8) {
            int ca = (a >>> sh) & 0xFF, cb = (b >>> sh) & 0xFF;
            out |= (Math.round(ca + (cb - ca) * t) & 0xFF) << sh;
        }
        return out;
    }

    /** The colour with its alpha multiplied by f (0..1). */
    public static int fade(int argb, float f) {
        int a = Math.round(((argb >>> 24) & 0xFF) * Math.max(0, Math.min(1, f)));
        return (a << 24) | (argb & 0xFFFFFF);
    }
}
