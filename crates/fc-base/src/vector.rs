//! 3D vector, mirroring FreeCAD `Base::Vector3`.

use std::ops::{Add, Mul, Neg, Sub};

/// A 3D vector of `f64`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub const X: Self = Self {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    pub const Y: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    pub const Z: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Euclidean dot product.
    pub fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Right-handed cross product (`self x other`).
    pub fn cross(&self, other: &Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Euclidean length.
    pub fn length(&self) -> f64 {
        self.length_squared().sqrt()
    }

    /// Squared Euclidean length (no sqrt).
    pub fn length_squared(&self) -> f64 {
        self.dot(self)
    }

    /// Unit-length copy. `None` for the zero vector (we prefer an explicit
    /// result over FreeCAD's silent no-op `Normalize`).
    pub fn normalized(&self) -> Option<Self> {
        let len = self.length();
        if len > 0.0 {
            Some(Self::new(self.x / len, self.y / len, self.z / len))
        } else {
            None
        }
    }

    /// Euclidean distance between two points.
    pub fn distance(a: &Self, b: &Self) -> f64 {
        (*a - *b).length()
    }
}

impl Add for Vector3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vector3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f64> for Vector3 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Neg for Vector3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}
