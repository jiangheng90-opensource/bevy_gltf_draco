use std::{collections::BTreeMap, vec};

use bevy_asset::LoadContext;
use bevy_platform::collections::HashMap;
use draco_decoder::{AttributeDataType, DracoDecodeConfig, decode_mesh_with_config_sync};
use gltf::{
    Document, Gltf, Primitive, Semantic,
    json::validation::{
        Checked::{self, Valid},
        USize64,
    },
};
use serde::Deserialize;
use tracing::warn;

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

#[derive(Debug, Deserialize, Default)]
pub struct DracoExtensionValue {
    #[serde(rename = "bufferView")]
    pub buffer_view: usize,
    #[allow(dead_code)]
    pub attributes: HashMap<String, usize>,
}

#[derive(Debug, Default)]
pub struct DracoSemanticLink {
    pub map: BTreeMap<usize, Semantic>,
    pub buffer_view: usize,
}

impl DracoSemanticLink {
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

pub(crate) struct DracoExtension {
    pub(crate) link: DracoSemanticLink,
}

impl DracoExtension {
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
            return None;
        };

        let link = DracoSemanticLink::from_extension_value(&value);

        Some(DracoExtension { link })
    }

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

        // fix when index below u32
        let indices = primitive.indices().unwrap();
        let data_type = match (indices.data_type(), indices.count()) {
            (gltf::accessor::DataType::U16, count) if count > u16::MAX as usize => {
                gltf::accessor::DataType::U32
            }
            (data_type, _) => data_type,
        };

        let indices_accessor = root.push(gltf::json::Accessor {
            buffer_view: Some(indices_index),
            byte_offset: None,
            count: USize64::from(primitive.indices().unwrap().count()),
            component_type: Valid(gltf::json::accessor::GenericComponentType(data_type)),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Valid(primitive.indices().unwrap().dimensions()),
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
                count: USize64::from(old_attr.count()),
                component_type: Valid(gltf::json::accessor::GenericComponentType(
                    old_attr.data_type(),
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

    pub fn decode_mesh(
        &self,
        gltf: &Gltf,
        buffer_data: &Vec<Vec<u8>>,
    ) -> Option<(DracoDecodeConfig, Vec<Vec<u8>>)> {
        let view = gltf.views().nth(self.link.buffer_view).unwrap();
        let draco_encode_slice: &[u8] =
            &buffer_data[view.buffer().index()][view.offset()..view.offset() + view.length()];
        let result_opt = decode_mesh_with_config_sync(draco_encode_slice);

        let Some(result) = result_opt else {
            warn!("draco decode fail!");
            return None;
        };

        Some((result.config, vec![result.data]))
    }
}
