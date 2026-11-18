fn lumos_controls_clamp(
    ui: &mut crate::options_ui::OptionsUi,
    top: f32,
    bottom: f32,
    content: f32,
) -> core::ops::ControlFlow<()> {
    crate::options_ui::lumos_controls_glue::clamp(ui, top, bottom, content)
}
