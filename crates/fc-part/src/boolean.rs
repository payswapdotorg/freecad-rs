//! Mesh boolean operations: difference, union and intersection on
//! tessellated triangle meshes — the `Part::Cut`, `Part::Fuse` and
//! `Part::Common` of this kernel, without OCCT.
//!
//! FreeCAD hands its boolean features to OpenCascade's `BRepAlgoAPI`
//! kernel; this kernel runs the operations directly on [`Mesh`]es with
//! the classic BSP-tree constructive-solid-geometry algorithm (the
//! public-domain `csg.js` lineage, reimplemented from the algorithm
//! description):
//!
//! 1. Build a binary space partitioning tree per operand; the plane of the
//!    first polygon arriving at a node becomes that node's splitting
//!    plane.
//! 2. Split every polygon against the planes it meets, classifying
//!    vertices with [`PLANE_EPS`]; a spanning polygon is cut into a front
//!    piece and a back piece (Sutherland–Hodgman clipping).
//! 3. Combine the trees with the canonical invert / clip-to / build
//!    construction for union, difference and intersection: clip each
//!    boundary to the relevant side of the other solid, flip windings
//!    where a surviving piece ends up facing the other way, and join the
//!    pieces.
//!
//! Output pieces are welded back into an indexed [`Mesh`]. Crossing
//! points are always computed from a canonicalized segment direction
//! (lexicographically smaller endpoint first), so every occurrence of
//! the same geometric crossing produces bit-identical vertices —
//! booleaning the watertight analytic primitives yields watertight
//! results (see [`mesh_is_closed`]). One more pass is needed for that:
//! the BSP clips neighboring polygons against different plane subsets,
//! so a seam can meet a long edge at a vertex the edge itself never
//! received — the classic CSG T-junction. [`stitch_t_junctions`]
//! re-splits every such edge at the exact `Vector3` the neighbor owns,
//! which is what actually closes the last seams (bit-exact welds).
//!
//! # Determinism
//!
//! Trees are built in input order, no hash container is iterated, and
//! welding goes through a `BTreeMap`: the same operands always produce
//! the same output mesh, bit for bit.
//!
//! # Contract and limitations
//!
//! * Operands are expected to be closed solids wound CCW seen from
//!   outside — exactly what
//!   [`TopoShape::tessellate`](crate::shape::TopoShape::tessellate)
//!   produces. Open, self-intersecting or non-finite meshes give
//!   unspecified but panic-free results.
//! * Degenerate triangles (area below `PLANE_EPS²`) are pruned on input.
//!   An operand left with fewer than 4 triangles cannot bound a solid;
//!   the infallible functions treat it as empty (`cut(a, ∅) = a`,
//!   `fuse(a, ∅) = a`, `common(a, ∅) = ∅`) while the `try_*` functions
//!   report [`BooleanError::DegenerateInput`]. An empty *result*
//!   (disjoint intersection, a cut that removed everything, ...) is
//!   returned as an empty mesh by the infallible functions and reported
//!   as [`BooleanError::EmptyResult`] by the `try_*` functions.
//! * Exactly coincident coplanar faces are the algorithm's weak spot.
//!   Opposed coincident walls (the usual "two solids sharing a face")
//!   are cleaned up, and identical operands are short-circuited
//!   exactly, but same-normal coincident faces of *different* solids
//!   survive as doubly covered sheets — the volume stays correct and
//!   edge counts stay closed, but the mesh is not a clean manifold
//!   there. Offset operands by a hair when a pristine result matters.

use std::collections::BTreeMap;
use std::fmt;
use std::mem;

use crate::shape::Mesh;
use fc_base::Vector3;

/// Vertex classification tolerance against a splitting plane, in model
/// units — the value the `csg.js` lineage uses.
pub const PLANE_EPS: f64 = 1e-5;

/// How close a projection parameter must be to an edge's endpoints for
/// the point to count as that endpoint rather than a T-vertex.
const STITCH_END_EPS: f64 = 1e-9;

/// Input triangles with area below this are pruned before the trees are
/// built (they carry no usable plane).
const DEGENERATE_AREA: f64 = PLANE_EPS * PLANE_EPS;

/// Fewest triangles that can bound a solid: a tetrahedron.
const MIN_SOLID_TRIANGLES: usize = 4;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failure modes of the checked boolean API
/// ([`try_fuse`], [`try_cut`], [`try_common`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanError {
    /// An operand cannot bound a solid: after pruning degenerate
    /// triangles, fewer than 4 remain.
    DegenerateInput {
        /// `"a"` or `"b"` — which operand was degenerate.
        operand: &'static str,
        /// How many non-degenerate triangles the operand had left.
        triangles: usize,
    },
    /// The operation produced no triangles (disjoint intersection, a cut
    /// that removed everything, ...).
    EmptyResult,
}

