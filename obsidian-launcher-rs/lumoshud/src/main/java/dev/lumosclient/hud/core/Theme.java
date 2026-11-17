package dev.lumosclient.hud.core;

import java.util.Locale;

/**
 * lumoshud.json's "theme": how every module is drawn.
 * <pre>
 * "theme": { "style": "lunar",          // lunar: rounded panels with icons | minimal: flat boxes, text only
 *            "accent": "#FF6B2C",       // pressed keys, icon highlights, the editor
 *            "cornerRadius": 3,         // 0 – 6 GUI pixels
 *            "panelOpacity": 45 }       // percent; a module's own "backgroundOpacity" wins
 * </pre>
 */
public record Theme(boolean lunar, int accent, int radius, int panelOpacity) {
    public static final int DEFAULT_ACCENT = 0xFFFF6B2C;
    public static final int DEFAULT_RADIUS = 3, MAX_RADIUS = 6;
    public static final int DEFAULT_OPACITY = 45;
    public static final Theme DEFAULT = new Theme(true, DEFAULT_ACCENT, DEFAULT_RADIUS, DEFAULT_OPACITY);

    /** From the JSON values (any may be null / NaN), checked and clamped. */
    public static Theme of(String style, String accent, double radius, double opacity) {
        boolean lunar = style == null || !style.trim().toLowerCase(Locale.ROOT).equals("minimal");
        int a = Colors.opaque(Colors.parse(accent, DEFAULT_ACCENT)) | 0xFF000000;
        int r = Double.isNaN(radius) ? DEFAULT_RADIUS : (int) Math.round(Math.max(0, Math.min(MAX_RADIUS, radius)));
        int o = Double.isNaN(opacity) ? DEFAULT_OPACITY : (int) Math.round(Math.max(0, Math.min(100, opacity)));
        return new Theme(lunar, a, r, o);
    }

    public String style() {
        return lunar ? "lunar" : "minimal";
    }
}
