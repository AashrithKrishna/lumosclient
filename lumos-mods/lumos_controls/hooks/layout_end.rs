fn lumos_controls_layout_end(
    value: (Vec<Widget>, f32, f32, f32),
    ui: &mut crate::options_ui::OptionsUi,
    s: &crate::settings::Settings,
    w: f32,
) -> (Vec<Widget>, f32, f32, f32) {
    crate::options_ui::lumos_controls_glue::after_layout(ui, s, w, value)
}
