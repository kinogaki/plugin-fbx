//! The node tree: binary and ASCII read into the same tree; hostile sizes,
//! damaged and truncated files fail cleanly; reading can be cancelled.

use crate::common::{Random, ascii, binary, fixture, fixtures, node, s};
use kinogaki_fbx::fbx::tree::{self, Array, Format, Node, Value};
use kinogaki_fbx::fbx::{self, Error};
use kinogaki_studio_api::CancelToken;
use kinogaki_studio_api::format::{ReadBudget, ReadLimits};

/// A tree with every kind of value.
fn sample() -> Vec<Node> {
    let header = node(
        "FBXHeaderExtension",
        vec![],
        vec![node("FBXVersion", vec![Value::Int(7400)], vec![])],
    );
    let geometry = node(
        "Geometry",
        vec![Value::Int(42), s("Geometry::Box"), s("Mesh")],
        vec![
            node(
                "Vertices",
                vec![Value::Array(Array::F64(
                    (0..3000).map(|i| f64::from(i) * 0.25 - 7.0).collect(),
                ))],
                vec![],
            ),
            node(
                "PolygonVertexIndex",
                vec![Value::Array(Array::I32(
                    (0..1200).map(|i| if i % 4 == 3 { !i } else { i }).collect(),
                ))],
                vec![],
            ),
            node(
                "Flags",
                vec![Value::Array(Array::Bool(vec![true, false, true]))],
                vec![],
            ),
            node(
                "Wide",
                vec![Value::Array(Array::I64(vec![i64::MAX, -5, 1 << 40]))],
                vec![],
            ),
            node(
                "Narrow",
                vec![Value::Array(Array::F32(vec![0.5, -1.25, 3.0e-3]))],
                vec![],
            ),
            node("Empty", vec![Value::Array(Array::F64(vec![]))], vec![]),
        ],
    );
    let scalars = node(
        "Scalars",
        vec![
            Value::Int(-3),
            Value::Int(1 << 40),
            Value::Float(0.1),
            Value::Float(-2.5e-12),
            s("quoted \"name\""),
            s(""),
        ],
        vec![node(
            "Inner",
            vec![],
            vec![node("Leaf", vec![s("deep")], vec![])],
        )],
    );
    vec![header, node("Objects", vec![], vec![geometry]), scalars]
}

/// `tree` as ASCII reads it: no element types, so integer arrays are 64-bit
/// and floats are 64-bit (a 32-bit float as its shortest spelling).
fn as_ascii_reads(tree: &[Node]) -> Vec<Node> {
    tree.iter()
        .map(|n| Node {
            name: n.name.clone(),
            values: n
                .values
                .iter()
                .map(|v| match v {
                    Value::Array(Array::I32(a)) => {
                        Value::Array(Array::I64(a.iter().map(|&x| x.into()).collect()))
                    }
                    Value::Array(Array::F32(a)) => Value::Array(Array::F64(
                        a.iter()
                            .map(|x| format!("{x:?}").parse().unwrap())
                            .collect(),
                    )),
                    Value::Array(Array::Bool(a)) => {
                        Value::Array(Array::I64(a.iter().map(|&x| x.into()).collect()))
                    }
                    Value::Array(Array::F64(a)) if a.is_empty() => Value::Array(Array::I64(vec![])),
                    other => other.clone(),
                })
                .collect(),
            children: as_ascii_reads(&n.children),
        })
        .collect()
}

#[test]
fn binary_files_read_back_their_tree_in_every_layout() {
    let tree = sample();
    for version in [7100, 7400, 7500, 7700] {
        for compress in [false, true] {
            let document = tree::parse(&binary(&tree, version, compress)).unwrap();
            assert_eq!(document.version, version);
            assert_eq!(document.format, Format::Binary);
            assert_eq!(
                document.nodes, tree,
                "version {version}, compressed {compress}"
            );
        }
    }
}

