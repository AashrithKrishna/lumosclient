// Lumos Fullbright Key: between Horizon's `Game` and the rules in `lumos_fullbright_key`.
use crate::game::Game;
use crate::game::lumos_fullbright_key as m;
use crate::game::lumos_fullbright_key_keys as keys;
use crate::mod_api::NativeMod;
use horizon_client::text::Text;

fn settings(game: &Game<'_>) -> m::Settings {
    m::settings(|key| game.native_setting(m::ID, key))
}

/// Flips Horizon's own Fullbright (unless it is held off here), keeps the choice as the Video
/// Settings row does, and says what happened.
fn flip(game: &mut Game<'_>, notify: bool) {
    // Up to date for this connection (it is also refreshed each tick).
    game.hold_fullbright();
    let t = m::toggle(game.settings.fullbright_held, game.settings.extras.fullbright);
    if let m::Toggle::Set(on) = t {
        game.settings.extras.fullbright = on;
        game.hold_fullbright();
        game.settings.save(&Game::options_path());
    }
    if notify {
        let (text, colour) = m::notice(t);
        game.native_client_action_bar(Text::colored(text, colour), false);
    }
}

/// `native_key`: the toggle key, while playing.
pub(super) fn on_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !game.native_on(m::ID) || !game.native_playing() || game.player.is_none() {
        return;
    }
    if keys::pressed(game, keys::TOGGLE, key, pressed) {
        let notify = settings(game).notify;
        flip(game, notify);
    }
}

pub(super) fn on_list(mods: &mut Vec<NativeMod>) {
    mods.push(NativeMod {
        id: m::ID.into(),
        title: format!("{} {}", m::NAME, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: keys::listed(),
    });
}

pub(super) fn on_settings(game: &mut Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id != m::ID {
        return;
    }
    game.hold_fullbright();
    let (on, held) = (game.settings.extras.fullbright, game.settings.fullbright_held);
    buttons.extend(settings(game).labels(on, held));
}

pub(super) fn on_click(game: &mut Game<'_>, mod_id: &str, button: usize) {
    if mod_id != m::ID {
        return;
    }
    let mut s = settings(game);
    match button {
        m::BUTTON_FULLBRIGHT => {
            // On the page there is no hotbar to show a notice over.
            flip(game, false);
            return;
        }
        m::BUTTON_NOTICE => s.notify = !s.notify,
        _ => return,
    }
    m::set_settings(s);
    for (key, value) in s.stored() {
        game.native_set_setting(m::ID, key, &value);
    }
}
