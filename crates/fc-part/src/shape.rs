//! Parametric primitives, triangle meshes, and the exact B-rep.
//!
//! # Conventions — verified against real FreeCAD 1.0 (AppImage r39109)
//!
//! Local coordinates of the analytic primitives, matching the shapes
//! `FreeCAD 1.0` itself builds (checked with `Shape.BoundBox` on
//! `Part::Box`/`Cylinder`/`Cone`/`Torus` with default placements):
//!
//! * **Box** — FreeCAD `Part::Box`: the corner sits at the origin:
//!   `x ∈ [0, length]`, `y ∈ [0, width]`, `z ∈ [0, height]`.
//! * **Sphere** — centered on the origin.
//! * **Cylinder** — along Z, base on the XY plane: `z ∈ [0, height]`.
//! * **Cone** — frustum with `radius1` at the **base** (`z = 0`) and
//!   `radius2` at the **top** (`z = height`); `radius2 == 0` is a plain
//!   cone with its apex up.
//! * **Torus** — in the XY plane, centered on the origin; `radius1` = ring
//!   radius, `radius2` = tube radius. Self-intersecting rings
//!   (`radius2 > radius1`) are allowed, like FreeCAD's.
//! * **Meshes** produced by [`TopoShape::tessellate`] are watertight for the
//!   analytic primitives (vertices shared through the index buffer) and wound
//!   CCW when viewed from outside (right-hand rule normals point outward).
//!
//! # The exact B-rep variant — and its honesty contract
//!
//! **Brep** carries an [`ExactBRep`] — typically read from a real OCCT
//! BRep payload (a FreeCAD `.FCStd` shape sidecar, [`crate::occt_brp`]).
//! Unlike the analytic five it is not guaranteed to measure or
//! tessellate: the exact tiers refuse some geometry **by name** (a
//! B-spline face has no deflection-true interior sampling yet). The
//! polymorphic surface therefore comes in two forms per operation:
//!
//! * the infallible `volume`/`surface_area`/`tessellate` — exact for
//!   the analytic five and meshes; for a measuring B-rep equally
//!   exact, for a refusing one honestly *empty* (`NaN` never reads as
//!   a number, the empty mesh never draws a solid — neither can
//!   silently lie the way a returned `0.0` would);
//! * the `try_volume`/`try_surface_area`/`try_tessellate` channels —
//!   `Result` with the refusing geometry named, for callers that
//!   report rather than display.
//!
//! The bounding box is the one place a cheap answer is documented as
//! approximate: the tessellation box grown by the deflection (an outer
//! bound — see [`TopoShape::bbox`]).

use fc_base::{BoundBox3, Matrix4, Vector3};

use crate::measure::{brep_area, brep_volume};
use crate::tessellate::tessellate_brep;
use crate::topology::{ExactBRep, Surface};

/// Tessellation quality used when a caller doesn't care
/// (max chordal deviation, model units).
pub const DEFAULT_DEFLECTION: f64 = 0.2;

/// An analytic primitive or a tessellated mesh — the `TopoShape` of this
/// kernel (FreeCAD/OCCT `TopoDS_Shape`, distilled to its honest core).
#[derive(Debug, Clone, PartialEq)]
pub enum TopoShape {
    /// Axis-aligned box, corner at the origin like FreeCAD `Part::Box`:
    /// occupies `[0, length] x [0, width] x [0, height]`.
    Box {
        length: f64,
        width: f64,
        height: f64,
    },
    Sphere {
        radius: f64,
    },
    /// Along +Z, base on the XY plane: `[0, height]`, like FreeCAD
    /// `Part::Cylinder`.
    Cylinder {
        radius: f64,
        height: f64,
    },
    Cone {
        radius1: f64,
        radius2: f64,
        height: f64,
    },
    /// FreeCAD `Part::Torus`: `radius1` = ring radius, `radius2` = tube
    /// radius, centered on origin in the XY plane.
    Torus {
        radius1: f64,
        radius2: f64,
    },
    /// The exact boundary representation — real OCCT BRep geometry
    /// (read from a FreeCAD payload by [`crate::occt_brp`], or built
    /// by hand). It measures and tessellates exactly wherever the
    /// exact tiers support the geometry, and refuses **by name**
    /// where they do not (B-spline faces) — see the module doc for the
    /// two-channel honesty contract.
    Brep(ExactBRep),
    Mesh(Mesh),
}

