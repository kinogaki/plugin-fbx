//! FBX as a read-only format plugin for Kinogaki Studio.
//!
//! [`fbx`] is the reader: binary FBX 7.1–7.7 and ASCII FBX 7.x into a plain
//! [`fbx::Scene`] of nodes, meshes with their layers, cameras, lights,
//! materials, textures and embedded media, with the file's axes and units.
//! It knows no SDK but its workers. The format converts its scene into the
//! standard values at its boundary (`read`) and nowhere else: placed in the
//! project's units and axes, its materials as MaterialX networks of the
//! model each shader family records into.
//!
//! ```no_run
//! let scene = kinogaki_fbx::fbx::read_file("model.fbx")?;
//! for node in &scene.models {
//!     println!("{} {:?}", node.name, node.attribute);
//! }
//! # Ok::<(), kinogaki_fbx::fbx::Error>(())
//! ```
pub mod fbx;

mod read;

use kinogaki_studio_api::format::options;
use kinogaki_studio_api::params::Schema;
use kinogaki_studio_api::{
    DataTypeId, Error, FileFormat, FormatId, FormatOptions, Imported, Input, Package, PackageId,
    Purpose, Reading, Value, standard,
};
use std::sync::LazyLock;

/// The package's id.
pub fn id() -> PackageId {
    PackageId::declared("kinogaki.fbx")
}

/// The format's id.
pub fn format() -> FormatId {
    FormatId::declared("kinogaki.format.fbx")
}

/// The package: the format alone.
pub fn package() -> Package {
    Package::new(id(), "FBX").with_format(Fbx)
}

/// What an FBX file holds: materials, polygons, instances of shared meshes,
/// lights and cameras.
static TYPES: LazyLock<[DataTypeId; 5]> = LazyLock::new(|| {
    [
        standard::materials().id(),
        standard::geometry().id(),
        standard::instances().id(),
        standard::light().id(),
        standard::camera().id(),
    ]
});

/// The format: it reads, and writes nothing (D24).
pub struct Fbx;

impl FileFormat for Fbx {
    fn id(&self) -> FormatId {
        format()
    }
    fn label(&self) -> &str {
        "Autodesk FBX"
    }
    fn extensions(&self) -> &[&str] {
        &["fbx"]
    }
    /// Binary by Kaydara's signature, ASCII by its first comment.
    fn detects(&self, head: &[u8]) -> bool {
        fbx::tree::detect(head).is_some()
    }
    fn produces(&self) -> &[DataTypeId] {
        &*TYPES
    }
    /// Textures on disk stay paths unless embedded; embedded media are
    /// embedded whatever it says. Read Files Outside the Folder, off.
    fn options(&self) -> FormatOptions {
        FormatOptions::from([
            (options::EMBED.to_owned(), Value::Bool(false)),
            (options::OUTSIDE.to_owned(), Value::Bool(false)),
        ])
    }
    fn schema(&self, chosen: &FormatOptions, _: Purpose) -> Schema {
        let on = |name| options::option(chosen, name, Value::bool).unwrap_or(false);
        options::outside(
            options::embed(Schema::default(), on(options::EMBED)),
            on(options::OUTSIDE),
        )
    }
    fn read(
        &self,
        input: &Input<'_>,
        options: &FormatOptions,
        reading: &Reading<'_>,
    ) -> Result<Imported, Error> {
        read::read(input, options, reading)
    }
}
