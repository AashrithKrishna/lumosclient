// The chat box's command tree: /wp gets its words (add, del, ...) to complete.
// Lumos port: the tree `chat_tree_for` builds at 9f42064c, then extended with /wp's words.
fn official_worldmap_chat_tree(
    game: &mut crate::game::Game<'_>,
    base: Option<&horizon_client::command::Dispatcher>,
) -> ::core::ops::ControlFlow<horizon_client::command::Dispatcher> {
    let mut names = Vec::new();
    game.native_commands(&mut names);
    let base = match base {
        Some(tree) => tree,
        None => horizon_client::command::vanilla::game_dispatcher(),
    };
    let tree = with_studio_commands(&with_native_commands(base, &names)).with_profiler();
    ::core::ops::ControlFlow::Break(crate::game::official_worldmap::extend_tree(tree))
}
