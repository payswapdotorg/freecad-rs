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
//! which closes planar and cylindrical seams bit-exactly.
//!
//! **Curved seams** (where *both* surfaces are tessellated — sphere
//! against sphere, crossed cylinders, a tilted cylinder through a
//! sphere) need more than bit-exactness: the two sides trace the same
//! mathematical seam curve with different floating-point vertices, up
//! to about one deflection apart, so the exact weld can never pair
//! them. After the stitch, `close_curved_seams` mends what is left —
//! snapping the two traces onto a consensus grid derived from the
//! seam's own sampling scale (the median boundary-edge step, the one
//! quantity a seam cannot misreport about itself), interleaving
//! staggered vertices (the tolerance-based generalization of the
//! T-junction stitch), welding fin pairs, thinning over-covered
//! sheets, and capping punctures — under a volume guard that reverts
//! the whole repair unless the result is closed *and* honest. Seams
//! with several branches that meet (the two closed curves of crossed
//! cylinders, which cross at two points) are mended by the same
//! machinery: what the interleave welds are the traces' own staggered
//! samples, and the sampling-scale tolerances keep distinct branches
//! distinct. A mesh the exact weld already closed passes through
//! bit-identically.
//!
//! # Determinism
//!
//! Trees are built in input order, no hash container is iterated, and
//! welding goes through a `BTreeMap`: the same operands always produce
//! the same output mesh, bit for bit. The seam repair keeps the same
//! discipline: its hash maps are lookup-only, every iteration order is
//! sorted or index-driven, and ties break on indices.
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
//! * The curved-seam repair is honest, not magic: geometry thinner
//!   than the operands' own tessellation resolution cannot be closed
//!   without fabricating structure, so a near-tangent intersection
//!   (a lens thinner than a chord of its operands' meshes) is
//!   returned open rather than patched shut — recompute at a finer
//!   deflection to resolve it.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// Vertex-side bits for [`Plane::classify`].
const FRONT_BIT: u8 = 1;
const BACK_BIT: u8 = 2;

/// Which operand a polygon (and every triangle fan-triangulated from
/// it) came from. The curved-seam repair matches operand A's
/// tessellation trace of a seam against operand B's — two traces of
/// the same mathematical curve that disagree by up to a deflection —
/// so it must know which side of the seam each boundary edge is on.
/// Planar seams never need this: their crossing points are bit-exact
/// and the weld closes them without help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    A,
    B,
}

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
    boolean_prepared(a, polygons_of(a, Side::A), polygons_of(b, Side::B), op)
}

fn try_boolean(a: &Mesh, b: &Mesh, op: Op) -> Result<Mesh, BooleanError> {
    let polygons_a = polygons_of(a, Side::A);
    if polygons_a.len() < MIN_SOLID_TRIANGLES {
        return Err(BooleanError::DegenerateInput {
            operand: "a",
            triangles: polygons_a.len(),
        });
    }
    let polygons_b = polygons_of(b, Side::B);
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
    // (Sides are assigned per operand, so equality compares geometry.)
    let same_geometry = polygons_a.len() == polygons_b.len()
        && polygons_a
            .iter()
            .zip(&polygons_b)
            .all(|(x, y)| x.vertices == y.vertices);
    if polygons_a.len() >= MIN_SOLID_TRIANGLES && same_geometry {
        return match op {
            Op::Union | Op::Intersect => a.clone(),
            Op::Subtract => Mesh::default(),
        };
    }
    let polygons = stitch_t_junctions(op_result(polygons_a, polygons_b, op));
    let (mut mesh, mut sides) = mesh_from_sided(polygons);
    close_curved_seams(&mut mesh, &mut sides);
    mesh
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
/// `side` records which operand the polygons (and everything the BSP
/// splits them into) came from.
fn polygons_of(mesh: &Mesh, side: Side) -> Vec<Polygon> {
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
                side,
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
            match new_polygon(stitched, polygon.side) {
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
/// stitch above — makes consistent. Every output triangle also reports
/// which operand its polygon came from, for the seam repair below.
fn mesh_from_sided(polygons: Vec<Polygon>) -> (Mesh, Vec<Side>) {
    let mut index: BTreeMap<VertexKey, u32> = BTreeMap::new();
    let mut vertices: Vec<Vector3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut sides: Vec<Side> = Vec::new();
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
                sides.push(polygon.side);
            }
        }
    }
    collapse_duplicate_triangles(&mut triangles, &mut sides);
    (Mesh::new(vertices, triangles), sides)
}

/// Collapse multiply-covered sheets: one triangle per distinct vertex
/// set, in the majority winding (ties keep the earliest occurrence's).
/// Where a surface is covered more than once — the BSP's coincident-face
/// handling leaving opposed windings of one region, or the consensus
/// snap welding two near-coincident slivers into exact duplicates —
/// every edge of the sheet is used by four or more triangles, never
/// twice, so nothing downstream could close it. Keeping one copy
/// preserves the surface and restores edge multiplicity two without
/// touching anything else. A clean list passes through bit-identically.
fn collapse_duplicate_triangles(triangles: &mut Vec<[u32; 3]>, sides: &mut Vec<Side>) {
    if triangles.len() < 2 {
        return;
    }
    // Orientation of a triangle relative to its sorted key: the two
    // possible windings are [a, b, c] and [a, c, b], and a triangle
    // carries one iff it is a cyclic rotation of it — which its own
    // smallest-starting rotation identifies.
    let orientation = |triangle: [u32; 3], ascending: [u32; 3]| -> bool {
        canonical_rotation(triangle) == ascending
    };
    let mut tally: HashMap<[u32; 3], [usize; 2]> = HashMap::with_capacity(triangles.len());
    for triangle in triangles.iter() {
        let mut key = *triangle;
        key.sort_unstable();
        let slot = tally.entry(key).or_insert([0, 0]);
        if orientation(*triangle, key) {
            slot[0] += 1;
        } else {
            slot[1] += 1;
        }
    }
    let mut kept: Vec<[u32; 3]> = Vec::with_capacity(triangles.len());
    let mut kept_sides: Vec<Side> = Vec::with_capacity(sides.len());
    let mut emitted: BTreeSet<[u32; 3]> = BTreeSet::new();
    for (i, triangle) in triangles.iter().enumerate() {
        let mut key = *triangle;
        key.sort_unstable();
        if !emitted.insert(key) {
            continue;
        }
        let descending = [key[0], key[2], key[1]];
        let [ascending_count, descending_count] = tally[&key];
        let keep_ascending = if ascending_count == descending_count {
            orientation(*triangle, key)
        } else {
            ascending_count > descending_count
        };
        kept.push(if keep_ascending { key } else { descending });
        kept_sides.push(sides[i]);
    }
    *triangles = kept;
    *sides = kept_sides;
}

/// The rotation of a triangle's indices that starts with the smallest
/// vertex — the winding-independent identity of its vertex set.
fn canonical_rotation(triangle: [u32; 3]) -> [u32; 3] {
    let rotations = [
        [triangle[0], triangle[1], triangle[2]],
        [triangle[1], triangle[2], triangle[0]],
        [triangle[2], triangle[0], triangle[1]],
    ];
    *rotations.iter().min().expect("three rotations exist")
}

// ---------------------------------------------------------------------------
// Curved-seam repair
// ---------------------------------------------------------------------------

/// Mend rounds: snap, interleave, thin, cap. Honest seams converge in
/// two or three rounds; the cap is a runaway guard.
const SEAM_MEND_ROUNDS: usize = 6;

/// The consensus grid, as a fraction of the mesh's sampling scale:
/// fine enough that vertices a full sampling width apart never merge,
/// coarse enough that the traces' disagreement — floating-point noise
/// up to a fraction of the deflection — always lands on one point.
const SNAP_GRID_FRACTION: f64 = 0.25;

/// How far a boundary vertex may sit from an opposing boundary edge and
/// still be inserted into it, in multiples of the mesh's sampling
/// scale: the traces each deviate from the true seam curve by less
/// than their own chord error, so a fraction of a sampling width
/// covers the stagger with margin.
const SEAM_BRIDGE_SCALES: f64 = 1.0;

/// How far the fan across a puncture may sag, as a multiple of the
/// mesh's sampling scale — the deflection the tessellation itself
/// already accepts. Bounded by the rim's own curvature, not its size.
const HOLE_CAP_SCALE: f64 = 2.0;

/// How much volume the repair may add or remove relative to the
/// un-repaired mesh. Legitimate repairs remove sub-scale slivers and
/// replace a band of surface with a ruled strip between the same rims,
/// moving volume by a fraction of a percent; a change beyond a small
/// fraction means the pass fabricated structure, and the result is
/// rejected wholesale.
const SEAM_REPAIR_VOLUME_GUARD: f64 = 0.05;

