package dev.lumosclient.hud.module;

import static dev.lumosclient.hud.core.Anchor.*;

import dev.lumosclient.hud.LumosHud;
import dev.lumosclient.hud.core.Anchor;
import dev.lumosclient.hud.core.Colors;
import dev.lumosclient.hud.core.Fmt;
import dev.lumosclient.hud.core.Icons;
import dev.lumosclient.hud.core.Shapes;
import java.time.LocalTime;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.gui.Gui;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.client.renderer.RenderPipelines;
import net.minecraft.client.resources.language.I18n;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.ProjectileWeaponItem;

/** Every module, in drawing order. Ids are the keys of lumoshud.json's "modules". */
public final class Modules {
    private Modules() {}

    public static List<HudModule> create() {
        List<HudModule> m = new ArrayList<>();
        m.add(new Fps());
        m.add(new Cps());
        m.add(new Coordinates());
        m.add(new Keystrokes());
        m.add(new Direction());
        m.add(new Biome());
        m.add(new Armor());
        m.add(new Potions());
        m.add(new Ping());
        m.add(new Clock());
        m.add(new Server());
        m.add(new Day());
        m.add(new Speed());
        m.add(new Reach());
        m.add(new Combo());
        m.add(new ItemCounter());
        m.add(new Memory());
        m.add(new Sprint());
        return m;
    }

    // ── one-line modules ──

    static final class Fps extends TextModule {
        Fps() { super("fps", "FPS", "FPS", Icons.FPS, true, TOP_LEFT, 4, 4); labelAfter(); }
        @Override protected String value(Ctx c) { return Integer.toString(c.mc().getFps()); }
        @Override protected String sample() { return "144"; }
    }

    static final class Cps extends TextModule {
        Cps() {
            super("cps", "CPS", "CPS", Icons.CPS, true, TOP_LEFT, 4, 22);
            labelAfter();
            option("showRight", true);
        }
        @Override protected String value(Ctx c) {
            int l = LumosHud.leftCps(c.now()), r = LumosHud.rightCps(c.now());
            return bool("showRight", true) ? l + " | " + r : Integer.toString(l);
        }
        @Override protected String sample() { return "0 | 0"; }
    }

    static final class Coordinates extends HudModule {
        Coordinates() {
            super("coordinates", "Coordinates", true, TOP_LEFT, 4, 40);
            option("vertical", false);
            option("decimals", 0);
            option("showLabel", true);
        }

        @Override public char[][] menuIcon() { return Icons.COORDS; }

        /** {x, y, z} as text, or null. */
        private String[] xyz(Ctx c) {
            Player p = c.mc().player;
            if (p == null || c.mc().showOnlyReducedInfo()) return c.preview() ? fmt(128, 64, -256) : null;
            return fmt(p.getX(), p.getY(), p.getZ());
        }

        private String[] fmt(double x, double y, double z) {
            int d = (int) num("decimals", 0);
            return new String[] {one(x, d), one(y, d), one(z, d)};
        }

        private static String one(double v, int d) {
            return d == 0 ? Long.toString((long) Math.floor(v)) : Fmt.decimals(v, d);
        }

        /** Minimal style: "XYZ: 1, 2, 3" or "X: 1" lines. */
        private List<String> lines(Ctx c) {
            String[] v = xyz(c);
            if (v == null) return null;
            boolean label = bool("showLabel", true);
            if (bool("vertical", false)) {
                return label ? List.of("X: " + v[0], "Y: " + v[1], "Z: " + v[2]) : List.of(v[0], v[1], v[2]);
            }
            return List.of((label ? "XYZ: " : "") + v[0] + ", " + v[1] + ", " + v[2]);
        }

        /** Lunar style: rows of {dimmed label or null, value}: "XYZ 1 2 3", or "X 1" / "Y 2" / "Z 3". */
        private String[][] rows(Ctx c) {
            String[] v = xyz(c);
            if (v == null) return null;
            boolean label = bool("showLabel", true);
            if (bool("vertical", false)) {
                String[][] r = new String[3][];
                String[] names = {"X", "Y", "Z"};
                for (int i = 0; i < 3; i++) r[i] = new String[] {label ? names[i] : null, v[i]};
                return r;
            }
            return new String[][] {{label ? "XYZ" : null, v[0] + " " + v[1] + " " + v[2]}};
        }

