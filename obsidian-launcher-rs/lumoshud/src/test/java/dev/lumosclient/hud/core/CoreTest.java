package dev.lumosclient.hud.core;

import static org.junit.jupiter.api.Assertions.*;

import java.util.List;
import org.junit.jupiter.api.Test;

class CoreTest {
    @Test
    void placeIsAnchoredAndStaysOnScreen() {
        Rect r = Layout.place(Anchor.TOP_RIGHT, -4, 4, 50, 16, 400, 300);
        assertEquals(346, r.left(), 1e-9);
        assertEquals(4, r.top(), 1e-9);
        Rect c = Layout.place(Anchor.BOTTOM_CENTER, 0, -2, 40, 10, 400, 300);
        assertEquals(180, c.left(), 1e-9);
        assertEquals(288, c.top(), 1e-9);
        // A window too small for the offset still shows the module.
        Rect off = Layout.place(Anchor.TOP_LEFT, 900, 900, 50, 16, 400, 300);
        assertEquals(350, off.left(), 1e-9);
        assertEquals(284, off.top(), 1e-9);
    }

    @Test
    void fromAbsoluteRoundTrips() {
        for (double[] p : new double[][] {{3, 5}, {200, 140}, {340, 280}, {10, 150}}) {
            Layout.Placement pl = Layout.fromAbsolute(p[0], p[1], 50, 16, 400, 300);
            Rect back = Layout.place(pl.anchor(), pl.x(), pl.y(), 50, 16, 400, 300);
            assertEquals(p[0], back.left(), 0.5);
            assertEquals(p[1], back.top(), 0.5);
        }
        assertEquals(Anchor.BOTTOM_RIGHT, Layout.fromAbsolute(340, 280, 50, 16, 400, 300).anchor());
        // Resolution independence: a right-anchored module keeps its distance to the right edge.
        Layout.Placement pl = Layout.fromAbsolute(346, 4, 50, 16, 400, 300);
        assertEquals(Anchor.TOP_RIGHT, pl.anchor());
        assertEquals(746, Layout.place(pl.anchor(), pl.x(), pl.y(), 50, 16, 800, 600).left(), 1e-9);
    }

    @Test
    void snapsToScreenEdgesAndOtherModules() {
        Layout.Snap s = Layout.snap(new Rect(3, 50, 40, 10), List.of(), 400, 300, 4);
        assertEquals(0, s.left(), 1e-9);
        assertEquals(List.of(0.0), s.guidesX());
        Rect other = new Rect(100, 100, 60, 12);
        Layout.Snap t = Layout.snap(new Rect(102, 114, 30, 10), List.of(other), 400, 300, 4);
        assertEquals(100, t.left(), 1e-9);   // left edges aligned
        assertEquals(112, t.top(), 1e-9);    // stacked under the other module
        Layout.Snap far = Layout.snap(new Rect(50, 60, 30, 10), List.of(other), 400, 300, 4);
        assertEquals(50, far.left(), 1e-9);
        assertTrue(far.guidesX().isEmpty());
    }

    @Test
    void scaleIsClamped() {
        assertEquals(0.5, Layout.clampScale(0.1));
        assertEquals(3.0, Layout.clampScale(9));
        assertEquals(1.25, Layout.clampScale(1.25));
        assertEquals(1.0, Layout.clampScale(Double.NaN));
    }

    @Test
    void anchorsParse() {
        assertEquals(Anchor.BOTTOM_CENTER, Anchor.parse("bottom_center", Anchor.TOP_LEFT));
        assertEquals(Anchor.TOP_LEFT, Anchor.parse("nowhere", Anchor.TOP_LEFT));
        assertEquals("middle_right", Anchor.MIDDLE_RIGHT.id());
    }

    @Test
    void colours() {
        assertEquals(0xFFFFAA00, Colors.parse("#FFAA00", 0));
        assertEquals(0x80112233, Colors.parse("#80112233", 0));
        assertEquals(7, Colors.parse("blue", 7));
        assertEquals("#FFAA00", Colors.format(0xFFFFAA00));
        assertEquals(0x80FFFFFF, Colors.withOpacity(0xFFFFFF, 50));
        assertEquals(0xFF123456, Colors.opaque(0x00123456));
    }

    @Test
    void text() {
        assertEquals("South (+Z)", Fmt.facing(0, true));
        assertEquals("West", Fmt.facing(90, false));
        assertEquals("North", Fmt.facing(-180, false));
        assertEquals("East", Fmt.facing(-90, false));
        assertEquals("NE", Fmt.compass(-135));
        assertEquals("1:05", Fmt.duration(65 * 20));
        assertEquals("1:02:03", Fmt.duration(3723 * 20));
        assertEquals("∞", Fmt.duration(-1));
        assertEquals("IV", Fmt.roman(4));
        assertEquals("XIV", Fmt.roman(14));
        assertEquals("Dark Forest", Fmt.prettyId("minecraft:dark_forest"));
        assertEquals("4.25", Fmt.decimals(4.249, 2));
    }

    @Test
    void clicksExpireAfterASecond() {
        ClickCounter c = new ClickCounter();
        c.click(0); c.click(500); c.click(900);
        assertEquals(3, c.perSecond(950));
        assertEquals(1, c.perSecond(1600));
        assertEquals(0, c.perSecond(5000));
    }

