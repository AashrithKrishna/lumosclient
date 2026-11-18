// Each client tick on a server: where the player and the other players are,
// then the payloads the voice chat has for the server, its settings and its
// chat lines.
fn voicechat_tick(game: &mut crate::game::Game<'_>) {
    use voicechat_engine as vc;
    let Some(r) = game.remote.as_ref() else {
        return;
    };
    if !game.native_on(vc::ID) || !r.joined || r.reconfiguring.is_some() {
        vc::tick(None, &|_| None);
        return;
    }
    let address = r.session.address.clone();
    let players: Vec<(u128, [f64; 3])> = r
        .level_players
        .iter()
        .filter_map(|(&id, &uuid)| {
            // Lumos port: `remote_entity_position` is gone from Horizon; a
            // server's player entities are in `mobs`, as the game's own
            // player-position readers find them.
            let m = game.mobs.iter().find(|m| m.alive() && m.id as i32 == id)?;
            let [x, y, z] = [m.position.x, m.position.y, m.position.z];
            Some((uuid, [x, y + vc::EYE_HEIGHT, z]))
        })
        .collect();
    let listener = game.native_player().map(|p| vc::mix::Listener {
        eye: [p.x, p.y + vc::EYE_HEIGHT, p.z],
        yaw: p.yaw,
    });
    let world = vc::World {
        address: &address,
        listener,
        players,
        redirect: horizon_protocol::server_redirect,
    };
    let done = {
        let game = &*game;
        vc::tick(Some(world), &|key| game.native_setting(vc::ID, key))
    };
    for (channel, data) in done.payloads {
        game.send(horizon_protocol::packets::play::Serverbound::CustomPayload(
            horizon_protocol::packets::configuration::CustomPayload { channel, data },
        ));
    }
    for (key, value) in done.store {
        game.native_set_setting(vc::ID, &key, &value);
    }
    for line in done.chat {
        game.native_chat(&line);
    }
}
