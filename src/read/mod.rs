//! An FBX file read into the standard values, in the project's units and
//! axes ([`space`]): the materials first, as one item (each keyed by its
//! name, made unique), then the nodes, parents first, each placed where its
//! world matrix puts it (every inheritance FBX knows, through the scene's
//! world matrices): a group, a mesh's polygons ([`geometry`]), a camera or
//! a light ([`lights`]) turned to look down its −Z.
//!
//! A mesh several nodes show is one value held once, made in the slots of the
//! node telling the most of them apart: each node places an instance of it by
//! its geometric transform, naming on the Instance domain its own `material`
//! where its slots give the mesh's polygons one, else its `material_map` where
//! they give them others name for name. A node whose slots a renaming can't
//! express shows its own copy: one giving two materials to two slots the mesh
//! names alike, or no material where the mesh names one (a node of fewer
//! slots), since an instance's materials map names, and a pair names no
//! blank.
mod geometry;
mod lights;
mod materials;
mod space;
mod textures;

use crate::fbx::{self, Attribute, Light, Matrix, Model, Scene};
use geometry::Shaping;
use kinogaki_studio_api::format::files::Named;
use kinogaki_studio_api::format::{Conversion, Unique};
use kinogaki_studio_api::geometry::{self as values, Attributes, Domain, Text, Vec3, Xform, names};
use kinogaki_studio_api::standard::{self, Instances, Materials};
use kinogaki_studio_api::{
    Data, Diagnostic, Error, FileItem, FormatOptions, Imported, Input, Reading,
};
use materials::Shading;
use space::{frame, shown};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use textures::Files;

/// Reads `input` as `chosen` says.
pub(crate) fn read(
    input: &Input<'_>,
    chosen: &FormatOptions,
    reading: &Reading<'_>,
) -> Result<Imported, Error> {
    reading.check()?;
    let bytes = input.bytes()?;
    let mut scene = fbx::read_with(&bytes, &reading.budget).map_err(|e| failed(input, e))?;
    drop(bytes);
    reading.check()?;
    let failed_units = |e: Error| Error::invalid(format!("{input}: {}", e.message()));
    let kept = reading.document_units(space::units(&scene.settings).map_err(failed_units)?);
    let space =
        space::conversion(&scene.settings, kept.unwrap_or(reading.units)).map_err(failed_units)?;
    let mut said: BTreeSet<String> = std::mem::take(&mut scene.warnings).into_iter().collect();
    let named = Named::new(input, chosen, reading);
    let mut files = Files::new(&mut scene.videos, &named);
    let keys = keys(&scene);
    let uv_sets = uv_sets(&scene);
    let mut looks = Materials::default();
    for (m, key) in scene.materials.iter().zip(&keys) {
        reading.check()?;
        let mut shading = Shading {
            scene: &scene,
            files: &mut files,
            colors: reading.colors,
            uv_sets: &uv_sets,
        };
        let (mut material, approximated) = materials::material(m, key, &mut shading);
        let unconverted = material.inside.scale_lengths(space.scale());
        if unconverted > 0 {
            said.insert(format!(
                "{}: {unconverted} lengths driven by textures keep the file's units",
                material.name
            ));
        }
        said.extend(approximated);
        looks.materials.push(material);
    }
    said.extend(named.warnings().into_iter().map(|w| w.message));
    let mut walk = Walk {
        scene: &scene,
        space,
        shaping: Shaping {
            colors: reading.colors,
            keys: &keys,
            budget: &reading.budget,
        },
        worlds: scene.world_matrices(),
        users: users(&scene),
        slots: telling(&scene, &keys),
        shared: HashMap::new(),
        said: Vec::new(),
    };
    let mut items = Vec::with_capacity(scene.models.len() + 1);
    if !looks.materials.is_empty() {
        items.push(FileItem::top(
            "Materials",
            standard::materials().wrap(looks),
        ));
    }
    let placed: Vec<Xform> = walk
        .worlds
        .iter()
        .map(|w| space.placement(&space::xform(w)))
        .collect();
    let mut item_of = Vec::with_capacity(scene.models.len());
    for (i, model) in scene.models.iter().enumerate() {
        reading.check()?;
        let above = model.parent.map_or(Xform::IDENTITY, |p| placed[p]);
        let (world, data) = walk.model(i, model, placed[i])?;
        let inverse = above.inverse().unwrap_or(Xform::IDENTITY);
        item_of.push(items.len());
        items.push(FileItem {
            name: model.name.clone(),
            parent: model.parent.map(|p| item_of[p]),
            placement: inverse.compose(&world),
            visible: model.visible,
            data,
        });
    }
    said.extend(walk.said);
    Ok(Imported {
        items,
        warnings: said.into_iter().map(Diagnostic::warning).collect(),
        units: kept,
        ..Imported::default()
    })
}

/// A failed read as ours, naming the input.
fn failed(input: &Input<'_>, e: fbx::Error) -> Error {
    match e {
        fbx::Error::Cancelled => Error::cancelled(),
        e => Error::invalid(format!("{input}: {e}")),
    }
}