impl TopoShape {
    /// `"Box"`, `"Sphere"`, `"Cylinder"`, `"Cone"`, `"Torus"`, `"Brep"`,
    /// `"Mesh"`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            TopoShape::Box { .. } => "Box",
            TopoShape::Sphere { .. } => "Sphere",
            TopoShape::Cylinder { .. } => "Cylinder",
            TopoShape::Cone { .. } => "Cone",
            TopoShape::Torus { .. } => "Torus",
            TopoShape::Brep(_) => "Brep",
            TopoShape::Mesh(_) => "Mesh",
        }
    }

    /// Reject non-finite or non-positive dimensions (an empty box is a
    /// modeling error, not a shape).
    ///
    /// Rules: every value must be finite; Box/Sphere/Cylinder dimensions
    /// must be `> 0`; Cone needs `height > 0`, radii `>= 0` with at least
    /// one `> 0`; Torus needs `radius1 > 0` and `radius2 > 0` (a
    /// self-intersecting ring — `radius2 > radius1` — is allowed, like
    /// FreeCAD's). Meshes must have finite vertices and in-range triangle
    /// indices. Messages use the FreeCAD property names so feature errors
    /// read like `Length must be > 0`.
    ///
    /// `Brep` re-runs [`ExactBRep::validate`] — the same structural
    /// contract the OCCT reader already enforced on read (finite
    /// geometry, bit-exact edge endpoints, closed wires); cheap,
    /// total, and honest about what it checks.
    pub fn validate(&self) -> Result<(), String> {
        fn require_finite(name: &str, v: f64) -> Result<(), String> {
            if v.is_finite() {
                Ok(())
            } else {
                Err(format!("{name} must be finite"))
            }
        }
        fn require_positive(name: &str, v: f64) -> Result<(), String> {
            if !v.is_finite() {
                Err(format!("{name} must be finite"))
            } else if v > 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be > 0"))
            }
        }

        match self {
            TopoShape::Box {
                length,
                width,
                height,
            } => {
                require_positive("Length", *length)?;
                require_positive("Width", *width)?;
                require_positive("Height", *height)
            }
            TopoShape::Sphere { radius } => require_positive("Radius", *radius),
            TopoShape::Cylinder { radius, height } => {
                require_positive("Radius", *radius)?;
                require_positive("Height", *height)
            }
            TopoShape::Cone {
                radius1,
                radius2,
                height,
            } => {
                require_finite("Radius1", *radius1)?;
                require_finite("Radius2", *radius2)?;
                if *radius1 < 0.0 {
                    return Err("Radius1 must be >= 0".to_string());
                }
                if *radius2 < 0.0 {
                    return Err("Radius2 must be >= 0".to_string());
                }
                require_positive("Height", *height)?;
                if *radius1 <= 0.0 && *radius2 <= 0.0 {
                    Err("Radius1 and Radius2 cannot both be 0".to_string())
                } else {
                    Ok(())
                }
            }
            TopoShape::Torus { radius1, radius2 } => {
                require_positive("Radius1", *radius1)?;
                require_positive("Radius2", *radius2)
            }
            TopoShape::Brep(brep) => brep.validate(),
            TopoShape::Mesh(mesh) => mesh.validate(),
        }
    }

    /// Exact analytic bounding box. A `Mesh` variant with no vertices yields
    /// the empty (default) box.
    ///
    /// `Brep` — the one documented approximation on this surface: the
    /// box of the [`DEFAULT_DEFLECTION`] tessellation, grown by that
    /// deflection on every side. Every mesh vertex lies ON the surface
    /// (rim samples and surface evaluations), so the mesh box sits
    /// inside the true box; every surface point lies within the
    /// deflection of the mesh (the sagitta discipline), so growing by
    /// the deflection contains the true box — an **outer bound** exact
    /// for planar faces (where the growth is pure slack) and
    /// deflection-true for curved ones. A BndLib-style per-surface
    /// closed form would be tight; this is the cheap honest path, and
    /// it says so. When the tessellator refuses (a B-spline face) the
    /// fallback is the union of the model's vertex box with every
    /// B-spline face's control-net box — the hull property makes the
    /// poles an outer bound of the patch, but the vertices alone can
    /// UNDER-bound curved analytic faces in such a mixed payload:
    /// documented, not silent.
    pub fn bbox(&self) -> BoundBox3 {
        match self {
            TopoShape::Box {
                length,
                width,
                height,
            } => BoundBox3::new(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(*length, *width, *height),
            ),
            TopoShape::Sphere { radius } => BoundBox3::new(
                Vector3::new(-radius, -radius, -radius),
                Vector3::new(*radius, *radius, *radius),
            ),
            TopoShape::Cylinder { radius, height } => BoundBox3::new(
                Vector3::new(-radius, -radius, 0.0),
                Vector3::new(*radius, *radius, *height),
            ),
            TopoShape::Cone {
                radius1,
                radius2,
                height,
            } => {
                // The widest ring bounds the horizontal extent.
                let r = radius1.max(*radius2);
                BoundBox3::new(Vector3::new(-r, -r, 0.0), Vector3::new(r, r, *height))
            }
            TopoShape::Torus { radius1, radius2 } => {
                let outer = radius1 + radius2;
                BoundBox3::new(
                    Vector3::new(-outer, -outer, -radius2),
                    Vector3::new(outer, outer, *radius2),
                )
            }
            TopoShape::Brep(brep) => brep_bbox(brep),
            TopoShape::Mesh(mesh) => mesh.bbox().unwrap_or_default(),
        }
    }

    /// Exact analytic volume (mesh variant: sum of signed tetrahedra
    /// against the origin).
    ///
    /// `Brep`: the exact divergence-theorem measure when every face
    /// reads ([`crate::measure`]), `NaN` when a face refuses by name —
    /// NaN never displays as a plausible number and it propagates
    /// through sums (a poisoned total stays poisoned), which is the
    /// honest "no number" a silent `0.0` would never be. Callers that
    /// need the refusing face use [`TopoShape::try_volume`].
    pub fn volume(&self) -> f64 {
        match self {
            TopoShape::Box {
                length,
                width,
                height,
            } => length * width * height,
            TopoShape::Sphere { radius } => {
                4.0 / 3.0 * std::f64::consts::PI * radius * radius * radius
            }
            TopoShape::Cylinder { radius, height } => {
                std::f64::consts::PI * radius * radius * height
            }
            TopoShape::Cone {
                radius1,
                radius2,
                height,
            } => {
                // Frustum: V = pi*h/3 * (r1^2 + r1*r2 + r2^2).
                std::f64::consts::PI * height / 3.0
                    * (radius1 * radius1 + radius1 * radius2 + radius2 * radius2)
            }
            TopoShape::Torus { radius1, radius2 } => {
                // V = 2*pi^2 * R * r^2 (swept volume; also what a
                // self-intersecting ring reports, by convention here).
                2.0 * std::f64::consts::PI * std::f64::consts::PI * radius1 * radius2 * radius2
            }
            TopoShape::Brep(brep) => brep_volume(brep).unwrap_or(f64::NAN),
            TopoShape::Mesh(mesh) => mesh.volume(),
        }
    }

    /// Exact analytic surface area (mesh variant: sum of triangle areas).
    ///
    /// `Brep`: exact when every face reads, `NaN` when a face refuses
    /// by name — the same honesty contract as [`TopoShape::volume`];
    /// the reason is available through [`TopoShape::try_surface_area`].
    pub fn surface_area(&self) -> f64 {
        let pi = std::f64::consts::PI;
        match self {
            TopoShape::Box {
                length,
                width,
                height,
            } => 2.0 * (length * width + length * height + width * height),
            TopoShape::Sphere { radius } => 4.0 * pi * radius * radius,
            TopoShape::Cylinder { radius, height } => {
                // Mantle + both caps.
                2.0 * pi * radius * height + 2.0 * pi * radius * radius
            }
            TopoShape::Cone {
                radius1,
                radius2,
                height,
            } => {
                // Lateral frustum surface + the base disc (when r1 > 0)
                // + the top disc (only when r2 > 0).
                let slant = (height * height + (radius1 - radius2) * (radius1 - radius2)).sqrt();
                let mut area = pi * (radius1 + radius2) * slant;
                if *radius1 > 0.0 {
                    area += pi * radius1 * radius1;
                }
                if *radius2 > 0.0 {
                    area += pi * radius2 * radius2;
                }
                area
            }
            TopoShape::Torus { radius1, radius2 } => {
                // A = 4*pi^2 * R * r.
                4.0 * pi * pi * radius1 * radius2
            }
            TopoShape::Brep(brep) => brep_area(brep).unwrap_or(f64::NAN),
            TopoShape::Mesh(mesh) => mesh.surface_area(),
        }
    }

    /// The exact volume, or the refusing geometry named — the honest
    /// channel behind [`TopoShape::volume`]. Infallible for the
    /// analytic five and meshes (always `Ok`).
    pub fn try_volume(&self) -> Result<f64, String> {
        match self {
            TopoShape::Brep(brep) => brep_volume(brep),
            other => Ok(other.volume()),
        }
    }

    /// The exact surface area, or the refusing geometry named — the
    /// honest channel behind [`TopoShape::surface_area`].
    pub fn try_surface_area(&self) -> Result<f64, String> {
        match self {
            TopoShape::Brep(brep) => brep_area(brep),
            other => Ok(other.surface_area()),
        }
    }

    /// Tessellate. `deflection` controls chordal deviation: smaller is
    /// finer. For `Mesh` variants the mesh is returned as-is.
    ///
    /// Non-positive or non-finite `deflection` means "finest" and clamps
    /// to the per-primitive segment maximum. All analytic primitives produce
    /// watertight meshes with vertices shared through the index buffer and
    /// triangles wound CCW viewed from outside.
    ///
    /// `Brep`: the exact tessellation ([`crate::tessellate`]) under the
    /// same deflection discipline, or — when the tessellator refuses
    /// (a B-spline face; a non-positive deflection, which the adaptive
    /// recursion has no segment-count clamp to interpret as
    /// "finest") — the **empty mesh**: visibly nothing, never a
    /// wrong solid. The reason rides [`TopoShape::try_tessellate`].
    pub fn tessellate(&self, deflection: f64) -> Mesh {
        match self {
            TopoShape::Box {
                length,
                width,
                height,
            } => tessellate_box(*length, *width, *height),
            TopoShape::Sphere { radius } => tessellate_sphere(*radius, deflection),
            TopoShape::Cylinder { radius, height } => {
                tessellate_cylinder(*radius, *height, deflection)
            }
            TopoShape::Cone {
                radius1,
                radius2,
                height,
            } => tessellate_cone(*radius1, *radius2, *height, deflection),
            TopoShape::Torus { radius1, radius2 } => {
                tessellate_torus(*radius1, *radius2, deflection)
            }
            TopoShape::Brep(brep) => tessellate_brep(brep, deflection).unwrap_or_default(),
            TopoShape::Mesh(mesh) => mesh.clone(),
        }
    }

    /// The tessellation, or the refusing geometry named — the honest
    /// channel behind [`TopoShape::tessellate`]. Infallible for the
    /// analytic five and meshes.
    pub fn try_tessellate(&self, deflection: f64) -> Result<Mesh, String> {
        match self {
            TopoShape::Brep(brep) => tessellate_brep(brep, deflection),
            other => Ok(other.tessellate(deflection)),
        }
    }
}

