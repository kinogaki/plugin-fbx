//! A `Geometry` of subclass `Mesh`: control points, polygons, edges, and
//! the layer elements over them, each with its mapping and reference.
//!
//! Arrays move out of the tree into the mesh; nothing is copied but the
//! polygon index, which is decoded (a negative index `!p` ends a polygon).
//! A layer whose length or indices don't fit its mapping is left out with a
//! warning; a corner naming no point fails the mesh.
use crate::fbx::tree::{Array, Node};

/// A polygon mesh as the file holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    /// The geometry's id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// Control points, three numbers each.
    pub points: Vec<f64>,
    /// Where each polygon's corners start in `corners`, then their end: one
    /// more than the polygons.
    pub polygons: Vec<u32>,
    /// Each polygon corner's point.
    pub corners: Vec<u32>,
    /// FBX's `Edges`: each edge as the corner it starts at; it runs to the
    /// next corner of that corner's polygon ([`Mesh::edge_points`]).
    pub edges: Vec<u32>,
    /// Normals (`LayerElementNormal`), three numbers each.
    pub normals: Option<Layer<f64>>,
    /// Tangents, three numbers each.
    pub tangents: Option<Layer<f64>>,
    /// Binormals, three numbers each.
    pub binormals: Option<Layer<f64>>,
    /// UV sets in layer order, two numbers each, named.
    pub uvs: Vec<Layer<f64>>,
    /// Color sets in layer order, RGBA each, named.
    pub colors: Vec<Layer<f64>>,
    /// Each polygon's material: an index into its node's materials.
    pub materials: Option<Layer<i32>>,
    /// Smoothing: by polygon, smoothing groups (bit masks); by edge, 0 for
    /// a hard edge and 1 for a smooth one.
    pub smoothing: Option<Layer<i32>>,
    /// Edge creases, 0 to 1.
    pub edge_creases: Option<Layer<f64>>,
    /// Vertex creases, 0 to 1.
    pub vertex_creases: Option<Layer<f64>>,
}

/// One layer element: values over a mesh's points, corners, polygons or
/// edges, directly or through indices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer<T> {
    /// Its name (a UV set's, a color set's), often empty.
    pub name: String,
    /// What it has a value for.
    pub mapping: Mapping,
    /// Numbers in one value: 3 for normals, 2 for UVs, 4 for colors, else 1.
    pub width: usize,
    /// The values, `width` numbers each.
    pub values: Vec<T>,
    /// `IndexToDirect`: for each element, its value's index. `None` for
    /// `Direct`, where element `i` has value `i`.
    pub indices: Option<Vec<u32>>,
}

/// What a layer has a value for (`MappingInformationType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mapping {
    /// Each control point (`ByControlPoint`, `ByVertex`, `ByVertice`).
    Point,
    /// Each polygon corner (`ByPolygonVertex`).
    #[default]
    Corner,
    /// Each polygon (`ByPolygon`).
    Polygon,
    /// Each edge of `Mesh::edges` (`ByEdge`).
    Edge,
    /// The whole mesh: one value (`AllSame`).
    All,
}

impl<T> Layer<T> {
    /// The value of element `i` of the mapping (a point, corner, polygon or
    /// edge; 0 for `All`), `width` numbers.
    pub fn get(&self, i: usize) -> &[T] {
        let i = if self.mapping == Mapping::All { 0 } else { i };
        let i = self
            .indices
            .as_ref()
            .map_or(i, |indices| indices[i] as usize);
        &self.values[i * self.width..(i + 1) * self.width]
    }

    /// How many values it holds.
    pub fn value_count(&self) -> usize {
        self.values.len() / self.width.max(1)
    }
}

impl Mesh {
    /// The number of control points.
    pub fn point_count(&self) -> usize {
        self.points.len() / 3
    }

    /// The number of polygons.
    pub fn polygon_count(&self) -> usize {
        self.polygons.len().saturating_sub(1)
    }

