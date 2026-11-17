package dev.lumosclient.hud;

import dev.lumosclient.hud.LumosHud.Placed;
import dev.lumosclient.hud.core.Colors;
import dev.lumosclient.hud.core.Icons;
import dev.lumosclient.hud.core.Layout;
import dev.lumosclient.hud.core.Rect;
import dev.lumosclient.hud.module.HudModule;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;
import org.lwjgl.glfw.GLFW;

/**
 * The HUD editor (Right Shift by default): drag a module to move it (it snaps to the screen's
 * edges and centre and to other modules; hold Shift to place freely), scroll over it or drag its
 * corner to resize, right-click to hide it. "Modules" switches modules on and off and their
 * icons (the square on the right of each), "Style" picks Lunar or Minimal. Closing saves.
 */
public abstract class HudEditorScreen extends Screen {
    private static final int OUTLINE = 0x80FFFFFF;
    private static final int HANDLE = 5;
    private static final double SNAP = 4;

    private record Btn(int x, int y, int w, int h, String label, Runnable action, HudModule iconOf) {
        Btn(int x, int y, int w, int h, String label, Runnable action) { this(x, y, w, h, label, action, null); }
        boolean hit(double mx, double my) { return mx >= x && mx < x + w && my >= y && my < y + h; }
    }

    private List<Placed> placed = new ArrayList<>();
    private final List<Btn> buttons = new ArrayList<>();
    private final List<Btn> cells = new ArrayList<>();
    private HudModule dragging, resizing, selected;
    private double grabX, grabY;
    private List<Double> guidesX = List.of(), guidesY = List.of();
    private boolean panelOpen;

    protected HudEditorScreen() {
        super(Component.literal("LumosHUD"));
    }

    @Override
    public boolean isPauseScreen() {
        return false;
    }

    @Override
    public void renderBackground(GuiGraphics g, int mouseX, int mouseY, float partialTick) {
        g.fill(0, 0, width, height, 0x60000000);
    }

    private HudModule.Ctx ctx(GuiGraphics g) {
        return new HudModule.Ctx(Minecraft.getInstance(), g, font, true, System.currentTimeMillis());
    }

    private static int accent() {
        return LumosHud.config().theme().accent();
    }

    @Override
    public void render(GuiGraphics g, int mouseX, int mouseY, float partialTick) {
        final int ACCENT = accent();
        HudModule.Ctx c = ctx(g);
        placed = LumosHud.layout(c);
        HudModule hover = panelOpen ? null : at(mouseX, mouseY);

        int guide = (ACCENT & 0xFFFFFF) | 0xC0000000;
        for (double x : guidesX) g.fill((int) Math.round(x), 0, (int) Math.round(x) + 1, height, guide);
        for (double y : guidesY) g.fill(0, (int) Math.round(y), width, (int) Math.round(y) + 1, guide);

        for (Placed p : placed) {
            LumosHud.draw(c, p);
            Rect r = p.rect();
            boolean active = p.module() == hover || p.module() == dragging || p.module() == resizing || p.module() == selected;
            outline(g, r, active ? ACCENT : OUTLINE);
            if (active) {
                int rx = (int) Math.round(r.right()), ry = (int) Math.round(r.bottom());
                g.fill(rx - HANDLE, ry - HANDLE, rx, ry, ACCENT);
            }
        }

        if (hover != null && dragging == null && resizing == null) {
            Placed p = find(hover);
            if (p != null) {
                String tip = hover.name + "  " + Math.round(hover.scale() * 100) + "%";
                int tw = font.width(tip) + 6;
                int tx = (int) Math.max(2, Math.min(width - tw - 2, p.rect().left()));
                int ty = (int) (p.rect().top() >= 14 ? p.rect().top() - 13 : p.rect().bottom() + 2);
                g.fill(tx, ty, tx + tw, ty + 12, 0xE0101014);
                g.drawString(font, tip, tx + 3, ty + 2, 0xFFFFFFFF, false);
            }
        }

        String hint = "Drag to move · Scroll or drag the corner to resize · Right-click to hide · Shift: no snapping";
        g.drawString(font, hint, (width - font.width(hint)) / 2, height / 2 - 4 - 30, 0xB0FFFFFF, true);

        layoutButtons();
        for (Btn b : buttons) button(g, b, b.hit(mouseX, mouseY) && !panelOpen, false);
        if (panelOpen) drawPanel(g, mouseX, mouseY);
    }

