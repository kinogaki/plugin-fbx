//! FBX's materials as the MaterialX model each shader family records into
//! (DESIGN §6.5), every property it says in its home and every texture an
//! Image node wired where the property it sits under goes:
//!
//! | Family | Model |
//! |---|---|
//! | Lambert, Phong, a family this reader doesn't know | `kinogaki_phong_surface` |
//! | Maya's, 3ds Max's and Arnold's Standard Surface; 3ds Max Physical | Standard Surface |
//! | 3ds Max OpenPBR | OpenPBR Surface |
//! | Stingray PBS, 3ds Max PBR metal/rough, 3ds Max glTF | glTF PBR |
//! | 3ds Max PBR spec/gloss | USD Preview Surface, its specular workflow |
//!
//! A normal map becomes a normal map node, a bump map a bump node, both
//! into the surface's normal; displacement a displacement node into a
//! Surface Material. A PBR material's classic properties (Maya and 3ds Max
//! write both) travel as an unconnected Phong Surface beside its surface.
use super::textures::{Files, Network, Sampler};
use crate::fbx::{Family, Material, Scene, Texture};
use kinogaki_mtlx::Type;
use kinogaki_studio_api::Value;
use kinogaki_studio_api::color::ColorService;
use kinogaki_studio_api::standard::ImportedMaterial;
use std::sync::Arc;

/// The Phong Surface's inputs by FBX's classic property names.
const CLASSIC: [(&str, &str); 15] = [
    ("DiffuseColor", "diffuse_color"),
    ("DiffuseFactor", "diffuse_factor"),
    ("AmbientColor", "ambient_color"),
    ("AmbientFactor", "ambient_factor"),
    ("SpecularColor", "specular_color"),
    ("SpecularFactor", "specular_factor"),
    ("Shininess", "shininess"),
    ("ShininessExponent", "shininess"),
    ("EmissiveColor", "emissive_color"),
    ("EmissiveFactor", "emissive_factor"),
    ("ReflectionColor", "reflection_color"),
    ("ReflectionFactor", "reflection_factor"),
    ("TransparentColor", "transparent_color"),
    ("TransparencyFactor", "transparency_factor"),
    ("Opacity", "opacity"),
];

/// Stingray PBS's properties (under `Maya|`) and maps into glTF PBR.
const STINGRAY: [(&str, &str); 5] = [
    ("base_color", "base_color"),
    ("metallic", "metallic"),
    ("roughness", "roughness"),
    ("emissive", "emissive"),
    ("emissive_intensity", "emissive_strength"),
];
const STINGRAY_MAPS: [(&str, &str, &str); 6] = [
    ("TEX_color_map", "use_color_map", "base_color"),
    ("TEX_metallic_map", "use_metallic_map", "metallic"),
    ("TEX_roughness_map", "use_roughness_map", "roughness"),
    ("TEX_emissive_map", "use_emissive_map", "emissive"),
    ("TEX_normal_map", "use_normal_map", "normal"),
    ("TEX_ao_map", "use_ao_map", "occlusion"),
];

/// 3ds Max's PBR material, metal and roughness (`3dsMax|main|`), into glTF
/// PBR; its maps are the properties' names with `_map`.
const METAL_ROUGH: [(&str, &str); 4] = [
    ("basecolor", "base_color"),
    ("metalness", "metallic"),
    ("emit_color", "emissive"),
    ("ao", "occlusion"),
];

/// 3ds Max's PBR material, specular and glossiness, into USD Preview
/// Surface (glossiness is inverted into roughness).
const SPEC_GLOSS: [(&str, &str); 6] = [
    ("basecolor", "diffuseColor"),
    ("base_color", "diffuseColor"),
    ("specular", "specularColor"),
    ("emit_color", "emissiveColor"),
    ("ao", "occlusion"),
    ("opacity", "opacity"),
];

/// 3ds Max's Physical material (`3dsMax|Parameters|`) into Standard
/// Surface, by Autodesk's own mapping; its maps are the names with `_map`.
const PHYSICAL: [(&str, &str); 18] = [
    ("base_weight", "base"),
    ("base_color", "base_color"),
    ("reflectivity", "specular"),
    ("refl_color", "specular_color"),
    ("roughness", "specular_roughness"),
    ("metalness", "metalness"),
    ("trans_ior", "specular_IOR"),
    ("transparency", "transmission"),
    ("trans_color", "transmission_color"),
    ("trans_depth", "transmission_depth"),
    ("scattering", "subsurface"),
    ("sss_color", "subsurface_color"),
    ("coating", "coat"),
    ("coat_color", "coat_color"),
    ("coat_roughness", "coat_roughness"),
    ("coat_ior", "coat_IOR"),
    ("emission", "emission"),
    ("emit_color", "emission_color"),
];