        private int labelW(Ctx c, String[][] rows) {
            int w = 0;
            for (String[] r : rows) if (r[0] != null) w = Math.max(w, c.font().width(r[0] + " "));
            return w;
        }

        @Override public boolean visible(Ctx c) { return xyz(c) != null; }

        @Override public int[] size(Ctx c) {
            if (!lunar()) return Lines.size(c, lines(c));
            String[][] rows = rows(c);
            if (rows == null) return new int[] {40, TextModule.L_H};
            int lw = labelW(c, rows), tw = 0;
            for (String[] r : rows) tw = Math.max(tw, lw + c.font().width(r[1]));
            int iw = showIcon() ? Icons.SIZE + TextModule.L_GAP : 0;
            int h = rows.length == 1 ? TextModule.L_H : rows.length * (c.font().lineHeight + 1) - 1 + 9;
            return new int[] {TextModule.L_PAD * 2 + iw + tw, h};
        }

        @Override public void draw(Ctx c, int w, int h) {
            if (!lunar()) { Lines.draw(this, c, lines(c), w, h, bool("vertical", false)); return; }
            String[][] rows = rows(c);
            if (rows == null) return;
            box(c, w, h);
            int x = TextModule.L_PAD;
            if (showIcon()) {
                icon(c, Icons.COORDS, x, (h - Icons.SIZE) / 2);
                x += Icons.SIZE + TextModule.L_GAP;
            }
            int lw = labelW(c, rows);
            int y = rows.length == 1 ? (h - 7) / 2 : 5;
            for (String[] r : rows) {
                if (r[0] != null) text(c, r[0], x, y, dimColor());
                text(c, r[1], x + lw, y, textColor());
                y += c.font().lineHeight + 1;
            }
        }
    }

    static final class Direction extends TextModule {
        Direction() {
            super("direction", "Direction", "Facing", Icons.compass(180), false, MIDDLE_LEFT, 4, -36);
            option("showAxis", true);
            option("showDegrees", false);
        }
        @Override protected String value(Ctx c) {
            Player p = c.mc().player;
            if (p == null) return null;
            String s = Fmt.facing(p.getYRot(), bool("showAxis", true));
            if (bool("showDegrees", false)) {
                float yaw = ((p.getYRot() % 360f) + 360f) % 360f;
                s += " " + Fmt.decimals(yaw, 1) + "°";
            }
            return s;
        }
        @Override protected String sample() { return "North (-Z)"; }
        @Override protected char[][] icon(Ctx c) {
            Player p = c.mc().player;
            return Icons.compass(p == null ? 180 : p.getYRot());
        }
    }

    static final class Biome extends TextModule {
        Biome() { super("biome", "Biome", "Biome", Icons.BIOME, false, MIDDLE_LEFT, 4, -18); }
        @Override protected String value(Ctx c) {
            Player p = c.mc().player;
            if (p == null || c.mc().level == null) return null;
            String id = c.mc().level.getBiome(p.blockPosition()).getRegisteredName();
            String key = "biome." + id.replace(':', '.');
            return I18n.exists(key) ? I18n.get(key) : Fmt.prettyId(id);
        }
        @Override protected String sample() { return "Plains"; }
    }

    static final class Ping extends TextModule {
        Ping() { super("ping", "Ping", "Ping", Icons.signal(42), false, BOTTOM_RIGHT, -4, -4); }
        private static int latency(Ctx c) {
            Player p = c.mc().player;
            if (p == null || c.mc().isSingleplayer() || c.mc().getConnection() == null) return -1;
            PlayerInfo info = c.mc().getConnection().getPlayerInfo(p.getUUID());
            return info == null ? -1 : Math.max(0, info.getLatency());
        }
        @Override protected String value(Ctx c) {
            int ms = latency(c);
            return ms < 0 ? null : ms + " ms";
        }
        @Override protected String sample() { return "42 ms"; }
        @Override protected char[][] icon(Ctx c) {
            int ms = latency(c);
            return Icons.signal(ms < 0 ? 42 : ms);
        }
    }

