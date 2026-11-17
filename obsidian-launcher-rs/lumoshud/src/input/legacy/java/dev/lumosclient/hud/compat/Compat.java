package dev.lumosclient.hud.compat;

import net.minecraft.client.Minecraft;

/** What differs between Minecraft versions outside the editor's input (1.21.6 – 1.21.8). */
public final class Compat {
    private Compat() {}

    public static long window(Minecraft mc) {
        return mc.getWindow().getWindow();
    }
}