/// What reading a material needs: its scene, the files its textures
/// read, the color service and the meshes' UV set names.
pub(super) struct Shading<'a, 's> {
    pub scene: &'s Scene,
    pub files: &'a mut Files<'s>,
    pub colors: &'a dyn ColorService,
    pub uv_sets: &'a [String],
}

/// Material `m` of the scene, keyed `key`, with what it approximated.
pub(super) fn material(
    m: &Material,
    key: &str,
    cx: &mut Shading<'_, '_>,
) -> (ImportedMaterial, Vec<String>) {
    let mut net = Network::new(cx.colors);
    let shown = match m.family {
        Family::Lambert | Family::Phong | Family::Other => {
            if m.family == Family::Other {
                net.said.push(format!(
                    "{}: its shader is none FBX's families name; its classic properties read",
                    m.name
                ));
            }
            phong(m, &mut net, cx)
        }
        Family::StandardSurface => named(m, "ND_standard_surface_surfaceshader", &[], &mut net, cx),
        Family::OpenPbr => named(m, "ND_open_pbr_surface_surfaceshader", &[], &mut net, cx),
        Family::Gltf => named(m, "ND_gltf_pbr_surfaceshader", &GLTF_NAMES, &mut net, cx),
        Family::Physical => named(
            m,
            "ND_standard_surface_surfaceshader",
            &PHYSICAL,
            &mut net,
            cx,
        ),
        Family::Stingray => stingray(m, &mut net, cx),
        Family::PbrMetalRough => named(m, "ND_gltf_pbr_surfaceshader", &METAL_ROUGH, &mut net, cx),
        Family::PbrSpecGloss => spec_gloss(m, &mut net, cx),
    };
    let classic = !matches!(m.family, Family::Lambert | Family::Phong | Family::Other);
    if classic && CLASSIC.iter().any(|(p, _)| m.properties.says(p)) {
        phong(m, &mut net, cx);
    }
    net.seed.nodes[shown].display = true;
    let material = ImportedMaterial {
        key: key.to_owned(),
        name: m.name.clone(),
        inside: net.seed,
        standard: None,
        double_sided: true,
    };
    (material, net.said)
}

/// glTF's names (3ds Max's glTF material, under `3dsMax|main|`) where glTF
/// PBR's differ.
const GLTF_NAMES: [(&str, &str); 2] = [("metalness", "metallic"), ("emission_color", "emissive")];

/// The Phong Surface of `m`'s classic properties and the textures under
/// them, its normal and bump maps and displacement; its node.
fn phong(m: &Material, net: &mut Network<'_>, cx: &mut Shading<'_, '_>) -> usize {
    let at = net.node("ND_kinogaki_phong_surface_surfaceshader", "Phong");
    let def = net.def(at);
    let model = if m.family == Family::Lambert {
        "lambert"
    } else {
        "phong"
    };
    net.set(at, "model", Value::Choice(Arc::from(model)));
    for (property, input) in CLASSIC {
        if let Some(p) = m.properties.get(property) {
            net.set_property(at, &def, input, p);
        }
        if let Some(texture) = texture_under(m, property, cx.scene) {
            let ty = def.input(input).map_or(Type::Float, |p| p.ty.clone());
            let (from, output) = net.texture(texture, cx.files, Sampler::Image, &ty, cx.uv_sets);
            net.seed.wire(from, output, at, input);
        }
    }
    normals(m, at, net, cx);
    displaced(m, at, net, cx)
}

/// `m`'s normal map (`NormalMap`) or bump map (`Bump`, `BumpFactor`) into
/// node `at`'s `normal`.
fn normals(m: &Material, at: usize, net: &mut Network<'_>, cx: &mut Shading<'_, '_>) {
    if let Some(texture) = texture_under(m, "NormalMap", cx.scene) {
        let (map, output) = net.texture(
            texture,
            cx.files,
            Sampler::Image,
            &Type::Vector3,
            cx.uv_sets,
        );
        let normal = net.node("ND_normalmap_float", "NormalMap");
        net.seed.wire(map, output, normal, "in");
        net.seed.wire(normal, "out", at, "normal");
    } else if let Some(texture) = texture_under(m, "Bump", cx.scene) {
        let (height, output) =
            net.texture(texture, cx.files, Sampler::Image, &Type::Float, cx.uv_sets);
        let bump = net.node("ND_bump_vector3", "Bump");
        net.seed.wire(height, output, bump, "height");
        if let Some(factor) = m.properties.number("BumpFactor") {
            net.set(bump, "scale", Value::Number(factor));
        }
        net.seed.wire(bump, "out", at, "normal");
    }
}

