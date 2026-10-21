//! Part features: `fc-app` document objects that own `TopoShape`s,
//! mirroring FreeCAD `Mod/Part/App` features (`Part::Box` et al.).
//!
//! Property names/types match FreeCAD (`Length`, `Width`, `Height`,
//! `Radius`, `Radius1`, `Radius2`, `Placement`, `Source`, `Scale`; all
//! dimensions are `App::PropertyFloat`, `Placement` is
//! `App::PropertyPlacement`, `Source` is `App::PropertyLink`).
//!
//! Honest limitation (mirroring the kernel's scope): the primitives'
//! `Placement` is carried as a persisted, editable document property but
//! the analytic `TopoShape` itself is always built in the canonical
//! position documented in [`crate::shape`]; consumers that need placed
//! geometry go through [`TransformFeature`] or `Mesh::transformed`.

use fc_app::{impl_document_object, Document, DocumentObject, Property, PropertyContainer};
use fc_base::Placement;

use crate::shape::{Mesh, TopoShape, DEFAULT_DEFLECTION};

/// Read a required `App::PropertyFloat` (registered by `new`, so a miss
/// means someone tampered with the property map).
fn require_float(container: &PropertyContainer, type_name: &str, key: &str) -> Result<f64, String> {
    container.get_float(key).ok_or_else(|| {
        format!(
            "{type_name} '{}': missing property '{key}'",
            container.name()
        )
    })
}

/// Validate + store a freshly built primitive shape.
fn store_primitive(
    container: &PropertyContainer,
    type_name: &str,
    shape: TopoShape,
    slot: &mut Option<TopoShape>,
) -> Result<(), String> {
    shape
        .validate()
        .map_err(|reason| format!("{type_name} '{}': {reason}", container.name()))?;
    *slot = Some(shape);
    Ok(())
}

/// FreeCAD `Part::Box` — default `Length/Width/Height` 10, like the C++
/// feature.
pub struct BoxFeature {
    pub container: PropertyContainer,
    /// Computed by `execute_impl`; `None` until first recompute.
    pub shape: Option<TopoShape>,
}

impl BoxFeature {
    /// Name + FreeCAD default properties (Length 10, Width 10, Height 10,
    /// Placement identity).
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Length", Property::Float(10.0));
        container.set("Width", Property::Float(10.0));
        container.set("Height", Property::Float(10.0));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            shape: None,
        }
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        let length = require_float(&self.container, "Part::Box", "Length")?;
        let width = require_float(&self.container, "Part::Box", "Width")?;
        let height = require_float(&self.container, "Part::Box", "Height")?;
        let shape = TopoShape::Box {
            length,
            width,
            height,
        };
        store_primitive(&self.container, "Part::Box", shape, &mut self.shape)
    }
}

impl_document_object!(BoxFeature, "Part::Box");

/// FreeCAD `Part::Sphere` — default `Radius` 5.
pub struct SphereFeature {
    pub container: PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl SphereFeature {
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Radius", Property::Float(5.0));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            shape: None,
        }
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        let radius = require_float(&self.container, "Part::Sphere", "Radius")?;
        let shape = TopoShape::Sphere { radius };
        store_primitive(&self.container, "Part::Sphere", shape, &mut self.shape)
    }
}

impl_document_object!(SphereFeature, "Part::Sphere");

/// FreeCAD `Part::Cylinder` — default `Radius` 2, `Height` 10.
pub struct CylinderFeature {
    pub container: PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl CylinderFeature {
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Radius", Property::Float(2.0));
        container.set("Height", Property::Float(10.0));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            shape: None,
        }
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        let radius = require_float(&self.container, "Part::Cylinder", "Radius")?;
        let height = require_float(&self.container, "Part::Cylinder", "Height")?;
        let shape = TopoShape::Cylinder { radius, height };
        store_primitive(&self.container, "Part::Cylinder", shape, &mut self.shape)
    }
}

impl_document_object!(CylinderFeature, "Part::Cylinder");

/// FreeCAD `Part::Cone` — default `Radius1` 2, `Radius2` 0, `Height` 10.
/// `Radius1` is the base (`z = -Height/2`), `Radius2` the top.
pub struct ConeFeature {
    pub container: PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl ConeFeature {
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Radius1", Property::Float(2.0));
        container.set("Radius2", Property::Float(0.0));
        container.set("Height", Property::Float(10.0));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            shape: None,
        }
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        let radius1 = require_float(&self.container, "Part::Cone", "Radius1")?;
        let radius2 = require_float(&self.container, "Part::Cone", "Radius2")?;
        let height = require_float(&self.container, "Part::Cone", "Height")?;
        let shape = TopoShape::Cone {
            radius1,
            radius2,
            height,
        };
        store_primitive(&self.container, "Part::Cone", shape, &mut self.shape)
    }
}

impl_document_object!(ConeFeature, "Part::Cone");

