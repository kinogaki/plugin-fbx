//! Times reading: a file's tree (arrays inflated or parsed side by side)
//! and its scene, best of several runs. With no file, a synthetic mesh of a
//! million triangles shaped as Blender writes one (points, polygons, edges,
//! indexed normals and UVs, smoothing by edge, materials), binary and ASCII.
//!
//! ```sh
//! cargo run --release --example bench              # the synthetic mesh
//! cargo run --release --example bench -- big.fbx   # a file
//! ```
#[path = "../tests/common/mod.rs"]
mod common;

use common::{node, s};
use kinogaki_fbx::fbx::tree::{self, Array, Node, Value};
use kinogaki_fbx::fbx::{Options, Scene};
use std::time::{Duration, Instant};

const RUNS: usize = 7;

fn main() {
    match std::env::args().nth(1) {
        Some(path) => time(&path, &std::fs::read(&path).expect("the file")),
        None => {
            let mesh = grid(1000, 500);
            time(
                "1M triangles, binary 7.4",
                &common::binary(&mesh, 7400, true),
            );
            time(
                "1M triangles, ASCII 7.4",
                common::ascii(&mesh, 7400).as_bytes(),
            );
        }
    }
}

fn time(what: &str, bytes: &[u8]) {
    let (mut parse, mut build) = (Duration::MAX, Duration::MAX);
    let mut triangles = 0;
    for _ in 0..RUNS {
        let start = Instant::now();
        let document = tree::parse(bytes).expect("reads");
        let parsed = Instant::now();
        let scene = Scene::from_document(document, &Options::default()).expect("a scene");
        let done = Instant::now();
        parse = parse.min(parsed - start);
        build = build.min(done - parsed);
        triangles = scene
            .meshes
            .iter()
            .map(|m| m.corners.len() - 2 * m.polygon_count())
            .sum::<usize>();
    }
    println!(
        "{what}: {:.1} MB, {triangles} triangles: tree {:.1} ms + scene {:.1} ms = {:.1} ms",
        bytes.len() as f64 / 1e6,
        parse.as_secs_f64() * 1e3,
        build.as_secs_f64() * 1e3,
        (parse + build).as_secs_f64() * 1e3
    );
}

/// A grid of `w × h` quads, each two triangles, in an FBX tree.
fn grid(w: usize, h: usize) -> Vec<Node> {
    let point = |x: usize, y: usize| (y * (w + 1) + x) as i32;
    let mut points = Vec::with_capacity((w + 1) * (h + 1) * 3);
    for y in 0..=h {
        for x in 0..=w {
            let (u, v) = (x as f64 / w as f64, y as f64 / h as f64);
            points.extend([u * 10.0, v * 5.0, (u * 40.0).sin() * (v * 30.0).cos() * 0.2]);
        }
    }
    let mut corners = Vec::with_capacity(w * h * 6);
    for y in 0..h {
        for x in 0..w {
            let (a, b, c, d) = (
                point(x, y),
                point(x + 1, y),
                point(x + 1, y + 1),
                point(x, y + 1),
            );
            corners.extend([a, b, !c, a, c, !d]);
        }
    }
    let triangles = corners.len() / 3;
    // Normals and UVs by corner through indices, as Blender writes them.
    let normals: Vec<f64> = (0..points.len() / 3)
        .flat_map(|i| [0.0, (i as f64 * 1e-3).sin() * 0.1, 1.0])
        .collect();
    let uvs: Vec<f64> = (0..=h)
        .flat_map(|y| (0..=w).flat_map(move |x| [x as f64 / w as f64, y as f64 / h as f64]))
        .collect();
    let corner_points: Vec<i32> = corners
        .iter()
        .map(|&c| if c < 0 { !c } else { c })
        .collect();
    let edges: Vec<i32> = (0..(corners.len() / 2) as i32).map(|e| e * 2).collect();
    let layer = |kind: &str, mapping: &str, arrays: Vec<(&str, Value)>| {
        let mut children = vec![
            node("Version", vec![Value::Int(101)], vec![]),
            node("Name", vec![s("")], vec![]),
            node("MappingInformationType", vec![s(mapping)], vec![]),
            node(
                "ReferenceInformationType",
                vec![s(if arrays.len() > 1 {
                    "IndexToDirect"
                } else {
                    "Direct"
                })],
                vec![],
            ),
        ];
        children.extend(
            arrays
                .into_iter()
                .map(|(name, value)| node(name, vec![value], vec![])),
        );
        node(kind, vec![Value::Int(0)], children)
    };
    let geometry = node(
        "Geometry",
        vec![Value::Int(1), s("Geometry::Grid"), s("Mesh")],
        vec![
            node("Vertices", vec![Value::Array(Array::F64(points))], vec![]),
            node(
                "PolygonVertexIndex",
                vec![Value::Array(Array::I32(corners))],
                vec![],
            ),
            node(
                "Edges",
                vec![Value::Array(Array::I32(edges.clone()))],
                vec![],
            ),
            node("GeometryVersion", vec![Value::Int(124)], vec![]),
            layer(
                "LayerElementNormal",
                "ByPolygonVertex",
                vec![
                    ("Normals", Value::Array(Array::F64(normals))),
                    (
                        "NormalsIndex",
                        Value::Array(Array::I32(corner_points.clone())),
                    ),
                ],
            ),
            layer(
                "LayerElementUV",
                "ByPolygonVertex",
                vec![
                    ("UV", Value::Array(Array::F64(uvs))),
                    ("UVIndex", Value::Array(Array::I32(corner_points))),
                ],
            ),
            layer(
                "LayerElementSmoothing",
                "ByEdge",
                vec![("Smoothing", Value::Array(Array::I32(vec![1; edges.len()])))],
            ),
            layer(
                "LayerElementMaterial",
                "ByPolygon",
                vec![("Materials", Value::Array(Array::I32(vec![0; triangles])))],
            ),
        ],
    );
    let model = node(
        "Model",
        vec![Value::Int(2), s("Model::Grid"), s("Mesh")],
        vec![],
    );
    let connections = vec![
        node("C", vec![s("OO"), Value::Int(2), Value::Int(0)], vec![]),
        node("C", vec![s("OO"), Value::Int(1), Value::Int(2)], vec![]),
    ];
    vec![
        node(
            "FBXHeaderExtension",
            vec![],
            vec![node("FBXVersion", vec![Value::Int(7400)], vec![])],
        ),
        node("Objects", vec![], vec![geometry, model]),
        node("Connections", vec![], connections),
    ]
}
