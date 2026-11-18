// Once a frame: the settings are read as soon as the game runs (the title screen's quick setup
// needs to know whether it was done).
fn horizon_physics_setup_load(game: &mut crate::game::Game<'_>) {
    use horizon_client::native_horizon_physics as hp;
    // Lumos: a new frame; the quick setup's presses follow what it draws in it (setup_click.rs).
    hp::setup_new_frame();
    if !hp::loaded() {
        hp::load(crate::game::Game::native_config_dir().ok());
    }
    // Options' "Physics Settings..." button shows while the mod is on; Physics Settings closes
    // when the Options screen is gone (a world loading, a disconnect).
    horizon_client_render::native_horizon_physics_sway::set_physics_button_wanted(game.native_on(hp::ID));
    if hp::menu_is_open() && game.screen != crate::game::Screen::Options {
        hp::menu_close();
    }
}
