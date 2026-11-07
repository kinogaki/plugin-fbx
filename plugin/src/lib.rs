//! FBX as a Studio plugin library (`formats/fbx.dylib`).
use kinogaki_studio_api::export_package;

kinogaki_shared::link!();
export_package!(kinogaki_fbx::package);