// ---------------------------------------------------------------------------
// Tessellation internals
// ---------------------------------------------------------------------------

/// The bounding box of an exact B-rep — see [`TopoShape::bbox`] for the
/// documented contract. The tessellable path: the mesh box (which
/// contains only on-surface points, so it sits inside the true box)
/// grown by the deflection (which pulls it back outside the true box
/// — an outer bound). The refused path: vertices ∪ B-spline control
/// nets — poles bound their patch by the hull property; plain vertices
/// bound the model's vertex table exactly but may under-bound curved
/// analytic faces of a mixed payload.
fn brep_bbox(brep: &ExactBRep) -> BoundBox3 {
    const SLACK: f64 = DEFAULT_DEFLECTION;
    if let Ok(mesh) = tessellate_brep(brep, DEFAULT_DEFLECTION) {
        let Some(mut box3) = mesh.bbox() else {
            return BoundBox3::default();
        };
        let s = Vector3::new(SLACK, SLACK, SLACK);
        box3.min = box3.min - s;
        box3.max = box3.max + s;
        return box3;
    }
    let mut points: Vec<Vector3> = brep.vertices.iter().map(|v| v.point).collect();
    for face in &brep.faces {
        if let Surface::Bspline { points: net, .. } = &face.surface {
            for row in net {
                points.extend(row.iter().copied());
            }
        }
    }
    BoundBox3::from_points(&points).unwrap_or_default()
}