    /// Control point `i`.
    pub fn point(&self, i: usize) -> [f64; 3] {
        let p = &self.points[i * 3..i * 3 + 3];
        [p[0], p[1], p[2]]
    }

    /// Polygon `i`'s corners, as indices into `corners`.
    pub fn polygon(&self, i: usize) -> std::ops::Range<usize> {
        self.polygons[i] as usize..self.polygons[i + 1] as usize
    }

    /// The polygon corner `c` belongs to.
    pub fn polygon_of(&self, c: usize) -> usize {
        self.polygons.partition_point(|&start| start as usize <= c) - 1
    }

    /// Edge `e`'s two points.
    pub fn edge_points(&self, e: usize) -> [u32; 2] {
        let c = self.edges[e] as usize;
        let polygon = self.polygon(self.polygon_of(c));
        let next = if c + 1 < polygon.end {
            c + 1
        } else {
            polygon.start
        };
        [self.corners[c], self.corners[next]]
    }

    /// How many elements `mapping` has a value for.
    fn elements(&self, mapping: Mapping) -> usize {
        match mapping {
            Mapping::Point => self.point_count(),
            Mapping::Corner => self.corners.len(),
            Mapping::Polygon => self.polygon_count(),
            Mapping::Edge => self.edges.len(),
            Mapping::All => 1,
        }
    }
}

/// One kind of layer element and its arrays' names.
struct Kind {
    element: &'static str,
    values: &'static str,
    /// Index arrays' names (FBX writes some two ways).
    indices: &'static [&'static str],
    width: usize,
    what: &'static str,
}

const NORMALS: Kind = Kind {
    element: "LayerElementNormal",
    values: "Normals",
    indices: &["NormalsIndex", "NormalIndex"],
    width: 3,
    what: "normal",
};
const TANGENTS: Kind = Kind {
    element: "LayerElementTangent",
    values: "Tangents",
    indices: &["TangentsIndex", "TangentIndex"],
    width: 3,
    what: "tangent",
};
const BINORMALS: Kind = Kind {
    element: "LayerElementBinormal",
    values: "Binormals",
    indices: &["BinormalsIndex", "BinormalIndex"],
    width: 3,
    what: "binormal",
};
const UVS: Kind = Kind {
    element: "LayerElementUV",
    values: "UV",
    indices: &["UVIndex"],
    width: 2,
    what: "UV",
};
const COLORS: Kind = Kind {
    element: "LayerElementColor",
    values: "Colors",
    indices: &["ColorIndex"],
    width: 4,
    what: "color",
};
// The values of these are what the layer says, never through indices.
const MATERIALS: Kind = Kind {
    element: "LayerElementMaterial",
    values: "Materials",
    indices: &[],
    width: 1,
    what: "material",
};
const SMOOTHING: Kind = Kind {
    element: "LayerElementSmoothing",
    values: "Smoothing",
    indices: &[],
    width: 1,
    what: "smoothing",
};
const EDGE_CREASES: Kind = Kind {
    element: "LayerElementEdgeCrease",
    values: "EdgeCrease",
    indices: &[],
    width: 1,
    what: "edge crease",
};
const VERTEX_CREASES: Kind = Kind {
    element: "LayerElementVertexCrease",
    values: "VertexCrease",
    indices: &[],
    width: 1,
    what: "vertex crease",
};

/// Layer elements read, and the ones left out with a warning.
const READ: [&str; 9] = [
    NORMALS.element,
    TANGENTS.element,
    BINORMALS.element,
    UVS.element,
    COLORS.element,
    MATERIALS.element,
    SMOOTHING.element,
    EDGE_CREASES.element,
    VERTEX_CREASES.element,
];

