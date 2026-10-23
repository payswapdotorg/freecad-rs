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
//! - [`transaction`] — undo/redo journal (FreeCAD `App::Transaction`)
//! - [`recompute`] — dependency-ordered execution with cycle detection
//! - [`preservation`] — raw round-trip data the model does not capture
//! - [`ForeignObject`] — an opaque stand-in for unmodeled object types
//!
//! Geometry lives one layer up in `fc-part`, just as `Mod/Part` sits on
//! top of `App` in FreeCAD.

pub mod document;
pub mod expression;
pub mod foreign;
pub mod object;
pub mod preservation;
pub mod property;
pub mod recompute;
pub mod transaction;

pub use document::{Document, ObjectId, ObjectSummary};
pub use foreign::ForeignObject;
pub use object::{DocumentObject, ObjectStatus, PropertyContainer};
pub use preservation::{
    is_preservation_warning, DocumentPreservation, ObjectPreservation, PreservedShape,
    PreservedZipEntry,
};
pub use property::Property;
pub use recompute::RecomputeError;
pub use transaction::{
    HistoryEntry, JournalChange, TransactionId, TransactionInfo, AUTO_TRANSACTION_LABEL,
    UNDO_STACK_CAP,
};
