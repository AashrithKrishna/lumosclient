fn lumos_paginated_advancements_all_tabs(value: usize) -> usize {
    // Keep every tab (vanilla keeps `value`, 26); the view's result then cuts them to a page,
    // or to vanilla's 26 with the mod off.
    let _vanilla = value;
    usize::MAX
}
