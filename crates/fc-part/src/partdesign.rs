//! PartDesign features: sketch-consuming solids, mirroring FreeCAD
//! `Mod/PartDesign` (`FeaturePad` / `FeaturePocket` /
//! `FeatureRevolution`).
//!
//! These are the payoff of the FC-6 wave: a solved
//! [`Sketcher::SketchObject`](fc_sketcher::SketchObject) in the document
//! is linked as the `Profile` of a feature, and the feature's `execute`
//! turns the sketch's **solved geometry** into a solid — extruded (Pad),
//! subtracted after extruding (Pocket), or swept around an axis
//! (Revolution). The dependency graph guarantees the sketch recomputes
//! first; a sketch that fails its solve marks the feature's recompute
//! as failed (FreeCAD behavior).
//!
//! Property names/types mirror FreeCAD (`Profile`, `Length`, `Midplane`,
//! `Reversed`, `Angle`, `Axis`, `Base`, `Inverted`); dimensions are
//! `App::PropertyFloat` like the sibling `Part::` features (so expression
//! bindings work on them, dimensionless — the property vocabulary has no
//! quantity-carrying float yet), links are `App::PropertyLink`.
//!
//! # Execute semantics (shared)
//!
//! `execute` resolves the `Profile` link, downcasts to `SketchObject` and
//! refuses a sketch in `Error` state (our recompute loop skips errored
//! objects, so a later re-run of the feature is the only place the broken
//! profile can be caught — FreeCAD aborts the dependent's recompute; we
//! fail it). The sketch's geometry is run through
//! [`extract_wires`](crate::profile::extract_wires) +
//! [`classify_wires`](crate::profile::classify_wires) (outer boundaries
//! CCW, holes CW — FreeCAD `Part::Face` semantics) and handed to the
//! profile kernel. Any failure (open wire, unsolved sketch, bad scalar)
//! returns `Err`, the document marks the object `Error`, and the feature's
//! previous result is dropped first — a failed feature has **no** output
//! geometry, so dependents re-running against it fail with the honest
//! "no computed geometry" error (the recompute aborts on the first
//! failure; dependents stay `Touched` until their own re-run).
//!
//! # Coordinate frames (honest v1 simplifications)
//!
//! * Sketches live in the XY plane at `z = 0` with no attachment system,
//!   so a Pad extrudes along ±Z of the global frame (FreeCAD pads along
//!   the sketch normal, which is the same thing for an unattached XY
//!   sketch).
//! * A Pocket extrudes its tool along +Z by default — into a base that
//!   sits on `z >= 0`, which is where this kernel's primitives live (the
//!   `Part::Box` spans `z ∈ [0, Height]`). FreeCAD's Pocket cuts along
//!   the negative sketch normal into attached material; without
//!   attachment that direction would cut into empty space for every
//!   canonical document, so the default is the direction that intersects
//!   the base here, and `Inverted` selects −Z.
//! * `Axis` on a Revolution: `z` (default) uses the kernel's classic
//!   lathe frame — sketch `(x, y)` reinterpreted as `(radius, height)`
//!   about the 3D Z axis (a sketch cannot be revolved about its own
//!   normal; the profile plane is the XZ half-plane). `y` revolves the
//!   drawn sketch about its vertical axis: the kernel's canonical frame
//!   for a +Y axis starts the sweep at the YZ half-plane, so the sketch's
//!   own plane sits 90° into the sweep — invisible for a full turn, and
//!   a placement question otherwise. See [`RevolutionFeature`].
//!
//! The kernels ([`crate::profile`]) are tessellated and honest about it:
//! exact volumes only up to tessellation resolution, watertight by
//! construction (shared vertex indices, seam-welded revolutions).
//!
//! Clean-room note: the behavior distilled here is described by the work
//! order and FreeCAD's user-facing documentation; no FreeCAD or OCCT code
//! was read or used.

use fc_app::{impl_document_object, Document, DocumentObject, Property, PropertyContainer};
use fc_base::{Matrix4, Placement, Rotation, Vector3};

use crate::profile::{
    classify_wires, extract_wires, extrude as extrude_kernel, revolve as revolve_kernel, Wire,
    DEFAULT_PROFILE_DEFLECTION, DEFAULT_TOLERANCE,
};
use crate::shape::Mesh;

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

/// Read a required boolean flag (default false when absent is *not*
/// assumed — the property map is the truth).
fn require_bool(container: &PropertyContainer, type_name: &str, key: &str) -> Result<bool, String> {
    match container.get(key) {
        Some(Property::Bool(value)) => Ok(*value),
        _ => Err(format!(
            "{type_name} '{}': missing property '{key}'",
            container.name()
        )),
    }
}

/// Resolve a feature's `Profile` link to its `Sketcher::SketchObject`.
/// Every error names the feature and the reason (honest recompute
/// failures; the document marks the object `Error`). FreeCAD types the
/// profile `App::PropertyLinkSub`; both the plain `Link` form (this
/// kernel's legacy authoring) and the `LinkSub` form resolve.
fn profile_sketch<'a>(
    container: &PropertyContainer,
    type_name: &str,
    doc: &'a Document,
) -> Result<&'a fc_sketcher::SketchObject, String> {
    let name = container.name();
    let target = match container.get("Profile") {
        Some(Property::Link(target)) if !target.is_empty() => target.clone(),
        Some(Property::LinkSub(target, _)) if !target.is_empty() => target.clone(),
        _ => {
            return Err(format!(
                "{type_name} '{name}': Profile must link to a sketch object"
            ))
        }
    };
    let object = doc
        .get_by_name(&target)
        .ok_or_else(|| format!("{type_name} '{name}': Profile '{target}' not found"))?;
    let sketch = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .ok_or_else(|| {
            format!(
                "{type_name} '{name}': Profile '{target}' is a {}, not a \
                 Sketcher::SketchObject",
                object.type_name()
            )
        })?;
    // Defense in depth: the recompute loop aborts on the sketch's own
    // failure before this feature runs, but a *later* recompute skips
    // the errored sketch and would otherwise silently consume its
    // last-known geometry. FreeCAD never lets a dependent run off a
    // failed profile; neither do we.
    if let fc_app::ObjectStatus::Error(reason) = sketch.status() {
        return Err(format!(
            "{type_name} '{name}': Profile '{target}' is in error state: {reason}"
        ));
    }
    Ok(sketch)
}

/// Extract + classify the profile's wires (outer boundaries CCW, holes
/// CW — the FreeCAD `Part::Face` step of the pipeline).
fn profile_wires(
    container: &PropertyContainer,
    type_name: &str,
    doc: &Document,
) -> Result<Vec<Wire>, String> {
    let sketch = profile_sketch(container, type_name, doc)?;
    let name = container.name();
    extract_wires(
        sketch.sketch.geometry(),
        DEFAULT_TOLERANCE,
        DEFAULT_PROFILE_DEFLECTION,
    )
    .and_then(classify_wires)
    .map_err(|err| format!("{type_name} '{name}': profile is not usable — {err}"))
}

/// Validate + store a freshly computed feature mesh. Mirrors
/// [`crate::features`]' `store_primitive`: structural validation
/// (finite vertices, in-range indices) — watertightness is a property
/// the profile kernels guarantee by construction.
fn store_result(
    container: &PropertyContainer,
    type_name: &str,
    mesh: Mesh,
    slot: &mut Option<Mesh>,
) -> Result<(), String> {
    mesh.validate()
        .map_err(|reason| format!("{type_name} '{}': {reason}", container.name()))?;
    *slot = Some(mesh);
    Ok(())
}

/// FreeCAD `PartDesign::Pad` (distilled): extrude the sketch's solved
/// profile along +Z (or −Z when `Reversed`), optionally symmetric about
/// the sketch plane (`Midplane`).
pub struct PadFeature {
    pub container: PropertyContainer,
    /// Extruded solid, set by `execute_impl`; `None` until the first
    /// successful recompute and cleared again by a failing one.
    pub result: Option<Mesh>,
}

impl PadFeature {
    /// Properties: `Profile` (Link, required), `Length` (10.0, FreeCAD's
    /// default), `Midplane` (false), `Reversed` (false), `Placement`.
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Profile", Property::LinkSub(String::new(), Vec::new()));
        container.set("Length", Property::Float(10.0));
        container.set("Midplane", Property::Bool(false));
        container.set("Reversed", Property::Bool(false));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "PartDesign::Pad";
        // A failed execute leaves no stale geometry behind: dependents
        // re-running against this feature must fail honestly.
        self.result = None;
        let mesh = pad_tool(&self.container, type_name, doc)?;
        store_result(&self.container, type_name, mesh, &mut self.result)
    }
}

