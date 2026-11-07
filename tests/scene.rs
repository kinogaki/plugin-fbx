//! Scenes from the committed exports: the values read are the values in the
//! files, transforms place nodes where the writing application had them,
//! and axes and units convert.
mod common;

use common::fixture;
use kinogaki_fbx::fbx::{
    self, Attribute, Axes, Camera, Error, Family, LightKind, Mapping, Matrix, Projection, Scene,
    Wrap,
};

fn read(name: &str) -> Scene {
    fbx::read(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

fn close3(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|i| close(a[i], b[i], tolerance))
}

fn close_matrix(a: &Matrix, b: &Matrix, tolerance: f64) -> bool {
    (0..4).all(|r| (0..4).all(|c| close(a.0[r][c], b.0[r][c], tolerance)))
}

fn model<'s>(scene: &'s Scene, name: &str) -> (usize, &'s fbx::Model) {
    scene
        .models
        .iter()
        .enumerate()
        .find(|(_, m)| m.name == name)
        .unwrap_or_else(|| panic!("no node {name}"))
}

/// The 8 × 8 checker the fixture tools write, as the PNG they embed.
fn is_the_checker(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes[16..24] == [0, 0, 0, 8, 0, 0, 0, 8]
}

#[test]
fn a_houdini_box_reads_as_its_file_says() {
    let scene = read("houdini/box_7700.fbx");
    assert_eq!(scene.version, 7700);
    assert!(scene.creator.starts_with("FBX SDK"));
    let (_, node) = model(&scene, "box");
    let Attribute::Mesh(m) = node.attribute else {
        panic!("box shows {:?}", node.attribute)
    };
    let mesh = &scene.meshes[m];
    assert_eq!(mesh.point_count(), 8);
    assert_eq!(mesh.point(1), [0.5, -0.5, -0.5]);
    assert_eq!(mesh.polygon_count(), 6);
    // `0,4,5,-2`: the first quad, its last index written as !1.
    assert_eq!(&mesh.corners[mesh.polygon(0)], &[0, 4, 5, 1]);

    let normals = mesh.normals.as_ref().unwrap();
    assert_eq!(
        (
            normals.mapping,
            normals.indices.is_none(),
            normals.name.as_str()
        ),
        (Mapping::Corner, true, "N")
    );
    assert_eq!(normals.get(4), &[1.0, 0.0, 0.0]);

    let names: Vec<&str> = mesh.uvs.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["uv", "uv2"]);
    let uv = &mesh.uvs[0];
    assert_eq!(uv.values, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0]);
    assert_eq!(&uv.indices.as_ref().unwrap()[..6], &[0, 1, 2, 3, 3, 2]);
    assert_eq!(uv.get(4), &[1.0, 0.0]);

    let colors = &mesh.colors[0];
    assert_eq!(
        (colors.name.as_str(), colors.mapping, colors.width),
        ("Cd", Mapping::Point, 4)
    );
    for p in 0..8 {
        let c = colors.get(p);
        let t = p as f64 / 7.0;
        assert!(
            close(c[0], t, 1e-7) && c[1] == 0.5 && close(c[2], 1.0 - t, 1e-7) && c[3] == 1.0,
            "{p}: {c:?}"
        );
    }

    let materials = mesh.materials.as_ref().unwrap();
    assert_eq!(
        (materials.mapping, materials.values.as_slice()),
        (Mapping::Polygon, &[0, 0, 0, 1, 1, 1][..])
    );
    let slots: Vec<&str> = node
        .materials
        .iter()
        .map(|&i| scene.materials[i].name.as_str())
        .collect();
    assert_eq!(slots, ["red", "blue"]);
    let red = &scene.materials[node.materials[0]];
    assert_eq!(
        (red.shading_model.as_str(), red.family),
        ("phong", Family::Phong)
    );
    assert_eq!(red.properties.vec3("DiffuseColor"), Some([0.8, 0.1, 0.05]));
    assert_eq!(red.properties.number("ShininessExponent"), Some(60.0));
    assert_eq!(red.textures.len(), 1);
    assert_eq!(red.textures[0].property, "DiffuseColor");
    let texture = &scene.textures[red.textures[0].texture];
    assert_eq!(
        (texture.relative_file_name.as_str(), texture.wrap),
        ("/checker.png", [Wrap::Repeat; 2])
    );
    assert!(scene.videos[texture.video.unwrap()].content.is_empty());
}

