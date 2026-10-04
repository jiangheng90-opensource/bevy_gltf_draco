use std::path::{Path, PathBuf};

use bevy::gltf::GltfAssetLabel;
use bevy::gltf::GltfPlugin;
use bevy::gltf::convert_coordinates::GltfConvertCoordinates;
use bevy::mesh::VertexAttributeValues;
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

/// Builds an app like `loads_draco_compressed_gltf` does, over `asset_dir`.
fn app(asset_dir: &Path, gltf: GltfPlugin, draco_first: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: asset_dir.to_string_lossy().into_owned(),
            ..Default::default()
        },
        bevy::image::ImagePlugin::default(),
    ));
    if draco_first {
        app.add_plugins((GltfDracoDecoderPlugin, gltf));
    } else {
        app.add_plugins((gltf, GltfDracoDecoderPlugin));
    }
    app.init_asset::<Mesh>();
    app.init_asset::<bevy::pbr::StandardMaterial>();
    app.init_asset::<bevy::animation::AnimationClip>();
    app.init_asset::<bevy::world_serialization::WorldAsset>();
    app.finish();
    app.cleanup();
    app
}

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// Loads primitive 0 of mesh 0 of `path` and pumps the app until it is ready.
fn load_mesh(
    app: &mut App,
    path: &str,
    convert_coordinates: Option<GltfConvertCoordinates>,
) -> Mesh {
    let handle: Handle<Mesh> = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(move |s: &mut bevy::gltf::GltfLoaderSettings| {
            s.validate = false;
            s.convert_coordinates = convert_coordinates;
        })
        .load(
            GltfAssetLabel::Primitive {
                mesh: 0,
                primitive: 0,
            }
            .from_asset(path.to_owned()),
        );
    for _ in 0..1000 {
        app.update();
        if let Some(mesh) = app.world().resource::<Assets<Mesh>>().get(&handle) {
            return mesh.clone();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!(
        "{path} did not load: {:?}",
        app.world()
            .resource::<AssetServer>()
            .get_load_state(&handle)
    );
}

fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("positions are {other:?}"),
    }
}

const GRID: &str = "models/DracoMorphGrid/DracoMorphGrid.glb";

/// The plugin used to panic when added before `GltfPlugin`, whose resource
/// it wrote to in `build`.
#[test]
fn plugin_order_does_not_matter() {
    let mut app = app(&assets(), GltfPlugin::default(), true);
    let mesh = load_mesh(&mut app, GRID, None);
    assert_eq!(mesh.count_vertices(), 25);
}

/// Morph targets are not compressed: they come from the primitive's own
/// accessors. The fixture lifts vertex `i` of its 5x5 grid by `0.1 + 0.01 * i`,
/// so each decoded vertex can be checked against the displacement it gets.
#[test]
fn morph_targets_survive_decoding() {
    let mut app = app(&assets(), GltfPlugin::default(), false);
    let mesh = load_mesh(&mut app, GRID, None);
    let targets = mesh.morph_targets().expect("morph targets");
    assert_eq!(targets.len(), mesh.count_vertices());
    assert_eq!(mesh.morph_target_names(), Some(&["Lift".to_string()][..]));
    for (position, target) in positions(&mesh).iter().zip(targets) {
        let x = ((position[0] + 0.5) * 4.0).round() as usize;
        let y = ((position[1] + 0.5) * 4.0).round() as usize;
        let expected = 0.1 + 0.01 * (y * 5 + x) as f32;
        assert!(
            (target.position.z - expected).abs() < 1e-4,
            "vertex at {position:?} lifted by {} instead of {expected}",
            target.position.z
        );
    }
}

/// A normalized COLOR_0 must widen to 0..1, not be read as raw integers.
#[test]
fn normalized_colors_stay_normalized() {
    let mut app = app(&assets(), GltfPlugin::default(), false);
    let mesh = load_mesh(&mut app, GRID, None);
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("colors are {:?}", mesh.attribute(Mesh::ATTRIBUTE_COLOR));
    };
    for color in colors {
        assert!(color.iter().all(|c| (0.0..=1.0).contains(c)), "{color:?}");
        assert!((color[3] - 1.0).abs() < 1e-6);
    }
}

/// With no per-load setting, `GltfPlugin::convert_coordinates` applies, as it
/// does to uncompressed meshes.
#[test]
fn plugin_coordinate_conversion_applies() {
    let rotate = GltfConvertCoordinates {
        rotate_meshes: true,
        ..Default::default()
    };
    let mut by_plugin = app(
        &assets(),
        GltfPlugin {
            convert_coordinates: rotate,
            ..Default::default()
        },
        false,
    );
    let mut by_setting = app(&assets(), GltfPlugin::default(), false);
    let default = positions(&load_mesh(&mut by_plugin, GRID, None));
    let explicit = positions(&load_mesh(&mut by_setting, GRID, Some(rotate)));
    assert_eq!(default, explicit);
}
