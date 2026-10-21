//! Profile kernels: closed-wire extraction, extrusion (Pad) and revolution
//! (Revolution) — the `BRepPrimAPI_MakePrism` / `BRepPrimAPI_MakeRevol`
//! slice of FreeCAD's PartDesign pipeline, distilled for a tessellated mesh
//! kernel.
//!
//! FreeCAD's PartDesign workflow is: solve a 2D sketch, extract its closed
//! wires, then extrude or revolve them into a solid. OCCT does this on exact
//! BRep geometry; this module is the honest tessellated equivalent —
//! tessellated wires in, watertight triangle [`Mesh`] out. Everything here is
//! a pure function: no document objects, no CLI (the PartDesign features wire
//! these kernels into the recompute loop).
//!
//! # Wire extraction ([`extract_wires`])
//!
//! Line segments contribute their two endpoints as an edge; arcs contribute a
//! sagitta-tessellated polyline (segment count from the deflection rule
//! below); full circles contribute a closed loop of their own and never enter
//! chaining. `Geometry::Point` items are *not* edges and are ignored.
//!
//! Edges are chained head-to-tail into closed loops, matching endpoints
//! within a geometric tolerance ([`DEFAULT_TOLERANCE`], FreeCAD's
//! `Precision::Confusion`). Any dangling endpoint makes the chain open and
//! aborts extraction with [`ProfileError::OpenWire`], listing every free
//! endpoint. When three or more edge endpoints meet at one point within the
//! tolerance the match is *first come, first served*: chains are scanned in
//! creation order and each chain's back endpoint is tested before its front,
//! so the earliest-created chain wins. The rule is fully deterministic.
//!
//! Arc/circle segment counts follow the kernel sagitta rule
//! `n = max(min_segments, ceil(sweep / (2 * acos(max(0, 1 - deflection/r)))))`
//! with a floor of 4 segments per arc and 16 per circle (safety cap 8192), so
//! the chord error of every tessellated segment stays `<= deflection`.
//!
//! # Nesting classification ([`classify_wires`])
//!
//! Wires are classified by containment depth: a wire whose representative
//! point (its first vertex) lies inside exactly `k` other wires has depth
//! `k`. Even depth = outer boundary, odd depth = hole. [`classify_wires`]
//! re-orients every wire accordingly — outer boundaries CCW, holes CW
//! (FreeCAD `Part::Face` semantics) — and returns them in input order.
//! Containment is a deterministic even-odd ray cast; a representative point
//! exactly on another wire's boundary counts as *outside*.
//!
//! # Extrusion ([`extrude`], the Pad kernel)
//!
//! The profile's 2D points are interpreted as XY sketch coordinates at
//! `z = 0`; the result is built there and then translated (never
//! re-oriented) by the midplane offset. `midplane == false` extrudes from
//! the base plane to `+length * dir`; `midplane == true` extrudes from
//! `-length/2` to `+length/2` along `dir`. Side walls are one quad (two
//! triangles) per wire segment, wound so face normals point away from the
//! material (outward = right of travel for CCW outers, into the cavity for
//! CW holes). Caps are ear-clipped per region — outer wire plus its holes —
//! with every hole bridged into the outer boundary by a zero-width cut
//! through a mutually visible vertex pair before clipping. Caps and walls
//! share the *same* vertex indices, so the mesh is watertight by
//! construction with no duplicated coordinates.
//!
//! # Revolution ([`revolve`], the Revolution kernel)
//!
//! The 2D profile `(u, v)` is mapped into the plane containing the axis:
//! `p = axis_base + u*e1 + v*axis_dir` where `e1` is a canonical unit vector
//! perpendicular to the axis (`e1 = +X` when the axis is `+Z`, giving the
//! classic lathe setup: profile in the XZ half-plane, axis = Z). A full turn
//! (`|angle| >= 360 - 1e-9`) is seam-welded: vertices are generated once per
//! profile point per ring and ring index arithmetic wraps modulo the segment
//! count, so no duplicated seam vertices exist. Profile points within 1e-9 of
//! the axis stay fixed as single shared vertices; edges lying entirely on the
//! axis sweep nothing and every other collapse is thereby excluded by
//! construction — the output contains no zero-area triangles. A partial turn
//! additionally caps the two flat side walls with the same ear-clip +
//! bridging machinery as the extrusion caps. The sign of the angle is the
//! sweep direction (negative = clockwise around the axis); the winding rules
//! flip with it so [`Mesh::volume`] stays positive for CCW-classified
//! profiles.
//!
//! # Honest limitations (v1)
//!
//! * Wire self-intersection and wire/wire intersection are not validated —
//!   a self-intersecting profile sweeps a self-intersecting solid (the
//!   caller's problem, exactly as in FreeCAD).
//! * Region grouping supports one nesting level (outers + their direct
//!   holes), like FreeCAD's `FaceMakerCheese`; deeper nesting is rejected.
//! * Extruding along a direction parallel to the sketch plane, or along a
//!   wire segment, is degenerate and rejected with a clear error.
//! * A profile lying entirely on the revolution axis has nothing to sweep
//!   and is rejected.
//! * Regions must not touch: an outer wire and its hole sharing a point
//!   breaks the region-boundary assumptions.
//!
//! Clean-room note: the behavior distilled here is described by the work
//! order alone; no FreeCAD or OCCT code was read or used.

use std::fmt;

use fc_base::Vector3;
use fc_sketcher::geometry::Geometry;

use crate::shape::Mesh;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Endpoint matching tolerance for wire chaining — FreeCAD's
/// `Precision::Confusion` value.
pub const DEFAULT_TOLERANCE: f64 = 1e-7;

/// Sagitta (max chordal deviation) used for arc/circle tessellation and
/// revolution sweeps when a caller doesn't care.
pub const DEFAULT_PROFILE_DEFLECTION: f64 = 0.01;

/// Two pi, spelled out once.
const TAU: f64 = 2.0 * std::f64::consts::PI;

/// Minimum tessellated segments per arc (work-order floor).
const MIN_ARC_SEGMENTS: usize = 4;
/// Minimum tessellated segments per full circle (work-order floor).
const MIN_CIRCLE_SEGMENTS: usize = 16;
/// Safety cap on arc/circle tessellation so a microscopic deflection cannot
/// request unbounded memory. Far above any sensible sketch.
const MAX_ARC_SEGMENTS: usize = 8192;
/// Minimum rings for a revolution sweep.
const MIN_SWEEP_SEGMENTS: usize = 16;
/// Cap on revolution rings (work-order suggestion).
const MAX_SWEEP_SEGMENTS: usize = 512;

/// Profile points closer than this to the revolution axis are treated as
/// exactly on it (single fixed vertex).
const AXIS_SNAP: f64 = 1e-9;
/// Zero-area epsilon for ear clipping (work-order value).
const AREA_EPS: f64 = 1e-12;
/// Consecutive wire points closer than this are rejected as degenerate.
const POINT_DUP_EPS: f64 = 1e-12;
/// Clearance epsilon for bridge-visibility segment tests.
const VISIBILITY_EPS: f64 = 1e-9;
/// `|angle_deg|` within this of 360 is a full, seam-welded turn.
const FULL_TURN_EPS_DEG: f64 = 1e-9;

// ---------------------------------------------------------------------------
// 2D point
// ---------------------------------------------------------------------------

/// A 2D point in the sketch frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2 {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

impl Point2 {
    /// Constructs a point from coordinates.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// True when both coordinates are finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Euclidean distance to `other`.
    pub fn dist(self, other: Self) -> f64 {
        (self - other).length()
    }

    /// Length of the point interpreted as a vector.
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    /// Z component of the 3D cross product `self x other` (both in the XY
    /// plane): positive when `other` is CCW of `self` around the origin.
    pub fn cross2(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    /// Component-wise minimum (bounding-box helper).
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }

    /// Component-wise maximum (bounding-box helper).
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
    }
}

impl std::ops::Add for Point2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Sub for Point2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::Mul<f64> for Point2 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

// ---------------------------------------------------------------------------
// Wire
// ---------------------------------------------------------------------------

/// A closed tessellated polyline in the sketch frame: the points in
/// traversal order, *without* a duplicated closing point (the first point is
/// the implicit successor of the last), plus its signed area.
///
/// Positive [`Wire::signed_area`] means counter-clockwise (CCW). Wires are
/// produced by [`extract_wires`] and oriented by [`classify_wires`]
/// (outer boundaries CCW, holes CW).
#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    points: Vec<Point2>,
    signed_area: f64,
}

impl Wire {
    /// Builds a wire from a closed polygon's points. Rejects fewer than
    /// three points, non-finite coordinates and coincident consecutive
    /// points (including `first == last` — the closing point must not be
    /// duplicated).
    pub fn new(points: Vec<Point2>) -> Result<Self, ProfileError> {
        if points.len() < 3 {
            return Err(ProfileError::InvalidInput(format!(
                "a closed wire needs at least 3 points, got {}",
                points.len()
            )));
        }
        for (k, p) in points.iter().enumerate() {
            if !p.is_finite() {
                return Err(ProfileError::InvalidInput(format!(
                    "wire point {k} has non-finite coordinates"
                )));
            }
        }
        let n = points.len();
        for k in 0..n {
            let a = points[k];
            let b = points[(k + 1) % n];
            if a.dist(b) <= POINT_DUP_EPS {
                return Err(ProfileError::InvalidInput(format!(
                    "wire points {k} and {} coincide: degenerate closing edge",
                    (k + 1) % n
                )));
            }
        }
        Ok(Self {
            signed_area: signed_area(&points),
            points,
        })
    }

    /// The wire's points in traversal order (no duplicated closing point).
    pub fn points(&self) -> &[Point2] {
        &self.points
    }

    /// Signed area (shoelace): positive CCW, negative CW.
    pub fn signed_area(&self) -> f64 {
        self.signed_area
    }

    /// Number of distinct points (== number of segments).
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// True when the wire is counter-clockwise.
    pub fn is_ccw(&self) -> bool {
        self.signed_area > 0.0
    }

    /// Reverses the traversal direction in place (flips the signed area).
    fn reverse(&mut self) {
        self.points.reverse();
        self.signed_area = -self.signed_area;
    }
}

