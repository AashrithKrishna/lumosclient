package dev.lumosclient.hud;

import org.lwjgl.glfw.GLFW;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayDeque;
import java.util.Deque;

public final class LumosHudRenderer {
    private static final int BG = 0xAA0B0B10;
    private static final int BG_ACTIVE = 0xCCFF5A1F;
    private static final int ACCENT = 0xFFFF5A1F;
    private static final int GREEN = 0xFF00FF87;
    private static final int TEXT = 0xFFF2F4F8;

    private static final Deque<Long> leftClicks = new ArrayDeque<>();
    private static final Deque<Long> rightClicks = new ArrayDeque<>();
    private static boolean prevLeft = false;
    private static boolean prevRight = false;

    private static boolean initAttempted = false;
    private static boolean ready = false;
    private static Method mcGetInstance;
    private static Method mcGetFps;
    private static Field mcFontField;
    private static Field mcPlayerField;
    private static Method entityGetX;
    private static Method entityGetY;
    private static Method entityGetZ;
    private static Method entityGetYaw;
    private static Method ctxFill;
    private static Method ctxDrawText;

    private LumosHudRenderer() {}

    private static void ensureInit() {
        if (initAttempted) return;
        initAttempted = true;
        try {
            Class<?> mcCls = Class.forName("gfj");
            Class<?> fontCls = Class.forName("gio");
            Class<?> ctxCls = Class.forName("gir");
            Class<?> entityCls = Class.forName("cgk");

            mcGetInstance = mcCls.getDeclaredMethod("V");
            mcGetFps = mcCls.getDeclaredMethod("s");
            mcFontField = mcCls.getDeclaredField("g");
            mcPlayerField = mcCls.getDeclaredField("s");

            entityGetX = entityCls.getDeclaredMethod("dP");
            entityGetY = entityCls.getDeclaredMethod("dR");
            entityGetZ = entityCls.getDeclaredMethod("dV");
            entityGetYaw = entityCls.getDeclaredMethod("ec");

            ctxFill = ctxCls.getDeclaredMethod("a", int.class, int.class, int.class, int.class, int.class);
            ctxDrawText = ctxCls.getDeclaredMethod("b", fontCls, String.class, int.class, int.class, int.class);
            ready = true;
        } catch (Throwable ignored) {
            ready = false;
        }
    }

    public static void render(Object ctx) {
        ensureInit();
        if (!ready || ctx == null) return;
        try {
            Object mc = mcGetInstance.invoke(null);
            if (mc == null) return;
            Object font = mcFontField.get(mc);
            Object player = mcPlayerField.get(mc);
            if (font == null || player == null) return;

            long win = GLFW.glfwGetCurrentContext();
            long now = System.currentTimeMillis();
            boolean leftDown = win != 0 && GLFW.glfwGetMouseButton(win, GLFW.GLFW_MOUSE_BUTTON_LEFT) == GLFW.GLFW_PRESS;
            boolean rightDown = win != 0 && GLFW.glfwGetMouseButton(win, GLFW.GLFW_MOUSE_BUTTON_RIGHT) == GLFW.GLFW_PRESS;
            if (leftDown && !prevLeft) leftClicks.addLast(now);
            if (rightDown && !prevRight) rightClicks.addLast(now);
            prevLeft = leftDown;
            prevRight = rightDown;
            while (!leftClicks.isEmpty() && now - leftClicks.peekFirst() > 1000) leftClicks.removeFirst();
            while (!rightClicks.isEmpty() && now - rightClicks.peekFirst() > 1000) rightClicks.removeFirst();

            int fps = (int) mcGetFps.invoke(mc);
            int x = (int) Math.floor((double) entityGetX.invoke(player));
            int y = (int) Math.floor((double) entityGetY.invoke(player));
            int z = (int) Math.floor((double) entityGetZ.invoke(player));
            String dir = facing((float) entityGetYaw.invoke(player));

            drawPill(ctx, font, 8, 8, "LUMOS", ACCENT);
            drawPill(ctx, font, 8, 26, "FPS: " + fps, GREEN);
            drawPill(ctx, font, 8, 44, "CPS: " + leftClicks.size() + " | " + rightClicks.size(), TEXT);
            drawPill(ctx, font, 8, 62, "XYZ: " + x + ", " + y + ", " + z + " (" + dir + ")", TEXT);

            if (win != 0) {
                int kx = 8;
                int ky = 84;
                drawKey(ctx, font, kx + 22, ky, 20, 20, "W", GLFW.glfwGetKey(win, GLFW.GLFW_KEY_W) == GLFW.GLFW_PRESS);
                drawKey(ctx, font, kx, ky + 22, 20, 20, "A", GLFW.glfwGetKey(win, GLFW.GLFW_KEY_A) == GLFW.GLFW_PRESS);
                drawKey(ctx, font, kx + 22, ky + 22, 20, 20, "S", GLFW.glfwGetKey(win, GLFW.GLFW_KEY_S) == GLFW.GLFW_PRESS);
                drawKey(ctx, font, kx + 44, ky + 22, 20, 20, "D", GLFW.glfwGetKey(win, GLFW.GLFW_KEY_D) == GLFW.GLFW_PRESS);
                drawKey(ctx, font, kx, ky + 44, 31, 16, "LMB", leftDown);
                drawKey(ctx, font, kx + 33, ky + 44, 31, 16, "RMB", rightDown);
                drawKey(ctx, font, kx, ky + 62, 64, 14, "SPACE", GLFW.glfwGetKey(win, GLFW.GLFW_KEY_SPACE) == GLFW.GLFW_PRESS);
            }
        } catch (Throwable ignored) {}
    }

    private static void drawPill(Object ctx, Object font, int x, int y, String text, int color) throws Throwable {
        int w = text.length() * 6 + 10;
        ctxFill.invoke(ctx, x, y, x + w, y + 15, BG);
        ctxDrawText.invoke(ctx, font, text, x + 5, y + 4, color);
    }

    private static void drawKey(Object ctx, Object font, int x, int y, int w, int h, String label, boolean pressed) throws Throwable {
        ctxFill.invoke(ctx, x, y, x + w, y + h, pressed ? BG_ACTIVE : BG);
        int tx = x + Math.max(2, (w - label.length() * 6) / 2);
        int ty = y + Math.max(2, (h - 8) / 2);
        ctxDrawText.invoke(ctx, font, label, tx, ty, pressed ? 0xFF0B0B10 : TEXT);
    }

    private static String facing(float yaw) {
        float y = ((yaw % 360f) + 360f) % 360f;
        if (y < 45f || y >= 315f) return "S";
        if (y < 135f) return "W";
        if (y < 225f) return "N";
        return "E";
    }
}