/// Each material's key: its name, made unique (`name_1`, `name_2`…).
fn keys(scene: &Scene) -> Vec<Arc<str>> {
    let mut taken = Unique::names();
    (scene.materials.iter())
        .map(|m| Arc::from(taken.take(&m.name)))
        .collect()
}

/// The UV sets' names in order: each position's name in the first mesh
/// that has one there (what a texture's UV set names).
fn uv_sets(scene: &Scene) -> Vec<String> {
    let most = scene.meshes.iter().map(|m| m.uvs.len()).max().unwrap_or(0);
    (0..most)
        .map(|k| {
            let first = scene.meshes.iter().find_map(|m| m.uvs.get(k));
            first.map_or_else(String::new, |l| l.name.clone())
        })
        .collect()
}

/// How many nodes show each mesh.
fn users(scene: &Scene) -> Vec<usize> {
    let mut users = vec![0; scene.meshes.len()];
    for model in &scene.models {
        if let Attribute::Mesh(k) = model.attribute {
            users[k] += 1;
        }
    }
    users
}

/// Each mesh's slots as the node showing it that tells the most materials
/// apart gives them (the most slots among equals; the first of those).
fn telling(scene: &Scene, keys: &[Arc<str>]) -> Vec<Vec<usize>> {
    let mut best: Vec<Option<(usize, usize, &[usize])>> = vec![None; scene.meshes.len()];
    for model in &scene.models {
        let Attribute::Mesh(k) = model.attribute else {
            continue;
        };
        let apart: HashSet<&str> = model.materials.iter().map(|&m| &*keys[m]).collect();
        let rank = (apart.len(), model.materials.len());
        if best[k].is_none_or(|(a, n, _)| rank > (a, n)) {
            best[k] = Some((rank.0, rank.1, &model.materials));
        }
    }
    (best.into_iter())
        .map(|b| b.map_or_else(Vec::new, |(_, _, slots)| slots.to_vec()))
        .collect()
}

/// The walk over the scene's nodes.
struct Walk<'s> {
    scene: &'s Scene,
    space: Conversion,
    shaping: Shaping<'s>,
    /// Each node's world matrix in the file.
    worlds: Vec<Matrix>,
    users: Vec<usize>,
    /// The slots each mesh is held in when nodes share it ([`telling`]).
    slots: Vec<Vec<usize>>,
    /// Each shared mesh as one value, with the slots it was made with.
    shared: HashMap<usize, (Data, Vec<usize>)>,
    said: Vec<String>,
}

