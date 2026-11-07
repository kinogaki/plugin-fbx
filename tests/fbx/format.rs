//! The format through the SDK's contract, its color service converting
//! nothing: the committed exports read into the standard values where the
//! project puts them (any units, axes or handedness), their layers on their
//! domains, their materials as MaterialX networks with their textures,
//! their cameras and lights; every shader family's material in its model;
//! a mesh two nodes show held once; damaged files and a cancelled read.
use crate::common::{binary, fixture, node, p, s};
use crate::graph::{document, float, floats, ints, material, object, oo, op, triangle};
use kinogaki_fbx::Fbx;
use kinogaki_fbx::fbx::tree::{Node, Value as Raw};
use kinogaki_render_api::material::mtlx::SIGNATURE;
use kinogaki_render_api::standard::LightShape;
use kinogaki_studio_api::geometry::image::ImageFile;
use kinogaki_studio_api::geometry::{Domain, Units, UpAxis, Vec3, Xform, names};
use kinogaki_studio_api::image_files::{IMAGE_NODE, image_source};
use kinogaki_studio_api::network::NetworkSeed;
use kinogaki_studio_api::standard::{self, Materials};
use kinogaki_studio_api::{
    CancelToken, Error, FileFormat, FileItem, Imported, Input, Purpose, Reading, Value,
};
use kinogaki_studio_api_testing::Unconverted;

fn metres() -> Units {
    Units::new(1.0, UpAxis::Y).unwrap()
}

/// `bytes` (a file named `name`) read into `units`.
fn read_in(name: &str, bytes: &[u8], units: Units) -> Imported {
    let reading = Reading::new(Purpose::Objects, units, &Unconverted);
    let input = Input::memory(name, bytes);
    Fbx.read(&input, &Fbx.options(), &reading).unwrap()
}

/// The fixture `name` read into metres, Y up.
fn read(name: &str) -> Imported {
    read_in(name, &fixture(name), metres())
}

fn looks(read: &Imported) -> &Materials {
    let first = read.items[0].data.as_ref().unwrap();
    standard::materials()
        .get(first)
        .expect("the materials first")
}

fn item<'a>(read: &'a Imported, name: &str) -> &'a FileItem {
    let found = read.items.iter().find(|i| i.name == name);
    found.unwrap_or_else(|| panic!("no {name}"))
}

/// Where item `name` is in the project: its placement after its parents'.
fn world(read: &Imported, name: &str) -> Xform {
    let mut at = item(read, name);
    let mut world = at.placement;
    while let Some(parent) = at.parent {
        at = &read.items[parent];
        world = at.placement.compose(&world);
    }
    world
}

/// Where the node of `seed` of MaterialX node `node` is.
fn position(seed: &NetworkSeed, node: &str) -> Option<usize> {
    seed.nodes
        .iter()
        .position(|n| match n.parameters.get(SIGNATURE) {
            Some(Value::Choice(sig)) => kinogaki_mtlx::library()
                .def(sig)
                .is_some_and(|d| d.node == node),
            _ => false,
        })
}

/// The node of `seed` of MaterialX node `node`.
fn of_node<'a>(
    seed: &'a NetworkSeed,
    node: &str,
) -> Option<&'a kinogaki_studio_api::network::NewNode> {
    position(seed, node).map(|at| &seed.nodes[at])
}

