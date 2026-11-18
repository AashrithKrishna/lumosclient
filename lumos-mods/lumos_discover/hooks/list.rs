fn lumos_discover_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    crate::game::lumos_discover_glue::on_list(mods);
}
