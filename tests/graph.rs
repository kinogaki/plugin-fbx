//! The object graph on synthetic files (each written binary and ASCII):
//! templates, shader families, instancing, inheritance, and what a damaged
//! graph does.
mod common;

use common::{ascii, binary, node, p, s};
use kinogaki_fbx::fbx::tree::{Array, Node, Value};
use kinogaki_fbx::fbx::{self, Attribute, Error, Family, Inherit, Scene};

fn int(i: i64) -> Value {
    Value::Int(i)
}

fn float(x: f64) -> Value {
    Value::Float(x)
}

fn floats(v: &[f64]) -> Value {
    Value::Array(Array::F64(v.to_vec()))
}

fn ints(v: &[i32]) -> Value {
    Value::Array(Array::I32(v.to_vec()))
}

/// An object node with a `Properties70`.
fn object(
    class: &str,
    id: i64,
    name: &str,
    subclass: &str,
    properties: Vec<Node>,
    mut more: Vec<Node>,
) -> Node {
    more.insert(0, node("Properties70", vec![], properties));
    node(
        class,
        vec![int(id), s(&format!("{class}::{name}")), s(subclass)],
        more,
    )
}

fn oo(child: i64, parent: i64) -> Node {
    node("C", vec![s("OO"), int(child), int(parent)], vec![])
}

fn op(child: i64, parent: i64, property: &str) -> Node {
    node(
        "C",
        vec![s("OP"), int(child), int(parent), s(property)],
        vec![],
    )
}

/// A unit triangle mesh, with `layers` under it.
fn triangle(id: i64, layers: Vec<Node>) -> Node {
    let mut children = vec![
        node(
            "Vertices",
            vec![floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])],
            vec![],
        ),
        node("PolygonVertexIndex", vec![ints(&[0, 1, !2])], vec![]),
    ];
    children.extend(layers);
    node(
        "Geometry",
        vec![int(id), s("Geometry::Tri"), s("Mesh")],
        children,
    )
}

/// A file of `objects` and `connections`, with Phong and Lambert material
/// templates, read binary and ASCII: both must agree.
fn scene(objects: Vec<Node>, connections: Vec<Node>) -> Result<Scene, Error> {
    let template = |name: &str, diffuse: f64| {
        node(
            "PropertyTemplate",
            vec![s(name)],
            vec![node(
                "Properties70",
                vec![],
                vec![p("DiffuseFactor", "Number", vec![float(diffuse)])],
            )],
        )
    };
    let camera = node(
        "PropertyTemplate",
        vec![s("FbxCamera")],
        vec![node(
            "Properties70",
            vec![],
            vec![p(
                "Color",
                "ColorRGB",
                vec![float(0.8), float(0.8), float(0.8)],
            )],
        )],
    );
    let definitions = node(
        "Definitions",
        vec![],
        vec![
            node(
                "ObjectType",
                vec![s("Material")],
                vec![
                    template("FbxSurfaceLambert", 0.25),
                    template("FbxSurfacePhong", 0.75),
                ],
            ),
            node("ObjectType", vec![s("NodeAttribute")], vec![camera]),
        ],
    );
    let tree = vec![
        node(
            "FBXHeaderExtension",
            vec![],
            vec![node("FBXVersion", vec![int(7400)], vec![])],
        ),
        definitions,
        node("Objects", vec![], objects),
        node("Connections", vec![], connections),
    ];
    let from_binary = fbx::read(&binary(&tree, 7400, true));
    let from_ascii = fbx::read(ascii(&tree, 7400).as_bytes());
    match (&from_binary, &from_ascii) {
        (Ok(b), Ok(a)) => assert_eq!(
            b,
            &Scene {
                format: b.format,
                ..a.clone()
            },
            "binary and ASCII read alike"
        ),
        (Err(_), Err(_)) => {}
        _ => panic!("binary {from_binary:?}, ASCII {from_ascii:?}"),
    }
    from_binary
}

fn material(id: i64, name: &str, model: &str, properties: Vec<Node>) -> Node {
    object(
        "Material",
        id,
        name,
        "",
        properties,
        vec![node("ShadingModel", vec![s(model)], vec![])],
    )
}

