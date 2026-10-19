//! Parametric primitives and triangle meshes.
//!
//! # Conventions (binding for this kernel)
//!
//! * **Box** — FreeCAD `Part::Box`: centered in XY, sitting on the XY plane:
//!   `x ∈ [-length/2, length/2]`, `y ∈ [-width/2, width/2]`, `z ∈ [0, height]`.
//! * **Sphere** — centered on the origin.
//! * **Cylinder** — along Z, `z ∈ [-height/2, height/2]` (FreeCAD convention).
//! * **Cone** — frustum with `radius1` at the **base** (`z = -height/2`) and
//!   `radius2` at the **top** (`z = +height/2`); `radius2 == 0` is a plain
//!   cone with its apex up.
//! * **Torus** — in the XY plane, centered on the origin; `radius1` = ring
//!   radius, `radius2` = tube radius. Self-intersecting rings
//!   (`radius2 > radius1`) are allowed, like FreeCAD's.
//! * **Meshes** produced by [`TopoShape::tessellate`] are watertight for the
//!   analytic primitives (vertices shared through the index buffer) and wound
//!   CCW when viewed from outside (right-hand rule normals point outward).

use fc_base::{BoundBox3, Matrix4, Vector3};

/// Tessellation quality used when a caller doesn't care
/// (max chordal deviation, model units).
pub const DEFAULT_DEFLECTION: f64 = 0.2;

/// An analytic primitive or a tessellated mesh — the `TopoShape` of this
/// kernel (FreeCAD/OCCT `TopoDS_Shape`, distilled to its honest core).
#[derive(Debug, Clone, PartialEq)]
pub enum TopoShape {
    /// Axis-aligned box, centered on the origin like FreeCAD `Part::Box`:
    /// occupies `[-length/2, length/2] x [-width/2, width/2] x
    /// [0, height]`.
    Box {
        length: f64,
        width: f64,
        height: f64,
    },
    Sphere {
        radius: f64,
    },
    /// Along +Z: `[-height/2, height/2]` like FreeCAD `Part::Cylinder`.
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
    Mesh(Mesh),
}

impl TopoShape {
    /// `"Box"`, `"Sphere"`, `"Cylinder"`, `"Cone"`, `"Torus"`, `"Mesh"`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            TopoShape::Box { .. } => "Box",
            TopoShape::Sphere { .. } => "Sphere",
            TopoShape::Cylinder { .. } => "Cylinder",
            TopoShape::Cone { .. } => "Cone",
            TopoShape::Torus { .. } => "Torus",
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
            TopoShape::Mesh(mesh) => mesh.validate(),
        }
    }

    /// Exact analytic bounding box. A `Mesh` variant with no vertices yields
    /// the empty (default) box.
    pub fn bbox(&self) -> BoundBox3 {
        match self {
            TopoShape::Box {
                length,
                width,
                height,
            } => BoundBox3::new(
                Vector3::new(-length / 2.0, -width / 2.0, 0.0),
                Vector3::new(length / 2.0, width / 2.0, *height),
            ),
            TopoShape::Sphere { radius } => BoundBox3::new(
                Vector3::new(-radius, -radius, -radius),
                Vector3::new(*radius, *radius, *radius),
            ),
            TopoShape::Cylinder { radius, height } => BoundBox3::new(
                Vector3::new(-radius, -radius, -height / 2.0),
                Vector3::new(*radius, *radius, *height / 2.0),
            ),
            TopoShape::Cone {
                radius1,
                radius2,
                height,
            } => {
                // The widest ring bounds the horizontal extent.
                let r = radius1.max(*radius2);
                BoundBox3::new(
                    Vector3::new(-r, -r, -height / 2.0),
                    Vector3::new(r, r, *height / 2.0),
                )
            }
            TopoShape::Torus { radius1, radius2 } => {
                let outer = radius1 + radius2;
                BoundBox3::new(
                    Vector3::new(-outer, -outer, -radius2),
                    Vector3::new(outer, outer, *radius2),
                )
            }
            TopoShape::Mesh(mesh) => mesh.bbox().unwrap_or_default(),
        }
    }

    /// Exact analytic volume (mesh variant: sum of signed tetrahedra
    /// against the origin).
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
            TopoShape::Mesh(mesh) => mesh.volume(),
        }
    }

    /// Exact analytic surface area (mesh variant: sum of triangle areas).
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
            TopoShape::Mesh(mesh) => mesh.surface_area(),
        }
    }

    /// Tessellate. `deflection` controls chordal deviation: smaller is
    /// finer. For `Mesh` variants the mesh is returned as-is.
    ///
    /// Non-positive or non-finite `deflection` means "finest" and clamps to
    /// the per-primitive segment maximum. All analytic primitives produce
    /// watertight meshes with vertices shared through the index buffer and
    /// triangles wound CCW viewed from outside.
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
            TopoShape::Mesh(mesh) => mesh.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tessellation internals
