//! `fc-app` — document model, mirroring FreeCAD's `src/App`.
//!
//! The architectural heart of FreeCAD is the **parametric document**:
//! typed objects carrying named properties, linked into a dependency
//! graph, recomputed topologically when inputs change. This crate
//! reimplements that core:
//!
//! - [`Property`] / [`PropertyContainer`] — FreeCAD's typed property system
//! - [`DocumentObject`] — the node trait (FreeCAD `App::DocumentObject`)
//! - [`Document`] — the object graph (FreeCAD `App::Document`)
//! - [`recompute`] — dependency-ordered execution with cycle detection
//!
//! Geometry lives one layer up in `fc-part`, just as `Mod/Part` sits on
//! top of `App` in FreeCAD.

pub mod document;
pub mod object;
pub mod property;
pub mod recompute;

pub use document::{Document, ObjectId, ObjectSummary};
pub use object::{DocumentObject, ObjectStatus, PropertyContainer};
pub use property::Property;
pub use recompute::RecomputeError;