/// Close the seams the bit-exact weld could not.
///
/// # The failure this repairs
///
/// The BSP clips operand A's polygons against the *planes* of operand
/// B's triangles and vice versa. On a planar or cylindrical seam the
/// two sides compute the *same* crossing points (the canonicalized
/// [`Plane::split_point`] makes them bit-identical) and the exact weld
/// in [`mesh_from_sided`] closes the output. On a curved seam — sphere
/// against sphere — both surfaces are tessellated: the seam is a curved
/// 3D polyline, and the crossing points computed on A's triangle edges
/// (against B's facet planes) are *different floating-point values*
/// than the ones computed on B's edges (against A's facet planes). They
/// represent the same mathematical points but disagree by anything from
/// last-bit noise up to about one tessellation deflection, so the
/// bit-exact weld cannot pair them. What the weld leaves behind has two
/// kinds of damage, each with its own treatment:
///
/// * **Overlapping sheets** — edges used by three or more triangles,
///   where the clip/stitch machinery covered one region several times
///   with different slivers. [`thin_overlaps`] peels them off: as long
///   as an edge is used too often, the smallest-area triangle touching
///   a over-used edge is removed. The removed pieces are sub-scale
///   slivers; the surface underneath survives.
/// * **The seam braid** — edges used exactly once, woven where the two
///   operands' traces of the seam disagree. [`cap_small_rims`] first
///   fan-caps the punctures that are smaller than the mesh's own
///   sampling scale (tears, thinned-out spots). The braid itself is
///   then mended in rounds ([`snap_to_consensus`] welds the traces'
///   near-agreeing vertices onto one consensus grid;
///   [`interleave_traces`] inserts each trace's staggered samples into
///   the other's boundary edges — the tolerance-based generalization
///   of the T-junction stitch, with the tolerance derived from the
///   seam's own sampling scale; [`weld_fin_edges`] merges the lone
///   counterpart edge pairs those passes leave, with orientation
///   votes keeping the two sheets opposed across every merged edge),
///   and [`collapse_pinholes`] closes the point-sized weld-noise gaps
///   directly.
///
/// # Contract
///
/// * A mesh that is already closed is returned **bit-identical** — the
///   planar/cylindrical exact-weld paths keep their exact current
///   behavior; this pass only engages where the weld left damage.
/// * A seam this pass cannot close — or whose closing would move
///   volume beyond [`SEAM_REPAIR_VOLUME_GUARD`], as when zipping would
///   bridge a structural opening — is left exactly as the weld produced
///   it: the repair applies **wholesale or not at all**, so partial
///   repairs never leak out.
/// * Deterministic throughout: sorted iteration orders, `BTreeMap`s,
///   tie-breaks by index — no hash container is ever iterated.
fn close_curved_seams(mesh: &mut Mesh, sides: &mut Vec<Side>) {
    debug_assert_eq!(mesh.triangles.len(), sides.len());
    if boundary_edges(mesh, sides).is_empty() {
        return; // Already watertight: the exact-weld path, untouched.
    }
    let original = mesh.clone();
    let mut repaired_sides = sides.clone();

    // Mend: snap the traces onto a consensus grid (welding what
    // agrees), interleave what staggers (the tolerance-based stitch),
    // weld the fin pairs the interleave leaves, and thin what the
    // snapping over-covered — then prune degenerate slivers and close
    // what their removal reopens (slits weld, noise loops collapse),
    // repeating a few rounds. Capping — the only pass that adds
    // geometry — runs first each round and never emits a degenerate
    // piece, so nothing churns against the pruner.
    for round in 0..SEAM_MEND_ROUNDS {
        if boundary_edges(mesh, &repaired_sides).is_empty() {
            break;
        }
        cap_small_rims(mesh, &mut repaired_sides);
        if boundary_edges(mesh, &repaired_sides).is_empty() {
            break;
        }
        // Snap once, on the untouched seam: afterwards the only new
        // vertices are the interleave's bit-exact copies of existing
        // ones, which need no snapping, and re-snapping would keep
        // welding fresh pairs into over-covers the thinning then has
        // to reopen.
        let snapped = round == 0 && snap_to_consensus(mesh, &mut repaired_sides);
        if snapped {
            collapse_duplicate_triangles(&mut mesh.triangles, &mut repaired_sides);
        }
        let interleaved = interleave_traces(mesh, &mut repaired_sides);
        let welded = weld_fin_edges(mesh, &mut repaired_sides);
        if !snapped && !interleaved && !welded {
            break;
        }
        thin_overlaps(mesh, &mut repaired_sides);
        prune_degenerate(mesh, &mut repaired_sides);
        // Close what the pruning reopened: needle slits weld (their
        // two long edges lie within reach of each other), noise loops
        // collapse, over-covers thin again.
        weld_fin_edges(mesh, &mut repaired_sides);
        collapse_pinholes(mesh, &mut repaired_sides);
        thin_overlaps(mesh, &mut repaired_sides);
    }
    // Degenerate sweep: prune the sub-bar slivers the rounds left
    // (the closers never emit any), then close what their removal
    // reopens. A seam that cannot take the sweep stays closed-but-
    // slivered only if the sweep's reopens are un-closable — in which
    // case the final gate below reverts the whole repair honestly.
    prune_degenerate(mesh, &mut repaired_sides);
    if !boundary_edges(mesh, &repaired_sides).is_empty() {
        cap_small_rims(mesh, &mut repaired_sides);
        weld_fin_edges(mesh, &mut repaired_sides);
        collapse_pinholes(mesh, &mut repaired_sides);
        thin_overlaps(mesh, &mut repaired_sides);
        cap_small_rims(mesh, &mut repaired_sides);
    }
    collapse_duplicate_triangles(&mut mesh.triangles, &mut repaired_sides);
    if boundary_edges(mesh, &repaired_sides).is_empty() && volume_within_guard(mesh, &original) {
        compact_vertices(mesh);
        *sides = repaired_sides;
    } else {
        // Wholesale-or-not-at-all: a seam that could not be closed —
        // or whose closing would have fabricated structure — is
        // returned exactly as the weld left it.
        *mesh = original;
    }
}

/// The seam's boundary structure: every undirected edge used by exactly
/// one triangle, with the side of the operand that triangle
/// (fan-)came from. Iteration order is the `BTreeMap`'s, so it is
/// deterministic.
fn boundary_edges(mesh: &Mesh, sides: &[Side]) -> Vec<((u32, u32), Side)> {
    // Count by sorting the edge stream (deterministic; faster than an
    // ordered map for this pass-heavy workload).
    let mut stream: Vec<((u32, u32), Side)> = Vec::with_capacity(3 * mesh.triangles.len());
    for (triangle, side) in mesh.triangles.iter().zip(sides) {
        for (u, v) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let edge = if u < v { (u, v) } else { (v, u) };
            stream.push((edge, *side));
        }
    }
    stream.sort_unstable();
    let mut boundary: Vec<((u32, u32), Side)> = Vec::new();
    let mut i = 0;
    while i < stream.len() {
        let mut j = i + 1;
        while j < stream.len() && stream[j].0 == stream[i].0 {
            j += 1;
        }
        if j - i == 1 {
            boundary.push(stream[i]);
        }
        i = j;
    }
    boundary
}

/// Twice the area of a triangle.
fn triangle_area_x2(mesh: &Mesh, triangle: &[u32; 3]) -> f64 {
    let a = mesh.vertices[triangle[0] as usize];
    let b = mesh.vertices[triangle[1] as usize];
    let c = mesh.vertices[triangle[2] as usize];
    (b - a).cross(&(c - a)).length()
}

/// Peel the overlapping sheets: while any edge is used by more than two
/// triangles, remove the smallest-area triangle touching an over-used
/// edge. The clips and the stitch can cover one region several times
/// with different slivers — always smallest-first, so the sub-scale
/// junk goes and the surface underneath survives. Each removal lowers
/// the over-use of at least one edge, so this terminates.
fn thin_overlaps(mesh: &mut Mesh, sides: &mut Vec<Side>) {
    // Edge -> users, maintained incrementally: removing a triangle
    // lowers its edges' counts, and new over-uses cannot appear. A
    // hash map: lookups only, never iterated — determinism comes from
    // the ordered `excess` set that drives the loop.
    let mut users: HashMap<(u32, u32), Vec<usize>> =
        HashMap::with_capacity(3 * mesh.triangles.len());
    for (i, triangle) in mesh.triangles.iter().enumerate() {
        for (u, v) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let edge = if u < v { (u, v) } else { (v, u) };
            users.entry(edge).or_default().push(i);
        }
    }
    let mut alive: Vec<bool> = vec![true; mesh.triangles.len()];
    let mut excess: BTreeSet<(u32, u32)> = users
        .iter()
        .filter(|(_, u)| u.len() > 2)
        .map(|(e, _)| *e)
        .collect();
    while let Some(&edge) = excess.iter().next() {
        // The victim: prefer a triangle all of whose edges are
        // over-used — removing it lowers three over-uses at once and
        // cannot create a boundary edge; among those (or, failing
        // them, among all triangles touching an over-used edge), the
        // smallest area wins, ties breaking on the triangle index.
        // A victim is only removable when none of its flanks is used
        // exactly twice — removing such a triangle would reopen its
        // neighbor's edge, and the cap/thin cycle would never settle.
        // Over-used flanks drop over-use, boundary flanks vanish
        // entirely; both are safe. When every candidate has a count-2
        // flank the over-cover stays (the caller's final check decides
        // honestly). Among removable candidates: most over-used flanks
        // first, then most boundary flanks (shrinking the boundary),
        // then smallest area; ties break on the index.
        let edge_state = |triangle: &[u32; 3]| -> (usize, usize, bool) {
            let mut overs = 0;
            let mut open = 0;
            let mut paired = false;
            for (u, v) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let e = if u < v { (u, v) } else { (v, u) };
                let count = users.get(&e).map_or(0, |u| u.len());
                if count > 2 {
                    overs += 1;
                } else if count == 1 {
                    open += 1;
                } else {
                    paired = true;
                }
            }
            (overs, open, paired)
        };
        let mut candidate: Option<(usize, usize, bool, Reverse<f64>, usize)> = None;
        for &over in &excess {
            for &i in &users[&over] {
                if !alive[i] {
                    continue;
                }
                let (overs, open, paired) = edge_state(&mesh.triangles[i]);
                let area = triangle_area_x2(mesh, &mesh.triangles[i]);
                // Rank: most over-used flanks, most boundary flanks,
                // removable-without-reopening, smallest area; ties on
                // the index.
                let better = candidate.map_or(true, |(bo, bo2, bp, ba, bi)| {
                    (overs, open, !paired, Reverse(area), i) > (bo, bo2, bp, ba, bi)
                });
                if better {
                    candidate = Some((overs, open, !paired, Reverse(area), i));
                }
            }
        }
        let Some((_, _, _, _, victim)) = candidate else {
            return;
        };
        alive[victim] = false;
        let triangle = mesh.triangles[victim];
        for (u, v) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let e = if u < v { (u, v) } else { (v, u) };
            if let Some(list) = users.get_mut(&e) {
                list.retain(|&i| i != victim);
                if list.len() <= 2 {
                    excess.remove(&e);
                }
            }
        }
        if users[&edge].len() <= 2 {
            excess.remove(&edge);
        }
    }
    // Apply the removals, keeping `sides` in sync.
    let keep: Vec<usize> = (0..mesh.triangles.len()).filter(|i| alive[*i]).collect();
    if keep.len() == mesh.triangles.len() {
        return;
    }
    let triangles: Vec<[u32; 3]> = keep.iter().map(|&i| mesh.triangles[i]).collect();
    let new_sides: Vec<Side> = keep.iter().map(|&i| sides[i]).collect();
    mesh.triangles = triangles;
    *sides = new_sides;
}