// ---------------------------------------------------------------------------

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
/// Vertex layout: `v0..v3` bottom ring (z = 0) counter-clockwise in XY
/// viewed from +Z, `v4..v7` the same points at z = height.
fn tessellate_box(length: f64, width: f64, height: f64) -> Mesh {
    let (hl, hw) = (length / 2.0, width / 2.0);
    let vertices = vec![
        Vector3::new(-hl, -hw, 0.0),    // 0 bottom
        Vector3::new(hl, -hw, 0.0),     // 1
        Vector3::new(hl, hw, 0.0),      // 2
        Vector3::new(-hl, hw, 0.0),     // 3
        Vector3::new(-hl, -hw, height), // 4 top
        Vector3::new(hl, -hw, height),  // 5
        Vector3::new(hl, hw, height),   // 6
        Vector3::new(-hl, hw, height),  // 7
    ];
    let triangles = vec![
        // Bottom (outward -Z): (0,3,2), (0,2,1)
        [0, 3, 2],
        [0, 2, 1],
        // Top (outward +Z): (4,5,6), (4,6,7)
        [4, 5, 6],
        [4, 6, 7],
        // Front y = -w/2 (outward -Y): (0,1,5), (0,5,4)
        [0, 1, 5],
        [0, 5, 4],
        // Back y = +w/2 (outward +Y): (3,7,6), (3,6,2)
        [3, 7, 6],
        [3, 6, 2],
        // Left x = -l/2 (outward -X): (0,4,7), (0,7,3)
        [0, 4, 7],
        [0, 7, 3],
        // Right x = +l/2 (outward +X): (1,2,6), (1,6,5)
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
/// vertices (watertight). `z ∈ [-h/2, h/2]`.
fn tessellate_cylinder(radius: f64, height: f64, deflection: f64) -> Mesh {
    let tau = 2.0 * std::f64::consts::PI;
    // Same angular step as the sphere stacks, over a full turn.
    let n = arc_segments(deflection, radius, tau, 8, 256);
    let half = height / 2.0;

    let mut vertices = Vec::with_capacity(2 * n + 2);
    for k in 0..n {
        let (s, c) = (tau * k as f64 / n as f64).sin_cos();
        vertices.push(Vector3::new(radius * c, radius * s, -half)); // bottom ring
    }
    for k in 0..n {
        let (s, c) = (tau * k as f64 / n as f64).sin_cos();
        vertices.push(Vector3::new(radius * c, radius * s, half)); // top ring
    }
    let bottom_center = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, -half));
    let top_center = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, half));

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

/// Cone frustum: base ring (`radius1` at `z = -h/2`), top ring (`radius2`
/// at `z = +h/2`) when `radius2 > 0`, otherwise a single shared apex.
/// Base fan always present (degenerate-but-harmless when `radius1 == 0`);
/// top fan only when `radius2 > 0`.
fn tessellate_cone(radius1: f64, radius2: f64, height: f64, deflection: f64) -> Mesh {
    let tau = 2.0 * std::f64::consts::PI;
    let n = arc_segments(deflection, radius1.max(radius2), tau, 8, 256);
    let half = height / 2.0;
    let has_top = radius2 > 0.0;

    let mut vertices = Vec::with_capacity(2 * n + 2);
    for k in 0..n {
        let (s, c) = (tau * k as f64 / n as f64).sin_cos();
        vertices.push(Vector3::new(radius1 * c, radius1 * s, -half)); // base ring
    }
    let top_ring_base = vertices.len();
    if has_top {
        for k in 0..n {
            let (s, c) = (tau * k as f64 / n as f64).sin_cos();
            vertices.push(Vector3::new(radius2 * c, radius2 * s, half)); // top ring
        }
    } else {
        vertices.push(Vector3::new(0.0, 0.0, half)); // shared apex
    }
    let base_center = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, -half));
    let top_center = vertices.len() as u32;
    vertices.push(Vector3::new(0.0, 0.0, half));

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
            triangles.push([top_center, top(i), top(i1)]);
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