#[test]
fn materials_take_their_shading_models_template_and_say_their_family() {
    let max_class = |a: u32, b: u32| {
        vec![
            p("3dsMax|ClassIDa", "int", vec![int(i64::from(a as i32))]),
            p("3dsMax|ClassIDb", "int", vec![int(i64::from(b as i32))]),
        ]
    };
    let objects = vec![
        material(1, "Lambert", "lambert", vec![]),
        material(
            2,
            "Phong",
            "Phong",
            vec![p("DiffuseFactor", "Number", vec![float(0.5)])],
        ),
        material(
            3,
            "Physical",
            "unknown",
            max_class(0x3d6b_1cec, 0xdead_c001),
        ),
        material(
            4,
            "MetalRough",
            "unknown",
            max_class(0xd00f_1e00, 0xbe77_e500),
        ),
        material(
            5,
            "SpecGloss",
            "unknown",
            max_class(0xd00f_1e00, 0x01db_ad33),
        ),
        material(6, "Gltf", "unknown", max_class(0x3842_0192, 0x45fe_4e1b)),
        material(7, "OpenPbr", "unknown", max_class(0xf155_1e33, 0x37fb_1337)),
        material(
            8,
            "Arnold",
            "lambert",
            vec![p(
                "Maya|baseColor",
                "Vector3D",
                vec![float(0.1), float(0.2), float(0.3)],
            )],
        ),
        object(
            "Implementation",
            9,
            "Arnold_Implementation",
            "",
            vec![p("RenderAPI", "KString", vec![s("ARNOLD_SHADER_ID")])],
            vec![],
        ),
        material(
            10,
            "Stingray",
            "unknown",
            vec![p(
                "Maya|TEX_color_map",
                "Vector3D",
                vec![float(1.0), float(1.0), float(1.0)],
            )],
        ),
        object(
            "Implementation",
            11,
            "Stingray_Implementation",
            "",
            vec![p("RenderAPI", "KString", vec![s("SFX_PBS_SHADER")])],
            vec![],
        ),
        material(12, "Plain", "unknown", vec![]),
    ];
    let scene = scene(objects, vec![oo(8, 9), oo(10, 11)]).unwrap();
    let families: Vec<(&str, Family)> = scene
        .materials
        .iter()
        .map(|m| (m.name.as_str(), m.family))
        .collect();
    assert_eq!(
        families,
        [
            ("Lambert", Family::Lambert),
            ("Phong", Family::Phong),
            ("Physical", Family::Physical),
            ("MetalRough", Family::PbrMetalRough),
            ("SpecGloss", Family::PbrSpecGloss),
            ("Gltf", Family::Gltf),
            ("OpenPbr", Family::OpenPbr),
            ("Arnold", Family::StandardSurface),
            ("Stingray", Family::Stingray),
            ("Plain", Family::Other),
        ]
    );
    // What a material doesn't say, its shading model's template does.
    let factor = |i: usize| scene.materials[i].properties.number("DiffuseFactor");
    assert_eq!((factor(0), factor(1)), (Some(0.25), Some(0.5)));
    assert!(!scene.materials[0].properties.says("DiffuseFactor"));
    assert_eq!(
        scene.materials[7].properties.vec3("Maya|baseColor"),
        Some([0.1, 0.2, 0.3])
    );
}

#[test]
fn textures_link_to_material_properties_and_share_embedded_media() {
    let video = |id: i64, content: &[u8]| {
        let mut more = vec![node("RelativeFilename", vec![s("maps/wood.png")], vec![])];
        if !content.is_empty() {
            more.push(node(
                "Content",
                vec![Value::Bytes(content.to_vec())],
                vec![],
            ));
        }
        object("Video", id, "wood", "Clip", vec![], more)
    };
    let texture = |id: i64, name: &str, wrap: i64| {
        let properties = vec![
            p("UVSet", "KString", vec![s("detail")]),
            p("WrapModeU", "enum", vec![int(wrap)]),
            p(
                "Scaling",
                "Vector",
                vec![float(2.0), float(2.0), float(1.0)],
            ),
        ];
        object(
            "Texture",
            id,
            name,
            "",
            properties,
            vec![node("RelativeFilename", vec![s("maps/wood.png")], vec![])],
        )
    };
    let objects = vec![
        material(1, "Wood", "phong", vec![]),
        texture(2, "Base", 1),
        texture(3, "Bump", 0),
        object("LayeredTexture", 4, "Layers", "", vec![], vec![]),
        video(5, b"\x89PNG wood"),
        video(6, b""),
    ];
    let connections = vec![
        op(4, 1, "DiffuseColor"),
        oo(2, 4),
        op(3, 1, "NormalMap"),
        oo(5, 2),
        oo(6, 3),
    ];
    let scene = scene(objects, connections).unwrap();
    let links: Vec<(&str, &str)> = scene.materials[0]
        .textures
        .iter()
        .map(|l| (l.property.as_str(), scene.textures[l.texture].name.as_str()))
        .collect();
    assert_eq!(links, [("DiffuseColor", "Base"), ("NormalMap", "Bump")]);
    let (base, bump) = (&scene.textures[0], &scene.textures[1]);
    assert_eq!(
        (base.uv_set.as_str(), base.wrap[0], base.scaling),
        ("detail", fbx::Wrap::Clamp, [2.0, 2.0, 1.0])
    );
    // The bump's own media isn't embedded; the same file embedded elsewhere is.
    assert_eq!(scene.videos[bump.video.unwrap()].content, b"\x89PNG wood");
    assert!(
        scene
            .warnings
            .iter()
            .any(|w| w.contains("layered textures")),
        "{:?}",
        scene.warnings
    );
}