/// The seam's sampling scale: the median length of the seam's own
/// boundary edges — the braid *is* the discretization of the seam
/// curve, so its step is the scale at which the two traces sample the
/// same mathematical curve and the only scale at which welding them is
/// meaningful. The traces' crossing points are computed on each
/// operand's facet edges against the other's facet planes, so the
/// steps reflect the operands' own sampling (made finer wherever the
/// BSP splitter subdivided) — pooled over both sheets and one median,
/// which tolerates the weld-noise slivers that hug every curved seam
/// and the occasional long straight run (a rim edge crossing a whole
/// facet) that a mean would be dragged up by.
///
/// Why not the triangles adjacent to the seam: on a coarse operand
/// (an 8-segment cylinder wall, say) the seam braid runs *across* the
/// facets, and a braid step is a fraction of the facet it cuts — the
/// adjacent triangles' edges overestimate the sampling by the facet's
/// full width, and every downstream tolerance (the consensus grid,
/// the interleave reach) inherits an overestimate that welds across
/// real structure. The braid's own median cannot lie about its own
/// step.
fn seam_scale(mesh: &Mesh, boundary: &[((u32, u32), Side)]) -> f64 {
    let mut steps: Vec<f64> = boundary
        .iter()
        .map(|((u, v), _)| (mesh.vertices[*v as usize] - mesh.vertices[*u as usize]).length())
        .filter(|l| *l >= PLANE_EPS)
        .collect();
    if let Some(scale) = median(&mut steps) {
        return scale;
    }
    // No boundary at all (or nothing usable): the whole-mesh median.
    let mut all: Vec<f64> = mesh
        .triangles
        .iter()
        .map(|t| {
            let (a, b, c) = (
                mesh.vertices[t[0] as usize],
                mesh.vertices[t[1] as usize],
                mesh.vertices[t[2] as usize],
            );
            (b - a).length().max((c - b).length()).max((a - c).length())
        })
        .filter(|l| *l >= PLANE_EPS)
        .collect();
    median(&mut all).unwrap_or(SEAM_TOLERANCE_FLOOR)
}

/// The middle value (lower of the two middles on even counts):
/// deterministic, and robust to the slivers and long straight runs
/// that share a boundary with the honest samples.
fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|x, y| x.partial_cmp(y).expect("finite lengths"));
    Some(values[values.len() / 2])
}

/// A floor for scale computations on degenerate boundaries: a few
/// classification tolerances, so pure floating-point noise always
/// registers as sub-scale.
const SEAM_TOLERANCE_FLOOR: f64 = 4.0 * PLANE_EPS;

/// One closed rim of a hole: its vertices in boundary direction (the
/// direction the surviving triangle winds along its boundary edge),
/// and the side those surviving triangles came from.
struct Rim {
    vertices: Vec<u32>,
    side: Side,
}

/// Walk every boundary rim **per operand sheet**: the triangles that
/// came from one side form a surface whose torn edges are exactly that
/// side's boundary edges, and the seam braid is where the two sheets'
/// tears interleave. Walking stays on one sheet and treats boundary
/// edges as undirected — the braid's clipped pieces can carry locally
/// inconsistent windings, so a direction-following walk dead-ends where
/// two edges point the same way, while closure only ever needs edge
/// *counts*, which do not care about direction. At a pinch (several
/// sheet boundary edges at one vertex) the straightest continuation
/// keeps the rim smooth instead of spiraling into the tangle. Rims that
/// still dead-end are skipped.
fn walk_boundary_rims(mesh: &Mesh, sides: &[Side], boundary: &[((u32, u32), Side)]) -> Vec<Rim> {
    // Undirected boundary edges with their owning side, indexed by
    // slot; adjacency lists map each vertex to its incident slots.
    let mut edges: Vec<((u32, u32), Side)> =
        boundary.iter().map(|((u, v), s)| ((*u, *v), *s)).collect();
    edges.sort_unstable();
    let mut at: HashMap<u32, Vec<usize>> = HashMap::with_capacity(2 * edges.len());
    for (slot, ((u, v), _)) in edges.iter().enumerate() {
        at.entry(*u).or_default().push(slot);
        at.entry(*v).or_default().push(slot);
    }
    // Per-vertex, per-sheet accumulated normal: the spin axis for the
    // angular ordering. Triangles contribute their winding normal.
    let mut spin: BTreeMap<(u32, Side), Vector3> = BTreeMap::new();
    for (triangle, side) in mesh.triangles.iter().zip(sides) {
        let (a, b, c) = (
            mesh.vertices[triangle[0] as usize],
            mesh.vertices[triangle[1] as usize],
            mesh.vertices[triangle[2] as usize],
        );
        let n = (b - a).cross(&(c - a));
        for &v in triangle {
            *spin.entry((v, *side)).or_insert(Vector3::ZERO) =
                spin.get(&(v, *side)).copied().unwrap_or(Vector3::ZERO) + n;
        }
    }

    let mut rims: Vec<Rim> = Vec::new();
    for sheet in [Side::A, Side::B] {
        let mut used = vec![false; edges.len()];
        for start in 0..edges.len() {
            if used[start] || edges[start].1 != sheet {
                continue;
            }
            let ((s, e), _) = edges[start];
            used[start] = true;
            let mut vertices = vec![s, e];
            let mut closed = false;
            'rim: loop {
                let prev = vertices[vertices.len() - 2];
                let v = vertices[vertices.len() - 1];
                // Planar continuation: among the unused same-sheet
                // boundary edges at `v`, take the first in angular
                // order around the sheet's local spin axis, starting
                // from back along the arrival direction. On a clean
                // rim (two edges at `v`) there is nothing to choose;
                // where several loops pass through one vertex, the
                // angular order keeps each on its own side of the
                // crossing instead of cutting across. Ties break on
                // the far vertex, then the slot.
                let axis = spin.get(&(v, sheet)).copied().unwrap_or(Vector3::ZERO);
                let axis_len = axis.length();
                let back = mesh.vertices[prev as usize] - mesh.vertices[v as usize];
                let candidates = at.get(&v).map_or(&[][..], |c| c.as_slice());
                let next = candidates
                    .iter()
                    .copied()
                    .filter(|&slot| !used[slot] && edges[slot].1 == sheet)
                    .map(|slot| {
                        let ((x, y), _) = edges[slot];
                        let far = if x == v { y } else { x };
                        let out = mesh.vertices[far as usize] - mesh.vertices[v as usize];
                        // Signed angle from `back` to `out` around the
                        // axis, in [0, 2*pi): undefined axis falls back
                        // to the straightest continuation.
                        let angle = if axis_len > PLANE_EPS {
                            let n = axis * (1.0 / axis_len);
                            let cross = back.cross(&out).dot(&n);
                            let dot = back.dot(&out);
                            let mut a = cross.atan2(dot);
                            if a < 0.0 {
                                a += 2.0 * std::f64::consts::PI;
                            }
                            a
                        } else {
                            let bl = back.length();
                            let ol = out.length();
                            if bl > 0.0 && ol > 0.0 {
                                -(back.dot(&out) / (bl * ol))
                            } else {
                                0.0
                            }
                        };
                        (angle, far, slot)
                    })
                    .min_by(|a, b| {
                        a.0.partial_cmp(&b.0)
                            .expect("finite angle")
                            .then(a.1.cmp(&b.1).then(a.2.cmp(&b.2)))
                    });
                let Some((_, far, slot)) = next else {
                    break 'rim; // Dead end: no unused sheet edge at v.
                };
                used[slot] = true;
                if far == vertices[0] {
                    closed = true;
                    break 'rim;
                }
                vertices.push(far);
            }
            if closed && vertices.len() >= 3 {
                rims.push(Rim {
                    vertices,
                    side: sheet,
                });
            }
            // Open chains mean genuinely un-walkable spots; their edges
            // stay boundary and the caller's final check decides.
        }
    }
    rims
}

/// Fan-cap the boundary rims that are smaller than the mesh's own
/// sampling scale: a puncture that small stays within the boolean's
/// deflection honesty when capped, while a larger rim (the seam braid,
/// a structural opening) must not be spiked through. The fan runs
/// against the rim's directed edges, so every rim edge pairs up.
fn cap_small_rims(mesh: &mut Mesh, sides: &mut Vec<Side>) {
    let boundary = boundary_edges(mesh, sides);
    if boundary.is_empty() {
        return;
    }
    let scale = seam_scale(mesh, &boundary);
    let rims = walk_boundary_rims(mesh, sides, &boundary);
    // The direction each boundary edge's single surviving owner uses
    // (keyed canonically, value: the lower-indexed endpoint comes
    // first in the owner's direction).
    let mut owner_direction: HashMap<(u32, u32), bool> = HashMap::new();
    {
        let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
        for triangle in &mesh.triangles {
            for (u, v) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let e = if u < v { (u, v) } else { (v, u) };
                *counts.entry(e).or_insert(0) += 1;
            }
        }
        for triangle in &mesh.triangles {
            for (u, v) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let key = if u < v { (u, v) } else { (v, u) };
                if counts.get(&key) == Some(&1) {
                    owner_direction.insert(key, u < v);
                }
            }
        }
    }
    let mut fans: Vec<[u32; 3]> = Vec::new();
    let mut fan_sides: Vec<Side> = Vec::new();
    for rim in &rims {
        let mut centroid = Vector3::ZERO;
        for &v in &rim.vertices {
            centroid = centroid + mesh.vertices[v as usize];
        }
        centroid = centroid * (1.0 / rim.vertices.len() as f64);
        let extent = rim
            .vertices
            .iter()
            .map(|v| (mesh.vertices[*v as usize] - centroid).length())
            .fold(0.0_f64, f64::max);
        // Honest-size test: a fan across the rim deviates from the
        // surface by about extent^2 / (8 R), with R the rim's own
        // curvature (the circumradius of three well-spread rim points).
        // Capping is honest while that sagitta stays within the
        // sampling scale — the deflection the tessellation itself
        // already accepts. Nearly flat rims (R -> infinity) always
        // pass; a wrong cap there is caught by the volume guard.
        let p0 = rim
            .vertices
            .iter()
            .map(|v| mesh.vertices[*v as usize])
            .max_by(|a, b| {
                (*a - centroid)
                    .length()
                    .partial_cmp(&(*b - centroid).length())
                    .expect("finite lengths")
            })
            .expect("nonempty rim");
        let p1 = rim
            .vertices
            .iter()
            .map(|v| mesh.vertices[*v as usize])
            .max_by(|a, b| {
                (*a - p0)
                    .length()
                    .partial_cmp(&(*b - p0).length())
                    .expect("finite lengths")
            })
            .expect("nonempty rim");
        let mid = (p0 + p1) * 0.5;
        let p2 = rim
            .vertices
            .iter()
            .map(|v| mesh.vertices[*v as usize])
            .max_by(|a, b| {
                (*a - mid)
                    .length()
                    .partial_cmp(&(*b - mid).length())
                    .expect("finite lengths")
            })
            .expect("nonempty rim");
        let circumradius = {
            let a = (p1 - p0).length();
            let b = (p2 - p1).length();
            let c = (p0 - p2).length();
            let area_x2 = (p1 - p0).cross(&(p2 - p0)).length();
            if area_x2 > PLANE_EPS {
                a * b * c / (2.0 * area_x2)
            } else {
                f64::INFINITY
            }
        };
        let sagitta = extent * extent / (8.0 * circumradius);
        if sagitta > HOLE_CAP_SCALE * scale {
            continue; // Too curved to cap honestly.
        }
        // Cap with a fan around a brand-new vertex at the rim's
        // centroid: every spoke is a new edge, so the cap cannot
        // collide with anything already in the mesh, and the fan spans
        // the puncture flat through its middle instead of spiking to a
        // rim vertex. Each fan triangle opposes the direction its rim
        // edge's surviving owner uses, so the cap follows the surface's
        // own orientation.
        let apex = mesh.vertices.len() as u32;
        mesh.vertices.push(centroid);
        let vs = &rim.vertices;
        for k in 0..vs.len() {
            let (a, b) = (vs[k], vs[(k + 1) % vs.len()]);
            // The surviving owner's direction on this rim edge.
            let owner_uses_a_to_b = owner_direction
                .get(&(a.min(b), a.max(b)))
                .copied()
                .expect("rim edge has an owner");
            let triangle = if owner_uses_a_to_b {
                [apex, b, a] // opposes a -> b
            } else {
                [apex, a, b] // opposes b -> a
            };
            // Never emit a degenerate: a fan piece under the bar
            // belongs to a noise rim the collapse/weld passes close;
            // emitting it would only churn against the pruner.
            if triangle_area_x2(mesh, &triangle) >= 2.0 * DEGENERATE_AREA {
                fans.push(triangle);
                fan_sides.push(rim.side);
            }
        }
    }
    mesh.triangles.extend(fans);
    sides.extend(fan_sides);
}