impl fmt::Display for BooleanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            BooleanError::DegenerateInput { operand, triangles } => write!(
                f,
                "degenerate boolean operand '{operand}': only {triangles} non-degenerate \
                 triangles, a closed solid needs at least {MIN_SOLID_TRIANGLES}"
            ),
            BooleanError::EmptyResult => {
                write!(
                    f,
                    "boolean operation produced an empty result: no triangles remain"
                )
            }
        }
    }
}

impl std::error::Error for BooleanError {}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Union (`Part::Fuse`): the boundary of the solid `a ∪ b`.
///
/// Operands with fewer than 4 non-degenerate triangles are treated as
/// empty, so `fuse(a, ∅) = a`; the union of two solid operands is never
/// empty. See the [module documentation](self) for the full contract.
pub fn fuse(a: &Mesh, b: &Mesh) -> Mesh {
    boolean(a, b, Op::Union)
}

/// Difference (`Part::Cut`): the boundary of the solid `a ∖ b`.
///
/// Operands with fewer than 4 non-degenerate triangles are treated as
/// empty, so `cut(a, ∅) = a` and `cut(∅, b) = ∅`. When `b` swallows `a`
/// entirely the result is the empty mesh.
pub fn cut(a: &Mesh, b: &Mesh) -> Mesh {
    boolean(a, b, Op::Subtract)
}

/// Intersection (`Part::Common`): the boundary of the solid `a ∩ b`.
///
/// Disjoint operands produce the empty mesh. Operands with fewer than
/// 4 non-degenerate triangles are treated as empty, so
/// `common(a, ∅) = ∅`. Identical operands are handled exactly (the
/// result is the operand itself).
pub fn common(a: &Mesh, b: &Mesh) -> Mesh {
    boolean(a, b, Op::Intersect)
}

/// Checked [`fuse`]: reports [`BooleanError::DegenerateInput`] for
/// non-solid operands and [`BooleanError::EmptyResult`] when nothing
/// survives.
pub fn try_fuse(a: &Mesh, b: &Mesh) -> Result<Mesh, BooleanError> {
    try_boolean(a, b, Op::Union)
}

/// Checked [`cut`]: reports [`BooleanError::DegenerateInput`] for
/// non-solid operands and [`BooleanError::EmptyResult`] when nothing
/// survives (for instance when the tool swallows the target).
pub fn try_cut(a: &Mesh, b: &Mesh) -> Result<Mesh, BooleanError> {
    try_boolean(a, b, Op::Subtract)
}

/// Checked [`common`]: reports [`BooleanError::DegenerateInput`] for
/// non-solid operands and [`BooleanError::EmptyResult`] when the
/// operands do not overlap.
pub fn try_common(a: &Mesh, b: &Mesh) -> Result<Mesh, BooleanError> {
    try_boolean(a, b, Op::Intersect)
}

/// Watertight check: every undirected edge is shared by exactly two
/// triangles.
///
/// Returns `false` for the empty mesh (no boundary at all), for
/// triangles with repeated or out-of-range vertex indices, and for any
/// edge whose triangle count differs from 2. This is a purely
/// topological test — it says nothing about self-intersections.
pub fn mesh_is_closed(mesh: &Mesh) -> bool {
    if mesh.triangles.is_empty() {
        return false;
    }
    let vertex_count = mesh.vertices.len() as u32;
    let mut edges: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for triangle in &mesh.triangles {
        let (a, b, c) = (triangle[0], triangle[1], triangle[2]);
        if a == b || b == c || a == c {
            return false;
        }
        if a >= vertex_count || b >= vertex_count || c >= vertex_count {
            return false;
        }
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let edge = if u < v { (u, v) } else { (v, u) };
            *edges.entry(edge).or_insert(0) += 1;
        }
    }
    edges.values().all(|count| *count == 2)
}

// ---------------------------------------------------------------------------
// Construction driver
// ---------------------------------------------------------------------------

/// The three boolean constructions, named after FreeCAD's features.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Union,
    Subtract,
    Intersect,
}

fn boolean(a: &Mesh, b: &Mesh, op: Op) -> Mesh {
    boolean_prepared(a, polygons_of(a), polygons_of(b), op)
}

fn try_boolean(a: &Mesh, b: &Mesh, op: Op) -> Result<Mesh, BooleanError> {
    let polygons_a = polygons_of(a);
    if polygons_a.len() < MIN_SOLID_TRIANGLES {
        return Err(BooleanError::DegenerateInput {
            operand: "a",
            triangles: polygons_a.len(),
        });
    }
    let polygons_b = polygons_of(b);
    if polygons_b.len() < MIN_SOLID_TRIANGLES {
        return Err(BooleanError::DegenerateInput {
            operand: "b",
            triangles: polygons_b.len(),
        });
    }
    let result = boolean_prepared(a, polygons_a, polygons_b, op);
    if result.triangles.is_empty() {
        Err(BooleanError::EmptyResult)
    } else {
        Ok(result)
    }
}

