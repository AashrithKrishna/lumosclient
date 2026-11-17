package dev.lumosclient.hud.compat;

import dev.lumosclient.hud.HudEditorScreen;

/** The HUD editor on 1.21.6 – 1.21.8, whose screens get input as plain numbers. */
public final class EditorScreen extends HudEditorScreen {
    @Override
    public boolean mouseClicked(double x, double y, int button) {
        return click(x, y, button);
    }

    @Override
    public boolean mouseDragged(double x, double y, int button, double dx, double dy) {
        return drag(x, y, hasShiftDown());
    }

    @Override
    public boolean mouseReleased(double x, double y, int button) {
        return release();
    }

    @Override
    public boolean keyPressed(int key, int scancode, int modifiers) {
        return key(key, hasShiftDown());
    }
}