impl_document_object!(PadFeature, "PartDesign::Pad");

/// A `PartDesign::Pad`'s **tool**: the extrusion of its solved profile
/// — the solid the pad fuses onto nothing (the pad's own result).
/// Factored out of `PadFeature::execute_impl` because feature patterns
/// re-derive the tools of their originals on every recompute (the
/// parametric path — a pattern never copies a cached result).
fn pad_tool(
    container: &PropertyContainer,
    type_name: &str,
    doc: &Document,
) -> Result<Mesh, String> {
    let name = container.name();
    let length = require_float(container, type_name, "Length")?;
    if !(length.is_finite() && length > 0.0) {
        return Err(format!(
            "{type_name} '{name}': Length must be finite and > 0 (got {length})"
        ));
    }
    let midplane = require_bool(container, type_name, "Midplane")?;
    let reversed = require_bool(container, type_name, "Reversed")?;
    let wires = profile_wires(container, type_name, doc)?;
    let direction = if reversed {
        Vector3::new(0.0, 0.0, -1.0)
    } else {
        Vector3::Z
    };
    extrude_kernel(
        &wires,
        direction,
        length,
        midplane,
        DEFAULT_PROFILE_DEFLECTION,
    )
    .map_err(|err| format!("{type_name} '{name}': {err}"))
}

/// FreeCAD `PartDesign::Pocket` (distilled): extrude the sketch's solved
/// profile into a tool solid and subtract it from the `Base` feature's
/// mesh — the classic "cut a hole through the body" feature. The tool
/// extrudes along +Z by default (into a base on `z >= 0`, see the module
/// docs) or −Z when `Inverted`; `Midplane` spans the tool symmetrically
/// about the sketch plane.
pub struct PocketFeature {
    pub container: PropertyContainer,
    /// `Base − extruded profile`, set by `execute_impl`.
    pub result: Option<Mesh>,
}

impl PocketFeature {
    /// Properties: `Profile` (Link, required), `Base` (Link, required),
    /// `Length` (5.0, FreeCAD's default), `Midplane` (false), `Inverted`
    /// (false), `Placement`.
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Profile", Property::LinkSub(String::new(), Vec::new()));
        container.set("Base", Property::Link(String::new()));
        container.set("Length", Property::Float(5.0));
        container.set("Midplane", Property::Bool(false));
        container.set("Inverted", Property::Bool(false));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "PartDesign::Pocket";
        let name = self.container.name().to_string();
        self.result = None;

        let base_target = match self.container.get("Base") {
            Some(Property::Link(target)) if !target.is_empty() => target.clone(),
            _ => {
                return Err(format!(
                    "{type_name} '{name}': Base must link to a solid feature"
                ))
            }
        };
        let base_object = doc
            .get_by_name(&base_target)
            .ok_or_else(|| format!("{type_name} '{name}': Base '{base_target}' not found"))?;
        let base = crate::features::placed_mesh_of(base_object, crate::shape::DEFAULT_DEFLECTION)
            .ok_or_else(|| {
            format!(
                "{type_name} '{name}': Base '{base_target}' has no computed geometry \
                     (not a part feature, or not yet recomputed)"
            )
        })?;

        let tool = pocket_tool(&self.container, type_name, doc)?;
        let mesh = crate::boolean::cut(&base, &tool);
        store_result(&self.container, type_name, mesh, &mut self.result)
    }
}

impl_document_object!(PocketFeature, "PartDesign::Pocket");

/// A `PartDesign::Pocket`'s **tool**: the extrusion it cuts from `Base`
/// (the pocket's own result is `Base − tool`; patterns want the tool).
fn pocket_tool(
    container: &PropertyContainer,
    type_name: &str,
    doc: &Document,
) -> Result<Mesh, String> {
    let name = container.name();
    let length = require_float(container, type_name, "Length")?;
    if !(length.is_finite() && length > 0.0) {
        return Err(format!(
            "{type_name} '{name}': Length must be finite and > 0 (got {length})"
        ));
    }
    let midplane = require_bool(container, type_name, "Midplane")?;
    let inverted = require_bool(container, type_name, "Inverted")?;
    let wires = profile_wires(container, type_name, doc)?;
    let direction = if inverted {
        Vector3::new(0.0, 0.0, -1.0)
    } else {
        Vector3::Z
    };
    extrude_kernel(
        &wires,
        direction,
        length,
        midplane,
        DEFAULT_PROFILE_DEFLECTION,
    )
    .map_err(|err| format!("{type_name} '{name}': {err}"))
}

/// FreeCAD `PartDesign::Revolution` (distilled): sweep the sketch's
/// solved profile around an axis through the origin.
///
/// * `Angle` is degrees; the sign is the sweep direction, flipped by
///   `Reversed`. `|Angle| >= 360` is a seam-welded full turn (the
///   kernel's rule — it never sweeps more than one turn).
/// * `Axis` is a direction vector through the origin: `(0, 0, 1)` (the
///   default, CLI `--axis z`) is the kernel's classic lathe frame —
///   sketch `x` is the radius and sketch `y` the height about the 3D Z
///   axis, because a sketch in the XY plane cannot be revolved about
///   its own normal. `(0, 1, 0)` (CLI `--axis y`) revolves the drawn
///   sketch about its vertical axis: the kernel's canonical frame for
///   a +Y axis starts the sweep at the YZ half-plane, so the sketch's
///   own plane sits 90° into the sweep — for a full turn the solid is
///   exactly the revolution of the drawn profile, and for a partial
///   turn the difference is a rotation about the axis.
/// * The profile must stay on the +x side of the sketch (the kernel's
///   `u` is the radial coordinate); points on the axis itself are
///   snapped, a profile crossing it sweeps a self-intersecting solid —
///   the caller's problem, exactly as in FreeCAD.
pub struct RevolutionFeature {
    pub container: PropertyContainer,
    /// Swept solid, set by `execute_impl`.
    pub result: Option<Mesh>,
}

impl RevolutionFeature {
    /// Properties: `Profile` (Link, required), `Angle` (360.0),
    /// `Axis` ((0, 0, 1)), `Reversed` (false), `Placement`.
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Profile", Property::LinkSub(String::new(), Vec::new()));
        container.set("Angle", Property::Float(360.0));
        container.set("Axis", Property::Vector(Vector3::Z));
        container.set("Reversed", Property::Bool(false));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "PartDesign::Revolution";
        self.result = None;
        let mesh = revolution_tool(&self.container, type_name, doc)?;
        store_result(&self.container, type_name, mesh, &mut self.result)
    }
}

impl_document_object!(RevolutionFeature, "PartDesign::Revolution");

/// A `PartDesign::Revolution`'s **tool**: the sweep of its solved
/// profile (the revolution's own result).
fn revolution_tool(
    container: &PropertyContainer,
    type_name: &str,
    doc: &Document,
) -> Result<Mesh, String> {
    let name = container.name();
    let angle = require_float(container, type_name, "Angle")?;
    if !(angle.is_finite() && angle != 0.0) {
        return Err(format!(
            "{type_name} '{name}': Angle must be finite and non-zero (got {angle})"
        ));
    }
    let axis = match container.get("Axis") {
        Some(Property::Vector(v)) => *v,
        _ => return Err(format!("{type_name} '{name}': missing property 'Axis'")),
    };
    if !axis.x.is_finite()
        || !axis.y.is_finite()
        || !axis.z.is_finite()
        || (axis.x == 0.0 && axis.y == 0.0 && axis.z == 0.0)
    {
        return Err(format!(
            "{type_name} '{name}': Axis must be a finite, non-zero vector (got \
             ({}, {}, {}))",
            axis.x, axis.y, axis.z
        ));
    }
    let reversed = require_bool(container, type_name, "Reversed")?;
    let wires = profile_wires(container, type_name, doc)?;
    let angle = if reversed { -angle } else { angle };
    revolve_kernel(
        &wires,
        Vector3::ZERO,
        axis,
        angle,
        DEFAULT_PROFILE_DEFLECTION,
    )
    .map_err(|err| format!("{type_name} '{name}': {err}"))
}

// ---------------------------------------------------------------------------
// Feature patterns (the mechanical-workflow milestone)
// ---------------------------------------------------------------------------

/// What a pattern's original does to the base when copied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Polarity {
    /// The original adds material (`Pad`, `Revolution`, primitives).
    Additive,
    /// The original removes material (`Pocket`).
    Subtractive,
}