/// Segment count over an arc of `span` radians for chordal deviation
/// `deflection` on a circle of `radius`, using the kernel convention
/// `step = 2 * asin(min(0.9, deflection / radius))`, clamped to `[lo, hi]`.
///
/// Non-positive/non-finite `deflection` (or a non-positive radius) requests
/// the finest mesh and clamps to `hi`. The cast is saturating, so the
/// infinity case lands on the clamp, never on UB.
fn arc_segments(deflection: f64, radius: f64, span: f64, lo: usize, hi: usize) -> usize {
    let raw = if deflection.is_finite() && deflection > 0.0 && radius > 0.0 {
        let ratio = (deflection / radius).clamp(f64::MIN_POSITIVE, 0.9);
        span / (2.0 * ratio.asin())
    } else {
        f64::INFINITY
    };
    (raw.ceil() as usize).clamp(lo, hi)
}

/// Box: 8 shared vertices, 12 triangles, CCW from outside.
///
/// Vertex layout — FreeCAD's corner-at-origin convention: `v0..v3` the
/// bottom ring (z = 0) counter-clockwise in XY viewed from +Z, `v4..v7`
/// the same points at z = height; the box spans `[0, l] x [0, w] x [0, h]`.
fn tessellate_box(length: f64, width: f64, height: f64) -> Mesh {
    let vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),         // 0 bottom
        Vector3::new(length, 0.0, 0.0),      // 1
        Vector3::new(length, width, 0.0),    // 2
        Vector3::new(0.0, width, 0.0),       // 3
        Vector3::new(0.0, 0.0, height),      // 4 top
        Vector3::new(length, 0.0, height),   // 5
        Vector3::new(length, width, height), // 6
        Vector3::new(0.0, width, height),    // 7
    ];
    let triangles = vec![
        // Bottom (outward -Z): (0,3,2), (0,2,1)
        [0, 3, 2],
        [0, 2, 1],
        // Top (outward +Z): (4,5,6), (4,6,7)
        [4, 5, 6],
        [4, 6, 7],
        // Front y = 0 (outward -Y): (0,1,5), (0,5,4)
        [0, 1, 5],
        [0, 5, 4],
        // Back y = w (outward +Y): (3,7,6), (3,6,2)
        [3, 7, 6],
        [3, 6, 2],
        // Left x = 0 (outward -X): (0,4,7), (0,7,3)
        [0, 4, 7],
        [0, 7, 3],
        // Right x = l (outward +X): (1,2,6), (1,6,5)
        [1, 2, 6],
        [1, 6, 5],
    ];
    Mesh::new(vertices, triangles)
}

