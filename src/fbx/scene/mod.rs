//! The scene a file describes: nodes in a tree, what each shows, and the
//! materials, textures and media they use. Values are the file's own, in
//! its axes and units; [`Settings::conversion`] places them in another
//! system.
mod attributes;
mod build;
mod graph;
mod mesh;
mod properties;
mod settings;
mod shading;
mod transform;

pub use attributes::{AreaShape, Camera, Decay, Light, LightKind, Projection};
pub use mesh::{Layer, Mapping, Mesh};
pub use properties::{Properties, Property, Template};
pub use settings::{Axes, Direction, Settings};
pub use shading::{Family, Material, Texture, TextureLink, Video, Wrap};
pub use transform::{Geometric, Inherit, Matrix, RotationOrder, Transform};

use crate::fbx::tree::{Document, Format};
use crate::fbx::{Error, Options};
use transform::Placed;

/// A file's scene, read at one time (no animation).
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// The FBX version, as `7400` for 7.4.
    pub version: u32,
    /// How the file was written.
    pub format: Format,
    /// The application that wrote it, as it says (`Creator`).
    pub creator: String,
    /// Its axes and unit.
    pub settings: Settings,
    /// The nodes (FBX's models), parents before children.
    pub models: Vec<Model>,
    /// Polygon meshes; one may be shown by several nodes.
    pub meshes: Vec<Mesh>,
    /// Cameras.
    pub cameras: Vec<Camera>,
    /// Lights.
    pub lights: Vec<Light>,
    /// Materials.
    pub materials: Vec<Material>,
    /// File textures.
    pub textures: Vec<Texture>,
    /// Media: the image files textures read, maybe embedded.
    pub videos: Vec<Video>,
    /// What was left out or read approximately, each said once.
    pub warnings: Vec<String>,
}

/// A node of the scene's tree (an FBX `Model`).
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// Its id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// Its parent, an index into the scene's models; `None` at the top.
    pub parent: Option<usize>,
    /// What it shows.
    pub attribute: Attribute,
    /// Its transform in its parent.
    pub transform: Transform,
    /// Where what it shows sits in it, its children unmoved.
    pub geometric: Geometric,
    /// Its material slots, indices into the scene's materials: a mesh's
    /// material layer indexes these, so each node showing a mesh may give
    /// it other materials.
    pub materials: Vec<usize>,
    /// Whether it is shown (`Visibility`, `Show`).
    pub visible: bool,
    /// Every property, over its template's defaults (user properties too).
    pub properties: Properties,
}

/// What a node shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Attribute {
    /// Nothing: a group or locator.
    Null,
    /// A mesh, an index into the scene's meshes.
    Mesh(usize),
    /// A camera, an index into the scene's cameras.
    Camera(usize),
    /// A light, an index into the scene's lights.
    Light(usize),
    /// A skeleton joint (`LimbNode`): a node, its skin left out.
    Bone,
    /// Something this reader leaves out, by FBX's name (`NurbsSurface`…).
    Other(String),
}

impl Scene {
    /// The scene in `document`, its arrays moved out of it.
    pub fn from_document(document: Document, options: &Options) -> Result<Scene, Error> {
        build::scene(document, options)
    }

    /// Each node's placement in the scene, in file units and axes: its
    /// parents' transforms and its own, as its inheritance says. Exact for
    /// every inheritance whose parents don't shear (a parent inheriting by
    /// [`Inherit::Matrix`] under a non-uniform scale and a rotation can).
    pub fn world_matrices(&self) -> Vec<Matrix> {
        let mut placed: Vec<Placed> = Vec::with_capacity(self.models.len());
        for model in &self.models {
            let parent = model.parent.map(|p| placed[p]);
            placed.push(Placed::new(&model.transform, parent.as_ref()));
        }
        placed.into_iter().map(|p| p.world).collect()
    }
}
