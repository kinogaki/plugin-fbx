//! An FBX file read into the standard values, in the project's units and
//! axes ([`space`]): the materials first, as one item (each keyed by its
//! name, made unique), then the nodes, parents first, each placed where its
//! world matrix puts it (every inheritance FBX knows, through the scene's
//! world matrices): a group, a mesh's polygons ([`geometry`]), a camera or
//! a light ([`lights`]) turned to look down its −Z.
//!
//! A mesh several nodes show is one value held once: each node places an
//! instance of it by its geometric transform, naming its own material on
//! the Instance domain where its slots give the one material another; a
//! node whose slots give several others shows its own copy.
mod geometry;
mod lights;
mod materials;
mod space;
mod textures;

use crate::fbx::{self, Attribute, Light, Matrix, Model, Scene};
use geometry::Shaping;
use kinogaki_studio_api::format::files::Named;
use kinogaki_studio_api::geometry::{self as values, Attributes, Domain, Text, Vec3, Xform, names};
use kinogaki_studio_api::standard::{self, Instances, Materials};
use kinogaki_studio_api::{
    Data, Diagnostic, Error, FileItem, FormatOptions, Imported, Input, Reading,
};
use materials::Shading;
use space::{Space, frame};
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
    let space = Space::new(&scene.settings, reading.units);
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
        let (material, approximated) = materials::material(m, key, &mut shading);
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
    let placed: Vec<Xform> = walk.worlds.iter().map(|w| space.placement(w)).collect();
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
    })
}

/// A failed read as ours, naming the input.
fn failed(input: &Input<'_>, e: fbx::Error) -> Error {
    match e {
        fbx::Error::Cancelled => Error::cancelled(),
        e => Error::invalid(format!("{input}: {e}")),
    }
}

/// Each material's key: its name, made unique (`name_1`, `name_2`…), each
/// name's copies counted from the last one taken, so many of one name cost
/// no more than many names.
fn keys(scene: &Scene) -> Vec<Arc<str>> {
    let mut taken = HashSet::new();
    let mut copies: HashMap<&str, usize> = HashMap::new();
    (scene.materials.iter())
        .map(|m| {
            let mut key = m.name.clone();
            while !taken.insert(key.clone()) {
                let copy = copies.entry(&m.name).or_insert(0);
                *copy += 1;
                key = format!("{}_{copy}", m.name);
            }
            Arc::from(key)
        })
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

/// The walk over the scene's nodes.
struct Walk<'s> {
    scene: &'s Scene,
    space: Space,
    shaping: Shaping<'s>,
    /// Each node's world matrix in the file.
    worlds: Vec<Matrix>,
    users: Vec<usize>,
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
                let at = self.space.shown(&model.geometric.matrix());
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
                let lens = lights::camera(camera, self.space.scale, scene.settings.unit);
                let turned = self.turned(&inner, fbx::Camera::LOOK, fbx::Camera::UP);
                match (turned, standard::lens(lens)) {
                    (Some(world), Ok(lens)) => (world, Some(standard::camera().wrap(lens))),
                    _ => self.left_out(model, placed, "a camera with no lens or no direction"),
                }
            }
            Attribute::Light(k) => {
                let light: &Light = &scene.lights[*k];
                self.said
                    .extend(lights::approximated(light).map(str::to_owned));
                let turned = self.turned(&inner, Light::DIRECTION, [0.0, 0.0, -1.0]);
                match (
                    turned,
                    lights::light(light, self.space.scale, self.shaping.colors)?,
                ) {
                    (Some(world), Some(light)) => (world, Some(standard::light().wrap(light))),
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
        let origin = self.space.shown(inner).apply_point(Vec3::ZERO);
        let direction = |v: [f64; 3]| self.space.direction(inner.vector(v));
        frame(origin, direction(look), direction(up))
    }

    /// An instance of shared mesh `k` where `model` places it: the mesh
    /// held once; the node's own material on the Instance domain where its
    /// slots give one other material; its own copy where they give several.
    fn instance(&mut self, k: usize, model: &Model) -> Result<Option<Data>, Error> {
        let scene = self.scene;
        if !self.shared.contains_key(&k) {
            let at = self.space.shown(&Matrix::IDENTITY);
            let mesh = geometry::mesh(
                &scene.meshes[k],
                &at,
                &model.materials,
                &self.shaping,
                &mut self.said,
            )?;
            self.shared.insert(
                k,
                (standard::geometry().wrap(mesh), model.materials.clone()),
            );
        }
        let (prototype, slots) = self.shared[&k].clone();
        let attributes = match (&slots[..], &model.materials[..]) {
            (held, own) if held == own => Attributes::new(),
            ([_], [own]) => {
                let key = &*self.shaping.keys[*own];
                let text = Text::from_strs([key]);
                Attributes::new().with(
                    Domain::Instance,
                    names::MATERIAL,
                    values::Attribute::text(text),
                )?
            }
            _ => {
                let at = self.space.shown(&model.geometric.matrix());
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
        let at = vec![self.space.placement(&model.geometric.matrix())].into();
        let instances = Instances::new(vec![prototype], at, None, attributes)?;
        Ok(Some(standard::instances().wrap(instances)))
    }
}
