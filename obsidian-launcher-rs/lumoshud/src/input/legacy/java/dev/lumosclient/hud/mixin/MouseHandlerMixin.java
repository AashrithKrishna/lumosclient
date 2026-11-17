package dev.lumosclient.hud.mixin;

import dev.lumosclient.hud.LumosHud;
import net.minecraft.client.MouseHandler;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Every mouse press, for clicks per second (1.21.6 – 1.21.8: onPress(window, button, action, mods)). */
@Mixin(MouseHandler.class)
public abstract class MouseHandlerMixin {
    @Inject(method = "onPress(JIII)V", at = @At("HEAD"))
    private void lumoshud$button(long window, int button, int action, int mods, CallbackInfo ci) {
        LumosHud.onMouseButton(button, action);
    }
}
