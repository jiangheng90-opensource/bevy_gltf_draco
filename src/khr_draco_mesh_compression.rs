use bevy::asset::LoadContext;
use bevy::platform::collections::HashMap;
use draco_decoder::{DracoDecodeConfig, MeshAttribute, decode_mesh_with_config};
#[cfg(target_arch = "wasm32")]
use futures::channel::oneshot;
use gltf::{
    Document, Gltf, Primitive, Semantic,
    accessor::{DataType, Dimensions},
    json::validation::{
        Checked::{self, Valid},
        USize64,
    },
};
use serde::Deserialize;
use std::{collections::BTreeMap, vec};
use tracing::{error, warn};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

/// Helper to convert a glTF attribute semantic string into a checked `Semantic`.
pub trait SemanticCheck {
    fn checked(s: &str) -> Checked<Semantic>;
}

impl SemanticCheck for Semantic {
    fn checked(s: &str) -> Checked<Self> {
        use self::Semantic::*;
        use gltf::json::validation::Checked::*;
        match s {
            "NORMAL" => Valid(Normals),
            "POSITION" => Valid(Positions),
            "TANGENT" => Valid(Tangents),

            _ if s.starts_with('_') => Valid(Extras(s[1..].to_string())),
            _ if s.starts_with("COLOR_") => match s["COLOR_".len()..].parse() {
                Ok(set) => Valid(Colors(set)),
                Err(_) => Invalid,
            },
            _ if s.starts_with("TEXCOORD_") => match s["TEXCOORD_".len()..].parse() {
                Ok(set) => Valid(TexCoords(set)),
                Err(_) => Invalid,
            },
            _ if s.starts_with("JOINTS_") => match s["JOINTS_".len()..].parse() {
                Ok(set) => Valid(Joints(set)),
                Err(_) => Invalid,
            },
            _ if s.starts_with("WEIGHTS_") => match s["WEIGHTS_".len()..].parse() {
                Ok(set) => Valid(Weights(set)),
                Err(_) => Invalid,
            },
            _ => Invalid,
        }
    }
}

/// JSON representation of the `KHR_draco_mesh_compression` primitive extension object.
#[derive(Debug, Deserialize, Default)]
pub struct DracoExtensionValue {
    #[serde(rename = "bufferView")]
    pub buffer_view: usize,
    #[allow(dead_code)]
    pub attributes: HashMap<String, usize>,
}

/// Maps Draco attribute IDs back to glTF semantics and stores the source buffer view.
#[derive(Debug, Default)]
pub struct DracoSemanticLink {
    pub map: BTreeMap<usize, Semantic>,
    pub buffer_view: usize,
}

impl DracoSemanticLink {
    /// Builds the semantic link from a parsed extension value.
    pub fn from_extension_value(value: &DracoExtensionValue) -> Self {
        let mut id = BTreeMap::new();
        for (sematic_str, index) in &value.attributes {
            id.insert(*index, Semantic::checked(sematic_str).unwrap());
        }
        Self {
            map: id,
            buffer_view: value.buffer_view,
        }
    }
}

/// Converts decoded Draco metadata into glTF accessor component types.
pub trait GltfDataType {
    fn component_data_type(&self) -> DataType;
}

impl GltfDataType for DracoDecodeConfig {
    fn component_data_type(&self) -> DataType {
        if self.index_count() > u16::MAX as u32 {
            DataType::U32
        } else {
            DataType::U16
        }
    }
}

impl GltfDataType for MeshAttribute {
    fn component_data_type(&self) -> DataType {
        match self.data_type() {
            draco_decoder::AttributeDataType::Int8 => DataType::I8,
            draco_decoder::AttributeDataType::UInt8 => DataType::U8,
            draco_decoder::AttributeDataType::Int16 => DataType::I16,
            draco_decoder::AttributeDataType::UInt16 => DataType::U16,
            draco_decoder::AttributeDataType::Int32 => {
                warn!("unspport i32 to u32");
                DataType::U32
            }
            draco_decoder::AttributeDataType::UInt32 => DataType::U32,
            draco_decoder::AttributeDataType::Float32 => DataType::F32,
        }
    }
}

/// Internal Draco extension processor for a single glTF primitive.
pub(crate) struct DracoExtension {
    pub(crate) link: DracoSemanticLink,
}

impl DracoExtension {
    /// Parses the `KHR_draco_mesh_compression` extension from a primitive, if present.
    pub(crate) fn parse(
        _: &mut LoadContext,
        _: &Document,
        primitive: &Primitive,
    ) -> Option<DracoExtension> {
        let extentions = primitive.extensions()?;

        if !extentions.contains_key("KHR_draco_mesh_compression") {
            return None;
        }

        let json_value = extentions.get("KHR_draco_mesh_compression")?;

        let Ok(value): Result<DracoExtensionValue, serde_json::Error> =
            serde_json::from_str(&json_value.to_string())
        else {
            error!("failed to parse KHR_draco_mesh_compression extension");
            return None;
        };

        let link = DracoSemanticLink::from_extension_value(&value);

        Some(DracoExtension { link })
    }

