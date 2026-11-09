//! The file's space and the project's: FBX's axes and unit against the
//! project's as the SDK's [`Conversion`] (a uniform scale, and a turn or a
//! mirroring where the file is left-handed). A node's world matrix `W`
//! becomes `Q·W·Q⁻¹` in the project, its translation scaled, and what it
//! shows `s·Q·p`: the hierarchy keeps its shape and no placement carries
//! the change of units. And FBX's matrices as placements.
use crate::fbx::{Axes, Direction, Matrix, Settings};
use kinogaki_studio_api::Error;
use kinogaki_studio_api::format::Conversion;
use kinogaki_studio_api::geometry::{Units, UpAxis, Vec3, Xform};

/// The units of `settings`' space: its unit, and its up axis where that is
/// Y or Z (else Y, which the file's axes turn into).
///
/// # Errors
/// A file unit that isn't a finite length above zero.
pub(crate) fn units(settings: &Settings) -> Result<Units, Error> {
    let up = match settings.axes.up {
        Direction::PosZ | Direction::NegZ => UpAxis::Z,
        _ => UpAxis::Y,
    };
    Units::new(settings.unit / 100.0, up)
}

/// The change from `settings`' space into `project`.
///
/// # Errors
/// A file unit that isn't a finite length above zero.
pub(crate) fn conversion(settings: &Settings, project: Units) -> Result<Conversion, Error> {
    let target = match project.up() {
        UpAxis::Y => Axes::Y_UP,
        UpAxis::Z => Axes::Z_UP,
    };
    let turn = xform(&settings.axes.to(&target));
    let centimetres = project.meters_per_unit() * 100.0;
    Conversion::of_axes(turn.axes(), settings.unit / centimetres)
}

/// What `inner` (a file-space matrix: a geometric transform) does to a
/// point of the file, in the project: `s·Q·inner`.
pub(crate) fn shown(conversion: &Conversion, inner: &Matrix) -> Xform {
    conversion.xform().compose(&xform(inner))
}

/// FBX's matrix (by rows, acting on column vectors) as a placement.
pub(crate) fn xform(m: &Matrix) -> Xform {
    let r = &m.0;
    Xform::from_rows([r[0], r[1], r[2]])
}

/// The determinant of `x`'s linear part: negative for a mirroring.
pub(crate) fn determinant(x: &Xform) -> f64 {
    let [a, b, c] = x.axes();
    a.dot(b.cross(c))
}

/// A frame at `origin` looking down `look` (its −Z) with `up` toward its
/// +Y, both world directions: a camera's or a light's placement.
pub(crate) fn frame(origin: Vec3, look: Vec3, up: Vec3) -> Option<Xform> {
    let z = (look * -1.0).normalized()?;
    let x = up.cross(z).normalized()?;
    let y = z.cross(x);
    Some(Xform::from_axes(x, y, z, origin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fbx::Direction;

    fn settings(axes: Axes, unit: f64) -> Settings {
        Settings {
            axes,
            unit,
            original_unit: unit,
            properties: Default::default(),
        }
    }

    #[test]
    fn a_centimetre_z_up_file_lands_in_metres_y_up_with_no_scale_in_placements() {
        let metres = Units::new(1.0, UpAxis::Y).unwrap();
        let space = conversion(&settings(Axes::Z_UP, 1.0), metres).unwrap();
        assert!((space.scale() - 0.01).abs() < 1e-15 && !space.mirrors());
        let raised = space.placement(&xform(&Matrix::translation([0.0, 0.0, 100.0])));
        assert!((raised.origin() - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-12);
        assert!((raised.axes()[1] - Vec3::Y).length() < 1e-12);
        let p = shown(&space, &Matrix::IDENTITY).apply_point(Vec3::new(0.0, 0.0, 50.0));
        assert!((p - Vec3::new(0.0, 0.5, 0.0)).length() < 1e-12);
    }

    #[test]
    fn a_left_handed_file_mirrors() {
        let left = Axes {
            right: Direction::NegX,
            up: Direction::PosY,
            front: Direction::PosZ,
        };
        let metres = Units::new(1.0, UpAxis::Y).unwrap();
        let space = conversion(&settings(left, 100.0), metres).unwrap();
        assert!(space.mirrors());
        // A world matrix of no mirroring stays one.
        assert!(determinant(&space.placement(&Xform::IDENTITY)) > 0.0);
    }

    #[test]
    fn a_frame_looks_down_its_minus_z() {
        let f = frame(Vec3::ZERO, Vec3::X, Vec3::Y).unwrap();
        assert!((f.apply_vector(Vec3::Z * -1.0) - Vec3::X).length() < 1e-12);
        assert!((f.apply_vector(Vec3::Y) - Vec3::Y).length() < 1e-12);
    }
}