/// The mesh in a `Geometry` node, its arrays moved out of the tree, with
/// warnings for what it leaves out; `Err` when its polygons are unusable.
pub(crate) fn read(id: i64, name: &str, node: &mut Node) -> Result<(Mesh, Vec<String>), String> {
    let mut warnings = Vec::new();
    let points = node
        .take_array("Vertices")
        .map(Array::into_f64s)
        .unwrap_or_default();
    if !points.len().is_multiple_of(3) {
        return Err(format!(
            "mesh `{name}` has {} point coordinates",
            points.len()
        ));
    }
    let indices = node
        .take_array("PolygonVertexIndex")
        .map(|a| a.into_i32s().ok_or("its polygon indices aren't integers"))
        .transpose()?
        .unwrap_or_default();
    let (polygons, corners) = polygons(indices, points.len() / 3, &mut warnings, name)?;
    let edges = node
        .take_array("Edges")
        .and_then(Array::into_i32s)
        .unwrap_or_default();
    let edges = if edges
        .iter()
        .all(|&e| e >= 0 && (e as usize) < corners.len())
    {
        edges.into_iter().map(|e| e as u32).collect()
    } else {
        warnings.push(format!(
            "mesh `{name}`'s edges name corners it doesn't have; left out"
        ));
        Vec::new()
    };
    let mut mesh = Mesh {
        id,
        name: name.to_owned(),
        points,
        polygons,
        corners,
        edges,
        ..Mesh::default()
    };
    let w = &mut warnings;
    mesh.uvs = layers(node, &UVS, &mesh, Array::into_f64s, w);
    mesh.colors = layers(node, &COLORS, &mesh, Array::into_f64s, w);
    mesh.normals = first(
        layers(node, &NORMALS, &mesh, Array::into_f64s, w),
        &NORMALS,
        name,
        w,
    );
    mesh.tangents = first(
        layers(node, &TANGENTS, &mesh, Array::into_f64s, w),
        &TANGENTS,
        name,
        w,
    );
    mesh.binormals = first(
        layers(node, &BINORMALS, &mesh, Array::into_f64s, w),
        &BINORMALS,
        name,
        w,
    );
    mesh.edge_creases = first(
        layers(node, &EDGE_CREASES, &mesh, Array::into_f64s, w),
        &EDGE_CREASES,
        name,
        w,
    );
    mesh.vertex_creases = first(
        layers(node, &VERTEX_CREASES, &mesh, Array::into_f64s, w),
        &VERTEX_CREASES,
        name,
        w,
    );
    mesh.materials = first(
        layers(node, &MATERIALS, &mesh, Array::into_i32s_or_empty, w),
        &MATERIALS,
        name,
        w,
    );
    mesh.smoothing = first(
        layers(node, &SMOOTHING, &mesh, Array::into_i32s_or_empty, w),
        &SMOOTHING,
        name,
        w,
    );
    for child in &node.children {
        if child.name.starts_with("LayerElement") && !READ.contains(&child.name.as_str()) {
            warnings.push(format!("{} layers left out", child.name));
        }
    }
    Ok((mesh, warnings))
}

/// `PolygonVertexIndex` decoded: polygon starts and corner points.
fn polygons(
    indices: Vec<i32>,
    points: usize,
    warnings: &mut Vec<String>,
    name: &str,
) -> Result<(Vec<u32>, Vec<u32>), String> {
    let mut starts = Vec::with_capacity(indices.len() / 3 + 2);
    starts.push(0u32);
    let mut most = 0u32;
    let corners: Vec<u32> = indices
        .into_iter()
        .enumerate()
        .map(|(i, index)| {
            let point = if index < 0 {
                starts.push(i as u32 + 1);
                !index
            } else {
                index
            } as u32;
            most = most.max(point);
            point
        })
        .collect();
    if !corners.is_empty() && most as usize >= points {
        return Err(format!("mesh `{name}` names point {most} of {points}"));
    }
    if u32::try_from(corners.len()).is_err() {
        return Err(format!("mesh `{name}` has more than 2³² corners"));
    }
    if *starts.last().unwrap_or(&0) as usize != corners.len() {
        warnings.push(format!("mesh `{name}`'s last polygon isn't closed; closed"));
        starts.push(corners.len() as u32);
    }
    Ok((starts, corners))
}