#[test]
fn ascii_files_read_back_their_tree() {
    let tree = sample();
    let document = tree::parse(ascii(&tree, 7400).as_bytes()).unwrap();
    assert_eq!(document.version, 7400);
    assert_eq!(document.format, Format::Ascii);
    assert_eq!(document.nodes, as_ascii_reads(&tree));
}

#[test]
fn a_long_ascii_array_is_cut_into_tasks_and_joined_in_order() {
    // Integers first and floats later: the pieces agree on floats.
    let values: Vec<f64> = (0..400_000)
        .map(|i| {
            if i < 300_000 {
                f64::from(i)
            } else {
                f64::from(i) + 0.5
            }
        })
        .collect();
    let tree = vec![node(
        "Big",
        vec![Value::Array(Array::F64(values.clone()))],
        vec![],
    )];
    let text = ascii(&tree, 7400).replace(".0,", ",");
    let document = tree::parse(text.as_bytes()).unwrap();
    assert_eq!(
        document.nodes[0].values,
        vec![Value::Array(Array::F64(values))]
    );
}

#[test]
fn ascii_spellings_of_exporters_read() {
    let text = "; FBX 7.7.0 project file\n\
        Node: 1,-2.5e3,1.#INF,-1.#IND, \"a &quot;b&quot;\", Y\n\
        Content: ,\n \"S2lu\", \"b2dha2k=\"\n\
        Empty: *0 {\n}\n\
        Block:  {\n\tChild: \"x\" ; a comment\n}\n";
    let document = tree::parse(text.as_bytes()).unwrap();
    assert_eq!(document.version, 7700);
    let values = &document.nodes[0].values;
    assert_eq!(values[0], Value::Int(1));
    assert_eq!(values[1], Value::Float(-2500.0));
    assert_eq!(values[2], Value::Float(f64::INFINITY));
    assert!(matches!(values[3], Value::Float(v) if v.is_nan()));
    assert_eq!(values[4], s("a \"b\""));
    assert_eq!(values[5], s("Y"));
    assert_eq!(document.nodes[1].values, vec![s("S2lu"), s("b2dha2k=")]);
    assert_eq!(
        document.nodes[2].values,
        vec![Value::Array(Array::I64(vec![]))]
    );
    assert_eq!(document.nodes[3].children[0].values, vec![s("x")]);
}

#[test]
fn binary_and_ascii_exports_of_one_scene_read_alike() {
    for (b, a) in [
        ("houdini/box_7400.fbx", "houdini/box_7400_ascii.fbx"),
        ("houdini/box_7700.fbx", "houdini/box_7700_ascii.fbx"),
    ] {
        let (b, a) = (
            fbx::read(&fixture(b)).unwrap(),
            fbx::read(&fixture(a)).unwrap(),
        );
        assert_eq!(b.meshes.len(), a.meshes.len());
        for (mb, ma) in b.meshes.iter().zip(&a.meshes) {
            assert_eq!((&mb.polygons, &mb.corners), (&ma.polygons, &ma.corners));
            // ASCII keeps 15 significant digits.
            let close = |x: &[f64], y: &[f64]| {
                x.len() == y.len()
                    && x.iter()
                        .zip(y)
                        .all(|(p, q)| (p - q).abs() <= 1e-12 * p.abs().max(1.0))
            };
            assert!(close(&mb.points, &ma.points));
            assert!(close(
                &mb.normals.as_ref().unwrap().values,
                &ma.normals.as_ref().unwrap().values
            ));
            for (ub, ua) in mb.uvs.iter().zip(&ma.uvs) {
                assert_eq!((&ub.name, &ub.indices), (&ua.name, &ua.indices));
                assert!(close(&ub.values, &ua.values));
            }
        }
        let names = |s: &fbx::Scene| s.models.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&b), names(&a));
        assert_eq!(b.materials.len(), a.materials.len());
    }
}

