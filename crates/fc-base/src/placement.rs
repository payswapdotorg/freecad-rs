//! Rigid transform (position + rotation), mirroring FreeCAD `Base::Placement`.

use crate::matrix::Matrix4;
use crate::rotation::Rotation;
use crate::vector::Vector3;

/// A rigid transform: rotate, then translate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub position: Vector3,
    pub rotation: Rotation,
}

impl Placement {
    pub const IDENTITY: Self = Self {
        position: Vector3::ZERO,
        rotation: Rotation::IDENTITY,
    };

    pub fn new(position: Vector3, rotation: Rotation) -> Self {
        Self { position, rotation }
    }

    /// Composition. `a.multiply(&b)` applies `a` first, then `b`
    /// (consistent with `a.to_matrix().multiply(&b.to_matrix())`).
    pub fn multiply(&self, other: &Self) -> Self {
        // `p * M(a) * M(b) = (p * R(a) + t(a)) * R(b) + t(b)`, so the
        // rotation composes a-then-b and `a`'s position is carried
        // through `b`'s rotation.
        Self {
            position: other.rotation.rotate_vector(&self.position) + other.position,
            rotation: self.rotation.multiply(&other.rotation),
        }
    }

    pub fn to_matrix(&self) -> Matrix4 {
        Matrix4::from_placement(self)
    }

    pub fn inverse(&self) -> Self {
        // `p' = p * R + t` inverts to `p = (p' - t) * R^-1`, i.e. rotate by
        // `R^-1` and translate by `-(t * R^-1)`.
        let rotation = self.rotation.inverse();
        let neg_position = -self.position;
        let position = rotation.rotate_vector(&neg_position);
        Self { position, rotation }
    }
}

impl Default for Placement {
    fn default() -> Self {
        Self::IDENTITY
    }
}