    @Test
    void iconsAreNineByNineWithKnownColours() throws Exception {
        for (var f : Icons.class.getFields()) {
            if (f.getType() != char[][].class) continue;
            char[][] g = (char[][]) f.get(null);
            assertEquals(Icons.SIZE, g.length, f.getName());
            for (char[] row : g) {
                assertEquals(Icons.SIZE, row.length, f.getName());
                for (char ch : row) assertTrue(".#da".indexOf(ch) >= 0 || Icons.color(ch) != 0, f.getName() + ": " + ch);
            }
        }
    }

    @Test
    void compassNeedlePointsTheWayThePlayerFaces() {
        // The tip is just inside the ring.
        assertEquals('a', Icons.compass(180)[1][4]);   // north: up
        assertEquals('a', Icons.compass(0)[7][4]);     // south: down
        assertEquals('a', Icons.compass(90)[4][1]);    // west: left
        assertEquals('a', Icons.compass(-90)[4][7]);   // east: right
        assertEquals('#', Icons.compass(180)[5][4]);   // the tail
    }

    @Test
    void signalBarsFollowLatency() {
        assertEquals(4, Icons.signalBars(20));
        assertEquals(3, Icons.signalBars(120));
        assertEquals(2, Icons.signalBars(250));
        assertEquals(1, Icons.signalBars(900));
        char[][] g = Icons.signal(250);
        assertEquals('O', g[8][1]);
        assertEquals('d', g[8][7]);
        assertEquals('.', g[0][1]);
    }

    /** Every pixel of the rounded shape is covered exactly once; the corners are cut. */
    @Test
    void roundedPanelsCoverTheirShapeOnce() {
        for (int r = 0; r <= 6; r++) {
            for (boolean border : new boolean[] {true, false}) {
                int w = 40, h = 17;
                int[][] hits = new int[h][w];
                int borderPx = 0;
                for (Shapes.Run run : Shapes.panel(w, h, r, border)) {
                    for (int y = run.y0(); y < run.y1(); y++) {
                        for (int x = run.x0(); x < run.x1(); x++) {
                            hits[y][x]++;
                            if (run.kind() == Shapes.BORDER) borderPx++;
                        }
                    }
                }
                for (int[] row : hits) for (int v : row) assertTrue(v <= 1, "overlap at r=" + r);
                assertEquals(r == 0 ? 1 : 0, hits[0][0], "corner at r=" + r);
                assertEquals(1, hits[h / 2][0], "left edge at r=" + r);
                assertEquals(1, hits[h / 2][w / 2], "middle at r=" + r);
                if (border) assertTrue(borderPx >= 2 * (w + h) - 8 - 4 * r, "ring at r=" + r);
                else assertEquals(0, borderPx);
                // Symmetric left/right and top/bottom.
                for (int y = 0; y < h; y++) for (int x = 0; x < w; x++) {
                    assertEquals(hits[y][x], hits[y][w - 1 - x]);
                    assertEquals(hits[y][x], hits[h - 1 - y][x]);
                }
            }
        }
        assertEquals(3, Shapes.panel(30, 17, 3, true).stream().filter(x -> x.y0() <= 8 && 8 < x.y1()).count());
        assertTrue(Shapes.panel(0, 10, 3, true).isEmpty());
        assertFalse(Shapes.panel(3, 3, 6, true).isEmpty());
    }

    @Test
    void keyGlowRisesFastAndFadesBack() {
        float t = Shapes.approach(0, 1, 16, 35, 200);
        assertTrue(t > 0.4f && t < 0.5f);
        assertEquals(1f, Shapes.approach(t, 1, 100, 35, 200));
        assertEquals(0.92f, Shapes.approach(1, 0, 16, 35, 200), 1e-6);
        assertEquals(0f, Shapes.approach(0.1f, 0, 1000, 35, 200));
        assertEquals(0.5f, Shapes.approach(0.5f, 1, 0, 35, 200));
    }

    @Test
    void themeDefaultsAndClamps() {
        Theme d = Theme.of(null, null, Double.NaN, Double.NaN);
        assertEquals(Theme.DEFAULT, d);
        assertTrue(d.lunar());
        assertEquals(0xFFFF6B2C, d.accent());
        Theme t = Theme.of("Minimal", "#00AAFF", 99, -5);
        assertFalse(t.lunar());
        assertEquals("minimal", t.style());
        assertEquals(0xFF00AAFF, t.accent());
        assertEquals(Theme.MAX_RADIUS, t.radius());
        assertEquals(0, t.panelOpacity());
        assertTrue(Theme.of("anything else", "nope", 2.4, 60).lunar());
        assertEquals(Theme.DEFAULT_ACCENT, Theme.of("lunar", "nope", 2, 60).accent());
    }

    @Test
    void colourBlending() {
        assertEquals(0xFF808080, Colors.lerp(0xFF000000, 0xFFFFFFFF, 0.5f));
        assertEquals(0xFFFF6B2C, Colors.lerp(0x00000000, 0xFFFF6B2C, 1f));
        assertEquals(0x00000000, Colors.lerp(0x00000000, 0xFFFF6B2C, 0f));
        assertEquals(0x80FFFFFF, Colors.fade(0xFFFFFFFF, 0.5f));
        assertEquals(0x00FFFFFF, Colors.fade(0xFFFFFFFF, 0f));
    }
}
