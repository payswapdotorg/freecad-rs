//! `fc-base` — foundational layer, mirroring FreeCAD's `src/Base`.
//!
//! Provides the 3D math vocabulary every other crate speaks: vectors,
//! quaternions (`Rotation`), 4x4 transforms (`Matrix4`), rigid transforms
//! (`Placement`), axis-aligned bounding boxes (`BoundBox3`) and a tiny
//! console logger. Dependency-free by design.

pub mod bbox;
pub mod console;
pub mod matrix;
pub mod placement;
pub mod rotation;
pub mod vector;

pub use bbox::BoundBox3;
pub use matrix::Matrix4;
pub use placement::Placement;
pub use rotation::{DegenerateAxis, Rotation};
pub use vector::Vector3;
