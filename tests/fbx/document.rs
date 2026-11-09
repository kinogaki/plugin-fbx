//! File ▸ Open's read: an FBX in its own unit scale and up axis (a
//! left-handed file still turned right-handed), its items as Import reads
//! them into those units.
use crate::common::{fixture, fixtures};
use kinogaki_fbx::Fbx;
use kinogaki_studio_api::geometry::{Units, UpAxis};
use kinogaki_studio_api::{FileFormat, Input, Purpose, Reading};
use kinogaki_studio_api_testing::Unconverted;
use kinogaki_studio_api_testing::format::document;

/// The units File ▸ Open reads the fixture `name` in.
fn opened(name: &str) -> Units {
    let bytes = fixture(name);
    let input = Input::memory(name, &bytes);
    document(&Fbx, &input, &Fbx.options()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn an_fbx_opens_in_its_unit_scale() {
    assert_eq!(
        opened("blender/zup_cm.fbx"),
        Units::new(0.01, UpAxis::Z).unwrap()
    );
    assert_eq!(opened("houdini/box_zup.fbx").up(), UpAxis::Z);
    assert_eq!(opened("houdini/box_yup_left.fbx").up(), UpAxis::Y);
    // Every fixture Import reads (FBX 6.1 it refuses).
    for name in fixtures() {
        let bytes = fixture(&name);
        let reading = Reading::new(Purpose::Objects, Units::METERS, &Unconverted);
        if Fbx
            .read(&Input::memory(&name, &bytes), &Fbx.options(), &reading)
            .is_ok()
        {
            opened(&name);
        }
    }
}
