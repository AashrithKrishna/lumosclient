// Lumos port: Horizon 9f42064c's loader with the model document and the item defaults.
fn hook(
    root: &Path,
    document: &EntityModelDocument,
    defaults: &horizon_world::item_defaults::Defaults,
) -> crate::Result<MobTypes> {
    evanders_models::load(root, true, Some(document), Some(defaults))
}