    private void layoutButtons() {
        buttons.clear();
        HudConfig cfg = LumosHud.config();
        boolean lunar = cfg.theme().lunar();
        String[] labels = {"Modules", "Style: " + (lunar ? "Lunar" : "Minimal"), "Snapping: " + (cfg.snapping() ? "On" : "Off"), "Reset", "Done"};
        Runnable[] actions = {
            () -> panelOpen = !panelOpen,
            () -> cfg.setTheme("style", lunar ? "minimal" : "lunar"),
            () -> cfg.setSnapping(!cfg.snapping()),
            () -> { cfg.resetModules(); selected = null; },
            this::onClose,
        };
        int bw = 80, gap = 5, total = labels.length * bw + (labels.length - 1) * gap;
        int x = (width - total) / 2, y = height / 2 - 10;
        for (int i = 0; i < labels.length; i++) {
            buttons.add(new Btn(x + i * (bw + gap), y, bw, 20, labels[i], actions[i]));
        }
    }

    private void drawPanel(GuiGraphics g, int mouseX, int mouseY) {
        cells.clear();
        List<HudModule> all = LumosHud.modules();
        int cols = 3, cw = 122, ch = 18, gap = 4;
        int rows = (all.size() + cols - 1) / cols;
        int pw = cols * cw + (cols - 1) * gap + 16, ph = rows * ch + (rows - 1) * gap + 34;
        int px = (width - pw) / 2, py = Math.max(4, (height - ph) / 2);
        Draw.panel(g, px, py, pw, ph, 4, 0xF0101014);
        g.drawString(font, "Modules — click to switch on or off; the square switches the icon", px + 8, py + 8, 0xFFFFFFFF, false);
        for (int i = 0; i < all.size(); i++) {
            HudModule m = all.get(i);
            int cx = px + 8 + (i % cols) * (cw + gap), cy = py + 24 + (i / cols) * (ch + gap);
            int nw = m.hasIcon() ? cw - ch - 2 : cw;
            Btn b = new Btn(cx, cy, nw, ch, m.name, () -> m.setEnabled(!m.enabled()));
            cells.add(b);
            button(g, b, b.hit(mouseX, mouseY), m.enabled());
            if (m.hasIcon()) {
                Btn ib = new Btn(cx + nw + 2, cy, ch, ch, "", () -> m.setIcon(!m.iconOn()), m);
                cells.add(ib);
                button(g, ib, ib.hit(mouseX, mouseY), m.iconOn());
            }
        }
    }

    private void button(GuiGraphics g, Btn b, boolean hover, boolean on) {
        int accent = accent();
        int onBg = Colors.withOpacity(accent, hover ? 94 : 80);
        int bg = on ? onBg : (hover ? 0xE0404048 : 0xD0202026);
        Draw.panel(g, b.x(), b.y(), b.w(), b.h(), 3, bg);
        if (hover) outline(g, new Rect(b.x(), b.y(), b.w(), b.h()), (accent & 0xFFFFFF) | 0x90000000);
        if (b.iconOf() != null) {
            int fg = on ? 0xFFFFFFFF : 0xFF8A8A92;
            Draw.icon(g, b.iconOf().menuIcon(), b.x() + (b.w() - Icons.SIZE) / 2, b.y() + (b.h() - Icons.SIZE) / 2, fg, on ? 0xFFFFFFFF : fg);
            return;
        }
        g.drawString(font, b.label(), b.x() + (b.w() - font.width(b.label())) / 2, b.y() + (b.h() - 8) / 2, 0xFFFFFFFF, false);
    }

    private static void outline(GuiGraphics g, Rect r, int color) {
        int l = (int) Math.round(r.left()), t = (int) Math.round(r.top());
        int rr = (int) Math.round(r.right()), b = (int) Math.round(r.bottom());
        g.fill(l, t, rr, t + 1, color);
        g.fill(l, b - 1, rr, b, color);
        g.fill(l, t, l + 1, b, color);
        g.fill(rr - 1, t, rr, b, color);
    }

    private Placed find(HudModule m) {
        for (Placed p : placed) if (p.module() == m) return p;
        return null;
    }

