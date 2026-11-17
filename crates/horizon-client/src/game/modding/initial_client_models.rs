//! Initial-load publication boundary shared by startup and its resource fixture.
use super::Game;
impl Game<'_> {
    pub(in crate::game) fn publish_initial_client_models(&mut self) {
        if self.options.is_client_start() {
            // The entity renderers resolve against the mob types in `Resources.rest`, which load on
            // a background thread. The initial load can finish before that thread's result has been
            // applied, so wait for it here instead of racing it ("world resources joined before
            // entry"). In the usual case it is applied already and this returns at once.
            if let Err(error) = self.ensure_world_rest() {
                log::warn!("Horizon: world resources unavailable for entity renderers: {error}");
            }
            if self.res.rest.get().is_some() {
                self.resolve_entity_renderers();
            }
            self.texture_atlases_stitched();
            self.native_model_baking_completed();
            self.native_client_setup();
        }
    }
}
