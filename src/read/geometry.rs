//! A mesh as the standard Geometry in the project's space: its polygons
//! (those repeating a point, or of fewer than three corners, left out;
//! wound the other way where placing them mirrors), and each layer on the
//! domain its mapping names: normals as `N`, UV sets as `uv`, `uv2`…, the
//! first color set as `Cd` and `Alpha`, each polygon's material as
//! `material`, smoothing as the `sharp` edges, creases as `crease`.
use super::space::determinant;
use crate::fbx::{Layer, Mapping, Mesh};
use kinogaki_studio_api::Error;
use kinogaki_studio_api::color::ColorService;
use kinogaki_studio_api::format::ReadBudget;
use kinogaki_studio_api::geometry::{
    Attribute, Attributes, Column, Domain, Elements, Geometry, Meaning, Primitives, Text, Vec3,
    Xform, names,
};
use std::collections::HashMap;
use std::sync::Arc;

/// The space FBX's colors are taken to be in: linear Rec.709.
pub(super) const COLORS: &str = "lin_rec709_scene";

/// What making geometry needs: the color service, each material's key by
/// its index in the scene, and the read's budget, which each mesh made is
/// charged to (a mesh several nodes show in their own materials is made
/// for each).
pub(super) struct Shaping<'a> {
    pub colors: &'a dyn ColorService,
    pub keys: &'a [Arc<str>],
    pub budget: &'a ReadBudget,
}

/// The polygons kept and where each kept corner was in the file.
struct Kept {
    /// The kept polygons, by their index in the file.
    faces: Vec<u32>,
    /// Each kept corner's corner in the file; none when every corner is
    /// kept in order.
    corners: Option<Vec<u32>>,
}

/// `mesh` placed by `at` (the file's points into the project's), its
/// polygons showing the materials `slots` (indices into the scene's) name.
pub(super) fn mesh(
    mesh: &Mesh,
    at: &Xform,
    slots: &[usize],
    cx: &Shaping<'_>,
    warnings: &mut Vec<String>,
) -> Result<Geometry, Error> {
    // Its points and corners, and what its layers spread over them.
    let corners = mesh.corners.len() as u64;
    let made = (mesh.point_count() as u64).saturating_mul(24);
    cx.budget
        .allocate(made.saturating_add(corners.saturating_mul(32)))?;
    let mirrored = determinant(at) < 0.0;
    let kept = kept(mesh, mirrored);
    let dropped = mesh.polygon_count() - kept.faces.len();
    if dropped > 0 {
        let name = &mesh.name;
        warnings.push(format!(
            "{name}: {dropped} polygons repeating a point or of fewer than three corners left out"
        ));
    }
    let positions: Vec<[f64; 3]> = (0..mesh.point_count())
        .map(|p| at.apply_point(Vec3::from(mesh.point(p))).to_array())
        .collect();
    let mut offsets = Vec::with_capacity(kept.faces.len() + 1);
    offsets.push(0u32);
    let vertices: Vec<u32> = match &kept.corners {
        None => {
            offsets.extend_from_slice(&mesh.polygons[1..]);
            mesh.corners.clone()
        }
        Some(corners) => {
            for &f in &kept.faces {
                let n = mesh.polygon(f as usize).len() as u32;
                offsets.push(offsets.last().copied().unwrap_or(0) + n);
            }
            corners.iter().map(|&c| mesh.corners[c as usize]).collect()
        }
    };
    let table = Primitives::of_polygons(positions.len(), offsets.into(), vertices.into())?;
    let elements = Domains { mesh, kept: &kept };
    let mut attributes = Attributes::new();
    if let Some(normals) = &mesh.normals {
        let normal = normal_matrix(at);
        if let Some((domain, mut values)) = elements.spread(normals, false) {
            for n in values.as_chunks_mut::<3>().0 {
                let v = normal.apply_vector(Vec3::from(n.map(f64::from)));
                *n = v.normalized().unwrap_or(v).to_f32();
            }
            let n = Attribute::tuples(triples(&values), Meaning::Normal)?;
            attributes = attributes.with(domain, names::N, n)?;
        }
    }
    for (k, set) in mesh.uvs.iter().enumerate() {
        let Some((domain, values)) = elements.spread(set, true) else {
            continue;
        };
        let pairs: Vec<[f32; 2]> = values.as_chunks::<2>().0.to_vec();
        let name = uv_name(k);
        attributes = attributes.with(
            domain,
            &name,
            Attribute::tuples(pairs, Meaning::TextureCoord)?,
        )?;
    }
    if let Some(colors) = mesh.colors.first()
        && let Some((domain, values)) = elements.spread(colors, false)
    {
        let mut rgb: Vec<[f64; 3]> = (values.as_chunks::<4>().0.iter())
            .map(|c| [c[0], c[1], c[2]].map(f64::from))
            .collect();
        if let Err(e) = cx.colors.to_working(COLORS, &mut rgb) {
            warnings.push(format!("{}: colors kept as they are: {e}", mesh.name));
        }
        let cd: Vec<[f32; 3]> = rgb.iter().map(|c| c.map(|v| v as f32)).collect();
        attributes = attributes.with(domain, names::CD, Attribute::tuples(cd, Meaning::Color)?)?;
        let alpha: Vec<f32> = values.as_chunks::<4>().0.iter().map(|c| c[3]).collect();
        if alpha.iter().any(|&a| a != 1.0) {
            attributes = attributes.with(
                domain,
                names::ALPHA,
                Attribute::tuples(alpha, Meaning::None)?,
            )?;
        }
    }
    if mesh.colors.len() > 1 {
        warnings.push(format!(
            "{}: color sets after the first left out",
            mesh.name
        ));
    }
    if let Some(text) = materials(mesh, &kept, slots, cx.keys) {
        attributes = attributes.with(Domain::Primitive, names::MATERIAL, Attribute::text(text?))?;
    }
    let geometry = Geometry::new(positions.into(), table, attributes)?;
    edges(geometry, mesh, &kept)
}

