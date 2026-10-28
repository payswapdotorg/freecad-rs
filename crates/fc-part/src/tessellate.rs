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
//!    forms for the analytic five; a coarse domain scan plus
//!    Gauss–Newton refinement for B-splines — the tessellation
//!    tier's own honesty: boundary points are exact, interior nodes
//!    are surface evaluations within the deflection), unwrapped
//!    continuously along the wire (the seam edge of a periodic
//!    surface walks `u = 0` up and `u = 2π` down), and the UV region —
//!    the outer wire plus the holes — is triangulated by the profile
//!    kernel's planar triangulator.
//! 3. **Degenerate edges** (sphere poles) are single points in 3D but
//!    real boundary in UV: they span the face's full `u` range at the
//!    pole's `v`, exactly the role the K1 sphere wire gives them.
//!
//! Orientation: each face's triangles wind against the surface's
//! natural normal (`∂u×∂v`), flipped when `reversed`; the assembled
//! shell is reconciled through the shared-edge graph and the global
//! divergence sign (the same discipline the STEP importer applies to
//! foreign shells), so the result is a solid boundary with outward
//! normals.
//!
//! Refused by name: untrimmed faces (no wires to bound the region)
//! and non-positive-volume results.

use std::collections::HashMap;

use fc_base::Vector3;

use crate::boolean::orient_closed_shell;
use crate::profile::{
    triangulate_planar_region_keep_boundary as triangulate_planar_region, Point2,
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
            minor_radius,
        } => {
            let y = axis.cross(ref_dir);
            let d = p - *center;
            let z = d.dot(axis);
            let du = d.dot(ref_dir);
            let dv = d.dot(&y);
            let rho = (du * du + dv * dv).sqrt();
            let u = dv.atan2(du);
            let mut v = ((rho - *major_radius) / *minor_radius)
                .clamp(-1.0, 1.0)
                .asin();
            if z < 0.0 {
                v = -v;
            }
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
    // The domain triangulation below is deflection-true only for
    // planar faces, where every UV triangle maps to a triangle IN the
    // face's own plane. On curved (periodic) surfaces the shared rim
    // samples project to parameter-collinear runs, and the 2D
    // triangulator fans from a corner across them — slanted panels
    // that cut the solid's interior, not the surface. A curvature-
    // respecting domain triangulation (constrained interior points)
    // is the next slice; until then, curved faces refuse by name.
    for (fi, f) in brep.faces.iter().enumerate() {
        if !matches!(f.surface, Surface::Plane { .. }) {
            return Err(format!(
                "face {fi}: a {} face — the curved-surface domain triangulation is not \
                 deflection-true yet (planar-faced solids only)",
                f.surface.kind_name()
            ));
        }
    }

    // 1. Edge discretization, shared by every face.
    let edge_samples: Vec<Vec<Vector3>> = brep
        .edges
        .iter()
        .map(|e| sample_edge(&e.curve, e.range, deflection))
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
                    // face's full u range at the pole's v.
                    let p3 = e.curve.point(e.range.0);
                    let v_pole = match prev {
                        Some((_, pv)) => pv,
                        None => {
                            let probe = surface_uv_of(&face.surface, p3)
                                .map(|(_, v)| v)
                                .unwrap_or(vmin);
                            if (probe - vmin).abs() < (probe - vmax).abs() {
                                vmin
                            } else {
                                vmax
                            }
                        }
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
                    match prev {
                        Some((pu, pv)) => {
                            if let Some(p) = uper {
                                u = pu + (u - pu + p / 2.0).rem_euclid(p) - p / 2.0;
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
        let mut tris = triangulate_planar_region(&outer, &[])
            .map_err(|e| format!("face {fi}: the UV region did not triangulate: {e:?}"))?;

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
        for tri in tris {
            let tri = if face.reversed {
                [tri[0], tri[2], tri[1]]
            } else {
                tri
            };
            let mut idx = [0u32; 3];
            for (k, ti) in tri.iter().enumerate() {
                let b = &loop_verts[*ti as usize];
                let key = [b.p3.x.to_bits(), b.p3.y.to_bits(), b.p3.z.to_bits()];
                if let Some(&i) = index_of.get(&key) {
                    idx[k] = i;
                } else {
                    vertices.push(b.p3);
                    let i = (vertices.len() - 1) as u32;
                    index_of.insert(key, i);
                    idx[k] = i;
                }
            }
            triangles.push(idx);
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
