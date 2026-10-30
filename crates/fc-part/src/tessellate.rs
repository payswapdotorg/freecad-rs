//! Tessellating the exact topology model — the K4 viewer path.
//!
//! [`tessellate_brep`] turns an [`ExactBRep`] into a watertight
//! [`Mesh`] under the kernel's sagitta discipline (chord error ≤
//! `deflection`), the way real B-rep kernels do it:
//!
//! 1. **Every edge is discretized once** — adaptively, by recursive
//!    bisection until each chord's midpoint misses the curve by at
//!    most the deflection — so the two faces sharing an edge inherit
//!    *the same* 3D points and the shell closes.
//! 2. **Each face is triangulated in its parameter domain**: the
//!    boundary edge samples are projected onto the surface (closed
//!    forms for the analytic five), unwrapped continuously along the
//!    wire (the seam edge of a periodic surface walks `u = 0` up and
//!    `u = 2π` down), and the UV region is triangulated by the profile
//!    kernel's planar triangulator.
//! 3. **Curved faces grow interior samples** (K4-2b): a grid laid out
//!    at the surface's own curvature steps — the same sagitta
//!    discipline the edge sampler applies to rims — is inserted into
//!    the domain triangulation, and a deflection referee splits any
//!    interior edge whose chord, or panel whose center, misses the
//!    surface by more than the deflection. Boundary samples are exact
//!    rim geometry; interior nodes are surface evaluations,
//!    deflection-true where they sit.
//! 4. **Degenerate edges** (sphere poles, cone apexes) are single
//!    points in 3D but real boundary in UV: they span the face's full
//!    `u` range at the pole's `v`, and the pole fan emerges as that
//!    boundary run collapses onto the one 3D point.
//!
//! Orientation: each face's triangles wind against the surface's
//! natural normal (`∂u×∂v`), flipped when `reversed`; the assembled
//! shell is reconciled through the shared-edge graph and the global
//! divergence sign (the same discipline the STEP importer applies to
//! foreign shells), so the result is a solid boundary with outward
//! normals.
//!
//! Refused by name: B-spline faces (no curvature estimate drives
//! their interior sampling yet), hole-trimmed faces (the 2D kernel's
//! bridging exists; the seam and stitch bookkeeping across hole wires
//! does not), untrimmed faces (no wires bound the region), and
//! non-positive-volume results.

use std::collections::{HashMap, HashSet};

use fc_base::Vector3;

use crate::boolean::orient_closed_shell;
use crate::profile::{
    point_in_polygon, triangulate_planar_region_keep_boundary as triangulate_planar_region, Point2,
};
use crate::shape::Mesh;
use crate::topology::{Curve, ExactBRep, Surface};

/// The u-period of a surface's parameterization (`None` for the
/// plane and B-splines — periodic B-splines are refused at read time,
/// and the plane is aperiodic).
fn u_period(s: &Surface) -> Option<f64> {
    match s {
        Surface::Plane { .. } | Surface::Bspline { .. } => None,
        _ => Some(std::f64::consts::TAU),
    }
}

/// The v-period (the torus only).
fn v_period(s: &Surface) -> Option<f64> {
    match s {
        Surface::Torus { .. } => Some(std::f64::consts::TAU),
        _ => None,
    }
}

/// Project a 3D point onto the surface's parameters. Closed forms for
/// the analytic five; for B-spline surfaces a coarse domain scan
/// followed by Gauss–Newton refinement of the squared distance (the
/// point lies on the surface, so a few steps land on it).
fn surface_uv_of(s: &Surface, p: Vector3) -> Result<(f64, f64), String> {
    match s {
        Surface::Plane {
            origin,
            normal,
            x_dir,
        } => {
            let y = normal.cross(x_dir);
            let d = p - *origin;
            Ok((d.dot(x_dir), d.dot(&y)))
        }
        Surface::Cylinder {
            origin,
            axis,
            ref_dir,
            ..
        } => {
            let y = axis.cross(ref_dir);
            let d = p - *origin;
            let v = d.dot(axis);
            // u runs CCW from ref_dir (atan2 of the y-component over
            // the x-component).
            Ok((d.dot(&y).atan2(d.dot(ref_dir)), v))
        }
        Surface::Cone {
            origin,
            axis,
            ref_dir,
            semi_angle,
            ..
        } => {
            let y = axis.cross(ref_dir);
            let d = p - *origin;
            let v = d.dot(axis) / semi_angle.cos();
            Ok((d.dot(&y).atan2(d.dot(ref_dir)), v))
        }
        Surface::Sphere {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            let y = axis.cross(ref_dir);
            let d = p - *center;
            let u = d.dot(&y).atan2(d.dot(ref_dir));
            let v = (d.dot(axis) / *radius).clamp(-1.0, 1.0).asin();
            Ok((u, v))
        }
        Surface::Torus {
            center,
            axis,
            ref_dir,
            major_radius,
            ..
        } => {
            let y = axis.cross(ref_dir);
            let d = p - *center;
            let z = d.dot(axis);
            let du = d.dot(ref_dir);
            let dv = d.dot(&y);
            let rho = (du * du + dv * dv).sqrt();
            let u = dv.atan2(du);
            // cos v = (rho − R)/r and sin v = z/r: v is their atan2 —
            // an asin of the cosine would return the complement.
            let v = z.atan2(rho - *major_radius);
            Ok((u, v))
        }
        Surface::Bspline {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            points,
            ..
        } => {
            let (ulo, uhi) = (u_knots[*u_degree], u_knots[points.len()]);
            let (vlo, vhi) = (v_knots[*v_degree], v_knots[points[0].len()]);
            // Coarse scan for a start.
            let mut best = (0.5 * (ulo + uhi), 0.5 * (vlo + vhi));
            let mut best_d2 = f64::INFINITY;
            for i in 0..8usize {
                for j in 0..8usize {
                    let u = ulo + (uhi - ulo) * (i as f64 + 0.5) / 8.0;
                    let v = vlo + (vhi - vlo) * (j as f64 + 0.5) / 8.0;
                    let d2 = (s.point(u, v) - p).length_squared();
                    if d2 < best_d2 {
                        best_d2 = d2;
                        best = (u, v);
                    }
                }
            }
            // Gauss–Newton on the squared distance: solve
            // [qu·qu  qu·qv][su]   [-qu·d]
            // [qu·qv  qv·qv][sv] = [-qv·d]
            let (mut u, mut v) = best;
            for _ in 0..16 {
                let d = s.point(u, v) - p;
                if !d.x.is_finite() || !d.y.is_finite() || !d.z.is_finite() {
                    return Err("the surface evaluates non-finite".to_string());
                }
                let (qu, qv) = s.derivatives(u, v);
                let a = qu.dot(&qu);
                let b = qu.dot(&qv);
                let c = qv.dot(&qv);
                let det = a * c - b * b;
                if det <= 1e-30 {
                    break;
                }
                let r0 = -qu.dot(&d);
                let r1 = -qv.dot(&d);
                let su = (c * r0 - b * r1) / det;
                let sv = (a * r1 - b * r0) / det;
                let un = (u + su).clamp(ulo, uhi);
                let vn = (v + sv).clamp(vlo, vhi);
                let done = (un - u).abs() < 1e-13 && (vn - v).abs() < 1e-13;
                u = un;
                v = vn;
                if done {
                    break;
                }
            }
            Ok((u, v))
        }
    }
}

/// Adaptive edge discretization: `points[0]` and the last point are
/// the endpoints; every chord's midpoint deviates from the curve by
/// at most `deflection`.
fn sample_edge(curve: &Curve, range: (f64, f64), deflection: f64) -> Vec<Vector3> {
    let p0 = curve.point(range.0);
    let mut out = vec![p0];
    subdivide(
        curve,
        range.0,
        range.1,
        p0,
        curve.point(range.1),
        deflection,
        0,
        &mut out,
    );
    out
}

