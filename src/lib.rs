use bevy::asset::LoadContext;
use bevy::asset::RenderAssetUsages;
use bevy::gltf::extensions::GltfExtensionHandlers;
use bevy::gltf::gltf_ext::mesh::primitive_topology;
use bevy::gltf::vertex_attributes::convert_attribute;
use bevy::gltf::{
    GltfAssetLabel, GltfLoaderSettings, MorphTargetNames, PrimitiveMorphAttributesIter,
};
use bevy::mesh::MeshVertexAttribute;
use bevy::{
    app::{App, Plugin},
    gltf::extensions::ErasedGltfExtensionHandler,
    log::error,
    mesh::{Indices, Mesh},
};
use bevy::{
    gltf::{
        extensions::GltfExtensionHandler,
        gltf::{Gltf as JsonGltf, Primitive},
    },
    platform::collections::HashMap,
};
use gltf::Semantic;
use gltf::mesh::util::ReadIndices;
use tracing::warn;

use crate::khr_draco_mesh_compression::DracoExtension;

mod khr_draco_mesh_compression;

/// Internal handler that decodes `KHR_draco_mesh_compression` data for each glTF primitive.
#[derive(Default, Clone)]
struct GltfDracoDecoderExtensionHandler {
    load_meshes: RenderAssetUsages,
    rotate_meshes: bool,
}

impl GltfExtensionHandler for GltfDracoDecoderExtensionHandler {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new((*self).clone())
    }

    fn on_root(&mut self, _: &mut LoadContext<'_>, _: &gltf::Gltf, settings: &GltfLoaderSettings) {
        self.load_meshes = settings.load_meshes;
        self.rotate_meshes = match settings.convert_coordinates {
            Some(cc) => cc.rotate_meshes,
            None => false,
        }
    }

    async fn on_gltf_primitive(
        &mut self,
        load_context: &mut LoadContext<'_>,
        gltf: &JsonGltf,
        gltf_mesh: &gltf::Mesh<'_>,
        gltf_primitive: &Primitive<'_>,
        buffer_data: &[Vec<u8>],
        custom_vertex_attributes: &HashMap<Box<str>, MeshVertexAttribute>,
        gltf_mesh_on_skinned_nodes: bool,
        gltf_mesh_on_non_skinned_nodes: bool,
        user_mesh: &mut Option<Mesh>,
    ) {
        let Some(draco_extension) = DracoExtension::parse(load_context, gltf, gltf_primitive)
        else {
            // This primitive does not use KHR_draco_mesh_compression; let the
            // default loader handle it.
            return;
        };
        let Some((config, decode_data)) = draco_extension.decode_mesh(gltf, buffer_data).await
        else {
            error!(
                "failed to decode draco mesh (mesh {}, primitive {})",
                gltf_mesh.index(),
                gltf_primitive.index()
            );
            return;
        };

        let Some(draco_primitive_document) =
            draco_extension.build_document(gltf_primitive, &config)
        else {
            error!(
                "failed to build draco primitive (mesh {}, primitive {})",
                gltf_mesh.index(),
                gltf_primitive.index()
            );
            return;
        };

        let draco_primitive = DracoExtension::primitive(&draco_primitive_document);

        let primitive_topology = primitive_topology(draco_primitive.mode())
            .unwrap_or_else(|err| panic!("fail to build draco primitive, error: {:?}", err));

        let primitive_label = GltfAssetLabel::Primitive {
            mesh: gltf_mesh.index(),
            primitive: gltf_primitive.index(),
        };

        let mut mesh = Mesh::new(primitive_topology, self.load_meshes);

        // Read vertex attributes
        for (semantic, accessor) in draco_primitive.attributes() {
            if [Semantic::Joints(0), Semantic::Weights(0)].contains(&semantic) {
                if !gltf_mesh_on_skinned_nodes {
                    warn!(
                        "Ignoring attribute {:?} for skinned mesh {} used on non skinned nodes (NODE_SKINNED_MESH_WITHOUT_SKIN)",
                        semantic, primitive_label
                    );
                    continue;
                } else if gltf_mesh_on_non_skinned_nodes {
                    error!(
                        "Skinned mesh {} used on both skinned and non skin nodes, this is likely to cause an error (NODE_SKINNED_MESH_WITHOUT_SKIN)",
                        primitive_label
                    );
                }
            }
            match convert_attribute(
                semantic,
                accessor,
                &decode_data,
                custom_vertex_attributes,
                self.rotate_meshes,
            ) {
                Ok((attribute, values)) => mesh.insert_attribute(attribute, values),
                Err(err) => warn!("{}", err),
            }
        }

        // Read vertex indices
        let reader = draco_primitive.reader(|buffer| Some(decode_data[buffer.index()].as_slice()));
        if let Some(indices) = reader.read_indices() {
            mesh.insert_indices(match indices {
                ReadIndices::U8(is) => Indices::U16(is.map(|x| x as u16).collect()),
                ReadIndices::U16(is) => Indices::U16(is.collect()),
                ReadIndices::U32(is) => Indices::U32(is.collect()),
            });
        };

        {
            let morph_target_reader = reader.read_morph_targets();
            if morph_target_reader.len() != 0 {
                mesh.set_morph_targets(
                    morph_target_reader
                        .flat_map(|i| PrimitiveMorphAttributesIter {
                            convert_coordinates: self.rotate_meshes,
                            positions: i.0,
                            normals: i.1,
                            tangents: i.2,
                        })
                        .collect(),
                );

                let extras = gltf_mesh.extras().as_ref();
                if let Some(names) = extras
                    .and_then(|extras| serde_json::from_str::<MorphTargetNames>(extras.get()).ok())
                {
                    mesh.set_morph_target_names(names.target_names);
                }
            }
        }

        *user_mesh = Some(mesh);
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
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async {
            app.world_mut()
                .resource_mut::<GltfExtensionHandlers>()
                .0
                .write()
                .await
                .push(Box::new(GltfDracoDecoderExtensionHandler::default()))
        });
        #[cfg(not(target_family = "wasm"))]
        app.world_mut()
            .resource_mut::<GltfExtensionHandlers>()
            .0
            .write_blocking()
            .push(Box::new(GltfDracoDecoderExtensionHandler::default()));
    }
}
