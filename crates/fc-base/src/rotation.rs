//! Unit quaternion rotation, mirroring FreeCAD `Base::Rotation`.

use crate::matrix::Matrix4;
use crate::vector::Vector3;

/// Error returned when constructing a rotation about a zero-length axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DegenerateAxis;

impl std::fmt::Display for DegenerateAxis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rotation axis must have non-zero length")
    }
}

impl std::error::Error for DegenerateAxis {}

/// Unit quaternion.
///
/// Component order matches FreeCAD's `Base::Rotation` XML attributes
/// `Q0..Q3`: `x, y, z` hold the vector part and `w` (Q3) is the scalar
/// part. Quaternion is kept normalized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Rotation {
    pub const IDENTITY: Self = Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    /// Rotation by `angle_rad` about `axis` (normalized internally).
    /// Matches FreeCAD `Rotation::setValue(axis, angle)`.
    pub fn from_axis_angle(axis: Vector3, angle_rad: f64) -> Result<Self, DegenerateAxis> {
        todo!()
    }

    pub fn around_x(angle_rad: f64) -> Self {
        todo!()
    }

    pub fn around_y(angle_rad: f64) -> Self {
        todo!()
    }

    pub fn around_z(angle_rad: f64) -> Self {
        todo!()
    }

    /// Quaternion composition. `a.multiply(&b)` is the rotation equivalent
    /// to applying `a` first, then `b` (consistent with
    /// `a.to_matrix().multiply(&b.to_matrix())` under the row-vector
    /// convention).
    pub fn multiply(&self, other: &Self) -> Self {
        todo!()
    }

    /// Inverse (conjugate for a unit quaternion).
    pub fn inverse(&self) -> Self {
        todo!()
    }

    /// Equivalent rotation matrix (3x3 block of a `Matrix4`, no translation).
    pub fn to_matrix(&self) -> Matrix4 {
        todo!()
    }

    /// Rotate a vector.
    pub fn rotate_vector(&self, v: &Vector3) -> Vector3 {
        todo!()
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Self::IDENTITY
    }
}