/// Sphere UV grid: `stacks` bands over the colatitude (pole to pole) with
/// `slices = 2 * stacks` columns; pole fans close the surface without
/// degenerate triangles.
fn tessellate_sphere(radius: f64, deflection: f64) -> Mesh {
    let pi = std::f64::consts::PI;
    // Kernel convention (spec): n = ceil(pi / (2*asin(min(0.9, d/r)))) in [8, 256].
    let stacks = arc_segments(deflection, radius, pi, 8, 256);
    let slices = 2 * stacks;

    let mut vertices = Vec::with_capacity(2 + (stacks - 1) * slices);
    let north = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, radius));
    for j in 1..stacks {
        let v = pi * j as f64 / stacks as f64;
        let (sv, cv) = v.sin_cos();
        for i in 0..slices {
            let u = 2.0 * pi * i as f64 / slices as f64;
            let (su, cu) = u.sin_cos();
            vertices.push(Vector3::new(
                radius * sv * cu,
                radius * sv * su,
                radius * cv,
            ));
        }
    }
    let south = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, -radius));

    // Ring j (1..stacks), column i -> vertex index.
    let ring = |j: usize, i: usize| -> u32 { (1 + (j - 1) * slices + i) as u32 };

    let mut triangles = Vec::with_capacity(2 * slices * (stacks - 1));
    for i in 0..slices {
        let i1 = (i + 1) % slices;
        // North pole fan: CCW seen from +Z (outside).
        triangles.push([north, ring(1, i), ring(1, i1)]);
        // South pole fan: reversed winding (outside is -Z there).
        triangles.push([south, ring(stacks - 1, i1), ring(stacks - 1, i)]);
    }
    for j in 1..stacks - 1 {
        for i in 0..slices {
            let i1 = (i + 1) % slices;
            let (a, b) = (ring(j, i), ring(j, i1));
            let (c, d) = (ring(j + 1, i1), ring(j + 1, i));
            // Quad (a, b, c, d) split so both normals point outward.
            triangles.push([a, d, c]);
            triangles.push([a, c, b]);
        }
    }
    Mesh::new(vertices, triangles)
}

