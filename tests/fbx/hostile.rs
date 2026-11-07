//! Hostile files: each shape that once asked FBX's reader for far more
//! than a file holds, took it seconds, or reached files outside the file's
//! folder, read as File ▸ Import reads one (`read_hostile`: a 2 MiB stack
//! and a budget), ending in its items, a clean error or a warning within a
//! time bound.
use crate::common::{binary, node, p, s};
use crate::graph::{document, floats, int, ints, material, object, oo, op, triangle};
use kinogaki_fbx::Fbx;
use kinogaki_fbx::fbx::tree::{self, Node};
use kinogaki_fbx::fbx::{self, Error};
use kinogaki_studio_api::format::options::EMBED;
use kinogaki_studio_api::format::{ReadBudget, ReadLimits};
use kinogaki_studio_api::{FileFormat, FormatOptions, Imported, Input, Value};
use kinogaki_studio_api_testing::format::Scratch;
use kinogaki_studio_api_testing::read_hostile;
use std::path::Path;
use std::time::Duration;

/// How long a hostile read may take in a debug build: seconds where the
/// shapes once took tens of seconds.
const BOUND: Duration = Duration::from_secs(10);

/// `bytes` read as a file held in memory within `limits`, in its time.
fn read(bytes: &[u8], limits: ReadLimits) -> Result<Imported, kinogaki_studio_api::Error> {
    let input = Input::memory("hostile.fbx", bytes);
    let (read, took) = read_hostile(&Fbx, &input, &Fbx.options(), limits);
    assert!(took < BOUND, "{took:?}");
    read
}

/// The file at `path` read with `options` set over the defaults.
fn read_file(path: &Path, options: &[(&str, bool)]) -> Imported {
    let mut chosen: FormatOptions = Fbx.options();
    for (name, on) in options {
        chosen.insert((*name).into(), Value::Bool(*on));
    }
    let (read, took) = read_hostile(&Fbx, &Input::file(path), &chosen, ReadLimits::default());
    assert!(took < BOUND, "{took:?}");
    read.unwrap()
}

/// A budget of `total` bytes in all.
fn total(total: u64) -> ReadLimits {
    ReadLimits {
        total,
        ..ReadLimits::default()
    }
}

#[test]
fn a_compressed_array_is_charged_as_it_inflates_and_as_its_elements() {
    // A megabyte of real zlib once peaked at twice its 1 GB of elements,
    // within budgets of 4 GiB an array and 32 GiB in all.
    let zeros = node("Big", vec![floats(&vec![0.0; 2 << 20])], vec![]);
    let bytes = binary(&[zeros], 7400, true);
    assert!(bytes.len() < 100_000, "{}", bytes.len());
    let within = |limits| tree::parse_with(&bytes, &ReadBudget::new(limits));
    assert!(
        within(total(40 << 20)).is_ok(),
        "16 MiB inflated beside 16 MiB of elements"
    );
    let refused = within(total(24 << 20)).unwrap_err();
    assert!(matches!(&refused, Error::TooLarge(why) if why.contains("over the read's budget")));
    // A count its compressed bytes can't hold is a broken file.
    let mut lying = bytes.clone();
    let count = 27 + 13 + 3 + 1;
    assert_eq!(&lying[count..count + 4], &(2u32 << 20).to_le_bytes());
    lying[count..count + 4].copy_from_slice(&(1u32 << 30).to_le_bytes());
    assert!(matches!(tree::parse(&lying), Err(Error::Malformed { .. })));
}

#[test]
fn twenty_thousand_materials_of_one_name_are_keyed_in_linear_time() {
    // 809 KB once took 10 s: each key was probed past every copy before it.
    let materials = (0..20_000)
        .map(|i| material(1000 + i, "M", "phong", vec![]))
        .collect();
    let bytes = binary(&document(materials, vec![]), 7400, true);
    let read = read(&bytes, ReadLimits::default()).unwrap();
    let held = kinogaki_studio_api::standard::materials()
        .get(read.items[0].data.as_ref().unwrap())
        .unwrap();
    assert_eq!(held.materials[19_999].key, "M_19999");
}