    private HudModule at(double x, double y) {
        for (int i = placed.size() - 1; i >= 0; i--) {
            if (placed.get(i).rect().contains(x, y)) return placed.get(i).module();
        }
        return null;
    }

    // ── input: the version's EditorScreen (src/input/*) turns Minecraft's events into these ──

    protected boolean click(double mx, double my, int button) {
        if (panelOpen) {
            for (Btn b : cells) {
                if (b.hit(mx, my)) { b.action().run(); return true; }
            }
            panelOpen = false;
            return true;
        }
        for (Btn b : buttons) {
            if (b.hit(mx, my)) { b.action().run(); return true; }
        }
        HudModule m = at(mx, my);
        selected = m;
        if (m == null) return true;
        if (button == GLFW.GLFW_MOUSE_BUTTON_RIGHT) {
            m.setEnabled(false);
            selected = null;
            return true;
        }
        if (button != GLFW.GLFW_MOUSE_BUTTON_LEFT) return true;
        Rect r = find(m).rect();
        if (mx >= r.right() - HANDLE - 1 && my >= r.bottom() - HANDLE - 1) {
            resizing = m;
        } else {
            dragging = m;
            grabX = mx - r.left();
            grabY = my - r.top();
        }
        return true;
    }

    protected boolean drag(double mx, double my, boolean shift) {
        if (dragging != null) {
            Placed p = find(dragging);
            if (p == null) return true;
            Rect moved = p.rect().at(mx - grabX, my - grabY);
            double l = moved.left(), t = moved.top();
            if (LumosHud.config().snapping() && !shift) {
                List<Rect> others = new ArrayList<>();
                for (Placed o : placed) if (o.module() != dragging) others.add(o.rect());
                Layout.Snap s = Layout.snap(moved, others, width, height, SNAP);
                l = s.left();
                t = s.top();
                guidesX = s.guidesX();
                guidesY = s.guidesY();
            } else {
                guidesX = guidesY = List.of();
            }
            moveTo(dragging, p, l, t, dragging.scale());
            return true;
        }
        if (resizing != null) {
            Placed p = find(resizing);
            if (p == null || p.w() <= 0) return true;
            double scale = Layout.clampScale((mx - p.rect().left()) / p.w());
            moveTo(resizing, p, p.rect().left(), p.rect().top(), scale);
        }
        return true;
    }

    protected boolean release() {
        dragging = null;
        resizing = null;
        guidesX = guidesY = List.of();
        return true;
    }

    protected boolean key(int key, boolean shift) {
        if (key == GLFW.GLFW_KEY_ESCAPE || LumosHud.matchesEditorKey(key)) {
            onClose();
            return true;
        }
        Placed p = selected == null ? null : find(selected);
        if (p != null) {
            int step = shift ? 10 : 1;
            double l = p.rect().left(), t = p.rect().top();
            switch (key) {
                case GLFW.GLFW_KEY_LEFT -> l -= step;
                case GLFW.GLFW_KEY_RIGHT -> l += step;
                case GLFW.GLFW_KEY_UP -> t -= step;
                case GLFW.GLFW_KEY_DOWN -> t += step;
                case GLFW.GLFW_KEY_DELETE, GLFW.GLFW_KEY_BACKSPACE -> { selected.setEnabled(false); selected = null; return true; }
                default -> { return true; }
            }
            moveTo(selected, p, l, t, selected.scale());
        }
        return true;
    }

    /** Puts a module's top-left at (left, top) at a scale, picking the nearest anchor. */
    private void moveTo(HudModule m, Placed p, double left, double top, double scale) {
        m.setScale(scale);
        double s = m.scale();
        Layout.Placement pl = Layout.fromAbsolute(left, top, p.w() * s, p.h() * s, width, height);
        m.place(pl.anchor(), pl.x(), pl.y());
    }

    @Override
    public boolean mouseScrolled(double mx, double my, double h, double v) {
        if (panelOpen) return true;
        HudModule m = at(mx, my);
        Placed p = m == null ? null : find(m);
        if (p != null && v != 0) {
            moveTo(m, p, p.rect().left(), p.rect().top(), m.scale() + (v > 0 ? 0.05 : -0.05));
            selected = m;
        }
        return true;
    }

    @Override
    public void onClose() {
        LumosHud.config().save();
        Minecraft.getInstance().setScreen(null);
    }

    @Override
    public void removed() {
        LumosHud.config().save();
    }
}