#[test]
fn houdinis_box_lands_where_houdini_put_it_with_its_layers_and_materials() {
    let read = read("houdini/box_7400.fbx");
    // A centimetre file in metres: the box's world origin, a hundredth.
    let at = world(&read, "box").origin();
    assert!(
        (at - Vec3::new(0.00189191, 0.00753798, 0.00014676)).length() < 1e-6,
        "{at:?}"
    );
    let box_ = standard::geometry()
        .get(item(&read, "box").data.as_ref().unwrap())
        .unwrap();
    assert_eq!((box_.positions().len(), box_.primitives().len()), (8, 6));
    let a = box_.attributes();
    assert_eq!(
        a.tuples::<[f32; 2]>(Domain::Vertex, "uv").map(<[_]>::len),
        Some(24)
    );
    assert_eq!(
        a.tuples::<[f32; 2]>(Domain::Vertex, "uv2").map(<[_]>::len),
        Some(24)
    );
    assert_eq!(
        a.tuples::<[f32; 3]>(Domain::Point, names::CD)
            .map(<[_]>::len),
        Some(8)
    );
    assert_eq!(
        a.tuples::<[f32; 3]>(Domain::Vertex, names::N)
            .map(<[_]>::len),
        Some(24)
    );
    let faces = a.text(Domain::Primitive, names::MATERIAL).unwrap();
    let keys: Vec<&str> = (0..6).map(|f| faces.get(f).unwrap()).collect();
    assert_eq!(keys, ["red", "red", "red", "blue", "blue", "blue"]);
    // Red: a Phong Surface whose diffuse color is a texture of checker.png.
    let red = &looks(&read).materials[0];
    assert_eq!((red.key.as_str(), red.name.as_str()), ("red", "red"));
    let phong = of_node(&red.inside, "kinogaki_phong_surface").unwrap();
    assert!(phong.display);
    assert_eq!(phong.parameters["model"], Value::Choice("phong".into()));
    let image = red
        .inside
        .nodes
        .iter()
        .find(|n| n.node_type == IMAGE_NODE)
        .unwrap();
    let source = image_source(&image.parameters).unwrap().unwrap();
    assert!(source.name().ends_with("checker.png"), "{}", source.name());
    let blue = of_node(&looks(&read).materials[1].inside, "kinogaki_phong_surface").unwrap();
    assert_eq!(
        blue.parameters["diffuse_color"],
        Value::Color([0.1, 0.2, 0.9])
    );
}

#[test]
fn cameras_look_down_their_minus_z_and_lights_shine_down_theirs() {
    let read = read("houdini/box_7400.fbx");
    let camera = world(&read, "camera");
    // Houdini's camera at (0, 2, 8) looks down the world's −Z.
    assert!((camera.origin() - Vec3::new(0.0, 0.02, 0.08)).length() < 1e-9);
    assert!((camera.apply_vector(Vec3::Z * -1.0) - Vec3::Z * -1.0).length() < 1e-9);
    let lens = standard::camera()
        .get(item(&read, "camera").data.as_ref().unwrap())
        .unwrap();
    assert_eq!((lens.focal_length, lens.horizontal_aperture), (35.0, 36.0));
    let sun = world(&read, "distant_light");
    let direction = sun.apply_vector(Vec3::Z * -1.0);
    // FBX's −Y of its node, in the world: down and away.
    let expected = Vec3::new(-0.353553, -std::f64::consts::FRAC_1_SQRT_2, -0.612372);
    assert!((direction - expected).length() < 1e-5, "{direction:?}");
    let point = standard::light()
        .get(item(&read, "point_light").data.as_ref().unwrap())
        .unwrap();
    assert!(matches!(
        point.shape,
        LightShape::Spot {
            cone_angle: 180.0,
            ..
        }
    ));
    assert_eq!(point.intensity, 2.5);
    assert!(
        read.warnings
            .iter()
            .any(|w| w.message.contains("approximated"))
    );
}

