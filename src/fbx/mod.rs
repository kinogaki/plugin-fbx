//! The FBX reader: binary FBX 7.1–7.7 and ASCII FBX 7.x into a [`Scene`].
//!
//! Reading has two layers. [`tree`] reads either format into the same node
//! tree, its arrays decoded side by side (compressed ones inflated with
//! Kinogaki's own DEFLATE). [`Scene`] reads the object graph on top:
//! nodes with their whole transform stack, meshes with every layer
//! element, cameras, lights, materials with every property and the family
//! of shader that wrote them, textures, and media with any embedded bytes.
//! Values stay the file's own, in its axes and units; [`Settings`] says
//! what they are and converts. Animation, skins and blend shapes are left
//! out with warnings: a file is read at one time. FBX 6 is refused.
//!
//! The reader knows no SDK, and every untrusted size is checked against
//! [`Limits`] before it is allocated.
mod error;
mod options;
mod parallel;
mod scene;
pub mod tree;

pub use error::{At, Error};
pub use options::{Limits, Options};
pub use scene::*;

use std::path::Path;

/// Reads a file's scene from its bytes.
pub fn read(bytes: &[u8]) -> Result<Scene, Error> {
    read_with(bytes, &Options::default())
}

/// [`read`] with budgets and cancellation.
pub fn read_with(bytes: &[u8], options: &Options) -> Result<Scene, Error> {
    Scene::from_document(tree::parse_with(bytes, options)?, options)
}

/// Reads the scene of the file at `path`.
pub fn read_file(path: impl AsRef<Path>) -> Result<Scene, Error> {
    read(&std::fs::read(path)?)
}