/// Snap every vertex to a grid of `SNAP_GRID_FRACTION` of the mesh's
/// sampling scale and re-weld: the two operands' traces of the seam
/// hug the same curve, and their corresponding vertices — computed
/// from different segment/plane pairs, disagreeing by floating-point
/// noise up to a fraction of the tessellation deflection — land on the
/// same grid point and weld, closing the seam where the traces agree.
/// Vertices the grid pulls together are within a fraction of a
/// sampling width, so the surface moves well inside the tessellation's
/// own accuracy. Returns whether anything moved.
fn snap_to_consensus(mesh: &mut Mesh, sides: &mut Vec<Side>) -> bool {
    let grid = SNAP_GRID_FRACTION * seam_scale(mesh, &boundary_edges(mesh, sides));
    if !(grid > 0.0 && grid.is_finite()) {
        return false;
    }
    // Quantize and weld through a bit-key map (lookup-only; the vertex
    // order comes from the mesh's own, so determinism is unaffected).
    let mut index: HashMap<VertexKey, u32> = HashMap::with_capacity(mesh.vertices.len());
    let mut vertices: Vec<Vector3> = Vec::with_capacity(mesh.vertices.len());
    let mut remap: Vec<u32> = Vec::with_capacity(mesh.vertices.len());
    let mut changed = false;
    for vertex in &mesh.vertices {
        let snapped = Vector3::new(
            (vertex.x / grid).round() * grid,
            (vertex.y / grid).round() * grid,
            (vertex.z / grid).round() * grid,
        );
        let key = vertex_key(&snapped);
        let next = vertices.len() as u32;
        let id = *index.entry(key).or_insert(next);
        if id == next {
            vertices.push(snapped);
        }
        if vertex_key(vertex) != key {
            changed = true;
        }
        remap.push(id);
    }
    if !changed {
        return false;
    }
    for triangle in &mut mesh.triangles {
        for index in triangle {
            *index = remap[*index as usize];
        }
    }
    mesh.vertices = vertices;
    drop_repeated_index_triangles(mesh, sides);
    collapse_duplicate_triangles(&mut mesh.triangles, sides);
    true
}

/// Insert boundary vertices of one sheet into the interior of the other
/// sheet's boundary edges within [`SEAM_BRIDGE_SCALES`] sampling
/// widths — the tolerance-based generalization of the T-junction
/// stitch. Insertion splits the single triangle owning the boundary
/// edge; nothing moves, nothing is removed, so the operation is
/// unconditionally safe: after it, a vertex the other sheet's
/// tessellation samples between two of this sheet's samples lies on
/// both traces, and the traces' edge sets align wherever the seam
/// curve is followed in the same order. Returns whether anything was
/// inserted.
fn interleave_traces(mesh: &mut Mesh, sides: &mut Vec<Side>) -> bool {
    let boundary = boundary_edges(mesh, sides);
    let reach = SEAM_BRIDGE_SCALES * seam_scale(mesh, &boundary);
    let mut edges_of: [Vec<(u32, u32)>; 2] = [Vec::new(), Vec::new()];
    for ((u, v), side) in &boundary {
        edges_of[index_of(*side)].push((*u, *v));
    }
    let mut any_inserted = false;
    for (from, to) in [(0usize, 1usize), (1usize, 0usize)] {
        if edges_of[from].is_empty() || edges_of[to].is_empty() {
            continue;
        }
        // Opposing vertices already on this sheet's trace (its edge
        // endpoints) need no insertion.
        let on_trace: BTreeSet<u32> = edges_of[to].iter().flat_map(|(u, v)| [*u, *v]).collect();
        // Schedule: edge -> inserted vertices sorted along it.
        let mut schedule: BTreeMap<(u32, u32), Vec<(f64, u32)>> = BTreeMap::new();
        let mut vertices: Vec<u32> = edges_of[from].iter().flat_map(|(u, v)| [*u, *v]).collect();
        vertices.sort_unstable();
        vertices.dedup();
        for &v in &vertices {
            if on_trace.contains(&v) {
                continue;
            }
            let p = mesh.vertices[v as usize];
            let mut best: Option<(f64, f64, (u32, u32))> = None;
            for &(a, b) in &edges_of[to] {
                let pa = mesh.vertices[a as usize];
                let pb = mesh.vertices[b as usize];
                let ab = pb - pa;
                let len = ab.length();
                if len <= 0.0 || !len.is_finite() {
                    continue;
                }
                let t = (p - pa).dot(&ab) / (len * len);
                // Interior foot only; the endcap zone is snapping's
                // business.
                if t * len <= PLANE_EPS || (1.0 - t) * len <= PLANE_EPS {
                    continue;
                }
                if t <= 0.0 || t >= 1.0 {
                    continue;
                }
                let dist = (p - (pa + ab * t)).length();
                if dist > reach {
                    continue;
                }
                let better = best.map_or(true, |(bd, _, be)| (dist, (a, b)) < (bd, be));
                if better {
                    best = Some((dist, t, (a, b)));
                }
            }
            if let Some((_, t, edge)) = best {
                schedule.entry(edge).or_default().push((t, v));
            }
        }
        if schedule.is_empty() {
            continue;
        }
        // Apply by rebuilding the triangle list: a triangle whose edge
        // carries insertions is replaced by the fan along the split
        // edge (winding preserved); pieces go back on a worklist in
        // case another of their edges is scheduled too.
        let mut new_triangles: Vec<[u32; 3]> = Vec::with_capacity(mesh.triangles.len());
        let mut new_sides: Vec<Side> = Vec::with_capacity(sides.len());
        for (i, &triangle) in mesh.triangles.iter().enumerate() {
            let side_here = sides[i];
            let mut worklist = vec![triangle];
            while let Some(tri) = worklist.pop() {
                let mut pieces: Option<Vec<[u32; 3]>> = None;
                for k in 0..3 {
                    let (x, y) = (tri[k], tri[(k + 1) % 3]);
                    let key = if x < y { (x, y) } else { (y, x) };
                    let Some(mut inserts) = schedule.remove(&key) else {
                        continue;
                    };
                    // Insertions are sorted by parameter along key.0 ->
                    // key.1; orient along this triangle's directed
                    // edge, then fan across the apex.
                    if (x, y) == key {
                        inserts.sort_by(|s, t| s.0.partial_cmp(&t.0).expect("finite t"));
                    } else {
                        inserts.sort_by(|s, t| t.0.partial_cmp(&s.0).expect("finite t"));
                    }
                    let apex = tri[(k + 2) % 3];
                    let mut chain: Vec<u32> = Vec::with_capacity(inserts.len() + 2);
                    chain.push(x);
                    chain.extend(inserts.iter().map(|&(_, v)| v));
                    chain.push(y);
                    pieces = Some(chain.windows(2).map(|w| [w[0], w[1], apex]).collect());
                    break;
                }
                match pieces {
                    Some(pieces) => worklist.extend(pieces),
                    None => {
                        new_triangles.push(tri);
                        new_sides.push(side_here);
                    }
                }
            }
        }
        mesh.triangles = new_triangles;
        *sides = new_sides;
        any_inserted = true;
    }
    any_inserted
}