#[allow(clippy::too_many_arguments)]
fn subdivide(
    curve: &Curve,
    t0: f64,
    t1: f64,
    p0: Vector3,
    p1: Vector3,
    deflection: f64,
    depth: usize,
    out: &mut Vec<Vector3>,
) {
    let tm = 0.5 * (t0 + t1);
    let pm = curve.point(tm);
    let sagitta = (pm - (p0 * 0.5 + p1 * 0.5)).length();
    if sagitta <= deflection || depth >= 24 {
        out.push(p1);
        return;
    }
    subdivide(curve, t0, tm, p0, pm, deflection, depth + 1, out);
    subdivide(curve, tm, t1, pm, p1, deflection, depth + 1, out);
}

/// One boundary vertex of a face's UV region.
struct BoundaryVert {
    uv: Point2,
    p3: Vector3,
}

/// Tessellate the exact B-rep into a watertight mesh.
pub fn tessellate_brep(brep: &ExactBRep, deflection: f64) -> Result<Mesh, String> {
    if !deflection.is_finite() || deflection <= 0.0 {
        return Err(format!(
            "deflection must be finite and > 0, got {deflection}"
        ));
    }
    brep.validate()?;
    // Planar faces are exact in their own plane; the analytic curved
    // five triangulate their parameter domains with curvature-
    // respecting interior samples (K4-2b). B-spline faces refuse by
    // name: their interior sampling needs surface curvature estimates
    // this kernel does not yet carry, and a parameters-flat grid that
    // pretended to be deflection-true on the surface would be exactly
    // the slanted-panel lie this milestone removes for the analytic
    // five.
    for (fi, f) in brep.faces.iter().enumerate() {
        if matches!(f.surface, Surface::Bspline { .. }) {
            return Err(format!(
                "face {fi}: a Bspline face — the B-spline domain triangulation is not \
                 deflection-true yet (analytic surfaces only)"
            ));
        }
    }

    // 1. Edge discretization, shared by every face. A closed edge's
    //    own 2π rounding leaves its last sample a bit-level stranger
    //    to the vertex it closes on; it is snapped to the first so
    //    every consumer of the rim shares ONE vertex index there and
    //    the rim — and any periodic seam — closes bit-exactly.
    let edge_samples: Vec<Vec<Vector3>> = brep
        .edges
        .iter()
        .map(|e| {
            let mut samples = sample_edge(&e.curve, e.range, deflection);
            if e.start == e.end {
                if let (Some(first), Some(last)) = (samples.first().copied(), samples.last_mut()) {
                    *last = first;
                }
            }
            samples
        })
        .collect();

    // The mesh vertex table, deduplicated by exact coordinates —
    // shared edge samples coincide bit-exactly by construction, so
    // exact dedup cannot weld distinct points.
    let mut vertices: Vec<Vector3> = Vec::new();
    let mut index_of: HashMap<[u64; 3], u32> = HashMap::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();

    for (fi, face) in brep.faces.iter().enumerate() {
        if face.wires.is_empty() {
            return Err(format!(
                "face {fi}: an untrimmed face — no wires bound the region"
            ));
        }
        let [umin, umax, vmin, vmax] = face.uv_bounds;
        let uper = u_period(&face.surface);
        let vper = v_period(&face.surface);

        // The boundary loops in (unwrapped) UV.
        let mut loops: Vec<Vec<BoundaryVert>> = Vec::new();
        for wire in &face.wires {
            let mut loop_verts: Vec<BoundaryVert> = Vec::new();
            let mut prev: Option<(f64, f64)> = None;
            let last_use = wire.edges.len().saturating_sub(1);
            for we in wire.edges.iter() {
                let e = &brep.edges[we.edge];
                if brep.is_degenerate(we.edge) {
                    // The pole line: a single 3D point spanning the
                    // face's full u range at the pole's v. The pole's
                    // own v comes from the closed-form inversion (a
                    // pole/apex is a bound of the v range) — never
                    // inherited from the walk, which stops one sample
                    // short of the pole.
                    let p3 = e.curve.point(e.range.0);
                    let probe = surface_uv_of(&face.surface, p3)
                        .map(|(_, v)| v)
                        .unwrap_or(vmin);
                    let v_pole = if (probe - vmin).abs() < (probe - vmax).abs() {
                        vmin
                    } else {
                        vmax
                    };
                    let u0 = prev.map(|(u, _)| u).unwrap_or(umin);
                    let u1 = if (u0 - umin).abs() < (umax - u0).abs() {
                        umax
                    } else {
                        umin
                    };
                    for k in 0..8usize {
                        let u = u0 + (u1 - u0) * (k as f64 / 8.0);
                        loop_verts.push(BoundaryVert {
                            uv: Point2 { x: u, y: v_pole },
                            p3,
                        });
                    }
                    // The pole line's far end continues into the next
                    // edge-use (which re-anchors the walk there).
                    prev = Some((u1, v_pole));
                    continue;
                }
                // Each edge-use contributes its samples except its
                // closing point: the next use provides it, and the
                // last use's closing point is the loop's first vertex
                // (the wire is closed — the K1 contract). A reversed
                // use walks the same samples from the far end. A
                // CLOSED edge (a rim circle) contributes ALL of its
                // samples: the last one is the loop closure itself
                // (bit-approximately the first), and dropping it
                // would make the neighboring face's closing chord cut
                // across the shared rim vertex.
                let closed = e.start == e.end;
                let n = edge_samples[we.edge].len();
                let take = if closed { n } else { n - 1 };
                let walked: Vec<Vector3> = if we.forward {
                    edge_samples[we.edge].iter().take(take).copied().collect()
                } else {
                    edge_samples[we.edge]
                        .iter()
                        .rev()
                        .take(take)
                        .copied()
                        .collect()
                };
                let _ = last_use;
                for &p3 in walked.iter() {
                    let (u_raw, v_raw) = surface_uv_of(&face.surface, p3)
                        .map_err(|err| format!("face {fi}: {err}"))?;
                    let (mut u, mut v) = (u_raw, v_raw);
                    // A sample at a pole or apex sits ON the axis: its u
                    // is atan2 of noise (a half-turn away from the
                    // walk's continuity), and the unwrap would drag the
                    // whole seam a full period off. Inherit the walk's
                    // u instead — every pole maps to one 3D point, so
                    // the inherited value is exactly as true.
                    let on_axis = match &face.surface {
                        Surface::Sphere { center, axis, .. }
                        | Surface::Cone {
                            origin: center,
                            axis,
                            ..
                        } => {
                            let d = p3 - *center;
                            let along = d.dot(axis);
                            let perp = (d - *axis * along).length();
                            perp <= 1e-9 * d.length().max(1e-30)
                        }
                        _ => false,
                    };
                    match prev {
                        Some((pu, pv)) => {
                            if let Some(p) = uper {
                                u = if on_axis {
                                    pu
                                } else {
                                    pu + (u - pu + p / 2.0).rem_euclid(p) - p / 2.0
                                };
                            }
                            if let Some(p) = vper {
                                v = pv + (v - pv + p / 2.0).rem_euclid(p) - p / 2.0;
                            }
                        }
                        None => {
                            if let Some(p) = uper {
                                u = umin + (u - umin).rem_euclid(p);
                            }
                            if let Some(p) = vper {
                                v = vmin + (v - vmin).rem_euclid(p);
                            }
                        }
                    }
                    loop_verts.push(BoundaryVert {
                        uv: Point2 { x: u, y: v },
                        p3,
                    });
                    prev = Some((u, v));
                }
            }
            if loop_verts.len() < 3 {
                return Err(format!("face {fi}: a degenerate boundary loop"));
            }
            loops.push(loop_verts);
        }

        // Triangulate the parameter domain with the planar kernel,
        // then STITCH the boundary vertices it dropped. On a periodic
        // surface the shared rim samples project to parameter-
        // collinear runs, and the 2D ear-clip removes collinear
        // vertices — but in 3D those points are the very vertices the
        // neighboring face carries, so dropping them opens the shell
        // (a T-junction whose "edge" is a 3D point). The stitch
        // re-inserts each dropped point by splitting the triangle
        // edge that spans it: legitimate in the parameter domain,
        // where the point is genuinely on the segment, and proper in
        // 3D, where the two split triangles are the deflection-bounded
        // slivers through the rim point.
        if loops.len() != 1 {
            return Err(format!(
                "face {fi}: a UV region with {} wires (holes) — hole-trimmed faces are not \
                 tessellated by this reader yet",
                loops.len()
            ));
        }
        let loop_verts = &loops[0];
        let n = loop_verts.len();
        let outer: Vec<Point2> = loop_verts.iter().map(|b| b.uv).collect();

        // The emit table: boundary vertices keep their shared 3D rim
        // samples bit-exactly; interior domain vertices (curved faces
        // only) evaluate the surface at their parameters — or, on the
        // seam, share one evaluated point from both sides so the seam
        // closes bit-exactly.
        let (face_tris, p3): (Vec<[u32; 3]>, Vec<Vector3>) =
            if !matches!(face.surface, Surface::Plane { .. }) {
                // K4-2b: a curved face's domain is a monotone band — its
                // boundary walk splits into two u-monotone chains (rim
                // chains, pole lines, seam lines), and the curvature-
                // respecting grid rows stack between them. The band
                // merge tiles that stack directly, so the rim chains
                // enter as genuine boundary edges — the neighboring
                // face's own samples, shared bit-exactly — and the
                // deflection referee only refines from there. No ear
                // clip, no fan, no stitch: the slanted-panel defect
                // cannot arise.
                //
                // Region inference first: the band machinery below tiles
                // the INTERIOR of a boundary walk that bounds the
                // face's own UV box — every K4-1/K4-2 corpus face
                // walks its box (natural domains and seam-wrapped bands
                // span the u period up to end-sampling slop; a partial
                // band touches its box's u-extremes), the rim chains
                // entering as genuine boundary edges. A loop that
                // neither spans the period nor touches the extremes is
                // an AMBIGUOUS trim: the same loop+box serves the cap
                // (interior) and the complement (the sphere minus its
                // cap — the exact booleans' outer faces), and tiling
                // the interior unconditionally would render the wrong
                // face as a plausible-looking mesh. A cap's loop spans
                // at most half the period (2·atan(ρ/c) < π), so
                // three-quarters of the period separates every
                // box-bounding walk from every cap/complement loop with
                // margin. The wire-orientation convention that would
                // classify the ambiguous case is real (region on the
                // left of the traversal, viewed from the outward
                // normal) but its per-surface parameter-orientation
                // signs are not yet carried here — the trim-domain
                // classification slice (K4-2d). Until then: refuse by
                // name, the exact channels still measuring the solid.
                // K4-2d/3: the spherical polygon — a sphere face whose
                // single wire is a loop of open, single-use arcs (the
                // box×sphere bite patches: the octant behind a corner
                // bite, the quarter behind an edge bite, their
                // complements). The band machinery reads latitude
                // rims, pole lines, and seam chains; a polygon domain
                // bounded by tilted great-circle arcs is none of
                // those, and tiling it through the band walk emits a
                // torn mesh — a plausible-looking wrong answer. Refuse
                // by name: the exact channels still measure it, and
                // the trim-domain classification slice (K4-2d) is the
                // honest widening path.
                // The cap and its complement share one loop: a
                // sphere face whose single wire is ONE closed circle
                // (a latitude) bounds both the cap (loop → pole) and
                // the complement (loop → the far pole) — the same
                // interior/exterior ambiguity, and the wire-orientation
                // classification that would settle it is the same
                // K4-2d slice. The band machinery cannot tile it
                // either: the boundary walk's two u-monotone chains
                // COINCIDE (the full latitude), the band has no
                // height, and nothing would be emitted — a silently
                // open shell, worse than a refusal. Found by the
                // K4-2d/3 offset stations (their caps and the
                // slice-1 face station's hemisphere patch all carry
                // exactly this trim).
                if let Surface::Sphere { .. } = face.surface {
                    let cap_loop = face.wires.len() == 1 && {
                        let uses: Vec<usize> = face.wires[0]
                            .edges
                            .iter()
                            .filter(|we| !brep.is_degenerate(we.edge))
                            .map(|we| we.edge)
                            .collect();
                        uses.len() == 1 && {
                            let e = &brep.edges[uses[0]];
                            e.start == e.end
                        }
                    };
                    if cap_loop {
                        return Err(format!(
                            "face {fi}: a sphere face trimmed by a single closed circle — the \
                             cap and its complement share that loop (the wire-orientation \
                             classification is the K4-2d slice); the band tessellator's two \
                             chains would coincide and emit nothing, so this is a refusal, not \
                             a silently open shell; the exact channels still measure it"
                        ));
                    }
                }
                if let Surface::Sphere { .. } = face.surface {
                    let polygon = face.wires.len() == 1 && {
                        let uses: Vec<usize> = face.wires[0]
                            .edges
                            .iter()
                            .filter(|we| !brep.is_degenerate(we.edge))
                            .map(|we| we.edge)
                            .collect();
                        uses.len() >= 2
                            && uses.iter().all(|e| {
                                brep.edges[*e].start != brep.edges[*e].end
                                    && uses.iter().filter(|e2| *e2 == e).count() == 1
                            })
                    };
                    if polygon {
                        return Err(format!(
                            "face {fi}: a sphere face trimmed by a loop of open arcs — the \
                             spherical polygon (the box×sphere bite patches of K4-2d/3); the \
                             band tessellator reads latitude rims, pole lines, and seams, and \
                             would tear this domain — trim-domain classification is the \
                             K4-2d slice; the exact channels still measure it"
                        ));
                    }
                }
                let loop_umin = outer.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
                let loop_umax = outer.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
                let loop_span = loop_umax - loop_umin;
                let touch_tol = (umax - umin).max(1.0) * 1e-6;
                let loop_bounds_box = match uper {
                    Some(p) => {
                        loop_span >= p - p / 4.0
                            || (loop_umax >= umax - touch_tol && loop_umin <= umin + touch_tol)
                    }
                    None => loop_umax >= umax - touch_tol && loop_umin <= umin + touch_tol,
                };
                if !loop_bounds_box {
                    return Err(format!(
                        "face {fi}: a {} face whose trim loop does not bound its UV box (an \
                         interior/exterior-ambiguous trim — the exact booleans' cap and \
                         complement faces) — trim-domain classification is the K4-2d slice; \
                         the exact channels still measure it",
                        face.surface.kind_name()
                    ));
                }
                let rows = grid_rows(&face.surface, &outer, deflection);
                let mut dm = build_band_domain(loop_verts, &face.surface, rows)
                    .map_err(|e| format!("face {fi}: {e}"))?;
                refine_domain(&mut dm, &face.surface, deflection);
                (dm.tris, dm.p3)
            } else {
                // The planar path — exact in the face's own plane: ear clip
                // with the boundary kept, degenerate ears dropped (zero
                // area covers nothing), and any dropped boundary points
                // stitched back into the spanning edges.
                let mut tris = triangulate_planar_region(&outer, &[])
                    .map_err(|e| format!("face {fi}: the UV region did not triangulate: {e:?}"))?;
                tris.retain(|t| {
                    let (a, b, c) = (
                        outer[t[0] as usize],
                        outer[t[1] as usize],
                        outer[t[2] as usize],
                    );
                    (b - a).cross2(c - a).abs() > DOMAIN_AREA_EPS
                });
                let mut used = vec![false; n];
                for t in &tris {
                    for ti in t {
                        used[*ti as usize] = true;
                    }
                }
                if !used.iter().all(|u| *u) {
                    // A cyclic walk starting at a used anchor; each maximal
                    // dropped run between anchors gets its spanning edge
                    // split so the dropped points re-enter the triangulation.
                    let mut start = 0usize;
                    while start < n && !used[start] {
                        start += 1;
                    }
                    if start == n {
                        return Err(format!(
                            "face {fi}: every boundary vertex was dropped by the triangulation"
                        ));
                    }
                    let order: Vec<usize> = (0..n).map(|k| (start + k) % n).collect();
                    let mut k = 0usize;
                    while k < n {
                        let anchor = order[k];
                        let mut j = k + 1;
                        let mut dropped: Vec<usize> = Vec::new();
                        while j < n && !used[order[j]] {
                            dropped.push(order[j]);
                            j += 1;
                        }
                        let next_anchor = if j < n { order[j] } else { order[0] };
                        if !dropped.is_empty() {
                            split_edge_chain(&mut tris, anchor, next_anchor, &dropped);
                        }
                        if j >= n {
                            break;
                        }
                        k = j;
                    }
                }
                (tris, loop_verts.iter().map(|b| b.p3).collect())
            };
        // One triangle per 3D panel. A domain triangle whose corners
        // collapse onto fewer than three distinct points — the pole
        // runs, the bit-exact seam corners — covers zero area and is
        // dropped exactly. A triangle whose corner SET repeats — the
        // two sides of a seam-spanning split — is one panel twice: the
        // first winding survives (the pair would cancel into a
        // zero-volume fold; the survivor pairs correctly with both
        // neighbors).
        let mut panels: HashSet<[u32; 3]> = HashSet::new();
        for tri in face_tris {
            let tri = if face.reversed {
                [tri[0], tri[2], tri[1]]
            } else {
                tri
            };
            let mut idx = [0u32; 3];
            for (k, ti) in tri.iter().enumerate() {
                let p3 = p3[*ti as usize];
                let key = [p3.x.to_bits(), p3.y.to_bits(), p3.z.to_bits()];
                if let Some(&i) = index_of.get(&key) {
                    idx[k] = i;
                } else {
                    vertices.push(p3);
                    let i = (vertices.len() - 1) as u32;
                    index_of.insert(key, i);
                    idx[k] = i;
                }
            }
            if idx[0] == idx[1] || idx[1] == idx[2] || idx[0] == idx[2] {
                continue;
            }
            let mut set = idx;
            set.sort_unstable();
            if panels.insert(set) {
                triangles.push(idx);
            }
        }
    }

    let mut mesh = Mesh::new(vertices, triangles);
    mesh.validate()
        .map_err(|e| format!("the tessellated shell is invalid: {e}"))?;
    orient_closed_shell(&mut mesh);
    let volume = mesh.volume();
    if volume <= 0.0 {
        return Err(format!(
            "the tessellated shell has non-positive volume ({volume:.6}) — it could not be \
             oriented as a solid boundary"
        ));
    }
    Ok(mesh)
}