#[test]
fn a_light_never_takes_a_cameras_defaults() {
    let objects = vec![
        object("NodeAttribute", 1, "Camera", "Camera", vec![], vec![]),
        object("NodeAttribute", 2, "Light", "Light", vec![], vec![]),
    ];
    let scene = scene(objects, vec![]).unwrap();
    assert_eq!(scene.cameras[0].properties.vec3("Color"), Some([0.8; 3]));
    assert_eq!(scene.lights[0].color, [1.0; 3]);
}

#[test]
fn one_mesh_shown_twice_takes_each_nodes_materials() {
    let layer = node(
        "LayerElementMaterial",
        vec![int(0)],
        vec![
            node("MappingInformationType", vec![s("AllSame")], vec![]),
            node("ReferenceInformationType", vec![s("IndexToDirect")], vec![]),
            node("Materials", vec![ints(&[0])], vec![]),
        ],
    );
    let objects = vec![
        triangle(100, vec![layer]),
        object("Model", 1, "A", "Mesh", vec![], vec![]),
        object(
            "Model",
            2,
            "B",
            "Mesh",
            vec![p(
                "Lcl Translation",
                "Lcl Translation",
                vec![float(5.0), float(0.0), float(0.0)],
            )],
            vec![],
        ),
        material(10, "Red", "phong", vec![]),
        material(11, "Blue", "phong", vec![]),
    ];
    let connections = vec![
        oo(1, 0),
        oo(2, 0),
        oo(100, 1),
        oo(100, 2),
        oo(10, 1),
        oo(11, 2),
    ];
    let scene = scene(objects, connections).unwrap();
    assert_eq!(scene.meshes.len(), 1);
    for (node, color) in scene.models.iter().zip(["Red", "Blue"]) {
        assert_eq!(node.attribute, Attribute::Mesh(0));
        let slot = scene.meshes[0].materials.as_ref().unwrap().get(0)[0] as usize;
        assert_eq!(scene.materials[node.materials[slot]].name, color);
    }
}

#[test]
fn children_inherit_their_parents_scale_as_they_say() {
    let scaled = vec![p(
        "Lcl Scaling",
        "Lcl Scaling",
        vec![float(2.0), float(1.0), float(1.0)],
    )];
    let child = |id: i64, inherit: i64| {
        let properties = vec![
            p(
                "Lcl Rotation",
                "Lcl Rotation",
                vec![float(0.0), float(0.0), float(90.0)],
            ),
            p(
                "Lcl Translation",
                "Lcl Translation",
                vec![float(1.0), float(0.0), float(0.0)],
            ),
            p("InheritType", "enum", vec![int(inherit)]),
            p(
                "GeometricTranslation",
                "Vector3D",
                vec![float(0.0), float(0.0), float(3.0)],
            ),
        ];
        object(
            "Model",
            id,
            &format!("Child{inherit}"),
            "Null",
            properties,
            vec![],
        )
    };
    let objects = vec![
        object("Model", 1, "Parent", "Null", scaled, vec![]),
        child(2, 0),
        child(3, 1),
        child(4, 2),
    ];
    let scene = scene(objects, vec![oo(1, 0), oo(2, 1), oo(3, 1), oo(4, 1)]).unwrap();
    let world = scene.world_matrices();
    let column = |i: usize, c: usize| [world[i].0[0][c], world[i].0[1][c], world[i].0[2][c]];
    let round = |v: [f64; 3]| v.map(|x| (x * 1e9).round() / 1e9 + 0.0);
    // The child's X axis turned onto Y: RrSs scales it by the parent's 2,
    // RSrs scales the parent's X (now the child's −Y), Rrs neither.
    assert_eq!(scene.models[1].transform.inherit, Inherit::Componentwise);
    assert_eq!(round(column(1, 0)), [0.0, 2.0, 0.0]);
    assert_eq!(round(column(2, 1)), [-2.0, 0.0, 0.0]);
    assert_eq!(round(column(3, 0)), [0.0, 1.0, 0.0]);
    // Every one moves by the parent's scale.
    for i in 1..4 {
        assert_eq!(round(column(i, 3)), [2.0, 0.0, 0.0]);
    }
    // The geometric offset places what the node shows, not its children.
    let shown = world[1] * scene.models[1].geometric.matrix();
    assert_eq!(round(shown.point([0.0; 3])), [2.0, 0.0, 3.0]);
}

