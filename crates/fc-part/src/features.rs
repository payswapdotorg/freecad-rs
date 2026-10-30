//! Part features: `fc-app` document objects that own `TopoShape`s,
//! mirroring FreeCAD `Mod/Part/App` features (`Part::Box` et al.).
//!
//! Property names/types match FreeCAD (`Length`, `Width`, `Height`,
//! `Radius`, `Radius1`, `Radius2`, `Placement`, `Source`, `Scale`; all
//! dimensions are `App::PropertyFloat`, `Placement` is
//! `App::PropertyPlacement`, `Source` is `App::PropertyLink`).
//!
//! Honest scope note: the primitives' `Placement` positions their
//! local shape in global space at consumption time
//! ([`placed_mesh_of`]) — the same rule FreeCAD applies when it hands
//! out `Part::Feature::Shape`. `Part::Transform` is the one exception:
//! it bakes its Placement (and Scale) into the computed mesh itself.

use fc_app::{impl_document_object, Document, DocumentObject, Property, PropertyContainer};
use fc_base::{Matrix4, Placement, Vector3};

use crate::brep_boolean::{
    box_box_boolean, box_cylinder_boolean, box_sphere_boolean, cylinder_cylinder_boolean,
    cylinder_sphere_boolean, sphere_sphere_boolean, BooleanOp, BooleanOutcome, BooleanOutcomeKind,
};
use crate::shape::{Mesh, TopoShape, DEFAULT_DEFLECTION};
use crate::topology::{ExactBRep, Surface};

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

    /// The exact-kernel operation of the same meaning — the bridge
    /// [`BooleanFeature`]'s dispatch uses to reach
    /// [`crate::brep_boolean`].
    fn as_boolean_op(self) -> BooleanOp {
        match self {
            BooleanKind::Cut => BooleanOp::Cut,
            BooleanKind::Fuse => BooleanOp::Fuse,
            BooleanKind::Common => BooleanOp::Common,
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

/// An exact analytic solid with the operand's placement baked into
/// its parameters — the dispatch vocabulary of ADR 0004 rule 3: the
/// operand classes the exact boolean kernel can pair (a sphere-,
/// cylinder-, or box-class `Brep`, a `Sphere`/`Cylinder`/`Box`
/// primitive).
#[derive(Debug, Clone, PartialEq)]
pub enum ExactSolid {
    Sphere {
        center: Vector3,
        radius: f64,
    },
    Cylinder {
        origin: Vector3,
        /// Unit length.
        axis: Vector3,
        radius: f64,
        height: f64,
    },
    /// A hexahedral box: a corner, the three unit edge directions
    /// (mutually perpendicular, either handedness), and the extents
    /// along them — the K4-2d/3 dispatch class (an imported
    /// box-class B-rep, or a `Box` primitive; the placement applied
    /// analytically: the corner transformed as a point, the axes as
    /// directions).
    Box {
        origin: Vector3,
        axes: [Vector3; 3],
        dims: [f64; 3],
    },
}

/// An operand resolved for execution (ADR 0004 rule 2): the computed
/// shape in its local frame plus the operand's own `Placement` — the
/// dispatch applies it analytically per operand class (a sphere's
/// center, a cylinder's origin and axis), the faceted path wholesale
/// as a mesh transform. Never a silent tessellation: the exact shape
/// stays exact until a branch that says what it is doing with it.
#[derive(Debug)]
struct OperandShape {
    shape: TopoShape,
    placement: Matrix4,
}

/// Resolve one boolean operand link (`Base`/`Tool`) to its shape with
/// named refusals: a missing link, an unresolved name, an object with
/// no computed geometry, and a tessellation refusal that names the
/// face (a B-spline) all arrive as their own message — never the
/// generic "no computed geometry" the old mesh-only path collapsed
/// them into.
fn resolve_operand(
    container: &PropertyContainer,
    doc: &Document,
    key: &str,
    type_name: &str,
    name: &str,
) -> Result<OperandShape, String> {
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
    let shape = shape_of(object).ok_or_else(|| {
        // The operand is a part feature without a computed shape — the
        // one ambiguity worth naming separately: the object exists,
        // its geometry does not (not yet recomputed).
        format!(
            "{type_name} '{name}': {key} '{target}' has no computed geometry \
         (not a part feature, or not yet recomputed)"
        )
    })?;
    let placement = match object.properties().get("Placement") {
        Some(Property::Placement(p)) => p.to_matrix(),
        _ => Matrix4::IDENTITY,
    };
    Ok(OperandShape { shape, placement })
}

/// ADR 0004 rule 2 — operand resolution resolves shapes, not meshes.
/// The operand's computed shape with named refusals: an object that is
/// not a part feature, a feature not yet recomputed, or geometry whose
/// channels refuse arrives as its own message. Meshes are returned
/// with the operand's `Placement` applied (the same rule
/// [`placed_mesh_of`] applies); exact shapes and primitives keep
/// their local frames — consumers apply placements analytically per
/// class (the boolean dispatch does: a sphere's center, a cylinder's
/// origin and axis), because a general B-rep transform is not part of
/// this contract.
pub fn placed_shape_of(obj: &dyn DocumentObject) -> Result<TopoShape, String> {
    let shape = shape_of(obj).ok_or_else(|| {
        format!(
            "object '{}': no computed geometry (not a part feature, or not yet recomputed)",
            obj.name()
        )
    })?;
    if let TopoShape::Mesh(mesh) = &shape {
        if obj.type_name() != "Part::Transform" {
            if let Some(Property::Placement(p)) = obj.properties().get("Placement") {
                return Ok(TopoShape::Mesh(mesh.transformed(&p.to_matrix())));
            }
        }
    }
    Ok(shape)
}

/// Classify an `ExactBRep` to the dispatch classes, with the
/// unsupported class named — never a guess. A sphere solid is its one
/// sphere face; a cylinder solid is one cylindrical lateral face plus
/// two planar caps perpendicular to the axis (the height is the cap
/// separation, the origin the lateral face's own base point); a box
/// solid is six planar faces in three antipodal, pairwise
/// perpendicular direction groups — a hexahedron whose every wire
/// vertex is one of the eight derived corners (the K4-2d/3 class).
fn classify_brep_solid(brep: &ExactBRep) -> Result<ExactSolid, String> {
    if brep.faces.len() == 1 {
        if let Surface::Sphere { center, radius, .. } = &brep.faces[0].surface {
            return Ok(ExactSolid::Sphere {
                center: *center,
                radius: *radius,
            });
        }
    }
    if brep.faces.len() == 3 {
        let mut lateral = None;
        let mut caps = Vec::new();
        for face in &brep.faces {
            match &face.surface {
                Surface::Cylinder { .. } if lateral.is_none() => lateral = Some(face),
                Surface::Plane { .. } => caps.push(face),
                _ => {
                    lateral = None;
                    break;
                }
            }
        }
        if let (Some(lateral), true) = (lateral, caps.len() == 2) {
            if let Surface::Cylinder {
                origin,
                axis,
                radius,
                ..
            } = &lateral.surface
            {
                // Both caps perpendicular to the axis: each planar
                // face's own normal is ±axis (the face orientation
                // flag flips the outward normal, not the plane's).
                let caps_perpendicular = caps.iter().all(|cap| match &cap.surface {
                    Surface::Plane { normal, .. } => normal.normalized().is_some_and(|n| {
                        (n.dot(axis) - 1.0).abs() < 1e-9 || (n.dot(axis) + 1.0).abs() < 1e-9
                    }),
                    _ => false,
                });
                let cap_points: Option<(Vector3, Vector3)> =
                    match (&caps[0].surface, &caps[1].surface) {
                        (Surface::Plane { origin: a, .. }, Surface::Plane { origin: b, .. }) => {
                            Some((*a, *b))
                        }
                        _ => None,
                    };
                if caps_perpendicular {
                    if let Some((a, b)) = cap_points {
                        let height = (a - b).dot(axis).abs();
                        if height > 0.0 {
                            return Ok(ExactSolid::Cylinder {
                                origin: *origin,
                                axis: *axis,
                                radius: *radius,
                                height,
                            });
                        }
                    }
                }
            }
        }
    }
    // The hexahedral box class (K4-2d/3): six planar faces in three
    // antipodal direction groups, pairwise perpendicular, every wire
    // vertex one of the eight derived corners.
    if let Some(sol) = classify_box_brep(brep) {
        return Ok(sol);
    }
    // Name the unsupported class honestly.
    let kinds: Vec<&str> = brep.faces.iter().map(|f| f.surface.kind_name()).collect();
    if kinds.iter().all(|k| *k == "Plane") {
        return Err(format!(
            "a planar-faced solid that is not a hexahedral box ({} faces)",
            brep.faces.len()
        ));
    }
    if kinds.contains(&"Bspline") {
        return Err("B-spline-faced geometry".to_string());
    }
    if brep.faces.len() == 1 && kinds[0] == "Torus" {
        return Err("a torus solid".to_string());
    }
    if brep.faces.len() == 1 && kinds[0] == "Sphere" {
        return Err("a sphere-solid variant this classifier does not recognize".to_string());
    }
    Err(format!(
        "a multi-surface solid (face kinds: {})",
        kinds.join(", ")
    ))
}

/// Recognize the hexahedral box in an `ExactBRep`: six `Plane` faces
/// grouped into three antipodal direction groups (each group exactly
/// one `+d̂` and one `−d̂` face), the three group directions pairwise
/// perpendicular, the extents the plane offsets' separation, and —
/// the hexahedron check — every face wire vertex within tolerance of
/// one of the eight derived corners. `None` for anything else (the
/// caller names the class).
fn classify_box_brep(brep: &ExactBRep) -> Option<ExactSolid> {
    if brep.faces.len() != 6 {
        return None;
    }
    let mut planes: Vec<(Vector3, Vector3)> = Vec::with_capacity(6);
    for face in &brep.faces {
        match &face.surface {
            Surface::Plane { origin, normal, .. } => {
                let n = normal.normalized()?;
                planes.push((*origin, n));
            }
            _ => return None,
        }
    }
    // Group by direction (antiparallel joins): each group carries
    // `(d̂, d⁺ offset, d⁻ offset)`.
    let mut groups: Vec<(Vector3, f64, f64)> = Vec::new();
    for (origin, n) in &planes {
        if let Some(g) = groups
            .iter_mut()
            .find(|(d, _, _)| d.dot(n).abs() > 1.0 - 1e-9)
        {
            let off = origin.dot(n) * n.dot(&g.0).signum();
            if off > g.1 {
                g.1 = off;
            }
            if off < g.2 {
                g.2 = off;
            }
        } else {
            let off = origin.dot(n);
            groups.push((*n, off, off));
        }
    }
    if groups.len() != 3 {
        return None;
    }
    let axes = [groups[0].0, groups[1].0, groups[2].0];
    // Pairwise perpendicular.
    for i in 0..3 {
        for j in (i + 1)..3 {
            if axes[i].dot(&axes[j]).abs() > 1e-7 {
                return None;
            }
        }
    }
    let dims = [
        groups[0].1 - groups[0].2,
        groups[1].1 - groups[1].2,
        groups[2].1 - groups[2].2,
    ];
    if !(dims[0] > 0.0 && dims[1] > 0.0 && dims[2] > 0.0) {
        return None;
    }
    // The low corner, in one canonical accumulation order.
    let mut origin = Vector3::new(0.0, 0.0, 0.0);
    for k in 0..3 {
        origin = Vector3::new(
            origin.x + groups[k].2 * axes[k].x,
            origin.y + groups[k].2 * axes[k].y,
            origin.z + groups[k].2 * axes[k].z,
        );
    }
    // The hexahedron check: every wire vertex is one of the eight
    // corners (within recognition tolerance).
    let scale = dims[0]
        .max(dims[1])
        .max(dims[2])
        .max(origin.x.abs().max(origin.y.abs()).max(origin.z.abs()))
        .max(1.0);
    let tol = 1e-9 * scale;
    let corners: Vec<Vector3> = (0..8u8)
        .map(|s| {
            let mut p = origin;
            for k in 0..3 {
                if (s >> k) & 1 == 1 {
                    p = Vector3::new(
                        p.x + dims[k] * axes[k].x,
                        p.y + dims[k] * axes[k].y,
                        p.z + dims[k] * axes[k].z,
                    );
                }
            }
            p
        })
        .collect();
    for face in &brep.faces {
        for wire in &face.wires {
            for we in &wire.edges {
                for vi in [brep.edges[we.edge].start, brep.edges[we.edge].end] {
                    let p = brep.vertices[vi].point;
                    if !corners.iter().any(|c| {
                        (c.x - p.x).abs() <= tol
                            && (c.y - p.y).abs() <= tol
                            && (c.z - p.z).abs() <= tol
                    }) {
                        return None;
                    }
                }
            }
        }
    }
    Some(ExactSolid::Box { origin, axes, dims })
}

/// The operand's exact class with its placement applied analytically
/// — or the named refusal. Primitives carry their parameters; a
/// `Brep` goes through the solid classifier; a mesh or an unpaired
/// primitive is an honest error naming what it is.
fn classify_operand(operand: &OperandShape) -> Result<ExactSolid, String> {
    match &operand.shape {
        TopoShape::Sphere { radius } => Ok(ExactSolid::Sphere {
            center: operand
                .placement
                .transform_point(&Vector3::new(0.0, 0.0, 0.0)),
            radius: *radius,
        }),
        TopoShape::Cylinder { radius, height } => Ok(ExactSolid::Cylinder {
            origin: operand
                .placement
                .transform_point(&Vector3::new(0.0, 0.0, 0.0)),
            axis: operand
                .placement
                .transform_direction(&Vector3::new(0.0, 0.0, 1.0)),
            radius: *radius,
            height: *height,
        }),
        TopoShape::Brep(brep) => {
            let solid = classify_brep_solid(brep)?;
            Ok(match solid {
                ExactSolid::Sphere { center, radius } => ExactSolid::Sphere {
                    center: operand.placement.transform_point(&center),
                    radius,
                },
                ExactSolid::Cylinder {
                    origin,
                    axis,
                    radius,
                    height,
                } => ExactSolid::Cylinder {
                    origin: operand.placement.transform_point(&origin),
                    axis: operand.placement.transform_direction(&axis),
                    radius,
                    height,
                },
                ExactSolid::Box { origin, axes, dims } => ExactSolid::Box {
                    origin: operand.placement.transform_point(&origin),
                    axes: [
                        operand.placement.transform_direction(&axes[0]),
                        operand.placement.transform_direction(&axes[1]),
                        operand.placement.transform_direction(&axes[2]),
                    ],
                    dims,
                },
            })
        }
        TopoShape::Mesh(_) => Err("a faceted operand (a mesh)".to_string()),
        TopoShape::Box {
            length,
            width,
            height,
        } => Ok(ExactSolid::Box {
            origin: operand
                .placement
                .transform_point(&Vector3::new(0.0, 0.0, 0.0)),
            axes: [
                operand
                    .placement
                    .transform_direction(&Vector3::new(1.0, 0.0, 0.0)),
                operand
                    .placement
                    .transform_direction(&Vector3::new(0.0, 1.0, 0.0)),
                operand
                    .placement
                    .transform_direction(&Vector3::new(0.0, 0.0, 1.0)),
            ],
            dims: [*length, *width, *height],
        }),
        TopoShape::Cone { .. } => Err("a Cone operand (no exact pairing)".to_string()),
        TopoShape::Torus { .. } => Err("a Torus operand (no exact pairing)".to_string()),
    }
}

/// The faceted path's placed mesh: the operand tessellated at
/// [`DEFAULT_DEFLECTION`] and transformed by its placement — with the
/// tessellation refusal named (a B-spline face), never collapsed into
/// "no computed geometry".
fn placed_operand_mesh(operand: &OperandShape, what: &str) -> Result<Mesh, String> {
    let mesh = operand
        .shape
        .try_tessellate(DEFAULT_DEFLECTION)
        .map_err(|reason| format!("{what}: the operand's tessellation refused ({reason})"))?;
    Ok(if operand.placement == Matrix4::IDENTITY {
        mesh
    } else {
        mesh.transformed(&operand.placement)
    })
}

/// The ADR 0004 rule 3 outcome as a result channel.
enum ExactOutcome {
    /// The exact solid — the result is `TopoShape::Brep`.
    Solid(ExactBRep),
    /// The empty set — an exact answer; an empty mesh result carries
    /// it (emptiness has no geometry identity to lose).
    Empty,
}

/// Pair the classified operands through the exact kernel — the
/// supported families of [`crate::brep_boolean`], the operand order
/// respected (`Cut` is `Base − Tool`).
fn exact_pair(
    op: BooleanKind,
    base: &ExactSolid,
    tool: &ExactSolid,
) -> Result<ExactOutcome, String> {
    let boolean_op = |out: Result<BooleanOutcome, String>| match out {
        Ok(BooleanOutcome {
            kind: BooleanOutcomeKind::Solid(brep),
        }) => Ok(ExactOutcome::Solid(brep)),
        Ok(BooleanOutcome {
            kind: BooleanOutcomeKind::Empty,
        }) => Ok(ExactOutcome::Empty),
        Ok(BooleanOutcome {
            kind: BooleanOutcomeKind::Refused { why },
        }) => Err(why),
        Err(e) => Err(e),
    };
    match (base, tool) {
        (
            ExactSolid::Sphere {
                center: ca,
                radius: ra,
            },
            ExactSolid::Sphere {
                center: cb,
                radius: rb,
            },
        ) => boolean_op(sphere_sphere_boolean(
            op.as_boolean_op(),
            *ca,
            *ra,
            *cb,
            *rb,
        )),
        (
            ExactSolid::Cylinder {
                origin: o1,
                axis: a1,
                radius: r1,
                height: h1,
            },
            ExactSolid::Cylinder {
                origin: o2,
                axis: a2,
                radius: r2,
                height: h2,
            },
        ) => boolean_op(cylinder_cylinder_boolean(
            op.as_boolean_op(),
            *o1,
            *a1,
            *r1,
            *h1,
            *o2,
            *a2,
            *r2,
            *h2,
        )),
        (
            ExactSolid::Cylinder {
                origin: o,
                axis: a,
                radius: r_c,
                height: h_c,
            },
            ExactSolid::Sphere {
                center: c,
                radius: r_s,
            },
        ) => boolean_op(cylinder_sphere_boolean(
            op.as_boolean_op(),
            *o,
            *a,
            *r_c,
            *h_c,
            *c,
            *r_s,
        )),
        (ExactSolid::Sphere { .. }, ExactSolid::Cylinder { .. }) => match op {
            // Fuse/Common are commutative: the kernel's cylinder-first
            // API answers them with the operands swapped.
            BooleanKind::Fuse | BooleanKind::Common => {
                let (
                    ExactSolid::Cylinder {
                        origin: o,
                        axis: a,
                        radius: r_c,
                        height: h_c,
                    },
                    ExactSolid::Sphere {
                        center: c,
                        radius: r_s,
                    },
                ) = (tool, base)
                else {
                    unreachable!("the match arm pinned the classes")
                };
                boolean_op(cylinder_sphere_boolean(
                    op.as_boolean_op(),
                    *o,
                    *a,
                    *r_c,
                    *h_c,
                    *c,
                    *r_s,
                ))
            }
            // Sphere − cylinder has no kernel path in that order: say
            // so, and say the workaround — never a silent swap that
            // changes the answer.
            BooleanKind::Cut => Err(
                "sphere − cylinder with the sphere as Base has no exact path — swap Base and \
                 Tool for the exact cut"
                    .to_string(),
            ),
        },
        // K4-2d/3: the box × sphere family — the bite taxonomy on the
        // box's boundary skeleton.
        (ExactSolid::Box { origin, axes, dims }, ExactSolid::Sphere { center, radius }) => {
            boolean_op(box_sphere_boolean(
                op.as_boolean_op(),
                *origin,
                axes[0],
                axes[1],
                axes[2],
                dims[0],
                dims[1],
                dims[2],
                *center,
                *radius,
            ))
        }
        (ExactSolid::Sphere { .. }, ExactSolid::Box { .. }) => match op {
            // Fuse/Common are commutative: the box-first kernel API
            // answers them with the operands swapped.
            BooleanKind::Fuse | BooleanKind::Common => {
                let (ExactSolid::Box { origin, axes, dims }, ExactSolid::Sphere { center, radius }) =
                    (tool, base)
                else {
                    unreachable!("the match arm pinned the classes")
                };
                boolean_op(box_sphere_boolean(
                    op.as_boolean_op(),
                    *origin,
                    axes[0],
                    axes[1],
                    axes[2],
                    dims[0],
                    dims[1],
                    dims[2],
                    *center,
                    *radius,
                ))
            }
            // Sphere − box has no kernel path in that order: say so,
            // and say the workaround — never a silent swap.
            BooleanKind::Cut => Err(
                "sphere − box with the sphere as Base has no exact path — swap Base and \
                 Tool for the exact cut"
                    .to_string(),
            ),
        },
        // K4-2d/3 slice 3: the box × box family — the rectilinear
        // composition of two frame-parallel boxes (Cut is
        // `Base − Tool`; the operands are NOT swapped).
        (
            ExactSolid::Box {
                origin: oa,
                axes: axes_a,
                dims: dims_a,
            },
            ExactSolid::Box {
                origin: ob,
                axes: axes_b,
                dims: dims_b,
            },
        ) => boolean_op(box_box_boolean(
            op.as_boolean_op(),
            *oa,
            *axes_a,
            *dims_a,
            *ob,
            *axes_b,
            *dims_b,
        )),
        // K4-2d/3 slice 4: the box × cylinder family — the
        // interior-cross-section stations (Cut is `Base − Tool`; the
        // operands are NOT swapped).
        (
            ExactSolid::Box { origin, axes, dims },
            ExactSolid::Cylinder {
                origin: oc,
                axis: ac,
                radius: rc,
                height: hc,
            },
        ) => boolean_op(box_cylinder_boolean(
            op.as_boolean_op(),
            *origin,
            axes[0],
            axes[1],
            axes[2],
            dims[0],
            dims[1],
            dims[2],
            *oc,
            *ac,
            *rc,
            *hc,
        )),
        (ExactSolid::Cylinder { .. }, ExactSolid::Box { .. }) => match op {
            // Fuse/Common are commutative: the box-first kernel API
            // answers them with the operands swapped.
            BooleanKind::Fuse | BooleanKind::Common => {
                let (
                    ExactSolid::Box { origin, axes, dims },
                    ExactSolid::Cylinder {
                        origin: oc,
                        axis: ac,
                        radius: rc,
                        height: hc,
                    },
                ) = (tool, base)
                else {
                    unreachable!("the match arm pinned the classes")
                };
                boolean_op(box_cylinder_boolean(
                    op.as_boolean_op(),
                    *origin,
                    axes[0],
                    axes[1],
                    axes[2],
                    dims[0],
                    dims[1],
                    dims[2],
                    *oc,
                    *ac,
                    *rc,
                    *hc,
                ))
            }
            // Cylinder − box has no kernel path in that order: say
            // so, and say the workaround — never a silent swap that
            // changes the answer.
            BooleanKind::Cut => Err(
                "cylinder − box with the cylinder as Base has no exact path — swap Base and \
                 Tool for the exact cut"
                    .to_string(),
            ),
        },
        // No `_` arm: the three analytic classes pair exhaustively
        // (nine combinations, every one landing above or refusing
        // through its own arm) — the compiler's exhaustiveness check
        // is the proof, and a fourth class joining `ExactSolid`
        // must grow this table by amendment, not compile past it.
    }
}

/// FreeCAD `Part::Cut` / `Part::Fuse` / `Part::Common`: links to a
/// `Base` and a `Tool` object and applies the ADR 0004 operation
/// contract at `execute` time.
///
/// * A `Brep` operand (imported exact geometry, or a derived exact
///   result) signals exact intent: the operands classify to the
///   supported analytic classes and the exact kernel runs, the result
///   carrying `TopoShape::Brep`; anything the kernel cannot pair
///   refuses with the operands and classes named — never a silent
///   tessellation of the imported geometry.
/// * Without a `Brep` operand, today's faceted path runs — the BSP
///   mesh boolean ([`crate::boolean`]) on the placed tessellations,
///   exact volumes up to tessellation resolution, the result carrying
///   `TopoShape::Mesh` so every consumer can tell faceted from exact.
pub struct BooleanFeature {
    pub container: PropertyContainer,
    pub kind: BooleanKind,
    /// Boolean result, set by `execute_impl` — the ADR 0004 result
    /// channel: `TopoShape::Brep` for an exact execution, a `Mesh` for
    /// the faceted path (or the exact empty set). The loader restores
    /// a readable payload into this field as the exact model it is
    /// (the last-good computation), with `result_from_payload` set.
    pub result: Option<TopoShape>,
    /// True when `result` is the geometry the loader restored from the
    /// file's own payload — the bytes still ARE that shape, so a
    /// re-save keeps the `Valid` promise. False once a recompute
    /// rebuilds the result from parameters: the preserved payload (if
    /// any) is stale beside the rebuild and must not vouch for it.
    pub result_from_payload: bool,
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
            result_from_payload: false,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = self.kind.type_name();
        let name = self.container.name().to_string();
        let base = resolve_operand(&self.container, doc, "Base", type_name, &name)?;
        let tool = resolve_operand(&self.container, doc, "Tool", type_name, &name)?;
        self.result = Some(self.dispatch(&base, &tool, type_name, &name)?);
        self.result_from_payload = false;
        Ok(())
    }

    /// The ADR 0004 rule 3 dispatch (see the type documentation).
    fn dispatch(
        &self,
        base: &OperandShape,
        tool: &OperandShape,
        type_name: &str,
        name: &str,
    ) -> Result<TopoShape, String> {
        let brep_operand =
            matches!(base.shape, TopoShape::Brep(_)) || matches!(tool.shape, TopoShape::Brep(_));
        if !brep_operand {
            // The faceted path — today's behavior, with the operand
            // refusals now named (rule 2: a B-spline tessellation
            // refusal arrives as the face's name).
            let base_mesh = placed_operand_mesh(base, "Base")
                .map_err(|reason| format!("{type_name} '{name}': {reason}"))?;
            let tool_mesh = placed_operand_mesh(tool, "Tool")
                .map_err(|reason| format!("{type_name} '{name}': {reason}"))?;
            return Ok(TopoShape::Mesh(self.kind.apply(&base_mesh, &tool_mesh)));
        }
        // Exact intent: classify both operands, pair them through the
        // kernel, refuse by name wherever the pairing does not exist.
        let classify = |operand: &OperandShape, role: &str, link: &str| {
            classify_operand(operand).map_err(|class| {
                format!(
                    "{type_name} '{name}': {role} '{link}' is {class}; the exact path pairs \
                 sphere×sphere, cylinder×cylinder, cylinder×sphere, and box×sphere — \
                 tessellate first or use mesh operands for the faceted workflow"
                )
            })
        };
        let base_link = self
            .container
            .get("Base")
            .and_then(|p| match p {
                Property::Link(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let tool_link = self
            .container
            .get("Tool")
            .and_then(|p| match p {
                Property::Link(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let base_solid = classify(base, "Base", &base_link)?;
        let tool_solid = classify(tool, "Tool", &tool_link)?;
        match exact_pair(self.kind, &base_solid, &tool_solid) {
            Ok(ExactOutcome::Solid(brep)) => Ok(TopoShape::Brep(brep)),
            Ok(ExactOutcome::Empty) => {
                // An exact empty answer: an empty mesh carries it —
                // emptiness has no geometry identity to lose.
                Ok(TopoShape::Mesh(Mesh::new(Vec::new(), Vec::new())))
            }
            Err(reason) => Err(format!(
                "{type_name} '{name}': the exact boolean refused — {reason}"
            )),
        }
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
    // Same container-backed preservation as the macro-built features:
    // a boolean feature loaded from a real FreeCAD file carries its
    // last-good OCCT shape (the errored-feature case) and unmodeled
    // properties that must round-trip verbatim.
    fn preservation(&self) -> &fc_app::ObjectPreservation {
        &self.container.preserved
    }
    fn preservation_mut(&mut self) -> Option<&mut fc_app::ObjectPreservation> {
        Some(&mut self.container.preserved)
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

/// FreeCAD `Part::MultiFuse` (`Mod/Part/App/FeaturePartFuse.cpp`): a
/// boolean union over a `Shapes` link list, in list order. FreeCAD
/// hands the operands to OCCT's multi-fuse in one call; this kernel
/// folds the pairwise BSP union left-to-right, which reaches the same
/// solid for well-behaved operands (volumes agree to tessellation
/// resolution).
pub struct MultiFuseFeature {
    pub container: PropertyContainer,
    /// Union result mesh, set by `execute_impl`.
    pub result: Option<Mesh>,
}

impl MultiFuseFeature {
    /// Properties: `Shapes` (LinkList, required), `Refine` (Bool, false),
    /// `Placement` identity — FreeCAD's set.
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Shapes", Property::LinkList(Vec::new()));
        container.set("Refine", Property::Bool(false));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "Part::MultiFuse";
        let name = self.container.name().to_string();
        let targets = match self.container.get("Shapes") {
            Some(Property::LinkList(list)) if !list.is_empty() => list.clone(),
            _ => {
                return Err(format!(
                    "{type_name} '{name}': Shapes must link to at least one document object"
                ))
            }
        };
        let mut meshes = Vec::with_capacity(targets.len());
        for target in &targets {
            let object = doc.get_by_name(target).ok_or_else(|| {
                format!("{type_name} '{name}': Shapes entry '{target}' not found")
            })?;
            meshes.push(placed_mesh_of(object, DEFAULT_DEFLECTION).ok_or_else(|| {
                format!(
                    "{type_name} '{name}': Shapes entry '{target}' has no computed geometry \
                     (not a part feature, or not yet recomputed)"
                )
            })?);
        }
        let mut accumulated = meshes.remove(0);
        for next in &meshes {
            accumulated = crate::boolean::fuse(&accumulated, next);
        }
        self.result = Some(accumulated);
        Ok(())
    }
}

impl_document_object!(MultiFuseFeature, "Part::MultiFuse");

/// FreeCAD `Part::Mirroring` (`Mod/Part/App/FeatureMirroring.cpp`):
/// reflects the `Source` shape through the plane given by `Base`
/// (point) and `Normal` (direction) — or, when `MirrorPlane` is linked,
/// through the plane that link names:
///
/// * **a datum plane** (`Part::Plane` and family): its `Placement`
///   gives the plane exactly — position, rotation · +Z. Verified
///   against the real r39109 binary: a `Part::Plane` at (30, 0, 0)
///   rotated 30° about X resolves to `Base` = (30, 0, 0),
///   `Normal` = (0, −0.5, 0.8660254…).
/// * **a face subelement** (e.g. `("Box", ["Face3"])`): a mesh
///   kernel has no OCCT face numbering to re-identify "Face3" — and
///   FreeCAD itself persists the resolved plane into `Base`/`Normal`
///   on every execute (observed on r39109: Face3 of an 8×6×4 box at
///   the origin resolved to `Base` = (8, 0, 0), `Normal` = (0, 1, 0)),
///   so the file's own resolution is the plane used.
///
/// Either way FreeCAD's execute writes the resolved plane back into
/// `Base`/`Normal` (observed on r39109); this kernel mirrors that
/// write-back for the datum case.
///
/// Mirroring flips orientation, so the reflected mesh's triangles are
/// re-wound to keep outward normals (FreeCAD/OCCT handles this inside
/// `BRepBuilderAPI_Transform` with the mirror flag).
pub struct MirroringFeature {
    pub container: PropertyContainer,
    /// Mirrored result mesh, set by `execute_impl`.
    pub result: Option<Mesh>,
}

impl MirroringFeature {
    /// Properties: `Source` (Link, required), `Base` (Vector, origin),
    /// `Normal` (Vector, +Z), `MirrorPlane` (LinkSub, unset),
    /// `Placement` identity — FreeCAD's set.
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Source", Property::Link(String::new()));
        container.set("Base", Property::Vector(Vector3::ZERO));
        container.set("Normal", Property::Vector(Vector3::new(0.0, 0.0, 1.0)));
        container.set("MirrorPlane", Property::LinkSub(String::new(), Vec::new()));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "Part::Mirroring";
        let name = self.container.name().to_string();
        let source = match self.container.get("Source") {
            Some(Property::Link(target)) if !target.is_empty() => target.clone(),
            _ => {
                return Err(format!(
                    "{type_name} '{name}': Source must link to a document object"
                ))
            }
        };
        let mut base = match self.container.get("Base") {
            Some(Property::Vector(v)) => *v,
            _ => return Err(format!("{type_name} '{name}': missing property 'Base'")),
        };
        let mut normal = match self.container.get("Normal") {
            Some(Property::Vector(v)) => *v,
            _ => return Err(format!("{type_name} '{name}': missing property 'Normal'")),
        };
        let length = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
        if !length.is_finite() || length <= 1e-12 {
            return Err(format!(
                "{type_name} '{name}': Normal must be a non-zero direction"
            ));
        }
        normal = Vector3::new(normal.x / length, normal.y / length, normal.z / length);

        // FreeCAD's `MirrorPlane` (a `PropertyLinkSub`) overrides
        // Base + Normal — FeatureMirroring resolves it into the plane
        // on every execute. The datum case is computed exactly (any
        // `Part::Datum` family member: FreeCAD reads its placement,
        // position + rotation · +Z — a `Part::Plane` loaded through this
        // kernel's unknown-family path carries its real type in the
        // `TypeId` property); the face-subelement case uses the plane
        // persisted in Base/Normal (FreeCAD's own last resolution — see
        // the struct docs). A linked target that is neither (a bare
        // shape without subelements, say) is the same error FreeCAD
        // raises ("No subelement specified").
        if let Some(Property::LinkSub(target, subs)) = self.container.get("MirrorPlane") {
            if !target.is_empty() && subs.is_empty() {
                let object = doc.get_by_name(target).ok_or_else(|| {
                    format!("{type_name} '{name}': MirrorPlane '{target}' not found")
                })?;
                let placement = datum_placement_of(object).ok_or_else(|| {
                    format!(
                        "{type_name} '{name}': MirrorPlane '{target}' is not a \
                         datum plane (no placement to read)"
                    )
                })?;
                if !is_datum_family(object) {
                    return Err(format!(
                        "{type_name} '{name}': MirrorPlane '{target}' is a {} — \
                         subelement links (faces) are not computable at mesh level, \
                         and FreeCAD accepts a bare link only for datum planes",
                        object.type_name()
                    ));
                }
                // Plane: the datum's position, its rotation · +Z
                // (FreeCAD: `Base::Rotation tmp; tmp.multVec(
                // (0,0,1), normal)`).
                let rotated = placement
                    .rotation
                    .to_matrix()
                    .transform_direction(&Vector3::new(0.0, 0.0, 1.0));
                let rotated_length =
                    (rotated.x * rotated.x + rotated.y * rotated.y + rotated.z * rotated.z).sqrt();
                if !rotated_length.is_finite() || rotated_length <= 1e-12 {
                    return Err(format!(
                        "{type_name} '{name}': MirrorPlane '{target}' placement is \
                         degenerate"
                    ));
                }
                normal = Vector3::new(
                    rotated.x / rotated_length,
                    rotated.y / rotated_length,
                    rotated.z / rotated_length,
                );
                // FreeCAD's write-back: the resolved plane lands in
                // Base/Normal (observed on r39109).
                base = placement.position;
                self.container
                    .set("Base", Property::Vector(placement.position));
                self.container.set("Normal", Property::Vector(normal));
            }
            // The face-subelement case keeps Base/Normal — the file's
            // persisted resolution (see the struct docs).
        }

        let object = doc
            .get_by_name(&source)
            .ok_or_else(|| format!("{type_name} '{name}': Source '{source}' not found"))?;
        let mesh = placed_mesh_of(object, DEFAULT_DEFLECTION).ok_or_else(|| {
            format!(
                "{type_name} '{name}': Source '{source}' has no computed geometry \
                 (not a part feature, or not yet recomputed)"
            )
        })?;

        // Reflect v through the plane (base, n): v' = v - 2 (n . (v - b)) n
        let mut reflected = Vec::with_capacity(mesh.vertices.len());
        for v in &mesh.vertices {
            let d =
                normal.x * (v.x - base.x) + normal.y * (v.y - base.y) + normal.z * (v.z - base.z);
            reflected.push(Vector3::new(
                v.x - 2.0 * d * normal.x,
                v.y - 2.0 * d * normal.y,
                v.z - 2.0 * d * normal.z,
            ));
        }
        // A reflection reverses handedness: swap two indices per triangle
        // so the windings stay outward.
        let triangles = mesh.triangles.iter().map(|t| [t[0], t[2], t[1]]).collect();
        self.result = Some(Mesh::new(reflected, triangles));
        Ok(())
    }
}

impl_document_object!(MirroringFeature, "Part::Mirroring");

/// The placement of a datum-plane link target: a modeled `Placement`
/// property when the object has one, else the `PropertyPlacement`
/// parsed out of a `ForeignObject`'s preserved raw `<ObjectData>`
/// (FreeCAD's datum planes — `Part::Plane`, `PartDesign::Plane`, ... —
/// store their plane in exactly that element).
fn datum_placement_of(object: &dyn DocumentObject) -> Option<Placement> {
    if let Some(Property::Placement(p)) = object.properties().get("Placement") {
        return Some(*p);
    }
    let foreign = object.as_any().downcast_ref::<fc_app::ForeignObject>()?;
    let raw = foreign.raw_object_data.as_ref()?;
    placement_from_raw_object_data(raw)
}

/// Is `object` a member of FreeCAD's `Part::Datum` family — the targets
/// `FeatureMirroring` accepts for a bare (subelement-less) `MirrorPlane`
/// link? The real type id rides in the loader's `TypeId` property for
/// unknown-family loads (a `Part::Plane` loads as the `Part::Feature`
/// parameter container carrying `TypeId = "Part::Plane"`), in
/// `ForeignObject::type_id` for opaque loads, and is the type name
/// itself for everything else.
fn is_datum_family(object: &dyn DocumentObject) -> bool {
    let type_id = match object.properties().get("TypeId") {
        Some(Property::String(s)) => s.clone(),
        _ => match object.as_any().downcast_ref::<fc_app::ForeignObject>() {
            Some(foreign) => foreign.type_id.clone(),
            None => object.type_name().to_string(),
        },
    };
    matches!(
        type_id.as_str(),
        "Part::Plane"
            | "Part::Line"
            | "Part::Point"
            | "PartDesign::Plane"
            | "PartDesign::Line"
            | "PartDesign::Point"
    )
}

/// Parse the `<Property name="Placement" type="App::PropertyPlacement">`
/// element out of raw object data: the position from `Px/Py/Pz`, the
/// rotation from the `Q0/Q1/Q2/Q3` quaternion (the same attributes
/// this crate's own writer emits — and FreeCAD's, verified against
/// r39109's `Part::Plane` serialization).
fn placement_from_raw_object_data(raw: &str) -> Option<Placement> {
    let start = raw.find("<PropertyPlacement ")?;
    let end = raw[start..].find("/>")? + start;
    let element = &raw[start..end];
    let attr = |name: &str| -> Option<f64> {
        let needle = format!("{name}=\"");
        let i = element.find(&needle)? + needle.len();
        let rest = &element[i..];
        let j = rest.find('"')?;
        rest[..j].trim().parse::<f64>().ok()
    };
    Some(Placement {
        position: Vector3::new(attr("Px")?, attr("Py")?, attr("Pz")?),
        rotation: fc_base::Rotation {
            x: attr("Q0")?,
            y: attr("Q1")?,
            z: attr("Q2")?,
            w: attr("Q3")?,
        },
    })
}

/// The operand geometry of `object` with its own `Placement` applied —
/// the rule FreeCAD applies when a consumer asks for a shape: local
/// shape in global space. Objects whose `execute` already bakes the
/// transform into the computed mesh (`Part::Transform`) skip the second
/// application; the boolean/mirror family bakes its *operands'*
/// placements but keeps its own `Placement` for consumers, exactly as
/// FreeCAD positions a `Part::Cut` result.
pub fn placed_mesh_of(obj: &dyn DocumentObject, deflection: f64) -> Option<Mesh> {
    let mut mesh = mesh_of(obj, deflection)?;
    if obj.type_name() != "Part::Transform" {
        if let Some(Property::Placement(p)) = obj.properties().get("Placement") {
            mesh.transform(&p.to_matrix());
        }
    }
    Some(mesh)
}

/// `Part::Feature` — FreeCAD's generic shape carrier: imported geometry
/// (our STEP import target) and the family root. Holds a plain
/// [`TopoShape`] as *data*: `execute` is a validation no-op — the shape
/// is not derived from parameters, so there is nothing to recompute.
#[derive(Debug)]
pub struct MeshFeature {
    pub container: fc_app::PropertyContainer,
    /// The carried shape (`TopoShape::Mesh` for STEP imports).
    pub shape: Option<TopoShape>,
}

impl MeshFeature {
    /// Properties: `Label` (String) and `Placement` (identity), the
    /// intersection of FreeCAD's `Part::Feature` surface.
    pub fn new(name: &str) -> Self {
        let mut container = fc_app::PropertyContainer::new(name);
        container.set("Label", fc_app::Property::String(name.to_string()));
        container.set(
            "Placement",
            fc_app::Property::Placement(fc_base::Placement::IDENTITY),
        );
        Self {
            container,
            shape: None,
        }
    }
}

impl DocumentObject for MeshFeature {
    fn type_name(&self) -> &'static str {
        "Part::Feature"
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
    // The preservation bag rides in the container: an imported
    // `Part::Feature`'s OCCT shape payload and its unmodeled properties
    // are the object's only data — they must round-trip verbatim.
    fn preservation(&self) -> &fc_app::ObjectPreservation {
        &self.container.preserved
    }
    fn preservation_mut(&mut self) -> Option<&mut fc_app::ObjectPreservation> {
        Some(&mut self.container.preserved)
    }
    fn execute(&mut self, _doc: &Document) -> Result<(), String> {
        // Imported geometry is data, not a derivation: validate only.
        if let Some(shape) = &self.shape {
            shape.validate()?;
        }
        Ok(())
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
/// downcast. `None` for non-part objects, features that have not
/// computed a shape yet — or a shape whose tessellation the exact tier
/// refuses (a B-spline face): no mesh is more honest than an empty
/// one pretending to be the solid. The refusal reason is available on
/// the shape itself (`TopoShape::try_tessellate`).
pub fn mesh_of(obj: &dyn DocumentObject, deflection: f64) -> Option<Mesh> {
    let any = obj.as_any();
    if let Some(f) = any.downcast_ref::<MeshFeature>() {
        return f
            .shape
            .as_ref()
            .and_then(|s| s.try_tessellate(deflection).ok());
    }
    if let Some(f) = any.downcast_ref::<MultiFuseFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<MirroringFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<BoxFeature>() {
        return f
            .shape
            .as_ref()
            .and_then(|s| s.try_tessellate(deflection).ok());
    }
    if let Some(f) = any.downcast_ref::<SphereFeature>() {
        return f
            .shape
            .as_ref()
            .and_then(|s| s.try_tessellate(deflection).ok());
    }
    if let Some(f) = any.downcast_ref::<CylinderFeature>() {
        return f
            .shape
            .as_ref()
            .and_then(|s| s.try_tessellate(deflection).ok());
    }
    if let Some(f) = any.downcast_ref::<ConeFeature>() {
        return f
            .shape
            .as_ref()
            .and_then(|s| s.try_tessellate(deflection).ok());
    }
    if let Some(f) = any.downcast_ref::<TorusFeature>() {
        return f
            .shape
            .as_ref()
            .and_then(|s| s.try_tessellate(deflection).ok());
    }
    if let Some(f) = any.downcast_ref::<TransformFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<BooleanFeature>() {
        // ADR 0004 rule 1: the result channel is a `TopoShape` — the
        // display mesh derives from it at the caller's deflection. A
        // tessellation refusal (a B-spline face) is `None` here and
        // the named reason on `try_tessellate`: no mesh is more
        // honest than an empty one pretending to be the solid.
        return f
            .result
            .as_ref()
            .and_then(|s| s.try_tessellate(deflection).ok());
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
    if let Some(f) = any.downcast_ref::<crate::partdesign::LinearPatternFeature>() {
        return f.result.clone();
    }
    if let Some(f) = any.downcast_ref::<crate::partdesign::PolarPatternFeature>() {
        return f.result.clone();
    }
    None
}

/// Analytic shape of any part feature, if computed.
pub fn shape_of(obj: &dyn DocumentObject) -> Option<TopoShape> {
    let any = obj.as_any();
    if let Some(f) = any.downcast_ref::<MeshFeature>() {
        return f.shape.clone();
    }
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
        // ADR 0004 rule 1: the result channel carries the
        // representation — exact or faceted — as itself.
        return f.result.clone();
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
