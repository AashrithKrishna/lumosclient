fn lumos_freelook_camera(game: &crate::game::Game<'_>, position: &mut [f64; 3], yaw: &mut f32, pitch: &mut f32) {
    crate::game::lumos_freelook_glue::on_camera_pose(game, position, yaw, pitch);
}
