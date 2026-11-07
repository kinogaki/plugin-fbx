//! A node's transform: FBX's full stack, and how a child inherits its
//! parent's scale.
//!
//! The local matrix is `T · Roff · Rp · Rpre · R · Rpost⁻¹ · Rp⁻¹ · Soff ·
//! Sp · S · Sp⁻¹` (the FBX SDK's documented order), the pre- and
//! post-rotations always XYZ, `R` in the node's rotation order.
use std::ops::Mul;

/// A 4 × 4 affine matrix by rows, acting on column vectors: the
/// translation is column 3.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [[f64; 4]; 4]);

/// The order Euler angles apply in: `Xyz` turns about X first, then Y,
/// then Z (`EulerXYZ`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RotationOrder {
    /// `EulerXYZ`, FBX's default; also `SphericXYZ`, which is read as it.
    #[default]
    Xyz,
    /// `EulerXZY`.
    Xzy,
    /// `EulerYZX`.
    Yzx,
    /// `EulerYXZ`.
    Yxz,
    /// `EulerZXY`.
    Zxy,
    /// `EulerZYX`.
    Zyx,
}

/// How a node inherits its parent's scale (`InheritType`). Rotation and
/// translation always inherit as matrices do; they differ only under a
/// non-uniformly scaled parent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Inherit {
    /// `RrSs` (0, FBX's default): the parent's scale applies after the
    /// child's rotation, axis by axis, so a child never shears.
    #[default]
    Componentwise,
    /// `RSrs` (1): plain matrix composition, `parent · local`.
    Matrix,
    /// `Rrs` (2): the parent's scale moves the child but doesn't scale it.
    NoParentScale,
}

/// A node's transform stack, angles in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// `Lcl Translation`.
    pub translation: [f64; 3],
    /// `Lcl Rotation`, in `rotation_order`.
    pub rotation: [f64; 3],
    /// `Lcl Scaling`.
    pub scaling: [f64; 3],
    /// `RotationOrder`.
    pub rotation_order: RotationOrder,
    /// `PreRotation` (XYZ).
    pub pre_rotation: [f64; 3],
    /// `PostRotation` (XYZ), applied inverted.
    pub post_rotation: [f64; 3],
    /// `RotationOffset`.
    pub rotation_offset: [f64; 3],
    /// `RotationPivot`.
    pub rotation_pivot: [f64; 3],
    /// `ScalingOffset`.
    pub scaling_offset: [f64; 3],
    /// `ScalingPivot`.
    pub scaling_pivot: [f64; 3],
    /// `InheritType`.
    pub inherit: Inherit,
}

/// The geometric transform: placing a node's mesh, camera or light in the
/// node without moving its children (`GeometricTranslation`, `…Rotation`
/// (XYZ), `…Scaling`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometric {
    /// `GeometricTranslation`.
    pub translation: [f64; 3],
    /// `GeometricRotation`, degrees, XYZ.
    pub rotation: [f64; 3],
    /// `GeometricScaling`.
    pub scaling: [f64; 3],
}