#[test]
fn houdini_cameras_and_lights_read_with_their_units() {
    let scene = read("houdini/box_7700.fbx");
    let camera = &scene.cameras[0];
    assert_eq!(
        (camera.projection, camera.focal_length),
        (Projection::Perspective, 35.0)
    );
    assert!(
        close(camera.film_size[0], 36.0, 1e-9),
        "{:?}",
        camera.film_size
    );
    assert_eq!(
        (camera.aspect, camera.clip),
        ([1920.0, 1080.0], [0.001, 10000.0])
    );
    let light = |name: &str| scene.lights.iter().find(|l| l.name == name).unwrap();
    let point = light("point_light");
    assert_eq!(
        (point.kind, point.color, point.intensity),
        (LightKind::Point, [1.0, 0.9, 0.8], 250.0)
    );
    assert_eq!(light("spot_light").cone[1], 40.0);
    assert_eq!(light("distant_light").kind, LightKind::Directional);
}

#[test]
fn transforms_place_nodes_where_houdini_has_them() {
    // Houdini's own world transforms of the scene (`worldTransform()`), by
    // columns acting on points: the rig rotated and scaled non-uniformly,
    // the box under it rotated ZXY about a pivot.
    let houdini = [
        (
            "rig",
            [
                [1.732050807569, 0.0, 0.5, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.866025403784, 0.0],
            ],
        ),
        (
            "box",
            [
                [
                    1.353668108584,
                    -0.568548641419,
                    1.046104724272,
                    0.189191349493,
                ],
                [
                    0.492403876506,
                    0.852868531952,
                    -0.173648177667,
                    0.753798061747,
                ],
                [
                    -1.029351133973,
                    0.688893044155,
                    0.464610177143,
                    0.014675566986,
                ],
            ],
        ),
    ];
    for name in [
        "houdini/box_7700.fbx",
        "houdini/box_7700_ascii.fbx",
        "houdini/box_7100.fbx",
    ] {
        let scene = read(name);
        let world = scene.world_matrices();
        for (node, rows) in houdini {
            let (i, _) = model(&scene, node);
            let expected = Matrix([rows[0], rows[1], rows[2], [0.0, 0.0, 0.0, 1.0]]);
            assert!(
                close_matrix(&world[i], &expected, 1e-9),
                "{name} {node}: {:?}",
                world[i]
            );
        }
        // Houdini's camera looks down its −Z, its distant light shines down
        // its −Z: FBX's +X look and −Y light, turned by post-rotations.
        let (camera, _) = model(&scene, "camera");
        assert!(close3(
            world[camera].vector(Camera::LOOK),
            [0.0, 0.0, -1.0],
            1e-12
        ));
        let (sun, _) = model(&scene, "distant_light");
        let down = world[sun].vector(fbx::Light::DIRECTION);
        assert!(
            close3(
                down,
                [
                    -0.353553390593,
                    -std::f64::consts::FRAC_1_SQRT_2,
                    -0.612372435696
                ],
                1e-9
            ),
            "{down:?}"
        );
    }
}

