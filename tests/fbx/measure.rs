//! The format's budget (render-agnostic PLAN, wave 10): a binary FBX of a
//! million triangles read into the standard values in at most 120 ms, its
//! arrays inflated in parallel, ASCII in at most three times that; release,
//! best of seven on workers a thread a core, as Studio installs them. The
//! file is synthetic, shaped as Blender writes one (points, polygons, edges,
//! normals and UVs by corner through indices, smoothing by edge, materials).
use crate::common::{ascii, binary, node, s};
use kinogaki_fbx::Fbx;
use kinogaki_fbx::fbx::tree::{Array, Node, Value};
use kinogaki_studio_api::geometry::parallel::{self, Workers};
use kinogaki_studio_api::geometry::{Units, UpAxis};
use kinogaki_studio_api::{FileFormat, Input, Purpose, Reading};
use kinogaki_studio_api_testing::Unconverted;
use std::ops::Range;
use std::panic::Location;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The workers a host installs: the calling thread and scoped helpers, one
/// a core, taking ranges from a shared counter.
struct Pool(usize);

impl Workers for Pool {
    fn run(
        &self,
        _: &'static Location<'static>,
        len: usize,
        minimum: usize,
        work: &(dyn Fn(Range<usize>) -> bool + Sync),
    ) {
        let step = minimum.max(1);
        let threads = self.0.min(len.div_ceil(step));
        if threads <= 1 {
            if len > 0 {
                work(0..len);
            }
            return;
        }
        let (next, stop) = (AtomicUsize::new(0), AtomicBool::new(false));
        let take = || {
            while !stop.load(Ordering::Relaxed) {
                let start = next.fetch_add(step, Ordering::Relaxed);
                if start >= len {
                    break;
                }
                if !work(start..(start + step).min(len)) {
                    stop.store(true, Ordering::Relaxed);
                }
            }
        };
        std::thread::scope(|scope| {
            (1..threads).for_each(|_| _ = scope.spawn(take));
            take();
        });
    }
}

/// The best of seven reads of `bytes` into the standard values.
fn best(bytes: &[u8]) -> Duration {
    let units = Units::new(1.0, UpAxis::Y).unwrap();
    let reading = Reading::new(Purpose::Objects, units, &Unconverted);
    (0..7)
        .map(|_| {
            let start = Instant::now();
            let read = Fbx.read(&Input::memory("grid.fbx", bytes), &Fbx.options(), &reading);
            assert!(read.unwrap().items.iter().any(|i| i.data.is_some()));
            start.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
#[ignore = "hand: a release measurement of a million triangles"]
fn hand_run_a_million_triangles_read_within_the_budget() {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    parallel::install(Box::leak(Box::new(Pool(threads))));
    let mesh = grid(1000, 500);
    let (binary_file, ascii_file) = (binary(&mesh, 7400, true), ascii(&mesh, 7400));
    let (b, a) = (best(&binary_file), best(ascii_file.as_bytes()));
    println!("MEASURE fbx 1M triangles: binary {b:.1?}, ASCII {a:.1?}");
    assert!(b <= Duration::from_millis(120), "binary {b:?}");
    assert!(a <= Duration::from_millis(360), "ASCII {a:?}");
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