fn boolean_prepared(a: &Mesh, polygons_a: Vec<Polygon>, polygons_b: Vec<Polygon>, op: Op) -> Mesh {
    // Identical operands: union and intersection are the operand itself,
    // difference is empty. The general construction handles union and
    // difference too, but the shortcut also makes `common(x, x)` exact —
    // coincident faces are exactly the case the BSP clips get wrong.
    if polygons_a.len() >= MIN_SOLID_TRIANGLES && polygons_a == polygons_b {
        return match op {
            Op::Union | Op::Intersect => a.clone(),
            Op::Subtract => Mesh::default(),
        };
    }
    mesh_from(stitch_t_junctions(op_result(polygons_a, polygons_b, op)))
}

/// The canonical BSP construction for each operation.
fn op_result(polygons_a: Vec<Polygon>, polygons_b: Vec<Polygon>, op: Op) -> Vec<Polygon> {
    let a_solid = polygons_a.len() >= MIN_SOLID_TRIANGLES;
    let b_solid = polygons_b.len() >= MIN_SOLID_TRIANGLES;
    if !a_solid || !b_solid {
        // A non-solid operand is the empty set.
        return match (op, a_solid, b_solid) {
            (_, false, false) => Vec::new(),
            (Op::Union, true, _) => polygons_a,
            (Op::Union, _, true) => polygons_b,
            (Op::Subtract, true, _) => polygons_a,
            _ => Vec::new(),
        };
    }
    let mut a = Node::default();
    a.build(polygons_a);
    let mut b = Node::default();
    b.build(polygons_b);
    match op {
        Op::Union => {
            // Keep the parts of ∂A outside B and of ∂B outside A; the
            // third clip removes B's pieces lying on A's removed
            // (coincident) faces.
            a.clip_to(&b);
            b.clip_to(&a);
            b.invert();
            b.clip_to(&a);
            b.invert();
            let b_pieces = b.all_polygons();
            a.build(b_pieces);
            a.all_polygons()
        }
        Op::Subtract => {
            a.invert();
            a.clip_to(&b);
            b.clip_to(&a);
            b.invert();
            b.clip_to(&a);
            b.invert();
            let b_pieces = b.all_polygons();
            a.build(b_pieces);
            a.invert();
            a.all_polygons()
        }
        Op::Intersect => {
            a.invert();
            b.clip_to(&a);
            b.invert();
            a.clip_to(&b);
            let b_pieces = b.all_polygons();
            a.build(b_pieces);
            a.invert();
            a.all_polygons()
        }
    }
}

// ---------------------------------------------------------------------------
// Mesh <-> polygon plumbing
// ---------------------------------------------------------------------------

/// Convert a mesh into clipper polygons, pruning degenerate triangles.
fn polygons_of(mesh: &Mesh) -> Vec<Polygon> {
    mesh.triangles
        .iter()
        .filter_map(|triangle| {
            let a = *mesh.vertices.get(triangle[0] as usize)?;
            let b = *mesh.vertices.get(triangle[1] as usize)?;
            let c = *mesh.vertices.get(triangle[2] as usize)?;
            // Twice the area must clear the degeneracy bar.
            if (b - a).cross(&(c - a)).length() < 2.0 * DEGENERATE_AREA {
                return None;
            }
            Some(Polygon {
                vertices: vec![a, b, c],
            })
        })
        .collect()
}

