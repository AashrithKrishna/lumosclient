package dev.lumosclient.hud.compat;

import dev.lumosclient.hud.HudEditorScreen;
import net.minecraft.client.input.KeyEvent;
import net.minecraft.client.input.MouseButtonEvent;

/** The HUD editor on 1.21.9 and later, whose screens get input as event records. */
public final class EditorScreen extends HudEditorScreen {
    @Override
    public boolean mouseClicked(MouseButtonEvent e, boolean doubleClick) {
        return click(e.x(), e.y(), e.button());
    }

    @Override
    public boolean mouseDragged(MouseButtonEvent e, double dx, double dy) {
        return drag(e.x(), e.y(), e.hasShiftDown());
    }

    @Override
    public boolean mouseReleased(MouseButtonEvent e) {
        return release();
    }

    @Override
    public boolean keyPressed(KeyEvent e) {
        return key(e.key(), e.hasShiftDown());
    }
}