/// Insert the dropped boundary points `dropped` (in walk order from
/// `a` to `b`) into the triangle edge that spans them: for each point
/// `p`, the triangle carrying the edge `(current, b)` is split at
/// `p` — the two halves preserve the original winding, and in the
/// parameter domain `p` lies genuinely on the segment (that is why
/// the ear-clip dropped it).
fn split_edge_chain(tris: &mut Vec<[u32; 3]>, a: usize, b: usize, dropped: &[usize]) {
    let mut cur = a;
    for &p in dropped {
        let pos = tris.iter().position(|t| {
            let v = [t[0] as usize, t[1] as usize, t[2] as usize];
            (0..3).any(|i| {
                let (x, y) = (v[i], v[(i + 1) % 3]);
                (x == cur && y == b) || (x == b && y == cur)
            })
        });
        let Some(pos) = pos else {
            return;
        };
        let t = tris[pos];
        let v = [t[0] as usize, t[1] as usize, t[2] as usize];
        for i in 0..3 {
            let (x, y) = (v[i], v[(i + 1) % 3]);
            if (x == cur && y == b) || (x == b && y == cur) {
                // Replace the edge (x→y) by (x→p→y): the triangle
                // (x, y, z) becomes (x, p, z) + (p, y, z), same
                // orientation.
                let (tx, ty, tz) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
                tris[pos] = [tx, p as u32, tz];
                tris.push([p as u32, ty, tz]);
                break;
            }
        }
        cur = p;
    }
}

