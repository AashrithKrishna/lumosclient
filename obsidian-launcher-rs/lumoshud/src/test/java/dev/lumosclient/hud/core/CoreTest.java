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
}
