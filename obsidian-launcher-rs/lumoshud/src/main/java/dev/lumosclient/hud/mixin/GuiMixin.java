package dev.lumosclient.hud.mixin;

import dev.lumosclient.hud.LumosHud;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.gui.Gui;
import net.minecraft.client.gui.GuiGraphics;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(Gui.class)
public abstract class GuiMixin {
    /** Drawn just before chat, so chat and the player list stay on top of the modules. */
    @Inject(
        method = "render(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/client/DeltaTracker;)V",
        at = @At(value = "INVOKE", target = "Lnet/minecraft/client/gui/Gui;renderChat(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/client/DeltaTracker;)V"))
    private void lumoshud$render(GuiGraphics g, DeltaTracker delta, CallbackInfo ci) {
        LumosHud.renderHud(g);
    }

    /** The Potion Effects module replaces vanilla's effect icons (its "hideVanilla" option). */
    @Inject(method = "renderEffects(Lnet/minecraft/client/gui/GuiGraphics;Lnet/minecraft/client/DeltaTracker;)V", at = @At("HEAD"), cancellable = true)
    private void lumoshud$effects(GuiGraphics g, DeltaTracker delta, CallbackInfo ci) {
        if (LumosHud.hidesVanillaEffects()) ci.cancel();
    }
}
