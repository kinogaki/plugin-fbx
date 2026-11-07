//! The scene from a file's tree: meshes first (their arrays move out of the
//! tree, side by side), then everything else through the object graph.
use super::graph::{Graph, object_name};
use super::properties::Properties;
use super::shading::{self, Material, Texture, TextureLink, Video};
use super::transform::{Geometric, Inherit, RotationOrder, Transform};
use super::{Attribute, Camera, Light, Mesh, Model, Scene, Settings, mesh};
use crate::fbx::Error;
use crate::fbx::error::At;
use crate::fbx::tree::{Document, Node};
use kinogaki_studio_api::format::ReadBudget;
use kinogaki_studio_api::geometry::parallel;
use std::collections::HashMap;
use std::sync::Mutex;

pub(super) fn scene(mut document: Document, budget: &ReadBudget) -> Result<Scene, Error> {
    if document.version != 0 && document.version < 7000 {
        return Err(Error::Unsupported(format!(
            "FBX {}.{} files aren't read: export FBX 2011 (7.1) or later",
            document.version / 1000,
            document.version % 1000 / 100
        )));
    }
    let mut warnings = Vec::new();
    let meshes = meshes(&mut document, budget, &mut warnings)?;
    budget.check()?;
    let graph = Graph::new(&document);
    let (settings, axes_warning) = Settings::from_properties(
        document
            .node("GlobalSettings")
            .map(|n| Properties::of(n, None))
            .unwrap_or_default(),
    );
    warnings.extend(axes_warning);

    let videos: Vec<Video> = graph.of_class("Video").map(|i| video(&graph, i)).collect();
    let video_at = indices(&graph, "Video");
    let texture_at = indices(&graph, "Texture");
    let packed = Packed::of(&videos);
    let textures: Vec<Texture> = graph
        .of_class("Texture")
        .map(|i| texture(&graph, i, &video_at, &videos, &packed))
        .collect();
    let mut layered = false;
    let materials: Vec<Material> = graph
        .of_class("Material")
        .map(|i| material(&graph, i, &texture_at, &mut layered))
        .collect();
    if layered {
        warnings.push("layered textures read as their layers; their blending left out".into());
    }

    let mut cameras = Vec::new();
    let mut lights = Vec::new();
    let mut attributes = HashMap::new();
    for i in graph.of_class("NodeAttribute") {
        let o = &graph.objects[i];
        let attribute = match o.subclass {
            "Camera" => {
                cameras.push(Camera::new(o.id, o.name, graph.properties(i)));
                Attribute::Camera(cameras.len() - 1)
            }
            "Light" => {
                lights.push(Light::new(o.id, o.name, graph.properties(i)));
                Attribute::Light(lights.len() - 1)
            }
            "Null" | "Root" => Attribute::Null,
            "LimbNode" => Attribute::Bone,
            other => Attribute::Other(other.to_owned()),
        };
        attributes.insert(i, attribute);
    }
    let mesh_at: HashMap<i64, usize> = meshes.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
    for i in graph.of_class("Geometry") {
        let o = &graph.objects[i];
        let attribute = match mesh_at.get(&o.id) {
            Some(&mesh) => Attribute::Mesh(mesh),
            None => Attribute::Other(o.subclass.to_owned()),
        };
        attributes.insert(i, attribute);
    }

    let material_at = indices(&graph, "Material");
    let models = models(&graph, &attributes, &material_at, &mut warnings);
    left_out(&graph, &mut warnings);
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|w| seen.insert(w.clone()));
    let creator = document
        .node("Creator")
        .or_else(|| document.node("FBXHeaderExtension")?.child("Creator"))
        .and_then(|n| n.str(0))
        .unwrap_or("")
        .to_owned();
    Ok(Scene {
        version: document.version,
        format: document.format,
        creator,
        settings,
        models,
        meshes,
        cameras,
        lights,
        materials,
        textures,
        videos,
        warnings,
    })
}

/// Every `Geometry` that is a `Mesh`, read side by side.
fn meshes(
    document: &mut Document,
    budget: &ReadBudget,
    warnings: &mut Vec<String>,
) -> Result<Vec<Mesh>, Error> {
    let Some(objects) = document.nodes.iter_mut().find(|n| n.name == "Objects") else {
        return Ok(Vec::new());
    };
    let nodes: Vec<Mutex<&mut Node>> = objects
        .children
        .iter_mut()
        .filter(|n| n.name == "Geometry" && n.str(2) == Some("Mesh"))
        .map(Mutex::new)
        .collect();
    let read = parallel::map(nodes.len(), |i| {
        budget.check()?;
        let mut node = nodes[i].lock().unwrap_or_else(|e| e.into_inner());
        let (id, name) = (node.int(0).unwrap_or(0), object_name(&node).to_owned());
        mesh::read(id, &name, &mut node).map_err(|message| Error::malformed(At::Graph, message))
    });
    let mut meshes = Vec::with_capacity(read.len());
    for result in read {
        let (mesh, said) = result?;
        meshes.push(mesh);
        warnings.extend(said);
    }
    Ok(meshes)
}

