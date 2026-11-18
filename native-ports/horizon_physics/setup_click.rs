// A press on the title screen while the quick setup shows is the quick setup's.
// Lumos: only while it was really drawn (`setup_on_screen`), not whenever it is due: a setup
// that isn't on screen must never swallow the title screen's clicks.
fn horizon_physics_setup_click(
    _title: &mut Title,
    x: f32,
    y: f32,
    size: (f32, f32),
) -> ::core::ops::ControlFlow<(Action, bool)> {
    use crate::native_horizon_physics as hp;
    if !hp::setup_visible() || !hp::setup_on_screen() {
        return ::core::ops::ControlFlow::Continue(());
    }
    let hit = hp::setup_click(size.0.ceil(), size.1.ceil(), x, y);
    ::core::ops::ControlFlow::Break((Action::None, hit))
}
