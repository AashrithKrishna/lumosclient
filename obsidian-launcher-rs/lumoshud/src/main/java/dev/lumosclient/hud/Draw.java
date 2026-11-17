package dev.lumosclient.hud;

import dev.lumosclient.hud.core.Colors;
import dev.lumosclient.hud.core.Icons;
import dev.lumosclient.hud.core.Shapes;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.gui.GuiGraphics;

/** The Lunar style's shapes: rounded translucent panels with a faint border, and pixel icons. */
public final class Draw {
    private Draw() {}

    private static final Map<Long, List<Shapes.Run>> PANELS = new HashMap<>();

    private static List<Shapes.Run> runs(int w, int h, int r, boolean border) {
        long key = ((long) w << 32) | ((long) (h & 0xFFFF) << 16) | ((r & 0xFF) << 1) | (border ? 1 : 0);
        synchronized (PANELS) {
            if (PANELS.size() > 512) PANELS.clear();
            return PANELS.computeIfAbsent(key, k -> Shapes.panel(w, h, r, border));
        }
    }

    /**
     * A rounded panel at (x, y): {@code fill} inside, and a 1px ring a little lighter than the
     * panel whose strength follows the fill's opacity (none on a clear panel).
     */
    public static void panel(GuiGraphics g, int x, int y, int w, int h, int radius, int fill) {
        int alpha = fill >>> 24;
        int border = alpha == 0 ? 0 : borderFor(fill);
        for (Shapes.Run r : runs(w, h, radius, border != 0)) {
            int color = r.kind() == Shapes.FILL ? fill : border;
            if ((color >>> 24) == 0) continue;
            g.fill(x + r.x0(), y + r.y0(), x + r.x1(), y + r.y1(), color);
        }
    }

    /** The ring around a panel: the panel's colour lifted toward white, a bit more opaque. */
    static int borderFor(int fill) {
        int a = fill >>> 24;
        int lifted = Colors.lerp(fill | 0xFF000000, 0xFFFFFFFF, 0.28f);
        return (Math.min(255, Math.round(a * 0.9f + 18)) << 24) | (lifted & 0xFFFFFF);
    }

    /**
     * A 9×9 icon at (x, y): '#' in {@code primary}, 'd' in it faded, 'a' in {@code accent},
     * fixed colours as {@link Icons#color}; runs of one colour are drawn as one rectangle.
     */
    public static void icon(GuiGraphics g, char[][] grid, int x, int y, int primary, int accent) {
        if (grid == null) return;
        int dim = Colors.fade(primary, 0.45f);
        for (int row = 0; row < grid.length; row++) {
            char[] line = grid[row];
            int col = 0;
            while (col < line.length) {
                char ch = line[col];
                int end = col + 1;
                while (end < line.length && line[end] == ch) end++;
                int color = switch (ch) {
                    case '.' -> 0;
                    case '#' -> primary;
                    case 'd' -> dim;
                    case 'a' -> accent;
                    default -> Icons.color(ch);
                };
                if ((color >>> 24) != 0) g.fill(x + col, y + row, x + end, y + row + 1, color);
                col = end;
            }
        }
    }
}