    static final class Clock extends TextModule {
        private static final DateTimeFormatter H24 = DateTimeFormatter.ofPattern("HH:mm", Locale.ROOT);
        private static final DateTimeFormatter H24S = DateTimeFormatter.ofPattern("HH:mm:ss", Locale.ROOT);
        private static final DateTimeFormatter H12 = DateTimeFormatter.ofPattern("h:mm a", Locale.ROOT);
        private static final DateTimeFormatter H12S = DateTimeFormatter.ofPattern("h:mm:ss a", Locale.ROOT);
        Clock() {
            super("clock", "Clock", null, Icons.CLOCK, false, TOP_CENTER, 0, 4);
            option("format24h", true);
            option("showSeconds", false);
        }
        @Override protected String value(Ctx c) {
            boolean h24 = bool("format24h", true), secs = bool("showSeconds", false);
            return LocalTime.now().format(h24 ? (secs ? H24S : H24) : (secs ? H12S : H12));
        }
        @Override protected String sample() { return "12:00"; }
    }

    static final class Server extends TextModule {
        Server() { super("server", "Server Address", null, Icons.SERVER, false, BOTTOM_RIGHT, -4, -22); }
        @Override protected String value(Ctx c) {
            var s = c.mc().getCurrentServer();
            return s == null || c.mc().isSingleplayer() ? null : s.ip;
        }
        @Override protected String sample() { return "play.example.net"; }
    }

    static final class Day extends TextModule {
        Day() { super("day", "Day Counter", "Day", Icons.SUN, false, BOTTOM_RIGHT, -4, -40); }
        @Override protected char[][] icon(Ctx c) {
            if (c.mc().level == null) return Icons.SUN;
            long t = c.mc().level.getDayTime() % 24000L;
            return t < 12500 || t > 23500 ? Icons.SUN : Icons.MOON;
        }
        @Override protected String value(Ctx c) {
            return c.mc().level == null ? null : Long.toString(c.mc().level.getDayTime() / 24000L + 1);
        }
        @Override protected String sample() { return "12"; }
    }

    static final class Speed extends TextModule {
        Speed() {
            super("speed", "Speed", "Speed", Icons.SPEED, false, MIDDLE_LEFT, 4, 0);
            option("decimals", 2);
        }
        @Override protected String value(Ctx c) {
            return c.mc().player == null ? null : Fmt.decimals(LumosHud.speed(), (int) num("decimals", 2)) + " m/s";
        }
        @Override protected String sample() { return "5.61 m/s"; }
    }

    static final class Reach extends TextModule {
        Reach() {
            super("reach", "Reach Display", "Reach", Icons.REACH, false, MIDDLE_LEFT, 4, 18);
            option("decimals", 2);
            option("holdSeconds", 3);
        }
        @Override protected String value(Ctx c) {
            double r = LumosHud.reach(c.now(), (long) (num("holdSeconds", 3) * 1000));
            return r < 0 ? null : Fmt.decimals(r, (int) num("decimals", 2)) + " blocks";
        }
        @Override protected String sample() { return "3.00 blocks"; }
    }

    static final class Combo extends TextModule {
        Combo() {
            super("combo", "Combo Counter", null, Icons.COMBO, false, MIDDLE_LEFT, 4, 36);
            option("showZero", true);
        }
        @Override protected String value(Ctx c) {
            int n = LumosHud.combo(c.now());
            if (n == 0) return bool("showZero", true) ? "No Combo" : null;
            return n + " Combo";
        }
        @Override protected String sample() { return "3 Combo"; }
    }

    static final class Memory extends TextModule {
        Memory() {
            super("memory", "Memory", "Mem", Icons.MEMORY, false, BOTTOM_RIGHT, -4, -58);
            option("showBytes", true);
        }
        @Override protected String value(Ctx c) {
            Runtime rt = Runtime.getRuntime();
            long max = rt.maxMemory(), used = rt.totalMemory() - rt.freeMemory();
            String s = (used * 100 / Math.max(1, max)) + "%";
            if (bool("showBytes", true)) s += " " + (used >> 20) + "/" + (max >> 20) + " MB";
            return s;
        }
        @Override protected String sample() { return "40%"; }
    }