/// FreeCAD `Part::Torus` — default `Radius1` 10, `Radius2` 2.
pub struct TorusFeature {
    pub container: PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl TorusFeature {
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Radius1", Property::Float(10.0));
        container.set("Radius2", Property::Float(2.0));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            shape: None,
        }
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        let radius1 = require_float(&self.container, "Part::Torus", "Radius1")?;
        let radius2 = require_float(&self.container, "Part::Torus", "Radius2")?;
        let shape = TopoShape::Torus { radius1, radius2 };
        store_primitive(&self.container, "Part::Torus", shape, &mut self.shape)
    }
}

impl_document_object!(TorusFeature, "Part::Torus");

/// `Part::Transform` (ours; FreeCAD exposes transforms via other
/// feature names — this one composes cleanly): links to a `Source`
/// object and applies `Placement` and `Scale` to its tessellated
/// geometry.
///
/// This is the canonical **parametric dependency** demo: change the
/// source's parameters and the transform recomputes, dependencies-first.
///
/// Semantics of `Scale`: the source mesh is placed by `Placement`, then
/// the *whole placed result* is scaled uniformly — i.e. the linear block
/// **and** the translation are multiplied by `Scale` (`p ↦ s ·
/// place(p)`). A `Scale` of 0 collapses everything onto the origin; a
/// negative `Scale` mirrors without flipping triangle winding.
pub struct TransformFeature {
    pub container: PropertyContainer,
    /// Transformed source mesh, set by `execute_impl`.
    pub result: Option<Mesh>,
}

impl TransformFeature {
    /// Properties: `Source` (Link, required), `Placement`
    /// (identity), `Scale` (1.0).
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Source", Property::Link(String::new()));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        container.set("Scale", Property::Float(1.0));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "Part::Transform";
        let name = self.container.name();
        let source = match self.container.get("Source") {
            Some(Property::Link(target)) if !target.is_empty() => target.clone(),
            _ => {
                return Err(format!(
                    "{type_name} '{name}': Source must link to a document object"
                ))
            }
        };
        let placement = match self.container.get("Placement") {
            Some(Property::Placement(p)) => *p,
            _ => {
                return Err(format!(
                    "{type_name} '{name}': missing property 'Placement'"
                ))
            }
        };
        let scale = require_float(&self.container, type_name, "Scale")?;
        if !scale.is_finite() {
            return Err(format!("{type_name} '{name}': Scale must be finite"));
        }

        let source_object = doc
            .get_by_name(&source)
            .ok_or_else(|| format!("{type_name} '{name}': source '{source}' not found"))?;
        let mesh = mesh_of(source_object, DEFAULT_DEFLECTION).ok_or_else(|| {
            format!(
                "{type_name} '{name}': source '{source}' has no computed geometry \
                 (not a part feature, or not yet recomputed)"
            )
        })?;

        // Row-vector convention: translation lives in the last row.
        // Scale the linear 3x3 block *and* the translation (see the
        // struct docs).
        let mut m = placement.to_matrix();
        for i in 0..3 {
            for j in 0..3 {
                m.m[i][j] *= scale;
            }
        }
        for j in 0..3 {
            m.m[3][j] *= scale;
        }

        self.result = Some(mesh.transformed(&m));
        Ok(())
    }
}

impl_document_object!(TransformFeature, "Part::Transform");

/// The boolean operation a [`BooleanFeature`] applies — FreeCAD's
/// `Part::Cut`, `Part::Fuse` and `Part::Common`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanKind {
    Cut,
    Fuse,
    Common,
}

impl BooleanKind {
    /// The FreeCAD feature type id.
    pub fn type_name(self) -> &'static str {
        match self {
            BooleanKind::Cut => "Part::Cut",
            BooleanKind::Fuse => "Part::Fuse",
            BooleanKind::Common => "Part::Common",
        }
    }

    /// Inverse of [`BooleanKind::type_name`].
    pub fn from_type_name(type_name: &str) -> Option<Self> {
        match type_name {
            "Part::Cut" => Some(BooleanKind::Cut),
            "Part::Fuse" => Some(BooleanKind::Fuse),
            "Part::Common" => Some(BooleanKind::Common),
            _ => None,
        }
    }

    fn apply(self, base: &Mesh, tool: &Mesh) -> Mesh {
        match self {
            BooleanKind::Cut => crate::boolean::cut(base, tool),
            BooleanKind::Fuse => crate::boolean::fuse(base, tool),
            BooleanKind::Common => crate::boolean::common(base, tool),
        }
    }
}

/// FreeCAD `Part::Cut` / `Part::Fuse` / `Part::Common`: links to a
/// `Base` and a `Tool` object and applies the mesh boolean of
/// [`BooleanKind`] to their tessellated geometry at `execute` time.
///
/// FreeCAD hands these to OCCT's `BRepAlgoAPI` on exact BRep geometry;
/// this kernel runs the BSP mesh boolean ([`crate::boolean`]) on the
/// tessellations — watertight for watertight operands, exact volumes
/// only up to tessellation resolution.
pub struct BooleanFeature {
    pub container: PropertyContainer,
    pub kind: BooleanKind,
    /// Boolean result mesh, set by `execute_impl`.
    pub result: Option<Mesh>,
}