#[test]
fn thousands_of_templates_are_matched_without_copying_their_names() {
    // 757 KB once took 2.7 s: every template's name lowercased for every
    // object.
    let template = |n: usize| node("PropertyTemplate", vec![s(&format!("FbxThing{n}"))], vec![]);
    let mut tree = document(
        (0..5_000)
            .map(|i| object("Model", 10 + i, "N", "Null", vec![], vec![]))
            .collect(),
        (0..5_000).map(|i| oo(10 + i, 0)).collect(),
    );
    let definitions = tree.iter_mut().find(|n| n.name == "Definitions").unwrap();
    let models = node(
        "ObjectType",
        vec![s("Model")],
        (0..5_000).map(template).collect(),
    );
    definitions.children.push(models);
    let read = read(&binary(&tree, 7400, true), ReadLimits::default()).unwrap();
    assert_eq!(read.items.len(), 5_000);
}

/// A chain of `depth` nodes.
fn nested(depth: usize) -> Node {
    (0..depth).fold(node("Leaf", vec![], vec![]), |inner, _| {
        node("N", vec![], vec![inner])
    })
}

#[test]
fn nesting_reads_to_the_budgets_depth_on_imports_stack_and_no_deeper() {
    let deep = binary(&[nested(255)], 7400, false);
    assert!(read(&deep, ReadLimits::default()).is_ok());
    let deeper = binary(&[nested(300)], 7400, false);
    let why = read(&deeper, ReadLimits::default()).unwrap_err();
    assert!(why.message().contains("nested deeper than 256"), "{why}");
}

#[test]
fn a_polygon_of_two_hundred_thousand_corners_is_checked_in_n_log_n() {
    let n = 200_000;
    let points: Vec<f64> = (0..n)
        .flat_map(|k| [k as f64, (k % 7) as f64, 0.0])
        .collect();
    let mut corners: Vec<i32> = (0..n as i32).collect();
    corners[n - 1] = !corners[n - 1];
    let mesh = node(
        "Geometry",
        vec![int(100), s("Geometry::Ring"), s("Mesh")],
        vec![
            node("Vertices", vec![floats(&points)], vec![]),
            node("PolygonVertexIndex", vec![ints(&corners)], vec![]),
        ],
    );
    let model = object("Model", 1, "Ring", "Mesh", vec![], vec![]);
    let bytes = binary(
        &document(vec![mesh, model], vec![oo(1, 0), oo(100, 1)]),
        7400,
        true,
    );
    assert!(read(&bytes, ReadLimits::default()).is_ok());
}

#[test]
fn a_mesh_copied_for_each_node_is_charged_for_each() {
    // 378 KB once made 312 MB: a mesh shown by hundreds of nodes, each in
    // materials of its own, made again for each. Nodes giving its two slots
    // two others hold it once and map them; nodes giving it another count
    // of slots than the first still copy it, each charged.
    let n = 20_000;
    let points: Vec<f64> = (0..n)
        .flat_map(|k| [k as f64, 0.0, (k % 3) as f64])
        .collect();
    let corners: Vec<i32> = (0..n - 2).flat_map(|k| [k, k + 1, !(k + 2)]).collect();
    let slots: Vec<i32> = (0..n - 2).map(|k| k % 2).collect();
    let layer = node(
        "LayerElementMaterial",
        vec![int(0)],
        vec![
            node("MappingInformationType", vec![s("ByPolygon")], vec![]),
            node("ReferenceInformationType", vec![s("IndexToDirect")], vec![]),
            node("Materials", vec![ints(&slots)], vec![]),
        ],
    );
    let mesh = node(
        "Geometry",
        vec![int(100), s("Geometry::Strip"), s("Mesh")],
        vec![
            node("Vertices", vec![floats(&points)], vec![]),
            node("PolygonVertexIndex", vec![ints(&corners)], vec![]),
            layer,
        ],
    );
    // 300 nodes, each in two materials of its own, past the first in
    // `more` more.
    let file = |more: usize| {
        let mut objects = vec![mesh.clone()];
        let mut links = Vec::new();
        for k in 0..300 {
            let id = 1_000 + 4 * k;
            objects.push(object(
                "Model",
                id,
                &format!("M{k}"),
                "Mesh",
                vec![],
                vec![],
            ));
            let own = if k == 0 { 2 } else { 2 + more };
            for m in 0..own {
                let name = format!("{}{k}", ["A", "B", "C"][m]);
                objects.push(material(id + 1 + m as i64, &name, "phong", vec![]));
                links.push(oo(id + 1 + m as i64, id));
            }
            links.extend([oo(id, 0), oo(100, id)]);
        }
        binary(&document(objects, links), 7400, true)
    };
    assert!(read(&file(0), total(64 << 20)).is_ok(), "held once, mapped");
    let why = read(&file(1), total(64 << 20)).unwrap_err();
    assert!(why.message().contains("over the read's budget"), "{why}");
}

