//! Property containers and the document-object trait, mirroring
//! FreeCAD's `App::PropertyContainer` + `App::DocumentObject`.

use std::any::Any;
use std::collections::BTreeMap;

use fc_base::{Placement, Vector3};

use crate::document::Document;
use crate::preservation::ObjectPreservation;
use crate::property::Property;

/// Stable object identifier inside a document (FreeCAD `getID`).
/// Ids are assigned by the document starting at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ObjectId(pub u64);

/// Recompute state of an object (FreeCAD object status, distilled).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ObjectStatus {
    /// Needs recompute (FreeCAD `Touched`).
    #[default]
    Touched,
    /// Successfully executed at least once.
    Valid,
    /// Last execution failed; carries the reason.
    Error(String),
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
    /// Expression bindings (FreeCAD's `App::PropertyExpressionEngine`,
    /// distilled): property name → expression text. Evaluated at
    /// recompute time, dependencies first.
    pub expressions: BTreeMap<String, String>,
    /// Round-trip preservation bag (loader-set, writer-consumed):
    /// raw data the model does not capture but re-emits verbatim.
    /// Empty for every object the API creates — it fills only on load.
    pub preserved: ObjectPreservation,
    status: ObjectStatus,
}

impl PropertyContainer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            properties: BTreeMap::new(),
            expressions: BTreeMap::new(),
            preserved: ObjectPreservation::default(),
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
        match self.properties.get(key)? {
            Property::Float(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        match self.properties.get(key)? {
            Property::String(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn get_vector(&self, key: &str) -> Option<Vector3> {
        match self.properties.get(key)? {
            Property::Vector(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_placement(&self, key: &str) -> Option<Placement> {
        match self.properties.get(key)? {
            Property::Placement(value) => Some(*value),
            _ => None,
        }
    }

    /// Link, LinkSub or LinkList targets as a flat list.
    pub fn get_links(&self, key: &str) -> Option<Vec<String>> {
        match self.properties.get(key)? {
            Property::Link(target) => Some(vec![target.clone()]),
            Property::LinkSub(target, _) if !target.is_empty() => Some(vec![target.clone()]),
            Property::LinkList(targets) => Some(targets.clone()),
            _ => None,
        }
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
    /// C++ class name equivalent, e.g. `Part::Box`. The lifetime is
    /// tied to the object: [`crate::ForeignObject`] answers with its
    /// stored source TypeId (a runtime string, exactly like FreeCAD's
    /// `getTypeId`), not a compile-time literal.
    fn type_name(&self) -> &str;

    /// Unique internal name (FreeCAD `Name`): ASCII, assigned at creation
    /// by the document, never renamed.
    fn name(&self) -> &str;

    /// Write back the (re)assigned unique name. Document-internal: used by
    /// [`crate::Document::add`] after suffixing the base name for
    /// uniqueness (`Box`, `Box001`, ...).
    fn set_name(&mut self, name: &str);

    fn properties(&self) -> &BTreeMap<String, Property>;

    fn properties_mut(&mut self) -> &mut BTreeMap<String, Property>;

    /// Expression bindings (property name → expression text).
    fn expressions(&self) -> &BTreeMap<String, String> {
        // Objects without container-backed storage have no bindings.
        static EMPTY: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(BTreeMap::new)
    }

    fn expressions_mut(&mut self) -> Option<&mut BTreeMap<String, String>> {
        None
    }

    /// The object's round-trip preservation bag (see [`crate::preservation`]).
    /// Objects without container-backed storage have nothing preserved.
    fn preservation(&self) -> &ObjectPreservation {
        static EMPTY: std::sync::OnceLock<ObjectPreservation> = std::sync::OnceLock::new();
        EMPTY.get_or_init(ObjectPreservation::default)
    }

    /// Mutable access to the preservation bag; `None` for objects
    /// without container-backed storage (nothing can be preserved on
    /// them, so the loader never needs it).
    fn preservation_mut(&mut self) -> Option<&mut ObjectPreservation> {
        None
    }

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
            fn set_name(&mut self, name: &str) {
                self.container.set_name(name)
            }
            fn properties(&self) -> &std::collections::BTreeMap<String, $crate::Property> {
                &self.container.properties
            }
            fn properties_mut(
                &mut self,
            ) -> &mut std::collections::BTreeMap<String, $crate::Property> {
                &mut self.container.properties
            }
            fn expressions(&self) -> &std::collections::BTreeMap<String, String> {
                &self.container.expressions
            }
            fn expressions_mut(
                &mut self,
            ) -> Option<&mut std::collections::BTreeMap<String, String>> {
                Some(&mut self.container.expressions)
            }
            fn preservation(&self) -> &$crate::preservation::ObjectPreservation {
                &self.container.preserved
            }
            fn preservation_mut(
                &mut self,
            ) -> Option<&mut $crate::preservation::ObjectPreservation> {
                Some(&mut self.container.preserved)
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

/// All link targets of an object, in property-key order: `Link` +
/// `LinkSub` + `LinkList` properties **plus** the objects referenced by
/// bound expressions (their dependency edges are just as real). Shared
/// by the document graph walks (link listing, dependent traversal,
/// topological recompute, DOT export).
pub(crate) fn link_targets(object: &dyn DocumentObject) -> Vec<String> {
    let mut targets = Vec::new();
    for property in object.properties().values() {
        match property {
            // An empty target is FreeCAD's "no link" (real files write
            // `<Link value=""/>`, e.g. a `PartDesign::Body`'s unset
            // `BaseFeature`) — not a dangling reference.
            Property::Link(target) if !target.is_empty() => targets.push(target.clone()),
            // A LinkSub's edge is to the whole target object — a
            // `MirrorPlane` → datum link orders the recompute exactly
            // like a plain link (the subelements name faces/edges of
            // the same object).
            Property::LinkSub(target, _) if !target.is_empty() => targets.push(target.clone()),
            Property::LinkList(list) => {
                targets.extend(list.iter().filter(|t| !t.is_empty()).cloned())
            }
            _ => {}
        }
    }
    for text in object.expressions().values() {
        if let Ok(expression) = crate::expression::parse(text) {
            for (object_name, _) in expression.variables() {
                if let Some(name) = object_name {
                    targets.push(name);
                }
            }
        }
        // A binding that no longer parses keeps its property edge only;
        // recompute reports the broken expression itself.
    }
    targets
}