/// The maximum pattern nesting depth (a pattern whose originals are a
/// pattern — FreeCAD's multiplicative `Transformed` composition, the
/// 2×2 grid from two chained linear patterns). The dependency graph
/// makes cycles impossible (a pattern can only reference features that
/// already exist, and links are DAG edges); this guards hand-edited
/// properties anyway.
const MAX_PATTERN_NESTING: usize = 8;

/// Read a pattern's required `Occurrences` (`App::PropertyInteger`,
/// FreeCAD's `PropertyInteger`).
fn require_occurrences(container: &PropertyContainer, type_name: &str) -> Result<i64, String> {
    match container.get("Occurrences") {
        Some(Property::Integer(n)) => Ok(*n),
        _ => Err(format!(
            "{type_name} '{}': missing property 'Occurrences'",
            container.name()
        )),
    }
}

/// Validate a span/step scalar (finite, strictly positive).
fn require_positive(
    container: &PropertyContainer,
    type_name: &str,
    key: &str,
) -> Result<f64, String> {
    let value = require_float(container, type_name, key)?;
    if !(value.is_finite() && value > 0.0) {
        return Err(format!(
            "{type_name} '{}': {key} must be finite and > 0 (got {value})",
            container.name()
        ));
    }
    Ok(value)
}

/// A `PartDesign::LinearPattern`'s copy transforms, one per occurrence
/// (the first is the identity — occurrence 1 is the untransformed
/// original, FreeCAD's rule). Spacing is `Length / (Occurrences − 1)`,
/// the total span between first and last copy.
fn linear_copies(container: &PropertyContainer, type_name: &str) -> Result<Vec<Matrix4>, String> {
    let name = container.name();
    let length = require_positive(container, type_name, "Length")?;
    let occurrences = require_occurrences(container, type_name)?;
    if occurrences < 2 {
        return Err(format!(
            "{type_name} '{name}': Occurrences must be >= 2 (got {occurrences})"
        ));
    }
    let direction = match container.get("Direction") {
        Some(Property::Vector(v)) => *v,
        _ => {
            return Err(format!(
                "{type_name} '{name}': missing property 'Direction'"
            ))
        }
    };
    let unit = direction.normalized().ok_or_else(|| {
        format!(
            "{type_name} '{name}': Direction must be a finite, non-zero vector (got \
             ({}, {}, {}))",
            direction.x, direction.y, direction.z
        )
    })?;
    let reversed = require_bool(container, type_name, "Reversed")?;
    let spacing = length / (occurrences - 1) as f64;
    let signed = if reversed { -spacing } else { spacing };
    Ok((0..occurrences)
        .map(|k| Matrix4::from_translation(&(unit * (signed * k as f64))))
        .collect())
}

/// A `PartDesign::PolarPattern`'s copy transforms (rotations about the
/// axis through the origin; occurrence 1 is the untransformed
/// original). FreeCAD's 360° rule: a full turn spaces occurrences
/// `360/N` apart so the last copy does not stack on the first; a
/// partial arc spaces them `Angle/(N−1)` across the span.
fn polar_copies(container: &PropertyContainer, type_name: &str) -> Result<Vec<Matrix4>, String> {
    let name = container.name();
    let angle = require_float(container, type_name, "Angle")?;
    if !angle.is_finite() || angle == 0.0 || angle.abs() > 360.0 {
        return Err(format!(
            "{type_name} '{name}': Angle must be finite, non-zero and within \
             ±360° (got {angle})"
        ));
    }
    let occurrences = require_occurrences(container, type_name)?;
    if occurrences < 2 {
        return Err(format!(
            "{type_name} '{name}': Occurrences must be >= 2 (got {occurrences})"
        ));
    }
    let axis = match container.get("Axis") {
        Some(Property::Vector(v)) => *v,
        _ => return Err(format!("{type_name} '{name}': missing property 'Axis'")),
    };
    if !axis.x.is_finite()
        || !axis.y.is_finite()
        || !axis.z.is_finite()
        || (axis.x == 0.0 && axis.y == 0.0 && axis.z == 0.0)
    {
        return Err(format!(
            "{type_name} '{name}': Axis must be a finite, non-zero vector (got \
             ({}, {}, {}))",
            axis.x, axis.y, axis.z
        ));
    }
    let reversed = require_bool(container, type_name, "Reversed")?;
    let step = if angle.abs() >= 360.0 {
        360.0 / occurrences as f64
    } else {
        angle / (occurrences - 1) as f64
    };
    let step = if reversed { -step } else { step };
    let step = Rotation::from_axis_angle(axis, step.to_radians()).map_err(|_| {
        format!(
            "{type_name} '{name}': Axis must be a finite, non-zero vector (got \
                 ({}, {}, {}))",
            axis.x, axis.y, axis.z
        )
    })?;
    let mut copies = Vec::with_capacity(occurrences as usize);
    let mut rotation = Rotation::IDENTITY;
    for _ in 0..occurrences {
        copies.push(Matrix4::from_placement(&Placement {
            position: Vector3::ZERO,
            rotation,
        }));
        rotation = step.multiply(&rotation);
    }
    Ok(copies)
}

/// Resolve a pattern's `Originals` link (the feature whose tool gets
/// copied — `App::PropertyLinkSub`, like the PartDesign profiles).
fn originals_target(container: &PropertyContainer, type_name: &str) -> Result<String, String> {
    match container.get("Originals") {
        Some(Property::Link(target)) if !target.is_empty() => Ok(target.clone()),
        Some(Property::LinkSub(target, _)) if !target.is_empty() => Ok(target.clone()),
        _ => Err(format!(
            "{type_name} '{}': Originals must link to the feature being patterned",
            container.name()
        )),
    }
}

/// A feature's **tool** — the solid its own operation adds or removes —
/// plus its [`Polarity`], re-derived from live properties (a pattern
/// recomputes against the current sketch and dimensions, never a
/// cached result).
///
/// * `Pad` / `Pocket` / `Revolution` contribute their profile tool
///   (additive, subtractive, additive).
/// * A pattern contributes the union of all its copies, polarity
///   inherited from its originals (recursion — FreeCAD's `Transformed`
///   composition: patterning a pattern copies every copy).
/// * Anything else contributes its placed mesh additively (primitives,
///   booleans — a documented superset of FreeCAD, which only patterns
///   PartDesign features).
///
/// `type_name`/`name` attribute the errors to the *pattern* asking.
fn original_tool(
    doc: &Document,
    object: &dyn DocumentObject,
    type_name: &str,
    name: &str,
    depth: usize,
) -> Result<(Mesh, Polarity), String> {
    if depth > MAX_PATTERN_NESTING {
        return Err(format!(
            "{type_name} '{name}': pattern nesting deeper than {MAX_PATTERN_NESTING} levels"
        ));
    }
    // Defense in depth: the recompute loop aborts on the original's own
    // failure before the pattern runs, but a *later* recompute skips
    // errored objects — never pattern off a failed feature.
    if let fc_app::ObjectStatus::Error(reason) = object.status() {
        return Err(format!(
            "{type_name} '{name}': Originals '{}' is in error state: {reason}",
            object.name()
        ));
    }
    if let Some(pad) = object.as_any().downcast_ref::<PadFeature>() {
        return Ok((
            pad_tool(&pad.container, "PartDesign::Pad", doc)?,
            Polarity::Additive,
        ));
    }
    if let Some(pocket) = object.as_any().downcast_ref::<PocketFeature>() {
        return Ok((
            pocket_tool(&pocket.container, "PartDesign::Pocket", doc)?,
            Polarity::Subtractive,
        ));
    }
    if let Some(revolution) = object.as_any().downcast_ref::<RevolutionFeature>() {
        return Ok((
            revolution_tool(&revolution.container, "PartDesign::Revolution", doc)?,
            Polarity::Additive,
        ));
    }
    match object.as_any().downcast_ref::<LinearPatternFeature>() {
        Some(pattern) => {
            // The inner pattern's tool: the union of all its copies,
            // polarity inherited from its originals.
            let container = &pattern.container;
            let target = originals_target(container, "PartDesign::LinearPattern")
                .map_err(|err| format!("{type_name} '{name}': {err}"))?;
            let inner = doc
                .get_by_name(&target)
                .ok_or_else(|| format!("{type_name} '{name}': Originals '{target}' not found"))?;
            let (tool, polarity) = original_tool(doc, inner, type_name, name, depth + 1)?;
            let copies = linear_copies(container, "PartDesign::LinearPattern")
                .map_err(|err| format!("{type_name} '{name}': {err}"))?;
            Ok((fuse_copies(&tool, &copies), polarity))
        }
        None => match object.as_any().downcast_ref::<PolarPatternFeature>() {
            Some(pattern) => {
                let container = &pattern.container;
                let target = originals_target(container, "PartDesign::PolarPattern")
                    .map_err(|err| format!("{type_name} '{name}': {err}"))?;
                let inner = doc.get_by_name(&target).ok_or_else(|| {
                    format!("{type_name} '{name}': Originals '{target}' not found")
                })?;
                let (tool, polarity) = original_tool(doc, inner, type_name, name, depth + 1)?;
                let copies = polar_copies(container, "PartDesign::PolarPattern")
                    .map_err(|err| format!("{type_name} '{name}': {err}"))?;
                Ok((fuse_copies(&tool, &copies), polarity))
            }
            None => {
                let mesh =
                    crate::features::placed_mesh_of(object, crate::shape::DEFAULT_DEFLECTION)
                        .ok_or_else(|| {
                            format!(
                                "{type_name} '{name}': Originals '{}' has no computed geometry \
                         (not a part feature, or not yet recomputed)",
                                object.name()
                            )
                        })?;
                Ok((mesh, Polarity::Additive))
            }
        },
    }
}