#[test]
fn a_cycle_of_parents_is_broken_and_said() {
    let objects = vec![
        object("Model", 1, "A", "Null", vec![], vec![]),
        object("Model", 2, "B", "Null", vec![], vec![]),
    ];
    let scene = scene(objects, vec![oo(1, 2), oo(2, 1)]).unwrap();
    assert_eq!(scene.models.len(), 2);
    let top = scene.models.iter().filter(|m| m.parent.is_none()).count();
    assert_eq!(top, 1);
    assert!(
        scene
            .warnings
            .iter()
            .any(|w| w.contains("its own ancestor")),
        "{:?}",
        scene.warnings
    );
    // Parents come before children.
    for (i, m) in scene.models.iter().enumerate() {
        assert!(m.parent.is_none_or(|p| p < i));
    }
}

#[test]
fn a_layer_that_doesnt_fit_is_left_out_and_said() {
    let uv = |name: &str, values: &[f64], indices: &[i32]| {
        node(
            "LayerElementUV",
            vec![int(0)],
            vec![
                node("Name", vec![s(name)], vec![]),
                node("MappingInformationType", vec![s("ByPolygonVertex")], vec![]),
                node("ReferenceInformationType", vec![s("IndexToDirect")], vec![]),
                node("UV", vec![floats(values)], vec![]),
                node("UVIndex", vec![ints(indices)], vec![]),
            ],
        )
    };
    let layers = vec![
        uv("good", &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0], &[0, 1, 2]),
        uv("short", &[0.0, 0.0], &[0, 0]),
        uv("outside", &[0.0, 0.0], &[0, 0, 7]),
        node("LayerElementHole", vec![int(0)], vec![]),
    ];
    let scene = scene(vec![triangle(1, layers)], vec![]).unwrap();
    let names: Vec<&str> = scene.meshes[0]
        .uvs
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(names, ["good"]);
    let said = scene.warnings.join("\n");
    assert!(
        said.contains("has 2 values for 3") && said.contains("names value 7 of 1"),
        "{said}"
    );
    assert!(said.contains("LayerElementHole layers left out"), "{said}");
}

#[test]
fn a_corner_naming_no_point_fails_the_read() {
    let bad = node(
        "Geometry",
        vec![int(1), s("Geometry::Bad"), s("Mesh")],
        vec![
            node("Vertices", vec![floats(&[0.0; 9])], vec![]),
            node("PolygonVertexIndex", vec![ints(&[0, 1, !5])], vec![]),
        ],
    );
    let error = scene(vec![bad], vec![]).unwrap_err();
    assert!(error.to_string().contains("names point 5 of 3"), "{error}");
}

#[test]
fn what_one_time_leaves_out_is_said() {
    let objects = vec![
        object("AnimationCurve", 1, "", "", vec![], vec![]),
        object("Deformer", 2, "Skin", "Skin", vec![], vec![]),
        object("Deformer", 3, "Shapes", "BlendShape", vec![], vec![]),
        object("Geometry", 4, "Surface", "NurbsSurface", vec![], vec![]),
        object("Model", 5, "Surface", "NurbsSurface", vec![], vec![]),
    ];
    let scene = scene(objects, vec![oo(4, 5)]).unwrap();
    let said = scene.warnings.join("\n");
    for expected in [
        "animation left out",
        "skins left out",
        "blend shapes left out",
        "NurbsSurface geometry left out",
    ] {
        assert!(said.contains(expected), "{said}");
    }
    assert_eq!(
        scene.models[0].attribute,
        Attribute::Other("NurbsSurface".into())
    );
}
