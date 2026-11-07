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
//! The reader uses nothing of the SDK but the read's budget
//! ([`ReadBudget`]): every untrusted size is charged to it before it is
//! allocated, nesting stops at its depth, and its cancellation stops a read
//! between nodes and inside each array's inflation.
mod error;
mod scene;
pub mod tree;

pub use error::{At, Error};
pub use scene::*;

use kinogaki_studio_api::format::ReadBudget;
use std::path::Path;

/// Reads a file's scene from its bytes.
pub fn read(bytes: &[u8]) -> Result<Scene, Error> {
    read_with(bytes, &ReadBudget::default())
}

/// [`read`] within `budget`, which also cancels it.
pub fn read_with(bytes: &[u8], budget: &ReadBudget) -> Result<Scene, Error> {
    Scene::from_document(tree::parse_with(bytes, budget)?, budget)
}

/// Reads the scene of the file at `path`: a regular file, never a device
/// or a pipe.
pub fn read_file(path: impl AsRef<Path>) -> Result<Scene, Error> {
    let path = path.as_ref();
    if !std::fs::metadata(path)?.is_file() {
        let why = std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file");
        return Err(Error::Io(why));
    }
    read(&std::fs::read(path)?)
}