#[test]
fn every_fixture_reads_its_tree() {
    for name in fixtures() {
        let document = tree::parse(&fixture(&name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(document.node("Objects").is_some(), "{name}");
    }
}

#[test]
fn hostile_array_headers_fail_before_allocating() {
    let tree = vec![node(
        "A",
        vec![Value::Array(Array::F64(vec![1.0, 2.0, 3.0]))],
        vec![],
    )];
    for compress in [false, true] {
        let mut bytes = binary(&tree, 7400, compress);
        // The array's count sits after the header, the name and its type.
        let count_at = 27 + 13 + 1 + 1;
        assert_eq!(&bytes[count_at..count_at + 4], &3u32.to_le_bytes());
        for lie in [4u32, 2, 1 << 30, u32::MAX] {
            bytes[count_at..count_at + 4].copy_from_slice(&lie.to_le_bytes());
            let error = tree::parse(&bytes).unwrap_err();
            assert!(matches!(error, Error::Malformed { .. }), "{lie}: {error}");
        }
    }
    let small = ReadBudget::new(ReadLimits {
        allocation: 16,
        ..ReadLimits::default()
    });
    let error = tree::parse_with(&binary(&tree, 7400, true), &small).unwrap_err();
    assert!(error.to_string().contains("budget"), "{error}");
    let ascii_lie = "; FBX 7.4.0 project file\nA: *3 {\n\ta: 1,2\n}\n";
    assert!(matches!(
        tree::parse(ascii_lie.as_bytes()),
        Err(Error::Malformed { .. })
    ));
}

#[test]
fn deep_nesting_passes_the_depth_budget_as_an_error() {
    let mut deep = node("Leaf", vec![], vec![]);
    for _ in 0..300 {
        deep = node("N", vec![], vec![deep]);
    }
    for bytes in [
        binary(std::slice::from_ref(&deep), 7400, false),
        ascii(std::slice::from_ref(&deep), 7400).into_bytes(),
    ] {
        let error = tree::parse(&bytes).unwrap_err();
        assert!(error.to_string().contains("deeper"), "{error}");
    }
}

#[test]
fn truncated_files_fail_cleanly() {
    for name in [
        "houdini/box_7400.fbx",
        "houdini/box_7700.fbx",
        "houdini/box_7400_ascii.fbx",
        "blender/textured.fbx",
    ] {
        let bytes = fixture(name);
        let full = fbx::read(&bytes).unwrap();
        let ascii = full.format == Format::Ascii;
        for len in (0..bytes.len()).step_by(97) {
            // A binary file cut in its footer loses nothing it needs; an
            // ASCII file cut between nodes is a shorter file.
            if let Ok(scene) = fbx::read(&bytes[..len]) {
                if ascii {
                    assert!(scene.meshes.len() <= full.meshes.len());
                } else {
                    assert_eq!(scene, full, "{name} cut at {len}");
                }
            }
        }
    }
}

#[test]
fn mutated_files_never_panic() {
    let mut random = Random(0x5eed_f00d);
    for name in fixtures() {
        let original = fixture(&name);
        for _ in 0..200 {
            let mut bytes = original.clone();
            for _ in 0..1 + random.below(8) {
                let at = random.below(bytes.len());
                bytes[at] = random.next() as u8;
            }
            let _ = fbx::read(&bytes);
        }
    }
}

#[test]
fn reading_stops_when_cancelled() {
    let token = CancelToken::default();
    token.cancel();
    let cancelled = ReadBudget::default().cancelled_by(token);
    for name in ["houdini/box_7700.fbx", "houdini/box_7700_ascii.fbx"] {
        assert!(matches!(
            fbx::read_with(&fixture(name), &cancelled),
            Err(Error::Cancelled)
        ));
    }
}

#[test]
fn what_is_not_fbx_says_so() {
    assert!(matches!(
        tree::parse(b"\x89PNG\r\n\x1a\n"),
        Err(Error::Unsupported(_))
    ));
    let mut newer = binary(&sample(), 7700, false);
    newer[23..27].copy_from_slice(&8000u32.to_le_bytes());
    assert!(matches!(tree::parse(&newer), Err(Error::Unsupported(_))));
}