/// Each object of `class` by its position among them.
fn indices(graph: &Graph, class: &'static str) -> HashMap<usize, usize> {
    graph
        .of_class(class)
        .enumerate()
        .map(|(n, i)| (i, n))
        .collect()
}

fn video(graph: &Graph, i: usize) -> Video {
    let o = &graph.objects[i];
    Video {
        id: o.id,
        name: o.name.to_owned(),
        file_name: o.node.child_str("Filename").unwrap_or("").to_owned(),
        relative_file_name: o
            .node
            .child_str("RelativeFilename")
            .unwrap_or("")
            .to_owned(),
        content: shading::content(o.node),
        properties: graph.properties(i),
    }
}

/// The first video packing each file, by its file name and by its
/// relative one.
struct Packed<'v> {
    by_file: HashMap<&'v str, usize>,
    by_relative: HashMap<&'v str, usize>,
}

impl<'v> Packed<'v> {
    fn of(videos: &'v [Video]) -> Packed<'v> {
        let mut packed = Packed {
            by_file: HashMap::new(),
            by_relative: HashMap::new(),
        };
        for (v, video) in videos.iter().enumerate().rev() {
            if !video.content.is_empty() {
                packed.by_file.insert(&video.file_name, v);
                packed.by_relative.insert(&video.relative_file_name, v);
            }
        }
        packed
    }

    /// The first video packing the file `video` names, by either name.
    fn of_video(&self, video: &Video) -> Option<usize> {
        let first = |map: &HashMap<&str, usize>, name: &str| {
            (!name.is_empty()).then(|| map.get(name).copied()).flatten()
        };
        match (
            first(&self.by_file, &video.file_name),
            first(&self.by_relative, &video.relative_file_name),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

fn texture(
    graph: &Graph,
    i: usize,
    video_at: &HashMap<usize, usize>,
    videos: &[Video],
    packed: &Packed<'_>,
) -> Texture {
    let o = &graph.objects[i];
    let p = graph.properties(i);
    // A file embedded once serves every video naming it.
    let video = graph
        .children_of_class(i, "Video")
        .find_map(|v| video_at.get(&v).copied());
    let video = video.map(|v| match videos[v].content.is_empty() {
        true => packed.of_video(&videos[v]).unwrap_or(v),
        false => v,
    });
    Texture {
        id: o.id,
        name: o.name.to_owned(),
        file_name: o.node.child_str("FileName").unwrap_or("").to_owned(),
        relative_file_name: o
            .node
            .child_str("RelativeFilename")
            .unwrap_or("")
            .to_owned(),
        video,
        uv_set: p.string("UVSet").unwrap_or("").to_owned(),
        translation: p.vec3_or("Translation", [0.0; 3]),
        rotation: p.vec3_or("Rotation", [0.0; 3]),
        scaling: p.vec3_or("Scaling", [1.0; 3]),
        wrap: [
            shading::wrap(p.int("WrapModeU")),
            shading::wrap(p.int("WrapModeV")),
        ],
        properties: p,
    }
}

fn material(
    graph: &Graph,
    i: usize,
    texture_at: &HashMap<usize, usize>,
    layered: &mut bool,
) -> Material {
    let o = &graph.objects[i];
    let shading_model = o.node.child_str("ShadingModel").unwrap_or("").to_owned();
    let mut textures = Vec::new();
    for link in graph.children(i) {
        let Some(property) = link.property else {
            continue;
        };
        let mut add = |t: usize| {
            if let Some(&texture) = texture_at.get(&t) {
                textures.push(TextureLink {
                    property: property.to_owned(),
                    texture,
                });
            }
        };
        match graph.objects[link.object].class {
            "Texture" => add(link.object),
            "LayeredTexture" => {
                *layered = true;
                graph
                    .children_of_class(link.object, "Texture")
                    .for_each(add);
            }
            _ => {}
        }
    }
    Material {
        id: o.id,
        name: o.name.to_owned(),
        family: shading::family(graph, i, &shading_model),
        shading_model,
        properties: graph.properties(i),
        textures,
    }
}

/// The models, parents before children, each in file order among its
/// siblings.
fn models(
    graph: &Graph,
    attributes: &HashMap<usize, Attribute>,
    material_at: &HashMap<usize, usize>,
    warnings: &mut Vec<String>,
) -> Vec<Model> {
    let objects: Vec<usize> = graph.of_class("Model").collect();
    let at = indices(graph, "Model");
    let mut parent: Vec<Option<usize>> = objects
        .iter()
        .map(|&i| {
            graph
                .parents(i)
                .iter()
                .find_map(|l| at.get(&l.object).copied())
        })
        .collect();
    // Walk up from each node; coming back to a node of the same walk is a
    // cycle of parents, broken there: that node goes to the top.
    let mut walk = vec![usize::MAX; objects.len()];
    for start in 0..objects.len() {
        let mut k = start;
        loop {
            if walk[k] != usize::MAX {
                if walk[k] == start {
                    let name = graph.objects[objects[k]].name;
                    warnings.push(format!(
                        "node `{name}` is its own ancestor; placed at the top"
                    ));
                    parent[k] = None;
                }
                break;
            }
            walk[k] = start;
            match parent[k] {
                Some(p) => k = p,
                None => break,
            }
        }
    }
    let mut children = vec![Vec::new(); objects.len()];
    for (k, p) in parent.iter().enumerate() {
        if let Some(p) = p {
            children[*p].push(k);
        }
    }
    // Depth first from the top nodes, siblings in file order.
    let mut order = Vec::with_capacity(objects.len());
    for top in (0..objects.len()).filter(|&k| parent[k].is_none()) {
        let mut stack = vec![top];
        while let Some(k) = stack.pop() {
            order.push(k);
            stack.extend(children[k].iter().rev());
        }
    }
    let position: HashMap<usize, usize> = order.iter().enumerate().map(|(n, &k)| (k, n)).collect();
    order
        .iter()
        .map(|&k| {
            let i = objects[k];
            model(
                graph,
                i,
                parent[k].map(|p| position[&p]),
                attributes,
                material_at,
            )
        })
        .collect()
}

fn model(
    graph: &Graph,
    i: usize,
    parent: Option<usize>,
    attributes: &HashMap<usize, Attribute>,
    material_at: &HashMap<usize, usize>,
) -> Model {
    let o = &graph.objects[i];
    let p = graph.properties(i);
    let v = |name, default| p.vec3_or(name, default);
    let transform = Transform {
        translation: v("Lcl Translation", [0.0; 3]),
        rotation: v("Lcl Rotation", [0.0; 3]),
        scaling: v("Lcl Scaling", [1.0; 3]),
        rotation_order: RotationOrder::from_fbx(p.int("RotationOrder").unwrap_or(0)),
        pre_rotation: v("PreRotation", [0.0; 3]),
        post_rotation: v("PostRotation", [0.0; 3]),
        rotation_offset: v("RotationOffset", [0.0; 3]),
        rotation_pivot: v("RotationPivot", [0.0; 3]),
        scaling_offset: v("ScalingOffset", [0.0; 3]),
        scaling_pivot: v("ScalingPivot", [0.0; 3]),
        inherit: Inherit::from_fbx(p.int("InheritType").unwrap_or(0)),
    };
    let geometric = Geometric {
        translation: v("GeometricTranslation", [0.0; 3]),
        rotation: v("GeometricRotation", [0.0; 3]),
        scaling: v("GeometricScaling", [1.0; 3]),
    };
    let attribute = graph
        .children(i)
        .iter()
        .find_map(|l| attributes.get(&l.object).cloned())
        .unwrap_or(match o.subclass {
            "LimbNode" => Attribute::Bone,
            "Null" | "Root" | "" => Attribute::Null,
            other => Attribute::Other(other.to_owned()),
        });
    let visible = p.number_or("Visibility", 1.0) != 0.0 && p.int("Show").is_none_or(|s| s != 0);
    Model {
        id: o.id,
        name: o.name.to_owned(),
        parent,
        attribute,
        transform,
        geometric,
        materials: graph
            .children_of_class(i, "Material")
            .filter_map(|m| material_at.get(&m).copied())
            .collect(),
        visible,
        properties: p,
    }
}

/// Warnings for what the file holds that a scene at one time leaves out.
fn left_out(graph: &Graph, warnings: &mut Vec<String>) {
    for o in &graph.objects {
        let said = match (o.class, o.subclass) {
            ("AnimationCurve", _) => "animation left out: the file is read at one time".to_owned(),
            ("Deformer", "Skin") => "skins left out: meshes read in their rest pose".to_owned(),
            ("Deformer", "BlendShape") => "blend shapes left out".to_owned(),
            ("Geometry", kind) if kind != "Mesh" && kind != "Shape" => {
                format!("{kind} geometry left out")
            }
            _ => continue,
        };
        warnings.push(said);
    }
}