    static final class Sprint extends TextModule {
        Sprint() {
            super("sprint", "Toggle Sprint", null, Icons.SPRINT, false, BOTTOM_LEFT, 4, -48);
            option("showSneak", true);
            noBackgroundByDefault();
        }
        @Override protected String value(Ctx c) {
            Player p = c.mc().player;
            if (p == null) return null;
            var o = c.mc().options;
            if (bool("showSneak", true) && p.isShiftKeyDown()) {
                return o.toggleCrouch().get() ? "[Sneaking (Toggled)]" : "[Sneaking (Key Held)]";
            }
            if (p.isSprinting() || o.keySprint.isDown()) {
                if (o.toggleSprint().get() && o.keySprint.isDown()) return "[Sprinting (Toggled)]";
                return o.keySprint.isDown() ? "[Sprinting (Key Held)]" : "[Sprinting (Vanilla)]";
            }
            return null;
        }
        @Override protected String sample() { return "[Sprinting (Toggled)]"; }
        @Override protected String lunarValue(String v) {
            return v.startsWith("[") && v.endsWith("]") ? v.substring(1, v.length() - 1) : v;
        }
        @Override protected char[][] icon(Ctx c) {
            Player p = c.mc().player;
            return p != null && bool("showSneak", true) && p.isShiftKeyDown() ? Icons.SNEAK : Icons.SPRINT;
        }
    }

    // ── multi-line helpers ──

    static final class Lines {
        static int[] size(HudModule.Ctx c, List<String> lines) {
            if (lines == null || lines.isEmpty()) return new int[] {40, c.font().lineHeight - 1 + HudModule.PAD_Y * 2};
            int w = 0;
            for (String s : lines) w = Math.max(w, c.font().width(s));
            int h = lines.size() * (c.font().lineHeight + 1) - 2 + HudModule.PAD_Y * 2;
            return new int[] {Math.max(28, w + HudModule.PAD_X * 2), h};
        }

        static void draw(HudModule m, HudModule.Ctx c, List<String> lines, int w, int h, boolean leftAligned) {
            if (lines == null) return;
            m.box(c, w, h);
            int y = HudModule.PAD_Y;
            for (String s : lines) {
                int x = leftAligned ? HudModule.PAD_X : (w - c.font().width(s)) / 2;
                m.text(c, s, x, y, m.textColor());
                y += c.font().lineHeight + 1;
            }
        }
    }

    // ── keystrokes ──

    static final class Keystrokes extends HudModule {
        private static final int K = 22, GAP = 2, W = K * 3 + GAP * 2;
        /** Lunar style: a pressed key lights up at once and fades back over this long. */
        private static final float RISE_MS = 35, FALL_MS = 200;
        private final Map<String, float[]> glow = new HashMap<>();
        private long lastFrame;
        private long dt;

        Keystrokes() {
            super("keystrokes", "Keystrokes", true, TOP_LEFT, 4, 58);
            option("showMouse", true);
            option("showSpace", true);
            option("showCps", true);
            option("pressedColor", "#FFFFFF");
        }

        private int mouseH() { return bool("showCps", true) ? K : 16; }

        @Override public int[] size(Ctx c) {
            int h = K * 2 + GAP;
            if (bool("showMouse", true)) h += GAP + mouseH();
            if (bool("showSpace", true)) h += GAP + 10;
            return new int[] {W, h};
        }

        @Override public void draw(Ctx c, int w, int h) {
            dt = lastFrame == 0 ? 0 : Math.max(0, Math.min(250, c.now() - lastFrame));
            lastFrame = c.now();
            var o = c.mc().options;
            key(c, "w", K + GAP, 0, K, K, label(o.keyUp, "W"), null, o.keyUp.isDown());
            key(c, "a", 0, K + GAP, K, K, label(o.keyLeft, "A"), null, o.keyLeft.isDown());
            key(c, "s", K + GAP, K + GAP, K, K, label(o.keyDown, "S"), null, o.keyDown.isDown());
            key(c, "d", (K + GAP) * 2, K + GAP, K, K, label(o.keyRight, "D"), null, o.keyRight.isDown());
            int y = K * 2 + GAP * 2;
            if (bool("showMouse", true)) {
                int mw = (W - GAP) / 2;
                boolean cps = bool("showCps", true);
                key(c, "lmb", 0, y, mw, mouseH(), "LMB", cps ? LumosHud.leftCps(c.now()) + " CPS" : null, o.keyAttack.isDown());
                key(c, "rmb", mw + GAP, y, W - mw - GAP, mouseH(), "RMB", cps ? LumosHud.rightCps(c.now()) + " CPS" : null, o.keyUse.isDown());
                y += mouseH() + GAP;
            }
            if (bool("showSpace", true)) {
                boolean down = o.keyJump.isDown();
                float t = fillKey(c, "space", 0, y, W, 10, down);
                int lc = lunar() ? Colors.lerp(textColor(), 0xFFFFFFFF, t) : down ? 0xFF000000 : textColor();
                c.g().fill(W / 2 - 10, y + 4, W / 2 + 10, y + 5, lc);
            }
        }

