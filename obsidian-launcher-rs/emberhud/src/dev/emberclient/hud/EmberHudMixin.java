package dev.emberclient.hud;

import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Coerce;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(targets = "giq", remap = false)
public class EmberHudMixin {
    @Inject(method = "a(Lgir;Lgez;)V", at = @At("TAIL"), remap = false)
    private void emberclient$onRenderHud(@Coerce Object drawContext, @Coerce Object tickCounter, CallbackInfo ci) {
        EmberHudRenderer.render(drawContext);
    }
}
