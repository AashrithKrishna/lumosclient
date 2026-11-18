// Lumos port: Horizon 9f42064c's loader with the item defaults already read.
fn hook(root: &Path, defaults: &horizon_world::item_defaults::Defaults) -> crate::Result<MobTypes> {
    evanders_models::load(root, true, None, Some(defaults))
}
