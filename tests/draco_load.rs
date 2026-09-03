use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy_gltf_draco::GltfDracoDecoderPlugin;

/// Loads a Draco-compressed glTF model through the real asset pipeline and
/// verifies that the decoded mesh has vertex data.
#[test]
fn loads_draco_compressed_gltf() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::log::LogPlugin::default(),
        AssetPlugin::default(),
        bevy::image::ImagePlugin::default(),
        bevy::gltf::GltfPlugin::default(),
        GltfDracoDecoderPlugin,
    ));
    app.init_asset::<Mesh>();
    app.init_asset::<bevy::pbr::StandardMaterial>();
    app.init_asset::<bevy::animation::AnimationClip>();
    app.init_asset::<bevy::world_serialization::WorldAsset>();
    app.finish();
    app.cleanup();

    let handle: Handle<Mesh> = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|s: &mut bevy::gltf::GltfLoaderSettings| {
            // Draco-compressed files use placeholder accessors without
            // bufferView, which fails strict validation.
            s.validate = false;
        })
        .load(
            GltfAssetLabel::Primitive {
                mesh: 0,
                primitive: 0,
            }
            .from_asset("models/DracoCompressed/CesiumMilkTruck.gltf"),
        );

    // Pump the app until the asset pipeline finishes loading.
    for _ in 0..1000 {
        app.update();
        let meshes = app.world().resource::<Assets<Mesh>>();
        if let Some(mesh) = meshes.get(&handle) {
            assert!(
                mesh.count_vertices() > 0,
                "decoded draco mesh has no vertices"
            );
            assert!(
                mesh.indices().is_some_and(|i| !i.is_empty()),
                "decoded draco mesh has no indices"
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    panic!(
        "draco compressed gltf failed to load within the time budget, state: {:?}",
        app.world()
            .resource::<AssetServer>()
            .get_load_state(&handle)
    );
}