#[test]
fn every_fbx7_version_reads_the_same_scene() {
    let reference = read("houdini/box_7700.fbx");
    for version in [7100, 7200, 7300, 7400, 7500] {
        let scene = read(&format!("houdini/box_{version}.fbx"));
        assert_eq!(scene.version, version);
        // Ids are the exporter's own each time.
        let meshes = |s: &Scene| {
            s.meshes
                .iter()
                .map(|m| fbx::Mesh { id: 0, ..m.clone() })
                .collect::<Vec<_>>()
        };
        assert_eq!(meshes(&scene), meshes(&reference), "{version}");
        let transforms = |s: &Scene| {
            s.models
                .iter()
                .map(|m| (m.name.clone(), m.transform, m.materials.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(transforms(&scene), transforms(&reference), "{version}");
        let colors = |s: &Scene| {
            s.materials
                .iter()
                .map(|m| m.properties.vec3("DiffuseColor"))
                .collect::<Vec<_>>()
        };
        assert_eq!(colors(&scene), colors(&reference), "{version}");
    }
}

#[test]
fn fbx6_is_refused_with_a_message() {
    match fbx::read(&fixture("houdini/box_6100.fbx")) {
        Err(Error::Unsupported(message)) => assert!(message.contains("FBX 6.1"), "{message}"),
        other => panic!("{:?}", other.map(|s| s.version)),
    }
}

#[test]
fn embedded_media_keeps_the_file_bytes() {
    let binary = read("houdini/box_embedded.fbx");
    let ascii = read("houdini/box_embedded_ascii.fbx");
    assert!(is_the_checker(&binary.videos[0].content));
    assert_eq!(binary.videos[0].content, ascii.videos[0].content);

    let scene = read("blender/textured.fbx");
    let material = &scene.materials[0];
    let links: Vec<&str> = material
        .textures
        .iter()
        .map(|l| l.property.as_str())
        .collect();
    assert_eq!(links, ["DiffuseColor", "NormalMap"]);
    for link in &material.textures {
        let texture = &scene.textures[link.texture];
        assert_eq!(texture.relative_file_name, "textured.fbm/checker.png");
        assert!(is_the_checker(
            &scene.videos[texture.video.unwrap()].content
        ));
    }
    let mesh = &scene.meshes[0];
    assert_eq!(mesh.materials.as_ref().unwrap().mapping, Mapping::All);
}

#[test]
fn blender_layers_read_with_their_mappings() {
    let scene = read("blender/mesh.fbx");
    assert!(scene.creator.starts_with("Blender"));
    let mesh = &scene.meshes[0];
    let uvs: Vec<&str> = mesh.uvs.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(uvs, ["UVMap", "Second"]);
    let colors: Vec<&str> = mesh.colors.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(colors, ["Corner", "Point"]);
    // A point color attribute, written by corner through indices.
    let point = &mesh.colors[1];
    assert_eq!((point.mapping, point.value_count()), (Mapping::Corner, 8));
    for c in 0..mesh.corners.len() {
        let expected = 0.1 * f64::from(mesh.corners[c]);
        assert!(close(point.get(c)[0], expected, 1e-6), "corner {c}");
    }
    assert!(close(mesh.colors[0].get(0)[1], 0.25, 1e-3));

    assert_eq!(mesh.materials.as_ref().unwrap().values, [0, 0, 0, 1, 1, 1]);
    let smoothing = mesh.smoothing.as_ref().unwrap();
    assert_eq!(smoothing.mapping, Mapping::Edge);
    assert_eq!(
        smoothing.values.iter().filter(|&&s| s == 0).count(),
        4,
        "four sharp edges"
    );
    // Every edge joins two points of the 2 m cube along one axis.
    assert_eq!(mesh.edges.len(), 12);
    for e in 0..12 {
        let [a, b] = mesh.edge_points(e);
        let (a, b) = (mesh.point(a as usize), mesh.point(b as usize));
        let differ: Vec<f64> = (0..3)
            .map(|i| (a[i] - b[i]).abs())
            .filter(|&d| d > 1e-9)
            .collect();
        assert_eq!(differ, [2.0], "edge {e}");
    }
    assert!(mesh.tangents.is_some() && mesh.binormals.is_some());
    let red = &scene.materials[0];
    assert_eq!((red.name.as_str(), red.family), ("Red", Family::Phong));
    assert!(close3(
        red.properties.vec3("DiffuseColor").unwrap(),
        [0.8, 0.05, 0.02],
        1e-7
    ));
}

#[test]
fn blender_cameras_and_lights_read_with_their_units() {
    let scene = read("blender/scene.fbx");
    let camera = |name: &str| {
        let (_, node) = model(&scene, name);
        let Attribute::Camera(c) = node.attribute else {
            panic!("{name}")
        };
        &scene.cameras[c]
    };
    let perspective = camera("Perspective");
    assert_eq!(
        (perspective.projection, perspective.focal_length),
        (Projection::Perspective, 50.0)
    );
    assert!(
        close(perspective.film_size[0], 36.0, 1e-9) && close(perspective.film_size[1], 24.0, 1e-9)
    );
    // A shift of 0.1 of the 36 mm film.
    assert!(
        close(perspective.film_offset[0], 3.6, 1e-6),
        "{:?}",
        perspective.film_offset
    );
    let ortho = camera("Orthographic");
    assert_eq!(
        (ortho.projection, ortho.ortho_zoom),
        (Projection::Orthographic, 6.0)
    );

    let light = |name: &str| scene.lights.iter().find(|l| l.name == name).unwrap();
    assert_eq!(light("Sun").kind, LightKind::Directional);
    assert_eq!(light("Area").kind, LightKind::Area);
    let spot = light("Spot");
    assert_eq!(spot.kind, LightKind::Spot);
    // Blender's 45° spot with a blend of 0.15.
    assert!(
        close(spot.cone[0], 38.25, 1e-5) && close(spot.cone[1], 45.0, 1e-5),
        "{:?}",
        spot.cone
    );
    assert!(close3(light("Point").color, [1.0, 0.9, 0.8], 1e-7));

    let (child, node) = model(&scene, "Child");
    assert_eq!(
        node.parent.map(|p| scene.models[p].name.as_str()),
        Some("Rig")
    );
    assert_eq!(node.transform.inherit, fbx::Inherit::Matrix);
    let world = scene.world_matrices();
    let parent = world[node.parent.unwrap()];
    assert!(close_matrix(
        &world[child],
        &(parent * node.transform.matrix()),
        1e-9
    ));
}

#[test]
fn a_z_up_file_converts_onto_the_y_up_one() {
    let y_up = read("houdini/box_7700.fbx");
    let z_up = read("houdini/box_zup.fbx");
    assert_eq!(z_up.settings.axes, Axes::Z_UP);
    let into_y = z_up.settings.conversion(&Axes::Y_UP, 1.0);
    assert!(close(into_y.determinant(), 1.0, 1e-12));
    let (from, to) = (z_up.world_matrices(), y_up.world_matrices());
    for (i, node) in y_up.models.iter().enumerate() {
        let (j, _) = model(&z_up, &node.name);
        assert!(
            close_matrix(&(into_y * from[j]), &to[i], 1e-9),
            "{}",
            node.name
        );
    }
    assert_eq!(z_up.meshes[0].points, y_up.meshes[0].points);
}

#[test]
fn axes_and_units_read_from_global_settings() {
    let left = read("houdini/box_yup_left.fbx");
    assert!(!left.settings.axes.is_right_handed());
    assert!(
        left.settings.conversion(&Axes::Y_UP, 1.0).determinant() < 0.0,
        "a mirroring"
    );

    let blender = read("blender/zup_cm.fbx");
    assert_eq!(
        (blender.settings.axes, blender.settings.unit),
        (Axes::Z_UP, 1.0)
    );
    // Centimetres into metres, Z up into Y up: (10, 20, 30) cm.
    let (i, _) = model(&blender, "Centimetres");
    let placed = blender.settings.conversion(&Axes::Y_UP, 100.0) * blender.world_matrices()[i];
    assert!(
        close3(placed.point([0.0; 3]), [0.1, 0.3, -0.2], 1e-12),
        "{:?}",
        placed.point([0.0; 3])
    );
}

#[test]
fn every_fixture_reads_and_says_what_it_leaves_out() {
    for name in common::fixtures() {
        match fbx::read(&fixture(&name)) {
            Ok(scene) => assert!(!scene.models.is_empty(), "{name}"),
            Err(Error::Unsupported(_)) => assert!(name.contains("6100"), "{name}"),
            Err(e) => panic!("{name}: {e}"),
        }
    }
    let blender = read("blender/mesh.fbx");
    assert!(
        blender
            .warnings
            .iter()
            .any(|w| w.contains("tangent layers after the first")),
        "{:?}",
        blender.warnings
    );
}
