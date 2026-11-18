fn lumos_controls_keyed(
    value: Vec<Event>,
    s: &mut crate::settings::Settings,
) -> Vec<Event> {
    crate::options_ui::lumos_controls_glue::after_input(s, value)
}
