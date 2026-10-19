//! Axis-aligned bounding box, mirroring FreeCAD `Base::BoundBox3d`.

use crate::vector::Vector3;

/// Axis-aligned bounding box. Invariant: `min <= max` per axis when
/// non-empty; `is_empty()` reports the unset/degenerate state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundBox3 {
    pub min: Vector3,
    pub max: Vector3,
}

impl BoundBox3 {
    pub fn new(min: Vector3, max: Vector3) -> Self {
        Self { min, max }
    }

    /// Smallest box containing all points; `None` when `points` is empty.
    pub fn from_points(points: &[Vector3]) -> Option<Self> {
        todo!()
    }

    /// Grow to include `p` (no-op semantics undefined for empty boxes are
    /// not acceptable: initialize from the first point).
    pub fn add_point(&mut self, p: &Vector3) {
        todo!()
    }

    /// Grow to include `other`.
    pub fn add_box(&mut self, other: &Self) {
        todo!()
    }

    pub fn contains_point(&self, p: &Vector3) -> bool {
        todo!()
    }

    pub fn center(&self) -> Vector3 {
        todo!()
    }

    /// Extent per axis (`max - min`).
    pub fn size(&self) -> Vector3 {
        todo!()
    }

    /// True when min exceeds max on any axis.
    pub fn is_empty(&self) -> bool {
        todo!()
    }

    pub fn volume(&self) -> f64 {
        todo!()
    }
}

impl Default for BoundBox3 {
    fn default() -> Self {
        Self {
            min: Vector3::new(f64::MAX, f64::MAX, f64::MAX),
            max: Vector3::new(f64::MIN, f64::MIN, f64::MIN),
        }
    }
}
