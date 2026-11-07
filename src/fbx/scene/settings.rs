//! `GlobalSettings`: the file's axes and unit, and the conversion into
//! another system.
//!
//! FBX names three axes: up, front (toward the viewer) and the coordinate
//! axis (right), each an axis and a sign. Maya's Y up is right +X, up +Y,
//! front +Z; 3ds Max's and Blender's Z up is right +X, up +Z, front −Y.
//! A file's data is in its own system, so converting is one matrix at the
//! top of the scene; a left-handed file converts to a right-handed system by
//! a mirroring, which turns its polygons' winding.
use super::properties::Properties;
use super::transform::Matrix;

/// What `GlobalSettings` says.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The file's axes.
    pub axes: Axes,
    /// Centimetres in one file unit (`UnitScaleFactor`): 1 for
    /// centimetres, 100 for metres.
    pub unit: f64,
    /// The unit the authoring application worked in
    /// (`OriginalUnitScaleFactor`).
    pub original_unit: f64,
    /// All of `GlobalSettings`' properties.
    pub properties: Properties,
}

/// A system of axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axes {
    /// Right (FBX's coordinate axis).
    pub right: Direction,
    /// Up.
    pub up: Direction,
    /// Front: toward the viewer.
    pub front: Direction,
}

/// A signed axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// +X.
    PosX,
    /// −X.
    NegX,
    /// +Y.
    PosY,
    /// −Y.
    NegY,
    /// +Z.
    PosZ,
    /// −Z.
    NegZ,
}

impl Direction {
    /// An FBX axis (0, 1, 2) and sign (±1).
    fn from_fbx(axis: i64, sign: i64) -> Option<Self> {
        let negative = sign < 0;
        Some(match (axis, negative) {
            (0, false) => Self::PosX,
            (0, true) => Self::NegX,
            (1, false) => Self::PosY,
            (1, true) => Self::NegY,
            (2, false) => Self::PosZ,
            (2, true) => Self::NegZ,
            _ => return None,
        })
    }

    /// The unit vector.
    pub fn vector(self) -> [f64; 3] {
        match self {
            Self::PosX => [1.0, 0.0, 0.0],
            Self::NegX => [-1.0, 0.0, 0.0],
            Self::PosY => [0.0, 1.0, 0.0],
            Self::NegY => [0.0, -1.0, 0.0],
            Self::PosZ => [0.0, 0.0, 1.0],
            Self::NegZ => [0.0, 0.0, -1.0],
        }
    }

    fn axis(self) -> usize {
        self.vector().iter().position(|&x| x != 0.0).unwrap_or(0)
    }
}

impl Axes {
    /// Y up, +Z front, +X right: Maya's, Houdini's, glTF's, and FBX's
    /// default.
    pub const Y_UP: Axes = Axes {
        right: Direction::PosX,
        up: Direction::PosY,
        front: Direction::PosZ,
    };

    /// Z up, −Y front, +X right: 3ds Max's and Blender's.
    pub const Z_UP: Axes = Axes {
        right: Direction::PosX,
        up: Direction::PosZ,
        front: Direction::NegY,
    };

    /// Whether right × up = front.
    pub fn is_right_handed(&self) -> bool {
        let (r, u, f) = (self.right.vector(), self.up.vector(), self.front.vector());
        let cross = [
            r[1] * u[2] - r[2] * u[1],
            r[2] * u[0] - r[0] * u[2],
            r[0] * u[1] - r[1] * u[0],
        ];
        cross == f
    }

    /// The rotation taking coordinates in these axes into `target`'s, or
    /// the mirroring when their handedness differs.
    pub fn to(&self, target: &Axes) -> Matrix {
        let mut m = Matrix([[0.0; 4]; 4]);
        m.0[3][3] = 1.0;
        for (from, to) in [
            (self.right, target.right),
            (self.up, target.up),
            (self.front, target.front),
        ] {
            let (f, t) = (from.vector(), to.vector());
            // A file axis's component lands on the target's same-role axis.
            m.0[to.axis()][from.axis()] = t[to.axis()] * f[from.axis()];
        }
        m
    }

    /// Whether the three axes are distinct.
    fn is_valid(&self) -> bool {
        let mut seen = [false; 3];
        for d in [self.right, self.up, self.front] {
            seen[d.axis()] = true;
        }
        seen == [true; 3]
    }
}

impl Settings {
    /// The settings `properties` give, or FBX's defaults (Y up,
    /// centimetres); `None` beside them when the axes were unusable.
    pub(crate) fn from_properties(properties: Properties) -> (Self, Option<String>) {
        let direction = |axis: &str, default: i64| {
            let sign = properties.int(&format!("{axis}Sign")).unwrap_or(1);
            Direction::from_fbx(properties.int(axis).unwrap_or(default), sign)
        };
        let axes = match (
            direction("CoordAxis", 0),
            direction("UpAxis", 1),
            direction("FrontAxis", 2),
        ) {
            (Some(right), Some(up), Some(front)) => Some(Axes { right, up, front }),
            _ => None,
        };
        let warning = match axes {
            Some(axes) if axes.is_valid() => None,
            _ => Some("the file's axes are unusable; read as Y up".to_owned()),
        };
        let unit = properties
            .number("UnitScaleFactor")
            .filter(|u| u.is_finite() && *u > 0.0);
        let settings = Self {
            axes: axes.filter(Axes::is_valid).unwrap_or(Axes::Y_UP),
            unit: unit.unwrap_or(1.0),
            original_unit: properties
                .number("OriginalUnitScaleFactor")
                .unwrap_or(unit.unwrap_or(1.0)),
            properties,
        };
        (settings, warning)
    }

    /// The matrix taking the file's coordinates into `target` axes and a
    /// unit of `centimetres` (100 for metres): applied above the top nodes,
    /// it places the scene in that system.
    pub fn conversion(&self, target: &Axes, centimetres: f64) -> Matrix {
        let s = self.unit / centimetres;
        Matrix::scaling([s; 3]) * self.axes.to(target)
    }
}
