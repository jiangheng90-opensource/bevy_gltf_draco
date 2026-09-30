# Bevy glTF Draco Decoder

A Bevy plugin that provides Draco mesh compression support for glTF loader. This extension enables loading glTF models with `KHR_draco_mesh_compression` extension in both native and WebAssembly environments.

## Features

- Decode Draco-compressed glTF meshes at runtime
- Pure Rust decoder ([draco-gltf](https://crates.io/crates/draco-gltf)): the same code on native and WASM, no C++ toolchain and no JavaScript decoder
- Seamless integration with Bevy's glTF loader
- Support for all standard mesh attributes (positions, normals, texture coordinates, joints, weights, etc.)
- Morph target support for Draco-compressed meshes

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
bevy_gltf_draco = "0.2"
```

No extra dependencies are needed for WASM.

## Bevy Version Support

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
    commands.spawn(WorldAssetRoot(
        asset_server
            .load_builder()
            .with_settings(|s: &mut GltfLoaderSettings| {
                s.validate = false; // Required when the file lists KHR_draco_mesh_compression in extensionsRequired
            })
            .load(GltfAssetLabel::Scene(0).from_asset("models/your_model.gltf")),
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

Decoding is synchronous Rust on every target, inside the glTF loader's
primitive hook. On WASM there is no JavaScript decoder to load, no worker and
no copy of the Draco data.

A primitive whose Draco data fails to decode loads as an empty mesh, with an
error in the log; the rest of the file still loads.

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

Attributes the extension does not compress are read from their ordinary
accessors, as the extension requires. Morph targets are never compressed and
index the decoded vertices in order: an encoder has to keep vertex order for
them (sequential encoding does), and targets whose count does not match the
decoded vertices are dropped with a warning.

## Supported Data Types

The glTF 2.0 component types: `BYTE` (i8), `UNSIGNED_BYTE` (u8), `SHORT` (i16),
`UNSIGNED_SHORT` (u16), `UNSIGNED_INT` (u32) and `FLOAT` (f32). An accessor's
`normalized` flag is honoured.

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

**Important**: `gltf-rs` rejects any file that lists an extension it does not
implement in `extensionsRequired`, before this plugin sees the file. Draco-only
files list `KHR_draco_mesh_compression` there, so validation must be disabled
when loading them:

```rust
use bevy::gltf::GltfLoaderSettings;

commands.spawn(WorldAssetRoot(
    asset_server
        .load_builder()
        .with_settings(|s: &mut GltfLoaderSettings| {
            s.validate = false; // Required for KHR_draco_mesh_compression
        })
        .load(GltfAssetLabel::Scene(0).from_asset("models/model.gltf")),
));
```

## How It Works

The plugin implements Bevy's `GltfExtensionHandler` and hooks into
`on_gltf_primitive`:

```
glTF Primitive with KHR_draco_mesh_compression
                    ↓
        Parse the extension, slice its buffer view
                    ↓
        Decode with draco-gltf, checked against the primitive's accessors
                    ↓
        Describe the decoded attributes as a one-buffer glTF document
                    ↓
        Convert them with Bevy's own convert_attribute; add indices,
        uncompressed attributes and morph targets from the original primitive
                    ↓
        Populate the Bevy Mesh returned to the glTF loader
```

Letting Bevy convert the attributes keeps semantic mapping, normalized
integers, custom attributes and coordinate conversion identical to
uncompressed meshes.

## Troubleshooting

### Model Not Rendering

1. Ensure the plugin is added (before or after `GltfPlugin`, either works)
2. Check that your glTF file uses `KHR_draco_mesh_compression` extension
3. Verify the model loads correctly in other glTF viewers

### WASM Build Errors

- Ensure `wasm-bindgen` CLI version matches the crate version

### Attribute Type Mismatches

Each decoded attribute is read as its glTF accessor declares it. If you see unexpected type conversions, check the original glTF's attribute definitions.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## Resources

- [Draco 3D Data Compression](https://google.github.io/draco/)
- [glTF KHR_draco_mesh_compression Extension](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_draco_mesh_compression)
- [Bevy Engine](https://bevyengine.org/)
