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
        let (first, rest) = points.split_first()?;
        let mut b = Self {
            min: *first,
            max: *first,
        };
        for p in rest {
            b.add_point(p);
        }
        Some(b)
    }

    /// Grow to include `p`. An empty box (see `is_empty`) initializes from
    /// the first point instead of staying degenerate.
    pub fn add_point(&mut self, p: &Vector3) {
        if self.is_empty() {
            self.min = *p;
            self.max = *p;
        } else {
            self.min = Vector3::new(
                self.min.x.min(p.x),
                self.min.y.min(p.y),
                self.min.z.min(p.z),
            );
            self.max = Vector3::new(
                self.max.x.max(p.x),
                self.max.y.max(p.y),
                self.max.z.max(p.z),
            );
        }
    }

    /// Grow to include `other`. Adding an empty box is a no-op.
    pub fn add_box(&mut self, other: &Self) {
        if other.is_empty() {
            return;
        }
        // An empty `self` initializes from `other`; otherwise both corners
        // extend the box to the union.
        self.add_point(&other.min);
        self.add_point(&other.max);
    }

    pub fn contains_point(&self, p: &Vector3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    pub fn center(&self) -> Vector3 {
        (self.min + self.max) * 0.5
    }

    /// Extent per axis (`max - min`).
    pub fn size(&self) -> Vector3 {
        self.max - self.min
    }

    /// True when min exceeds max on any axis.
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    /// Box volume. Empty boxes report 0 rather than a meaningless product
    /// of negative extents.
    pub fn volume(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            let s = self.size();
            s.x * s.y * s.z
        }
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