/// A new folder holding `scene/` (where the file is read), by its
/// canonical path.
fn scratch(name: &str) -> Scratch {
    let dir = Scratch::new(&format!("fbx-hostile-{name}"));
    std::fs::create_dir_all(dir.join("scene")).unwrap();
    dir
}

/// A file whose material reads a texture of each `(FileName,
/// RelativeFilename)`.
fn textured(files: &[(&str, &str)]) -> Vec<u8> {
    let mut objects = vec![
        triangle(1, vec![]),
        object("Model", 2, "Tri", "Mesh", vec![], vec![]),
        material(3, "Look", "phong", vec![]),
    ];
    let mut links = vec![oo(2, 0), oo(1, 2), oo(3, 2)];
    let channels = [
        "DiffuseColor",
        "SpecularColor",
        "EmissiveColor",
        "NormalMap",
        "Bump",
    ];
    for (k, (file, relative)) in files.iter().enumerate() {
        let id = 10 + k as i64;
        objects.push(object(
            "Texture",
            id,
            &format!("T{k}"),
            "",
            vec![p("UVSet", "KString", vec![s("")])],
            vec![
                node("FileName", vec![s(file)], vec![]),
                node("RelativeFilename", vec![s(relative)], vec![]),
            ],
        ));
        links.push(op(id, 3, channels[k]));
    }
    binary(&document(objects, links), 7400, true)
}

#[test]
fn textures_out_of_the_folder_are_read_and_what_isnt_regular_never_is() {
    let dir = scratch("textures");
    std::fs::write(dir.join("secret.png"), b"key").unwrap();
    std::fs::write(dir.join("scene/wood.png"), b"wood").unwrap();
    let pipe = dir.join("scene/pipe.png");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success()
    );
    let file = dir.join("scene/hostile.fbx");
    let files = [
        ("/etc/hosts", ""),
        ("", "../secret.png"),
        ("/anywhere/wood.png", "wood.png"),
        ("", "pipe.png"),
        ("", "/../../etc/hosts"),
    ];
    std::fs::write(&file, textured(&files)).unwrap();
    let read = read_file(&file, &[(EMBED, true)]);
    let said: Vec<&str> = read.warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(
        said.iter().any(|w| w.ends_with("never read: pipe.png")),
        "{said:?}"
    );
    let held = kinogaki_studio_api::standard::materials()
        .get(read.items[0].data.as_ref().unwrap())
        .unwrap();
    let inside = format!("{:?}", held.materials[0].inside);
    for read in ["wood", "hosts", "secret"] {
        assert!(inside.contains(read), "{read}, embedded wherever it is");
    }
    assert!(!inside.contains("pipe"));
}

#[test]
fn a_cancelled_read_stops_inside_the_arrays() {
    let token = kinogaki_studio_api::CancelToken::default();
    token.cancel();
    let budget = ReadBudget::default().cancelled_by(token);
    let zeros = node("Big", vec![floats(&vec![0.0; 1 << 20])], vec![]);
    let bytes = binary(&[zeros], 7400, true);
    assert!(matches!(
        fbx::read_with(&bytes, &budget),
        Err(Error::Cancelled)
    ));
}
