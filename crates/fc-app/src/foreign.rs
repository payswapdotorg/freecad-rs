//! `ForeignObject` — an object of a type this kernel does not model,
//! preserved as an opaque blob.
//!
//! FreeCAD documents routinely carry object types outside any
//! re-implementation's vocabulary (`Spreadsheet::Sheet`, `Mesh::Feature`,
//! FEM objects, workbench-specific features...). The old loader policy
//! skipped them (honest, but lossy: a re-save dropped their data and
//! every link into them dangled). The preservation policy keeps them:
//! a `ForeignObject` stands in the document under its real name and
//! type, carries no modeled state of its own (nothing to drift out of
//! sync), executes as a no-op, and re-emits its original `<ObjectData>`
//! section verbatim on save — so a load→save round-trip is lossless and
//! links to it resolve.
//!
//! FreeCAD-parity note: FreeCAD's own reader demands a registered C++
//! type for every `<Object>` row; when it lacks one it fails the restore.
//! The clean-room stance here is the opposite of failure: keep the bytes,
//! name the type, execute nothing.

use std::collections::BTreeMap;

use crate::document::Document;
use crate::object::{DocumentObject, ObjectStatus, PropertyContainer};
use crate::property::Property;

/// An opaque stand-in for an object type this kernel does not model.
pub struct ForeignObject {
    /// Common storage: name, status, expressions. The property map stays
    /// empty by design — the object's data lives in `raw_object_data`
    /// as raw XML, and letting modeled properties shadow parts of it
    /// would create two truths for one element.
    pub container: PropertyContainer,
    /// The source TypeId exactly as the file spelled it (e.g.
    /// `Mesh::Feature`): listed and re-saved verbatim.
    pub type_id: String,
    /// Attributes of the `<Object>` element beyond `name` (e.g. FreeCAD's
    /// `Extensions="True"`), re-emitted on the regenerated element.
    pub object_attrs: Vec<(String, String)>,
    /// The raw inner XML of the object's `<ObjectData>` entry (everything
    /// between `<Object name="…">` and `</Object>`), re-emitted verbatim.
    /// `None` when the file named the object but gave it no ObjectData.
    pub raw_object_data: Option<String>,
}

impl ForeignObject {
    pub fn new(name: &str, type_id: &str) -> Self {
        Self {
            container: PropertyContainer::new(name),
            type_id: type_id.to_string(),
            object_attrs: Vec::new(),
            raw_object_data: None,
        }
    }
}

impl DocumentObject for ForeignObject {
    fn type_name(&self) -> &str {
        // The real FreeCAD type: list output and the saved `<Objects>`
        // row both show what the file carried, not a placeholder.
        &self.type_id
    }
    fn name(&self) -> &str {
        self.container.name()
    }
    fn set_name(&mut self, name: &str) {
        self.container.set_name(name)
    }
    fn properties(&self) -> &BTreeMap<String, Property> {
        &self.container.properties
    }
    fn properties_mut(&mut self) -> &mut BTreeMap<String, Property> {
        &mut self.container.properties
    }
    fn expressions(&self) -> &BTreeMap<String, String> {
        &self.container.expressions
    }
    fn expressions_mut(&mut self) -> Option<&mut BTreeMap<String, String>> {
        Some(&mut self.container.expressions)
    }
    // The preservation bag rides in the container (a foreign object's
    // data lives in `raw_object_data` as one blob, but its shape
    // payloads and any modeled-but-unhandled properties preserve the
    // same way every container-backed object's do).
    fn preservation(&self) -> &crate::preservation::ObjectPreservation {
        &self.container.preserved
    }
    fn preservation_mut(&mut self) -> Option<&mut crate::preservation::ObjectPreservation> {
        Some(&mut self.container.preserved)
    }
    fn execute(&mut self, _doc: &Document) -> Result<(), String> {
        // Opaque payload: there is nothing this kernel could recompute,
        // and "executing" foreign data it cannot read would only ever be
        // a destruction. A no-op keeps the object Valid/stable across
        // document recomputes.
        Ok(())
    }
    fn status(&self) -> &ObjectStatus {
        self.container.status()
    }
    fn set_status(&mut self, status: ObjectStatus) {
        self.container.set_status(status)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl std::fmt::Debug for ForeignObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForeignObject")
            .field("name", &self.container.name())
            .field("type_id", &self.type_id)
            .field(
                "raw_object_data",
                &self.raw_object_data.as_ref().map(|s| s.len()),
            )
            .finish()
    }
}