/// Cylinder: side quad grid + two triangle-fan caps, all sharing the ring
/// vertices (watertight). `z ∈ [0, h]` — base on the XY plane, like
/// FreeCAD's `Part::Cylinder`.
fn tessellate_cylinder(radius: f64, height: f64, deflection: f64) -> Mesh {
    let tau = 2.0 * std::f64::consts::PI;
    // Same angular step as the sphere stacks, over a full turn.
    let n = arc_segments(deflection, radius, tau, 8, 256);
    let z_top = height;

    let mut vertices = Vec::with_capacity(2 * n + 2);
    for k in 0..n {
        let (s, c) = (tau * k as f64 / n as f64).sin_cos();
        vertices.push(Vector3::new(radius * c, radius * s, 0.0)); // bottom ring
    }
    for k in 0..n {
        let (s, c) = (tau * k as f64 / n as f64).sin_cos();
        vertices.push(Vector3::new(radius * c, radius * s, z_top)); // top ring
    }
    let bottom_center = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, 0.0));
    let top_center = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, z_top));

    let bot = |i: usize| -> u32 { (i % n) as u32 };
    let top = |i: usize| -> u32 { (n + i % n) as u32 };

    let mut triangles = Vec::with_capacity(4 * n);
    for i in 0..n {
        let i1 = (i + 1) % n;
        // Side quad (bot_i, bot_i1, top_i1, top_i): outward normals.
        triangles.push([bot(i), bot(i1), top(i)]); // (A, B, D)
        triangles.push([bot(i1), top(i1), top(i)]); // (B, C, D)
                                                    // Bottom cap: viewed from outside (-Z) the fan runs backwards.
        triangles.push([bottom_center, bot(i1), bot(i)]);
        // Top cap: fan runs forwards viewed from +Z.
        triangles.push([top_center, top(i), top(i1)]);
    }
    Mesh::new(vertices, triangles)
}

/// Cone frustum: base ring (`radius1` at `z = 0`), top ring (`radius2`
/// at `z = height`) when `radius2 > 0`, otherwise a single shared apex
/// — FreeCAD's `Part::Cone` base-on-XY convention.
/// Base fan always present (degenerate-but-harmless when `radius1 == 0`);
/// top fan only when `radius2 > 0`.
fn tessellate_cone(radius1: f64, radius2: f64, height: f64, deflection: f64) -> Mesh {
    let tau = 2.0 * std::f64::consts::PI;
    let n = arc_segments(deflection, radius1.max(radius2), tau, 8, 256);
    let has_top = radius2 > 0.0;

    let mut vertices = Vec::with_capacity(2 * n + 2);
    for k in 0..n {
        let (s, c) = (tau * k as f64 / n as f64).sin_cos();
        vertices.push(Vector3::new(radius1 * c, radius1 * s, 0.0)); // base ring
    }
    let top_ring_base = vertices.len();
    if has_top {
        for k in 0..n {
            let (s, c) = (tau * k as f64 / n as f64).sin_cos();
            vertices.push(Vector3::new(radius2 * c, radius2 * s, height)); // top ring
        }
    } else {
        vertices.push(Vector3::new(0.0, 0.0, height)); // shared apex
    }
    let base_center = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, 0.0));
    // Only the truncated cone needs a top disc center; the plain cone
    // closes at the shared apex, so an extra `top_center` would be an
    // orphan no triangle references.
    let top_center = has_top.then(|| {
        let idx = vertices.len() as u32;
        vertices.push(Vector3::new(0.0, 0.0, height));
        idx
    });

    let bot = |i: usize| -> u32 { (i % n) as u32 };
    let top = |i: usize| -> u32 { (top_ring_base + i % n) as u32 };
    let apex = top(0);

    let mut triangles = Vec::with_capacity(4 * n);
    for i in 0..n {
        let i1 = (i + 1) % n;
        // Base fan (outward -Z).
        triangles.push([base_center, bot(i1), bot(i)]);
        if has_top {
            // Side quads, same winding as the cylinder mantle.
            triangles.push([bot(i), bot(i1), top(i)]); // (A, B, D)
            triangles.push([bot(i1), top(i1), top(i)]); // (B, C, D)
                                                        // Top fan (outward +Z).
            triangles.push([top_center.expect("top disc"), top(i), top(i1)]);
        } else {
            // Side collapses to triangles sharing the single apex vertex.
            triangles.push([bot(i), bot(i1), apex]);
        }
    }
    Mesh::new(vertices, triangles)
}

