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
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Rotation by `angle_rad` about `axis` (normalized internally).
    /// Matches FreeCAD `Rotation::setValue(axis, angle)`:
    /// `q = [axis_normalized * sin(theta/2), cos(theta/2)]`.
    pub fn from_axis_angle(axis: Vector3, angle_rad: f64) -> Result<Self, DegenerateAxis> {
        let n = axis.normalized().ok_or(DegenerateAxis)?;
        Ok(Self::from_unit_axis(n, angle_rad))
    }

    pub fn around_x(angle_rad: f64) -> Self {
        Self::from_unit_axis(Vector3::X, angle_rad)
    }

    pub fn around_y(angle_rad: f64) -> Self {
        Self::from_unit_axis(Vector3::Y, angle_rad)
    }

    pub fn around_z(angle_rad: f64) -> Self {
        Self::from_unit_axis(Vector3::Z, angle_rad)
    }

    /// Quaternion composition. `a.multiply(&b)` is the rotation equivalent
    /// to applying `a` first, then `b` (consistent with
    /// `a.to_matrix().multiply(&b.to_matrix())` under the row-vector
    /// convention).
    pub fn multiply(&self, other: &Self) -> Self {
        // Under the row-vector convention `p' = p * M`, "apply `self`,
        // then `other`" composes matrices as `M(self) * M(other)`, which
        // corresponds to the Hamilton product `other ⊗ self` (note the
        // reversed operand order compared to the column-vector world).
        Self {
            x: other.w * self.x + other.x * self.w + other.y * self.z - other.z * self.y,
            y: other.w * self.y - other.x * self.z + other.y * self.w + other.z * self.x,
            z: other.w * self.z + other.x * self.y - other.y * self.x + other.z * self.w,
            w: other.w * self.w - other.x * self.x - other.y * self.y - other.z * self.z,
        }
        .renormalized()
    }

    /// Inverse (conjugate for a unit quaternion).
    pub fn inverse(&self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
        .renormalized()
    }

    /// Equivalent rotation matrix (3x3 block of a `Matrix4`, no translation).
    pub fn to_matrix(&self) -> Matrix4 {
        // Defensive renormalization so a slightly denormalized quaternion
        // (e.g. parsed from file) still yields a rigid rotation.
        let q = self.renormalized();
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);

        let mut m = Matrix4::IDENTITY;
        // Transpose of the common column-vector quaternion matrix: rows
        // are the images of the basis vectors under `p' = p * M`.
        m.m[0][0] = 1.0 - 2.0 * (yy + zz);
        m.m[0][1] = 2.0 * (xy + wz);
        m.m[0][2] = 2.0 * (xz - wy);
        m.m[1][0] = 2.0 * (xy - wz);
        m.m[1][1] = 1.0 - 2.0 * (xx + zz);
        m.m[1][2] = 2.0 * (yz + wx);
        m.m[2][0] = 2.0 * (xz + wy);
        m.m[2][1] = 2.0 * (yz - wx);
        m.m[2][2] = 1.0 - 2.0 * (xx + yy);
        m
    }

    /// Rotate a vector.
    pub fn rotate_vector(&self, v: &Vector3) -> Vector3 {
        // `q v q^-1` expanded as `v + 2w(u×v) + 2u×(u×v)` for `q = (u, w)`.
        let q = self.renormalized();
        let u = Vector3::new(q.x, q.y, q.z);
        let t = u.cross(v) * 2.0;
        *v + t * q.w + u.cross(&t)
    }

    /// Quaternion for a rotation about `axis`, which must be unit-length.
    fn from_unit_axis(axis: Vector3, angle_rad: f64) -> Self {
        let half = angle_rad * 0.5;
        let sin = half.sin();
        Self {
            x: axis.x * sin,
            y: axis.y * sin,
            z: axis.z * sin,
            w: half.cos(),
        }
        .renormalized()
    }

    /// Scale to unit length; guards the invariant that a `Rotation` is a
    /// unit quaternion against accumulated rounding.
    fn renormalized(self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if len > 0.0 && len.is_finite() {
            Self {
                x: self.x / len,
                y: self.y / len,
                z: self.z / len,
                w: self.w / len,
            }
        } else {
            self
        }
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Self::IDENTITY
    }
}