        private static String label(KeyMapping k, String fallback) {
            String s = k.getTranslatedKeyMessage().getString();
            if (s.isEmpty()) return fallback;
            return s.length() > 3 ? s.substring(0, 3) : s;
        }

        /** Draws the key's box; returns how lit it is (0..1). */
        private float fillKey(Ctx c, String id, int x, int y, int w, int h, boolean down) {
            if (!lunar()) {
                int pressed = Colors.withOpacity(Colors.parse(str("pressedColor", "#FFFFFF"), 0xFFFFFFFF), 75);
                if (down) c.g().fill(x, y, x + w, y + h, pressed);
                else if (background()) c.g().fill(x, y, x + w, y + h, backgroundColor());
                return down ? 1 : 0;
            }
            float[] g = glow.computeIfAbsent(id, k -> new float[1]);
            g[0] = Shapes.approach(g[0], down ? 1 : 0, dt, RISE_MS, FALL_MS);
            int rest = background() ? backgroundColor() : 0;
            int lit = Colors.withOpacity(theme().accent(), 85);
            int fill = Colors.lerp(rest == 0 ? lit & 0xFFFFFF : rest, lit, g[0]);
            if ((fill >>> 24) != 0) dev.lumosclient.hud.Draw.panel(c.g(), x, y, w, h, theme().radius(), fill);
            return g[0];
        }

        private void key(Ctx c, String id, int x, int y, int w, int h, String label, String sub, boolean down) {
            float t = fillKey(c, id, x, y, w, h, down);
            boolean lunar = lunar();
            int color = lunar ? Colors.lerp(textColor(), 0xFFFFFFFF, t) : down ? 0xFF000000 : textColor();
            boolean shadow = shadow() && (lunar || !down);
            var f = c.font();
            if (sub == null) {
                c.g().drawString(f, label, x + (w - f.width(label)) / 2, y + (h - 7) / 2, color, shadow);
            } else {
                c.g().pose().pushMatrix();
                c.g().pose().translate(x + w / 2f, y + 3);
                c.g().drawString(f, label, -f.width(label) / 2, 0, color, shadow);
                c.g().pose().translate(0, 10);
                c.g().pose().scale(0.75f, 0.75f);
                int subColor = lunar ? Colors.lerp(dimColor(), 0xFFFFFFFF, t) : color;
                c.g().drawString(f, sub, -f.width(sub) / 2, 0, subColor, shadow);
                c.g().pose().popMatrix();
            }
        }
    }

    // ── icon rows: armour, item counter, effects ──

    static final class Armor extends HudModule {
        private static final EquipmentSlot[] SLOTS = {EquipmentSlot.HEAD, EquipmentSlot.CHEST, EquipmentSlot.LEGS, EquipmentSlot.FEET};
        Armor() {
            super("armor", "Armor Status", true, MIDDLE_RIGHT, -4, 0);
            option("durability", "value");   // value | percent | none
            option("showMainHand", false);
            option("durabilityColors", true);
            option("horizontal", false);
            noBackgroundByDefault();
        }

