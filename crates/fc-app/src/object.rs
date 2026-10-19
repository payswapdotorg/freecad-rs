//! Property containers and the document-object trait, mirroring
//! FreeCAD's `App::PropertyContainer` + `App::DocumentObject`.

use std::any::Any;
use std::collections::BTreeMap;

use fc_base::{Placement, Vector3};

use crate::document::Document;
use crate::property::Property;

/// Stable object identifier inside a document (FreeCAD `getID`).
/// Ids are assigned by the document starting at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ObjectId(pub u64);

/// Recompute state of an object (FreeCAD object status, distilled).
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectStatus {
    /// Needs recompute (FreeCAD `Touched`).
    Touched,
    /// Successfully executed at least once.
    Valid,
    /// Last execution failed; carries the reason.
    Error(String),
}

impl Default for ObjectStatus {
    fn default() -> Self {
        Self::Touched
    }
}

impl std::fmt::Display for ObjectStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjectStatus::Touched => write!(f, "Touched"),
            ObjectStatus::Valid => write!(f, "Valid"),
            ObjectStatus::Error(reason) => write!(f, "Error: {reason}"),
        }
    }
}

/// Common property storage every `DocumentObject` embeds.
#[derive(Debug, Default)]
pub struct PropertyContainer {
    name: String,
    /// Named, ordered properties (FreeCAD property order is defined by
    /// registration; `BTreeMap` gives deterministic persistence).
    pub properties: BTreeMap<String, Property>,
    status: ObjectStatus,
}

impl PropertyContainer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            properties: BTreeMap::new(),
            status: ObjectStatus::Touched,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn get(&self, key: &str) -> Option<&Property> {
        self.properties.get(key)
    }

    /// Typed getters return `None` on wrong variant or missing key.
    pub fn get_float(&self, key: &str) -> Option<f64> {
        todo!()
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        todo!()
    }

    pub fn get_vector(&self, key: &str) -> Option<Vector3> {
        todo!()
    }

    pub fn get_placement(&self, key: &str) -> Option<Placement> {
        todo!()
    }

    /// Link or LinkList targets as a flat list.
    pub fn get_links(&self, key: &str) -> Option<Vec<String>> {
        todo!()
    }

    pub fn set(&mut self, key: &str, value: Property) {
        self.properties.insert(key.to_string(), value);
    }

    pub fn status(&self) -> &ObjectStatus {
        &self.status
    }

    pub fn set_status(&mut self, status: ObjectStatus) {
        self.status = status;
    }

    /// Mark as needing recompute.
    pub fn touch(&mut self) {
        self.status = ObjectStatus::Touched;
    }
}

/// A node in the document graph (FreeCAD `App::DocumentObject`).
///
/// Objects own named properties and know how to `execute` themselves from
/// their inputs. `execute` receives read-only access to the document so
/// linked objects can be inspected; the executing object is temporarily
/// taken out of the map, so aliasing is impossible.
pub trait DocumentObject: Send {
    /// C++ class name equivalent, e.g. `Part::Box`.
    fn type_name(&self) -> &'static str;

    /// Unique internal name (FreeCAD `Name`): ASCII, assigned at creation
    /// by the document, never renamed.
    fn name(&self) -> &str;

    fn properties(&self) -> &BTreeMap<String, Property>;

    fn properties_mut(&mut self) -> &mut BTreeMap<String, Property>;

    /// Recompute outputs from inputs. On `Err` the document marks the
    /// object `Error` and aborts the recompute (FreeCAD-style).
    fn execute(&mut self, doc: &Document) -> Result<(), String>;

    fn status(&self) -> &ObjectStatus;

    fn set_status(&mut self, status: ObjectStatus);

    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Implements [`DocumentObject`] for a struct with a `pub container:
/// PropertyContainer` field and an inherent
/// `fn execute_impl(&mut self, doc: &Document) -> Result<(), String>`.
#[macro_export]
macro_rules! impl_document_object {
    ($ty:ty, $type_name:literal) => {
        impl $crate::DocumentObject for $ty {
            fn type_name(&self) -> &'static str {
                $type_name
            }
            fn name(&self) -> &str {
                self.container.name()
            }
            fn properties(&self) -> &std::collections::BTreeMap<String, $crate::Property> {
                &self.container.properties
            }
            fn properties_mut(&mut self) -> &mut std::collections::BTreeMap<String, $crate::Property> {
                &mut self.container.properties
            }
            fn execute(&mut self, doc: &$crate::Document) -> Result<(), String> {
                self.execute_impl(doc)
            }
            fn status(&self) -> &$crate::ObjectStatus {
                self.container.status()
            }
            fn set_status(&mut self, status: $crate::ObjectStatus) {
                self.container.set_status(status)
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
    };
}