/// Re-split polygon edges at T-vertices.
///
/// The BSP clips each polygon only against the planes of the subtree it
/// descends into; two neighbors of one original face can therefore end
/// with different vertex sets along their shared seam — one carries a
/// point the other's edge passes through without a vertex there (a
/// T-junction), and the bit-exact weld in [`mesh_from`] cannot pair the
/// mismatched edge lengths. This pass collects every distinct output
/// vertex, then inserts (a copy of) every vertex that lies in the
/// interior of another polygon's edge into that edge, sorted along it.
/// Inserted points are the exact bit patterns the neighbor owns, so the
/// weld closes.
///
/// Candidates are found through a uniform hash grid over the vertices
/// (`points_in_cells`), so an edge only tests the points near its
/// bounding box instead of all of them — the difference between
/// milliseconds and a quadratic scan on tessellated spheres.
fn stitch_t_junctions(polygons: Vec<Polygon>) -> Vec<Polygon> {
    // Every distinct vertex of the output, deterministically ordered.
    let mut points: BTreeMap<VertexKey, Vector3> = BTreeMap::new();
    for polygon in &polygons {
        for vertex in &polygon.vertices {
            points.entry(vertex_key(vertex)).or_insert(*vertex);
        }
    }
    let points: Vec<Vector3> = points.into_values().collect();
    if points.len() < 3 {
        return polygons;
    }

    // Grid: points binned by cell; edges query the cells their bounding
    // box overlaps. Cell size ~ a sixteenth of the model diagonal, so a
    // typical tessellation edge spans one or two cells.
    let mut lo = points[0];
    let mut hi = points[0];
    for p in &points {
        lo = min_point(lo, *p);
        hi = max_point(hi, *p);
    }
    let diag = (hi - lo).length();
    let cell_size = (diag / 16.0).max(PLANE_EPS * 10.0);
    let inv_cell = 1.0 / cell_size;
    let cell_of = |p: &Vector3| {
        [
            (p.x * inv_cell).floor() as i32,
            (p.y * inv_cell).floor() as i32,
            (p.z * inv_cell).floor() as i32,
        ]
    };
    let mut points_in_cells: BTreeMap<[i32; 3], Vec<usize>> = BTreeMap::new();
    for (i, p) in points.iter().enumerate() {
        points_in_cells.entry(cell_of(p)).or_default().push(i);
    }

    polygons
        .into_iter()
        .map(|mut polygon| {
            let n = polygon.vertices.len();
            if n < 3 {
                return polygon;
            }
            let mut stitched: Vec<Vector3> = Vec::with_capacity(n + 2);
            for i in 0..n {
                let a = polygon.vertices[i];
                let b = polygon.vertices[(i + 1) % n];
                stitched.push(a);
                // Points strictly inside the segment `a`-`b`, within
                // [`PLANE_EPS`] of it, gathered from the grid cells the
                // segment's bounding box overlaps.
                let ab = b - a;
                let ab_len_sq = ab.dot(&ab);
                if ab_len_sq <= 0.0 || !ab_len_sq.is_finite() {
                    continue;
                }
                let clo = cell_of(&min_point(a, b));
                let chi = cell_of(&max_point(a, b));
                let mut hits: Vec<(f64, Vector3)> = Vec::new();
                for cx in clo[0]..=chi[0] {
                    for cy in clo[1]..=chi[1] {
                        for cz in clo[2]..=chi[2] {
                            let Some(indices) = points_in_cells.get(&[cx, cy, cz]) else {
                                continue;
                            };
                            for &idx in indices {
                                let v = points[idx];
                                let av = v - a;
                                // Projection parameter along the edge.
                                let t = av.dot(&ab) / ab_len_sq;
                                if t <= STITCH_END_EPS || t >= 1.0 - STITCH_END_EPS {
                                    continue;
                                }
                                // Perpendicular distance to the (infinite)
                                // line.
                                if av.cross(&ab).length() > PLANE_EPS {
                                    continue;
                                }
                                hits.push((t, v));
                            }
                        }
                    }
                }
                hits.sort_by(|x, y| x.0.partial_cmp(&y.0).expect("finite t"));
                for (_, v) in hits {
                    stitched.push(v);
                }
            }
            match new_polygon(stitched) {
                Some(stitched) => polygon = stitched,
                None => {
                    // A stitch that collapses the polygon below three
                    // distinct vertices: keep the original (the
                    // collapse would mean the polygon was degenerate
                    // already).
                }
            }
            polygon
        })
        .collect()
}