    /// Builds a temporary glTF `Document` that describes the decoded Draco buffer layout.
    pub fn build_document(
        &self,
        primitive: &Primitive,
        decode_config: &DracoDecodeConfig,
    ) -> Option<Document> {
        let buffer_length = decode_config.estimate_buffer_size();
        let mut root = gltf::json::Root::default();
        let buffer = root.push(gltf::json::Buffer {
            byte_length: USize64::from(buffer_length),
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            uri: None,
        });
        let indices_index = root.push(gltf::json::buffer::View {
            buffer,
            byte_length: USize64::from(buffer_length),
            byte_offset: Some(USize64::from(0_u64)),
            byte_stride: None,
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            target: Some(Valid(gltf::json::buffer::Target::ArrayBuffer)),
        });

        let data_type = decode_config.component_data_type();

        let indices_accessor = root.push(gltf::json::Accessor {
            buffer_view: Some(indices_index),
            byte_offset: None,
            count: USize64::from(decode_config.index_count() as usize),
            component_type: Valid(gltf::json::accessor::GenericComponentType(data_type)),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Valid(Dimensions::Scalar),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
        });

        let mut map = BTreeMap::new();
        for (index, mesh_attribute) in decode_config.attributes().iter().enumerate() {
            let semantic = self.link.map.get(&index).unwrap();
            let old_attr = primitive
                .get(semantic)
                .unwrap_or_else(|| panic!("can not get accessor by {:?}", semantic));
            let view_index = root.push(gltf::json::buffer::View {
                buffer,
                byte_length: USize64::from(mesh_attribute.lenght() as u64),
                byte_offset: Some(USize64::from(mesh_attribute.offset() as u64)),
                byte_stride: None,
                extensions: Default::default(),
                extras: Default::default(),
                name: None,
                target: Some(Valid(gltf::json::buffer::Target::ArrayBuffer)),
            });
            let attr_index = root.push(gltf::json::Accessor {
                buffer_view: Some(view_index),
                byte_offset: None,
                count: USize64::from(decode_config.vertex_count() as usize),
                component_type: Valid(gltf::json::accessor::GenericComponentType(
                    mesh_attribute.component_data_type(),
                )),
                extensions: Default::default(),
                extras: Default::default(),
                type_: Valid(old_attr.dimensions()),
                min: Some(gltf::json::Value::from(old_attr.min())),
                max: Some(gltf::json::Value::from(old_attr.max())),
                name: None,
                normalized: false,
                sparse: None,
            });
            map.insert(Valid(semantic.clone()), attr_index);
        }

        let primitive_json = gltf::json::mesh::Primitive {
            attributes: map,
            extensions: Default::default(),
            extras: Default::default(),
            indices: Some(indices_accessor),
            material: None,
            mode: Valid(gltf::json::mesh::Mode::Triangles),
            targets: None,
        };

        let _mesh_json = root.push(gltf::json::Mesh {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            primitives: vec![primitive_json],
            weights: None,
        });

        let json = Some(root);

        json.map(Document::from_json_without_validation)
    }

    /// Decodes the Draco-compressed buffer for this primitive.
    ///
    /// On WASM, decoding is delegated to a `spawn_local` task because JavaScript interop
    /// types are not `Send`. The result is returned through a one-shot channel.
    pub async fn decode_mesh(
        &self,
        gltf: &Gltf,
        buffer_data: &[Vec<u8>],
    ) -> Option<(DracoDecodeConfig, Vec<Vec<u8>>)> {
        let view = gltf.views().nth(self.link.buffer_view)?;
        let draco_encode_slice: &[u8] =
            &buffer_data[view.buffer().index()][view.offset()..view.offset() + view.length()];

        #[cfg(not(target_arch = "wasm32"))]
        {
            let result = decode_mesh_with_config(draco_encode_slice).await?;
            Some((result.config, vec![result.data]))
        }

        #[cfg(target_arch = "wasm32")]
        {
            let data = draco_encode_slice.to_vec();
            let (tx, rx) = oneshot::channel();

            spawn_local(async move {
                let result = decode_mesh_with_config(&data).await;
                if tx.send(result).is_err() {
                    warn!("Draco decode channel send failed");
                }
            });

            rx.await
                .ok()
                .flatten()
                .map(|result| (result.config, vec![result.data]))
        }
    }

    /// Returns the single primitive contained in the temporary decoded document.
    pub fn primitive(doc: &Document) -> Primitive<'_> {
        doc.meshes().next().unwrap().primitives().next().unwrap()
    }
}