/// Torus UV grid: `minor` segments around the tube (`radius2`), `major =
/// 2 * minor` around the ring (`radius1`), in the XY plane.
fn tessellate_torus(radius1: f64, radius2: f64, deflection: f64) -> Mesh {
    let tau = 2.0 * std::f64::consts::PI;
    let minor = arc_segments(deflection, radius2, tau, 8, 256);
    let major = 2 * minor;

    let mut vertices = Vec::with_capacity(major * minor);
    for i in 0..major {
        let (su, cu) = (tau * i as f64 / major as f64).sin_cos();
        for j in 0..minor {
            let (sv, cv) = (tau * j as f64 / minor as f64).sin_cos();
            vertices.push(Vector3::new(
                (radius1 + radius2 * cv) * cu,
                (radius1 + radius2 * cv) * su,
                radius2 * sv,
            ));
        }
    }
    let idx = |i: usize, j: usize| -> u32 { (i % major * minor + j % minor) as u32 };

    let mut triangles = Vec::with_capacity(2 * major * minor);
    for i in 0..major {
        for j in 0..minor {
            let (a, b) = (idx(i, j), idx(i + 1, j));
            let (c, d) = (idx(i + 1, j + 1), idx(i, j + 1));
            // dU x dV points outward on the tube, so (A,B,C) / (A,C,D)
            // are CCW seen from outside.
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    Mesh::new(vertices, triangles)
}

/// Indexed triangle mesh.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub vertices: Vec<Vector3>,
    /// Ccw winding when viewed from outside.
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn new(vertices: Vec<Vector3>, triangles: Vec<[u32; 3]>) -> Self {
        Self {
            vertices,
            triangles,
        }
    }

    /// Structural validity: finite vertices, triangle indices in range.
    /// (Watertightness is a property the analytic tessellators guarantee by
    /// construction; it is not re-checked here.)
    pub fn validate(&self) -> Result<(), String> {
        for (i, v) in self.vertices.iter().enumerate() {
            if !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite() {
                return Err(format!("vertex {i} has non-finite coordinates"));
            }
        }
        let count = self.vertices.len();
        for (i, t) in self.triangles.iter().enumerate() {
            for &v in t {
                if v as usize >= count {
                    return Err(format!(
                        "triangle {i} references vertex {v} but the mesh has {count} vertices"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Signed volume: sum of tetrahedron volumes against the origin,
    /// `dot(v0, cross(v1, v2)) / 6`. Correct for closed, consistently
    /// wound meshes; a leaky mesh reports garbage, as it should.
    pub fn volume(&self) -> f64 {
        let mut total = 0.0;
        for t in &self.triangles {
            let (v0, v1, v2) = (
                &self.vertices[t[0] as usize],
                &self.vertices[t[1] as usize],
                &self.vertices[t[2] as usize],
            );
            total += v0.dot(&v1.cross(v2)) / 6.0;
        }
        total
    }

    /// Sum of triangle areas.
    pub fn surface_area(&self) -> f64 {
        let mut total = 0.0;
        for t in &self.triangles {
            let (v0, v1, v2) = (
                &self.vertices[t[0] as usize],
                &self.vertices[t[1] as usize],
                &self.vertices[t[2] as usize],
            );
            total += (*v1 - *v0).cross(&(*v2 - *v0)).length() / 2.0;
        }
        total
    }

    /// In-place transform by `m` (points, not directions — translation
    /// applies).
    pub fn transform(&mut self, m: &Matrix4) {
        for v in &mut self.vertices {
            *v = m.transform_point(v);
        }
    }

    pub fn transformed(&self, m: &Matrix4) -> Mesh {
        let mut mesh = self.clone();
        mesh.transform(m);
        mesh
    }

    /// Append `other`'s geometry with index offsets.
    pub fn merge(&mut self, other: &Mesh) {
        let offset = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.triangles.extend(
            other
                .triangles
                .iter()
                .map(|t| [t[0] + offset, t[1] + offset, t[2] + offset]),
        );
    }

    pub fn bbox(&self) -> Option<BoundBox3> {
        BoundBox3::from_points(&self.vertices)
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
}