        private List<ItemStack> items(Ctx c) {
            List<ItemStack> out = new ArrayList<>();
            Player p = c.mc().player;
            if (p != null) {
                for (EquipmentSlot s : SLOTS) {
                    ItemStack st = p.getItemBySlot(s);
                    if (!st.isEmpty()) out.add(st);
                }
                if (bool("showMainHand", false) && !p.getMainHandItem().isEmpty()) out.add(p.getMainHandItem());
            }
            if (out.isEmpty() && c.preview()) {
                out.add(new ItemStack(Items.DIAMOND_HELMET));
                out.add(new ItemStack(Items.DIAMOND_CHESTPLATE));
                out.add(new ItemStack(Items.DIAMOND_LEGGINGS));
                out.add(new ItemStack(Items.DIAMOND_BOOTS));
            }
            return out;
        }

        private String dur(ItemStack st) {
            String mode = str("durability", "value");
            if ("none".equals(mode) || !st.isDamageableItem()) return st.getCount() > 1 ? Integer.toString(st.getCount()) : "";
            int max = st.getMaxDamage(), left = max - st.getDamageValue();
            return "percent".equals(mode) ? (left * 100 / Math.max(1, max)) + "%" : Integer.toString(left);
        }

        private int color(ItemStack st) {
            if (bool("durabilityColors", true) && st.isDamageableItem()) return 0xFF000000 | st.getBarColor();
            return textColor();
        }

        private boolean horizontal() { return lunar() && bool("horizontal", false); }

        @Override public boolean visible(Ctx c) { return !items(c).isEmpty(); }

        @Override public int[] size(Ctx c) {
            List<ItemStack> it = items(c);
            if (horizontal()) {
                int w = 4, anyText = 0;
                for (ItemStack st : it) {
                    int tw = c.font().width(dur(st));
                    if (tw > 0) anyText = 1;
                    w += Math.max(18, tw + 2);
                }
                return new int[] {w + 2, 20 + anyText * 9};
            }
            int tw = 0;
            for (ItemStack st : it) tw = Math.max(tw, c.font().width(dur(st)));
            return new int[] {16 + (tw > 0 ? 3 + tw : 0) + (lunar() ? 7 : 4), Math.max(1, it.size()) * 17 + 3};
        }

        @Override public void draw(Ctx c, int w, int h) {
            box(c, w, h);
            boolean lunar = lunar();
            if (horizontal()) {
                int x = 3;
                for (ItemStack st : items(c)) {
                    String d = dur(st);
                    int cw = Math.max(18, c.font().width(d) + 2);
                    int ix = x + (cw - 16) / 2;
                    c.g().renderItem(st, ix, 2);
                    c.g().renderItemDecorations(c.font(), st, ix, 2, "");
                    if (!d.isEmpty()) text(c, d, x + (cw - c.font().width(d)) / 2, 19, color(st));
                    x += cw;
                }
                return;
            }
            int y = 2, ix = lunar ? 3 : 2;
            for (ItemStack st : items(c)) {
                c.g().renderItem(st, ix, y);
                if (lunar) c.g().renderItemDecorations(c.font(), st, ix, y, "");
                String d = dur(st);
                if (!d.isEmpty()) text(c, d, ix + 19, y + 4, color(st));
                y += 17;
            }
        }
    }

    static final class ItemCounter extends HudModule {
        ItemCounter() {
            super("itemcounter", "Item Counter", false, BOTTOM_CENTER, 110, -4);
            noBackgroundByDefault();
        }

        /** {shown stack, total} or null. Bows and crossbows count their ammunition. */
        private Object[] count(Ctx c) {
            Player p = c.mc().player;
            if (p != null && !p.getMainHandItem().isEmpty()) {
                ItemStack held = p.getMainHandItem();
                ItemStack target = held;
                if (held.getItem() instanceof ProjectileWeaponItem) {
                    target = p.getProjectile(held);
                    if (target.isEmpty()) return new Object[] {new ItemStack(Items.ARROW), 0};
                }
                Inventory inv = p.getInventory();
                int n = 0;
                for (int i = 0; i < inv.getContainerSize(); i++) {
                    ItemStack s = inv.getItem(i);
                    if (!s.isEmpty() && ItemStack.isSameItem(s, target)) n += s.getCount();
                }
                return new Object[] {target, n};
            }
            return c.preview() ? new Object[] {new ItemStack(Items.ARROW), 64} : null;
        }

        @Override public boolean visible(Ctx c) { return count(c) != null; }