/// Component-wise extrema (Vector3 carries no `Ord`).
fn min_point(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn max_point(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

/// Weld polygons back into an indexed mesh, fan-triangulating pieces with
/// more than three vertices. Welding is by exact bit-equal coordinates,
/// which the canonical crossing computation — plus the T-junction
/// stitch above — makes consistent.
fn mesh_from(polygons: Vec<Polygon>) -> Mesh {
    let mut index: BTreeMap<VertexKey, u32> = BTreeMap::new();
    let mut vertices: Vec<Vector3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for polygon in polygons {
        let mut ids: Vec<u32> = Vec::with_capacity(polygon.vertices.len());
        for vertex in &polygon.vertices {
            let key = vertex_key(vertex);
            let next = vertices.len() as u32;
            let id = *index.entry(key).or_insert(next);
            if id == next {
                vertices.push(*vertex);
            }
            if ids.last() != Some(&id) {
                ids.push(id);
            }
        }
        // Collapse the wraparound duplicate, then fan-triangulate.
        if ids.len() > 1 && ids.last() == Some(&ids[0]) {
            ids.pop();
        }
        for k in 1..ids.len().saturating_sub(1) {
            let triangle = [ids[0], ids[k], ids[k + 1]];
            if triangle[0] != triangle[1]
                && triangle[1] != triangle[2]
                && triangle[0] != triangle[2]
            {
                triangles.push(triangle);
            }
        }
    }
    Mesh::new(vertices, triangles)
}

/// Exact-bit vertex identity — the key the weld map uses.
type VertexKey = (u64, u64, u64);

fn vertex_key(v: &Vector3) -> VertexKey {
    (v.x.to_bits(), v.y.to_bits(), v.z.to_bits())
}

fn same_point(a: &Vector3, b: &Vector3) -> bool {
    vertex_key(a) == vertex_key(b)
}

// ---------------------------------------------------------------------------
// BSP tree
// ---------------------------------------------------------------------------

/// Oriented plane `normal · p = w` with a unit normal.
#[derive(Debug, Clone, Copy)]
struct Plane {
    normal: Vector3,
    w: f64,
}

impl Plane {
    /// Plane through three points; `None` when they are collinear.
    fn from_points(a: Vector3, b: Vector3, c: Vector3) -> Option<Plane> {
        let normal = (b - a).cross(&(c - a));
        let length = normal.length();
        if length > 0.0 && length.is_finite() {
            let normal = normal * (1.0 / length);
            Some(Plane {
                w: normal.dot(&a),
                normal,
            })
        } else {
            None
        }
    }

    /// Same plane, complement region: flip the normal.
    fn flip(&mut self) {
        self.normal = -self.normal;
        self.w = -self.w;
    }

    /// Where the segment `a`–`b` crosses the plane. The segment is
    /// canonicalized (lexicographically smaller endpoint first) so an
    /// edge and its reverse produce bit-identical crossing points — this
    /// is what keeps the welded output seam-tight.
    fn split_point(&self, a: Vector3, b: Vector3) -> Vector3 {
        let (lo, hi) = if (a.x, a.y, a.z) < (b.x, b.y, b.z) {
            (a, b)
        } else {
            (b, a)
        };
        let t = (self.w - self.normal.dot(&lo)) / (self.normal.dot(&hi) - self.normal.dot(&lo));
        lo + (hi - lo) * t
    }

    /// Split `polygon` by this plane.
    fn split_polygon(&self, polygon: Polygon) -> Split {
        const COPLANAR: u8 = 0;
        const FRONT: u8 = 1;
        const BACK: u8 = 2;

        let vertices = polygon.vertices;
        let mut types = Vec::with_capacity(vertices.len());
        let mut polygon_type = COPLANAR;
        for vertex in &vertices {
            let t = self.normal.dot(vertex) - self.w;
            let side = if t < -PLANE_EPS {
                BACK
            } else if t > PLANE_EPS {
                FRONT
            } else {
                COPLANAR
            };
            polygon_type |= side;
            types.push(side);
        }
        match polygon_type {
            COPLANAR => {
                // Coplanar polygons follow their normal alignment.
                let aligned =
                    polygon_plane(&vertices).is_some_and(|p| p.normal.dot(&self.normal) > 0.0);
                if aligned {
                    Split::CoplanarFront(Polygon { vertices })
                } else {
                    Split::CoplanarBack(Polygon { vertices })
                }
            }
            FRONT => Split::Front(Polygon { vertices }),
            BACK => Split::Back(Polygon { vertices }),
            _ => {
                // SPANNING: Sutherland–Hodgman clipping into a front and a
                // back piece; coplanar vertices belong to both.
                let n = vertices.len();
                let mut front: Vec<Vector3> = Vec::with_capacity(n + 2);
                let mut back: Vec<Vector3> = Vec::with_capacity(n + 2);
                for i in 0..n {
                    let j = (i + 1) % n;
                    let (ti, tj) = (types[i], types[j]);
                    let vi = vertices[i];
                    if ti != BACK {
                        front.push(vi);
                    }
                    if ti != FRONT {
                        back.push(vi);
                    }
                    if (ti == FRONT && tj == BACK) || (ti == BACK && tj == FRONT) {
                        let crossing = self.split_point(vi, vertices[j]);
                        front.push(crossing);
                        back.push(crossing);
                    }
                }
                Split::Spanning {
                    front: new_polygon(front),
                    back: new_polygon(back),
                }
            }
        }
    }
}

/// How a polygon comes out of a plane split.
#[derive(Debug)]
enum Split {
    /// Coplanar with the plane, normal pointing the same way.
    CoplanarFront(Polygon),
    /// Coplanar, normal pointing the other way.
    CoplanarBack(Polygon),
    /// Entirely on the front side.
    Front(Polygon),
    /// Entirely on the back side.
    Back(Polygon),
    /// Straddles the plane: split into a front and a back piece (either
    /// may have collapsed below three distinct vertices and be absent).
    Spanning {
        front: Option<Polygon>,
        back: Option<Polygon>,
    },
}

/// A clipped piece of a surface: three or more vertices in order, wound
/// like the triangle it came from.
#[derive(Debug, Clone, PartialEq)]
struct Polygon {
    vertices: Vec<Vector3>,
}

impl Polygon {
    fn flip(&mut self) {
        self.vertices.reverse();
    }

    fn plane(&self) -> Option<Plane> {
        polygon_plane(&self.vertices)
    }
}

/// Plane through the first three distinct, non-collinear vertices.
fn polygon_plane(vertices: &[Vector3]) -> Option<Plane> {
    if vertices.len() < 3 {
        return None;
    }
    let a = vertices[0];
    let bi = vertices.iter().position(|v| !same_point(v, &a))?;
    let b = vertices[bi];
    (bi + 1..vertices.len()).find_map(|k| Plane::from_points(a, b, vertices[k]))
}

/// Collapse consecutive duplicate vertices (and the wraparound pair);
/// `None` if fewer than three distinct vertices remain.
fn new_polygon(vertices: Vec<Vector3>) -> Option<Polygon> {
    let mut distinct: Vec<Vector3> = Vec::with_capacity(vertices.len());
    for vertex in vertices {
        match distinct.last() {
            Some(last) if same_point(last, &vertex) => {}
            _ => distinct.push(vertex),
        }
    }
    if distinct.len() > 1 && distinct.last().is_some_and(|l| same_point(l, &distinct[0])) {
        distinct.pop();
    }
    if distinct.len() >= 3 {
        Some(Polygon { vertices: distinct })
    } else {
        None
    }
}

/// A BSP tree node: a splitting plane, the coplanar polygons stored at
/// it, and the two subtrees. `front` is the outside of the solid, `back`
/// the inside.
#[derive(Debug, Default)]
struct Node {
    plane: Option<Plane>,
    polygons: Vec<Polygon>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
}

impl Node {
    /// Insert polygons into the (sub)tree, splitting them as needed.
    fn build(&mut self, polygons: Vec<Polygon>) {
        if polygons.is_empty() {
            return;
        }
        let plane = match self.plane {
            Some(plane) => plane,
            None => match polygons.iter().find_map(Polygon::plane) {
                Some(plane) => {
                    self.plane = Some(plane);
                    plane
                }
                None => {
                    // Nothing here can define a plane; park the pieces.
                    self.polygons.extend(polygons);
                    return;
                }
            },
        };
        let mut front = Vec::with_capacity(polygons.len());
        let mut back = Vec::with_capacity(polygons.len());
        for polygon in polygons {
            match plane.split_polygon(polygon) {
                Split::CoplanarFront(piece) | Split::CoplanarBack(piece) => {
                    self.polygons.push(piece)
                }
                Split::Front(piece) => front.push(piece),
                Split::Back(piece) => back.push(piece),
                Split::Spanning {
                    front: front_piece,
                    back: back_piece,
                } => {
                    if let Some(piece) = front_piece {
                        front.push(piece);
                    }
                    if let Some(piece) = back_piece {
                        back.push(piece);
                    }
                }
            }
        }
        if !front.is_empty() {
            self.front.get_or_insert_with(Box::default).build(front);
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Box::default).build(back);
        }
    }

    /// Keep the parts of `polygons` outside this tree's solid: polygons
    /// (and pieces) fully inside are dropped.
    fn clip_polygons(&self, polygons: Vec<Polygon>) -> Vec<Polygon> {
        let Some(plane) = self.plane else {
            return polygons;
        };
        let mut front = Vec::with_capacity(polygons.len());
        let mut back = Vec::with_capacity(polygons.len());
        for polygon in polygons {
            match plane.split_polygon(polygon) {
                Split::CoplanarFront(piece) | Split::Front(piece) => front.push(piece),
                Split::CoplanarBack(piece) | Split::Back(piece) => back.push(piece),
                Split::Spanning {
                    front: front_piece,
                    back: back_piece,
                } => {
                    if let Some(piece) = front_piece {
                        front.push(piece);
                    }
                    if let Some(piece) = back_piece {
                        back.push(piece);
                    }
                }
            }
        }
        let mut kept = Vec::new();
        match &self.front {
            // Pieces routed into a missing front subtree are outside.
            Some(child) => kept.extend(child.clip_polygons(front)),
            None => kept.extend(front),
        }
        if let Some(child) = &self.back {
            kept.extend(child.clip_polygons(back));
        }
        // Pieces routed into a missing back subtree were inside: dropped.
        kept
    }

    /// Remove every polygon of this tree that is inside `other`'s solid.
    fn clip_to(&mut self, other: &Node) {
        self.polygons = other.clip_polygons(mem::take(&mut self.polygons));
        if let Some(child) = &mut self.front {
            child.clip_to(other);
        }
        if let Some(child) = &mut self.back {
            child.clip_to(other);
        }
    }

    /// Swap solid and empty space: flip planes and windings, swap children.
    fn invert(&mut self) {
        if let Some(plane) = &mut self.plane {
            plane.flip();
        }
        for polygon in &mut self.polygons {
            polygon.flip();
        }
        mem::swap(&mut self.front, &mut self.back);
        if let Some(child) = &mut self.front {
            child.invert();
        }
        if let Some(child) = &mut self.back {
            child.invert();
        }
    }

    fn all_polygons(&self) -> Vec<Polygon> {
        let mut all = self.polygons.clone();
        if let Some(child) = &self.front {
            all.extend(child.all_polygons());
        }
        if let Some(child) = &self.back {
            all.extend(child.all_polygons());
        }
        all
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::{Mesh, TopoShape, DEFAULT_DEFLECTION};
    use fc_base::Vector3;
    use std::time::{Duration, Instant};

    /// A `length x width x height` box (FreeCAD convention: centered in
    /// XY, sitting on Z=0) translated by `(dx, dy, dz)`.
    fn box_at(length: f64, width: f64, height: f64, dx: f64, dy: f64, dz: f64) -> Mesh {
        let mut mesh = TopoShape::Box {
            length,
            width,
            height,
        }
        .tessellate(DEFAULT_DEFLECTION);
        for v in &mut mesh.vertices {
            v.x += dx;
            v.y += dy;
            v.z += dz;
        }
        mesh
    }

    /// Two 2x2x2 boxes overlapping with offsets on all three axes:
    /// A spans [-1,1]x[-1,1]x[0,2] and B spans [0,2]x[-0.5,1.5]x[1,3],
    /// so the intersection is 1.0 * 1.5 * 1.0 and no face planes
    /// coincide.
    fn overlapping_boxes() -> (Mesh, Mesh) {
        (
            box_at(2.0, 2.0, 2.0, 0.0, 0.0, 0.0),
            box_at(2.0, 2.0, 2.0, 1.0, 0.5, 1.0),
        )
    }

    const VA: f64 = 8.0;
    const VB: f64 = 8.0;
    const OVERLAP: f64 = 1.5;

    fn assert_volume(result: &Mesh, expected: f64, relative: f64, what: &str) {
        let got = result.volume();
        assert!(
            (got - expected).abs() <= relative * expected,
            "{what}: volume {got} differs from expected {expected} by more than {relative}"
        );
    }

    #[test]
    fn boolean_cut_offset_boxes_volume_and_watertight() {
        let (a, b) = overlapping_boxes();
        let result = cut(&a, &b);
        assert!(result.triangle_count() > 0);
        assert!(mesh_is_closed(&result), "cut result must be watertight");
        assert_volume(&result, VA - OVERLAP, 0.02, "box cut box");
    }

    #[test]
    fn boolean_fuse_offset_boxes_volume_and_watertight() {
        let (a, b) = overlapping_boxes();
        let result = fuse(&a, &b);
        assert!(result.triangle_count() > 0);
        assert!(mesh_is_closed(&result), "fuse result must be watertight");
        assert_volume(&result, VA + VB - OVERLAP, 0.02, "box fuse box");
    }

    #[test]
    fn boolean_common_offset_boxes_volume_and_watertight() {
        let (a, b) = overlapping_boxes();
        let result = common(&a, &b);
        assert!(result.triangle_count() > 0);
        assert!(mesh_is_closed(&result), "common result must be watertight");
        assert_volume(&result, OVERLAP, 0.02, "box common box");
    }

    #[test]
    fn boolean_sphere_cut_by_box_cap_volume() {
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.12);
        assert!(
            sphere.triangle_count() <= 800,
            "coarse sphere: {} tris",
            sphere.triangle_count()
        );
        // A 4x4x1 box whose bottom face sits at z = 0.5; cutting it away
        // removes the spherical cap of height h = 0.5.
        let mut tool = TopoShape::Box {
            length: 4.0,
            width: 4.0,
            height: 1.0,
        }
        .tessellate(DEFAULT_DEFLECTION);
        for v in &mut tool.vertices {
            v.z += 0.5;
        }
        let result = cut(&sphere, &tool);
        assert!(result.triangle_count() > 0);
        assert!(result.volume() < sphere.volume(), "the cap must be gone");
        let pi = std::f64::consts::PI;
        let h = 0.5_f64;
        let cap = pi * h * h * (3.0 - h) / 3.0;
        let expected = 4.0 / 3.0 * pi - cap;
        assert_volume(&result, expected, 0.05, "sphere cut box");
    }

    #[test]
    fn boolean_disjoint_fuse_and_common() {
        let a = box_at(2.0, 2.0, 2.0, 0.0, 0.0, 0.0);
        let b = box_at(2.0, 2.0, 2.0, 10.0, 0.0, 0.0);
        let fused = fuse(&a, &b);
        assert!(mesh_is_closed(&fused), "two separate shells are closed");
        assert_volume(&fused, VA + VB, 1e-12, "disjoint fuse");
        let common_result = common(&a, &b);
        assert_eq!(common_result.triangle_count(), 0);
        assert!(matches!(try_common(&a, &b), Err(BooleanError::EmptyResult)));
    }

    #[test]
    fn boolean_cut_interior_box_makes_void() {
        let big = box_at(4.0, 4.0, 4.0, 0.0, 0.0, 1.0);
        let small = box_at(1.0, 1.0, 1.0, 0.0, 0.0, 2.5);
        let hollow = cut(&big, &small);
        assert!(mesh_is_closed(&hollow), "outer shell plus void is closed");
        assert_volume(&hollow, 64.0 - 1.0, 0.02, "box cut interior box");
        let gone = cut(&small, &big);
        assert_eq!(gone.triangle_count(), 0);
        assert!(matches!(
            try_cut(&small, &big),
            Err(BooleanError::EmptyResult)
        ));
    }

    #[test]
    fn boolean_identical_operands() {
        let a = box_at(2.0, 2.0, 2.0, 0.0, 0.0, 0.0);
        let b = a.clone();
        let fused = fuse(&a, &b);
        assert_volume(&fused, VA, 0.02, "fuse with self");
        assert!(mesh_is_closed(&fused));
        assert_eq!(cut(&a, &b).triangle_count(), 0, "self minus self is empty");
        let shared = common(&a, &b);
        assert_volume(&shared, VA, 0.02, "common with self");
        assert!(mesh_is_closed(&shared));
    }

    #[test]
    fn boolean_degenerate_operands() {
        let a = box_at(2.0, 2.0, 2.0, 0.0, 0.0, 0.0);
        let empty = Mesh::default();
        assert_volume(&cut(&a, &empty), VA, 1e-12, "cut with empty tool");
        assert_volume(&fuse(&a, &empty), VA, 1e-12, "fuse with empty");
        assert_volume(&fuse(&empty, &a), VA, 1e-12, "fuse into empty");
        assert_eq!(common(&a, &empty).triangle_count(), 0);
        assert_eq!(cut(&empty, &a).triangle_count(), 0);
        // A single triangle is not a solid.
        let tri = Mesh {
            vertices: vec![Vector3::ZERO, Vector3::X, Vector3::Y],
            triangles: vec![[0, 1, 2]],
        };
        assert!(matches!(
            try_cut(&a, &tri),
            Err(BooleanError::DegenerateInput { .. })
        ));
        assert!(matches!(
            try_fuse(&tri, &a),
            Err(BooleanError::DegenerateInput { .. })
        ));
        assert!(matches!(
            try_common(&a, &tri),
            Err(BooleanError::DegenerateInput { .. })
        ));
        // The infallible variants treat it as empty.
        assert_volume(&cut(&a, &tri), VA, 1e-12, "cut with non-solid tool");
        assert_volume(&fuse(&a, &tri), VA, 1e-12, "fuse with non-solid");
    }

    #[test]
    fn boolean_error_reporting() {
        let err = BooleanError::DegenerateInput {
            operand: "b",
            triangles: 3,
        };
        let text = err.to_string();
        assert!(text.contains('b'), "{text}");
        assert!(text.contains('3'), "{text}");
        assert!(BooleanError::EmptyResult.to_string().contains("empty"));
        fn as_error<E: std::error::Error>(_: &E) {}
        as_error(&err);
        as_error(&BooleanError::EmptyResult);
    }

    #[test]
    fn boolean_mesh_is_closed_checks() {
        let cube = box_at(1.0, 1.0, 1.0, 0.0, 0.0, 0.0);
        assert!(mesh_is_closed(&cube));
        assert!(!mesh_is_closed(&Mesh::default()), "no boundary at all");
        let mut missing_face = cube.clone();
        missing_face.triangles.pop();
        assert!(!mesh_is_closed(&missing_face));
        let mut collapsed = cube.clone();
        collapsed.triangles.push([0, 0, 1]);
        assert!(!mesh_is_closed(&collapsed));
        let mut out_of_range = cube.clone();
        out_of_range.triangles.push([0, 1, 999]);
        assert!(!mesh_is_closed(&out_of_range));
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.5);
        assert!(mesh_is_closed(&sphere));
    }

    #[test]
    fn boolean_try_matches_infallible() {
        let (a, b) = overlapping_boxes();
        let cases = [
            (try_fuse(&a, &b), fuse(&a, &b)),
            (try_cut(&a, &b), cut(&a, &b)),
            (try_common(&a, &b), common(&a, &b)),
        ];
        for (tried, plain) in cases {
            let tried = tried.expect("overlapping boxes yield results");
            assert_eq!(tried.volume(), plain.volume());
            assert_eq!(tried.triangles, plain.triangles);
        }
    }

    #[test]
    fn boolean_fuse_two_thousand_tri_spheres_performance() {
        let a = TopoShape::Sphere { radius: 1.0 }.tessellate(0.072);
        assert!((1500..=2000).contains(&a.triangle_count()));
        let mut b = a.clone();
        for v in &mut b.vertices {
            v.x += 1.2;
            v.y += 0.3;
            v.z += 0.5;
        }
        let start = Instant::now();
        let fused = fuse(&a, &b);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "fusing {} + {} triangles took {elapsed:?}",
            a.triangle_count(),
            b.triangle_count()
        );
        assert!(fused.triangle_count() > 0);
        // Two unit spheres at distance d share the lens
        // pi * (4 + d) * (2 - d)^2 / 12.
        let d = (1.2_f64 * 1.2 + 0.3 * 0.3 + 0.5 * 0.5).sqrt();
        let lens = std::f64::consts::PI * (4.0 + d) * (2.0 - d) * (2.0 - d) / 12.0;
        assert_volume(&fused, 2.0 * a.volume() - lens, 0.05, "sphere fuse sphere");
    }
}