/// Shoelace signed area of a closed polygon without duplicated closing
/// point: positive when CCW.
fn signed_area(points: &[Point2]) -> f64 {
    let n = points.len();
    let mut sum = 0.0;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % n];
        sum += a.x * b.y - b.x * a.y;
    }
    sum / 2.0
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Everything that can go wrong in the profile kernels.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileError {
    /// No profile to work with: an empty geometry list, a geometry list with
    /// only `Point` items (points are not edges), or an empty wire list.
    EmptyGeometry,
    /// Wire chaining ended with open chains; `free_endpoints` lists every
    /// dangling endpoint in geometry order.
    OpenWire {
        /// The dangling endpoints, each the unmatched head or tail of an
        /// open chain.
        free_endpoints: Vec<Point2>,
    },
    /// Bad scalar parameters (tolerance/deflection), degenerate geometry
    /// (zero radius, zero-length line, coincident points), a wire segment
    /// parallel to the extrusion direction, or nesting deeper than one hole
    /// level.
    InvalidInput(String),
    /// Zero, non-finite or sketch-plane-parallel extrusion direction, or a
    /// zero/non-finite revolution axis direction.
    InvalidDirection(String),
    /// Non-positive or non-finite extrusion length.
    InvalidLength(f64),
    /// Non-finite or zero revolution angle.
    InvalidAngle(f64),
    /// The region triangulator could not produce a valid triangulation
    /// (typically a self-intersecting wire).
    TriangulationFailed(String),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::EmptyGeometry => {
                write!(f, "no profile: the geometry/wire list is empty (or contains only points, which are not edges)")
            }
            ProfileError::OpenWire { free_endpoints } => {
                let pts: Vec<String> = free_endpoints
                    .iter()
                    .map(|p| format!("({:.6}, {:.6})", p.x, p.y))
                    .collect();
                write!(
                    f,
                    "open wire: {} dangling endpoint(s) at [{}]",
                    free_endpoints.len(),
                    pts.join(", ")
                )
            }
            ProfileError::InvalidInput(msg) => write!(f, "invalid profile input: {msg}"),
            ProfileError::InvalidDirection(msg) => write!(f, "invalid direction: {msg}"),
            ProfileError::InvalidLength(v) => {
                write!(f, "invalid extrusion length {v}: must be finite and > 0")
            }
            ProfileError::InvalidAngle(v) => {
                write!(
                    f,
                    "invalid revolution angle {v}: must be finite and non-zero"
                )
            }
            ProfileError::TriangulationFailed(msg) => {
                write!(f, "region triangulation failed: {msg}")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

// ---------------------------------------------------------------------------
// Wire extraction
// ---------------------------------------------------------------------------

/// Extracts closed wires from sketch geometry.
///
/// Line segments contribute an edge between their endpoints; arcs contribute
/// a sagitta-tessellated polyline (chord error `<= deflection`, floor 4
/// segments); full circles become closed wires on their own;
/// `Geometry::Point` items are ignored (they are not edges). Edges are
/// chained head-to-tail within `tolerance` ([`DEFAULT_TOLERANCE`] when in
/// doubt). See the [module docs](self) for the chaining and ambiguity rules.
///
/// Errors: empty input ([`ProfileError::EmptyGeometry`]), degenerate
/// geometry, or open chains ([`ProfileError::OpenWire`] with every dangling
/// endpoint). Output order is deterministic (closure order).
pub fn extract_wires(
    geoms: &[Geometry],
    tolerance: f64,
    deflection: f64,
) -> Result<Vec<Wire>, ProfileError> {
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(ProfileError::InvalidInput(
            "tolerance must be finite and > 0".to_string(),
        ));
    }
    if !deflection.is_finite() || deflection <= 0.0 {
        return Err(ProfileError::InvalidInput(
            "deflection must be finite and > 0".to_string(),
        ));
    }
    if geoms.is_empty() {
        return Err(ProfileError::EmptyGeometry);
    }

    let mut wires = Vec::new();
    // Open chains; `None` marks a chain that closed (its wire was emitted)
    // or was merged away. Slots keep creation order for determinism.
    let mut chains: Vec<Option<Vec<Point2>>> = Vec::new();

    for (gi, g) in geoms.iter().enumerate() {
        match *g {
            // Documented: bare points are not edges.
            Geometry::Point { .. } => {}
            Geometry::Circle { center, radius } => {
                if !center.x.is_finite() || !center.y.is_finite() {
                    return Err(ProfileError::InvalidInput(format!(
                        "geometry {gi}: circle center has non-finite coordinates"
                    )));
                }
                if !radius.is_finite() || radius <= 0.0 {
                    return Err(ProfileError::InvalidInput(format!(
                        "geometry {gi}: circle radius must be finite and > 0"
                    )));
                }
                let pts = tessellate_circle(Point2::new(center.x, center.y), radius, deflection);
                wires.push(Wire::new(pts)?);
            }
            Geometry::LineSegment { start, end } => {
                let a = Point2::new(start.x, start.y);
                let b = Point2::new(end.x, end.y);
                if !a.is_finite() || !b.is_finite() {
                    return Err(ProfileError::InvalidInput(format!(
                        "geometry {gi}: line segment has non-finite endpoints"
                    )));
                }
                if a.dist(b) <= tolerance {
                    return Err(ProfileError::InvalidInput(format!(
                        "geometry {gi}: line segment of ~zero length"
                    )));
                }
                if let Some(w) = attach_edge(&mut chains, &[a, b], tolerance)? {
                    wires.push(w);
                }
            }
            Geometry::ArcOfCircle {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let c = Point2::new(center.x, center.y);
                if !c.is_finite() {
                    return Err(ProfileError::InvalidInput(format!(
                        "geometry {gi}: arc center has non-finite coordinates"
                    )));
                }
                if !radius.is_finite() || radius <= 0.0 {
                    return Err(ProfileError::InvalidInput(format!(
                        "geometry {gi}: arc radius must be finite and > 0"
                    )));
                }
                if !start_angle.is_finite() || !end_angle.is_finite() {
                    return Err(ProfileError::InvalidInput(format!(
                        "geometry {gi}: arc angles must be finite"
                    )));
                }
                let pts = tessellate_arc(c, radius, start_angle, end_angle, deflection)?;
                if pts[0].dist(pts[pts.len() - 1]) <= tolerance {
                    // Endpoints coincide within tolerance: a (near-)full turn
                    // closes on itself without chaining.
                    let mut loop_pts = pts;
                    loop_pts.pop();
                    wires.push(Wire::new(loop_pts)?);
                } else if let Some(w) = attach_edge(&mut chains, &pts, tolerance)? {
                    wires.push(w);
                }
            }
        }
    }

    // Any remaining open chain is an open wire.
    let mut free_endpoints = Vec::new();
    for chain in chains.iter().flatten() {
        free_endpoints.push(chain[0]);
        free_endpoints.push(chain[chain.len() - 1]);
    }
    if !free_endpoints.is_empty() {
        return Err(ProfileError::OpenWire { free_endpoints });
    }
    if wires.is_empty() {
        // Only Point geometries were present.
        return Err(ProfileError::EmptyGeometry);
    }
    Ok(wires)
}

/// Max angular step for chordal deviation `deflection` on a circle of
/// `radius` (sagitta `r * (1 - cos(step/2)) <= deflection`): the work-order
/// rule `2 * acos(max(0, 1 - deflection/radius))`.
fn arc_step_max(deflection: f64, radius: f64) -> f64 {
    2.0 * (1.0 - deflection / radius).clamp(0.0, 1.0).acos()
}

/// Tessellates a full circle into `n` points (no duplicated closing point),
/// CCW from angle 0.
fn tessellate_circle(center: Point2, radius: f64, deflection: f64) -> Vec<Point2> {
    let step_max = arc_step_max(deflection, radius);
    let n = ((TAU / step_max).ceil() as usize).clamp(MIN_CIRCLE_SEGMENTS, MAX_ARC_SEGMENTS);
    let step = TAU / n as f64;
    (0..n)
        .map(|k| {
            let a = step * k as f64;
            let (s, c) = a.sin_cos();
            Point2::new(center.x + radius * c, center.y + radius * s)
        })
        .collect()
}

/// Tessellates a CCW arc from `start_angle` to `end_angle` (radians) into
/// `n + 1` points including both endpoints. A sweep that normalizes to ~0
/// is treated as a full turn. Chord error of every segment is
/// `<= deflection` by the [`arc_step_max`] rule.
fn tessellate_arc(
    center: Point2,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    deflection: f64,
) -> Result<Vec<Point2>, ProfileError> {
    // Arcs are CCW from start to end; a negative/raw delta wraps.
    let mut sweep = (end_angle - start_angle).rem_euclid(TAU);
    if sweep <= TAU * 1e-12 {
        // Coincident angles: a full circle (documented convention).
        sweep = TAU;
    }
    let step_max = arc_step_max(deflection, radius);
    let n = ((sweep / step_max).ceil() as usize).clamp(MIN_ARC_SEGMENTS, MAX_ARC_SEGMENTS);
    let step = sweep / n as f64;
    let mut pts = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let a = start_angle + step * k as f64;
        let (s, c) = a.sin_cos();
        pts.push(Point2::new(center.x + radius * c, center.y + radius * s));
    }
    Ok(pts)
}

/// How a fresh edge polyline joins an existing chain.
#[derive(Debug, Clone, Copy)]
enum Join {
    /// Chain back == edge start: append the edge forward.
    AppendFwd,
    /// Chain back == edge end: append the edge reversed.
    AppendRev,
    /// Chain front == edge end: prepend the edge forward.
    PrependFwd,
    /// Chain front == edge start: prepend the edge reversed.
    PrependRev,
}

/// How two open chains merge (named by the ends that touch).
#[derive(Debug, Clone, Copy)]
enum Merge {
    /// Other's front touches this chain's back.
    BackFront,
    /// Other's back touches this chain's back.
    BackBack,
    /// Other's back touches this chain's front.
    FrontBack,
    /// Other's front touches this chain's front.
    FrontFront,
}

