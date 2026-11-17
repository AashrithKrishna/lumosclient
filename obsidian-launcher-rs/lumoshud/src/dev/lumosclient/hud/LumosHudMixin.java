package dev.lumosclient.hud;

import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Coerce;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Draws LumosHUD after the vanilla HUD. Fabric runs the game with its "intermediary" names
 * (net.minecraft.class_329 = InGameHud, method_1753 = render), which stay the same across
 * versions; Mojang's short obfuscated names change every release and don't exist at runtime.
 */
@Mixin(targets = "net.minecraft.class_329", remap = false)
public class LumosHudMixin {
    // 1.21 and later: render(DrawContext, RenderTickCounter).
    @Inject(method = "method_1753(Lnet/minecraft/class_332;Lnet/minecraft/class_9779;)V", at = @At("TAIL"), remap = false, require = 0)
    private void lumos$onRenderHud(@Coerce Object drawContext, @Coerce Object tickCounter, CallbackInfo ci) {
        LumosHudRenderer.render(drawContext);
    }

    // 1.20.x: render(DrawContext, float tickDelta).
    @Inject(method = "method_1753(Lnet/minecraft/class_332;F)V", at = @At("TAIL"), remap = false, require = 0)
    private void lumos$onRenderHudLegacy(@Coerce Object drawContext, float tickDelta, CallbackInfo ci) {
        LumosHudRenderer.render(drawContext);
    }
}
