//! The file's space and the project's: FBX's [`Settings::conversion`]
//! (its axes and unit into the project's) split into a uniform scale and a
//! turn, a mirroring where the file is left-handed. A node's world matrix
//! `W` becomes `Q·W·Q⁻¹` in the project, its translation scaled, and what
//! it shows `s·Q·p`: the hierarchy keeps its shape and no placement carries
//! the change of units.
use crate::fbx::{Axes, Matrix, Settings};
use kinogaki_studio_api::geometry::{Units, UpAxis, Vec3, Xform};

/// The change from a file's space into the project's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Space {
    /// The project's units per file unit.
    pub scale: f64,
    /// The turn (or mirroring) between their axes.
    turn: Xform,
}

impl Space {
    /// The change from `settings`' space into `project`.
    pub fn new(settings: &Settings, project: Units) -> Space {
        let axes = match project.up() {
            UpAxis::Y => Axes::Y_UP,
            UpAxis::Z => Axes::Z_UP,
        };
        let centimetres = project.meters_per_unit() * 100.0;
        let c = xform(&settings.conversion(&axes, centimetres));
        let scale = settings.unit / centimetres;
        let [x, y, z] = c.axes().map(|a| a * (1.0 / scale));
        Space {
            scale,
            turn: Xform::from_axes(x, y, z, Vec3::ZERO),
        }
    }

    /// A node's world matrix in the project.
    pub fn placement(&self, m: &Matrix) -> Xform {
        let inverse = self.turn.inverse().unwrap_or(Xform::IDENTITY);
        let moved = self.turn.compose(&xform(m)).compose(&inverse);
        let rows = moved.rows();
        let t = [rows[0][3], rows[1][3], rows[2][3]].map(|c| c * self.scale);
        Xform::from_rows([
            [rows[0][0], rows[0][1], rows[0][2], t[0]],
            [rows[1][0], rows[1][1], rows[1][2], t[1]],
            [rows[2][0], rows[2][1], rows[2][2], t[2]],
        ])
    }

    /// What `inner` (a file-space matrix: a geometric transform) does to a
    /// point of the file, in the project: `s·Q·inner`.
    pub fn shown(&self, inner: &Matrix) -> Xform {
        let scaled = Xform::from_axes(
            self.turn.axes()[0] * self.scale,
            self.turn.axes()[1] * self.scale,
            self.turn.axes()[2] * self.scale,
            Vec3::ZERO,
        );
        scaled.compose(&xform(inner))
    }

    /// A direction of the file in the project: turned, not scaled.
    pub fn direction(&self, v: [f64; 3]) -> Vec3 {
        self.turn.apply_vector(Vec3::from(v))
    }
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
        let space = Space::new(&settings(Axes::Z_UP, 1.0), metres);
        assert!((space.scale - 0.01).abs() < 1e-15 && determinant(&space.turn) > 0.0);
        let raised = space.placement(&Matrix::translation([0.0, 0.0, 100.0]));
        assert!((raised.origin() - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-12);
        assert!((raised.axes()[1] - Vec3::Y).length() < 1e-12);
        let p = space
            .shown(&Matrix::IDENTITY)
            .apply_point(Vec3::new(0.0, 0.0, 50.0));
        assert!((p - Vec3::new(0.0, 0.5, 0.0)).length() < 1e-12);
    }

    #[test]
    fn a_left_handed_file_mirrors() {
        let left = Axes {
            right: Direction::NegX,
            up: Direction::PosY,
            front: Direction::PosZ,
        };
        let space = Space::new(&settings(left, 100.0), Units::new(1.0, UpAxis::Y).unwrap());
        assert!(determinant(&space.turn) < 0.0);
        // A world matrix of no mirroring stays one.
        assert!(determinant(&space.placement(&Matrix::IDENTITY)) > 0.0);
    }

    #[test]
    fn a_frame_looks_down_its_minus_z() {
        let f = frame(Vec3::ZERO, Vec3::X, Vec3::Y).unwrap();
        assert!((f.apply_vector(Vec3::Z * -1.0) - Vec3::X).length() < 1e-12);
        assert!((f.apply_vector(Vec3::Y) - Vec3::Y).length() < 1e-12);
    }
}