/// Whether `corners` names a point twice: by pairs for a few, by sorting
/// for many (a polygon of 200,000 corners).
fn repeats(corners: &[u32]) -> bool {
    if corners.len() <= 16 {
        return (1..corners.len()).any(|i| corners[..i].contains(&corners[i]));
    }
    let mut sorted = corners.to_vec();
    sorted.sort_unstable();
    sorted.windows(2).any(|w| w[0] == w[1])
}

/// The polygons to keep, and their corners in the order they keep them.
fn kept(mesh: &Mesh, mirrored: bool) -> Kept {
    let fits = |f: usize| {
        let corners = &mesh.corners[mesh.polygon(f)];
        corners.len() >= 3 && !repeats(corners)
    };
    let faces: Vec<u32> = (0..mesh.polygon_count())
        .filter(|&f| fits(f))
        .map(|f| f as u32)
        .collect();
    if !mirrored && faces.len() == mesh.polygon_count() {
        return Kept {
            faces,
            corners: None,
        };
    }
    let mut corners = Vec::with_capacity(mesh.corners.len());
    for &f in &faces {
        let range = mesh.polygon(f as usize);
        let n = range.len();
        for j in 0..n {
            let from = if mirrored && j > 0 { n - j } else { j };
            corners.push((range.start + from) as u32);
        }
    }
    Kept {
        faces,
        corners: Some(corners),
    }
}

/// Where a layer's values land.
struct Domains<'a> {
    mesh: &'a Mesh,
    kept: &'a Kept,
}

impl Domains<'_> {
    /// `layer`'s values as single floats on the domain its mapping names:
    /// points, corners (vertices), polygons (primitives), the whole mesh
    /// (detail); `corners` puts a polygon's or the mesh's values on each
    /// corner (what UV sets must be on). None for an edge mapping.
    fn spread(&self, layer: &Layer<f64>, corners: bool) -> Option<(Domain, Vec<f32>)> {
        let width = layer.width;
        let mut out = Vec::new();
        let mut take = |e: usize| out.extend(layer.get(e).iter().map(|&v| v as f32));
        let domain = match (layer.mapping, corners) {
            (Mapping::Point, _) => {
                (0..self.mesh.point_count()).for_each(&mut take);
                Domain::Point
            }
            (Mapping::Corner, _) => {
                match &self.kept.corners {
                    None if layer.indices.is_none() => {
                        return Some((
                            Domain::Vertex,
                            layer.values.iter().map(|&v| v as f32).collect(),
                        ));
                    }
                    None => (0..self.mesh.corners.len()).for_each(&mut take),
                    Some(kept) => kept.iter().for_each(|&c| take(c as usize)),
                }
                Domain::Vertex
            }
            (Mapping::Polygon, false) => {
                self.kept.faces.iter().for_each(|&f| take(f as usize));
                Domain::Primitive
            }
            (Mapping::All, false) => {
                take(0);
                Domain::Detail
            }
            (Mapping::Polygon | Mapping::All, true) => {
                for &f in &self.kept.faces {
                    let n = self.mesh.polygon(f as usize).len();
                    let e = if layer.mapping == Mapping::All {
                        0
                    } else {
                        f as usize
                    };
                    (0..n).for_each(|_| take(e));
                }
                Domain::Vertex
            }
            (Mapping::Edge, _) => return None,
        };
        debug_assert_eq!(out.len() % width.max(1), 0);
        Some((domain, out))
    }
}

/// The attribute UV set `k` is: `uv`, then `uv2`, `uv3`….
pub(super) fn uv_name(k: usize) -> String {
    match k {
        0 => names::UV.to_owned(),
        k => format!("{}{}", names::UV, k + 1),
    }
}