impl BooleanFeature {
    /// Properties: `Base` (Link, required), `Tool` (Link, required).
    pub fn new(name: &str, kind: BooleanKind) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Base", Property::Link(String::new()));
        container.set("Tool", Property::Link(String::new()));
        Self {
            container,
            kind,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = self.kind.type_name();
        let name = self.container.name().to_string();
        let operand =
            |container: &PropertyContainer, doc: &Document, key: &str| -> Result<Mesh, String> {
                let target = match container.get(key) {
                    Some(Property::Link(target)) if !target.is_empty() => target.clone(),
                    _ => {
                        return Err(format!(
                            "{type_name} '{name}': {key} must link to a document object"
                        ))
                    }
                };
                let object = doc
                    .get_by_name(&target)
                    .ok_or_else(|| format!("{type_name} '{name}': {key} '{target}' not found"))?;
                mesh_of(object, DEFAULT_DEFLECTION).ok_or_else(|| {
                    format!(
                        "{type_name} '{name}': {key} '{target}' has no computed geometry \
                     (not a part feature, or not yet recomputed)"
                    )
                })
            };
        let base = operand(&self.container, doc, "Base")?;
        let tool = operand(&self.container, doc, "Tool")?;
        self.result = Some(self.kind.apply(&base, &tool));
        Ok(())
    }
}

// Manual impl: the type name is per-instance (Cut/Fuse/Common).
impl DocumentObject for BooleanFeature {
    fn type_name(&self) -> &'static str {
        self.kind.type_name()
    }
    fn name(&self) -> &str {
        self.container.name()
    }
    fn set_name(&mut self, name: &str) {
        self.container.set_name(name)
    }
    fn properties(&self) -> &std::collections::BTreeMap<String, fc_app::Property> {
        &self.container.properties
    }
    fn properties_mut(&mut self) -> &mut std::collections::BTreeMap<String, fc_app::Property> {
        &mut self.container.properties
    }
    fn expressions(&self) -> &std::collections::BTreeMap<String, String> {
        &self.container.expressions
    }
    fn expressions_mut(&mut self) -> Option<&mut std::collections::BTreeMap<String, String>> {
        Some(&mut self.container.expressions)
    }
    fn execute(&mut self, doc: &Document) -> Result<(), String> {
        self.execute_impl(doc)
    }
    fn status(&self) -> &fc_app::ObjectStatus {
        self.container.status()
    }
    fn set_status(&mut self, status: fc_app::ObjectStatus) {
        self.container.set_status(status)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Tessellated geometry of any part feature (any concrete type), via
/// downcast. `None` for non-part objects or features that have not
/// computed a shape yet.
pub fn mesh_of(obj: &dyn DocumentObject, deflection: f64) -> Option<Mesh> {
    let any = obj.as_any();
    if let Some(f) = any.downcast_ref::<BoxFeature>() {
        return f.shape.as_ref().map(|s| s.tessellate(deflection));
    }
    if let Some(f) = any.downcast_ref::<SphereFeature>() {
        return f.shape.as_ref().map(|s| s.tessellate(deflection));
    }
    if let Some(f) = any.downcast_ref::<CylinderFeature>() {
        return f.shape.as_ref().map(|s| s.tessellate(deflection));
    }
    if let Some(f) = any.downcast_ref::<ConeFeature>() {
        return f.shape.as_ref().map(|s| s.tessellate(deflection));
    }
    if let Some(f) = any.downcast_ref::<TorusFeature>() {
        return f.shape.as_ref().map(|s| s.tessellate(deflection));
    }
    if let Some(f) = any.downcast_ref::<TransformFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<BooleanFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<crate::partdesign::PadFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<crate::partdesign::PocketFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<crate::partdesign::RevolutionFeature>() {
        return f.result.clone();
    }
    None
}

/// Analytic shape of any part feature, if computed.
pub fn shape_of(obj: &dyn DocumentObject) -> Option<TopoShape> {
    let any = obj.as_any();
    if let Some(f) = any.downcast_ref::<BoxFeature>() {
        return f.shape.clone();
    }
    if let Some(f) = any.downcast_ref::<SphereFeature>() {
        return f.shape.clone();
    }
    if let Some(f) = any.downcast_ref::<CylinderFeature>() {
        return f.shape.clone();
    }
    if let Some(f) = any.downcast_ref::<ConeFeature>() {
        return f.shape.clone();
    }
    if let Some(f) = any.downcast_ref::<TorusFeature>() {
        return f.shape.clone();
    }
    if let Some(f) = any.downcast_ref::<TransformFeature>() {
        return f.result.clone().map(TopoShape::Mesh);
    }
    if let Some(f) = any.downcast_ref::<BooleanFeature>() {
        return f.result.clone().map(TopoShape::Mesh);
    }
    if let Some(f) = any.downcast_ref::<crate::partdesign::PadFeature>() {
        return f.result.clone().map(TopoShape::Mesh);
    }
    if let Some(f) = any.downcast_ref::<crate::partdesign::PocketFeature>() {
        return f.result.clone().map(TopoShape::Mesh);
    }
    if let Some(f) = any.downcast_ref::<crate::partdesign::RevolutionFeature>() {
        return f.result.clone().map(TopoShape::Mesh);
    }
    None
}