/// The first of a kind's layers: a mesh has one normal set, one material
/// assignment…
fn first<T>(
    mut all: Vec<Layer<T>>,
    kind: &Kind,
    name: &str,
    warnings: &mut Vec<String>,
) -> Option<Layer<T>> {
    if all.len() > 1 {
        warnings.push(format!(
            "mesh `{name}`: {} layers after the first left out",
            kind.what
        ));
    }
    (!all.is_empty()).then(|| all.swap_remove(0))
}

impl Array {
    /// Integers, or none when they aren't (a layer then fails its length).
    fn into_i32s_or_empty(self) -> Vec<i32> {
        self.into_i32s().unwrap_or_default()
    }
}

/// Every layer element of `kind` in `node`, in layer order, checked
/// against `mesh`.
fn layers<T>(
    node: &mut Node,
    kind: &Kind,
    mesh: &Mesh,
    convert: fn(Array) -> Vec<T>,
    warnings: &mut Vec<String>,
) -> Vec<Layer<T>> {
    let mut found: Vec<(i64, Layer<T>)> = Vec::new();
    for element in node.children.iter_mut().filter(|c| c.name == kind.element) {
        let index = element.int(0).unwrap_or(0);
        let name = element.child_str("Name").unwrap_or("").to_owned();
        match layer(element, kind, mesh, convert, name) {
            Ok(Some(layer)) => found.push((index, layer)),
            Ok(None) => {}
            Err(why) => warnings.push(format!(
                "mesh `{}`: its {} layer {why}; left out",
                mesh.name, kind.what
            )),
        }
    }
    found.sort_by_key(|(index, _)| *index);
    found.into_iter().map(|(_, layer)| layer).collect()
}

/// One layer element, `Ok(None)` when it maps nothing.
fn layer<T>(
    element: &mut Node,
    kind: &Kind,
    mesh: &Mesh,
    convert: fn(Array) -> Vec<T>,
    name: String,
) -> Result<Option<Layer<T>>, String> {
    let mapping = match element.child_str("MappingInformationType").unwrap_or("") {
        "ByControlPoint" | "ByVertex" | "ByVertice" => Mapping::Point,
        "ByPolygonVertex" => Mapping::Corner,
        "ByPolygon" => Mapping::Polygon,
        "ByEdge" => Mapping::Edge,
        "AllSame" => Mapping::All,
        "NoMappingInformation" | "" => return Ok(None),
        other => return Err(format!("maps by `{other}`")),
    };
    let indexed = matches!(
        element.child_str("ReferenceInformationType"),
        Some("IndexToDirect" | "Index")
    );
    let values = element
        .take_array(kind.values)
        .map(convert)
        .unwrap_or_default();
    let width = kind.width;
    if !values.len().is_multiple_of(width) {
        return Err(format!(
            "holds {} numbers, not {width} a value",
            values.len()
        ));
    }
    let count = values.len() / width;
    let indices = match kind.indices.iter().find_map(|n| element.take_array(n)) {
        Some(array) if indexed => {
            let indices = array
                .into_i32s()
                .ok_or("has indices that aren't integers")?;
            if let Some(bad) = indices.iter().find(|&&i| i < 0 || i as usize >= count) {
                return Err(format!("names value {bad} of {count}"));
            }
            Some(indices.into_iter().map(|i| i as u32).collect::<Vec<u32>>())
        }
        _ => None,
    };
    let needed = mesh.elements(mapping);
    let has = indices.as_ref().map_or(count, Vec::len);
    if has < needed {
        return Err(format!("has {has} values for {needed}"));
    }
    Ok(Some(Layer {
        name,
        mapping,
        width,
        values,
        indices,
    }))
}