impl Walk<'_> {
    /// Node `i`'s world placement in the project (`placed`, turned for a
    /// camera or a light) and what it shows.
    fn model(
        &mut self,
        i: usize,
        model: &Model,
        placed: Xform,
    ) -> Result<(Xform, Option<Data>), Error> {
        let scene = self.scene;
        let inner = self.worlds[i] * model.geometric.matrix();
        Ok(match &model.attribute {
            Attribute::Null | Attribute::Bone => (placed, None),
            Attribute::Mesh(k) if self.users[*k] > 1 => (placed, self.instance(*k, model)?),
            Attribute::Mesh(k) => {
                let at = shown(&self.space, &model.geometric.matrix());
                let mesh = geometry::mesh(
                    &scene.meshes[*k],
                    &at,
                    &model.materials,
                    &self.shaping,
                    &mut self.said,
                )?;
                (placed, Some(standard::geometry().wrap(mesh)))
            }
            Attribute::Camera(k) => {
                let camera = &scene.cameras[*k];
                let lens = lights::camera(camera, self.space.scale(), scene.settings.unit);
                let turned = self.turned(&inner, fbx::Camera::LOOK, fbx::Camera::UP);
                match (turned, standard::camera().wrap(lens)) {
                    (Some(world), Ok(lens)) => (world, Some(lens)),
                    _ => self.left_out(model, placed, "a camera with no lens or no direction"),
                }
            }
            Attribute::Light(k) => {
                let light: &Light = &scene.lights[*k];
                self.said
                    .extend(lights::approximated(light).map(str::to_owned));
                let turned = self.turned(&inner, Light::DIRECTION, [0.0, 0.0, -1.0]);
                let light = lights::light(light, self.space.scale(), self.shaping.colors)?;
                match (turned, light.map(|l| standard::light().wrap(l))) {
                    (Some(world), Some(Ok(light))) => (world, Some(light)),
                    (_, Some(Err(_))) => self.left_out(model, placed, "a light out of range"),
                    _ => (placed, None),
                }
            }
            Attribute::Other(what) => self.left_out(model, placed, &format!("a {what}")),
        })
    }

    /// Says `model` is left out, as a group where it was.
    fn left_out(&mut self, model: &Model, placed: Xform, what: &str) -> (Xform, Option<Data>) {
        self.said.push(format!("{}: {what}, left out", model.name));
        (placed, None)
    }

    /// The placement of a camera or light in a node of world `inner` (its
    /// geometric transform after its world matrix), looking down `look` with
    /// `up` up in its node: turned to look down its −Z with +Y up, at its
    /// node's origin in the project.
    fn turned(&self, inner: &Matrix, look: [f64; 3], up: [f64; 3]) -> Option<Xform> {
        let origin = shown(&self.space, inner).apply_point(Vec3::ZERO);
        let direction = |v: [f64; 3]| self.space.direction(inner.vector(v).into());
        frame(origin, direction(look), direction(up))
    }

    /// An instance of shared mesh `k` where `model` places it: the mesh held
    /// once in its telling node's slots; on the Instance domain the node's
    /// own `material` where its slots give every polygon one, else its
    /// `material_map` where they give the mesh's materials others name for
    /// name ([`renamed`]); its own copy where they can't.
    fn instance(&mut self, k: usize, model: &Model) -> Result<Option<Data>, Error> {
        let scene = self.scene;
        if !self.shared.contains_key(&k) {
            let at = shown(&self.space, &Matrix::IDENTITY);
            let slots = self.slots[k].clone();
            let mesh =
                geometry::mesh(&scene.meshes[k], &at, &slots, &self.shaping, &mut self.said)?;
            self.shared
                .insert(k, (standard::geometry().wrap(mesh), slots));
        }
        let (prototype, slots) = self.shared[&k].clone();
        let used = used_slots(&prototype, slots.len());
        let keys = self.shaping.keys;
        let attributes = match renamed(&used, (&slots, &model.materials), keys) {
            Some(Renamed::Same) => Attributes::new(),
            Some(Renamed::Whole(key)) => Attributes::new().with(
                Domain::Instance,
                names::MATERIAL,
                values::Attribute::text(Text::from_strs([key])),
            )?,
            Some(Renamed::Mapped(pairs)) => {
                let ty = values::AttributeType {
                    array: true,
                    ..values::AttributeType::TEXT
                };
                let offsets = vec![0, 2 * pairs.len() as u32].into();
                let flat = pairs.iter().flat_map(|&(a, b)| [a, b]);
                let text = values::Values::Text(Text::from_strs(flat));
                Attributes::new().with(
                    Domain::Instance,
                    names::MATERIAL_MAP,
                    values::Attribute::arrays(ty, offsets, text)?,
                )?
            }
            None => {
                let at = shown(&self.space, &model.geometric.matrix());
                let mesh = geometry::mesh(
                    &scene.meshes[k],
                    &at,
                    &model.materials,
                    &self.shaping,
                    &mut self.said,
                )?;
                return Ok(Some(standard::geometry().wrap(mesh)));
            }
        };
        let at = vec![
            self.space
                .placement(&space::xform(&model.geometric.matrix())),
        ]
        .into();
        let instances = Instances::new(vec![prototype], at, None, attributes)?;
        Ok(Some(standard::instances().wrap(instances)))
    }
}

/// The slots of `slots_held` a held mesh's polygons are in: its `material`
/// text's indices are their slots ([`Text::NONE`] for none).
fn used_slots(prototype: &Data, slots_held: usize) -> Vec<usize> {
    let polygons = standard::geometry().get(prototype);
    let text = polygons.and_then(|p| p.attributes().text(Domain::Primitive, names::MATERIAL));
    let used: BTreeSet<usize> = (text.into_iter())
        .flat_map(|t| t.indices().iter().map(|&i| i as usize))
        .filter(|&i| i < slots_held)
        .collect();
    used.into_iter().collect()
}

/// How a node shows a mesh held in other slots.
#[derive(Debug, PartialEq)]
enum Renamed<'k> {
    /// As it is held.
    Same,
    /// In one material.
    Whole(&'k str),
    /// Each pair's first material as its second.
    Mapped(Vec<(&'k str, &'k str)>),
}

/// How a node giving the slots `own` shows a mesh held in `held`'s, over the
/// slots its polygons are in (`used`): none when a renaming of materials
/// can't say it (two slots held in one material it gives two, or a slot held
/// in a material it gives none).
fn renamed<'k>(
    used: &[usize],
    (held, own): (&[usize], &[usize]),
    keys: &'k [Arc<str>],
) -> Option<Renamed<'k>> {
    let key = |slots: &[usize], s: usize| slots.get(s).map(|&m| &*keys[m]);
    let pairs: Vec<(&str, Option<&str>)> = (used.iter())
        .filter_map(|&s| Some((key(held, s)?, key(own, s))))
        .collect();
    if pairs.iter().all(|&(a, b)| Some(a) == b) {
        return Some(Renamed::Same);
    }
    let mut given = pairs.iter().map(|&(_, b)| b);
    if let Some(Some(one)) = given.next()
        && given.all(|b| b == Some(one))
    {
        return Some(Renamed::Whole(one));
    }
    let mut map: Vec<(&str, &str)> = Vec::new();
    for (a, b) in pairs {
        let b = b?;
        match map.iter().find(|(from, _)| *from == a) {
            Some(&(_, to)) if to != b => return None,
            Some(_) => {}
            None => map.push((a, b)),
        }
    }
    map.retain(|(a, b)| a != b);
    Some(Renamed::Mapped(map))
}
