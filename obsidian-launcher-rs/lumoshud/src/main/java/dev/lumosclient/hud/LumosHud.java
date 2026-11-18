package dev.lumosclient.hud;

import com.mojang.blaze3d.platform.InputConstants;
import dev.lumosclient.hud.core.ClickCounter;
import dev.lumosclient.hud.core.Layout;
import dev.lumosclient.hud.core.Rect;
import dev.lumosclient.hud.module.HudModule;
import dev.lumosclient.hud.module.Modules;
import java.util.ArrayList;
import java.util.List;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.phys.EntityHitResult;
import net.minecraft.world.phys.Vec3;
import org.lwjgl.glfw.GLFW;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/** LumosHUD's state: the config, the modules, the numbers they show, and drawing them. */
public final class LumosHud {
    public static final Logger LOG = LoggerFactory.getLogger("LumosHUD");
    private LumosHud() {}

    private static HudConfig config;
    private static List<HudModule> modules;

    public static synchronized HudConfig config() {
        if (config == null) {
            config = new HudConfig(FabricLoader.getInstance().getConfigDir().resolve("lumoshud.json"));
            config.load();
            LOG.info("[LumosHUD] Ready; settings in {}", config.path());
        }
        return config;
    }

    public static synchronized List<HudModule> modules() {
        if (modules == null) modules = Modules.create();
        return modules;
    }

    // ── what the modules measure ──

    private static final ClickCounter LEFT = new ClickCounter(), RIGHT = new ClickCounter();
    private static double lastReach = -1;
    private static long lastReachMs;
    private static int combo;
    private static long lastComboMs;
    private static Entity pendingTarget;
    private static long pendingTick;
    private static int prevHurtTime;
    private static long ticks;
    private static Vec3 prevPos;
    private static double speed;
    private static boolean editorKeyWasDown;

    public static int leftCps(long now) { return LEFT.perSecond(now); }
    public static int rightCps(long now) { return RIGHT.perSecond(now); }
    public static double speed() { return speed; }

    /** The last hit's distance, or -1 once {@code holdMs} have passed. */
    public static double reach(long now, long holdMs) {
        return lastReach >= 0 && now - lastReachMs <= holdMs ? lastReach : -1;
    }

    /** Hits in a row that landed, until this player is hurt or 3 seconds pass. */
    public static int combo(long now) {
        return now - lastComboMs > 3000 ? 0 : combo;
    }

    /** MouseHandler.onButton: counts presses while playing (not in menus). */
    public static void onMouseButton(int button, int action) {
        Minecraft mc = Minecraft.getInstance();
        if (action != GLFW.GLFW_PRESS || mc.screen != null) return;
        long now = System.currentTimeMillis();
        if (button == GLFW.GLFW_MOUSE_BUTTON_LEFT) LEFT.click(now);
        else if (button == GLFW.GLFW_MOUSE_BUTTON_RIGHT) RIGHT.click(now);
    }

    /** MultiPlayerGameMode.attack: the reach of the hit, and a candidate for the combo. */
    public static void onAttack(Player player, Entity target) {
        Minecraft mc = Minecraft.getInstance();
        if (player != mc.player || target == null) return;
        Vec3 eye = player.getEyePosition();
        double d;
        if (mc.hitResult instanceof EntityHitResult hit && hit.getEntity() == target) {
            d = eye.distanceTo(hit.getLocation());
        } else {
            d = Math.sqrt(target.getBoundingBox().distanceToSqr(eye));
        }
        lastReach = d;
        lastReachMs = System.currentTimeMillis();
        pendingTarget = target;
        pendingTick = ticks;
    }

    /** Minecraft.tick, 20 times a second. */
    public static void onTick(Minecraft mc) {
        ticks++;
        if (ticks % 40 == 0 && !(mc.screen instanceof HudEditorScreen)) config().reloadIfChanged();
        pollEditorKey(mc);
        Player p = mc.player;
        if (p == null) {
            prevPos = null;
            speed = 0;
            return;
        }
        long now = System.currentTimeMillis();
        // Combo: a hit counts once the server shows the target hurt; getting hurt ends it.
        if (p.hurtTime > 0 && prevHurtTime == 0) combo = 0;
        prevHurtTime = p.hurtTime;
        if (pendingTarget != null) {
            if (pendingTarget instanceof LivingEntity le && le.hurtTime >= 9) {
                combo = now - lastComboMs > 3000 ? 1 : combo + 1;
                lastComboMs = now;
                pendingTarget = null;
            } else if (ticks - pendingTick > 10 || pendingTarget.isRemoved()) {
                pendingTarget = null;
            }
        }
        Vec3 pos = p.position();
        if (prevPos != null) {
            double dx = pos.x - prevPos.x, dz = pos.z - prevPos.z;
            double inst = Math.sqrt(dx * dx + dz * dz) * 20;
            speed = inst > 100 ? 0 : speed * 0.5 + inst * 0.5;
            if (speed < 0.005) speed = 0;
        }
        prevPos = pos;
    }