        @Override public int[] size(Ctx c) {
            Object[] r = count(c);
            String s = r == null ? "0" : r[1].toString();
            return new int[] {16 + 3 + c.font().width(s) + (lunar() ? 8 : 4), 20};
        }

        @Override public void draw(Ctx c, int w, int h) {
            Object[] r = count(c);
            if (r == null) return;
            box(c, w, h);
            int x = lunar() ? 3 : 2;
            c.g().renderItem((ItemStack) r[0], x, 2);
            text(c, r[1].toString(), x + 19, 6, textColor());
        }
    }

    static final class Potions extends HudModule {
        private static final int ROW = 22;
        /** Lunar style: effects with less than this left blink. */
        private static final int BLINK_TICKS = 200;
        Potions() {
            super("potions", "Potion Effects", true, TOP_RIGHT, -4, 4);
            option("hideVanilla", true);
            option("effectColors", false);
        }

        @Override public char[][] menuIcon() { return Icons.POTION; }

        /** "icon", or the older "showIcons" when the file has only that. */
        @Override public boolean iconOn() {
            return json().has("icon") ? super.iconOn() : bool("showIcons", true);
        }

        private boolean icons() { return lunar() ? showIcon() : iconOn(); }

        private List<MobEffectInstance> effects(Ctx c) {
            List<MobEffectInstance> out = new ArrayList<>();
            if (c.mc().player != null) out.addAll(c.mc().player.getActiveEffects());
            if (out.isEmpty() && c.preview()) {
                out.add(new MobEffectInstance(MobEffects.SPEED, 20 * 95, 1));
                out.add(new MobEffectInstance(MobEffects.FIRE_RESISTANCE, 20 * 8, 0));
            }
            out.sort(Comparator.comparing((MobEffectInstance e) -> !e.getEffect().value().isBeneficial())
                .thenComparing(e -> -e.getDuration()));
            return out;
        }

        private static String title(MobEffectInstance e) {
            String n = e.getEffect().value().getDisplayName().getString();
            return e.getAmplifier() > 0 ? n + " " + Fmt.roman(e.getAmplifier() + 1) : n;
        }

        @Override public boolean visible(Ctx c) { return !effects(c).isEmpty(); }

        @Override public int[] size(Ctx c) {
            List<MobEffectInstance> list = effects(c);
            int tw = 0;
            for (MobEffectInstance e : list) tw = Math.max(tw, Math.max(c.font().width(title(e)), c.font().width(time(e))));
            int icon = icons() ? 20 : 0;
            if (lunar()) return new int[] {icon + tw + 10, Math.max(1, list.size()) * ROW + 4};
            return new int[] {icon + tw + PAD_X * 2, Math.max(1, list.size()) * ROW + 2};
        }

        private static String time(MobEffectInstance e) {
            return Fmt.duration(e.isInfiniteDuration() ? -1 : e.getDuration());
        }

        private static boolean ending(MobEffectInstance e) {
            return !e.isInfiniteDuration() && e.getDuration() <= BLINK_TICKS;
        }

        @Override public void draw(Ctx c, int w, int h) {
            box(c, w, h);
            boolean lunar = lunar(), icons = icons(), colors = bool("effectColors", false);
            int y = lunar ? 3 : 2;
            for (MobEffectInstance e : effects(c)) {
                int x = lunar ? 5 : PAD_X;
                boolean blink = lunar && ending(e);
                if (icons) {
                    var sprite = Gui.getMobEffectSprite(e.getEffect());
                    // Fades in and out twice a second in its last 10 seconds.
                    float a = blink ? 0.3f + 0.7f * (float) (0.5 + 0.5 * Math.cos(c.now() * Math.PI * 2 / 500.0)) : 1f;
                    c.g().blitSprite(RenderPipelines.GUI_TEXTURED, sprite, x - 2, y + 1, 18, 18, Colors.fade(0xFFFFFFFF, a));
                    x += 20;
                }
                int color = colors ? 0xFF000000 | e.getEffect().value().getColor() : textColor();
                text(c, title(e), x, y + 2, color);
                int timeColor = blink ? theme().accent() : lunar ? dimColor() : 0xFFAAAAAA;
                text(c, time(e), x, y + 12, timeColor);
                y += ROW;
            }
        }
    }
}
