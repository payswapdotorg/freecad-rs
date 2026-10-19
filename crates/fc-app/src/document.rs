//! The parametric document: object graph + name registry, mirroring
//! FreeCAD `App::Document`.

use std::collections::BTreeMap;

pub use crate::object::{DocumentObject, ObjectId, ObjectStatus};
use crate::property::Property;
use crate::recompute::RecomputeError;

/// Flat, serializable view of one object (for CLI/JSON output).
#[derive(Debug, Clone)]
pub struct ObjectSummary {
    pub id: ObjectId,
    pub name: String,
    pub type_name: String,
    pub status: ObjectStatus,
}

/// A FreeCAD-style document: an ordered collection of typed, named,
/// linked objects that recompute dependencies-first.
#[derive(Default)]
pub struct Document {
    label: String,
    creator: String,
    /// RFC 3339 UTC creation timestamp.
    created: String,
    objects: BTreeMap<ObjectId, Box<dyn DocumentObject>>,
    /// Creation order for stable listings.
    order: Vec<ObjectId>,
    /// Unique name -> id.
    index: BTreeMap<String, ObjectId>,
    next_id: ObjectId,
}

impl Document {
    /// New empty document. `created` is stamped here.
    pub fn new(label: impl Into<String>) -> Self {
        todo!()
    }

    pub fn label(&self) -> &str {
        todo!()
    }

    pub fn set_label(&mut self, label: &str) {
        self.label = label.to_string();
    }

    pub fn creator(&self) -> &str {
        &self.creator
    }

    pub fn set_creator(&mut self, creator: &str) {
        self.creator = creator.to_string();
    }

    pub fn created(&self) -> &str {
        &self.created
    }

    /// Overwrite the creation stamp (used by the FCStd loader, which
    /// restores the persisted value).
    pub fn set_created(&mut self, created: String) {
        self.created = created;
    }

    /// Insert an object. The document assigns the id, keeps the object's
    /// current name as a *base*, and may suffix it for uniqueness
    /// (`Box`, `Box001`, `Box002`, FreeCAD-style). The final name is
    /// written back onto the object.
    pub fn add(&mut self, object: Box<dyn DocumentObject>) -> ObjectId {
        todo!()
    }

    /// The name `add` would assign for base `base` if it were added now.
    pub fn unique_name(&self, base: &str) -> String {
        todo!()
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn get(&self, id: ObjectId) -> Option<&dyn DocumentObject> {
        todo!()
    }

    pub fn get_by_name(&self, name: &str) -> Option<&dyn DocumentObject> {
        todo!()
    }

    pub fn get_id(&self, name: &str) -> Option<ObjectId> {
        todo!()
    }

    /// Object summaries in creation order.
    pub fn summary(&self) -> Vec<ObjectSummary> {
        todo!()
    }

    /// Link targets of `name` (from Link/LinkList properties).
    pub fn links_of(&self, name: &str) -> Vec<String> {
        todo!()
    }

    /// Names of objects that (transitively) link to `name`.
    pub fn dependents_of(&self, name: &str) -> Vec<String> {
        todo!()
    }

    /// Remove an object. Fails when other objects still link to it
    /// (list them in the error) — parametric integrity, FreeCAD-style.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        todo!()
    }

    /// Set one property and mark the object **and its transitive
    /// dependents** as touched. This is the trigger of the parametric
    /// cascade.
    pub fn set_property(&mut self, name: &str, key: &str, value: Property) -> Result<(), String> {
        todo!()
    }

    /// Recompute all touched objects, dependencies first.
    /// Returns the ids that were recomputed, in execution order.
    pub fn recompute_all(&mut self) -> Result<Vec<ObjectId>, RecomputeError> {
        todo!()
    }

    /// Graphviz DOT rendering of the dependency graph. Edge `A -> B`
    /// means *A depends on B*.
    pub fn dependencies_dot(&self) -> String {
        todo!()
    }
}

/// Take an object out of the map so it can be executed with `&Document`
/// (aliasing-free). Not part of the public API.
pub(crate) fn take_out(
    doc: &mut Document,
    id: ObjectId,
) -> Option<Box<dyn DocumentObject>> {
    doc.objects.remove(&id)
}

/// Put a (possibly executed) object back. Not part of the public API.
pub(crate) fn put_back(doc: &mut Document, id: ObjectId, object: Box<dyn DocumentObject>) {
    doc.objects.insert(id, object);
}

/// Direct mutable access (used by the recompute loop and the loader).
pub(crate) fn get_mut(doc: &mut Document, id: ObjectId) -> Option<&mut dyn DocumentObject> {
    doc.objects
        .get_mut(&id)
        .map(|b| &mut **b as &mut dyn DocumentObject)
}
