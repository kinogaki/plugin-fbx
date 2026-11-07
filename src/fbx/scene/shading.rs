//! Materials, textures and the media they read.
//!
//! A material keeps every property it has, by FBX's names, and which family
//! of shader wrote them, so each family can be read into its MaterialX home
//! by name. A texture is a file and how it is placed; its media (`Video`)
//! may hold the file's bytes.
use super::graph::Graph;
use super::properties::Properties;
use crate::fbx::tree::{Node, Value};

/// A material.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Its id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// `ShadingModel` as written: `lambert`, `phong`, `unknown`…
    pub shading_model: String,
    /// The shader that wrote its properties.
    pub family: Family,
    /// Every property, over its template's defaults.
    pub properties: Properties,
    /// The textures connected to its properties, in file order.
    pub textures: Vec<TextureLink>,
}

/// The shader whose properties a material holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// FBX's Lambert (`FbxSurfaceLambert`).
    Lambert,
    /// FBX's Phong (`FbxSurfacePhong`).
    Phong,
    /// Autodesk Standard Surface (Arnold's, or Maya's OSL one), its
    /// properties under `Maya|` or `3dsMax|` names.
    StandardSurface,
    /// 3ds Max's Physical Material (`3dsMax|Parameters|…`).
    Physical,
    /// Stingray PBS, Maya's ShaderFX game material.
    Stingray,
    /// 3ds Max's PBR Material, metal and roughness (`3dsMax|main|…`).
    PbrMetalRough,
    /// 3ds Max's PBR Material, specular and glossiness (`3dsMax|main|…`).
    PbrSpecGloss,
    /// 3ds Max's glTF Material.
    Gltf,
    /// 3ds Max's OpenPBR Material.
    OpenPbr,
    /// No family this reader knows.
    Other,
}

/// A texture under one of a material's properties.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureLink {
    /// The property (`DiffuseColor`, `NormalMap`, `Maya|baseColor`…).
    pub property: String,
    /// The texture, an index into the scene's textures.
    pub texture: usize,
}

/// A file texture.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    /// Its id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// `FileName` as written (usually absolute, on the writer's machine).
    pub file_name: String,
    /// `RelativeFilename`: relative to the FBX file.
    pub relative_file_name: String,
    /// Its media, an index into the scene's videos: one holding the file's
    /// bytes when any does.
    pub video: Option<usize>,
    /// `UVSet`: the UV set it reads, by name (`default` or empty for the
    /// first).
    pub uv_set: String,
    /// `Translation` (u, v, w).
    pub translation: [f64; 3],
    /// `Rotation`, degrees.
    pub rotation: [f64; 3],
    /// `Scaling`.
    pub scaling: [f64; 3],
    /// `WrapModeU`, `WrapModeV`.
    pub wrap: [Wrap; 2],
    /// Every property, over its template's defaults.
    pub properties: Properties,
}

/// How a texture repeats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    /// It repeats (0).
    #[default]
    Repeat,
    /// Its edge repeats (1).
    Clamp,
}

/// Media: an image file, maybe with its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    /// Its id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// `Filename` as written.
    pub file_name: String,
    /// `RelativeFilename`.
    pub relative_file_name: String,
    /// The file's bytes when embedded (`Content`), else empty.
    pub content: Vec<u8>,
    /// Every property, over its template's defaults.
    pub properties: Properties,
}

/// The family of the material `object`, from the shader it is connected
/// under, its 3ds Max class, or its shading model.
pub(crate) fn family(graph: &Graph, object: usize, model: &str) -> Family {
    // A material lies under its shader (`Implementation`) in Maya's files.
    for shader in graph.parents(object).iter().map(|l| l.object) {
        if graph.objects[shader].class != "Implementation" {
            continue;
        }
        match graph.properties(shader).string("RenderAPI") {
            Some("ARNOLD_SHADER_ID" | "OSL") => return Family::StandardSurface,
            Some("SFX_PBS_SHADER") => return Family::Stingray,
            _ => {}
        }
    }
    if model.eq_ignore_ascii_case("lambert") {
        return Family::Lambert;
    }
    if model.eq_ignore_ascii_case("phong") {
        return Family::Phong;
    }
    let properties = graph.properties(object);
    let class = |name| properties.int(name).map(|v| v as u32);
    match (class("3dsMax|ClassIDa"), class("3dsMax|ClassIDb")) {
        (Some(0x3d6b_1cec), Some(0xdead_c001)) => Family::Physical,
        (Some(0xf155_1e33), Some(0x37fb_1337)) => Family::OpenPbr,
        (Some(0x3842_0192), Some(0x45fe_4e1b)) => Family::Gltf,
        (Some(0xd00f_1e00), Some(0xbe77_e500)) => Family::PbrMetalRough,
        (Some(0xd00f_1e00), Some(0x01db_ad33)) => Family::PbrSpecGloss,
        _ => Family::Other,
    }
}

/// A texture's wrap mode value.
pub(crate) fn wrap(value: Option<i64>) -> Wrap {
    if value == Some(1) {
        Wrap::Clamp
    } else {
        Wrap::Repeat
    }
}

/// A video's `Content`: binary's bytes, or ASCII's base64 text in one or
/// more strings. Empty when not embedded or unreadable.
pub(crate) fn content(node: &Node) -> Vec<u8> {
    let Some(content) = node.child("Content") else {
        return Vec::new();
    };
    let mut text = String::new();
    for value in &content.values {
        match value {
            Value::Bytes(bytes) => return bytes.clone(),
            Value::String(s) => text.push_str(s),
            _ => {}
        }
    }
    base64(text.as_bytes()).unwrap_or_default()
}

/// Standard base64, padding optional, whitespace skipped.
fn base64(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut held) = (0u32, 0u32);
    for &c in text {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        bits = bits << 6 | u32::from(v);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
            bits &= (1 << held) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_decodes_with_and_without_padding() {
        assert_eq!(super::base64(b"S2lub2dha2k=").unwrap(), b"Kinogaki");
        assert_eq!(super::base64(b"S2lub2dha2k").unwrap(), b"Kinogaki");
        assert_eq!(super::base64(b"Rk\nJY").unwrap(), b"FBX");
        assert!(super::base64(b"F*X").is_none());
    }
}
