//! Prints what the reader reads from an FBX file, one line a thing: for
//! looking at a file, and for comparing with other readers. `--tree`
//! prints the node tree instead.
//!
//! ```sh
//! cargo run --release --example dump -- model.fbx
//! cargo run --release --example dump -- --tree model.fbx
//! ```
use kinogaki_fbx::fbx::tree::{self, Node, Value};
use kinogaki_fbx::fbx::{self, Attribute, Layer, Scene};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (tree, path) = match args.as_slice() {
        [flag, path] if flag == "--tree" => (true, path),
        [path] => (false, path),
        _ => {
            eprintln!("usage: dump [--tree] <file.fbx>");
            std::process::exit(2);
        }
    };
    let read = std::fs::read(path).map_err(fbx::Error::from);
    let result = if tree {
        read.and_then(|bytes| tree::parse(&bytes))
            .map(|d| d.nodes.iter().for_each(|n| print_node(n, 0)))
    } else {
        read.and_then(|bytes| fbx::read(&bytes))
            .map(|scene| print(&scene))
    };
    if let Err(e) = result {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    }
}

fn print_node(node: &Node, depth: usize) {
    let values: Vec<String> = node
        .values
        .iter()
        .map(|v| match v {
            Value::Array(a) => format!("[{} elements]", a.len()),
            Value::Bytes(b) => format!("<{} bytes>", b.len()),
            other => format!("{other:?}"),
        })
        .collect();
    println!("{}{}: {}", "  ".repeat(depth), node.name, values.join(", "));
    node.children.iter().for_each(|c| print_node(c, depth + 1));
}

fn print(scene: &Scene) {
    let s = &scene.settings;
    println!(
        "version {} {:?} creator {:?}",
        scene.version, scene.format, scene.creator
    );
    println!(
        "axes right {:?} up {:?} front {:?} right-handed {} unit {} original {}",
        s.axes.right,
        s.axes.up,
        s.axes.front,
        s.axes.is_right_handed(),
        s.unit,
        s.original_unit
    );
    let world = scene.world_matrices();
    for (model, world) in scene.models.iter().zip(&world) {
        let parent = model.parent.map_or("-", |p| &scene.models[p].name);
        let attribute = match &model.attribute {
            Attribute::Mesh(m) => format!("mesh {}", scene.meshes[*m].name),
            Attribute::Camera(c) => format!("camera {}", scene.cameras[*c].name),
            Attribute::Light(l) => format!("light {}", scene.lights[*l].name),
            other => format!("{other:?}"),
        };
        let materials: Vec<&str> = model
            .materials
            .iter()
            .map(|&m| scene.materials[m].name.as_str())
            .collect();
        println!(
            "model {} parent {parent} {attribute} materials {materials:?} visible {}",
            model.name, model.visible
        );
        let t = &model.transform;
        println!(
            "  t {:?} r {:?} {:?} s {:?} pre {:?} post {:?} inherit {:?}",
            t.translation,
            t.rotation,
            t.rotation_order,
            t.scaling,
            t.pre_rotation,
            t.post_rotation,
            t.inherit
        );
        println!("  world {:?}", world.0.map(|row| row.map(round)));
    }
    for mesh in &scene.meshes {
        println!(
            "mesh {} points {} polygons {} corners {} edges {}",
            mesh.name,
            mesh.point_count(),
            mesh.polygon_count(),
            mesh.corners.len(),
            mesh.edges.len()
        );
        let layer = |what: &str, l: Option<&Layer<f64>>| {
            if let Some(l) = l {
                println!(
                    "  {what} {:?} {:?} values {} indexed {} first {:?}",
                    l.name,
                    l.mapping,
                    l.value_count(),
                    l.indices.is_some(),
                    l.get(0)
                );
            }
        };
        layer("normals", mesh.normals.as_ref());
        layer("tangents", mesh.tangents.as_ref());
        layer("binormals", mesh.binormals.as_ref());
        mesh.uvs.iter().for_each(|l| layer("uv", Some(l)));
        mesh.colors.iter().for_each(|l| layer("color", Some(l)));
        for (what, l) in [
            ("materials", &mesh.materials),
            ("smoothing", &mesh.smoothing),
        ] {
            if let Some(l) = l {
                println!(
                    "  {what} {:?} values {:?}",
                    l.mapping,
                    &l.values[..l.values.len().min(12)]
                );
            }
        }
    }
    for m in &scene.materials {
        let diffuse = m.properties.vec3("DiffuseColor");
        println!(
            "material {} {:?} {:?} properties {} diffuse {diffuse:?}",
            m.name,
            m.shading_model,
            m.family,
            m.properties.own.len()
        );
        for link in &m.textures {
            println!(
                "  {} <- texture {}",
                link.property, scene.textures[link.texture].name
            );
        }
    }
    for t in &scene.textures {
        let video = t.video.map(|v| scene.videos[v].name.as_str());
        println!(
            "texture {} file {:?} relative {:?} video {video:?} uv {:?} wrap {:?} t {:?} s {:?}",
            t.name, t.file_name, t.relative_file_name, t.uv_set, t.wrap, t.translation, t.scaling
        );
    }
    for v in &scene.videos {
        println!(
            "video {} file {:?} relative {:?} content {} bytes",
            v.name,
            v.file_name,
            v.relative_file_name,
            v.content.len()
        );
    }
    for c in &scene.cameras {
        println!(
            "camera {} {:?} focal {} film {:?} offset {:?} clip {:?} ortho {} aspect {:?}",
            c.name,
            c.projection,
            round(c.focal_length),
            c.film_size.map(round),
            c.film_offset.map(round),
            c.clip,
            c.ortho_zoom,
            c.aspect
        );
    }
    for l in &scene.lights {
        println!(
            "light {} {:?} color {:?} intensity {} decay {:?} cone {:?} {:?} shadows {}",
            l.name, l.kind, l.color, l.intensity, l.decay, l.cone, l.area_shape, l.cast_shadows
        );
    }
    for w in &scene.warnings {
        println!("warning: {w}");
    }
}

/// Six decimals, without a negative zero.
fn round(x: f64) -> f64 {
    let r = (x * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}
