//! Typed property values, mirroring FreeCAD's `App::Property*` family.

use fc_base::{Placement, Vector3};

/// A property value. Variant names map to FreeCAD property types via
/// [`Property::type_name`].
#[derive(Debug, Clone, PartialEq)]
pub enum Property {
    Bool(bool),
    Float(f64),
    Integer(i64),
    String(String),
    Vector(Vector3),
    Placement(Placement),
    /// Link to another object, by its unique internal name.
    Link(String),
    LinkList(Vec<String>),
}

impl Property {
    /// FreeCAD-compatible type name, e.g. `App::PropertyFloat`.
    pub fn type_name(&self) -> &'static str {
        todo!()
    }

    /// Human/CLI text form: `true`, `2.5`, `7`, `hello`, `1,2,3`,
    /// `pos=(0,0,0)`, link names comma-separated.
    pub fn to_text(&self) -> String {
        todo!()
    }

    /// Parse a CLI value (from `freecad-rs set`) for the property with the
    /// given FreeCAD type name. Returns `None` on unknown type or malformed
    /// text.
    pub fn parse(type_name: &str, text: &str) -> Option<Property> {
        todo!()
    }
}

impl Default for Property {
    fn default() -> Self {
        Property::Bool(false)
    }
}