    private static void pollEditorKey(Minecraft mc) {
        boolean down = isDown(mc, config().editorKey());
        if (down && !editorKeyWasDown && mc.screen == null && mc.level != null) {
            mc.setScreen(new dev.lumosclient.hud.compat.EditorScreen());
        }
        editorKeyWasDown = down;
    }

    /** Whether a Minecraft key name ("key.keyboard.right.shift", "key.mouse.4") is held. */
    public static boolean isDown(Minecraft mc, String name) {
        try {
            InputConstants.Key key = InputConstants.getKey(name);
            long win = dev.lumosclient.hud.compat.Compat.window(mc);
            int code = key.getValue();
            if (code < 0 || win == 0) return false;
            if (key.getType() == InputConstants.Type.MOUSE) return GLFW.glfwGetMouseButton(win, code) == GLFW.GLFW_PRESS;
            if (key.getType() == InputConstants.Type.KEYSYM) return GLFW.glfwGetKey(win, code) == GLFW.GLFW_PRESS;
        } catch (Exception ignored) {
        }
        return false;
    }

    public static boolean matchesEditorKey(int glfwKey) {
        try {
            InputConstants.Key key = InputConstants.getKey(config().editorKey());
            return key.getType() == InputConstants.Type.KEYSYM && key.getValue() == glfwKey;
        } catch (Exception e) {
            return false;
        }
    }

    public static boolean hidesVanillaEffects() {
        for (HudModule m : modules()) {
            if (m.id.equals("potions")) return m.enabled() && m.bool("hideVanilla", true);
        }
        return false;
    }

    // ── drawing ──

    /** A module where it is drawn: rect is scaled screen pixels; w/h its unscaled size. */
    public record Placed(HudModule module, Rect rect, int w, int h) {}

    public static Placed place(HudModule m, HudModule.Ctx c, int screenW, int screenH) {
        int[] size = m.size(c);
        double s = m.scale();
        Rect r = Layout.place(m.anchor(), m.x(), m.y(), size[0] * s, size[1] * s, screenW, screenH);
        return new Placed(m, r, size[0], size[1]);
    }

    /** The enabled modules with something to show (all enabled ones in the editor). */
    public static List<Placed> layout(HudModule.Ctx c) {
        int sw = c.g().guiWidth(), sh = c.g().guiHeight();
        List<Placed> out = new ArrayList<>();
        for (HudModule m : modules()) {
            if (!m.enabled() || !m.visible(c)) continue;
            out.add(place(m, c, sw, sh));
        }
        return out;
    }

    public static void draw(HudModule.Ctx c, Placed p) {
        var pose = c.g().pose();
        pose.pushMatrix();
        pose.translate((float) p.rect().left(), (float) p.rect().top());
        float s = (float) p.module().scale();
        pose.scale(s, s);
        p.module().draw(c, p.w(), p.h());
        pose.popMatrix();
    }

    private static boolean reportedError;

    /** Gui.render: the HUD over the game (the editor draws its own). */
    public static void renderHud(GuiGraphics g) {
        Minecraft mc = Minecraft.getInstance();
        if (mc.options.hideGui || mc.player == null || mc.screen instanceof HudEditorScreen) return;
        // No "hide under F3" check: since 1.21.9 the debug overlay also reports itself shown for
        // F3+B hitboxes and other debug entries, which hid every module while hitboxes were on.
        try {
            HudModule.Ctx c = new HudModule.Ctx(mc, g, mc.font, false, System.currentTimeMillis());
            for (Placed p : layout(c)) draw(c, p);
        } catch (Throwable t) {
            if (!reportedError) {
                reportedError = true;
                LOG.error("[LumosHUD] Drawing failed", t);
            }
        }
    }
}
