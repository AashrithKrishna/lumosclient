package dev.lumosclient.hud;

import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Coerce;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(targets = "giq", remap = false)
public class LumosHudMixin {
    @Inject(method = "a(Lgir;Lgez;)V", at = @At("TAIL"), remap = false)
    private void lumos$onRenderHud(@Coerce Object drawContext, @Coerce Object tickCounter, CallbackInfo ci) {
        LumosHudRenderer.render(drawContext);
    }
}
