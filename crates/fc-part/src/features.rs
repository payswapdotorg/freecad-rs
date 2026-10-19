//! Part features: `fc-app` document objects that own `TopoShape`s,
//! mirroring FreeCAD `Mod/Part/App` features (`Part::Box` et al.).

use fc_app::{impl_document_object, Document, DocumentObject};
use fc_base::Placement;

use crate::shape::{Mesh, TopoShape, DEFAULT_DEFLECTION};

/// FreeCAD `Part::Box` — default `Length/Width/Height` 10, like the C++
/// feature.
pub struct BoxFeature {
    pub container: fc_app::PropertyContainer,
    /// Computed by `execute_impl`; `None` until first recompute.
    pub shape: Option<TopoShape>,
}

impl BoxFeature {
    /// Name + FreeCAD default properties (Length 10, Width 10, Height 10,
    /// Placement identity).
    pub fn new(name: &str) -> Self {
        todo!()
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        todo!()
    }
}

impl_document_object!(BoxFeature, "Part::Box");

/// FreeCAD `Part::Sphere` — default `Radius` 5.
pub struct SphereFeature {
    pub container: fc_app::PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl SphereFeature {
    pub fn new(name: &str) -> Self {
        todo!()
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        todo!()
    }
}

impl_document_object!(SphereFeature, "Part::Sphere");

/// FreeCAD `Part::Cylinder` — default `Radius` 2, `Height` 10.
pub struct CylinderFeature {
    pub container: fc_app::PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl CylinderFeature {
    pub fn new(name: &str) -> Self {
        todo!()
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        todo!()
    }
}

impl_document_object!(CylinderFeature, "Part::Cylinder");

/// FreeCAD `Part::Cone` — default `Radius1` 2, `Radius2` 0, `Height` 10.
pub struct ConeFeature {
    pub container: fc_app::PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl ConeFeature {
    pub fn new(name: &str) -> Self {
        todo!()
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        todo!()
    }
}

impl_document_object!(ConeFeature, "Part::Cone");

/// FreeCAD `Part::Torus` — default `Radius1` 10, `Radius2` 2.
pub struct TorusFeature {
    pub container: fc_app::PropertyContainer,
    pub shape: Option<TopoShape>,
}

impl TorusFeature {
    pub fn new(name: &str) -> Self {
        todo!()
    }

    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        todo!()
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
pub struct TransformFeature {
    pub container: fc_app::PropertyContainer,
    /// Transformed source mesh, set by `execute_impl`.
    pub result: Option<Mesh>,
}

impl TransformFeature {
    /// Properties: `Source` (Link, required), `Placement`
    /// (identity), `Scale` (1.0).
    pub fn new(name: &str) -> Self {
        todo!()
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        todo!()
    }
}

impl_document_object!(TransformFeature, "Part::Transform");

/// Tessellated geometry of any part feature (any concrete type), via
/// downcast. `None` for non-part objects or features that have not
/// computed a shape yet.
pub fn mesh_of(obj: &dyn DocumentObject, deflection: f64) -> Option<Mesh> {
    todo!()
}

/// Analytic shape of any part feature, if computed.
pub fn shape_of(obj: &dyn DocumentObject) -> Option<TopoShape> {
    todo!()
}

/// Silences the unused-import warning for the default while the scaffold
/// body is unimplemented; the implementer uses it in `mesh_of`.
const _: () = {
    let _ = DEFAULT_DEFLECTION;
    let _ = Placement::IDENTITY;
};
