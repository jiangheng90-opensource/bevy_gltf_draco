use bevy::asset::{LoadContext, RenderAssetUsages};
use bevy::gltf::extensions::{
    ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers,
};
use bevy::gltf::{GltfAssetLabel, GltfLoaderSettings, GltfPlugin};
use bevy::log::error;
use bevy::mesh::{Mesh, MeshVertexAttribute};
use bevy::platform::collections::HashMap;
use bevy::tasks::ConditionalSendFuture;
use bevy::{
    app::{App, Plugin},
    gltf::gltf::Gltf as JsonGltf,
};
use draco_gltf::{DecodeLimits, KHR_DRACO_MESH_COMPRESSION};

use crate::decode::decode_primitive;
use crate::mesh::{MeshContext, build_mesh, empty_mesh};

mod decode;
mod mesh;

/// Internal handler that decodes `KHR_draco_mesh_compression` data for each glTF primitive.
#[derive(Default, Clone)]
struct GltfDracoDecoderExtensionHandler {
    load_meshes: RenderAssetUsages,
    rotate_meshes: bool,
    /// `GltfPlugin::convert_coordinates`, which a load falls back to when its
    /// settings leave `convert_coordinates` unset.
    default_rotate_meshes: bool,
}

impl GltfExtensionHandler for GltfDracoDecoderExtensionHandler {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new((*self).clone())
    }

    fn on_root(&mut self, _: &mut LoadContext<'_>, _: &gltf::Gltf, settings: &GltfLoaderSettings) {
        self.load_meshes = settings.load_meshes;
        self.rotate_meshes = settings
            .convert_coordinates
            .map_or(self.default_rotate_meshes, |cc| cc.rotate_meshes);
    }

    fn on_gltf_primitive(
        &mut self,
        _load_context: &mut LoadContext<'_>,
        gltf: &JsonGltf,
        gltf_mesh: &gltf::Mesh<'_>,
        gltf_primitive: &gltf::Primitive<'_>,
        buffer_data: &[Vec<u8>],
        custom_vertex_attributes: &HashMap<Box<str>, MeshVertexAttribute>,
        gltf_mesh_on_skinned_nodes: bool,
        gltf_mesh_on_non_skinned_nodes: bool,
        user_mesh: &mut Option<Mesh>,
    ) -> impl ConditionalSendFuture<Output = ()> {
        // The decode is synchronous on every target, so it runs here and the
        // returned future is already complete.
        if let Some(extension) = gltf_primitive
            .extensions()
            .and_then(|extensions| extensions.get(KHR_DRACO_MESH_COMPRESSION))
        {
            let label = GltfAssetLabel::Primitive {
                mesh: gltf_mesh.index(),
                primitive: gltf_primitive.index(),
            }
            .to_string();
            let mesh = match decode_primitive(
                gltf,
                gltf_primitive,
                extension,
                buffer_data,
                DecodeLimits::default(),
            ) {
                Ok(decoded) => build_mesh(
                    &decoded,
                    gltf_primitive,
                    gltf_mesh,
                    buffer_data,
                    &MeshContext {
                        load_meshes: self.load_meshes,
                        rotate_meshes: self.rotate_meshes,
                        custom_vertex_attributes,
                        on_skinned_nodes: gltf_mesh_on_skinned_nodes,
                        on_non_skinned_nodes: gltf_mesh_on_non_skinned_nodes,
                        label: &label,
                    },
                ),
                Err(err) => {
                    error!("{label}: cannot decode {KHR_DRACO_MESH_COMPRESSION}: {err}");
                    empty_mesh(self.load_meshes)
                }
            };
            *user_mesh = Some(mesh);
        }
        core::future::ready(())
    }
}

/// Bevy plugin that adds runtime Draco decoding support to the glTF loader.
///
/// Add this plugin to your app to enable loading glTF models that use the
/// `KHR_draco_mesh_compression` extension on both native and WebAssembly platforms.
///
/// # Example
///
/// ```rust,no_run
/// use bevy::prelude::*;
/// use bevy_gltf_draco::GltfDracoDecoderPlugin;
///
/// fn main() {
///     App::new()
///         .add_plugins(DefaultPlugins)
///         .add_plugins(GltfDracoDecoderPlugin)
///         .run();
/// }
/// ```
pub struct GltfDracoDecoderPlugin;

impl Plugin for GltfDracoDecoderPlugin {
    fn build(&self, app: &mut App) {
        // Created here as well, so the plugin can be added before `GltfPlugin`:
        // both only initialize it, and the loader takes the shared list in
        // `GltfPlugin::finish`.
        app.init_resource::<GltfExtensionHandlers>();
    }

    fn finish(&self, app: &mut App) {
        // Every plugin has been added by now, so `GltfPlugin`'s coordinate
        // default can be read whichever order the two were added in.
        let handler = GltfDracoDecoderExtensionHandler {
            default_rotate_meshes: app
                .get_added_plugins::<GltfPlugin>()
                .first()
                .is_some_and(|gltf| gltf.convert_coordinates.rotate_meshes),
            ..Default::default()
        };
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async {
            app.world_mut()
                .resource_mut::<GltfExtensionHandlers>()
                .0
                .write()
                .await
                .push(Box::new(handler))
        });
        #[cfg(not(target_family = "wasm"))]
        app.world_mut()
            .resource_mut::<GltfExtensionHandlers>()
            .0
            .write_blocking()
            .push(Box::new(handler));
    }
}