/// Surface `at`, or a Surface Material of it and `m`'s displacement map
/// (`DisplacementColor`, `DisplacementFactor`): what the material shows.
fn displaced(m: &Material, at: usize, net: &mut Network<'_>, cx: &mut Shading<'_, '_>) -> usize {
    let Some(texture) = texture_under(m, "DisplacementColor", cx.scene) else {
        return at;
    };
    let (height, output) = net.texture(texture, cx.files, Sampler::Image, &Type::Float, cx.uv_sets);
    let displacement = net.node("ND_displacement_float", "Displacement");
    net.seed.wire(height, output, displacement, "displacement");
    if let Some(factor) = m.properties.number("DisplacementFactor") {
        net.set(displacement, "scale", Value::Number(factor));
    }
    let wrap = net.node("ND_surfacematerial", "Material");
    net.seed.wire(at, "out", wrap, "surfaceshader");
    net.seed
        .wire(displacement, "out", wrap, "displacementshader");
    wrap
}

/// A surface of definition `signature` holding each of `m`'s properties
/// under an application's prefix (`Maya|baseColor`, `3dsMax|main|basecolor`)
/// that names one of its inputs: renamed by `names`, else by its own name
/// in snake case (`specularIOR` as `specular_IOR`); a map (`…_map`, `…Map`)
/// or a texture under a property into the input it names. Properties named
/// `glossiness` are left to the caller.
fn named(
    m: &Material,
    signature: &str,
    names: &[(&str, &str)],
    net: &mut Network<'_>,
    cx: &mut Shading<'_, '_>,
) -> usize {
    let at = net.node(signature, "Surface");
    let def = net.def(at);
    let sampler = match signature {
        "ND_UsdPreviewSurface_surfaceshader" => Sampler::Preview,
        s if s.starts_with("ND_gltf") => Sampler::Gltf,
        _ => Sampler::Image,
    };
    let input_of = |property: &str| -> Option<String> {
        let leaf = property.rsplit('|').next()?;
        let stem = leaf
            .strip_suffix("_map")
            .or_else(|| leaf.strip_suffix("Map"))
            .unwrap_or(leaf);
        let snake = snake(stem);
        let named = names
            .iter()
            .find(|(from, _)| *from == snake || *from == stem)
            .map(|(_, to)| to.to_string());
        let input = named.unwrap_or(snake);
        let input = if input == "normal_camera" {
            "normal".to_owned()
        } else {
            input
        };
        def.input(&input).is_some().then_some(input)
    };
    let mut read = false;
    for p in m.properties.own.iter().filter(|p| p.name.contains('|')) {
        if let Some(input) = input_of(&p.name)
            && !p.name.ends_with("_map")
            && !p.name.ends_with("Map")
        {
            read |= net.set_property(at, &def, &input, p);
        }
    }
    for link in m
        .textures
        .iter()
        .filter(|l| !l.property.ends_with("|glossiness_map"))
    {
        let texture = &cx.scene.textures[link.texture];
        let Some(input) = input_of(&link.property) else {
            net.said.push(format!(
                "{}: the texture under {} has no home; kept unwired",
                m.name, link.property
            ));
            net.unused(texture, cx.files);
            continue;
        };
        let ty = def.input(&input).map_or(Type::Float, |p| p.ty.clone());
        if input == "normal" {
            let (map, output) = net.texture(texture, cx.files, sampler, &Type::Vector3, cx.uv_sets);
            let normal = match sampler {
                Sampler::Gltf => net.node("ND_gltf_normalmap_vector3_1_0", "NormalMap"),
                _ => net.node("ND_normalmap_float", "NormalMap"),
            };
            net.seed.wire(
                map,
                output,
                normal,
                if sampler == Sampler::Gltf {
                    "file"
                } else {
                    "in"
                },
            );
            net.seed.wire(normal, "out", at, "normal");
            continue;
        }
        let (from, output) = net.texture(texture, cx.files, sampler, &ty, cx.uv_sets);
        net.seed.wire(from, output, at, &input);
    }
    if !read && m.textures.is_empty() {
        net.said.push(format!(
            "{}: no property of its shader read; its surface stands at its defaults",
            m.name
        ));
    }
    at
}

