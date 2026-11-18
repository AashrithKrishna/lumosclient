// No first-person hand while the camera flies free.
// Lumos port: `hand_geometry` now returns its vertices with their draw ranges.
fn freecam_hand() -> core::ops::ControlFlow<(Vec<Vertex>, Vec<(u32, i32)>)> {
    if crate::game::fair_freecam::active() {
        core::ops::ControlFlow::Break((Vec::new(), Vec::new()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
