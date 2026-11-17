fn lumos_paginated_advancements_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    crate::game::lumos_paginated_advancements_glue::on_list(mods);
}
