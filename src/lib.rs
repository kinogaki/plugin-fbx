//! FBX for Kinogaki Studio, read-only.
//!
//! [`fbx`] is the reader: binary FBX 7.1–7.7 and ASCII FBX 7.x into a plain
//! [`fbx::Scene`] of nodes, meshes with their layers, cameras, lights,
//! materials, textures and embedded media, with the file's axes and units.
//! It knows no SDK; the format plugin converts its scene into standard
//! values at its boundary.
//!
//! ```no_run
//! let scene = kinogaki_fbx::fbx::read_file("model.fbx")?;
//! for node in &scene.models {
//!     println!("{} {:?}", node.name, node.attribute);
//! }
//! # Ok::<(), kinogaki_fbx::fbx::Error>(())
//! ```
pub mod fbx;