/// Attaches one edge polyline (start..end, both endpoints included) to the
/// chaining state, closing loops and merging chains as endpoints meet.
///
/// Deterministic first-come rule: chains are scanned in creation order and
/// each chain's *back* endpoint is tested before its front, so when several
/// open ends match, the earliest-created chain wins. Returns a wire when the
/// attach closed a loop.
fn attach_edge(
    chains: &mut Vec<Option<Vec<Point2>>>,
    poly: &[Point2],
    tolerance: f64,
) -> Result<Option<Wire>, ProfileError> {
    let start = poly[0];
    let end = poly[poly.len() - 1];

    // 1. Find the first open chain with a matching end (back before front).
    let mut join: Option<(usize, Join)> = None;
    for (ci, slot) in chains.iter().enumerate() {
        let Some(chain) = slot else { continue };
        let front = chain[0];
        let back = chain[chain.len() - 1];
        if back.dist(start) <= tolerance {
            join = Some((ci, Join::AppendFwd));
            break;
        }
        if back.dist(end) <= tolerance {
            join = Some((ci, Join::AppendRev));
            break;
        }
        if front.dist(end) <= tolerance {
            join = Some((ci, Join::PrependFwd));
            break;
        }
        if front.dist(start) <= tolerance {
            join = Some((ci, Join::PrependRev));
            break;
        }
    }

    let Some((ci, join)) = join else {
        chains.push(Some(poly.to_vec()));
        return Ok(None);
    };

    // 2. Apply the join (never duplicating the shared endpoint).
    {
        let chain = chains[ci].as_mut().expect("join target must be open");
        match join {
            Join::AppendFwd => chain.extend_from_slice(&poly[1..]),
            Join::AppendRev => {
                let mut rev = poly.to_vec();
                rev.reverse();
                chain.extend_from_slice(&rev[1..]);
            }
            Join::PrependFwd => {
                let mut new = poly[..poly.len() - 1].to_vec();
                new.extend_from_slice(chain);
                *chain = new;
            }
            Join::PrependRev => {
                let mut rev = poly.to_vec();
                rev.reverse();
                rev.extend_from_slice(&chain[1..]);
                *chain = rev;
            }
        }
    }

    // 3. Close or merge until stable. Every merge removes one chain, so the
    //    loop terminates.
    loop {
        let (front, back) = {
            let chain = chains[ci].as_mut().expect("chain must stay open here");
            (chain[0], chain[chain.len() - 1])
        };

        if front.dist(back) <= tolerance {
            // Closed loop: drop the duplicated closing point and emit.
            let mut pts = chains[ci].take().expect("closing chain must be open");
            pts.pop();
            return Wire::new(pts).map(Some);
        }

        // Find another open chain touching either end (back before front,
        // creation order) and merge it in.
        let mut merge: Option<(usize, Merge)> = None;
        for (cj, slot) in chains.iter().enumerate() {
            if cj == ci {
                continue;
            }
            let Some(other) = slot else { continue };
            let of = other[0];
            let ob = other[other.len() - 1];
            if ob.dist(back) <= tolerance {
                merge = Some((cj, Merge::BackBack));
                break;
            }
            if ob.dist(front) <= tolerance {
                merge = Some((cj, Merge::FrontBack));
                break;
            }
            if of.dist(back) <= tolerance {
                merge = Some((cj, Merge::BackFront));
                break;
            }
            if of.dist(front) <= tolerance {
                merge = Some((cj, Merge::FrontFront));
                break;
            }
        }

        let Some((cj, kind)) = merge else {
            return Ok(None);
        };
        let other = chains[cj].take().expect("merge source must be open");
        let chain = chains[ci].as_mut().expect("chain must stay open here");
        match kind {
            Merge::BackFront => chain.extend_from_slice(&other[1..]),
            Merge::BackBack => {
                let mut rev = other;
                rev.reverse();
                chain.extend_from_slice(&rev[1..]);
            }
            Merge::FrontBack => {
                let mut joined = other;
                joined.extend_from_slice(&chain[1..]);
                *chain = joined;
            }
            Merge::FrontFront => {
                let mut rev = other;
                rev.reverse();
                rev.extend_from_slice(&chain[1..]);
                *chain = rev;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Nesting classification
// ---------------------------------------------------------------------------

/// Even-odd ray cast (PNPOLY): strictly inside when the crossings are odd.
/// A point exactly on the boundary is unstable by design — the nesting
/// classifier documents that as "on-boundary counts as outside".
fn point_in_polygon(p: Point2, poly: &[Point2]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for (i, b) in poly.iter().enumerate() {
        let a = poly[j];
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if x > p.x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Containment depth of every wire: how many *other* wires contain its
/// representative point (its first vertex).
fn nesting_depths(wires: &[Wire]) -> Vec<usize> {
    let mut depths = vec![0usize; wires.len()];
    for (i, w) in wires.iter().enumerate() {
        let rep = w.points[0];
        for (j, other) in wires.iter().enumerate() {
            if i != j && point_in_polygon(rep, &other.points) {
                depths[i] += 1;
            }
        }
    }
    depths
}

/// Groups wires into regions: `(outer_index, [hole indices])`.
///
/// v1 supports one nesting level (outer + direct holes, the
/// `FaceMakerCheese` model); deeper nesting is rejected with a clear error.
fn region_groups(wires: &[Wire]) -> Result<Vec<(usize, Vec<usize>)>, ProfileError> {
    let depths = nesting_depths(wires);
    if let Some(i) = depths.iter().position(|&d| d >= 2) {
        return Err(ProfileError::InvalidInput(format!(
            "wire {i} is nested more than one level deep; v1 regions support outer wires \
             with direct holes only (like FreeCAD's FaceMakerCheese)"
        )));
    }
    let mut groups: Vec<(usize, Vec<usize>)> = (0..wires.len())
        .filter(|&i| depths[i] == 0)
        .map(|i| (i, Vec::new()))
        .collect();
    for (i, &d) in depths.iter().enumerate() {
        if d != 1 {
            continue;
        }
        // A depth-1 wire has exactly one container.
        let parent = wires
            .iter()
            .enumerate()
            .find(|(j, other)| *j != i && point_in_polygon(wires[i].points[0], &other.points))
            .map(|(j, _)| j);
        let Some(parent) = parent else {
            return Err(ProfileError::InvalidInput(format!(
                "wire {i} is classified as a hole but no container wire was found"
            )));
        };
        if let Some(group) = groups.iter_mut().find(|(o, _)| *o == parent) {
            group.1.push(i);
        }
    }
    Ok(groups)
}

/// Orients wires for face semantics: even depth = outer boundary (CCW),
/// odd depth = hole (CW). Input order is preserved.
///
/// Errors on an empty list. Containment ties (a representative point on
/// another wire's boundary) count as outside; intersecting wires are the
/// caller's responsibility and are not validated (v1).
pub fn classify_wires(mut wires: Vec<Wire>) -> Result<Vec<Wire>, ProfileError> {
    if wires.is_empty() {
        return Err(ProfileError::EmptyGeometry);
    }
    let depths = nesting_depths(&wires);
    for (w, d) in wires.iter_mut().zip(depths) {
        let want_ccw = d % 2 == 0;
        if want_ccw != w.is_ccw() {
            w.reverse();
        }
    }
    Ok(wires)
}

// ---------------------------------------------------------------------------
// Region triangulation: hole bridging + ear clipping
// ---------------------------------------------------------------------------

/// One vertex of a 2D cap polygon: its mesh vertex index plus its 2D
/// position (sketch coordinates).
#[derive(Debug, Clone, Copy)]
struct PolyVert {
    idx: u32,
    pos: Point2,
}

fn poly_signed_area(poly: &[PolyVert]) -> f64 {
    let n = poly.len();
    let mut sum = 0.0;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % n];
        sum += a.pos.x * b.pos.y - b.pos.x * a.pos.y;
    }
    sum / 2.0
}

fn poly_points(poly: &[PolyVert]) -> Vec<Point2> {
    poly.iter().map(|v| v.pos).collect()
}

/// Triangulates one planar region: `outer` boundary plus `holes`.
///
/// The outer loop is normalized CCW, holes CW, then every hole is bridged
/// into the outer boundary by a zero-width cut through a mutually visible
/// vertex pair, and the merged polygon is ear-clipped. Emitted triangles are
/// CCW in 2D and reference the *original* mesh vertex indices, so caps stay
/// watertight against whatever shares those indices.
fn triangulate_region(
    outer: &[PolyVert],
    holes: &[Vec<PolyVert>],
) -> Result<Vec<[u32; 3]>, ProfileError> {
    let mut poly = outer.to_vec();
    if poly_signed_area(&poly) < 0.0 {
        poly.reverse();
    }
    let mut holes: Vec<Vec<PolyVert>> = holes.to_vec();
    for hole in &mut holes {
        if poly_signed_area(hole) > 0.0 {
            hole.reverse();
        }
    }
    // Deterministic merge order: biggest hole first (ties keep input order).
    holes.sort_by(|a, b| {
        poly_signed_area(b)
            .abs()
            .total_cmp(&poly_signed_area(a).abs())
    });
    for hi in 0..holes.len() {
        bridge_hole(&mut poly, &holes, hi)?;
    }
    ear_clip(&poly)
}

/// Splices `holes[hi]` into the merged boundary polygon through a mutually
/// visible vertex pair (the classic hole-bridging ear clip).
fn bridge_hole(
    poly: &mut Vec<PolyVert>,
    holes: &[Vec<PolyVert>],
    hi: usize,
) -> Result<(), ProfileError> {
    let hole = &holes[hi];
    let mut found: Option<(usize, usize)> = None;
    'search: for (h, hv) in hole.iter().enumerate() {
        for (i, ov) in poly.iter().enumerate() {
            if bridge_visible(ov.pos, hv.pos, poly, i, holes, hi, h) {
                found = Some((i, h));
                break 'search;
            }
        }
    }
    let Some((i, h)) = found else {
        return Err(ProfileError::TriangulationFailed(format!(
            "no mutually visible vertex pair to bridge a hole of {} vertices into the outer boundary",
            hole.len()
        )));
    };
    // Splice: outer[..=i], hole[h..], hole[..=h], outer[i..] — the hole is
    // traversed once (in its CW order) and both cut endpoints are doubled,
    // forming a zero-width cut. Doubled entries keep the same mesh index.
    let mut merged = Vec::with_capacity(poly.len() + hole.len() + 1);
    merged.extend_from_slice(&poly[..=i]);
    merged.extend_from_slice(&hole[h..]);
    merged.extend_from_slice(&hole[..=h]);
    merged.extend_from_slice(&poly[i..]);
    *poly = merged;
    Ok(())
}

/// Mutual visibility of an outer vertex `o` and a hole vertex `p`:
/// the open segment stays inside the material (midpoint inside the merged
/// region, outside every hole) and touches no boundary edge except the two
/// incident to each endpoint.
fn bridge_visible(
    o: Point2,
    p: Point2,
    poly: &[PolyVert],
    oi: usize,
    holes: &[Vec<PolyVert>],
    current_hole: usize,
    current_vert: usize,
) -> bool {
    let mid = (o + p) * 0.5;
    // The merged polygon (outer + previously bridged holes, a weakly-simple
    // boundary) classifies the material by parity.
    let pts = poly_points(poly);
    if !point_in_polygon(mid, &pts) {
        return false;
    }
    for hole in holes {
        let hpts = poly_points(hole);
        if point_in_polygon(mid, &hpts) {
            return false;
        }
    }
    // Segment clearance against merged boundary edges (skip the two incident
    // to `o` — they legitimately touch at the endpoint).
    let n = poly.len();
    for k in 0..n {
        if k == oi || (k + 1) % n == oi {
            continue;
        }
        if segments_touch_or_cross(o, p, poly[k].pos, poly[(k + 1) % n].pos) {
            return false;
        }
    }
    // And against every hole's edges (skip the two incident to `p` on the
    // current hole; other holes are still unmerged, test all their edges).
    for (hk, hole) in holes.iter().enumerate() {
        let m = hole.len();
        for k in 0..m {
            if hk == current_hole && (k == current_vert || (k + 1) % m == current_vert) {
                continue;
            }
            if segments_touch_or_cross(o, p, hole[k].pos, hole[(k + 1) % m].pos) {
                return false;
            }
        }
    }
    true
}

/// Conservative segment interference: a strict proper crossing, or any
/// near-contact (an orientation determinant within epsilon while the
/// bounding boxes overlap). Overly shy is fine here — it only rejects a
/// bridge candidate that a stricter test might accept.
fn segments_touch_or_cross(a1: Point2, a2: Point2, b1: Point2, b2: Point2) -> bool {
    let d1 = (b2 - b1).cross2(a1 - b1);
    let d2 = (b2 - b1).cross2(a2 - b1);
    let d3 = (a2 - a1).cross2(b1 - a1);
    let d4 = (a2 - a1).cross2(b2 - a1);
    let opposite = |x: f64, y: f64| {
        (x > VISIBILITY_EPS && y < -VISIBILITY_EPS) || (x < -VISIBILITY_EPS && y > VISIBILITY_EPS)
    };
    if opposite(d1, d2) && opposite(d3, d4) {
        return true;
    }
    let near = d1.abs() <= VISIBILITY_EPS
        || d2.abs() <= VISIBILITY_EPS
        || d3.abs() <= VISIBILITY_EPS
        || d4.abs() <= VISIBILITY_EPS;
    if near {
        let (alo, ahi) = (a1.min(a2), a1.max(a2));
        let (blo, bhi) = (b1.min(b2), b1.max(b2));
        if alo.x <= bhi.x + VISIBILITY_EPS
            && blo.x <= ahi.x + VISIBILITY_EPS
            && alo.y <= bhi.y + VISIBILITY_EPS
            && blo.y <= ahi.y + VISIBILITY_EPS
        {
            return true;
        }
    }
    false
}

/// True when `v` is strictly inside the CCW triangle `a b c` by more than
/// the area epsilon on every edge (boundary/coincident points never block).
fn point_strictly_in_triangle(v: Point2, a: Point2, b: Point2, c: Point2) -> bool {
    (b - a).cross2(v - a) > AREA_EPS
        && (c - b).cross2(v - b) > AREA_EPS
        && (a - c).cross2(v - c) > AREA_EPS
}

/// Ear clipping over a CCW polygon (possibly weakly simple after hole
/// bridging). Emits CCW triangles of original mesh indices.
///
/// Collinear/degenerate ears (|cross| <= 1e-12) are skipped honestly: the
/// vertex is removed without emitting a zero-area triangle.
fn ear_clip(poly: &[PolyVert]) -> Result<Vec<[u32; 3]>, ProfileError> {
    let n = poly.len();
    debug_assert!(n >= 3);
    let mut idx: Vec<usize> = (0..n).collect();
    let mut tris = Vec::with_capacity(n);
    let mut guard = n * n + 64;
    while idx.len() > 3 {
        guard -= 1;
        if guard == 0 {
            return Err(ProfileError::TriangulationFailed(
                "ear clipping made no progress".to_string(),
            ));
        }
        let len = idx.len();
        let mut clipped: Option<usize> = None;
        for k in 0..len {
            let i0 = idx[(k + len - 1) % len];
            let i1 = idx[k];
            let i2 = idx[(k + 1) % len];
            let (p0, p1, p2) = (poly[i0].pos, poly[i1].pos, poly[i2].pos);
            let cross = (p1 - p0).cross2(p2 - p1);
            if cross.abs() <= AREA_EPS {
                // Collinear/zero-area ear: remove the vertex, emit nothing.
                clipped = Some(k);
                break;
            }
            if cross < 0.0 {
                continue; // reflex vertex on a CCW polygon
            }
            let mut blocked = false;
            for &j in &idx {
                if j == i0 || j == i1 || j == i2 {
                    continue;
                }
                if point_strictly_in_triangle(poly[j].pos, p0, p1, p2) {
                    blocked = true;
                    break;
                }
            }
            if !blocked {
                tris.push([poly[i0].idx, poly[i1].idx, poly[i2].idx]);
                clipped = Some(k);
                break;
            }
        }
        let Some(k) = clipped else {
            return Err(ProfileError::TriangulationFailed(
                "no valid ear found: the wire region is likely self-intersecting".to_string(),
            ));
        };
        idx.remove(k);
    }
    // Final triangle — skipped when degenerate (a fully collinear remainder
    // bounds zero area, so skipping is exact, not lossy).
    let (a, b, c) = (poly[idx[0]], poly[idx[1]], poly[idx[2]]);
    if (b.pos - a.pos).cross2(c.pos - b.pos).abs() > AREA_EPS {
        tris.push([a.idx, b.idx, c.idx]);
    }
    Ok(tris)
}

// ---------------------------------------------------------------------------
// Extrusion (Pad)
// ---------------------------------------------------------------------------

/// Extrudes classified profile wires into a watertight prism — the Pad
/// kernel.
///
/// * `direction` is normalized internally; it must be finite, non-zero and
///   not parallel to the sketch plane (a Pad along the sketch plane is
///   degenerate; FreeCAD pads along the sketch normal).
/// * `length` must be finite and `> 0` (the *distance* along the normalized
///   direction).
/// * `midplane == false`: the profile stays at its base plane and the solid
///   spans `0..=length` along the direction. `midplane == true`: it spans
///   `-length/2..=+length/2` (implemented as a pure translation after
///   building at `z = 0` — never a re-orientation).
/// * The 2D wire points are interpreted as XY sketch coordinates at `z = 0`.
/// * Wires may be passed in any orientation; they are re-oriented by
///   containment (outers CCW, holes CW) before winding decisions, so
///   `Mesh::volume()` is positive for non-degenerate profiles.
/// * `deflection` is validated and reserved for API symmetry with
///   [`revolve`]; the given wires arrive pre-tessellated and are respected
///   as-is.
pub fn extrude(
    wires: &[Wire],
    direction: Vector3,
    length: f64,
    midplane: bool,
    deflection: f64,
) -> Result<Mesh, ProfileError> {
    if !deflection.is_finite() || deflection <= 0.0 {
        return Err(ProfileError::InvalidInput(
            "deflection must be finite and > 0".to_string(),
        ));
    }
    if !direction.x.is_finite() || !direction.y.is_finite() || !direction.z.is_finite() {
        return Err(ProfileError::InvalidDirection(
            "direction must be finite".to_string(),
        ));
    }
    let dir_n = direction.normalized().ok_or_else(|| {
        ProfileError::InvalidDirection("direction must be finite and non-zero".to_string())
    })?;
    if !length.is_finite() || length <= 0.0 {
        return Err(ProfileError::InvalidLength(length));
    }
    if wires.is_empty() {
        return Err(ProfileError::EmptyGeometry);
    }
    if dir_n.z.abs() < 1e-12 {
        return Err(ProfileError::InvalidDirection(
            "direction is parallel to the sketch plane: a Pad along the sketch plane is \
             degenerate (FreeCAD pads along the sketch normal)"
                .to_string(),
        ));
    }

    // Orient wires by nesting (outers CCW, holes CW) so all winding decisions
    // below are correct for classified and unclassified input alike.
    let depths = nesting_depths(wires);
    let oriented: Vec<Vec<Point2>> = wires
        .iter()
        .zip(&depths)
        .map(|(w, &d)| {
            let want_ccw = d % 2 == 0;
            if want_ccw != w.is_ccw() {
                let mut pts = w.points.clone();
                pts.reverse();
                pts
            } else {
                w.points.clone()
            }
        })
        .collect();

    // A segment parallel to the extrusion direction would sweep a degenerate
    // (collinear) side wall — but for any *accepted* direction (|z| >= 1e-12)
    // and any non-zero sketch-plane segment, `|segment x direction| >=
    // |segment| * |dir_z| > 0`, so parallelism is impossible and there is
    // nothing to check. The sketch-plane rejection above subsumes it.

    let groups = region_groups(wires)?;

    // One shared vertex array: wire points at z = 0 (bottom), then the same
    // points at z = length (top). Caps and walls reference these indices —
    // watertight by construction, no duplicated coordinates.
    let mut vertices = Vec::with_capacity(2 * oriented.iter().map(Vec::len).sum::<usize>());
    let mut bottom_base = Vec::with_capacity(oriented.len());
    let mut top_base = Vec::with_capacity(oriented.len());
    for pts in &oriented {
        bottom_base.push(vertices.len() as u32);
        for p in pts {
            vertices.push(Vector3::new(p.x, p.y, 0.0));
        }
    }
    for pts in &oriented {
        top_base.push(vertices.len() as u32);
        for p in pts {
            // The top ring is the bottom ring translated by the extrusion
            // vector `length * dir_n`: side walls are parallelograms, and for
            // skew directions the top face is sheared sideways. For +Z this
            // is exactly `(x, y, length)`.
            vertices.push(Vector3::new(
                p.x + length * dir_n.x,
                p.y + length * dir_n.y,
                length * dir_n.z,
            ));
        }
    }

    let mut tris = Vec::new();
    // Side walls: one quad per segment. For a CCW outer, travel has the
    // material on the left and `(edge x dir)` points outward; for a CW hole
    // the same formula points into the cavity — outward from the solid in
    // both cases.
    for (wi, pts) in oriented.iter().enumerate() {
        let n = pts.len();
        let b = bottom_base[wi];
        let t = top_base[wi];
        for i in 0..n {
            let i1 = (i + 1) % n;
            let (b0, b1) = (b + i as u32, b + i1 as u32);
            let (t0, t1) = (t + i as u32, t + i1 as u32);
            tris.push([b0, b1, t1]);
            tris.push([b0, t1, t0]);
        }
    }
    // Caps: triangulate each region (outer + its holes); bottom cap wound
    // backwards (outward -Z), top cap forwards (+Z).
    for &(outer, ref holes) in &groups {
        let outer_bottom = cap_poly(&oriented[outer], bottom_base[outer]);
        let outer_top = cap_poly(&oriented[outer], top_base[outer]);
        let holes_bottom: Vec<Vec<PolyVert>> = holes
            .iter()
            .map(|&h| cap_poly(&oriented[h], bottom_base[h]))
            .collect();
        let holes_top: Vec<Vec<PolyVert>> = holes
            .iter()
            .map(|&h| cap_poly(&oriented[h], top_base[h]))
            .collect();
        for tri in triangulate_region(&outer_bottom, &holes_bottom)? {
            tris.push([tri[0], tri[2], tri[1]]);
        }
        for tri in triangulate_region(&outer_top, &holes_top)? {
            tris.push(tri);
        }
    }

    // An extrusion towards -Z flips every winding (the profile normal is +Z).
    if dir_n.z < 0.0 {
        for tri in &mut tris {
            tri.swap(0, 2);
        }
    }
    if midplane {
        let offset = dir_n * (-length / 2.0);
        for v in &mut vertices {
            *v = *v + offset;
        }
    }
    Ok(Mesh::new(vertices, tris))
}

/// Cap polygon for a wire: mesh indices offset by `base`, 2D positions.
fn cap_poly(pts: &[Point2], base: u32) -> Vec<PolyVert> {
    pts.iter()
        .enumerate()
        .map(|(k, p)| PolyVert {
            idx: base + k as u32,
            pos: *p,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Revolution (Revolution)
// ---------------------------------------------------------------------------

/// Where a profile point lives in the revolve vertex table.
#[derive(Debug, Clone, Copy)]
enum Slot {
    /// Single fixed vertex index (point on the axis).
    OnAxis(u32),
    /// Slot among the off-axis points; ring `j` owns vertex
    /// `axis_count + j * off_axis_count + slot`.
    OffAxis(usize),
}

/// Revolves classified profile wires around an axis into a watertight solid
/// — the Revolution kernel.
///
/// * The 2D profile `(u, v)` is mapped to `axis_base + u*e1 + v*axis_dir`
///   with `e1` a canonical unit perpendicular to the axis (`e1 = +X` for a
///   `+Z` axis — the classic lathe: profile in the XZ half-plane, axis Z).
///   `u` is therefore the signed radial coordinate and `v` the axial one.
/// * `axis_dir` is normalized internally; it must be finite and non-zero.
/// * `angle_deg`: full turn when `|angle| >= 360 - 1e-9` (treated as exactly
///   360, seam-welded — no duplicated seam vertices). A partial turn sweeps
///   `|angle|` degrees; the sign is the sweep direction (negative =
///   clockwise around the axis) and the two flat side walls are capped with
///   the extrusion cap machinery. Zero or non-finite angles are rejected.
/// * Ring count follows the sagitta rule on `r_max` (the largest profile
///   distance from the axis): step `2*acos(max(0, 1 - deflection/r_max))`,
///   clamped to 16..=512 rings.
/// * Points within 1e-9 of the axis stay fixed as single shared vertices;
///   profile edges lying entirely on the axis sweep nothing. Both rules keep
///   every emitted triangle non-degenerate by construction.
/// * The profile may lie anywhere relative to the axis; a profile crossing
///   or touching the axis off a boundary sweeps a self-intersecting solid —
///   the caller's problem, exactly as in FreeCAD.
/// * `Mesh::volume()` is positive for CCW-classified profiles.
pub fn revolve(
    wires: &[Wire],
    axis_base: Vector3,
    axis_dir: Vector3,
    angle_deg: f64,
    deflection: f64,
) -> Result<Mesh, ProfileError> {
    if !deflection.is_finite() || deflection <= 0.0 {
        return Err(ProfileError::InvalidInput(
            "deflection must be finite and > 0".to_string(),
        ));
    }
    if !axis_dir.x.is_finite() || !axis_dir.y.is_finite() || !axis_dir.z.is_finite() {
        return Err(ProfileError::InvalidDirection(
            "axis direction must be finite".to_string(),
        ));
    }
    let dir_n = axis_dir.normalized().ok_or_else(|| {
        ProfileError::InvalidDirection("axis direction must be finite and non-zero".to_string())
    })?;
    if !angle_deg.is_finite() {
        return Err(ProfileError::InvalidAngle(angle_deg));
    }
    if angle_deg == 0.0 {
        return Err(ProfileError::InvalidAngle(angle_deg));
    }
    if wires.is_empty() {
        return Err(ProfileError::EmptyGeometry);
    }

    // Canonical frame. Lathe canonical case: axis +Z => e1 = +X, so the
    // profile (u, v) lands at (u, 0, v) in the XZ half-plane.
    let e1 = {
        let c = Vector3::Y.cross(&dir_n);
        if c.length_squared() > 1e-18 {
            c * (1.0 / c.length())
        } else {
            // Axis parallel to Y: X x dir is a valid perpendicular.
            let c2 = Vector3::X.cross(&dir_n);
            c2 * (1.0 / c2.length())
        }
    };
    let e2 = dir_n.cross(&e1);

    // Orient wires by nesting (outers CCW, holes CW).
    let depths = nesting_depths(wires);
    let oriented: Vec<Vec<Point2>> = wires
        .iter()
        .zip(&depths)
        .map(|(w, &d)| {
            let want_ccw = d % 2 == 0;
            if want_ccw != w.is_ccw() {
                let mut pts = w.points.clone();
                pts.reverse();
                pts
            } else {
                w.points.clone()
            }
        })
        .collect();

    // Radial extent and on-axis classification.
    let mut r_max = 0.0f64;
    for pts in &oriented {
        for p in pts {
            r_max = r_max.max(p.x.abs());
        }
    }
    if r_max < AXIS_SNAP {
        return Err(ProfileError::InvalidInput(
            "every profile point lies on the revolution axis (within 1e-9): nothing to sweep"
                .to_string(),
        ));
    }

    // Sweep discretization.
    let full_turn = angle_deg.abs() >= 360.0 - FULL_TURN_EPS_DEG;
    let sweep = if full_turn {
        TAU
    } else {
        angle_deg.abs().to_radians()
    };
    let step_max = arc_step_max(deflection, r_max);
    let n = ((sweep / step_max).ceil() as usize).clamp(MIN_SWEEP_SEGMENTS, MAX_SWEEP_SEGMENTS);
    let step_sign = if angle_deg < 0.0 { -1.0 } else { 1.0 };
    let step = step_sign * sweep / n as f64;
    let ring_count = if full_turn { n } else { n + 1 };

    let groups = region_groups(wires)?;

    // Vertex table: one fixed vertex per on-axis profile point, then
    // `ring_count` rings over the off-axis points. A full turn wraps ring
    // indices modulo `n` — the seam is welded, no duplicated vertices.
    let mut vertices = Vec::new();
    let mut slots: Vec<Vec<Slot>> = Vec::with_capacity(oriented.len());
    for pts in &oriented {
        let mut wire_slots = Vec::with_capacity(pts.len());
        for p in pts {
            if p.x.abs() < AXIS_SNAP {
                let idx = vertices.len() as u32;
                // The point stays fixed at its mapped position (u ~ 0).
                vertices.push(axis_base + e1 * p.x + dir_n * p.y);
                wire_slots.push(Slot::OnAxis(idx));
            } else {
                wire_slots.push(Slot::OffAxis(0)); // slot filled in below
            }
        }
        slots.push(wire_slots);
    }
    let mut next_slot = 0usize;
    for wire_slots in &mut slots {
        for slot in wire_slots.iter_mut() {
            if matches!(slot, Slot::OffAxis(_)) {
                *slot = Slot::OffAxis(next_slot);
                next_slot += 1;
            }
        }
    }
    let off_axis_count = next_slot;
    let axis_count = vertices.len() as u32;
    // Ring positions: base + u*(cos*e1 + sin*e2) + v*dir (Rodrigues on the
    // in-plane component; the axial component is invariant).
    for j in 0..ring_count {
        let (s, c) = (step * j as f64).sin_cos();
        let radial = e1 * c + e2 * s;
        for (wi, pts) in oriented.iter().enumerate() {
            for (pi, p) in pts.iter().enumerate() {
                if matches!(slots[wi][pi], Slot::OnAxis(_)) {
                    continue;
                }
                vertices.push(axis_base + radial * p.x + dir_n * p.y);
            }
        }
    }
    let ring_vertex = |ring: usize, wi: usize, pi: usize| -> u32 {
        match slots[wi][pi] {
            Slot::OnAxis(a) => a,
            Slot::OffAxis(slot) => axis_count + (ring * off_axis_count + slot) as u32,
        }
    };

    // Swept surface, one strip per profile edge. Winding: the sweep runs
    // around the axis; for step > 0 the outward normal is to the *right* of
    // the profile travel (outward from the material for CCW outers, into
    // the cavity for CW holes). Negative steps flip every triangle.
    let mut tris = Vec::new();
    let wrap = |j: usize| -> usize {
        if full_turn {
            j % n
        } else {
            j
        }
    };
    for (wi, pts) in oriented.iter().enumerate() {
        let m = pts.len();
        for i in 0..m {
            let i1 = (i + 1) % m;
            let a_on = matches!(slots[wi][i], Slot::OnAxis(_));
            let b_on = matches!(slots[wi][i1], Slot::OnAxis(_));
            if a_on && b_on {
                // The edge lies on the axis; it sweeps nothing.
                continue;
            }
            if a_on {
                // Radial edge from the axis: the strip is a cone around the
                // fixed apex, one triangle per ring step.
                let Slot::OnAxis(apex) = slots[wi][i] else {
                    unreachable!("classified on-axis above")
                };
                for j in 0..n {
                    let r0 = ring_vertex(wrap(j + 1), wi, i1);
                    let r1 = ring_vertex(wrap(j), wi, i1);
                    if step > 0.0 {
                        tris.push([apex, r0, r1]);
                    } else {
                        tris.push([apex, r1, r0]);
                    }
                }
            } else if b_on {
                let Slot::OnAxis(apex) = slots[wi][i1] else {
                    unreachable!("classified on-axis above")
                };
                for j in 0..n {
                    let r0 = ring_vertex(wrap(j), wi, i);
                    let r1 = ring_vertex(wrap(j + 1), wi, i);
                    if step > 0.0 {
                        tris.push([apex, r0, r1]);
                    } else {
                        tris.push([apex, r1, r0]);
                    }
                }
            } else {
                for j in 0..n {
                    let q0 = ring_vertex(wrap(j), wi, i);
                    let q1 = ring_vertex(wrap(j), wi, i1);
                    let q2 = ring_vertex(wrap(j + 1), wi, i1);
                    let q3 = ring_vertex(wrap(j + 1), wi, i);
                    if step > 0.0 {
                        tris.push([q0, q2, q1]);
                        tris.push([q0, q3, q2]);
                    } else {
                        tris.push([q0, q1, q2]);
                        tris.push([q0, q2, q3]);
                    }
                }
            }
        }
    }

    // Partial turn: cap the two flat side walls with the cap machinery.
    // The 2D triangulation is CCW; mapped into 3D its normal is -e2. The
    // start cap faces -e2 * sign(step), the end cap +e2 * sign(step).
    if !full_turn {
        for &(outer, ref holes) in &groups {
            let cap = |ring: usize| -> (Vec<PolyVert>, Vec<Vec<PolyVert>>) {
                let outer_poly: Vec<PolyVert> = oriented[outer]
                    .iter()
                    .enumerate()
                    .map(|(k, p)| PolyVert {
                        idx: ring_vertex(ring, outer, k),
                        pos: *p,
                    })
                    .collect();
                let hole_polys: Vec<Vec<PolyVert>> = holes
                    .iter()
                    .map(|&h| {
                        oriented[h]
                            .iter()
                            .enumerate()
                            .map(|(k, p)| PolyVert {
                                idx: ring_vertex(ring, h, k),
                                pos: *p,
                            })
                            .collect()
                    })
                    .collect();
                (outer_poly, hole_polys)
            };
            let (start_outer, start_holes) = cap(0);
            let (end_outer, end_holes) = cap(n);
            for tri in triangulate_region(&start_outer, &start_holes)? {
                if step > 0.0 {
                    tris.push(tri);
                } else {
                    tris.push([tri[0], tri[2], tri[1]]);
                }
            }
            for tri in triangulate_region(&end_outer, &end_holes)? {
                if step > 0.0 {
                    tris.push([tri[0], tri[2], tri[1]]);
                } else {
                    tris.push(tri);
                }
            }
        }
    }

    Ok(Mesh::new(vertices, tris))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const PI: f64 = std::f64::consts::PI;

    /// Builds a wire from `(x, y)` tuples.
    fn w(pts: &[(f64, f64)]) -> Wire {
        Wire::new(pts.iter().map(|&(x, y)| Point2::new(x, y)).collect()).expect("valid wire")
    }

    /// Every undirected edge of every triangle appears exactly twice — the
    /// shared watertight helper for this whole module.
    fn assert_watertight(mesh: &Mesh) {
        assert!(mesh.validate().is_ok(), "mesh failed structural validation");
        assert!(!mesh.triangles.is_empty(), "mesh has no triangles");
        let mut edges: BTreeMap<[u32; 2], usize> = BTreeMap::new();
        for t in &mesh.triangles {
            assert_ne!(t[0], t[1], "index-degenerate triangle {t:?}");
            assert_ne!(t[1], t[2], "index-degenerate triangle {t:?}");
            assert_ne!(t[0], t[2], "index-degenerate triangle {t:?}");
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = [a.min(b), a.max(b)];
                *edges.entry(key).or_insert(0) += 1;
            }
        }
        for (edge, count) in &edges {
            assert_eq!(*count, 2, "edge {edge:?} appears {count} times");
        }
    }

    /// No two vertices share exact coordinates.
    fn assert_unique_vertices(mesh: &Mesh) {
        let mut sorted: Vec<Vector3> = mesh.vertices.clone();
        sorted.sort_by(|a, b| {
            a.x.total_cmp(&b.x)
                .then(a.y.total_cmp(&b.y))
                .then(a.z.total_cmp(&b.z))
        });
        for pair in sorted.windows(2) {
            assert!(
                pair[0].x != pair[1].x || pair[0].y != pair[1].y || pair[0].z != pair[1].z,
                "duplicated vertex coordinates at {:?}",
                pair[0]
            );
        }
    }

    /// No triangle with (twice the area of) a cross product below `eps`.
    fn assert_no_degenerate_triangles(mesh: &Mesh, eps: f64) {
        for (k, t) in mesh.triangles.iter().enumerate() {
            let (v0, v1, v2) = (
                &mesh.vertices[t[0] as usize],
                &mesh.vertices[t[1] as usize],
                &mesh.vertices[t[2] as usize],
            );
            let cross = (*v1 - *v0).cross(&(*v2 - *v0)).length();
            assert!(
                cross > eps,
                "triangle {k} ({t:?}) is degenerate: |cross| = {cross}"
            );
        }
    }

    /// Even-odd ray cast against the mesh (strict interiors only, so no
    /// triangle edge is ever double-counted).
    fn ray_crossings(mesh: &Mesh, origin: Vector3, dir: Vector3) -> u32 {
        let mut hits = 0;
        for t in &mesh.triangles {
            let (v0, v1, v2) = (
                mesh.vertices[t[0] as usize],
                mesh.vertices[t[1] as usize],
                mesh.vertices[t[2] as usize],
            );
            let e1 = v1 - v0;
            let e2 = v2 - v0;
            let p = dir.cross(&e2);
            let det = e1.dot(&p);
            if det.abs() < 1e-12 {
                continue;
            }
            let inv = 1.0 / det;
            let tv = origin - v0;
            let u = tv.dot(&p) * inv;
            if u <= 1e-9 || u >= 1.0 - 1e-9 {
                continue;
            }
            let q = tv.cross(&e1);
            let v = dir.dot(&q) * inv;
            if v <= 1e-9 || u + v >= 1.0 - 1e-9 {
                continue;
            }
            let t = e2.dot(&q) * inv;
            if t > 1e-9 {
                hits += 1;
            }
        }
        hits
    }

    fn assert_outside(mesh: &Mesh, origin: Vector3) {
        let hits = ray_crossings(mesh, origin, Vector3::X);
        assert_eq!(
            hits % 2,
            0,
            "expected {origin:?} outside, got {hits} crossings"
        );
    }

    fn assert_inside(mesh: &Mesh, origin: Vector3) {
        let hits = ray_crossings(mesh, origin, Vector3::X);
        assert_eq!(
            hits % 2,
            1,
            "expected {origin:?} inside, got {hits} crossings"
        );
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    fn rel_err(got: f64, want: f64) -> f64 {
        ((got - want) / want).abs()
    }

    fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Geometry {
        Geometry::LineSegment {
            start: fc_sketcher::geometry::Point::new(x1, y1),
            end: fc_sketcher::geometry::Point::new(x2, y2),
        }
    }

    fn point(x: f64, y: f64) -> Geometry {
        Geometry::Point { x, y }
    }

    fn circle(cx: f64, cy: f64, r: f64) -> Geometry {
        Geometry::Circle {
            center: fc_sketcher::geometry::Point::new(cx, cy),
            radius: r,
        }
    }

    fn arc(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Geometry {
        Geometry::ArcOfCircle {
            center: fc_sketcher::geometry::Point::new(cx, cy),
            radius: r,
            start_angle: a0,
            end_angle: a1,
        }
    }

    /// The 10x10 square [-5,5]^2 as four CCW line segments.
    fn square_geoms() -> Vec<Geometry> {
        vec![
            line(-5.0, -5.0, 5.0, -5.0),
            line(5.0, -5.0, 5.0, 5.0),
            line(5.0, 5.0, -5.0, 5.0),
            line(-5.0, 5.0, -5.0, -5.0),
        ]
    }

    // -- extraction ---------------------------------------------------------

    #[test]
    fn extract_square_from_four_lines() {
        let wires = extract_wires(&square_geoms(), DEFAULT_TOLERANCE, 0.01).unwrap();
        assert_eq!(wires.len(), 1);
        let wire = &wires[0];
        assert_eq!(wire.point_count(), 4);
        assert!(
            close(wire.signed_area(), 100.0, 1e-9),
            "area {}",
            wire.signed_area()
        );
        assert!(wire.is_ccw());
        // Traversal starts at the first line's start and stays in order.
        assert_eq!(wire.points()[0], Point2::new(-5.0, -5.0));
        assert_eq!(wire.points()[1], Point2::new(5.0, -5.0));
        // No duplicated closing point.
        assert_ne!(wire.points()[0], wire.points()[3]);
    }

    #[test]
    fn extract_circle_produces_closed_wire() {
        let geoms = vec![circle(0.0, 0.0, 5.0)];
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.005).unwrap();
        assert_eq!(wires.len(), 1);
        let wire = &wires[0];
        assert!(wire.point_count() >= 16, "floor 16 circle segments");
        assert!(
            rel_err(wire.signed_area(), PI * 25.0) < 0.01,
            "circle area {} vs pi*r^2",
            wire.signed_area()
        );
        for p in wire.points() {
            assert!(close(p.dist(Point2::new(0.0, 0.0)), 5.0, 1e-9));
        }
        assert_ne!(wire.points()[0], wire.points()[wire.point_count() - 1]);
    }

    #[test]
    fn arc_tessellation_respects_deflection_and_formula() {
        // Quarter arc r = 2 closed by two lines through the origin.
        let deflection: f64 = 0.005;
        let step_max = 2.0 * (1.0 - deflection / 2.0).clamp(0.0, 1.0).acos();
        let want_n = ((PI / 2.0) / step_max).ceil() as usize; // 12
        let geoms = vec![
            arc(0.0, 0.0, 2.0, 0.0, PI / 2.0),
            line(0.0, 2.0, 0.0, 0.0),
            line(0.0, 0.0, 2.0, 0.0),
        ];
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, deflection).unwrap();
        assert_eq!(wires.len(), 1);
        // Wire = arc (n+1 points) + two line corners, one shared endpoint
        // popped: n + 2 points.
        assert_eq!(wires[0].point_count(), want_n + 2);
        // Sagitta of every arc chord <= deflection: the chord midpoint sits
        // within deflection of the true radius. The two closing-line chords
        // (the wrap pair included) never have both endpoints on the arc.
        let mut arc_chords = 0;
        for pair in wires[0].points().windows(2) {
            let on_arc = close(pair[0].length(), 2.0, 1e-9) && close(pair[1].length(), 2.0, 1e-9);
            if !on_arc {
                continue;
            }
            arc_chords += 1;
            let mid = (pair[0] + pair[1]) * 0.5;
            let sagitta = 2.0 - mid.length();
            assert!(
                sagitta <= deflection + 1e-12,
                "sagitta {sagitta} > {deflection} at chord {pair:?}"
            );
        }
        assert_eq!(arc_chords, want_n);
        // Tiny arc with a coarse deflection still gets the floor of 4.
        let geoms = vec![
            arc(0.0, 0.0, 1.0, 0.0, 0.01),
            line(0.01f64.cos(), 0.01f64.sin(), 0.0, 0.0),
            line(0.0, 0.0, 1.0, 0.0),
        ];
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.5).unwrap();
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].point_count(), 4 + 2);
    }

    #[test]
    fn arc_sweep_wraps_ccw_and_full_turn_is_a_loop() {
        // end < start: the CCW arc wraps through angle 0 (sweep = pi/2).
        let geoms = vec![
            arc(0.0, 0.0, 1.0, 7.0 * PI / 4.0, PI / 4.0),
            line((PI / 4.0).cos(), (PI / 4.0).sin(), 0.0, 0.0),
            line(0.0, 0.0, (7.0 * PI / 4.0).cos(), (7.0 * PI / 4.0).sin()),
        ];
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.005).unwrap();
        assert_eq!(wires.len(), 1);
        // The wrap arc passes through angle 0 (point (1, 0)).
        let pts = wires[0].points();
        assert!(
            pts.iter()
                .any(|p| close(p.x, 1.0, 1e-9) && close(p.y, 0.0, 1e-9)),
            "wrapped arc must traverse angle 0"
        );
        // start == end: full turn -> closed loop without chaining.
        let geoms = vec![arc(0.0, 0.0, 2.0, 1.0, 1.0)];
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01).unwrap();
        assert_eq!(wires.len(), 1);
        assert!(wires[0].point_count() >= 16);
        assert!(rel_err(wires[0].signed_area(), PI * 4.0) < 0.01);
    }

    #[test]
    fn points_are_not_edges() {
        let mut geoms = square_geoms();
        geoms.push(point(0.0, 0.0));
        geoms.push(point(9.0, 9.0));
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01).unwrap();
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].point_count(), 4);
        // Points alone are no profile at all.
        let geoms = vec![point(0.0, 0.0), point(1.0, 1.0)];
        assert_eq!(
            extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01),
            Err(ProfileError::EmptyGeometry)
        );
    }

    #[test]
    fn open_wire_reports_free_endpoints() {
        let geoms = vec![
            line(0.0, 0.0, 1.0, 0.0),
            line(1.0, 0.0, 1.0, 1.0),
            line(1.0, 1.0, 0.5, 1.0),
        ];
        match extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01) {
            Err(ProfileError::OpenWire { free_endpoints }) => {
                assert_eq!(free_endpoints.len(), 2);
                let a = Point2::new(0.0, 0.0);
                let b = Point2::new(0.5, 1.0);
                assert!(
                    (free_endpoints[0].dist(a) < 1e-12 && free_endpoints[1].dist(b) < 1e-12)
                        || (free_endpoints[0].dist(b) < 1e-12 && free_endpoints[1].dist(a) < 1e-12),
                    "unexpected free endpoints {free_endpoints:?}"
                );
            }
            other => panic!("expected OpenWire, got {other:?}"),
        }
    }

    #[test]
    fn empty_geometry_is_an_error() {
        // Documented choice: empty input errors (it cannot mean a solid).
        assert_eq!(
            extract_wires(&[], DEFAULT_TOLERANCE, 0.01),
            Err(ProfileError::EmptyGeometry)
        );
    }

    #[test]
    fn ambiguous_junction_uses_first_come_rule() {
        // Three edges meet at (1,0): A ends there, B starts there, D starts
        // there. First come, first served (chains in creation order, back
        // before front): A's chain claims the junction, B attaches next and
        // closes its triangle, D opens a fresh chain. The outcome is two
        // clean triangles and it is fully deterministic.
        let geoms = vec![
            line(0.0, 0.0, 1.0, 0.0),   // A: chain1, dangling at (1,0)
            line(1.0, 0.0, 1.0, 1.0),   // B: attaches to chain1
            line(1.0, 1.0, 0.0, 0.0),   // C: closes triangle 1
            line(1.0, 0.0, -1.0, 0.0),  // D: junction already consumed, new chain
            line(-1.0, 0.0, 0.0, -1.0), // E
            line(0.0, -1.0, 1.0, 0.0),  // F: closes triangle 2
        ];
        let first = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01).unwrap();
        let second = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01).unwrap();
        assert_eq!(first, second, "extraction must be deterministic");
        assert_eq!(first.len(), 2, "first-come rule closes both loops");
        let mut areas: Vec<f64> = first.iter().map(Wire::signed_area).collect();
        areas.sort_by(|a, b| a.total_cmp(b));
        assert!(
            close(areas[0], 0.5, 1e-9) && close(areas[1], 1.0, 1e-9),
            "{areas:?}"
        );
    }

    #[test]
    fn chain_merge_bridges_two_chains() {
        // L1 and L2 start two separate chains; L3 attaches to chain1 and then
        // merges chain2 onto its back; L4 closes the loop.
        let geoms = vec![
            line(0.0, 0.0, 1.0, 0.0),
            line(2.0, 1.0, 1.0, 1.0),
            line(1.0, 0.0, 2.0, 1.0),
            line(1.0, 1.0, 0.0, 0.0),
        ];
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01).unwrap();
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].point_count(), 4);
        assert!(close(wires[0].signed_area(), 1.0, 1e-9));
    }

    #[test]
    fn degenerate_geometry_is_rejected() {
        let geoms = vec![circle(0.0, 0.0, 0.0)];
        assert!(matches!(
            extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01),
            Err(ProfileError::InvalidInput(_))
        ));
        let geoms = vec![line(1.0, 1.0, 1.0, 1.0)];
        assert!(matches!(
            extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01),
            Err(ProfileError::InvalidInput(_))
        ));
        let geoms = vec![line(0.0, f64::NAN, 1.0, 0.0)];
        assert!(matches!(
            extract_wires(&geoms, DEFAULT_TOLERANCE, 0.01),
            Err(ProfileError::InvalidInput(_))
        ));
        assert!(matches!(
            extract_wires(&square_geoms(), 0.0, 0.01),
            Err(ProfileError::InvalidInput(_))
        ));
    }

    // -- classification -----------------------------------------------------

    #[test]
    fn classify_two_concentric_circles() {
        let wires = extract_wires(
            &[circle(0.0, 0.0, 3.0), circle(0.0, 0.0, 1.5)],
            DEFAULT_TOLERANCE,
            0.01,
        )
        .unwrap();
        let classified = classify_wires(wires).unwrap();
        assert_eq!(classified.len(), 2);
        let outer = &classified[0];
        let inner = &classified[1];
        assert!(
            outer.is_ccw(),
            "outer must be CCW, area {}",
            outer.signed_area()
        );
        assert!(
            !inner.is_ccw(),
            "hole must be CW, area {}",
            inner.signed_area()
        );
        assert!(rel_err(outer.signed_area(), PI * 9.0) < 0.01);
        assert!(rel_err(-inner.signed_area(), PI * 2.25) < 0.01);
    }

    #[test]
    fn classify_three_nested_squares() {
        let wires = vec![
            w(&[(-6.0, -6.0), (6.0, -6.0), (6.0, 6.0), (-6.0, 6.0)]),
            w(&[(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)]),
            w(&[(-2.0, -2.0), (2.0, -2.0), (2.0, 2.0), (-2.0, 2.0)]),
        ];
        let classified = classify_wires(wires).unwrap();
        let areas: Vec<f64> = classified.iter().map(Wire::signed_area).collect();
        assert!(close(areas[0], 144.0, 1e-9), "outer CCW: {areas:?}");
        assert!(close(areas[1], -64.0, 1e-9), "middle CW: {areas:?}");
        assert!(close(areas[2], 16.0, 1e-9), "inner CCW: {areas:?}");
    }

    #[test]
    fn classify_empty_errors() {
        assert_eq!(classify_wires(Vec::new()), Err(ProfileError::EmptyGeometry));
    }

    // -- extrusion ----------------------------------------------------------

    #[test]
    fn extrude_square_exact_volume_and_bbox() {
        let wires = extract_wires(&square_geoms(), DEFAULT_TOLERANCE, 0.01).unwrap();
        let mesh = extrude(&wires, Vector3::Z, 10.0, false, 0.01).unwrap();
        assert!(mesh.validate().is_ok());
        assert_watertight(&mesh);
        assert_unique_vertices(&mesh);
        // Vertices are exact, so the volume is an analytic equality.
        assert!(
            close(mesh.volume(), 100.0 * 10.0, 1e-9),
            "volume {}",
            mesh.volume()
        );
        let bbox = mesh.bbox().unwrap();
        assert_eq!(bbox.min, Vector3::new(-5.0, -5.0, 0.0));
        assert_eq!(bbox.max, Vector3::new(5.0, 5.0, 10.0));
        // 8 side triangles + 2 + 2 cap triangles.
        assert_eq!(mesh.triangle_count(), 12);
        assert!(mesh.volume() > 0.0);
    }

    #[test]
    fn extrude_circle_volume_within_tessellation_error() {
        let geoms = vec![circle(0.0, 0.0, 5.0)];
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.005).unwrap();
        let mesh = extrude(&wires, Vector3::Z, 10.0, false, 0.005).unwrap();
        assert_watertight(&mesh);
        let want = PI * 25.0 * 10.0;
        assert!(
            rel_err(mesh.volume(), want) < 0.01,
            "{} vs {want}",
            mesh.volume()
        );
        assert!(mesh.volume() > 0.0);
    }

    #[test]
    fn extrude_washer_hole_is_really_a_hole() {
        let mut geoms = square_geoms();
        geoms.push(circle(0.0, 0.0, 2.0));
        let classified = classify_wires(extract_wires(&geoms, DEFAULT_TOLERANCE, 0.005).unwrap())
            .expect("classify");
        let mesh = extrude(&classified, Vector3::Z, 10.0, false, 0.005).unwrap();
        assert_watertight(&mesh);
        assert_unique_vertices(&mesh);
        let want = (100.0 - PI * 4.0) * 10.0;
        assert!(
            rel_err(mesh.volume(), want) < 0.01,
            "{} vs {want}",
            mesh.volume()
        );
        // A point on the hole's axis is OUTSIDE the solid; a point in the
        // material and a point beyond the outer wall behave as expected.
        assert_outside(&mesh, Vector3::new(0.0, 0.3, 5.0));
        assert_inside(&mesh, Vector3::new(4.0, 0.3, 5.0));
        assert_outside(&mesh, Vector3::new(6.0, 0.3, 5.0));
    }

    #[test]
    fn extrude_two_holes_stay_watertight() {
        let mut geoms = square_geoms();
        geoms.push(circle(-2.0, 0.0, 1.0));
        geoms.push(circle(2.0, 0.0, 1.0));
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, 0.005).unwrap();
        let mesh = extrude(&wires, Vector3::Z, 10.0, false, 0.005).unwrap();
        assert_watertight(&mesh);
        let want = (100.0 - 2.0 * PI) * 10.0;
        assert!(
            rel_err(mesh.volume(), want) < 0.01,
            "{} vs {want}",
            mesh.volume()
        );
        assert_outside(&mesh, Vector3::new(-2.0, 0.3, 5.0));
        assert_outside(&mesh, Vector3::new(2.0, 0.3, 5.0));
    }

    #[test]
    fn extrude_rounded_rect_sagitta_and_watertight() {
        // 10x10 rect with corner radius 2: 4 lines + 4 quarter arcs.
        let geoms = vec![
            line(2.0, 0.0, 8.0, 0.0),
            arc(8.0, 2.0, 2.0, -PI / 2.0, 0.0),
            line(10.0, 2.0, 10.0, 8.0),
            arc(8.0, 8.0, 2.0, 0.0, PI / 2.0),
            line(8.0, 10.0, 2.0, 10.0),
            arc(2.0, 8.0, 2.0, PI / 2.0, PI),
            line(0.0, 8.0, 0.0, 2.0),
            arc(2.0, 2.0, 2.0, PI, 3.0 * PI / 2.0),
        ];
        let deflection: f64 = 0.005;
        let wires = extract_wires(&geoms, DEFAULT_TOLERANCE, deflection).unwrap();
        assert_eq!(wires.len(), 1);
        let mesh = extrude(&wires, Vector3::Z, 5.0, false, deflection).unwrap();
        assert_watertight(&mesh);
        // Corner arc sagitta <= deflection: every chord whose endpoints sit
        // on a known arc must pass within deflection of that arc's center.
        let centers = [
            Point2::new(8.0, 2.0),
            Point2::new(8.0, 8.0),
            Point2::new(2.0, 8.0),
            Point2::new(2.0, 2.0),
        ];
        let pts = wires[0].points();
        for pair in pts.windows(2) {
            for c in centers {
                let on_arc = close(pair[0].dist(c), 2.0, 1e-9) && close(pair[1].dist(c), 2.0, 1e-9);
                if !on_arc {
                    continue;
                }
                let mid = (pair[0] + pair[1]) * 0.5;
                let sagitta = 2.0 - mid.dist(c);
                assert!(
                    sagitta <= deflection + 1e-9,
                    "sagitta {sagitta} > {deflection} at chord {:?}",
                    pair
                );
            }
        }
        // Area = 100 - (4 - pi)*r^2.
        let want = (100.0 - (4.0 - PI) * 4.0) * 5.0;
        assert!(
            rel_err(mesh.volume(), want) < 0.01,
            "{} vs {want}",
            mesh.volume()
        );
    }

    #[test]
    fn extrude_midplane_spans_symmetrically() {
        let wires = extract_wires(&square_geoms(), DEFAULT_TOLERANCE, 0.01).unwrap();
        let mesh = extrude(&wires, Vector3::Z, 10.0, true, 0.01).unwrap();
        assert_watertight(&mesh);
        let bbox = mesh.bbox().unwrap();
        assert_eq!(bbox.min.z, -5.0);
        assert_eq!(bbox.max.z, 5.0);
        assert!(close(mesh.volume(), 1000.0, 1e-9));
        assert!(mesh.volume() > 0.0);
    }

    #[test]
    fn extrude_direction_is_normalized_reversible_and_skewable() {
        let wires = extract_wires(&square_geoms(), DEFAULT_TOLERANCE, 0.01).unwrap();
        let unit = extrude(&wires, Vector3::Z, 10.0, false, 0.01).unwrap();
        let scaled = extrude(&wires, Vector3::new(0.0, 0.0, 7.0), 10.0, false, 0.01).unwrap();
        assert_eq!(unit, scaled, "direction must be normalized internally");

        let flipped = extrude(&wires, Vector3::new(0.0, 0.0, -1.0), 10.0, false, 0.01).unwrap();
        assert_watertight(&flipped);
        assert!(
            close(flipped.volume(), 1000.0, 1e-9),
            "reversed pad stays positive"
        );
        assert_eq!(flipped.bbox().unwrap().min.z, -10.0);
        assert_eq!(flipped.bbox().unwrap().max.z, 0.0);

        // Skew direction: the top ring is sheared by `length * dir_n`, so the
        // solid is an oblique prism: V = area * length * (dir_n . z) and the
        // top face sits at z = length * dir_n.z.
        let skew = extrude(&wires, Vector3::new(1.0, 0.0, 1.0), 10.0, false, 0.01).unwrap();
        assert_watertight(&skew);
        assert_unique_vertices(&skew);
        let want = 1000.0 / (2.0f64).sqrt();
        assert!(
            close(skew.volume(), want, 1e-6),
            "volume {} vs {want}",
            skew.volume()
        );
        let bbox = skew.bbox().unwrap();
        let l = 10.0 / (2.0f64).sqrt();
        assert!(close(bbox.max.z, l, 1e-9) && close(bbox.min.z, 0.0, 1e-9));
        assert!(close(bbox.max.x, 5.0 + l, 1e-9), "sheared top extends +x");
    }

    #[test]
    fn extrude_rejects_bad_parameters() {
        let wires = extract_wires(&square_geoms(), DEFAULT_TOLERANCE, 0.01).unwrap();
        assert_eq!(
            extrude(&wires, Vector3::Z, 0.0, false, 0.01),
            Err(ProfileError::InvalidLength(0.0))
        );
        assert_eq!(
            extrude(&wires, Vector3::Z, -3.0, false, 0.01),
            Err(ProfileError::InvalidLength(-3.0))
        );
        assert!(matches!(
            extrude(&wires, Vector3::Z, f64::NAN, false, 0.01),
            Err(ProfileError::InvalidLength(_))
        ));
        assert!(matches!(
            extrude(&wires, Vector3::ZERO, 10.0, false, 0.01),
            Err(ProfileError::InvalidDirection(_))
        ));
        assert!(matches!(
            extrude(&wires, Vector3::new(f64::NAN, 0.0, 1.0), 10.0, false, 0.01),
            Err(ProfileError::InvalidDirection(_))
        ));
        // In-plane direction: a Pad along the sketch plane is degenerate.
        assert!(matches!(
            extrude(&wires, Vector3::new(0.0, 1.0, 0.0), 10.0, false, 0.01),
            Err(ProfileError::InvalidDirection(_))
        ));
        assert!(matches!(
            extrude(&wires, Vector3::Z, 10.0, false, 0.0),
            Err(ProfileError::InvalidInput(_))
        ));
        assert_eq!(
            extrude(&[], Vector3::Z, 10.0, false, 0.01),
            Err(ProfileError::EmptyGeometry)
        );
    }

    // -- revolution ---------------------------------------------------------

    /// Half-disc r = 5 with its diameter on the axis: line (0,10)->(0,0)
    /// plus a CCW arc bulging +u. Revolved about Z this is a sphere.
    fn half_disc_geoms() -> Vec<Geometry> {
        vec![
            line(0.0, 10.0, 0.0, 0.0),
            arc(0.0, 5.0, 5.0, -PI / 2.0, PI / 2.0),
        ]
    }

    #[test]
    fn revolve_half_disc_into_sphere() {
        let wires = extract_wires(&half_disc_geoms(), DEFAULT_TOLERANCE, 0.005).unwrap();
        assert_eq!(wires.len(), 1);
        let mesh = revolve(&wires, Vector3::ZERO, Vector3::Z, 360.0, 0.005).unwrap();
        assert_watertight(&mesh);
        assert_unique_vertices(&mesh);
        let want = 4.0 / 3.0 * PI * 125.0;
        assert!(
            rel_err(mesh.volume(), want) < 0.01,
            "{} vs {want}",
            mesh.volume()
        );
        assert!(mesh.volume() > 0.0);
        let bbox = mesh.bbox().unwrap();
        // Ring 0 lands exactly on theta = 0 (+X reached), but with 71 rings
        // none lands exactly on theta = pi: the x extent stays within one
        // ring step of the analytic sphere.
        assert!(close(bbox.min.x, -5.0, 0.01) && close(bbox.max.x, 5.0, 1e-9));
        assert!(close(bbox.min.z, 0.0, 1e-9) && close(bbox.max.z, 10.0, 1e-9));
    }

    #[test]
    fn revolve_rectangle_into_seam_welded_cylinder() {
        // One edge on the axis: (0,0)->(2,0)->(2,3)->(0,3).
        let wires = vec![w(&[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)])];
        let deflection: f64 = 0.001;
        let mesh = revolve(&wires, Vector3::ZERO, Vector3::Z, 360.0, deflection).unwrap();
        assert_watertight(&mesh);
        assert_unique_vertices(&mesh);
        assert_no_degenerate_triangles(&mesh, 1e-12);
        assert!(mesh.volume() > 0.0);
        let want = PI * 4.0 * 3.0;
        assert!(
            rel_err(mesh.volume(), want) < 0.005,
            "{} vs {want}",
            mesh.volume()
        );

        // Seam welding: exactly n rings over the 2 off-axis points plus the
        // 2 fixed axis points — no duplicated seam ring.
        let step_max = 2.0 * (1.0 - deflection / 2.0).clamp(0.0, 1.0).acos();
        let n = (((2.0 * PI) / step_max).ceil() as usize).clamp(16, 512);
        assert_eq!(mesh.vertex_count(), 2 + 2 * n);
        // 2 cones (n each) + 1 quad strip (2n) = 4n triangles; no caps.
        assert_eq!(mesh.triangle_count(), 4 * n);
    }

    #[test]
    fn revolve_half_turn_halves_volume_and_caps() {
        let wires = vec![w(&[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)])];
        let full = revolve(&wires, Vector3::ZERO, Vector3::Z, 360.0, 0.001).unwrap();
        let half = revolve(&wires, Vector3::ZERO, Vector3::Z, 180.0, 0.001).unwrap();
        assert_watertight(&half);
        assert!(rel_err(half.volume(), full.volume() / 2.0) < 0.01);
        let want = PI * 4.0 * 3.0 / 2.0;
        assert!(
            rel_err(half.volume(), want) < 0.01,
            "{} vs {want}",
            half.volume()
        );
        // Flat caps present: both lie in the y = 0 plane (theta = 0 and pi).
        let flat = half
            .triangles
            .iter()
            .filter(|t| {
                t.iter().all(|&i| {
                    let v = half.vertices[i as usize];
                    v.y.abs() < 1e-9
                })
            })
            .count();
        assert_eq!(flat, 4, "two caps of two triangles each, found {flat}");
        assert!(half.volume() > 0.0);
    }

    #[test]
    fn revolve_annulus_full_turn_is_watertight() {
        // Rect (1..3 x 0..2) with a rectangular hole (1.5..2.5 x 0.5..1.5):
        // V = 2*pi*(A_outer*x_outer - A_hole*x_hole) = 12*pi.
        let outer = w(&[(1.0, 0.0), (3.0, 0.0), (3.0, 2.0), (1.0, 2.0)]);
        let hole = w(&[(1.5, 0.5), (1.5, 1.5), (2.5, 1.5), (2.5, 0.5)]);
        let classified = classify_wires(vec![outer, hole]).unwrap();
        let mesh = revolve(&classified, Vector3::ZERO, Vector3::Z, 360.0, 0.001).unwrap();
        assert_watertight(&mesh);
        assert_unique_vertices(&mesh);
        let want = 12.0 * PI;
        assert!(
            rel_err(mesh.volume(), want) < 0.001,
            "{} vs {want}",
            mesh.volume()
        );
        assert!(mesh.volume() > 0.0);
    }

    #[test]
    fn revolve_negative_angle_mirrors() {
        let wires = vec![w(&[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)])];
        let cw = revolve(&wires, Vector3::ZERO, Vector3::Z, -180.0, 0.001).unwrap();
        let ccw = revolve(&wires, Vector3::ZERO, Vector3::Z, 180.0, 0.001).unwrap();
        assert_watertight(&cw);
        assert!(
            close(cw.volume(), ccw.volume(), 1e-6),
            "mirror sweep, same volume"
        );
        assert!(cw.volume() > 0.0);
    }

    #[test]
    fn revolve_axis_base_places_the_solid() {
        let wires = vec![w(&[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)])];
        let mesh = revolve(
            &wires,
            Vector3::new(3.0, 0.0, 0.0),
            Vector3::Z,
            360.0,
            0.001,
        )
        .unwrap();
        assert_watertight(&mesh);
        let bbox = mesh.bbox().unwrap();
        assert!(close(bbox.min.x, 1.0, 1e-6) && close(bbox.max.x, 5.0, 1e-6));
        assert!(close(bbox.min.y, -2.0, 1e-6) && close(bbox.max.y, 2.0, 1e-6));
        assert!(close(mesh.volume(), PI * 4.0 * 3.0, 0.05));
    }

    #[test]
    fn revolve_overshoot_angle_is_a_full_turn() {
        let wires = vec![w(&[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)])];
        let exact = revolve(&wires, Vector3::ZERO, Vector3::Z, 360.0, 0.001).unwrap();
        let over = revolve(&wires, Vector3::ZERO, Vector3::Z, 720.0, 0.001).unwrap();
        assert_eq!(exact.vertex_count(), over.vertex_count());
        assert_eq!(exact.triangle_count(), over.triangle_count());
        assert!(close(exact.volume(), over.volume(), 1e-9));
    }

    #[test]
    fn revolve_rejects_bad_parameters() {
        let wires = vec![w(&[(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)])];
        assert_eq!(
            revolve(&wires, Vector3::ZERO, Vector3::Z, 0.0, 0.01),
            Err(ProfileError::InvalidAngle(0.0))
        );
        assert!(matches!(
            revolve(&wires, Vector3::ZERO, Vector3::Z, f64::NAN, 0.01),
            Err(ProfileError::InvalidAngle(_))
        ));
        assert!(matches!(
            revolve(&wires, Vector3::ZERO, Vector3::ZERO, 90.0, 0.01),
            Err(ProfileError::InvalidDirection(_))
        ));
        assert!(matches!(
            revolve(
                &wires,
                Vector3::ZERO,
                Vector3::new(0.0, f64::NAN, 1.0),
                90.0,
                0.01
            ),
            Err(ProfileError::InvalidDirection(_))
        ));
        assert!(matches!(
            revolve(&wires, Vector3::ZERO, Vector3::Z, 90.0, 0.0),
            Err(ProfileError::InvalidInput(_))
        ));
        assert_eq!(
            revolve(&[], Vector3::ZERO, Vector3::Z, 90.0, 0.01),
            Err(ProfileError::EmptyGeometry)
        );
        // Entirely on the axis: nothing to sweep.
        let on_axis = vec![w(&[(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)])];
        assert!(matches!(
            revolve(&on_axis, Vector3::ZERO, Vector3::Z, 90.0, 0.01),
            Err(ProfileError::InvalidInput(_))
        ));
        // Deep nesting rejected before any sweeping.
        let deep = vec![
            w(&[(-6.0, -6.0), (6.0, -6.0), (6.0, 6.0), (-6.0, 6.0)]),
            w(&[(-4.0, -4.0), (4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)]),
            w(&[(-2.0, -2.0), (2.0, -2.0), (2.0, 2.0), (-2.0, 2.0)]),
        ];
        assert!(matches!(
            revolve(&deep, Vector3::ZERO, Vector3::Z, 90.0, 0.01),
            Err(ProfileError::InvalidInput(_))
        ));
    }

    // -- contracts ----------------------------------------------------------

    #[test]
    fn classified_profiles_keep_positive_volume() {
        // The binding contract: extract -> classify -> kernel keeps
        // Mesh::volume() positive for both kernels on a hole-bearing profile.
        // (The revolved profile stays strictly on the +u side of the axis:
        // a profile crossing the axis sweeps a self-cancelling shell whose
        // signed volume is ~0, which is exactly the caller's problem.)
        let geoms = vec![
            line(1.0, 1.0, 4.0, 1.0),
            line(4.0, 1.0, 4.0, 4.0),
            line(4.0, 4.0, 1.0, 4.0),
            line(1.0, 4.0, 1.0, 1.0),
            circle(2.5, 2.5, 0.75),
        ];
        let classified = classify_wires(extract_wires(&geoms, DEFAULT_TOLERANCE, 0.005).unwrap())
            .expect("classify");
        let pad = extrude(&classified, Vector3::Z, 10.0, false, 0.005).unwrap();
        assert_watertight(&pad);
        assert!(pad.volume() > 0.0);
        let want_pad = (9.0 - PI * 0.5625) * 10.0;
        assert!(rel_err(pad.volume(), want_pad) < 0.01);
        let revolved = revolve(&classified, Vector3::ZERO, Vector3::Z, 360.0, 0.005).unwrap();
        assert_watertight(&revolved);
        assert!(revolved.volume() > 0.0);
    }

    #[test]
    fn wire_new_rejects_degenerate_input() {
        assert!(matches!(
            Wire::new(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)]),
            Err(ProfileError::InvalidInput(_))
        ));
        assert!(matches!(
            Wire::new(vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 0.0),
            ]),
            Err(ProfileError::InvalidInput(_))
        ));
        // Duplicated closing point is rejected (first == last).
        assert!(matches!(
            Wire::new(vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
                Point2::new(0.0, 0.0),
            ]),
            Err(ProfileError::InvalidInput(_))
        ));
        assert!(matches!(
            Wire::new(vec![
                Point2::new(0.0, f64::NAN),
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
            ]),
            Err(ProfileError::InvalidInput(_))
        ));
    }

    #[test]
    fn error_display_is_honest() {
        let e = ProfileError::OpenWire {
            free_endpoints: vec![Point2::new(0.0, 0.0), Point2::new(0.5, 1.0)],
        };
        let msg = e.to_string();
        assert!(
            msg.contains("open wire") && msg.contains("2 dangling"),
            "{msg}"
        );
        assert!(!ProfileError::EmptyGeometry.to_string().is_empty());
        assert!(!ProfileError::InvalidLength(-1.0).to_string().is_empty());
        assert!(!ProfileError::InvalidAngle(0.0).to_string().is_empty());
        assert!(!ProfileError::TriangulationFailed("x".into())
            .to_string()
            .is_empty());
        assert!(!ProfileError::InvalidDirection("y".into())
            .to_string()
            .is_empty());
        assert!(!ProfileError::InvalidInput("z".into())
            .to_string()
            .is_empty());
        // std::error::Error is implemented (compile-time check).
        fn assert_error<T: std::error::Error>(_: &T) {}
        assert_error(&e);
    }
}
