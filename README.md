# Bevy glTF Draco Decoder

A Bevy plugin that provides Draco mesh compression support for glTF loader. This extension enables loading glTF models with `KHR_draco_mesh_compression` extension in both native and WebAssembly environments.

## Features

- Decode Draco-compressed glTF meshes at runtime
- Support for both native and WASM platforms
- Seamless integration with Bevy's glTF loader
- Support for all standard mesh attributes (positions, normals, texture coordinates, joints, weights, etc.)
- Morph target support for Draco-compressed meshes

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
bevy_gltf_draco = "0.2"
```

For WASM support, ensure you have the following dependencies:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen-futures = "0.4"
```

## Bevy Version Support

We intend to track the latest Bevy release. PRs supporting this are welcome!

| bevy | bevy_gltf_draco |
| ---- | --------------- |
| 0.19 | 0.1, 0.2        |

## Quick Start

### 1. Add the Plugin

Add `GltfDracoDecoderPlugin` to your Bevy app:

```rust
use bevy::prelude::*;
use bevy_gltf_draco::GltfDracoDecoderPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GltfDracoDecoderPlugin)  // Add Draco decoder plugin
        .add_systems(Startup, setup)
        .run();
}
```

### 2. Load Draco-Compressed glTF Models

Load your glTF models with validation disabled (required for Draco-compressed models):

```rust
fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(SceneRoot(
        asset_server.load_with_settings(
            GltfAssetLabel::Scene(0).from_asset("models/your_model.gltf"),
            |s: &mut GltfLoaderSettings| {
                s.validate = false;  // Required: gltf-rs cannot validate KHR_draco_mesh_compression
            },
        )
    ));
}
```

The plugin automatically handles `KHR_draco_mesh_compression` extension when the glTF loader processes primitives.

## Running the Example

### Native

```bash
# Run the example on native platforms
cargo run --example main
```

### WebAssembly

Build and serve (one command):

```bash
npm install
npm start
```

Or run separately:

```bash
npm run build    # Build WASM
npm run serve    # Serve on http://localhost:3000
```

Then open browser: http://localhost:3000

## Platform-Specific Notes

### Native (Desktop)

The decoder runs directly in the async context with zero-copy data handling where possible.

### WebAssembly (WASM)

Due to JavaScript/WASM interop constraints, the plugin uses a channel-based delegation pattern:

1. Draco data is copied to satisfy `'static` lifetime requirements
2. Decoding runs via `spawn_local` to handle non-`Send` JS/WASM objects
3. Results are returned through a channel

This architecture is necessary because:
- JS interop types (like `Uint8Array`) contain raw pointers that don't implement `Send + Sync`
- `wasm_bindgen_futures` uses `Rc<RefCell>` internally

## Supported Mesh Attributes

The decoder supports the following glTF semantic attributes:

| Semantic | Description |
|----------|-------------|
| POSITION | Vertex positions |
| NORMAL | Vertex normals |
| TANGENT | Vertex tangents |
| TEXCOORD_n | Texture coordinates (set n) |
| COLOR_n | Vertex colors (set n) |
| JOINTS_n | Joint indices for skeletal animation (set n) |
| WEIGHTS_n | Joint weights for skeletal animation (set n) |
| _CUSTOM | Custom attributes (prefixed with underscore) |

## Supported Data Types

| Draco DataType | Rust Type |
|---------------|-----------|
| DT_INT8 | i8 |
| DT_UINT8 | u8 |
| DT_INT16 | i16 |
| DT_UINT16 | u16 |
| DT_INT32 | i32 |
| DT_UINT32 | u32 |
| DT_FLOAT32 | f32 |

## Advanced Usage

### Custom Vertex Attributes

If your model uses custom vertex attributes, configure the `GltfPlugin`:

```rust
use bevy::mesh::{MeshVertexAttribute, VertexFormat};

App::new()
    .add_plugins(DefaultPlugins.set(GltfPlugin::default().add_custom_vertex_attribute(
        "BATCHID",
        MeshVertexAttribute::new("_BATCHID", 2137464976, VertexFormat::Float32),
    )))
    .add_plugins(GltfDracoDecoderPlugin)
```

### Validation Settings (Required)

**Important**: Draco-compressed glTF models cannot pass `gltf-rs` validation. You **must** disable validation when loading:

```rust
use bevy::gltf::GltfLoaderSettings;

commands.spawn(SceneRoot(
    asset_server.load_with_settings(
        GltfAssetLabel::Scene(0).from_asset("models/model.gltf"),
        |s: &mut GltfLoaderSettings| {
            s.validate = false;  // Required for KHR_draco_mesh_compression
        },
    )
));
```

## How It Works

### Extension Handler

The plugin implements the `GltfExtensionHandler` trait and hooks into `on_gltf_primitive`:

```rust
#[async_trait::async_trait]
impl GltfExtensionHandler for GltfDracoDecoderExtensionHandler {
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
        // 1. Parse KHR_draco_mesh_compression extension from the primitive
        // 2. Decode the Draco-encoded buffer via draco_decoder
        // 3. Build a temporary glTF document describing the decoded data
        // 4. Convert attributes and indices into a Bevy Mesh
        // 5. Populate user_mesh so Bevy's glTF loader uses the decoded result
    }
}
```

### Decoding Pipeline

```
glTF Primitive with KHR_draco_mesh_compression
                    ↓
        Extract buffer view reference
                    ↓
        Read encoded Draco data
                    ↓
        Decode mesh via draco_decoder
                    ↓
        Build temporary glTF document structure
                    ↓
        Convert attributes, indices and morph targets
                    ↓
        Populate Bevy Mesh returned to the glTF loader
```

## Troubleshooting

### Model Not Rendering

1. Ensure the plugin is added **before** loading models
2. Check that your glTF file uses `KHR_draco_mesh_compression` extension
3. Verify the model loads correctly in other glTF viewers

### WASM Build Errors

- Ensure `wasm-bindgen` CLI version matches the crate version
- Add required WASM dependencies to `Cargo.toml`

### Attribute Type Mismatches

The decoder automatically maps Draco data types to glTF component types. If you see unexpected type conversions, check the original glTF's attribute definitions.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## Resources

- [Draco 3D Data Compression](https://google.github.io/draco/)
- [glTF KHR_draco_mesh_compression Extension](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_draco_mesh_compression)
- [Bevy Engine](https://bevyengine.org/)