#[test]
fn a_light_out_of_range_is_left_out_and_said() {
    let name = "houdini/box_7400_ascii.fbx";
    let text = String::from_utf8(fixture(name)).unwrap();
    let bright = r#"P: "Intensity", "Number", "", "A",250"#;
    assert!(text.contains(bright));
    let dim = text.replace(bright, r#"P: "Intensity", "Number", "", "A",-250"#);
    let read = read_in(name, dim.as_bytes(), metres());
    let point = read.items.iter().find(|i| i.name == "point_light");
    assert!(point.is_none_or(|i| i.data.is_none()), "no light value");
    assert!(
        (read.warnings.iter()).any(|w| w.message.contains("point_light: a light out of range")),
        "{:?}",
        read.warnings
    );
    let sun = item(&read, "distant_light").data.as_ref();
    assert!(
        sun.is_some_and(|d| standard::light().is(d)),
        "the others read"
    );
}

#[test]
fn a_z_up_export_lands_on_the_y_up_one_and_a_left_handed_one_faces_out() {
    let read_box = |name: &str| {
        let read = read(name);
        let at = world(&read, "box");
        let g = standard::geometry()
            .get(item(&read, "box").data.as_ref().unwrap())
            .unwrap()
            .clone();
        (at, g)
    };
    let (y_at, y_up) = read_box("houdini/box_7400.fbx");
    let (z_at, z_up) = read_box("houdini/box_zup.fbx");
    for (a, b) in y_up.positions().iter().zip(z_up.positions().iter()) {
        let (a, b) = (
            y_at.apply_point(Vec3::from(*a)),
            z_at.apply_point(Vec3::from(*b)),
        );
        assert!((a - b).length() < 1e-6, "{a:?} {b:?}");
    }
    // A left-handed file mirrors into the project: its faces are wound the
    // other way, so each faces where its normals say.
    let (_, left) = read_box("houdini/box_yup_left.fbx");
    let normals = left
        .attributes()
        .tuples::<[f32; 3]>(Domain::Vertex, names::N)
        .unwrap();
    for face in 0..left.primitives().len() {
        let corner = left.primitives().primitive_vertices(face).start;
        let said = Vec3::from(normals[corner].map(f64::from));
        let wound = left.polygon_normal(face).normalized().unwrap();
        assert!(wound.dot(said) > 0.9, "face {face}");
    }
}

/// Houdini's box `name` read into `units`, in the project: each point where
/// it lands, and each face's normal by its winding beside the normal its
/// first corner says (the box's normals are smoothed: within 60° of the
/// face's, never facing away).
fn box_in(name: &str, units: Units) -> (Vec<Vec3>, Vec<(Vec3, Vec3)>) {
    let read = read_in(name, &fixture(name), units);
    let at = world(&read, "box");
    let g = standard::geometry()
        .get(item(&read, "box").data.as_ref().unwrap())
        .unwrap();
    let points: Vec<Vec3> = (g.positions().iter())
        .map(|p| at.apply_point(Vec3::from(*p)))
        .collect();
    let normals = (g.attributes())
        .tuples::<[f32; 3]>(Domain::Vertex, names::N)
        .unwrap();
    let faces = (0..g.primitives().len())
        .map(|face| {
            let corners = g.primitives().primitive(face);
            let corner = |c: usize| points[corners[c] as usize];
            let wound = (corner(1) - corner(0)).cross(corner(2) - corner(0));
            let first = g.primitives().primitive_vertices(face).start;
            let said = at.apply_vector(Vec3::from(normals[first].map(f64::from)));
            (wound.normalized().unwrap(), said.normalized().unwrap())
        })
        .collect();
    (points, faces)
}

/// A Y-up file read into a Z-up project turns as a rotation does (its +Y the
/// project's +Z, its +Z the project's −Y), and a Z-up file into a Y-up project
/// the other way: each lands where the turn puts it, its faces facing where
/// their normals say.
#[test]
fn y_up_and_z_up_files_read_into_the_other_up_axis_turned_not_mirrored() {
    let z_up = Units::new(1.0, UpAxis::Z).unwrap();
    let (y_in_y, _) = box_in("houdini/box_7400.fbx", metres());
    let (y_in_z, faces) = box_in("houdini/box_7400.fbx", z_up);
    for (a, b) in y_in_y.iter().zip(&y_in_z) {
        let turned = Vec3::new(a.x, -a.z, a.y);
        assert!((turned - *b).length() < 1e-6, "{a:?} at {b:?}");
    }
    for (face, (wound, said)) in faces.iter().enumerate() {
        assert!(wound.dot(*said) > 0.5, "Y up into Z up, face {face}");
    }
    // Where it lands, `a_z_up_export_lands_on_the_y_up_one_and_a_left_handed_one_faces_out`.
    let (_, faces) = box_in("houdini/box_zup.fbx", metres());
    for (face, (wound, said)) in faces.iter().enumerate() {
        assert!(wound.dot(*said) > 0.5, "Z up into Y up, face {face}");
    }
}

#[test]
fn embedded_media_are_embedded_and_blenders_layers_land_on_their_domains() {
    let read_box = read("houdini/box_embedded.fbx");
    let red = &looks(&read_box).materials[0];
    let image = red
        .inside
        .nodes
        .iter()
        .find(|n| n.node_type == IMAGE_NODE)
        .unwrap();
    let source = image_source(&image.parameters).unwrap().unwrap();
    let ImageFile::Embedded(file) = source.file() else {
        panic!("the file packs it");
    };
    assert!(file.bytes().starts_with(b"\x89PNG"));
    // Blender's textured plane: a normal map into the normal.
    let plane = read("blender/textured.fbx");
    let checker = &looks(&plane).materials[0].inside;
    assert!(of_node(checker, "normalmap").is_some());
    // Blender's cube: smoothing by edge as hard edges, two UV sets, a
    // second color set said.
    let cube = read("blender/mesh.fbx");
    let g = standard::geometry()
        .get(item(&cube, "Layered").data.as_ref().unwrap())
        .unwrap();
    let hard = g.attributes().group(Domain::Edge, names::SHARP).unwrap();
    assert_eq!(hard.len(), 4);
    assert!(
        g.attributes()
            .tuples::<[f32; 2]>(Domain::Vertex, "uv2")
            .is_some()
    );
    assert!(
        cube.warnings
            .iter()
            .any(|w| w.message.contains("color sets after the first"))
    );
}

#[test]
fn units_scale_points_not_placements() {
    let millimetres = Units::new(0.001, UpAxis::Z).unwrap();
    let read = read_in("zup_cm.fbx", &fixture("blender/zup_cm.fbx"), millimetres);
    let at = world(&read, "Centimetres");
    assert!(
        (at.origin() - Vec3::new(100.0, 200.0, 300.0)).length() < 1e-9,
        "{:?}",
        at.origin()
    );
    assert!(
        (at.axes()[0] - Vec3::X).length() < 1e-12,
        "no scale in the placement"
    );
}

/// A material of `family`'s properties, on a triangle a model shows.
fn family_file(
    name: &str,
    model: &str,
    properties: Vec<Node>,
    more: Vec<Node>,
    links: Vec<Node>,
) -> Vec<u8> {
    let mut objects = vec![
        triangle(1, vec![]),
        object("Model", 2, "Tri", "Mesh", vec![], vec![]),
        material(3, name, model, properties),
    ];
    objects.extend(more);
    let mut connections = vec![oo(2, 0), oo(1, 2), oo(3, 2)];
    connections.extend(links);
    binary(&document(objects, connections), 7400, true)
}

#[test]
fn each_shader_family_lands_in_its_model() {
    let int = |i: i64| Raw::Int(i);
    let v3 = |a: f64, b: f64, c: f64| vec![float(a), float(b), float(c)];
    let class = |a: u32, b: u32| {
        vec![
            p("3dsMax|ClassIDa", "int", vec![int(i64::from(a as i32))]),
            p("3dsMax|ClassIDb", "int", vec![int(i64::from(b as i32))]),
        ]
    };
    let stingray_shader = object(
        "Implementation",
        9,
        "Stingray_Implementation",
        "",
        vec![p("RenderAPI", "KString", vec![s("SFX_PBS_SHADER")])],
        vec![],
    );
    let arnold_shader = object(
        "Implementation",
        9,
        "Arnold_Implementation",
        "",
        vec![p("RenderAPI", "KString", vec![s("ARNOLD_SHADER_ID")])],
        vec![],
    );
    let mut metal_rough = class(0xd00f_1e00, 0xbe77_e500);
    metal_rough.push(p("3dsMax|main|basecolor", "Color", v3(0.5, 0.25, 0.125)));
    metal_rough.push(p("3dsMax|main|metalness", "Float", vec![float(1.0)]));
    let mut spec_gloss = class(0xd00f_1e00, 0x01db_ad33);
    spec_gloss.push(p("3dsMax|main|glossiness", "Float", vec![float(0.75)]));
    let mut physical = class(0x3d6b_1cec, 0xdead_c001);
    physical.push(p(
        "3dsMax|Parameters|base_color",
        "Color",
        v3(0.25, 0.5, 0.75),
    ));
    physical.push(p("3dsMax|Parameters|trans_ior", "Float", vec![float(1.33)]));
    let cases: Vec<(Vec<u8>, &str, &str, Value)> = vec![
        (
            family_file(
                "Arnold",
                "lambert",
                vec![
                    p("Maya|baseColor", "Vector3D", v3(0.1, 0.2, 0.3)),
                    p("Maya|specularIOR", "Number", vec![float(1.4)]),
                ],
                vec![arnold_shader],
                vec![oo(3, 9)],
            ),
            "standard_surface",
            "specular_IOR",
            Value::Number(1.4),
        ),
        (
            family_file(
                "Stingray",
                "unknown",
                vec![
                    p("Maya|base_color", "Vector3D", v3(0.5, 0.5, 0.25)),
                    p("Maya|emissive_intensity", "Number", vec![float(2.0)]),
                ],
                vec![stingray_shader],
                vec![oo(3, 9)],
            ),
            "gltf_pbr",
            "emissive_strength",
            Value::Number(2.0),
        ),
        (
            family_file("MetalRough", "unknown", metal_rough, vec![], vec![]),
            "gltf_pbr",
            "metallic",
            Value::Number(1.0),
        ),
        (
            family_file("SpecGloss", "unknown", spec_gloss, vec![], vec![]),
            "UsdPreviewSurface",
            "roughness",
            Value::Number(0.25),
        ),
        (
            family_file("Physical", "unknown", physical, vec![], vec![]),
            "standard_surface",
            "specular_IOR",
            Value::Number(1.33),
        ),
        (
            family_file(
                "Lambert",
                "lambert",
                vec![p("DiffuseColor", "Color", v3(0.5, 0.5, 0.5))],
                vec![],
                vec![],
            ),
            "kinogaki_phong_surface",
            "model",
            Value::Choice("lambert".into()),
        ),
    ];
    for (bytes, model, input, value) in cases {
        let read = read_in("family.fbx", &bytes, metres());
        let m = &looks(&read).materials[0];
        let surface = of_node(&m.inside, model)
            .unwrap_or_else(|| panic!("{}: no {model}: {:?}", m.name, m.inside.nodes));
        assert!(surface.display, "{}", m.name);
        assert_eq!(surface.parameters.get(input), Some(&value), "{}", m.name);
    }
}

#[test]
fn a_materials_lengths_move_into_the_projects_units() {
    let int = |i: i64| Raw::Int(i);
    // 3ds Max's Physical material: its transmission depth, 2 file units
    // (centimetres), is Standard Surface's in metres.
    let physical = vec![
        p("3dsMax|ClassIDa", "int", vec![int(0x3d6b_1cec)]),
        p(
            "3dsMax|ClassIDb",
            "int",
            vec![int(0xdead_c001_u32 as i32 as i64)],
        ),
        p("3dsMax|Parameters|trans_depth", "Float", vec![float(2.0)]),
    ];
    let bytes = family_file("Physical", "unknown", physical, vec![], vec![]);
    let read = read_in("glass.fbx", &bytes, metres());
    let m = &looks(&read).materials[0];
    let surface = of_node(&m.inside, "standard_surface").unwrap();
    let Some(Value::Number(depth)) = surface.parameters.get("transmission_depth") else {
        panic!("{:?}", surface.parameters);
    };
    assert!((depth - 0.02).abs() < 1e-12, "{depth}");
}

#[test]
fn a_mesh_two_nodes_show_is_held_once_each_naming_its_material() {
    let objects = vec![
        triangle(1, vec![]),
        object("Model", 2, "A", "Mesh", vec![], vec![]),
        object("Model", 3, "B", "Mesh", vec![], vec![]),
        material(4, "Red", "phong", vec![]),
        material(5, "Blue", "phong", vec![]),
    ];
    let connections = vec![oo(2, 0), oo(3, 0), oo(1, 2), oo(1, 3), oo(4, 2), oo(5, 3)];
    let bytes = binary(&document(objects, connections), 7400, true);
    let read = read_in("shared.fbx", &bytes, metres());
    let a = standard::instances()
        .get(item(&read, "A").data.as_ref().unwrap())
        .unwrap();
    let b = standard::instances()
        .get(item(&read, "B").data.as_ref().unwrap())
        .unwrap();
    assert!(
        a.prototypes()[0].ptr_eq(&b.prototypes()[0]),
        "one mesh, held once"
    );
    let named = b
        .attributes()
        .text(Domain::Instance, names::MATERIAL)
        .unwrap();
    assert_eq!(named.get(0), Some("Blue"));
}

/// A mesh of two polygons in two material slots, shown by a node giving
/// them Red and Green and one giving them Blue and Red: held once, the
/// second node's instance mapping each of the mesh's materials to its own.
#[test]
fn a_mesh_nodes_give_other_materials_is_held_once_with_their_maps() {
    use kinogaki_studio_api::geometry::Values;
    let slots = node(
        "LayerElementMaterial",
        vec![Raw::Int(0)],
        vec![
            node("MappingInformationType", vec![s("ByPolygon")], vec![]),
            node("ReferenceInformationType", vec![s("IndexToDirect")], vec![]),
            node("Materials", vec![ints(&[0, 1])], vec![]),
        ],
    );
    let mesh = node(
        "Geometry",
        vec![Raw::Int(1), s("Geometry::Two"), s("Mesh")],
        vec![
            node(
                "Vertices",
                vec![floats(&[
                    0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0,
                ])],
                vec![],
            ),
            node(
                "PolygonVertexIndex",
                vec![ints(&[0, 1, !2, 0, 2, !3])],
                vec![],
            ),
            slots,
        ],
    );
    let objects = vec![
        mesh,
        object("Model", 2, "A", "Mesh", vec![], vec![]),
        object("Model", 3, "B", "Mesh", vec![], vec![]),
        material(4, "Red", "phong", vec![]),
        material(5, "Green", "phong", vec![]),
        material(6, "Blue", "phong", vec![]),
    ];
    let connections = vec![
        oo(2, 0),
        oo(3, 0),
        oo(1, 2),
        oo(1, 3),
        oo(4, 2),
        oo(5, 2),
        oo(6, 3),
        oo(4, 3),
    ];
    let bytes = binary(&document(objects, connections), 7400, true);
    let read = read_in("mapped.fbx", &bytes, metres());
    let [a, b] = ["A", "B"].map(|name| {
        let data = item(&read, name).data.clone().expect("an instance");
        standard::instances().get(&data).expect("instances").clone()
    });
    assert!(
        a.prototypes()[0].ptr_eq(&b.prototypes()[0]),
        "one mesh, held once"
    );
    assert!(
        a.attributes()
            .get(Domain::Instance, names::MATERIAL_MAP)
            .is_none()
    );
    let map = b
        .attributes()
        .get(Domain::Instance, names::MATERIAL_MAP)
        .expect("a map");
    let Values::Text(text) = map.values() else {
        panic!("text");
    };
    let pairs: Vec<&str> = (0..text.len()).filter_map(|k| text.get(k)).collect();
    assert_eq!(pairs, ["Red", "Blue", "Green", "Red"]);
    assert_eq!(
        map.offsets().map(|o| o.as_slice().to_vec()),
        Some(vec![0, 4])
    );
}

#[test]
fn a_texture_under_a_property_reaches_its_input_placed_and_wrapped() {
    let texture = object(
        "Texture",
        6,
        "Wood",
        "",
        vec![
            p(
                "Translation",
                "Vector",
                vec![float(0.5), float(0.25), float(0.0)],
            ),
            p("WrapModeU", "enum", vec![Raw::Int(1)]),
        ],
        vec![
            node("FileName", vec![s("/maps/wood.png")], vec![]),
            node("RelativeFilename", vec![s("maps/wood.png")], vec![]),
        ],
    );
    let bytes = family_file(
        "Wood",
        "phong",
        vec![],
        vec![texture],
        vec![op(6, 3, "DiffuseColor")],
    );
    let read = read_in("wood.fbx", &bytes, metres());
    let wood = &looks(&read).materials[0].inside;
    let image = of_node(wood, "image").unwrap();
    assert_eq!(
        image.parameters["uaddressmode"],
        Value::Choice("clamp".into())
    );
    let place = position(wood, "place2d").expect("its translation places it");
    // place2d reads no UVs unwired (a constant (0, 0) in MaterialX): its
    // texture coordinates are wired, the first set's.
    let uvs = (wood.wires.iter())
        .find(|w| w.to == place && w.input == "texcoord")
        .expect("its texture coordinates are wired");
    assert_eq!(Some(uvs.from), position(wood, "texcoord"));
    let file = wood
        .nodes
        .iter()
        .find(|n| n.node_type == IMAGE_NODE)
        .unwrap();
    let source = image_source(&file.parameters).unwrap().unwrap();
    // A file in memory names nothing on disk: its relative name stays as
    // written, for the project's folder to find.
    assert_eq!(source.name(), "maps/wood.png");
}

#[test]
fn damaged_files_fail_and_a_cancelled_read_stops() {
    let bytes = fixture("houdini/box_7400.fbx");
    let reading = Reading::new(Purpose::Objects, metres(), &Unconverted);
    for cut in (0..bytes.len()).step_by(211) {
        let input = Input::memory("cut.fbx", &bytes[..cut]);
        if let Err(e) = Fbx.read(&input, &Fbx.options(), &reading) {
            assert!(
                e.message().contains("cut.fbx") || e == Error::cancelled(),
                "{e}"
            );
        }
    }
    let stop = CancelToken::default();
    stop.cancel();
    let cancelled = Reading {
        budget: kinogaki_studio_api::format::ReadBudget::default().cancelled_by(stop),
        ..Reading::new(Purpose::Objects, metres(), &Unconverted)
    };
    let input = Input::memory("box.fbx", &bytes);
    assert_eq!(
        Fbx.read(&input, &Fbx.options(), &cancelled).unwrap_err(),
        Error::cancelled()
    );
    assert!(Fbx.detects(&bytes[..64]));
}
