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
        let mut m = [[0.0; 4]; 4];
        for (i, out_row) in m.iter_mut().enumerate() {
            for (j, out) in out_row.iter_mut().enumerate() {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += self.m[i][k] * other.m[k][j];
                }
                *out = sum;
            }
        }
        Self { m }
    }

    /// Transform a point (translation applied).
    pub fn transform_point(&self, p: &Vector3) -> Vector3 {
        // Row-vector convention: `p' = p * M`, so the translation lives in
        // the last row `m[3]`.
        Vector3::new(
            p.x * self.m[0][0] + p.y * self.m[1][0] + p.z * self.m[2][0] + self.m[3][0],
            p.x * self.m[0][1] + p.y * self.m[1][1] + p.z * self.m[2][1] + self.m[3][1],
            p.x * self.m[0][2] + p.y * self.m[1][2] + p.z * self.m[2][2] + self.m[3][2],
        )
    }

    /// Transform a direction (translation ignored).
    pub fn transform_direction(&self, v: &Vector3) -> Vector3 {
        Vector3::new(
            v.x * self.m[0][0] + v.y * self.m[1][0] + v.z * self.m[2][0],
            v.x * self.m[0][1] + v.y * self.m[1][1] + v.z * self.m[2][1],
            v.x * self.m[0][2] + v.y * self.m[1][2] + v.z * self.m[2][2],
        )
    }

    pub fn transpose(&self) -> Self {
        let mut m = [[0.0; 4]; 4];
        for (i, out_row) in m.iter_mut().enumerate() {
            for (j, out) in out_row.iter_mut().enumerate() {
                *out = self.m[j][i];
            }
        }
        Self { m }
    }

    /// General 4x4 inverse via Gauss-Jordan elimination; `None` when
    /// singular.
    pub fn invert(&self) -> Option<Self> {
        // Gauss-Jordan with partial pivoting on the augmented system
        // `[A | I]`: once `A` is reduced to the identity, the right half
        // holds `A^-1`.
        let mut a = self.m;
        let mut inv = Self::IDENTITY.m;

        // Reference magnitude for the relative singularity test below.
        let mut scale: f64 = 0.0;
        for row in &a {
            for v in row {
                scale = scale.max(v.abs());
            }
        }

        for col in 0..4 {
            // Partial pivoting: pick the row with the largest |entry| in
            // this column at or below the diagonal.
            let mut pivot_row = col;
            let mut pivot_abs = a[col][col].abs();
            for (r, row) in a.iter().enumerate().skip(col + 1) {
                let cand = row[col].abs();
                if cand > pivot_abs {
                    pivot_abs = cand;
                    pivot_row = r;
                }
            }
            // A pivot far below the overall matrix scale means the matrix
            // is (numerically) singular.
            if pivot_abs <= f64::EPSILON * scale {
                return None;
            }
            if pivot_row != col {
                a.swap(col, pivot_row);
                inv.swap(col, pivot_row);
            }

            // Scale the pivot row so the diagonal entry becomes 1.
            let pivot = a[col][col];
            for j in 0..4 {
                a[col][j] /= pivot;
                inv[col][j] /= pivot;
            }

            // Eliminate this column from every other row.
            for r in 0..4 {
                if r != col {
                    let factor = a[r][col];
                    for j in 0..4 {
                        a[r][j] -= factor * a[col][j];
                        inv[r][j] -= factor * inv[col][j];
                    }
                }
            }
        }
        Some(Self { m: inv })
    }

    /// Pure translation matrix.
    pub fn from_translation(t: &Vector3) -> Self {
        let mut m = Self::IDENTITY;
        m.m[3][0] = t.x;
        m.m[3][1] = t.y;
        m.m[3][2] = t.z;
        m
    }

    /// Rigid transform from a `Placement` (rotation then translation).
    pub fn from_placement(p: &Placement) -> Self {
        let mut m = p.rotation.to_matrix();
        m.m[3][0] = p.position.x;
        m.m[3][1] = p.position.y;
        m.m[3][2] = p.position.z;
        m
    }
}

impl std::ops::Mul for Matrix4 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.multiply(&rhs)
    }
}
