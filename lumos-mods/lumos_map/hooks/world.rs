fn lumos_map_world(game: &crate::game::Game<'_>, camera: [f64; 3], primitives: &mut WorldPrimitives) {
    crate::game::lumos_map_glue::on_world(game, camera, primitives);
}