/// Weld near-coincident boundary edge pairs endpoint to endpoint: the
/// fins the mend leaves — a triangle with one unpaired edge whose
/// counterpart from the other sheet also survived as a lone edge —
/// close when both edges' endpoints merge and the two edges become
/// one, used exactly twice. Requiring *both* endpoints of each edge
/// near the other edge is what distinguishes a counterpart pair from
/// legitimately staggered neighbors (whose far endpoint is a whole
/// sampling width away). The slivers spanning a welded pair collapse
/// through the repeated-index drop. Returns whether anything welded.
fn weld_fin_edges(mesh: &mut Mesh, sides: &mut Vec<Side>) -> bool {
    let boundary = boundary_edges(mesh, sides);
    if boundary.is_empty() {
        return false;
    }
    let reach = SEAM_BRIDGE_SCALES * seam_scale(mesh, &boundary);
    let mut edges: Vec<(u32, u32)> = boundary.iter().map(|((u, v), _)| (*u, *v)).collect();
    edges.sort_unstable();
    let segment_distance = |p: Vector3, a: Vector3, b: Vector3| -> f64 {
        let ab = b - a;
        let len2 = ab.dot(&ab);
        if len2 <= 0.0 {
            return (p - a).length();
        }
        let t = ((p - a).dot(&ab) / len2).clamp(0.0, 1.0);
        (p - (a + ab * t)).length()
    };
    // Candidate vertex pairs, deterministic in edge order. A uniform
    // grid over the edges (cell = reach, edges registered in every cell
    // their reach-inflated bounding box touches) keeps the pair search
    // local instead of quadratic.
    let cell = reach.max(PLANE_EPS);
    let cell_of = |p: &Vector3| {
        [
            (p.x / cell).floor() as i32,
            (p.y / cell).floor() as i32,
            (p.z / cell).floor() as i32,
        ]
    };
    let mut buckets: BTreeMap<[i32; 3], Vec<usize>> = BTreeMap::new();
    for (i, &(a1, a2)) in edges.iter().enumerate() {
        let (pa, pb) = (mesh.vertices[a1 as usize], mesh.vertices[a2 as usize]);
        let lo = cell_of(&Vector3::new(
            pa.x.min(pb.x) - reach,
            pa.y.min(pb.y) - reach,
            pa.z.min(pb.z) - reach,
        ));
        let hi = cell_of(&Vector3::new(
            pa.x.max(pb.x) + reach,
            pa.y.max(pb.y) + reach,
            pa.z.max(pb.z) + reach,
        ));
        for cx in lo[0]..=hi[0] {
            for cy in lo[1]..=hi[1] {
                for cz in lo[2]..=hi[2] {
                    buckets.entry([cx, cy, cz]).or_default().push(i);
                }
            }
        }
    }
    // The single owner of each boundary edge, and the direction it
    // uses (keyed canonically; value: lower endpoint first).
    let mut owner: HashMap<(u32, u32), (usize, bool)> = HashMap::new();
    {
        let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
        for (i, triangle) in mesh.triangles.iter().enumerate() {
            for (u, v) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let e = if u < v { (u, v) } else { (v, u) };
                *counts.entry(e).or_insert(0) += 1;
                if counts[&e] == 1 {
                    owner.insert(e, (i, u < v));
                }
            }
        }
    }
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    // Orientation votes: welding must leave the two owners opposing
    // each other across the merged edge; a vote flips an owner that
    // would agree instead.
    let mut votes: Vec<bool> = vec![false; mesh.triangles.len()];
    for i in 0..edges.len() {
        let (a1, a2) = edges[i];
        let (pa1, pa2) = (mesh.vertices[a1 as usize], mesh.vertices[a2 as usize]);
        // Deterministic candidate set: the grid cells this edge's own
        // bounding box touches, deduplicated, in sorted order.
        let lo = cell_of(&min_point(pa1, pa2));
        let hi = cell_of(&max_point(pa1, pa2));
        let mut candidates: Vec<usize> = Vec::new();
        for cx in lo[0]..=hi[0] {
            for cy in lo[1]..=hi[1] {
                for cz in lo[2]..=hi[2] {
                    if let Some(list) = buckets.get(&[cx, cy, cz]) {
                        candidates.extend_from_slice(list);
                    }
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        for &j in candidates.iter().filter(|&&j| j > i) {
            let (b1, b2) = edges[j];
            let (pb1, pb2) = (mesh.vertices[b1 as usize], mesh.vertices[b2 as usize]);
            // Comparable sampling densities only: welding pairs one
            // edge from each trace, so a pair whose edges differ in
            // length by more than a factor of two is not a counterpart
            // pair at all — it is a coarse trace's chord standing next
            // to a fine trace's segment, and merging their endpoints
            // would drag the fine geometry a whole chord length. (The
            // interleave pass is what reconciles density mismatches,
            // by inserting rather than merging.)
            let edge_i = (pa2 - pa1).length();
            let edge_j = (pb2 - pb1).length();
            if edge_i > 2.0 * edge_j || edge_j > 2.0 * edge_i {
                continue;
            }
            let d_a1 = segment_distance(pa1, pb1, pb2);
            let d_a2 = segment_distance(pa2, pb1, pb2);
            if d_a1 > reach || d_a2 > reach {
                continue;
            }
            let d_b1 = segment_distance(pb1, pa1, pa2);
            let d_b2 = segment_distance(pb2, pa1, pa2);
            if d_b1 > reach || d_b2 > reach {
                continue;
            }
            // Endpoint correspondence: straight or crossed, whichever
            // is shorter in total.
            let straight = (pa1 - pb1).length() + (pa2 - pb2).length();
            let crossed = (pa1 - pb2).length() + (pa2 - pb1).length();
            let (m_a1, m_a2, m_b1, m_b2) = if straight <= crossed {
                (a1, a2, b1, b2)
            } else {
                (a1, a2, b2, b1)
            };
            pairs.push((m_a1, m_b1));
            pairs.push((m_a2, m_b2));
            // Orientation: after the weld, edge i's owner must use the
            // merged edge opposite to edge j's owner. Owner i uses
            // (a1 -> a2) iff its stored direction matches that order.
            let (owner_i, forward_i) = owner
                .get(&(a1.min(a2), a1.max(a2)))
                .copied()
                .unwrap_or((usize::MAX, true));
            let (owner_j, forward_j) = owner
                .get(&(b1.min(b2), b1.max(b2)))
                .copied()
                .unwrap_or((usize::MAX, true));
            if owner_i != usize::MAX && owner_j != usize::MAX {
                // Does owner i run m_a1 -> m_a2?
                let i_forward = (forward_i && (a1, a2) == (m_a1, m_a2))
                    || (!forward_i && (a2, a1) == (m_a1, m_a2));
                // Does owner j run m_b1 -> m_b2? Note m_b1 corresponds
                // to m_a1 after the weld.
                let j_forward = (forward_j && (b1, b2) == (m_b1, m_b2))
                    || (!forward_j && (b2, b1) == (m_b1, m_b2));
                if i_forward == j_forward {
                    // They would agree; flip one owner's winding.
                    votes[owner_j] ^= true;
                }
            }
        }
    }
    for (i, vote) in votes.iter().enumerate() {
        if *vote {
            mesh.triangles[i].reverse();
        }
    }
    if pairs.is_empty() {
        return false;
    }
    // Union-find; weld each component to its smallest index.
    let vertex_count = mesh.vertices.len();
    let mut parent: Vec<u32> = (0..vertex_count as u32).collect();
    for &(v, a) in &pairs {
        let (rv, ra) = (find_root(&mut parent, v), find_root(&mut parent, a));
        if rv != ra {
            if rv < ra {
                parent[ra as usize] = rv;
            } else {
                parent[rv as usize] = ra;
            }
        }
    }
    let remap: Vec<u32> = (0..vertex_count as u32)
        .map(|i| find_root(&mut parent, i))
        .collect();
    let changed = remap.iter().enumerate().any(|(i, r)| *r != i as u32);
    if !changed {
        return false;
    }
    for triangle in &mut mesh.triangles {
        for index in triangle {
            *index = remap[*index as usize];
        }
    }
    drop_repeated_index_triangles(mesh, sides);
    collapse_duplicate_triangles(&mut mesh.triangles, sides);
    true
}

/// Union-find root with path halving.
fn find_root(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

/// Drop triangles below the input degeneracy bar (the same
/// `DEGENERATE_AREA` the clipper prunes on): the consensus snap welds
/// vertices that were a fraction of a sampling width apart, and the
/// triangles between them shrink under the bar. Their removal may
/// reopen edges, which the next cap round closes honestly.
fn prune_degenerate(mesh: &mut Mesh, sides: &mut Vec<Side>) {
    let keep: Vec<usize> = mesh
        .triangles
        .iter()
        .enumerate()
        .filter(|(_, t)| triangle_area_x2(mesh, t) >= 2.0 * DEGENERATE_AREA)
        .map(|(i, _)| i)
        .collect();
    if keep.len() == mesh.triangles.len() {
        return;
    }
    let triangles: Vec<[u32; 3]> = keep.iter().map(|&i| mesh.triangles[i]).collect();
    let new_sides: Vec<Side> = keep.iter().map(|&i| sides[i]).collect();
    mesh.triangles = triangles;
    *sides = new_sides;
}

/// Collapse pinholes: rims smaller than the consensus grid are
/// degenerate spots — loops of weld-noise edges around a point-sized
/// gap — not punctures to fan over. Merging all their vertices onto
/// one closes the gap directly; the surrounding slivers collapse into
/// repeated-index triangles and drop. Returns whether anything
/// collapsed.
fn collapse_pinholes(mesh: &mut Mesh, sides: &mut Vec<Side>) {
    let boundary = boundary_edges(mesh, sides);
    if boundary.is_empty() {
        return;
    }
    let grid = SNAP_GRID_FRACTION * seam_scale(mesh, &boundary);
    let rims = walk_boundary_rims(mesh, sides, &boundary);
    let mut parent: BTreeMap<u32, u32> = BTreeMap::new();
    let mut changed = false;
    for rim in &rims {
        let arc: f64 = rim
            .vertices
            .iter()
            .enumerate()
            .map(|(k, &v)| {
                let w = rim.vertices[(k + 1) % rim.vertices.len()];
                (mesh.vertices[w as usize] - mesh.vertices[v as usize]).length()
            })
            .sum();
        if arc > grid {
            continue;
        }
        let target = *rim.vertices.iter().min().expect("nonempty rim");
        for &v in &rim.vertices {
            if v != target {
                parent.insert(v, target);
                changed = true;
            }
        }
    }
    if !changed {
        return;
    }
    // Resolve to final targets (a target may itself be remapped).
    let resolve = |mut v: u32| -> u32 {
        while let Some(&next) = parent.get(&v) {
            v = next;
        }
        v
    };
    for triangle in &mut mesh.triangles {
        for index in triangle {
            *index = resolve(*index);
        }
    }
    drop_repeated_index_triangles(mesh, sides);
    collapse_duplicate_triangles(&mut mesh.triangles, sides);
}

/// Drop triangles that reference a vertex twice — the slivers a snap
/// weld collapses. Keeps `sides` in sync.
fn drop_repeated_index_triangles(mesh: &mut Mesh, sides: &mut Vec<Side>) {
    let mut keep: Vec<usize> = Vec::with_capacity(mesh.triangles.len());
    for (i, t) in mesh.triangles.iter().enumerate() {
        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
            keep.push(i);
        }
    }
    if keep.len() == mesh.triangles.len() {
        return;
    }
    let triangles: Vec<[u32; 3]> = keep.iter().map(|&i| mesh.triangles[i]).collect();
    let new_sides: Vec<Side> = keep.iter().map(|&i| sides[i]).collect();
    mesh.triangles = triangles;
    *sides = new_sides;
}

/// Volume sanity for the repair: removing sub-scale slivers and
/// replacing a band of surface with a ruled strip between the same rims
/// moves volume by a whisker; anything beyond a small fraction means
/// the pass fabricated structure, and the repair is rejected wholesale.
fn volume_within_guard(repaired: &Mesh, original: &Mesh) -> bool {
    let before = original.volume();
    let after = repaired.volume();
    (after - before).abs() <= SEAM_REPAIR_VOLUME_GUARD * before.abs() + PLANE_EPS
}

/// Drop vertices no triangle references (the ones the cut band left
/// behind), remapping indices in place. The survivors keep their
/// relative order.
fn compact_vertices(mesh: &mut Mesh) {
    let mut used = vec![false; mesh.vertices.len()];
    for triangle in &mesh.triangles {
        for &index in triangle {
            used[index as usize] = true;
        }
    }
    if used.iter().all(|u| *u) {
        return;
    }
    let mut remap = vec![u32::MAX; mesh.vertices.len()];
    let mut next = 0u32;
    for (i, is_used) in used.iter().enumerate() {
        if *is_used {
            remap[i] = next;
            next += 1;
        }
    }
    let vertices: Vec<Vector3> = mesh
        .vertices
        .iter()
        .enumerate()
        .filter(|(i, _)| used[*i])
        .map(|(_, v)| *v)
        .collect();
    for triangle in &mut mesh.triangles {
        for index in triangle {
            *index = remap[*index as usize];
        }
    }
    mesh.vertices = vertices;
}

/// Array index of a [`Side`] for the per-side edge vectors.
fn index_of(side: Side) -> usize {
    match side {
        Side::A => 0,
        Side::B => 1,
    }
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

    /// Classify `polygon` against this plane without splitting: a
    /// bitmask of the vertex sides (`FRONT_BIT`/`BACK_BIT`), used by the
    /// splitter scorer.
    fn classify(&self, polygon: &Polygon) -> u8 {
        let mut mask = 0;
        for vertex in &polygon.vertices {
            let t = self.normal.dot(vertex) - self.w;
            if t > PLANE_EPS {
                mask |= FRONT_BIT;
            } else if t < -PLANE_EPS {
                mask |= BACK_BIT;
            }
        }
        mask
    }

    /// Split `polygon` by this plane.
    fn split_polygon(&self, polygon: Polygon) -> Split {
        const COPLANAR: u8 = 0;
        const FRONT: u8 = 1;
        const BACK: u8 = 2;

        let origin = polygon.side;
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
                    Split::CoplanarFront(Polygon {
                        vertices,
                        side: origin,
                    })
                } else {
                    Split::CoplanarBack(Polygon {
                        vertices,
                        side: origin,
                    })
                }
            }
            FRONT => Split::Front(Polygon {
                vertices,
                side: origin,
            }),
            BACK => Split::Back(Polygon {
                vertices,
                side: origin,
            }),
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
                    front: new_polygon(front, origin),
                    back: new_polygon(back, origin),
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
/// like the triangle it came from, with the operand it originated from.
#[derive(Debug, Clone, PartialEq)]
struct Polygon {
    vertices: Vec<Vector3>,
    side: Side,
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
fn new_polygon(vertices: Vec<Vector3>, side: Side) -> Option<Polygon> {
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
        Some(Polygon {
            vertices: distinct,
            side,
        })
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

/// Pick the splitting plane for a node: the best-scoring of (up to 8)
/// evenly-spaced polygon planes; when a **large** set is one-sided under
/// every polygon plane (every surface triangle's plane is tangent-ish —
/// exactly the tessellated-sphere case, whose back chain would otherwise
/// become a linked list thousands of levels deep and overflow a standard
/// 2 MiB thread), the axis-aligned midplanes of the bounding box join
/// the scoring and halve the set instead.
fn choose_splitter(polygons: &[Polygon]) -> Option<Plane> {
    // Splits cost double (they duplicate geometry and fan out the tree);
    // imbalance keeps the sides even.
    let score_of = |plane: &Plane| -> f64 {
        let (mut front, mut back, mut splits) = (0usize, 0usize, 0usize);
        for polygon in polygons {
            match plane.classify(polygon) {
                FRONT_BIT => front += 1,
                BACK_BIT => back += 1,
                0 => {} // coplanar with the candidate
                _ => splits += 1,
            }
        }
        2.0 * splits as f64 + front.abs_diff(back) as f64
    };

    let step = (polygons.len() / 8).max(1);
    let mut best: Option<(f64, Plane)> = None;
    for candidate in polygons.iter().step_by(step) {
        let Some(plane) = candidate.plane() else {
            continue;
        };
        let score = score_of(&plane);
        if best.map_or(true, |(best_score, _)| score < best_score) {
            best = Some((score, plane));
        }
        // A perfect splitter is not going to be beaten.
        if score == 0.0 {
            return best.map(|(_, plane)| plane);
        }
    }

    // Large + one-sided: no polygon plane divides the set; halve it with
    // a bounding-box midplane instead of degenerating into a chain.
    if polygons.len() > 512 {
        if let Some((score, _)) = best {
            if score >= 0.75 * polygons.len() as f64 {
                let (mut lo, mut hi) = (
                    Vector3::new(f64::MAX, f64::MAX, f64::MAX),
                    Vector3::new(f64::MIN, f64::MIN, f64::MIN),
                );
                for polygon in polygons {
                    for vertex in &polygon.vertices {
                        lo = min_point(lo, *vertex);
                        hi = max_point(hi, *vertex);
                    }
                }
                for (normal, min, max) in [
                    (Vector3::new(1.0, 0.0, 0.0), lo.x, hi.x),
                    (Vector3::new(0.0, 1.0, 0.0), lo.y, hi.y),
                    (Vector3::new(0.0, 0.0, 1.0), lo.z, hi.z),
                ] {
                    if max.is_finite() && min.is_finite() && max - min > PLANE_EPS {
                        let plane = Plane {
                            normal,
                            w: (min + max) / 2.0,
                        };
                        let mid_score = score_of(&plane);
                        if mid_score < score {
                            best = Some((mid_score, plane));
                        }
                    }
                }
            }
        }
    }
    best.map(|(_, plane)| plane)
}

impl Node {
    /// Insert polygons into the (sub)tree, splitting them as needed.
    fn build(&mut self, polygons: Vec<Polygon>) {
        if polygons.is_empty() {
            return;
        }
        let plane = match self.plane {
            Some(plane) => plane,
            None => match choose_splitter(&polygons) {
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

    /// The analytic intersection (lens) volume of two spheres with
    /// radii `r1`, `r2`, centers `d` apart (`|r1-r2| < d < r1+r2`).
    /// Limits: `d -> r1+r2` gives 0, and for `r1 == r2` the formula
    /// reduces to `pi*(4+d)*(2-d)^2/12`.
    fn lens_volume(r1: f64, r2: f64, d: f64) -> f64 {
        let pi = std::f64::consts::PI;
        pi * (r1 + r2 - d).powi(2) * (d * d + 2.0 * d * (r1 + r2) - 3.0 * (r1 - r2).powi(2))
            / (12.0 * d)
    }

    /// A unit sphere at a chosen deflection, offset by `(dx, dy, dz)`
    /// — the operand every curved-seam test uses.
    fn sphere_at(deflection: f64, dx: f64, dy: f64, dz: f64) -> Mesh {
        let mut mesh = TopoShape::Sphere { radius: 1.0 }.tessellate(deflection);
        for v in &mut mesh.vertices {
            v.x += dx;
            v.y += dy;
            v.z += dz;
        }
        mesh
    }

    /// The honest volume tolerance for a boolean of tessellated
    /// operands: three times the operands' own tessellation volume
    /// error (an intersection of two under-sampled spheres
    /// under-samples roughly twice over) plus a percent of seam-repair
    /// allowance. Derived from the deflection through the operands'
    /// own meshes, never a magic constant.
    fn derived_tolerance(operands: &[&Mesh], exact: &[f64]) -> f64 {
        let operand_error = operands
            .iter()
            .zip(exact)
            .map(|(mesh, want)| ((mesh.volume() - want) / want).abs())
            .fold(0.0, f64::max);
        3.0 * operand_error + 0.01
    }

    fn assert_closed_positive_near(result: &Mesh, expected: f64, tolerance: f64, what: &str) {
        assert!(result.triangle_count() > 0, "{what}: no triangles");
        assert!(mesh_is_closed(result), "{what}: mesh must be watertight");
        let got = result.volume();
        assert!(got > 0.0, "{what}: volume must be positive, got {got}");
        assert!(
            (got - expected).abs() <= tolerance * expected,
            "{what}: volume {got} vs expected {expected} (tolerance {tolerance})"
        );
    }

    /// No degenerate triangles: every triangle clears the same
    /// `DEGENERATE_AREA` bar the clipper prunes on input.
    fn assert_no_degenerate(result: &Mesh, what: &str) {
        for (i, t) in result.triangles.iter().enumerate() {
            let (a, b, c) = (
                result.vertices[t[0] as usize],
                result.vertices[t[1] as usize],
                result.vertices[t[2] as usize],
            );
            assert!(
                (b - a).cross(&(c - a)).length() >= 2.0 * DEGENERATE_AREA,
                "{what}: triangle {i} is degenerate"
            );
        }
    }

    #[test]
    fn boolean_sphere_sphere_fuse_cut_common_watertight_and_honest() {
        let pi = std::f64::consts::PI;
        // Two overlapping unit spheres; d = sqrt(1.21) is comfortably
        // inside (0, 2). This is the scenario that failed on main:
        // the curved seam's two tessellated traces never bit-welded
        // and the fuse came out open (hundreds of boundary edges at
        // this deflection before the repair).
        let a = sphere_at(0.05, 0.0, 0.0, 0.0);
        let b = sphere_at(0.05, 1.0, 0.2, 0.1);
        let d = (1.0_f64 * 1.0 + 0.2 * 0.2 + 0.1 * 0.1).sqrt();
        let lens = lens_volume(1.0, 1.0, d);
        let sphere = 4.0 * pi / 3.0;
        let tolerance = derived_tolerance(&[&a, &b], &[sphere, sphere]);
        assert!(tolerance < 0.03, "fine operands keep the bound tight");

        let fused = fuse(&a, &b);
        assert_closed_positive_near(&fused, 2.0 * sphere - lens, tolerance, "sphere fuse sphere");
        let cut = cut(&a, &b);
        assert_closed_positive_near(&cut, sphere - lens, tolerance, "sphere cut sphere");
        let shared = common(&a, &b);
        assert_closed_positive_near(&shared, lens, tolerance, "sphere common sphere");

        // The repair only engages on open seams; verify it did engage
        // by checking the un-repaired pipeline really is open here.
        let polygons = stitch_t_junctions(op_result(
            polygons_of(&a, Side::A),
            polygons_of(&b, Side::B),
            Op::Union,
        ));
        let (raw, _) = mesh_from_sided(polygons);
        assert!(
            !mesh_is_closed(&raw),
            "the scenario must exercise the repair (it closed bit-exactly?)"
        );
    }

    #[test]
    fn boolean_sphere_sphere_different_radii_and_default_deflection() {
        let pi = std::f64::consts::PI;
        // Radii 1.0 and 0.7 at the crate's default deflection: the
        // coarse operands dominate the volume error, so the tolerance
        // is derived from them, not hand-picked.
        let a = sphere_at(DEFAULT_DEFLECTION, 0.0, 0.0, 0.0);
        let b = {
            let mut mesh = TopoShape::Sphere { radius: 0.7 }.tessellate(DEFAULT_DEFLECTION);
            for v in &mut mesh.vertices {
                v.x += 0.9;
                v.y += 0.15;
            }
            mesh
        };
        let d = (0.9_f64 * 0.9 + 0.15 * 0.15).sqrt();
        let lens = lens_volume(1.0, 0.7, d);
        let tolerance = derived_tolerance(&[&a, &b], &[4.0 * pi / 3.0, 4.0 * pi / 3.0 * 0.343]);
        let fused = fuse(&a, &b);
        assert_closed_positive_near(
            &fused,
            4.0 * pi / 3.0 * 1.343 - lens,
            tolerance,
            "radii fuse",
        );
        let shared = common(&a, &b);
        assert_closed_positive_near(&shared, lens, tolerance, "radii common");
    }

    #[test]
    fn boolean_sphere_cut_by_centered_box_is_half() {
        // A box through the sphere's center removes exactly half of it
        // — the symmetric curved-vs-planar case with a closed-form
        // answer.
        let pi = std::f64::consts::PI;
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.12);
        let tool = TopoShape::Box {
            length: 4.0,
            width: 4.0,
            height: 2.0,
        }
        .tessellate(DEFAULT_DEFLECTION);
        let half_sphere = 2.0 * pi / 3.0;
        let tolerance = derived_tolerance(&[&sphere], &[4.0 * pi / 3.0]);
        let result = cut(&sphere, &tool);
        assert_closed_positive_near(&result, half_sphere, tolerance, "sphere cut half");
    }

    #[test]
    fn boolean_sphere_cut_by_box_cap() {
        // The off-center case: cutting away the cap of height 0.5.
        let pi = std::f64::consts::PI;
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.12);
        let mut tool = TopoShape::Box {
            length: 4.0,
            width: 4.0,
            height: 1.0,
        }
        .tessellate(DEFAULT_DEFLECTION);
        for v in &mut tool.vertices {
            v.z += 0.5;
        }
        let h = 0.5_f64;
        let cap = pi * h * h * (3.0 - h) / 3.0;
        let tolerance = derived_tolerance(&[&sphere], &[4.0 * pi / 3.0]);
        let result = cut(&sphere, &tool);
        assert_closed_positive_near(
            &result,
            4.0 * pi / 3.0 - cap,
            tolerance,
            "sphere cut box cap",
        );
    }

    #[test]
    fn boolean_cylinder_sphere_aligned_axes() {
        // A cylinder along Z through the sphere's center: the shared
        // core is the spherical segment inside radius a,
        // (4*pi/3)*(1 - (1-a^2)^(3/2)).
        let pi = std::f64::consts::PI;
        let cylinder = TopoShape::Cylinder {
            radius: 0.6,
            height: 4.0,
        }
        .tessellate(0.1);
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.1);
        let core = 4.0 * pi / 3.0 * (1.0 - (1.0 - 0.36_f64).powf(1.5));
        let tolerance =
            derived_tolerance(&[&cylinder, &sphere], &[pi * 0.36 * 4.0, 4.0 * pi / 3.0]);
        let fused = fuse(&cylinder, &sphere);
        assert_closed_positive_near(
            &fused,
            pi * 0.36 * 4.0 + 4.0 * pi / 3.0 - core,
            tolerance,
            "cylinder fuse sphere",
        );
        let cored = cut(&sphere, &cylinder);
        assert_closed_positive_near(
            &cored,
            4.0 * pi / 3.0 - core,
            tolerance,
            "sphere cut cylinder",
        );
        let shared = common(&cylinder, &sphere);
        assert_closed_positive_near(&shared, core, tolerance, "cylinder common sphere");
    }

    #[test]
    fn boolean_planar_cases_stay_bit_exact() {
        // Planar regression: where the bit-exact weld already closes
        // the output (box-box), the repair must not touch a single
        // byte — same triangles, same vertices, in the same order, as
        // the bare BSP + stitch + weld pipeline produces.
        let (a, b) = overlapping_boxes();
        for (result, op, what) in [
            (fuse(&a, &b), Op::Union, "fuse"),
            (cut(&a, &b), Op::Subtract, "cut"),
            (common(&a, &b), Op::Intersect, "common"),
        ] {
            let polygons = stitch_t_junctions(op_result(
                polygons_of(&a, Side::A),
                polygons_of(&b, Side::B),
                op,
            ));
            let (bare, _) = mesh_from_sided(polygons);
            assert_eq!(
                result.triangles, bare.triangles,
                "planar {what}: triangle list must be bit-identical"
            );
            assert_eq!(
                result.vertices, bare.vertices,
                "planar {what}: vertex list must be bit-identical"
            );
        }
        // And the exact values the planar tests have always asserted.
        assert_volume(&fuse(&a, &b), VA + VB - OVERLAP, 1e-12, "planar fuse");
        assert_volume(&cut(&a, &b), VA - OVERLAP, 1e-12, "planar cut");
        assert_volume(&common(&a, &b), OVERLAP, 1e-12, "planar common");
    }

    #[test]
    fn boolean_curved_outputs_have_no_degenerate_triangles() {
        let a = sphere_at(0.05, 0.0, 0.0, 0.0);
        let b = sphere_at(0.05, 1.0, 0.2, 0.1);
        assert_no_degenerate(&fuse(&a, &b), "sphere fuse");
        assert_no_degenerate(&cut(&a, &b), "sphere cut");
        assert_no_degenerate(&common(&a, &b), "sphere common");
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.12);
        let mut tool = TopoShape::Box {
            length: 4.0,
            width: 4.0,
            height: 1.0,
        }
        .tessellate(DEFAULT_DEFLECTION);
        for v in &mut tool.vertices {
            v.z += 0.5;
        }
        assert_no_degenerate(&cut(&sphere, &tool), "sphere cut box");
        let cylinder = TopoShape::Cylinder {
            radius: 0.6,
            height: 4.0,
        }
        .tessellate(0.1);
        let sph = TopoShape::Sphere { radius: 1.0 }.tessellate(0.1);
        assert_no_degenerate(&fuse(&cylinder, &sph), "cylinder fuse sphere");
        assert_no_degenerate(&common(&cylinder, &sph), "cylinder common sphere");
    }

    #[test]
    fn boolean_sphere_sphere_is_bit_deterministic() {
        let a = sphere_at(0.05, 0.0, 0.0, 0.0);
        let b = sphere_at(0.05, 1.0, 0.2, 0.1);
        let first = fuse(&a, &b);
        let second = fuse(&a, &b);
        assert_eq!(
            first.triangle_count(),
            second.triangle_count(),
            "triangle counts must match"
        );
        assert_eq!(
            first.vertices, second.vertices,
            "vertex sequences must be bit-equal"
        );
        assert_eq!(
            first.triangles, second.triangles,
            "triangle sequences must be bit-equal"
        );
        let cut_first = cut(&a, &b);
        let cut_second = cut(&a, &b);
        assert_eq!(cut_first.vertices, cut_second.vertices);
        assert_eq!(cut_first.triangles, cut_second.triangles);
    }

    #[test]
    fn boolean_near_tangent_sphere_pair_is_honest() {
        // d = 1.9, just inside r1 + r2 = 2: a lens 0.1 thick. Fuse and
        // cut must close; the common (the lens itself) is thinner than
        // a coarse tessellation's chord, so at deflection 0.1 the
        // honest outcome is the documented one: the repair reverts and
        // the lens stays open rather than fabricating structure. At a
        // deflection that resolves the lens (0.05), it closes within
        // the derived tolerance.
        let pi = std::f64::consts::PI;
        let lens = lens_volume(1.0, 1.0, 1.9);
        let a = sphere_at(0.1, 0.0, 0.0, 0.0);
        let b = sphere_at(0.1, 1.9, 0.0, 0.0);
        let fused = fuse(&a, &b);
        assert!(mesh_is_closed(&fused), "near-tangent fuse must close");
        let cut = cut(&a, &b);
        assert!(mesh_is_closed(&cut), "near-tangent cut must close");
        assert!(fused.volume().is_finite() && cut.volume().is_finite());
        // Coarse lens: no panic, finite volume; open is the documented
        // honest outcome (sub-resolution geometry).
        let coarse = common(&a, &b);
        assert!(coarse.volume().is_finite());
        // Fine lens: resolved and closed.
        let a = sphere_at(0.05, 0.0, 0.0, 0.0);
        let b = sphere_at(0.05, 1.9, 0.0, 0.0);
        let fine = common(&a, &b);
        // Near tangency the chord error concentrates: the lens is a
        // sliver of the very surface region where both spheres are
        // flattest, so its relative volume error is an order of
        // magnitude above the operands' bulk error. The bound widens
        // accordingly (still deflection-derived, not hand-picked).
        let tolerance = 10.0 * derived_tolerance(&[&a, &b], &[4.0 * pi / 3.0, 4.0 * pi / 3.0]);
        assert_closed_positive_near(&fine, lens, tolerance, "near-tangent common (fine)");
    }

    /// Two perpendicular equal cylinders, A along Z and B along Y (A
    /// rotated a quarter turn about X). A tilted mesh, like the probe
    /// below builds, has no exact form — this construction keeps the
    /// classic ones.
    fn crossed_cylinders(deflection: f64) -> (Mesh, Mesh) {
        let a = TopoShape::Cylinder {
            radius: 1.0,
            height: 4.0,
        }
        .tessellate(deflection);
        let mut b = TopoShape::Cylinder {
            radius: 1.0,
            height: 4.0,
        }
        .tessellate(deflection);
        let (s, c) = (
            f64::sin(std::f64::consts::FRAC_PI_2),
            f64::cos(std::f64::consts::FRAC_PI_2),
        );
        for v in &mut b.vertices {
            let y = v.y * c - v.z * s;
            let z = v.y * s + v.z * c;
            v.y = y;
            v.z = z;
        }
        (a, b)
    }

    /// The seam of two crossed cylinders is two closed space curves
    /// that meet at two points — not the single closed loop of a
    /// sphere pair. Near the meeting points the two branches come
    /// within a sampling width of each other, which is exactly where
    /// a tolerance derived from anything but the seam's own sampling
    /// step welds across structure.
    #[test]
    fn boolean_steinmetz_crossed_cylinders_exact_volume() {
        let pi = std::f64::consts::PI;
        let (a, b) = crossed_cylinders(0.08);
        // The bicylinder is 16 r^3 / 3 (r = 1); both finite cylinders
        // (height 4, so [-2, 2]) fully contain it.
        let bicylinder = 16.0 / 3.0;
        let cylinder = pi * 4.0;
        let tolerance = derived_tolerance(&[&a, &b], &[cylinder, cylinder]);
        assert_closed_positive_near(
            &common(&a, &b),
            bicylinder,
            tolerance,
            "crossed cylinders common (Steinmetz)",
        );
        assert_closed_positive_near(
            &fuse(&a, &b),
            2.0 * cylinder - bicylinder,
            tolerance,
            "crossed cylinders fuse",
        );
        assert_closed_positive_near(
            &cut(&a, &b),
            cylinder - bicylinder,
            tolerance,
            "crossed cylinders cut",
        );
        assert_no_degenerate(&common(&a, &b), "crossed cylinders common");
    }

    /// Two parallel cylinders with offset axes: the common is the
    /// extruded circle-circle lens, a closed form. The seam is two
    /// straight vertical lines — where both walls' discretizations
    /// disagree by up to their chord errors — plus the lens arcs on
    /// the coincident cap planes.
    #[test]
    fn boolean_parallel_offset_cylinders_lens_volume() {
        let pi = std::f64::consts::PI;
        let deflection = 0.04;
        let a = TopoShape::Cylinder {
            radius: 1.0,
            height: 4.0,
        }
        .tessellate(deflection);
        let mut b = TopoShape::Cylinder {
            radius: 0.7,
            height: 4.0,
        }
        .tessellate(deflection);
        for v in &mut b.vertices {
            v.x += 0.8;
        }
        // Lens of circles r1 = 1, r2 = 0.7, centers 0.8 apart,
        // extruded over the common height 4.
        let (r1, r2, d) = (1.0_f64, 0.7_f64, 0.8_f64);
        let lens = 4.0
            * (r1 * r1 * ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1)).acos()
                + r2 * r2 * ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2)).acos()
                - 0.5 * ((-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2)).sqrt());
        let cyl_a = pi * 4.0;
        let cyl_b = pi * 0.49 * 4.0;
        let tolerance = derived_tolerance(&[&a, &b], &[cyl_a, cyl_b]);
        assert_closed_positive_near(&common(&a, &b), lens, tolerance, "lens common");
        assert_closed_positive_near(&fuse(&a, &b), cyl_a + cyl_b - lens, tolerance, "lens fuse");
        assert_closed_positive_near(&cut(&a, &b), cyl_a - lens, tolerance, "lens cut");
        assert_no_degenerate(&common(&a, &b), "lens common");
    }

    /// A cylinder tilted 30 degrees through a sphere: a genuine space
    /// curve seam with no closed form. The honest assertions are the
    /// structural ones — watertight, no degenerate pieces, and the
    /// boolean identities that an exact kernel satisfies: cut + common
    /// partitions the first operand.
    #[test]
    fn boolean_tilted_cylinder_sphere_skew_seam() {
        let pi = std::f64::consts::PI;
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.1);
        let mut cylinder = TopoShape::Cylinder {
            radius: 0.55,
            height: 4.0,
        }
        .tessellate(0.1);
        let (s, c) = ((pi / 6.0).sin(), (pi / 6.0).cos());
        for v in &mut cylinder.vertices {
            let y = v.y * c - v.z * s;
            let z = v.y * s + v.z * c;
            v.y = y;
            v.z = z;
        }
        let fused = fuse(&cylinder, &sphere);
        let holed = cut(&sphere, &cylinder);
        let core = common(&cylinder, &sphere);
        assert!(mesh_is_closed(&fused), "tilted fuse must be watertight");
        assert!(mesh_is_closed(&holed), "tilted cut must be watertight");
        assert!(mesh_is_closed(&core), "tilted common must be watertight");
        assert_no_degenerate(&fused, "tilted fuse");
        assert_no_degenerate(&holed, "tilted cut");
        assert_no_degenerate(&core, "tilted common");
        // Partition identities, both directions.
        let sphere_exact = 4.0 * pi / 3.0;
        let cylinder_exact = pi * 0.55 * 0.55 * 4.0;
        let tolerance = derived_tolerance(&[&cylinder, &sphere], &[cylinder_exact, sphere_exact]);
        let got = holed.volume() + core.volume();
        assert!(
            (got - sphere_exact).abs() <= tolerance * sphere_exact,
            "cut + common must partition the sphere: {got} vs {sphere_exact}"
        );
        let got = cut(&cylinder, &sphere).volume() + core.volume();
        assert!(
            (got - cylinder_exact).abs() <= tolerance * cylinder_exact,
            "cut + common must partition the cylinder: {got} vs {cylinder_exact}"
        );
        // The union swallows both operands.
        assert!(
            fused.volume() >= cylinder_exact - tolerance * cylinder_exact
                && fused.volume() >= sphere_exact - tolerance * sphere_exact,
            "fuse must contain both operands: {}",
            fused.volume()
        );
    }

    /// A cone tilted 36 degrees against a sphere: mixed primitives,
    /// mixed curvatures, no symmetry at all.
    #[test]
    fn boolean_tilted_cone_sphere() {
        let pi = std::f64::consts::PI;
        let deflection = 0.08;
        let mut cone = TopoShape::Cone {
            radius1: 0.9,
            radius2: 0.1,
            height: 2.5,
        }
        .tessellate(deflection);
        let (s, c) = ((pi / 5.0).sin(), (pi / 5.0).cos());
        for v in &mut cone.vertices {
            let y = v.y * c - v.z * s;
            let z = v.y * s + v.z * c;
            v.y = y;
            v.z = z;
        }
        let sphere = TopoShape::Sphere { radius: 1.1 }.tessellate(deflection);
        let fused = fuse(&cone, &sphere);
        let core = common(&cone, &sphere);
        let holed = cut(&sphere, &cone);
        assert!(
            mesh_is_closed(&fused),
            "tilted cone fuse must be watertight"
        );
        assert!(
            mesh_is_closed(&core),
            "tilted cone common must be watertight"
        );
        assert!(mesh_is_closed(&holed), "tilted cone cut must be watertight");
        assert_no_degenerate(&fused, "tilted cone fuse");
        assert_no_degenerate(&core, "tilted cone common");
        let sphere_exact = 4.0 * pi * 1.1 * 1.1 * 1.1 / 3.0;
        let cone_exact = pi * (0.9 * 0.9 + 0.9 * 0.1 + 0.1 * 0.1) * 2.5 / 3.0;
        let tolerance = derived_tolerance(&[&cone, &sphere], &[cone_exact, sphere_exact]);
        let got = holed.volume() + core.volume();
        assert!(
            (got - sphere_exact).abs() <= tolerance * sphere_exact,
            "cut + common must partition the sphere: {got} vs {sphere_exact}"
        );
    }

    /// Two cylinders on skew axes (45 and -30 degrees, offset in Z):
    /// every seam here is a closed space curve neither axis-aligned
    /// nor planar.
    #[test]
    fn boolean_skew_cylinder_pair() {
        let pi = std::f64::consts::PI;
        let deflection = 0.08;
        let mut a = TopoShape::Cylinder {
            radius: 0.8,
            height: 4.0,
        }
        .tessellate(deflection);
        let mut b = TopoShape::Cylinder {
            radius: 0.6,
            height: 4.0,
        }
        .tessellate(deflection);
        let (s, c) = ((pi / 4.0).sin(), (pi / 4.0).cos());
        for v in &mut a.vertices {
            let x = v.x * c - v.y * s;
            let y = v.x * s + v.y * c;
            v.x = x;
            v.y = y;
        }
        let (s, c) = ((-pi / 6.0).sin(), (-pi / 6.0).cos());
        for v in &mut b.vertices {
            let y = v.y * c - v.z * s;
            let z = v.y * s + v.z * c;
            v.y = y;
            v.z = z;
            v.z += 0.3;
        }
        let fused = fuse(&a, &b);
        let core = common(&a, &b);
        let holed = cut(&a, &b);
        assert!(mesh_is_closed(&fused), "skew fuse must be watertight");
        assert!(mesh_is_closed(&core), "skew common must be watertight");
        assert!(mesh_is_closed(&holed), "skew cut must be watertight");
        assert_no_degenerate(&fused, "skew fuse");
        assert_no_degenerate(&core, "skew common");
        let cyl_a = pi * 0.64 * 4.0;
        let tolerance = derived_tolerance(&[&a, &b], &[cyl_a, pi * 0.36 * 4.0]);
        let got = holed.volume() + core.volume();
        assert!(
            (got - cyl_a).abs() <= tolerance * cyl_a,
            "cut + common must partition A: {got} vs {cyl_a}"
        );
    }

    /// The mixed-curved repairs must stay bit-deterministic: same
    /// operands, same bytes, twice — including the two-branch seam of
    /// the crossed cylinders and the tilted space-curve seam.
    #[test]
    fn boolean_mixed_curved_is_bit_deterministic() {
        let (a, b) = crossed_cylinders(0.08);
        let first = common(&a, &b);
        let second = common(&a, &b);
        assert_eq!(first.vertices, second.vertices);
        assert_eq!(first.triangles, second.triangles);

        let pi = std::f64::consts::PI;
        let sphere = TopoShape::Sphere { radius: 1.0 }.tessellate(0.1);
        let mut cylinder = TopoShape::Cylinder {
            radius: 0.55,
            height: 4.0,
        }
        .tessellate(0.1);
        let (s, c) = ((pi / 6.0).sin(), (pi / 6.0).cos());
        for v in &mut cylinder.vertices {
            let y = v.y * c - v.z * s;
            let z = v.y * s + v.z * c;
            v.y = y;
            v.z = z;
        }
        let first = fuse(&cylinder, &sphere);
        let second = fuse(&cylinder, &sphere);
        assert_eq!(first.vertices, second.vertices);
        assert_eq!(first.triangles, second.triangles);
    }
}
