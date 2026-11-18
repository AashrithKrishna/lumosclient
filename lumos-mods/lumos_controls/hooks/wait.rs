fn lumos_controls_wait(i: usize, k: usize) -> core::ops::ControlFlow<()> {
    crate::options_ui::lumos_controls_glue::wait(i, k)
}
