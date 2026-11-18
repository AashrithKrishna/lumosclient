// SPDX-License-Identifier: GPL-3.0-or-later
// Lumos port: `Streaming::region` now returns the region in an `Arc`.
fn inject(
    world: &crate::world::World,
    x: i32,
    z: i32,
) -> ::core::ops::ControlFlow<Option<Arc<Region>>> {
    if crate::vfp_render_compat::legacy() {
        return ::core::ops::ControlFlow::Break(
            crate::vfp_render_compat::region(world, x, z).map(Arc::from),
        );
    }
    ::core::ops::ControlFlow::Continue(())
}
