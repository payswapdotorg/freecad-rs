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
        todo!()
    }

    pub fn to_matrix(&self) -> Matrix4 {
        todo!()
    }

    pub fn inverse(&self) -> Self {
        todo!()
    }
}

impl Default for Placement {
    fn default() -> Self {
        Self::IDENTITY
    }
}
