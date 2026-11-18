fn lumos_freelook_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    crate::game::lumos_freelook_glue::on_list(mods);
}
