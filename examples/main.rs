use std::f32::consts::{FRAC_PI_4, PI};

#[cfg(not(target_family = "wasm"))]
use bevy::gltf::extensions::GltfExtensionHandlers;
use bevy::{
    gltf::{GltfLoaderSettings, GltfPlugin},
    light::CascadeShadowConfigBuilder,
    mesh::{MeshVertexAttribute, VertexFormat},
    platform::collections::HashSet,
    prelude::*,
    scene::SceneInstanceReady,
};
use bevy_asset::LoadContext;
use bevy_ecs::entity::EntityHashSet;
use bevy_gltf::extensions::GltfExtensionHandler;
use bevy_gltf_draco::GltfDracoDecoderPlugin;
use bevy_platform::collections::HashMap;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        fullsize_content_view: true,
                        titlebar_transparent: true,
                        titlebar_show_title: false,
                        present_mode: bevy::window::PresentMode::AutoVsync,
                        ..Default::default()
                    }),
                    ..default()
                })
                .set(GltfPlugin::default().add_custom_vertex_attribute(
                    "BATCHID",
                    MeshVertexAttribute::new("_BATCHID", 2137464976, VertexFormat::Float32),
                )),
            GltfDracoDecoderPlugin,
            GltfExtensionHandlerAnimationPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(PostUpdate, animate_light_direction)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(4.0, 4.0, 4.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        EnvironmentMapLight {
            diffuse_map: asset_server.load("environment_maps/pisa_diffuse_rgb9e5_zstd.ktx2"),
            specular_map: asset_server.load("environment_maps/pisa_specular_rgb9e5_zstd.ktx2"),
            intensity: 250.0,
            ..default()
        },
    ));

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            maximum_distance: 1000.0,
            ..default()
        }
        .build(),
    ));

    commands
        .spawn(SceneRoot(asset_server.load_with_settings(
            GltfAssetLabel::Scene(0).from_asset("models/DracoCompressed/CesiumMilkTruck.gltf"),
            |s: &mut GltfLoaderSettings| {
                s.gltf_without_validation = true;
            },
        )))
        .observe(play_animation_when_ready);
}

fn animate_light_direction(
    time: Res<Time>,
    mut query: Query<&mut Transform, With<DirectionalLight>>,
) {
    for mut transform in &mut query {
        transform.rotation = Quat::from_euler(
            EulerRot::ZYX,
            0.0,
            time.elapsed_secs() * PI / 5.0,
            -FRAC_PI_4,
        );
    }
}

struct GltfExtensionHandlerAnimationPlugin;

impl Plugin for GltfExtensionHandlerAnimationPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async {
            app.world_mut()
                .resource_mut::<GltfExtensionHandlers>()
                .0
                .write()
                .await
                .push(Box::new(GltfExtensionHandlerAnimation::default()))
        });
        #[cfg(not(target_family = "wasm"))]
        app.world_mut()
            .resource_mut::<GltfExtensionHandlers>()
            .0
            .write_blocking()
            .push(Box::new(GltfExtensionHandlerAnimation::default()));
    }
}

#[derive(Component, Reflect)]
#[reflect(Component)]
struct AnimationToPlay {
    graph_handle: Handle<AnimationGraph>,
    index: AnimationNodeIndex,
}

fn play_animation_when_ready(
    scene_ready: On<SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    mut players: Query<(&mut AnimationPlayer, &AnimationToPlay)>,
) {
    for child in children.iter_descendants(scene_ready.entity) {
        let Ok((mut player, animation_to_play)) = players.get_mut(child) else {
            continue;
        };

        // Tell the animation player to start the animation and keep
        // repeating it.
        //
        // If you want to try stopping and switching animations, see the
        // `animated_mesh_control.rs` example.
        player.play(animation_to_play.index).repeat();

        // Add the animation graph. This only needs to be done once to
        // connect the animation player to the mesh.
        commands
            .entity(child)
            .insert(AnimationGraphHandle(animation_to_play.graph_handle.clone()));
    }
}

#[derive(Default, Clone)]
struct GltfExtensionHandlerAnimation {
    animation_root_indices: HashSet<usize>,
    animation_root_entities: EntityHashSet,
    clip: Option<Handle<AnimationClip>>,
}

impl GltfExtensionHandler for GltfExtensionHandlerAnimation {
    fn dyn_clone(&self) -> Box<dyn GltfExtensionHandler> {
        Box::new((*self).clone())
    }

    fn on_animation(&mut self, _gltf_animation: &gltf::Animation, handle: Handle<AnimationClip>) {
        self.clip = Some(handle.clone());
    }
    fn on_animations_collected(
        &mut self,
        _load_context: &mut LoadContext<'_>,
        _animations: &[Handle<AnimationClip>],
        _named_animations: &HashMap<Box<str>, Handle<AnimationClip>>,
        animation_roots: &HashSet<usize>,
    ) {
        self.animation_root_indices = animation_roots.clone();
    }

    fn on_gltf_node(
        &mut self,
        _load_context: &mut LoadContext<'_>,
        gltf_node: &gltf::Node,
        entity: &mut EntityWorldMut,
    ) {
        if self.animation_root_indices.contains(&gltf_node.index()) {
            self.animation_root_entities.insert(entity.id());
        }
    }

    /// Called when an individual Scene is done processing
    fn on_scene_completed(
        &mut self,
        load_context: &mut LoadContext<'_>,
        _scene: &gltf::Scene,
        _world_root_id: Entity,
        world: &mut World,
    ) {
        // Create an AnimationGraph from the desired clip
        let (graph, index) = AnimationGraph::from_clip(self.clip.clone().unwrap());
        // Store the animation graph as an asset with an arbitrary label
        // We only have one graph, so this label will be unique
        let graph_handle =
            load_context.add_labeled_asset("MyAnimationGraphLabel".to_string(), graph);

        // Create a component that stores a reference to our animation
        let animation_to_play = AnimationToPlay {
            graph_handle,
            index,
        };

        // Insert the `AnimationToPlay` component on the first animation root
        let mut entity = world.entity_mut(*self.animation_root_entities.iter().next().unwrap());
        entity.insert(animation_to_play);
    }
}