/// `name` in snake case: `baseColor` as `base_color`, `specularIOR` as
/// `specular_IOR`; a name in snake case already as it is.
fn snake(name: &str) -> String {
    let (head, ior) = match name.strip_suffix("IOR") {
        Some(head) if !head.is_empty() => (head, true),
        _ => (name, false),
    };
    let mut out = String::with_capacity(name.len() + 4);
    for c in head.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    if ior {
        out.push_str("_IOR");
    }
    out
}

/// Stingray PBS as glTF PBR: its values, its maps where its `use_…_map`
/// says (the others kept as unwired Image nodes), placed by its
/// `uv_offset` and `uv_scale`.
fn stingray(m: &Material, net: &mut Network<'_>, cx: &mut Shading<'_, '_>) -> usize {
    let at = net.node("ND_gltf_pbr_surfaceshader", "Surface");
    let def = net.def(at);
    let maya = |name: &str| m.properties.get(&format!("Maya|{name}"));
    for (from, to) in STINGRAY {
        if let Some(p) = maya(from) {
            net.set_property(at, &def, to, p);
        }
    }
    let offset = maya("uv_offset").and_then(|p| Some([p.number(0)?, p.number(1)?]));
    let scale = maya("uv_scale").and_then(|p| Some([p.number(0)?, p.number(1)?]));
    for (map, used, input) in STINGRAY_MAPS {
        let Some(texture) = texture_under(m, &format!("Maya|{map}"), cx.scene) else {
            continue;
        };
        if maya(used).and_then(|p| p.number(0)) == Some(0.0) {
            net.unused(texture, cx.files);
            continue;
        }
        let ty = match input {
            "base_color" | "emissive" => Type::Color3,
            "normal" => Type::Vector3,
            _ => Type::Float,
        };
        let (from, output) = net.texture(texture, cx.files, Sampler::Gltf, &ty, cx.uv_sets);
        if let Some(offset) = offset {
            net.set(from, "offset", Value::Numbers(offset.to_vec()));
        }
        if let Some(scale) = scale {
            net.set(from, "scale", Value::Numbers(scale.to_vec()));
        }
        if input == "normal" {
            let normal = net.node("ND_gltf_normalmap_vector3_1_0", "NormalMap");
            net.seed.wire(from, output, normal, "file");
            net.seed.wire(normal, "out", at, "normal");
        } else {
            net.seed.wire(from, output, at, input);
        }
    }
    at
}

/// 3ds Max's PBR spec/gloss as USD Preview Surface's specular workflow,
/// glossiness inverted into roughness.
fn spec_gloss(m: &Material, net: &mut Network<'_>, cx: &mut Shading<'_, '_>) -> usize {
    let at = named(
        m,
        "ND_UsdPreviewSurface_surfaceshader",
        &SPEC_GLOSS,
        net,
        cx,
    );
    net.set(at, "useSpecularWorkflow", Value::Integer(1));
    let glossiness = m
        .properties
        .own
        .iter()
        .find(|p| p.name.ends_with("|glossiness"));
    if let Some(g) = glossiness.and_then(|p| p.number(0)) {
        net.set(at, "roughness", Value::Number(1.0 - g));
    }
    let map = (m.textures.iter()).find(|l| l.property.ends_with("|glossiness_map"));
    if let Some(link) = map {
        let texture = &cx.scene.textures[link.texture];
        let (from, output) = net.texture(
            texture,
            cx.files,
            Sampler::Preview,
            &Type::Float,
            cx.uv_sets,
        );
        let invert = net.node("ND_invert_float", "Roughness");
        net.seed.wire(from, output, invert, "in");
        net.seed.wire(invert, "out", at, "roughness");
    }
    at
}

/// The texture under `m`'s property `property`, if one is.
fn texture_under<'s>(m: &Material, property: &str, scene: &'s Scene) -> Option<&'s Texture> {
    let link = m.textures.iter().find(|l| l.property == property)?;
    scene.textures.get(link.texture)
}

#[cfg(test)]
mod tests {
    use super::snake;

    #[test]
    fn names_turn_to_snake_case_with_ior_kept() {
        assert_eq!(snake("baseColor"), "base_color");
        assert_eq!(snake("specularIOR"), "specular_IOR");
        assert_eq!(snake("thinFilmIOR"), "thin_film_IOR");
        assert_eq!(snake("base_color"), "base_color");
        assert_eq!(
            snake("transmissionExtraRoughness"),
            "transmission_extra_roughness"
        );
    }
}