// ---------------------------------------------------------------------------
// K4-2b: curved faces — curvature-respecting interior domain samples
// ---------------------------------------------------------------------------

/// "Strictly inside a domain triangle" tolerance (area units — the
/// same order as the profile kernel's own area epsilon).
const DOMAIN_AREA_EPS: f64 = 1e-12;
/// The deflection referee's budgets: rounds of repair, and vertices
/// per face. Both sit far above what the analytic fixtures reach; a
/// mesh that exhausts them stays valid, merely no longer refined.
const MAX_REFINE_ROUNDS: usize = 32;
const MAX_DOMAIN_VERTS: usize = 16_384;

/// The chord step of a circular arc under `deflection`: the span whose
/// sagitta just reaches the deflection, `2·acos(1 − d/r)`, clamped to
/// `[τ/512, π/2]` — a floor against pathological radii, a cap that
/// keeps at least four columns around any full turn (when `r ≤ d`
/// every chord is already deflection-true; the referee below is the
/// arbiter regardless).
fn arc_step(r: f64, deflection: f64) -> f64 {
    if !r.is_finite() || r <= 0.0 {
        return std::f64::consts::FRAC_PI_2;
    }
    let c = (1.0 - deflection / r).clamp(-1.0, 1.0);
    (2.0 * c.acos()).clamp(std::f64::consts::TAU / 512.0, std::f64::consts::FRAC_PI_2)
}

/// Cell-center offsets spanning `range` at target step `step`: the
/// span is cut into `ceil(range/step)` cells (at least one, at most
/// 4096) with one sample at each center — never on the span's own
/// ends.
fn cell_centers(range: f64, step: f64) -> Vec<f64> {
    let n = ((range / step).ceil() as usize).clamp(1, 4096);
    let cell = range / n as f64;
    (0..n).map(|i| (i as f64 + 0.5) * cell).collect()
}

