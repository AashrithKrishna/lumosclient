package dev.lumosclient.hud.mixin;

import dev.lumosclient.hud.LumosHud;
import net.minecraft.client.MouseHandler;
import net.minecraft.client.input.MouseButtonInfo;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Every mouse press, for clicks per second (counted here rather than once a frame, so none are missed). */
@Mixin(MouseHandler.class)
public abstract class MouseHandlerMixin {
    @Inject(method = "onButton(JLnet/minecraft/client/input/MouseButtonInfo;I)V", at = @At("HEAD"))
    private void lumoshud$button(long window, MouseButtonInfo info, int action, CallbackInfo ci) {
        LumosHud.onMouseButton(info.button(), action);
    }
}
