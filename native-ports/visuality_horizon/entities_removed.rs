// Entity ids were removed (a server's RemoveEntities, or one entity): Visuality
// forgets their cooldowns. Lumos port: Horizon now removes a packet's ids
// together in `remote_removal::remove_entities`.
fn visuality_entities_removed(ids: &[i32]) {
    for &id in ids {
        crate::game::visuality_horizon::entity_changed(id);
    }
}