/// The bounds of a closed UV walk.
fn loop_bounds(loop_pts: &[Point2]) -> Option<(Point2, Point2)> {
    let mut it = loop_pts.iter().copied();
    let first = it.next()?;
    let (mut lo, mut hi) = (first, first);
    for p in it {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    Some((lo, hi))
}

/// Distance from `p` to the closed polyline through `loop_pts`.
fn dist_to_loop(p: Point2, loop_pts: &[Point2]) -> f64 {
    let n = loop_pts.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let a = loop_pts[i];
        let b = loop_pts[(i + 1) % n];
        let d = b - a;
        let l2 = d.x * d.x + d.y * d.y;
        let t = if l2 > 0.0 {
            (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = a + d * t;
        best = best.min((p.x - q.x).hypot(p.y - q.y));
    }
    best
}

/// Split the closed UV walk into its two u-monotone chains: the two
/// stretches between the walk's extreme-u vertices. Both chains run
/// left (u-minimum) to right (u-maximum); the one with the greater
/// mean v is the upper. A walk whose chains double back in u is not a
/// band, and the band merge below refuses it by name.
fn monotone_chains(uv: &[Point2]) -> Result<(Vec<usize>, Vec<usize>), String> {
    let n = uv.len();
    let umin = uv.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let umax = uv.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let slack = (umax - umin).max(1.0) * 1e-9;
    let lo = uv
        .iter()
        .position(|p| p.x <= umin + slack)
        .ok_or_else(|| "the UV walk has no u-minimum".to_string())?;
    let hi = uv
        .iter()
        .position(|p| p.x >= umax - slack)
        .ok_or_else(|| "the UV walk has no u-maximum".to_string())?;
    let walk = |from: usize, to: usize| -> Vec<usize> {
        if from <= to {
            (from..=to).collect()
        } else {
            (from..n).chain(0..=to).collect()
        }
    };
    let a = walk(lo, hi);
    let mut b = walk(hi, lo);
    b.reverse(); // b walked right-to-left; chains run left-to-right
    for (name, chain) in [("one", &a), ("two", &b)] {
        for w in chain.windows(2) {
            if uv[w[1]].x < uv[w[0]].x - slack {
                return Err(format!(
                    "the UV walk's {name} chain doubles back in u ({:.6} after {:.6}) — \
                     the curved-face domain triangulation supports band-like trims only (yet)",
                    uv[w[1]].x, uv[w[0]].x
                ));
            }
        }
    }
    let mean_v = |chain: &[usize]| chain.iter().map(|&i| uv[i].y).sum::<f64>() / chain.len() as f64;
    if mean_v(&a) >= mean_v(&b) {
        Ok((a, b))
    } else {
        Ok((b, a))
    }
}

/// Interior domain rows for one curved face — the curvature-respecting
/// grid, one row per parameter line. The periodic direction steps by
/// [`arc_step`] on the relevant curvature radius; the second direction
/// steps by its own [`arc_step`] where it is curved (sphere colatitude,
/// torus minor ring) and by the arc length of the periodic step where
/// it is straight (the cylinder axis, the cone generatrix). Samples
/// land at cell centers — half a step clear of every domain boundary —
/// only those strictly inside the face's UV loop survive, and a run
/// that the region punches shorter than two points is dropped (the
/// referee re-densifies locally). Rows run bottom-up in v.
fn grid_rows(s: &Surface, loop_pts: &[Point2], deflection: f64) -> Vec<Vec<Point2>> {
    let Some((lo, hi)) = loop_bounds(loop_pts) else {
        return Vec::new();
    };
    let (urange, vrange) = (hi.x - lo.x, hi.y - lo.y);
    if !(urange.is_finite() && urange > 0.0 && vrange.is_finite() && vrange > 0.0) {
        return Vec::new();
    }
    // (v offset, u offsets) of each row's candidate samples.
    let mut rows: Vec<(f64, Vec<f64>)> = Vec::new();
    match s {
        Surface::Cylinder { radius, .. } => {
            let du = arc_step(*radius, deflection);
            let dv = (du * radius).min(vrange);
            for v in cell_centers(vrange, dv) {
                rows.push((v, cell_centers(urange, du)));
            }
        }
        Surface::Cone {
            semi_angle,
            ref_radius,
            ..
        } => {
            // The generatrix is straight in v; the parallel radius
            // follows the slant parameterization, so the step uses the
            // widest end of the face's own v range.
            let r_lo = ref_radius + lo.y * semi_angle.sin();
            let r_hi = ref_radius + hi.y * semi_angle.sin();
            let r_max = r_lo.max(r_hi).max(0.0);
            let du = arc_step(r_max, deflection);
            let dv = (du * r_max).min(vrange);
            for v in cell_centers(vrange, dv) {
                rows.push((v, cell_centers(urange, du)));
            }
        }
        Surface::Sphere { radius, .. } => {
            // The colatitude step is global; each row's parallel
            // radius r·cos v shrinks toward the poles, so every row
            // steps by its own — coarser — chord.
            let dv = arc_step(*radius, deflection);
            for v in cell_centers(vrange, dv) {
                let r_parallel = radius * (lo.y + v).cos();
                let du = arc_step(r_parallel, deflection);
                rows.push((v, cell_centers(urange, du)));
            }
        }
        Surface::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            // The tightest parallel ring is the inner equator R − r;
            // the minor ring carries radius r.
            let du = arc_step(major_radius - minor_radius, deflection);
            let dv = arc_step(*minor_radius, deflection);
            for v in cell_centers(vrange, dv) {
                rows.push((v, cell_centers(urange, du)));
            }
        }
        // The caller refuses B-spline faces by name, and planar faces
        // take the exact planar path — neither reaches this grid.
        Surface::Plane { .. } | Surface::Bspline { .. } => {}
    }
    let diag = urange.hypot(vrange).max(1e-12);
    let margin = diag * 1e-7;
    let mut out = Vec::new();
    for (v, us) in rows {
        let cell = if us.len() >= 2 {
            (us[1] - us[0]).abs()
        } else {
            urange
        };
        let kept: Vec<f64> = us
            .into_iter()
            .filter(|u| {
                let p = Point2::new(lo.x + u, lo.y + v);
                point_in_polygon(p, loop_pts) && dist_to_loop(p, loop_pts) > margin
            })
            .collect();
        if kept.len() < 2 {
            continue;
        }
        // The region can punch a row into several runs (a wavy band);
        // a gap wider than a few cells starts a new polyline.
        let mut run: Vec<Point2> = Vec::with_capacity(kept.len());
        for u in kept {
            if let Some(last) = run.last() {
                if u - (last.x - lo.x) > 3.0 * cell.max(1e-15) {
                    if run.len() >= 2 {
                        out.push(std::mem::take(&mut run));
                    } else {
                        run.clear();
                    }
                }
            }
            run.push(Point2::new(lo.x + u, lo.y + v));
        }
        if run.len() >= 2 {
            out.push(run);
        }
    }
    out
}

/// Emit one band triangle, wound CCW; degenerate (zero-area) steps —
/// shared endpoints, duplicate closure points — are skipped exactly:
/// they cover no parameter region.
fn emit_band_tri(dm: &mut DomainMesh, t: [u32; 3]) {
    let (a, b, c) = (
        dm.pts[t[0] as usize],
        dm.pts[t[1] as usize],
        dm.pts[t[2] as usize],
    );
    let area = (b - a).cross2(c - a);
    if area.abs() <= DOMAIN_AREA_EPS {
        return;
    }
    if area > 0.0 {
        dm.tris.push(t);
    } else {
        dm.tris.push([t[0], t[2], t[1]]);
    }
}

/// Tile the band between the lower polyline `p` and the upper polyline
/// `q` — both u-ascending — with the classic two-pointer monotone
/// merge: advance whichever chain's next vertex lies further left, and
/// emit the triangle that consumes the gap. Shared endpoints make the
/// first and last steps degenerate; they are skipped, and the band
/// still closes.
fn band_merge(dm: &mut DomainMesh, p: &[u32], q: &[u32]) {
    if p.len() < 2 || q.len() < 2 {
        return;
    }
    let (mut i, mut j) = (0usize, 0usize);
    let (mut pl, mut ql) = (p[0], q[0]);
    while i + 1 < p.len() || j + 1 < q.len() {
        let pu = (i + 1 < p.len()).then(|| dm.pts[p[i + 1] as usize].x);
        let qu = (j + 1 < q.len()).then(|| dm.pts[q[j + 1] as usize].x);
        let take_p = match (pu, qu) {
            (Some(a), Some(b)) => a <= b,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if take_p {
            let x = p[i + 1];
            emit_band_tri(dm, [pl, ql, x]);
            pl = x;
            i += 1;
        } else {
            let x = q[j + 1];
            emit_band_tri(dm, [ql, pl, x]);
            ql = x;
            j += 1;
        }
    }
}

/// Merge one grid `run` into the growing stack: triangulate between
/// the current polyline's overlapping stretch and the run, then splice
/// the run into the current polyline (which keeps its own points
/// outside the run's u-range). The stack invariant: the emitted
/// triangles so far tile exactly the region below the current
/// polyline.
fn splice_merge(
    dm: &mut DomainMesh,
    current: &mut Vec<u32>,
    run: &[u32],
    prev_run_last: Option<u32>,
) {
    let umin = dm.pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let umax = dm.pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let slack = (umax - umin).max(1.0) * 1e-9;
    let u = |i: u32| dm.pts[i as usize].x;
    let v = |i: u32| dm.pts[i as usize].y;
    let (rlo, rhi, run_v) = (u(run[0]), u(run[run.len() - 1]), v(run[0]));
    if rhi <= rlo {
        return;
    }
    let mut k0 = 0usize;
    while k0 + 1 < current.len() && u(current[k0 + 1]) <= rlo + slack {
        k0 += 1;
    }
    // A current vertex is UNDER the run when it lies within the run's
    // u-span at or below the run's level — the already-tiled frontier
    // the run replaces. The first vertex NOT under it (beyond the
    // u-span, or above the level) starts the tail: the frontier the
    // next merges and the final merge build on. A wavy band's two runs
    // per row exercise both: the left run's tail is the rim continuing
    // rightward; the right run's tail is the rim above its level.
    let under = |i: usize| u(current[i]) <= rhi + slack && v(current[i]) <= run_v;
    let mut k1 = current.len();
    for i in (k0 + 1)..current.len() {
        if !under(i) {
            k1 = i;
            break;
        }
    }
    k1 = k1.max(k0 + 1).min(current.len());
    // The merge input ends at the previous run's own last vertex: past
    // it, the under-stretch is stale — seam-edge samples the earlier
    // splices left below — which the splice consumes without merging.
    // Stale samples hug the run's u-span boundaries (the seam columns
    // sit at the u-extremes); a stretch carrying points STRICTLY INSIDE
    // the run's span is genuine untiled frontier — the widening case:
    // a wavy LOWER chain punching ever-wider runs (the fillet corner's
    // ellipse rim) leaves chain vertices below the run's level inside
    // its span, and cutting the merge before them leaves the wedge
    // between them and the run uncovered — a torn mesh. The merge must
    // reach them.
    let p_end = match prev_run_last {
        Some(last) => current
            .iter()
            .position(|&c| c == last)
            .filter(|&pos| pos >= k0 && pos < k1)
            .map(|pos| {
                let cut = pos + 1;
                let interior_frontier = current[cut..k1]
                    .iter()
                    .any(|&c| u(c) > rlo + slack && u(c) < rhi - slack);
                if interior_frontier {
                    k1
                } else {
                    cut
                }
            })
            .unwrap_or(k1),
        None => k1,
    };
    band_merge(dm, &current[k0..p_end.max(k0 + 1)], run);
    // The pinch-out closure (K4-2c): a run whose window pinches shut
    // before the frontier does — a wavy band's rim dipping through the
    // row level — ends its band at an edge (under-stretch's last, run's
    // last) that still faces untiled region on its upper side: the new
    // frontier chain edge (run's last → the first not-under vertex)
    // gets its carrier from above by the next splice or the final
    // merge, but nothing tiles the wedge between the two. One closure
    // triangle does: {under-stretch's last, run's last, first
    // not-under} — its bottom edge pairs with the previous band's own
    // upper boundary, its right edge with the final merge's carrier of
    // the new chain edge. In the full-width case the three vertices
    // are the collinear seam corner (all at the u-maximum) and the
    // emit skips the zero-area triangle exactly — the analytic five's
    // green paths are untouched. Only a merge that ran its p-range to
    // the under-stretch's end (p_end == k1) may close: an earlier cut
    // (the previous run's last, the widening case) leaves the stretch
    // between, and a triangle spanning it would create unpaired edges.
    if p_end == k1 && k1 >= k0 + 2 && k1 < current.len() {
        emit_band_tri(dm, [current[k1 - 1], run[run.len() - 1], current[k1]]);
    }
    let mut next = Vec::with_capacity(current.len() + run.len());
    next.extend_from_slice(&current[..=k0]);
    next.extend_from_slice(run);
    if k1 < current.len() {
        next.extend_from_slice(&current[k1..]);
    }
    *current = next;
}

/// The face's parameter-domain triangulation grown past its boundary:
/// the vertex table carries every point's 3D image — boundary verts
/// keep their shared rim samples bit-exactly, interior verts evaluate
/// the surface, and the two sides of a periodic seam SHARE one
/// evaluated point so the seam closes bit-exactly.
struct DomainMesh {
    pts: Vec<Point2>,
    p3: Vec<Vector3>,
    tris: Vec<[u32; 3]>,
}

impl DomainMesh {
    /// Insert one interior vertex by splitting its containing triangle
    /// into three — the split only adds edges, so every boundary
    /// constraint edge survives untouched. A point not strictly inside
    /// any triangle (on an edge, on a vertex, outside the region) is
    /// skipped: the triangulation stays valid, merely locally coarser.
    fn insert_point(&mut self, s: &Surface, p: Point2) -> bool {
        for (ti, t) in self.tris.iter().enumerate() {
            let (a, b, c) = (
                self.pts[t[0] as usize],
                self.pts[t[1] as usize],
                self.pts[t[2] as usize],
            );
            if (b - a).cross2(p - a) > DOMAIN_AREA_EPS
                && (c - b).cross2(p - b) > DOMAIN_AREA_EPS
                && (a - c).cross2(p - c) > DOMAIN_AREA_EPS
            {
                let [ia, ib, ic] = *t;
                let ip = self.pts.len() as u32;
                self.pts.push(p);
                self.p3.push(s.point(p.x, p.y));
                self.tris[ti] = [ia, ib, ip];
                self.tris.push([ib, ic, ip]);
                self.tris.push([ic, ia, ip]);
                return true;
            }
        }
        false
    }

    /// Split the interior edge `(a, b)` at `p`: both carrying triangles
    /// split into two, winding preserved. Edges of any other
    /// multiplicity — domain-boundary rims (one carrier) — are left
    /// alone: their chords are the rim sampler's own deflection-true
    /// business.
    fn split_edge(&mut self, s: &Surface, a: u32, b: u32, p: Point2) -> bool {
        let mut carriers: Vec<usize> = Vec::new();
        for (ti, t) in self.tris.iter().enumerate() {
            if (0..3).any(|i| {
                let (x, y) = (t[i], t[(i + 1) % 3]);
                (x == a && y == b) || (x == b && y == a)
            }) {
                carriers.push(ti);
            }
        }
        if carriers.len() != 2 {
            return false;
        }
        let ip = self.pts.len() as u32;
        self.pts.push(p);
        self.p3.push(s.point(p.x, p.y));
        for ti in carriers {
            let t = self.tris[ti];
            for i in 0..3 {
                let (x, y, z) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
                if (x == a && y == b) || (x == b && y == a) {
                    self.tris[ti] = [x, ip, z];
                    self.tris.push([ip, y, z]);
                    break;
                }
            }
        }
        true
    }
}

/// Build the curved face's domain mesh: the two u-monotone chains, the
/// periodic seam densified at the row levels (shared-point pairs, and
/// the rows carry the seam points as endpoints so the seam strips tile
/// locally instead of fanning from the corners), the grid rows spliced
/// bottom-up, and the final merge against the upper chain.
fn build_band_domain(
    loop_verts: &[BoundaryVert],
    surface: &Surface,
    rows: Vec<Vec<Point2>>,
) -> Result<DomainMesh, String> {
    let outer: Vec<Point2> = loop_verts.iter().map(|b| b.uv).collect();
    let (upper, lower) = monotone_chains(&outer)?;
    let mut dm = DomainMesh {
        pts: outer,
        p3: loop_verts.iter().map(|b| b.p3).collect(),
        tris: Vec::new(),
    };
    let umin = dm.pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let umax = dm.pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let slack = (umax - umin).max(1.0) * 1e-9;

    // The periodic seam: a vertical run at the u-minimum side (a chain
    // prefix) and one at the u-maximum side (a chain suffix) whose
    // endpoints are the same 3D points — the same edge this face walks
    // twice. Which chain carries the meridian depends on where the wire
    // walk begins, so all prefix/suffix pairings are tried in order. (A
    // vertical rim line the NEIGHBORING face owns fails the endpoint
    // test and gets no seam points: its correspondence is the
    // neighbor's business.)
    let pref_up: Vec<usize> = upper
        .iter()
        .take_while(|&&i| dm.pts[i].x <= umin + slack)
        .copied()
        .collect();
    let mut suff_up: Vec<usize> = upper
        .iter()
        .rev()
        .take_while(|&&i| dm.pts[i].x >= umax - slack)
        .copied()
        .collect();
    suff_up.reverse();
    let pref_lo: Vec<usize> = lower
        .iter()
        .take_while(|&&i| dm.pts[i].x <= umin + slack)
        .copied()
        .collect();
    let mut suff_lo: Vec<usize> = lower
        .iter()
        .rev()
        .take_while(|&&i| dm.pts[i].x >= umax - slack)
        .copied()
        .collect();
    suff_lo.reverse();

    let ends = |run: &[usize]| (dm.p3[run[0]], dm.p3[run[run.len() - 1]]);
    let same_seam = |a: &[usize], b: &[usize]| {
        if a.len() < 2 || b.len() < 2 {
            return false;
        }
        // Straight seam lines (a cylinder's, a band's) carry their end
        // vertices on both walks: the endpoint sets match outright.
        let (aa, ab) = ends(a);
        let (ba, bb) = ends(b);
        if (aa == ba && ab == bb) || (aa == bb && ab == ba) {
            return true;
        }
        // Curved seams (a sphere's meridian, a torus's seam circle) drop
        // their closing vertex on one walk, so the endpoints differ by
        // one sample — but the two walks are the SAME edge table in
        // opposite directions: the interior samples match bit-exactly,
        // reversed. A real curved seam carries interior samples to
        // verify (n ≥ 3); two 2-point runs with mismatched endpoints
        // are NOT a seam — the fillet corner's two distinct generator
        // lines pair vacuously without this guard, and the "seam
        // points" it would mint land interior to shared straight edges
        // the neighboring plane face carries whole (a torn mesh).
        let n = a.len().min(b.len());
        n >= 3 && (1..n - 1).all(|k| dm.p3[a[k]] == dm.p3[b[b.len() - 1 - k]])
    };
    let seam_v_range = [&pref_up, &pref_lo, &suff_up, &suff_lo]
        .iter()
        .find_map(|run| {
            // a run pairs with SOME opposite-side run?
            let paired = match run {
                r if std::ptr::eq(*r, &pref_up) => {
                    same_seam(&pref_up, &suff_up) || same_seam(&pref_up, &suff_lo)
                }
                r if std::ptr::eq(*r, &pref_lo) => {
                    same_seam(&pref_lo, &suff_up) || same_seam(&pref_lo, &suff_lo)
                }
                r if std::ptr::eq(*r, &suff_up) => {
                    same_seam(&pref_up, &suff_up) || same_seam(&pref_lo, &suff_up)
                }
                _ => same_seam(&pref_lo, &suff_lo) || same_seam(&pref_up, &suff_lo),
            };
            if !paired {
                return None;
            }
            let vs: Vec<f64> = run.iter().map(|&i| dm.pts[i].y).collect();
            Some((
                vs.iter().copied().fold(f64::INFINITY, f64::min),
                vs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ))
        });

    // The rows become index runs. A row that reaches the seam carries
    // shared-point seam endpoints (one Vector3 evaluated once, two
    // domain vertices) — the seam strips then tile locally between
    // consecutive rows instead of fanning from the corners, and the
    // seam closes bit-exactly through the shared points.
    let (u_left, u_right) = (umin, umax);
    let mut runs: Vec<Vec<u32>> = Vec::with_capacity(rows.len());
    let mut lone_left: Vec<(f64, u32)> = Vec::new();
    let mut lone_right: Vec<(f64, u32)> = Vec::new();
    // The per-level seam registry (K4-2c): a wavy row can punch into
    // several runs, and every run of the SAME level must share one
    // pair of seam points. Per-run points would duplicate an attached
    // point with an unattached twin — one 3D point, two domain
    // vertices — and the twin's insertion into the frontier would
    // make the final merge walk it, emitting carriers whose edges
    // skip over the true column points (the fine-deflection defect).
    // Loneship is decided only after every run had its chance to
    // attach: a side's point is lone when NO run of its level reached
    // that side.
    let mut seam_of: HashMap<u64, (u32, u32)> = HashMap::new();
    let mut left_attached: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut right_attached: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for row in rows {
        if row.len() < 2 || dm.pts.len() >= MAX_DOMAIN_VERTS {
            continue;
        }
        let v = row[0].y;
        let cell = if row.len() >= 2 {
            (row[1].x - row[0].x).abs()
        } else {
            f64::INFINITY
        };
        let in_range = match seam_v_range {
            Some((vlo, vhi)) => v > vlo && v < vhi,
            None => false,
        };
        // A run floating in the middle of a wavy band must not drag a
        // seam endpoint across outside territory.
        let reaches_left = in_range && row[0].x <= u_left + 2.5 * cell + slack;
        let reaches_right = in_range && row[row.len() - 1].x >= u_right - 2.5 * cell - slack;
        let mut run: Vec<u32> = Vec::with_capacity(row.len() + 2);
        // Both seam points exist for every in-range level — the two
        // columns must carry the same levels or the seam cannot close.
        // A point whose side's runs were all punched away (a wavy
        // band's one-cell window) stays unattached and joins the
        // frontier directly after the splices.
        let (mut m_left, mut m_right) = (None, None);
        if in_range {
            let key = v.to_bits();
            let (l, r) = *seam_of.entry(key).or_insert_with(|| {
                let seam_p3 = surface.point(u_left, v);
                dm.pts.push(Point2::new(u_left, v));
                dm.p3.push(seam_p3);
                let l = dm.pts.len() as u32 - 1;
                dm.pts.push(Point2::new(u_right, v));
                dm.p3.push(seam_p3);
                let r = dm.pts.len() as u32 - 1;
                (l, r)
            });
            m_left = Some(l);
            m_right = Some(r);
            if reaches_left {
                left_attached.insert(key);
            }
            if reaches_right {
                right_attached.insert(key);
            }
        }
        if reaches_left {
            run.push(m_left.unwrap());
        }
        for p in row {
            let i = dm.pts.len() as u32;
            dm.pts.push(p);
            dm.p3.push(surface.point(p.x, p.y));
            run.push(i);
        }
        if reaches_right {
            run.push(m_right.unwrap());
        }
        runs.push(run);
    }
    for (&key, &(l, r)) in &seam_of {
        if !left_attached.contains(&key) {
            lone_left.push((f64::from_bits(key), l));
        }
        if !right_attached.contains(&key) {
            lone_right.push((f64::from_bits(key), r));
        }
    }
    // v-ascending (the rows were; a hash map's iteration is not).
    lone_left.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    lone_right.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    // The stack: rows spliced into the lower chain, then the final
    // merge against the upper chain. The corner regions fall out of
    // the merges (the bottom corner from the first row's splice, the
    // top corner from the final merge); degenerate steps — pole runs,
    // duplicate closure points — are skipped exactly.
    let upper_u: Vec<u32> = upper.iter().map(|&i| i as u32).collect();
    let mut current: Vec<u32> = lower.iter().map(|&i| i as u32).collect();
    let mut prev_run_last: Option<u32> = None;
    for run in &runs {
        splice_merge(&mut dm, &mut current, run, prev_run_last);
        prev_run_last = Some(run[run.len() - 1]);
    }
    // The unattached seam points join the frontier directly: the left
    // ones into the u-minimum prefix (v-ordered among the seam points),
    // the right ones before the u-maximum tail. Each lone point also
    // gets its seam-column carrier (K4-2c): the strip between the seam
    // column segment (previous seam level → the lone level) and the
    // frontier chain below it would otherwise stay untiled — no run
    // reached this level, so no splice's band paired the column here,
    // and the final merge only tiles ABOVE the frontier. The carrier
    // is the triangle {frontier vertex before, lone point, frontier
    // vertex after}: its seam edge pairs with the opposite column's
    // band carrier of the same segment (the shared-point seam closes
    // bit-exactly through it), its chain edges pair with the previous
    // band below and the final merge above. When the frontier's
    // next vertex is itself a seam point (a higher row did reach the
    // seam — the strip already tiled by that row's splice), the three
    // vertices are collinear on the seam and the emit skips the
    // zero-area triangle exactly.
    if !lone_left.is_empty() {
        let prefix_end = current
            .iter()
            .position(|&c| dm.pts[c as usize].x > umin + slack)
            .unwrap_or(current.len());
        for (inserted, (v, m)) in lone_left.into_iter().enumerate() {
            let pos = current[..prefix_end + inserted]
                .iter()
                .rposition(|&c| dm.pts[c as usize].y <= v)
                .map(|p| p + 1)
                .unwrap_or(0);
            current.insert(pos, m);
            if pos > 0 && pos + 1 < current.len() {
                emit_band_tri(&mut dm, [current[pos - 1], m, current[pos + 1]]);
            }
        }
    }
    if !lone_right.is_empty() {
        let suffix_start = current
            .iter()
            .rposition(|&c| dm.pts[c as usize].x < umax - slack)
            .map(|p| p + 1)
            .unwrap_or(current.len());
        let mut at = suffix_start;
        for (v, m) in lone_right {
            let pos = current[at..]
                .iter()
                .position(|&c| dm.pts[c as usize].y > v)
                .map(|p| at + p)
                .unwrap_or(current.len());
            current.insert(pos, m);
            if pos > 0 && pos + 1 < current.len() {
                emit_band_tri(&mut dm, [current[pos - 1], m, current[pos + 1]]);
            }
            at = pos + 1;
        }
    }
    band_merge(&mut dm, &current, &upper_u);
    Ok(dm)
}

/// Chordal deviation of one domain edge: how far the surface at the
/// edge's parameter midpoint stands from the 3D chord between the
/// endpoints' own surface points.
fn edge_deviation(s: &Surface, a: Point2, b: Point2) -> f64 {
    let mid = Point2::new(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
    let pa = s.point(a.x, a.y);
    let pb = s.point(b.x, b.y);
    let pm = s.point(mid.x, mid.y);
    let d = pb - pa;
    let l2 = d.length_squared();
    if l2 <= 1e-30 {
        return (pm - pa).length();
    }
    let t = ((pm - pa).dot(&d) / l2).clamp(0.0, 1.0);
    (pm - (pa + d * t)).length()
}

/// Panel deviation of one domain triangle: how far the surface at the
/// triangle's parameter centroid stands from the triangle's 3D plane.
fn panel_deviation(s: &Surface, t: [Point2; 3]) -> f64 {
    let centroid = Point2::new(
        (t[0].x + t[1].x + t[2].x) / 3.0,
        (t[0].y + t[1].y + t[2].y) / 3.0,
    );
    let pa = s.point(t[0].x, t[0].y);
    let pb = s.point(t[1].x, t[1].y);
    let pc = s.point(t[2].x, t[2].y);
    let n = (pb - pa).cross(&(pc - pa));
    let ln = n.length();
    if ln <= 1e-30 {
        return 0.0; // a degenerate panel — the pole runs — measures
                    // zero; its edges carry the deflection contract
    }
    ((s.point(centroid.x, centroid.y) - pa).dot(&n) / ln).abs()
}

/// The deflection referee. Each round measures every interior domain
/// edge (exactly two carriers) by its chord — the surface at the
/// edge's parameter midpoint against the 3D chord — and every panel
/// by its center — the surface at the triangle's parameter centroid
/// against the triangle's 3D plane — and repairs what misses by more
/// than `deflection`: edges split at their parameter midpoints (both
/// carriers, winding preserved), panels gain their centroid. Rounds
/// and vertices are capped ([`MAX_REFINE_ROUNDS`],
/// [`MAX_DOMAIN_VERTS`]); a mesh that exhausts the caps stays valid,
/// merely no longer refined.
fn refine_domain(dm: &mut DomainMesh, s: &Surface, deflection: f64) {
    for _ in 0..MAX_REFINE_ROUNDS {
        if dm.pts.len() >= MAX_DOMAIN_VERTS {
            break;
        }
        let mut uses: HashMap<(u32, u32), usize> = HashMap::new();
        for t in &dm.tris {
            for (u, v) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let key = if u < v { (u, v) } else { (v, u) };
                *uses.entry(key).or_default() += 1;
            }
        }
        let mut splits: Vec<(u32, u32)> = Vec::new();
        for (&key, &n) in &uses {
            if n == 2
                && edge_deviation(s, dm.pts[key.0 as usize], dm.pts[key.1 as usize]) > deflection
            {
                splits.push(key);
            }
        }
        // The census iterates a hash map; sorting makes the repair
        // order — and therefore the mesh — deterministic.
        splits.sort_unstable();
        let mut inserts: Vec<Point2> = Vec::new();
        for t in &dm.tris {
            let tri = [
                dm.pts[t[0] as usize],
                dm.pts[t[1] as usize],
                dm.pts[t[2] as usize],
            ];
            if panel_deviation(s, tri) > deflection {
                inserts.push(Point2::new(
                    (tri[0].x + tri[1].x + tri[2].x) / 3.0,
                    (tri[0].y + tri[1].y + tri[2].y) / 3.0,
                ));
            }
        }
        if splits.is_empty() && inserts.is_empty() {
            break;
        }
        let mut applied = 0usize;
        for (a, b) in splits {
            if dm.pts.len() >= MAX_DOMAIN_VERTS {
                break;
            }
            let mid = Point2::new(
                0.5 * (dm.pts[a as usize].x + dm.pts[b as usize].x),
                0.5 * (dm.pts[a as usize].y + dm.pts[b as usize].y),
            );
            if dm.split_edge(s, a, b, mid) {
                applied += 1;
            }
        }
        for p in inserts {
            if dm.pts.len() >= MAX_DOMAIN_VERTS {
                break;
            }
            if dm.insert_point(s, p) {
                applied += 1;
            }
        }
        if applied == 0 {
            break;
        }
    }
}