/// Union of every transformed copy of a tool (the nested pattern's own
/// contribution — `occurrences >= 2` guarantees at least one copy).
fn fuse_copies(tool: &Mesh, copies: &[Matrix4]) -> Mesh {
    let mut fused: Option<Mesh> = None;
    for m in copies {
        let copy = tool.transformed(m);
        fused = Some(match fused {
            None => copy,
            Some(acc) => crate::boolean::fuse(&acc, &copy),
        });
    }
    fused.expect("occurrences >= 2 guarantees a copy")
}

/// The shared execute of both pattern features: resolve `Base` (the
/// solid the copies apply to — semantically the state *before* the
/// original feature was applied, FreeCAD's `BaseFeature`), derive the
/// original's tool + polarity, then fold every copy onto the base.
fn pattern_execute(
    container: &PropertyContainer,
    type_name: &str,
    doc: &Document,
    copies: Vec<Matrix4>,
) -> Result<Mesh, String> {
    let name = container.name().to_string();
    let base_target = match container.get("Base") {
        Some(Property::Link(target)) if !target.is_empty() => target.clone(),
        _ => {
            return Err(format!(
                "{type_name} '{name}': Base must link to a solid feature"
            ))
        }
    };
    let base_object = doc
        .get_by_name(&base_target)
        .ok_or_else(|| format!("{type_name} '{name}': Base '{base_target}' not found"))?;
    let base = crate::features::placed_mesh_of(base_object, crate::shape::DEFAULT_DEFLECTION)
        .ok_or_else(|| {
            format!(
                "{type_name} '{name}': Base '{base_target}' has no computed geometry \
             (not a part feature, or not yet recomputed)"
            )
        })?;

    let target = originals_target(container, type_name)?;
    let original = doc
        .get_by_name(&target)
        .ok_or_else(|| format!("{type_name} '{name}': Originals '{target}' not found"))?;
    let (tool, polarity) = original_tool(doc, original, type_name, &name, 0)?;

    // Fold all copies into ONE tool union first, then a single boolean
    // against the base. Set-theoretically identical to per-copy folds
    // (`cut(A, B∪C) == cut(cut(A,B),C)` in exact arithmetic) and far
    // cheaper: a per-copy cut re-classifies the whole — growing — base
    // mesh every occurrence (quadratic), while the union of disjoint
    // copies costs only the pairwise fuses of the small tools.
    let combined = fuse_copies(&tool, &copies);
    let mesh = match polarity {
        Polarity::Additive => crate::boolean::fuse(&base, &combined),
        Polarity::Subtractive => crate::boolean::cut(&base, &combined),
    };
    Ok(mesh)
}

/// FreeCAD `PartDesign::LinearPattern` (distilled): repeat an original
/// feature's tool along a direction — the boss grid and the row of
/// holes. `Length` is the span between the first and last occurrence
/// (spacing `Length/(Occurrences−1)`); occurrence 1 is the untransformed
/// original's position. An additive original (Pad/Revolution/primitive)
/// fuses its copies onto `Base`; a subtractive one (Pocket) cuts them.
/// Patterning a pattern composes multiplicatively (FreeCAD's
/// `Transformed` semantics): every copy of the inner pattern gets
/// copied again.
pub struct LinearPatternFeature {
    pub container: PropertyContainer,
    /// `Base` folded with every copy of the original's tool.
    pub result: Option<Mesh>,
}

impl LinearPatternFeature {
    /// Properties: `Originals` (LinkSub, required), `Base` (Link,
    /// required), `Direction` ((1, 0, 0)), `Length` (20.0),
    /// `Occurrences` (2), `Reversed` (false), `Placement`.
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Originals", Property::LinkSub(String::new(), Vec::new()));
        container.set("Base", Property::Link(String::new()));
        container.set("Direction", Property::Vector(Vector3::new(1.0, 0.0, 0.0)));
        container.set("Length", Property::Float(20.0));
        container.set("Occurrences", Property::Integer(2));
        container.set("Reversed", Property::Bool(false));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "PartDesign::LinearPattern";
        self.result = None;
        let copies = linear_copies(&self.container, type_name)?;
        let mesh = pattern_execute(&self.container, type_name, doc, copies)?;
        store_result(&self.container, type_name, mesh, &mut self.result)
    }
}

impl_document_object!(LinearPatternFeature, "PartDesign::LinearPattern");

/// FreeCAD `PartDesign::PolarPattern` (distilled): repeat an original
/// feature's tool around an axis through the origin — the bolt circle.
/// `Angle` is the swept arc: a full 360° spaces occurrences `360/N`
/// apart (the last copy never stacks on the first, FreeCAD's rule); a
/// partial arc spaces them `Angle/(N−1)`. Polarity and nesting behave
/// exactly like [`LinearPatternFeature`].
pub struct PolarPatternFeature {
    pub container: PropertyContainer,
    /// `Base` folded with every rotated copy of the original's tool.
    pub result: Option<Mesh>,
}

impl PolarPatternFeature {
    /// Properties: `Originals` (LinkSub, required), `Base` (Link,
    /// required), `Axis` ((0, 0, 1)), `Angle` (360.0), `Occurrences`
    /// (2), `Reversed` (false), `Placement`.
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Originals", Property::LinkSub(String::new(), Vec::new()));
        container.set("Base", Property::Link(String::new()));
        container.set("Axis", Property::Vector(Vector3::Z));
        container.set("Angle", Property::Float(360.0));
        container.set("Occurrences", Property::Integer(2));
        container.set("Reversed", Property::Bool(false));
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        Self {
            container,
            result: None,
        }
    }

    fn execute_impl(&mut self, doc: &Document) -> Result<(), String> {
        let type_name = "PartDesign::PolarPattern";
        self.result = None;
        let copies = polar_copies(&self.container, type_name)?;
        let mesh = pattern_execute(&self.container, type_name, doc, copies)?;
        store_result(&self.container, type_name, mesh, &mut self.result)
    }
}

