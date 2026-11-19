fn lumos_world_map_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    crate::game::lumos_world_map_glue::on_list(mods);
}
