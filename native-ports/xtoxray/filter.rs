// While on, the mesher reads a copy of its tables: hidden states look like air
// (state 0), the others shine at full brightness.
fn xtoxray_mesh(mesher: &mut Mesher<'_>) {
    static FILTERED: std::sync::RwLock<Option<(usize, &'static MeshTables)>> =
        std::sync::RwLock::new(None);
    if !xtoxray::active() {
        return;
    }
    let source = mesher.tables as *const MeshTables as usize;
    let known = *FILTERED.read().unwrap();
    if let Some((from, tables)) = known
        && from == source
    {
        mesher.tables = tables;
        return;
    }
    let from = mesher.tables;
    let air = from.info[0];
    let mut info = Vec::with_capacity(from.info.len());
    for (id, state) in from.info.iter().enumerate() {
        if xtoxray::state_visible(id) {
            let mut state = *state;
            state.emissive = true;
            info.push(state);
        } else {
            info.push(air);
        }
    }
    // Lumos port: MeshTables now also packs `solid` and `full_faces` by state
    // (derived from `info` as `MeshTables::new` does) and carries the animated
    // layers and Polytone colormaps, which are kept as they are.
    let tables: &'static MeshTables = Box::leak(Box::new(MeshTables {
        solid: info.iter().map(|i| i.solid).collect(),
        full_faces: info
            .iter()
            .map(|i| (0..6).fold(0, |bits, d| bits | u8::from(i.face[d] == 1) << d))
            .collect(),
        info,
        visibility: from.visibility.clone(),
        water_still: from.water_still,
        water_flow: from.water_flow,
        water_overlay: from.water_overlay,
        lava_still: from.lava_still,
        lava_flow: from.lava_flow,
        animated_layers: from.animated_layers.clone(),
        colormaps: from.colormaps.clone(),
        water_colormap: from.water_colormap,
    }));
    *FILTERED.write().unwrap() = Some((source, tables));
    mesher.tables = tables;
}
