fn lumos_hud_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    crate::game::lumos_hud_glue::on_list(mods);
}
