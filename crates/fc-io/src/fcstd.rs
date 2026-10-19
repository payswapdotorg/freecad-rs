//! `.FCStd` reader/writer. See the crate docs for the format.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use fc_app::{Document, ObjectStatus, Property};
use fc_base::Placement;
use fc_part::brep;
use fc_part::features::{
    BoxFeature, ConeFeature, CylinderFeature, SphereFeature, TorusFeature, TransformFeature,
};

#[derive(Debug)]
pub enum FcstdError {
    Io(std::io::Error),
    Zip(String),
    Xml(String),
    Unsupported(String),
}

impl std::fmt::Display for FcstdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FcstdError::Io(e) => write!(f, "i/o error: {e}"),
            FcstdError::Zip(e) => write!(f, "zip container error: {e}"),
            FcstdError::Xml(e) => write!(f, "Document.xml error: {e}"),
            FcstdError::Unsupported(e) => write!(f, "unsupported: {e}"),
        }
    }
}

impl std::error::Error for FcstdError {}

impl From<std::io::Error> for FcstdError {
    fn from(e: std::io::Error) -> Self {
        FcstdError::Io(e)
    }
}

/// What happened while loading (never fatal by itself; warnings are
/// reported, objects are still created when possible).
#[derive(Debug, Default, Clone)]
pub struct LoadReport {
    pub warnings: Vec<String>,
    /// Objects successfully constructed.
    pub objects: usize,
    /// Objects skipped entirely (unknown type, unreadable).
    pub skipped: usize,
}

/// Save `doc` as `.FCStd`. Writes `Document.xml` first, then one FCBREP
/// side file per part feature (`Shape001.brp`, `Shape002.brp`, ...).
/// Deflate-compressed entries, like FreeCAD.
pub fn save_document(doc: &Document, path: &Path) -> Result<(), FcstdError> {
    todo!()
}

/// Load a `.FCStd`. Accepts our own files and real FreeCAD files (see
/// crate docs for fidelity limits). Object ids from `<Objects>` are
/// respected so links keep resolving.
pub fn load_document(path: &Path) -> Result<(Document, LoadReport), FcstdError> {
    todo!()
}

// ---------------------------------------------------------------------------
// Implementation notes (binding, for the implementer):
//
// * Use `zip::ZipWriter` (deflate) / `zip::ZipArchive`.
// * XML: `quick-xml` events, or hand-write with `quick_xml::Writer`.
// * Property `status` attribute on `<Object>` in `<Objects>`: we write
//   `status="Touched"|"Valid"` ourselves (FreeCAD writes `Touched="1"`
//   and `Invalid="1"`); our reader accepts BOTH dialects and maps
//   Touched="1" -> Touched, Invalid="1" -> Error("invalid").
// * Unknown property types while reading: skip the element, warn, keep
//   the object.
// * Shape files: probe each payload; OcctBrep -> warning + no shape
//   (object stays Touched so `recompute` can regenerate what it can);
//   FcBrep -> parse and attach.
// * FreeCAD writes floats with full precision; match with
//   `format!("{{}}", v)` equivalents (Rust `{}` for f64 prints shortest
//   round-trip — good).
// * The `LinkList` element in real FreeCAD files is
//   `<LinkList value="A,B"/>`; ours matches.
// ---------------------------------------------------------------------------

/// Escape a string for an XML attribute body.
pub(crate) fn escape_attr(s: &str) -> String {
    todo!()
}

/// Map a FreeCAD type id (e.g. `Part::Box`) to a fresh object instance.
/// Returns `None` for unknown types (the loader reports + skips).
pub(crate) fn build_object(
    type_id: &str,
    name: &str,
) -> Option<Box<dyn fc_app::DocumentObject>> {
    todo!()
}