fn triples(values: &[f32]) -> Vec<[f32; 3]> {
    values.as_chunks::<3>().0.to_vec()
}

/// What normals go through when points go through `at`: its linear part's
/// inverse transpose.
fn normal_matrix(at: &Xform) -> Xform {
    let linear = Xform::from_axes(at.axes()[0], at.axes()[1], at.axes()[2], Vec3::ZERO);
    let inverse = linear.inverse().unwrap_or(Xform::IDENTITY);
    let r = inverse.rows();
    Xform::from_rows([
        [r[0][0], r[1][0], r[2][0], 0.0],
        [r[0][1], r[1][1], r[2][1], 0.0],
        [r[0][2], r[1][2], r[2][2], 0.0],
    ])
}

/// Each kept polygon's material key, from its slot of the node's
/// `slots`; none when the mesh names no materials.
fn materials(
    mesh: &Mesh,
    kept: &Kept,
    slots: &[usize],
    keys: &[Arc<str>],
) -> Option<Result<Text, Error>> {
    let layer = mesh.materials.as_ref();
    if slots.is_empty() {
        return None;
    }
    let slot_of = |f: usize| -> Option<usize> {
        let slot = match layer {
            Some(l) if matches!(l.mapping, Mapping::Polygon | Mapping::All) => *l.get(f).first()?,
            _ => 0,
        };
        usize::try_from(slot).ok()
    };
    // The slots' keys once each, an index past them naming nothing.
    let table: Vec<Arc<str>> = slots.iter().map(|&m| keys[m].clone()).collect();
    let none = table.len() as u32;
    let faces: Vec<u32> = (kept.faces.iter())
        .map(|&f| {
            slot_of(f as usize)
                .filter(|&s| s < slots.len())
                .map_or(none, |s| s as u32)
        })
        .collect();
    let mut table = table;
    if faces.contains(&none) {
        table.push(Arc::from(""));
    }
    Some(Text::new(Column::new(faces), table.into()))
}

/// `geometry` with the mesh's hard edges as `sharp` (smoothing by edge,
/// 0 hard; by polygon, no smoothing group shared across the edge) and its
/// creases as `crease` on edges and points.
fn edges(geometry: Geometry, mesh: &Mesh, kept: &Kept) -> Result<Geometry, Error> {
    let table = geometry.primitives().clone();
    let edge_of = |[a, b]: [u32; 2]| table.edge_of(a, b);
    let mut attributes = (**geometry.attributes()).clone();
    let mut sharp: Vec<u32> = Vec::new();
    match &mesh.smoothing {
        Some(l) if l.mapping == Mapping::Edge => {
            for e in 0..mesh.edges.len() {
                if l.get(e).first() == Some(&0) {
                    sharp.extend(edge_of(mesh.edge_points(e)));
                }
            }
        }
        Some(l) if l.mapping == Mapping::Polygon => {
            let mut first: HashMap<[u32; 2], i32> = HashMap::new();
            for &f in &kept.faces {
                let group = l.get(f as usize).first().copied().unwrap_or(0);
                let corners = &mesh.corners[mesh.polygon(f as usize)];
                for (i, &a) in corners.iter().enumerate() {
                    let b = corners[(i + 1) % corners.len()];
                    let key = [a.min(b), a.max(b)];
                    match first.get(&key) {
                        Some(&other) if other & group == 0 => sharp.extend(edge_of(key)),
                        Some(_) => {}
                        None => _ = first.insert(key, group),
                    }
                }
            }
        }
        _ => {}
    }
    if !sharp.is_empty() {
        let members = Elements::from_indices(table.edge_count(), sharp)?;
        attributes = attributes.with_group(Domain::Edge, names::SHARP, members)?;
    }
    if let Some(l) = &mesh.edge_creases
        && l.mapping == Mapping::Edge
    {
        let mut crease = vec![0.0f32; table.edge_count()];
        for e in 0..mesh.edges.len() {
            if let Some(at) = edge_of(mesh.edge_points(e)) {
                crease[at as usize] = l.get(e).first().copied().unwrap_or(0.0) as f32;
            }
        }
        attributes = attributes.with(
            Domain::Edge,
            "crease",
            Attribute::tuples(crease, Meaning::None)?,
        )?;
    }
    if let Some(l) = &mesh.vertex_creases
        && l.mapping == Mapping::Point
    {
        let crease: Vec<f32> = (0..mesh.point_count())
            .map(|p| l.get(p)[0] as f32)
            .collect();
        attributes = attributes.with(
            Domain::Point,
            "crease",
            Attribute::tuples(crease, Meaning::None)?,
        )?;
    }
    geometry.with_attributes(attributes)
}
