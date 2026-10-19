//! 4x4 transform matrix, mirroring FreeCAD `Base::Matrix4D`.

use crate::placement::Placement;
use crate::vector::Vector3;

/// Row-major 4x4 matrix. Follows FreeCAD's **row-vector** convention:
/// points are transformed as `p' = p * M` (row vector times matrix).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix4 {
    /// `m[row][col]`.
    pub m: [[f64; 4]; 4],
}

impl Matrix4 {
    pub const IDENTITY: Self = Self {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// Matrix product `self * other`.
    ///
    /// With the row-vector convention, `self.multiply(&other)` applied to a
    /// point composes as: apply `self` first, then `other`.
    pub fn multiply(&self, other: &Self) -> Self {
        todo!()
    }

    /// Transform a point (translation applied).
    pub fn transform_point(&self, p: &Vector3) -> Vector3 {
        todo!()
    }

    /// Transform a direction (translation ignored).
    pub fn transform_direction(&self, v: &Vector3) -> Vector3 {
        todo!()
    }

    pub fn transpose(&self) -> Self {
        todo!()
    }

    /// General 4x4 inverse via Gauss-Jordan elimination; `None` when
    /// singular.
    pub fn invert(&self) -> Option<Self> {
        todo!()
    }

    /// Pure translation matrix.
    pub fn from_translation(t: &Vector3) -> Self {
        todo!()
    }

    /// Rigid transform from a `Placement` (rotation then translation).
    pub fn from_placement(p: &Placement) -> Self {
        todo!()
    }
}

impl std::ops::Mul for Matrix4 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.multiply(&rhs)
    }
}