impl_document_object!(PolarPatternFeature, "PartDesign::PolarPattern");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boolean::mesh_is_closed;
    use crate::features::{BoxFeature, TransformFeature};
    use fc_app::ObjectStatus;
    use fc_sketcher::constraint::Constraint;
    use fc_sketcher::geometry::{Geometry, Point};
    use fc_sketcher::{PointPos, Sketch, SketchObject};

    /// An empty document to hand to direct `execute_impl` calls (the
    /// validation-only tests; the flow tests go through `recompute_all`).
    fn empty_doc() -> Document {
        Document::new("test")
    }

    /// A constrained rectangle of `w x h` with its lower-left corner at
    /// `(x0, y0)`: four corner-coincident lines, two horizontal, two
    /// vertical, width and height dimensioned — the sketch_object.rs
    /// rectangle pattern, sized for round numbers. Drawn exactly to
    /// spec, so the solve converges instantly and the geometry stays
    /// exact (volume assertions rely on that).
    fn rectangle_at(x0: f64, y0: f64, w: f64, h: f64) -> Sketch {
        let mut sk = Sketch::new();
        let top = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(x0, y0),
            end: Point::new(x0 + w, y0),
        });
        let right = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(x0 + w, y0),
            end: Point::new(x0 + w, y0 + h),
        });
        let bottom = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(x0 + w, y0 + h),
            end: Point::new(x0, y0 + h),
        });
        let left = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(x0, y0 + h),
            end: Point::new(x0, y0),
        });
        for (a, pa, b, pb) in [
            (top, PointPos::End, right, PointPos::Start),
            (right, PointPos::End, bottom, PointPos::Start),
            (bottom, PointPos::End, left, PointPos::Start),
            (left, PointPos::End, top, PointPos::Start),
        ] {
            sk.add_constraint(Constraint::Coincident {
                g1: a,
                pos1: pa,
                g2: b,
                pos2: pb,
            })
            .unwrap();
        }
        sk.add_constraint(Constraint::Horizontal { g: top })
            .unwrap();
        sk.add_constraint(Constraint::Vertical { g: right })
            .unwrap();
        sk.add_constraint(Constraint::Horizontal { g: bottom })
            .unwrap();
        sk.add_constraint(Constraint::Vertical { g: left }).unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: top,
            pos1: PointPos::Start,
            g2: top,
            pos2: PointPos::End,
            value: w,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: right,
            pos1: PointPos::Start,
            g2: right,
            pos2: PointPos::End,
            value: h,
        })
        .unwrap();
        sk
    }

    /// A constrained `w x h` rectangle with its lower-left corner at the
    /// origin.
    fn rectangle_sketch(w: f64, h: f64) -> Sketch {
        rectangle_at(0.0, 0.0, w, h)
    }

    /// A centered `2s x 2s` square sketch (for pockets through a box).
    fn centered_square_sketch(s: f64) -> Sketch {
        rectangle_at(-s, -s, 2.0 * s, 2.0 * s)
    }

    /// A half-disc of radius `r`: a semicircular arc bulging towards +x,
    /// closed by the diameter line on the (vertical) y axis. Revolved
    /// about the z axis (the kernel's lathe frame: sketch x = radius,
    /// sketch y = height) it sweeps a sphere — the classic test profile.
    /// Drawn unconstrained (it solves trivially; the tessellation is
    /// what is under test).
    fn half_disc_sketch(r: f64) -> Sketch {
        let mut sk = Sketch::new();
        sk.add_geometry(Geometry::ArcOfCircle {
            center: Point::new(0.0, 0.0),
            radius: r,
            start_angle: -std::f64::consts::FRAC_PI_2,
            end_angle: std::f64::consts::FRAC_PI_2,
        });
        sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, r),
            end: Point::new(0.0, -r),
        });
        sk
    }

    /// One dangling line (an open profile): solves fine as a sketch, but
    /// extracts to an open wire.
    fn dangling_line_sketch() -> Sketch {
        let mut sk = Sketch::new();
        sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(20.0, 0.0),
        });
        sk
    }

    /// An over-constrained sketch (Horizontal + Vertical + three
    /// distances on one line): the solve itself fails.
    fn over_constrained_sketch() -> Sketch {
        let mut sk = Sketch::new();
        let line = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 0.0),
        });
        sk.add_constraint(Constraint::Horizontal { g: line })
            .unwrap();
        sk.add_constraint(Constraint::Vertical { g: line }).unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: line,
            pos1: PointPos::Start,
            g2: line,
            pos2: PointPos::End,
            value: 10.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: line,
            pos1: PointPos::Start,
            g2: line,
            pos2: PointPos::End,
            value: 0.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::Distance {
            g1: line,
            pos1: PointPos::Start,
            g2: line,
            pos2: PointPos::End,
            value: 10.0,
        })
        .unwrap();
        sk
    }

    /// A document with a solved sketch named `Sketch` carrying `sketch`,
    /// plus a Pad named `Pad` linked to it.
    fn doc_with_pad(sketch: Sketch) -> Document {
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch("Sketch", sketch)));
        doc.add(Box::new(PadFeature::new("Pad")));
        doc.set_property("Pad", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc
    }

    fn pad_mesh_of(doc: &Document) -> Mesh {
        doc.get_by_name("Pad")
            .unwrap()
            .as_any()
            .downcast_ref::<PadFeature>()
            .unwrap()
            .result
            .clone()
            .expect("pad computed a mesh")
    }

    #[test]
    fn pad_extrudes_a_constrained_rectangle_into_a_watertight_box() {
        let mut doc = doc_with_pad(rectangle_sketch(20.0, 10.0));
        doc.set_property("Pad", "Length", Property::Float(10.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let mesh = pad_mesh_of(&doc);
        // Volume = area x length, analytic for a line-only profile.
        let volume = mesh.volume();
        assert!(
            (volume - 20.0 * 10.0 * 10.0).abs() < 1e-9,
            "expected a 20x10x10 box (2000), got {volume}"
        );
        assert!(mesh_is_closed(&mesh), "pad mesh must be watertight");
        // 4 wall quads + 2 triangulated caps.
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(
            *doc.get_by_name("Pad").unwrap().status(),
            ObjectStatus::Valid
        );
        // The dependency edge exists: the sketch has the pad as dependent.
        assert_eq!(doc.dependents_of("Sketch"), vec!["Pad".to_string()]);
    }

    #[test]
    fn pad_midplane_spans_both_sides_of_the_sketch_plane() {
        let mut doc = doc_with_pad(rectangle_sketch(20.0, 10.0));
        doc.set_property("Pad", "Length", Property::Float(10.0))
            .unwrap();
        doc.set_property("Pad", "Midplane", Property::Bool(true))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let mesh = pad_mesh_of(&doc);
        assert!((mesh.volume() - 2000.0).abs() < 1e-9);
        let bbox = mesh.bbox().expect("non-empty mesh");
        assert!(
            (bbox.min.z + 5.0).abs() < 1e-9,
            "min z {} != -5",
            bbox.min.z
        );
        assert!((bbox.max.z - 5.0).abs() < 1e-9, "max z {} != 5", bbox.max.z);
    }

    #[test]
    fn pad_reversed_extrudes_along_negative_z() {
        let mut doc = doc_with_pad(rectangle_sketch(20.0, 10.0));
        doc.set_property("Pad", "Length", Property::Float(10.0))
            .unwrap();
        doc.set_property("Pad", "Reversed", Property::Bool(true))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let mesh = pad_mesh_of(&doc);
        // The kernel flips every winding for a -Z extrusion, so the
        // volume stays positive.
        assert!((mesh.volume() - 2000.0).abs() < 1e-9);
        let bbox = mesh.bbox().expect("non-empty mesh");
        assert!(
            (bbox.min.z + 10.0).abs() < 1e-9,
            "min z {} != -10",
            bbox.min.z
        );
        assert!(bbox.max.z.abs() < 1e-9, "max z {} != 0", bbox.max.z);
    }

    #[test]
    fn pad_from_circle_sketch_extrudes_a_cylinder() {
        // A full circle is its own closed loop (never enters chaining).
        let mut sk = Sketch::new();
        sk.add_geometry(Geometry::Circle {
            center: Point::new(0.0, 0.0),
            radius: 3.0,
        });
        let mut doc = doc_with_pad(sk);
        doc.set_property("Pad", "Length", Property::Float(10.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let mesh = pad_mesh_of(&doc);
        assert!(mesh_is_closed(&mesh), "cylinder pad must be watertight");
        // Tessellated circle at 0.01 deflection on r = 3 (~39-gon):
        // within ~0.5% of pi*r^2*h — the honest tessellation error.
        let want = std::f64::consts::PI * 9.0 * 10.0;
        let rel = ((mesh.volume() - want) / want).abs();
        assert!(rel < 0.01, "cylinder volume off by {rel:.4}");
    }

    #[test]
    fn pocket_cuts_a_notch_out_of_a_box() {
        let mut doc = Document::new("test");
        let mut body = BoxFeature::new("Body");
        // Real-assembly geometry: the box (corner-origin `[0, 10]³`,
        // FreeCAD 1.0 convention) is centered under the sketch.
        body.container.set(
            "Placement",
            Property::Placement(Placement::new(
                Vector3::new(-5.0, -5.0, 0.0),
                fc_base::Rotation::IDENTITY,
            )),
        );
        doc.add(Box::new(body));
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            centered_square_sketch(2.0),
        )));
        doc.add(Box::new(PocketFeature::new("Pocket")));
        doc.set_property("Pocket", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc.set_property("Pocket", "Base", Property::Link("Body".into()))
            .unwrap();
        doc.set_property("Pocket", "Length", Property::Float(4.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let pocket = doc
            .get_by_name("Pocket")
            .unwrap()
            .as_any()
            .downcast_ref::<PocketFeature>()
            .unwrap();
        let mesh = pocket.result.as_ref().expect("pocket computed a mesh");
        // Box 10x10x10 minus a 4x4x4 notch from the bottom face.
        let volume = mesh.volume();
        assert!(
            (volume - (1000.0 - 4.0 * 4.0 * 4.0)).abs() < 1e-6,
            "expected 936, got {volume}"
        );
        assert!(mesh_is_closed(mesh), "pocket result must be watertight");
    }

    #[test]
    fn pocket_inverted_tool_misses_a_base_above_the_sketch_plane() {
        // The inverted tool extrudes below z = 0, where the box (which
        // spans [0, 10]) is not: the cut removes nothing.
        let mut doc = Document::new("test");
        doc.add(Box::new(BoxFeature::new("Body")));
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            centered_square_sketch(2.0),
        )));
        doc.add(Box::new(PocketFeature::new("Pocket")));
        doc.set_property("Pocket", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc.set_property("Pocket", "Base", Property::Link("Body".into()))
            .unwrap();
        doc.set_property("Pocket", "Length", Property::Float(4.0))
            .unwrap();
        doc.set_property("Pocket", "Inverted", Property::Bool(true))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let pocket = doc
            .get_by_name("Pocket")
            .unwrap()
            .as_any()
            .downcast_ref::<PocketFeature>()
            .unwrap();
        let volume = pocket
            .result
            .as_ref()
            .expect("pocket computed a mesh")
            .volume();
        assert!(
            (volume - 1000.0).abs() < 1e-6,
            "inverted pocket must not cut the box, got {volume}"
        );
    }

    #[test]
    fn pocket_without_base_link_is_an_honest_dangling_dependency() {
        // An unset Base is an empty link — "no link" in FreeCAD terms
        // (real files carry them for optional links), so the graph walk
        // no longer rejects it; the pocket's own execute reports the
        // missing required base and the object lands in Error.
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            centered_square_sketch(2.0),
        )));
        doc.add(Box::new(PocketFeature::new("Pocket")));
        doc.set_property("Pocket", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        let err = doc.recompute_all().expect_err("Base is required");
        assert!(err.to_string().contains("Base must link"), "{err}");
        // The failed execute marks the pocket Error (FreeCAD behavior:
        // a feature that fails recompute is flagged, its dependents
        // stop), not merely Touched.
        assert!(matches!(
            doc.get_by_name("Pocket").unwrap().status(),
            ObjectStatus::Error(_)
        ));

        // The feature-level message surfaces on a direct execute (the
        // validation the CLI and future API callers hit first; the
        // Profile branch is the same shared helper the Pad tests hit).
        let mut pocket = PocketFeature::new("Pocket");
        let err = pocket.execute_impl(&empty_doc()).unwrap_err();
        assert!(err.contains("Base must link to a solid feature"), "{err}");
    }

    #[test]
    fn revolution_of_a_half_disc_is_a_sphere() {
        let mut doc = doc_with_revolution(half_disc_sketch(5.0));
        doc.recompute_all().expect("recompute succeeds");
        let mesh = revolution_mesh_of(&doc);
        assert!(
            mesh_is_closed(&mesh),
            "full-turn revolution must be seam-welded"
        );
        let want = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
        let rel = ((mesh.volume() - want) / want).abs();
        assert!(
            rel < 0.01,
            "sphere volume off by {rel:.4} ({})",
            mesh.volume()
        );
    }

    #[test]
    fn revolution_about_the_sketch_vertical_axis_is_the_same_solid() {
        // --axis y: the same half disc, revolved about the sketch's
        // vertical axis. A full turn sweeps the identical sphere (the
        // kernel's frame start is a rotation about the axis).
        let mut doc = doc_with_revolution(half_disc_sketch(5.0));
        doc.set_property("Rev", "Axis", Property::Vector(Vector3::Y))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let mesh = revolution_mesh_of(&doc);
        assert!(mesh_is_closed(&mesh));
        let want = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
        let rel = ((mesh.volume() - want) / want).abs();
        assert!(
            rel < 0.01,
            "sphere volume off by {rel:.4} ({})",
            mesh.volume()
        );
    }

    #[test]
    fn revolution_partial_turn_sweeps_a_fraction_of_the_volume() {
        let mut doc = doc_with_revolution(half_disc_sketch(5.0));
        doc.set_property("Rev", "Angle", Property::Float(180.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let mesh = revolution_mesh_of(&doc);
        assert!(
            mesh_is_closed(&mesh),
            "partial turn caps must close the solid"
        );
        let want = 2.0 / 3.0 * std::f64::consts::PI * 125.0;
        let rel = ((mesh.volume() - want) / want).abs();
        assert!(
            rel < 0.01,
            "half-sphere volume off by {rel:.4} ({})",
            mesh.volume()
        );
    }

    #[test]
    fn revolution_reversed_sweeps_the_other_way_around() {
        // For a full turn the sweep direction cannot change the solid:
        // Reversed must reproduce the same volume (and positive winding).
        let mut doc = doc_with_revolution(half_disc_sketch(5.0));
        doc.set_property("Rev", "Reversed", Property::Bool(true))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let mesh = revolution_mesh_of(&doc);
        let want = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
        let rel = ((mesh.volume() - want) / want).abs();
        assert!(rel < 0.01, "reversed sphere volume off by {rel:.4}");
    }

    fn doc_with_revolution(sketch: Sketch) -> Document {
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch("Sketch", sketch)));
        doc.add(Box::new(RevolutionFeature::new("Rev")));
        doc.set_property("Rev", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc
    }

    fn revolution_mesh_of(doc: &Document) -> Mesh {
        doc.get_by_name("Rev")
            .unwrap()
            .as_any()
            .downcast_ref::<RevolutionFeature>()
            .unwrap()
            .result
            .clone()
            .expect("revolution computed a mesh")
    }

    #[test]
    fn pad_from_an_open_profile_fails_and_so_do_its_dependents() {
        let mut doc = doc_with_pad(dangling_line_sketch());
        // A dependent of the pad: it must fail once the pad has no
        // computed geometry to transform.
        let mut transform = TransformFeature::new("Dep");
        transform
            .container
            .set("Source", Property::Link("Pad".to_string()));
        doc.add(Box::new(transform));

        // First recompute aborts at the pad: open wire, no geometry.
        let err = doc.recompute_all().expect_err("open profile must fail");
        assert!(
            err.to_string().contains("open wire"),
            "honest open-wire error: {err}"
        );
        let pad = doc.get_by_name("Pad").unwrap();
        assert!(matches!(
            pad.status(),
            ObjectStatus::Error(reason) if reason.contains("open wire")
        ));
        let pad = pad.as_any().downcast_ref::<PadFeature>().unwrap();
        assert!(pad.result.is_none(), "failed pad keeps no stale geometry");
        // The dependent never ran: still Touched (the run aborted).
        assert_eq!(
            *doc.get_by_name("Dep").unwrap().status(),
            ObjectStatus::Touched
        );

        // The next recompute skips the errored objects and runs the
        // dependent, which honestly fails on the pad's missing geometry.
        let err = doc.recompute_all().expect_err("dependent must fail too");
        assert!(
            err.to_string().contains("recompute of 'Dep' failed"),
            "{err}"
        );
        assert!(matches!(
            doc.get_by_name("Dep").unwrap().status(),
            ObjectStatus::Error(reason) if reason.contains("no computed geometry")
        ));
    }

    #[test]
    fn pad_refuses_a_profile_in_error_state() {
        let mut doc = doc_with_pad(over_constrained_sketch());
        // The sketch's own solve fails first and aborts the run.
        let err = doc
            .recompute_all()
            .expect_err("over-constrained sketch fails");
        assert!(
            err.to_string().contains("recompute of 'Sketch' failed"),
            "{err}"
        );
        assert!(matches!(
            doc.get_by_name("Sketch").unwrap().status(),
            ObjectStatus::Error(reason) if reason.contains("over-constrained")
        ));
        // The pad is still Touched (never ran).
        assert_eq!(
            *doc.get_by_name("Pad").unwrap().status(),
            ObjectStatus::Touched
        );

        // A later recompute skips the errored sketch and runs the pad —
        // which must refuse the broken profile instead of silently
        // consuming its last-known geometry.
        let err = doc
            .recompute_all()
            .expect_err("pad must refuse an errored profile");
        assert!(
            err.to_string()
                .contains("Profile 'Sketch' is in error state"),
            "{err}"
        );
        assert!(matches!(
            doc.get_by_name("Pad").unwrap().status(),
            ObjectStatus::Error(reason) if reason.contains("in error state")
        ));
    }

    #[test]
    fn pad_rejects_bad_scalar_parameters_upfront() {
        // No Profile link at all.
        let mut pad = PadFeature::new("Pad");
        let err = pad.execute_impl(&empty_doc()).unwrap_err();
        assert!(
            err.contains("Profile must link to a sketch object"),
            "{err}"
        );

        // Non-positive / non-finite Length.
        let mut doc = doc_with_pad(rectangle_sketch(20.0, 10.0));
        doc.set_property("Pad", "Length", Property::Float(0.0))
            .unwrap();
        let err = doc.recompute_all().expect_err("zero length must fail");
        assert!(
            err.to_string().contains("Length must be finite and > 0"),
            "{err}"
        );
        doc.set_property("Pad", "Length", Property::Float(f64::NAN))
            .unwrap();
        let err = doc.recompute_all().expect_err("NaN length must fail");
        assert!(
            err.to_string().contains("Length must be finite and > 0"),
            "{err}"
        );
    }

    #[test]
    fn revolution_rejects_bad_axis_and_angle_upfront() {
        let mut rev = RevolutionFeature::new("Rev");
        let err = rev.execute_impl(&empty_doc()).unwrap_err();
        assert!(
            err.contains("Profile must link to a sketch object"),
            "{err}"
        );

        let mut doc = doc_with_revolution(half_disc_sketch(5.0));
        doc.set_property("Rev", "Angle", Property::Float(0.0))
            .unwrap();
        let err = doc.recompute_all().expect_err("zero angle must fail");
        assert!(
            err.to_string()
                .contains("Angle must be finite and non-zero"),
            "{err}"
        );
        doc.set_property("Rev", "Angle", Property::Float(360.0))
            .unwrap();
        doc.set_property("Rev", "Axis", Property::Vector(Vector3::ZERO))
            .unwrap();
        let err = doc.recompute_all().expect_err("zero axis must fail");
        assert!(
            err.to_string()
                .contains("Axis must be a finite, non-zero vector"),
            "{err}"
        );
    }

    // -----------------------------------------------------------------
    // Feature patterns
    // -----------------------------------------------------------------

    /// A centered `2s x 2s x height` box base spanning z in [0, height].
    fn box_base(side: f64, height: f64) -> BoxFeature {
        let mut body = BoxFeature::new("Base");
        body.container.set("Length", Property::Float(side));
        body.container.set("Width", Property::Float(side));
        body.container.set("Height", Property::Float(height));
        body.container.set(
            "Placement",
            Property::Placement(Placement::new(
                Vector3::new(-side / 2.0, -side / 2.0, 0.0),
                fc_base::Rotation::IDENTITY,
            )),
        );
        body
    }

    /// A document holding a centered 30x30x10 base and a through pocket
    /// of a centered 4x4 square (the pattern test bed): `Pocket` cuts
    /// 160 mm³, leaving 8840.
    fn pattern_doc() -> Document {
        let mut doc = Document::new("test");
        doc.add(Box::new(box_base(30.0, 10.0)));
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            centered_square_sketch(2.0),
        )));
        doc.add(Box::new(PocketFeature::new("Pocket")));
        doc.set_property("Pocket", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc.set_property("Pocket", "Base", Property::Link("Base".into()))
            .unwrap();
        doc.set_property("Pocket", "Length", Property::Float(10.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        doc
    }

    /// Wire up a linear pattern on `originals` over `base`.
    fn add_linear(doc: &mut Document, name: &str, originals: &str, base: &str) {
        doc.add(Box::new(LinearPatternFeature::new(name)));
        doc.set_property(name, "Originals", Property::Link(originals.into()))
            .unwrap();
        doc.set_property(name, "Base", Property::Link(base.into()))
            .unwrap();
    }

    /// The pattern feature's computed volume (panics when it failed).
    fn result_volume(doc: &Document, name: &str) -> f64 {
        let object = doc.get_by_name(name).unwrap();
        let mesh = match object.type_name() {
            "PartDesign::LinearPattern" => object
                .as_any()
                .downcast_ref::<LinearPatternFeature>()
                .unwrap()
                .result
                .as_ref()
                .expect("pattern computed a mesh"),
            _ => object
                .as_any()
                .downcast_ref::<PolarPatternFeature>()
                .unwrap()
                .result
                .as_ref()
                .expect("pattern computed a mesh"),
        };
        mesh.volume()
    }

    /// The pattern feature's computed mesh, for watertightness checks.
    fn result_mesh<'a>(doc: &'a Document, name: &str) -> &'a Mesh {
        let object = doc.get_by_name(name).unwrap();
        if let Some(f) = object.as_any().downcast_ref::<LinearPatternFeature>() {
            return f.result.as_ref().expect("pattern computed a mesh");
        }
        object
            .as_any()
            .downcast_ref::<PolarPatternFeature>()
            .unwrap()
            .result
            .as_ref()
            .expect("pattern computed a mesh")
    }

    #[test]
    fn linear_pattern_of_pocket_cuts_exactly_n_disjoint_holes() {
        let mut doc = pattern_doc();
        add_linear(&mut doc, "Row", "Pocket", "Base");
        // 3 occurrences over a 12 mm span: holes at (0, 0), (0, -6)
        // and (0, -12) — each 4x4, all inside the 30x30 plate, all
        // disjoint.
        doc.set_property("Row", "Direction", Property::Vector(Vector3::Y))
            .unwrap();
        doc.set_property("Row", "Length", Property::Float(12.0))
            .unwrap();
        doc.set_property("Row", "Occurrences", Property::Integer(3))
            .unwrap();
        doc.set_property("Row", "Reversed", Property::Bool(true))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let volume = result_volume(&doc, "Row");
        // 9000 - 3 x (4 x 4 x 10) — exact arithmetic on rectangles.
        assert!(
            (volume - (9000.0 - 3.0 * 160.0)).abs() < 1e-6,
            "expected 8520, got {volume}"
        );
        assert!(
            mesh_is_closed(result_mesh(&doc, "Row")),
            "pattern result must be watertight"
        );
    }

    #[test]
    fn nested_linear_patterns_compose_multiplicatively() {
        let mut doc = pattern_doc();
        // Y row: holes at (0, 0) and (0, -8).
        add_linear(&mut doc, "Row", "Pocket", "Base");
        doc.set_property("Row", "Direction", Property::Vector(Vector3::Y))
            .unwrap();
        doc.set_property("Row", "Length", Property::Float(8.0))
            .unwrap();
        doc.set_property("Row", "Occurrences", Property::Integer(2))
            .unwrap();
        doc.set_property("Row", "Reversed", Property::Bool(true))
            .unwrap();
        // X grid over the ROW's copies: (0, 0), (0, -8) and (8, 0),
        // (8, -8) — a 2x2 grid from one pocket.
        add_linear(&mut doc, "Grid", "Row", "Base");
        doc.set_property(
            "Grid",
            "Direction",
            Property::Vector(Vector3::new(1.0, 0.0, 0.0)),
        )
        .unwrap();
        doc.set_property("Grid", "Length", Property::Float(8.0))
            .unwrap();
        doc.set_property("Grid", "Occurrences", Property::Integer(2))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let volume = result_volume(&doc, "Grid");
        // 9000 - 4 x 160 — the multiplicative Transformed composition.
        assert!(
            (volume - (9000.0 - 4.0 * 160.0)).abs() < 1e-6,
            "expected 8360, got {volume}"
        );
        assert!(
            mesh_is_closed(result_mesh(&doc, "Grid")),
            "nested pattern result must be watertight"
        );
    }

    #[test]
    fn linear_pattern_of_pad_fuses_exactly_n_disjoint_copies() {
        let mut doc = Document::new("test");
        // The base sits strictly below the pads' z slab (a 1 mm air
        // gap): the union is exact disjoint-set arithmetic, no
        // coplanar-face booleans in the way of the volume assertion.
        let mut base = BoxFeature::new("Base");
        base.container.set("Length", Property::Float(30.0));
        base.container.set("Width", Property::Float(30.0));
        base.container.set("Height", Property::Float(4.0));
        base.container.set(
            "Placement",
            Property::Placement(Placement::new(
                Vector3::new(-15.0, -15.0, -5.0),
                fc_base::Rotation::IDENTITY,
            )),
        );
        doc.add(Box::new(base));
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            rectangle_at(-12.0, -2.5, 5.0, 5.0),
        )));
        doc.add(Box::new(PadFeature::new("Pad")));
        doc.set_property("Pad", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc.set_property("Pad", "Length", Property::Float(4.0))
            .unwrap();
        // 3 copies over a 20 mm span: prisms at x = -12, -2 and +8
        // (each 5 wide — disjoint), all additive.
        add_linear(&mut doc, "Row", "Pad", "Base");
        doc.set_property(
            "Row",
            "Direction",
            Property::Vector(Vector3::new(1.0, 0.0, 0.0)),
        )
        .unwrap();
        doc.set_property("Row", "Length", Property::Float(20.0))
            .unwrap();
        doc.set_property("Row", "Occurrences", Property::Integer(3))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let volume = result_volume(&doc, "Row");
        // 3600 (base) + 3 x 100 (5x5x4 prisms).
        assert!(
            (volume - (3600.0 + 3.0 * 100.0)).abs() < 1e-6,
            "expected 3900, got {volume}"
        );
        assert!(
            mesh_is_closed(result_mesh(&doc, "Row")),
            "additive pattern result must be watertight"
        );
    }

    #[test]
    fn polar_pattern_full_turn_spaces_360_over_n() {
        let mut doc = Document::new("test");
        doc.add(Box::new(box_base(30.0, 10.0)));
        // A 2x2 square pocket centered at (10, 0).
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            rectangle_at(9.0, -1.0, 2.0, 2.0),
        )));
        doc.add(Box::new(PocketFeature::new("Pocket")));
        doc.set_property("Pocket", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc.set_property("Pocket", "Base", Property::Link("Base".into()))
            .unwrap();
        doc.set_property("Pocket", "Length", Property::Float(10.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");

        doc.add(Box::new(PolarPatternFeature::new("Circle")));
        doc.set_property("Circle", "Originals", Property::Link("Pocket".into()))
            .unwrap();
        doc.set_property("Circle", "Base", Property::Link("Base".into()))
            .unwrap();
        doc.set_property("Circle", "Axis", Property::Vector(Vector3::Z))
            .unwrap();
        doc.set_property("Circle", "Angle", Property::Float(360.0))
            .unwrap();
        doc.set_property("Circle", "Occurrences", Property::Integer(6))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let volume = result_volume(&doc, "Circle");
        // Six 2x2x10 holes on the r=10 circle (chord 10 apart —
        // disjoint), each removing exactly 40: 9000 - 240.
        assert!(
            (volume - (9000.0 - 6.0 * 40.0)).abs() < 1e-6,
            "expected 8760, got {volume}"
        );
        assert!(
            mesh_is_closed(result_mesh(&doc, "Circle")),
            "polar pattern result must be watertight"
        );
    }

    #[test]
    fn polar_pattern_partial_arc_and_reversed() {
        let mut doc = Document::new("test");
        doc.add(Box::new(box_base(30.0, 10.0)));
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            rectangle_at(9.0, -1.0, 2.0, 2.0),
        )));
        doc.add(Box::new(PocketFeature::new("Pocket")));
        doc.set_property("Pocket", "Profile", Property::Link("Sketch".into()))
            .unwrap();
        doc.set_property("Pocket", "Base", Property::Link("Base".into()))
            .unwrap();
        doc.set_property("Pocket", "Length", Property::Float(10.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");

        doc.add(Box::new(PolarPatternFeature::new("Arc")));
        doc.set_property("Arc", "Originals", Property::Link("Pocket".into()))
            .unwrap();
        doc.set_property("Arc", "Base", Property::Link("Base".into()))
            .unwrap();
        doc.set_property("Arc", "Axis", Property::Vector(Vector3::Z))
            .unwrap();
        doc.set_property("Arc", "Angle", Property::Float(90.0))
            .unwrap();
        doc.set_property("Arc", "Occurrences", Property::Integer(3))
            .unwrap();
        // Partial arc: steps of 90/(3-1) = 45 degrees.
        doc.recompute_all().expect("recompute succeeds");
        let volume = result_volume(&doc, "Arc");
        assert!(
            (volume - (9000.0 - 3.0 * 40.0)).abs() < 1e-6,
            "expected 8880, got {volume}"
        );

        // Reversed flips the sweep: same three holes, mirrored side —
        // same volume.
        doc.set_property("Arc", "Reversed", Property::Bool(true))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let volume = result_volume(&doc, "Arc");
        assert!(
            (volume - (9000.0 - 3.0 * 40.0)).abs() < 1e-6,
            "reversed arc must cut the same volume: got {volume}"
        );
    }

    #[test]
    fn pattern_parametric_edit_cascades_through_the_graph() {
        let mut doc = pattern_doc();
        add_linear(&mut doc, "Row", "Pocket", "Base");
        doc.set_property("Row", "Direction", Property::Vector(Vector3::Y))
            .unwrap();
        doc.set_property("Row", "Length", Property::Float(8.0))
            .unwrap();
        doc.set_property("Row", "Occurrences", Property::Integer(2))
            .unwrap();
        doc.set_property("Row", "Reversed", Property::Bool(true))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");

        // Deepen is wrong way round — HALVE the pocket depth: the cut
        // volume halves, the pattern follows through the graph.
        doc.set_property("Pocket", "Length", Property::Float(5.0))
            .unwrap();
        doc.recompute_all().expect("recompute succeeds");
        let volume = result_volume(&doc, "Row");
        assert!(
            (volume - (9000.0 - 2.0 * (4.0 * 4.0 * 5.0))).abs() < 1e-6,
            "expected 9000 - 160 = 8840, got {volume}"
        );
    }

    #[test]
    fn pattern_validation_is_upfront_and_honest() {
        // Occurrences below 2 is not a pattern.
        let mut doc = pattern_doc();
        add_linear(&mut doc, "Row", "Pocket", "Base");
        doc.set_property("Row", "Occurrences", Property::Integer(1))
            .unwrap();
        let err = doc.recompute_all().expect_err("occurrences 1 must fail");
        assert!(
            err.to_string().contains("Occurrences must be >= 2"),
            "{err}"
        );

        // A zero-length span stacks every copy.
        doc.set_property("Row", "Occurrences", Property::Integer(2))
            .unwrap();
        doc.set_property("Row", "Length", Property::Float(0.0))
            .unwrap();
        let err = doc.recompute_all().expect_err("zero length must fail");
        assert!(
            err.to_string().contains("Length must be finite and > 0"),
            "{err}"
        );

        // A zero direction has no unit vector.
        doc.set_property("Row", "Length", Property::Float(8.0))
            .unwrap();
        doc.set_property("Row", "Direction", Property::Vector(Vector3::ZERO))
            .unwrap();
        let err = doc.recompute_all().expect_err("zero direction must fail");
        assert!(
            err.to_string()
                .contains("Direction must be a finite, non-zero vector"),
            "{err}"
        );

        // Missing links are named, not guessed.
        doc.set_property("Row", "Direction", Property::Vector(Vector3::Y))
            .unwrap();
        doc.set_property("Row", "Originals", Property::Link(String::new()))
            .unwrap();
        let err = doc.recompute_all().expect_err("empty originals must fail");
        assert!(err.to_string().contains("Originals must link to"), "{err}");
        doc.set_property("Row", "Originals", Property::Link("Ghost".into()))
            .unwrap();
        let err = doc.recompute_all().expect_err("ghost originals must fail");
        assert!(err.to_string().contains("Ghost"), "{err}");
        doc.set_property("Row", "Originals", Property::Link("Pocket".into()))
            .unwrap();
        doc.set_property("Row", "Base", Property::Link("Ghost".into()))
            .unwrap();
        let err = doc.recompute_all().expect_err("ghost base must fail");
        assert!(err.to_string().contains("Ghost"), "{err}");
    }

    #[test]
    fn polar_pattern_validation_is_upfront_and_honest() {
        let mut doc = pattern_doc();
        doc.add(Box::new(PolarPatternFeature::new("Circle")));
        doc.set_property("Circle", "Originals", Property::Link("Pocket".into()))
            .unwrap();
        doc.set_property("Circle", "Base", Property::Link("Base".into()))
            .unwrap();
        // Zero angle is no revolution at all.
        doc.set_property("Circle", "Angle", Property::Float(0.0))
            .unwrap();
        doc.set_property("Circle", "Occurrences", Property::Integer(3))
            .unwrap();
        let err = doc.recompute_all().expect_err("zero angle must fail");
        assert!(
            err.to_string().contains("Angle must be finite, non-zero"),
            "{err}"
        );
        // More than a full turn is refused (FreeCAD's rule).
        doc.set_property("Circle", "Angle", Property::Float(361.0))
            .unwrap();
        let err = doc.recompute_all().expect_err("over-360 must fail");
        assert!(err.to_string().contains("within ±360"), "{err}");
        // A zero axis is not an axis.
        doc.set_property("Circle", "Angle", Property::Float(360.0))
            .unwrap();
        doc.set_property("Circle", "Axis", Property::Vector(Vector3::ZERO))
            .unwrap();
        let err = doc.recompute_all().expect_err("zero axis must fail");
        assert!(
            err.to_string()
                .contains("Axis must be a finite, non-zero vector"),
            "{err}"
        );
    }
}
