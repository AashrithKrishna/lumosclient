package dev.lumosclient.capes.mixin;

import dev.lumosclient.capes.LumosCapes;
import net.minecraft.client.player.AbstractClientPlayer;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.player.PlayerSkin;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * Every player the game draws (you and others) gets its skin from
 * {@link AbstractClientPlayer#getSkin()}; the cape and elytra layers read their texture from
 * that skin. LumosCapes swaps in the player's Lumos cape there.
 */
@Mixin(AbstractClientPlayer.class)
public abstract class AbstractClientPlayerMixin {
    @Inject(method = "getSkin", at = @At("RETURN"), cancellable = true)
    private void lumoscapes$wearCape(CallbackInfoReturnable<PlayerSkin> cir) {
        PlayerSkin skin = cir.getReturnValue();
        PlayerSkin withCape = LumosCapes.apply(((Player) (Object) this).getGameProfile().name(), skin);
        if (withCape != skin) {
            cir.setReturnValue(withCape);
        }
    }
}