impl Matrix {
    /// No transform.
    pub const IDENTITY: Matrix = Matrix([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    /// A move by `t`.
    pub fn translation(t: [f64; 3]) -> Matrix {
        let mut m = Matrix::IDENTITY;
        (0..3).for_each(|i| m.0[i][3] = t[i]);
        m
    }

    /// A scale by `s` along the axes.
    pub fn scaling(s: [f64; 3]) -> Matrix {
        let mut m = Matrix::IDENTITY;
        (0..3).for_each(|i| m.0[i][i] = s[i]);
        m
    }

    /// Euler angles in degrees, applied in `order`.
    pub fn rotation(degrees: [f64; 3], order: RotationOrder) -> Matrix {
        let axis = |i: usize| {
            let (s, c) = degrees[i].to_radians().sin_cos();
            let (a, b) = ((i + 1) % 3, (i + 2) % 3);
            let mut m = Matrix::IDENTITY;
            m.0[a][a] = c;
            m.0[a][b] = -s;
            m.0[b][a] = s;
            m.0[b][b] = c;
            m
        };
        let [first, second, third] = order.axes();
        axis(third) * axis(second) * axis(first)
    }

    /// The matrix with rows and columns of its linear part swapped and no
    /// translation: a rotation's inverse.
    fn transposed_linear(self) -> Matrix {
        let mut m = Matrix::IDENTITY;
        (0..3).for_each(|r| (0..3).for_each(|c| m.0[r][c] = self.0[c][r]));
        m
    }

    /// Where the point `p` goes.
    pub fn point(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.0;
        std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
    }

    /// Where the direction `v` goes (no translation).
    pub fn vector(&self, v: [f64; 3]) -> [f64; 3] {
        let m = &self.0;
        std::array::from_fn(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
    }

    /// The determinant of the linear part: negative for a mirroring.
    pub fn determinant(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    fn column(&self, c: usize) -> [f64; 3] {
        [self.0[0][c], self.0[1][c], self.0[2][c]]
    }
}

impl Mul for Matrix {
    type Output = Matrix;

    fn mul(self, b: Matrix) -> Matrix {
        let a = &self.0;
        Matrix(std::array::from_fn(|r| {
            std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b.0[k][c]).sum())
        }))
    }
}

impl RotationOrder {
    /// `RotationOrder`'s value (0–6; `SphericXYZ` is 6).
    pub(crate) fn from_fbx(value: i64) -> Self {
        match value {
            1 => Self::Xzy,
            2 => Self::Yzx,
            3 => Self::Yxz,
            4 => Self::Zxy,
            5 => Self::Zyx,
            _ => Self::Xyz,
        }
    }

    /// The axes in the order they apply.
    fn axes(self) -> [usize; 3] {
        match self {
            Self::Xyz => [0, 1, 2],
            Self::Xzy => [0, 2, 1],
            Self::Yzx => [1, 2, 0],
            Self::Yxz => [1, 0, 2],
            Self::Zxy => [2, 0, 1],
            Self::Zyx => [2, 1, 0],
        }
    }
}

impl Inherit {
    /// `InheritType`'s value.
    pub(crate) fn from_fbx(value: i64) -> Self {
        match value {
            1 => Self::Matrix,
            2 => Self::NoParentScale,
            _ => Self::Componentwise,
        }
    }
}

impl Transform {
    /// The rotation part of the stack: `Rpre · R · Rpost⁻¹`.
    fn rotation_matrix(&self) -> Matrix {
        Matrix::rotation(self.pre_rotation, RotationOrder::Xyz)
            * Matrix::rotation(self.rotation, self.rotation_order)
            * Matrix::rotation(self.post_rotation, RotationOrder::Xyz).transposed_linear()
    }

    /// The node in its parent: the whole stack as one matrix.
    pub fn matrix(&self) -> Matrix {
        let t = Matrix::translation;
        let neg = |v: [f64; 3]| v.map(|x| -x);
        t(self.translation)
            * t(self.rotation_offset)
            * t(self.rotation_pivot)
            * self.rotation_matrix()
            * t(neg(self.rotation_pivot))
            * t(self.scaling_offset)
            * t(self.scaling_pivot)
            * Matrix::scaling(self.scaling)
            * t(neg(self.scaling_pivot))
    }
}

impl Geometric {
    /// No geometric transform.
    pub const NONE: Geometric = Geometric {
        translation: [0.0; 3],
        rotation: [0.0; 3],
        scaling: [1.0; 3],
    };

    /// The mesh, camera or light in its node.
    pub fn matrix(&self) -> Matrix {
        Matrix::translation(self.translation)
            * Matrix::rotation(self.rotation, RotationOrder::Xyz)
            * Matrix::scaling(self.scaling)
    }
}

/// A node placed in the scene, as its children need it.
#[derive(Clone, Copy)]
pub(crate) struct Placed {
    pub world: Matrix,
    /// The world without its accumulated scale.
    unscaled: Matrix,
    /// The scale its children inherit, axis by axis.
    scale: [f64; 3],
}

impl Placed {
    /// A node under `parent` (or at the top), inheriting as it says.
    pub(crate) fn new(transform: &Transform, parent: Option<&Placed>) -> Placed {
        let local = transform.matrix();
        let rotation = transform.rotation_matrix();
        let s = transform.scaling;
        let Some(parent) = parent else {
            return Placed {
                world: local,
                unscaled: Matrix::translation(local.column(3)) * rotation,
                scale: s,
            };
        };
        let mul = |a: [f64; 3], b: [f64; 3]| std::array::from_fn(|i| a[i] * b[i]);
        let moved = mul(local.column(3), parent.scale);
        let unscaled = parent.unscaled * Matrix::translation(moved) * rotation;
        let (world, scale) = match transform.inherit {
            Inherit::Matrix => (parent.world * local, mul(parent.scale, s)),
            Inherit::Componentwise => {
                let scale = mul(parent.scale, s);
                (unscaled * Matrix::scaling(scale), scale)
            }
            Inherit::NoParentScale => (unscaled * Matrix::scaling(s), s),
        };
        Placed {
            world,
            unscaled,
            scale,
        }
    }
}
