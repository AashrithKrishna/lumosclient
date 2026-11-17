package dev.lumosclient.hud.mixin;

import dev.lumosclient.hud.LumosHud;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(Minecraft.class)
public abstract class MinecraftMixin {
    @Inject(method = "tick()V", at = @At("TAIL"))
    private void lumoshud$tick(CallbackInfo ci) {
        LumosHud.onTick((Minecraft) (Object) this);
    }
}
