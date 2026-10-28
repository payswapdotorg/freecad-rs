//! K3c-2 — exact measurement of composed B-reps: volume and area.
//!
//! The volume is the divergence theorem,
//!
//! ```text
//! V = (1/3) ∮ p · n̂ dA,
//! ```
//!
//! evaluated **per face kind in closed form** — no tessellation, no
//! quadrature, no tolerances in the classification. Each face's
//! trimmed region is read from its WIRES (the UV bounds are boxes,
//! not regions; they are never consulted). The recognizable trims
//! are exactly the ones the K1 generators and the boolean composers
//! emit:
//!
//! * **Plane** faces: polygons (line edges — the box) and disks (a
//!   single closed circle). The flux reduces to `d · A` with `d` the
//!   plane's signed offset — `p·n̂` is constant on a plane, so no
//!   centroid is ever needed. Holes (wires after the first)
//!   subtract.
//! * **Sphere** faces: the full sphere (no wires, or the K1 seam
//!   pattern — every non-degenerate edge a great-circle arc used
//!   forward and reversed) and the cap/complement (a single section
//!   circle; which side is kept follows the wire-orientation
//!   convention and [`TopoFace::reversed`]). On a sphere `p·n̂ = r +
//!   c·(p−c)/r` and the patch's vector area is `π r (r² − s²) k̂`,
//!   so
//!   `F = σ (r A + (c·k̂) π (r² − s²))` — exact for any obliqueness.
//! * **Cylinder** faces: the full-turn strip between two coaxial
//!   section circles (the K1 mantle; seam lines are topological
//!   glue and carry no region). `∫r̂ du = 0` over the turn kills the
//!   origin term: `F = σ 2π R² h`.
//! * **Cone** faces: the full-turn strip between two coaxial
//!   section circles in the slant parameter (the frustum mantle;
//!   the apex's degenerate circle is a legal rim). The tangent
//!   planes all pass through the apex, which makes the definite
//!   integral collapse to
//!   `F = σ 2π (c·(r1²Δ1 + r1 s Δ2 + s²Δ3/3) − s (o·â)(r1Δ1 + sΔ2/2) − s c (r1Δ2/2 + sΔ3/3))`.
//! * **Torus** faces: the full torus (no wires or the K1
//!   seam-crossing pattern): `F = σ 6π² R r²`.
//!
//! `σ = −1` on a [`TopoFace::reversed`] face (the outward normal is
//! the natural one negated); wire flags enter only through the
//! sphere cap's kept side — everywhere else the region or the stored
//! frame determines the flux, which is why the historically
//! unopposed mantle/cap wires of the K1 cylinder and cone (their
//! rim circles are traversed in the same direction by mantle and
//! cap) do not disturb the measure. The guard `measure` does
//! enforce: `validate()`, and every non-degenerate edge used by
//! exactly two faces of the shell.
//!
//! Honest refusals, by name: B-spline faces (no closed form on
//! free-form surfaces — K4), untrimmed planes and cylinders
//! (unbounded regions), and any wire the module cannot read as one
//! of the trims above. Never an approximation.

use fc_base::Vector3;

use crate::topology::{Curve, ExactBRep, Surface, TopoFace, WireEdge};

/// Relative tolerance for the geometric recognition checks (the
/// trims must lie on their surfaces to machine scale).
const REL_TOL: f64 = 1e-9;

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// The scale for tolerance checks.
fn scale_of(brep: &ExactBRep) -> f64 {
    let mut s = 1.0f64;
    for v in &brep.vertices {
        s = s
            .max(v.point.x.abs())
            .max(v.point.y.abs())
            .max(v.point.z.abs());
    }
    s
}

/// Every non-degenerate edge must bound exactly two faces of the
/// shell (the measure precondition — degenerate edges are excluded,
/// as in the Euler count).
fn require_closed_shell(brep: &ExactBRep) -> Result<(), String> {
    for e in 0..brep.edges.len() {
        if brep.is_degenerate(e) {
            continue;
        }
        let uses = brep.edge_uses(e);
        if uses != 2 {
            return Err(format!(
                "not a closed shell: edge {e} ({}) is used by {uses} face wires",
                brep.edges[e].curve.kind_name()
            ));
        }
    }
    Ok(())
}

/// The exact volume of a closed, validated shell.
///
/// Composed of the per-face closed forms documented in the module
/// doc; the divergence theorem assembles them. Faces the module
/// cannot read are refused by name — never approximated.
pub fn brep_volume(brep: &ExactBRep) -> Result<f64, String> {
    brep.validate()
        .map_err(|e| format!("the B-rep does not validate: {e}"))?;
    require_closed_shell(brep)?;
    let mut total = 0.0;
    for (i, face) in brep.faces.iter().enumerate() {
        total += face_flux(brep, face, i)?;
    }
    Ok(total / 3.0)
}

/// The exact total boundary area of a closed, validated shell.
pub fn brep_area(brep: &ExactBRep) -> Result<f64, String> {
    brep.validate()
        .map_err(|e| format!("the B-rep does not validate: {e}"))?;
    require_closed_shell(brep)?;
    let mut total = 0.0;
    for (i, face) in brep.faces.iter().enumerate() {
        total += face_measure(brep, face, i)?.0;
    }
    Ok(total)
}

/// One face's `(area, flux)` — the flux of `p·n̂` with `n̂` the
/// OUTWARD unit normal (`F = ∫∫ p·n̂ dA`; the `1/3` is applied once,
/// by [`brep_volume`]).
fn face_measure(brep: &ExactBRep, face: &TopoFace, i: usize) -> Result<(f64, f64), String> {
    let ctx = FaceRef {
        brep,
        face,
        i,
        sigma: if face.reversed { -1.0 } else { 1.0 },
        tol: REL_TOL * scale_of(brep),
    };
    match &face.surface {
        Surface::Plane {
            origin,
            normal,
            x_dir,
        } => plane_face(ctx, *origin, *normal, *x_dir),
        Surface::Sphere { center, radius, .. } => sphere_face(ctx, *center, *radius),
        Surface::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => cylinder_face(ctx, *origin, *axis, *radius),
        Surface::Cone {
            origin,
            axis,
            semi_angle,
            ref_radius,
            ..
        } => cone_face(ctx, *origin, *axis, *semi_angle, *ref_radius),
        Surface::Torus {
            center,
            major_radius,
            minor_radius,
            ..
        } => torus_face(ctx, *center, *major_radius, *minor_radius),
        Surface::Bspline { .. } => Err(format!(
            "face {}: a B-spline surface — trimmed measure on free-form surfaces has no \
             closed form in this model (K4 work)",
            ctx.i
        )),
    }
}

fn face_flux(brep: &ExactBRep, face: &TopoFace, i: usize) -> Result<f64, String> {
    Ok(face_measure(brep, face, i)?.1)
}

/// The per-face reading context: the shell, the face, its index, its
/// orientation sign, and the recognition tolerance.
struct FaceRef<'a> {
    brep: &'a ExactBRep,
    face: &'a TopoFace,
    i: usize,
    sigma: f64,
    tol: f64,
}

// ---------------------------------------------------------------------------
// plane faces — polygons and disks
// ---------------------------------------------------------------------------

fn plane_face(
    ctx: FaceRef<'_>,
    o: Vector3,
    normal: Vector3,
    x_dir: Vector3,
) -> Result<(f64, f64), String> {
    let FaceRef {
        brep,
        face,
        i,
        sigma,
        tol,
        ..
    } = ctx;
    if face.wires.is_empty() {
        return Err(format!(
            "face {i}: an untrimmed plane face — an unbounded region"
        ));
    }
    let n_len = normal.length();
    if (n_len - 1.0).abs() > REL_TOL {
        return Err(format!(
            "face {i}: the plane normal must be unit (the K1 frame contract), |n| = {n_len}"
        ));
    }
    let frame = PlaneFrame {
        o,
        normal,
        x_dir,
        y_dir: normal.cross(&x_dir),
    };
    // The signed offset: p·n̂ is constant on the plane.
    let d = o.dot(&normal);
    let mut area = 0.0;
    for (wi, wire) in face.wires.iter().enumerate() {
        let a = wire_area_in_frame(brep, wire, i, wi, &frame, tol)?;
        area += if wi == 0 { a } else { -a };
    }
    if area < 0.0 {
        return Err(format!(
            "face {i}: the wires do not bound a region (outer area is negative)"
        ));
    }
    Ok((area, sigma * d * area))
}

/// The plane face's own frame: origin, unit normal, and the `(u, v)`
/// reading directions.
struct PlaneFrame {
    o: Vector3,
    normal: Vector3,
    x_dir: Vector3,
    y_dir: Vector3,
}

/// One wire's area in the face's `(x_dir, y_dir)` frame: a disk (a
/// single closed circle lying in the plane) or a polygon (line
/// edges, shoelace).
fn wire_area_in_frame(
    brep: &ExactBRep,
    wire: &crate::topology::TopoWire,
    i: usize,
    wi: usize,
    frame: &PlaneFrame,
    tol: f64,
) -> Result<f64, String> {
    let PlaneFrame {
        o,
        normal,
        x_dir,
        y_dir,
    } = *frame;
    // The disk: one non-degenerate closed circle lying in the plane.
    if wire.edges.len() == 1 {
        let edge = &brep.edges[wire.edges[0].edge];
        if let Curve::Circle {
            center,
            axis,
            radius,
            ..
        } = &edge.curve
        {
            let closed = edge.start == edge.end;
            if closed && *radius > 0.0 {
                // The circle lies in the plane: its center is on it
                // and its axis is the plane's normal (either way).
                let off = v3(center.x - o.x, center.y - o.y, center.z - o.z);
                if off.dot(&normal).abs() > tol || axis.dot(&normal).abs() < 1.0 - REL_TOL {
                    return Err(format!(
                        "face {i} wire {wi}: a circle trim not lying in the plane"
                    ));
                }
                return Ok(std::f64::consts::PI * radius * radius);
            }
        }
        // The elliptical disk: one closed ellipse lying in the plane
        // — the conic area πab (`p·n̂` is constant on the plane, so
        // the flux needs no centroid either).
        if let Curve::Ellipse {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } = &edge.curve
        {
            let closed = edge.start == edge.end;
            if closed && *minor_radius > 0.0 && *major_radius >= *minor_radius {
                let off = v3(center.x - o.x, center.y - o.y, center.z - o.z);
                if off.dot(&normal).abs() > tol || axis.dot(&normal).abs() < 1.0 - REL_TOL {
                    return Err(format!(
                        "face {i} wire {wi}: an ellipse trim not lying in the plane"
                    ));
                }
                return Ok(std::f64::consts::PI * major_radius * minor_radius);
            }
        }
    }
    // The conic-arc path (the K3d caps): exact line integrals per
    // edge — `∫(x dy − y dx) = R²Δθ + c × Δp` over each arc (closed
    // form), the straight terms `a × b` over each chord/bridge, the
    // area |Σ|/2 (the shoelace generalized to arcs).
    let has_arc = wire
        .edges
        .iter()
        .any(|we| matches!(&brep.edges[we.edge].curve, Curve::Bspline { degree: 2, .. }));
    if has_arc {
        let pt2 = |p: Vector3| {
            let off = v3(p.x - o.x, p.y - o.y, p.z - o.z);
            (off.dot(&x_dir), off.dot(&y_dir))
        };
        let mut twice = 0.0;
        for we in &wire.edges {
            let edge = &brep.edges[we.edge];
            if edge.start == edge.end {
                return Err(format!(
                    "face {i} wire {wi}: a closed edge in an arc-trimmed plane wire"
                ));
            }
            let (si, ei) = if we.forward {
                (edge.start, edge.end)
            } else {
                (edge.end, edge.start)
            };
            let ps = pt2(brep.vertices[si].point);
            let pe = pt2(brep.vertices[ei].point);
            let term = match &edge.curve {
                Curve::Line { .. } => ps.0 * pe.1 - ps.1 * pe.0,
                Curve::Bspline {
                    degree: 1,
                    points,
                    weights: None,
                    ..
                } if points.len() == 2 => ps.0 * pe.1 - ps.1 * pe.0,
                Curve::Bspline {
                    degree: 2, points, ..
                } => {
                    // The arc lies in the plane: its control points and
                    // its circle's center are on it.
                    for p in points {
                        let off = v3(p.x - o.x, p.y - o.y, p.z - o.z);
                        if off.dot(&normal).abs() > tol {
                            return Err(format!(
                                "face {i} wire {wi}: an arc trim not lying in the plane"
                            ));
                        }
                    }
                    let arc = match conic_arc_of(&edge.curve)? {
                        Some(a) => a,
                        None => {
                            return Err(format!(
                                "face {i} wire {wi}: a degree-2 trim that is not a \
                                 circular arc this reader knows"
                            ))
                        }
                    };
                    let off_c = v3(arc.center.x - o.x, arc.center.y - o.y, arc.center.z - o.z);
                    if off_c.dot(&normal).abs() > tol {
                        return Err(format!(
                            "face {i} wire {wi}: an arc trim not lying in the plane"
                        ));
                    }
                    let c2 = (off_c.dot(&x_dir), off_c.dot(&y_dir));
                    // The arc's sweep is CCW around its own axis; in
                    // the face frame (right-handed against the normal)
                    // it runs CCW iff the axes agree.
                    let sgn = arc.normal.dot(&normal).signum();
                    let fwd = if we.forward { 1.0 } else { -1.0 };
                    // The line integral over the stored direction,
                    // negated by a reversed use: R²·(sgn·sweep) plus
                    // the center's cross with the displacement.
                    fwd * arc.radius * arc.radius * sgn * arc.sweep + c2.0 * (pe.1 - ps.1)
                        - c2.1 * (pe.0 - ps.0)
                }
                _ => {
                    return Err(format!(
                        "face {i} wire {wi}: an edge the arc reader does not know ({})",
                        edge.curve.kind_name()
                    ))
                }
            };
            twice += term;
        }
        return Ok(twice.abs() / 2.0);
    }
    // The polygon: line edges, shoelace over the walk-order vertices.
    let mut points: Vec<(f64, f64)> = Vec::new();
    for we in &wire.edges {
        let edge = &brep.edges[we.edge];
        let s = if we.forward { edge.start } else { edge.end };
        let p = brep.vertices[s].point;
        let q = v3(p.x - o.x, p.y - o.y, p.z - o.z);
        points.push((q.dot(&x_dir), q.dot(&y_dir)));
    }
    if points.len() < 3 {
        return Err(format!(
            "face {i} wire {wi}: a plane wire of {} edge uses — neither polygon nor disk",
            points.len()
        ));
    }
    let mut twice = 0.0;
    let n = points.len();
    for k in 0..n {
        let (uk, vk) = points[k];
        let (u2, v2) = points[(k + 1) % n];
        twice += uk * v2 - u2 * vk;
    }
    Ok(twice.abs() / 2.0)
}

// ---------------------------------------------------------------------------
// sphere faces — the full sphere and the cap/complement
// ---------------------------------------------------------------------------

/// Does the face carry the K1 seam pattern (the whole sphere): every
/// non-degenerate edge a great-circle arc used exactly twice, once
/// forward and once reversed, within this face?
fn is_sphere_seam(brep: &ExactBRep, face: &TopoFace, c: Vector3, r: f64, tol: f64) -> bool {
    let mut seen_any = false;
    for wire in &face.wires {
        for we in &wire.edges {
            let edge = &brep.edges[we.edge];
            if brep.is_degenerate(we.edge) {
                continue;
            }
            seen_any = true;
            let uses: Vec<&WireEdge> = face
                .wires
                .iter()
                .flat_map(|w| &w.edges)
                .filter(|u| u.edge == we.edge)
                .collect();
            if uses.len() != 2 || uses[0].forward == uses[1].forward {
                return false;
            }
            match &edge.curve {
                Curve::Circle { center, radius, .. } => {
                    let dv = v3(center.x - c.x, center.y - c.y, center.z - c.z);
                    if dv.length() > tol || (radius - r).abs() > tol {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
    seen_any
}

fn sphere_face(ctx: FaceRef<'_>, c: Vector3, r: f64) -> Result<(f64, f64), String> {
    let FaceRef {
        brep,
        face,
        i,
        sigma,
        tol,
    } = ctx;
    let pi = std::f64::consts::PI;
    if face.wires.is_empty() || is_sphere_seam(brep, face, c, r, tol) {
        // The full sphere: p·n̂ = r everywhere and the vector area
        // vanishes by symmetry.
        let area = 4.0 * pi * r * r;
        return Ok((area, sigma * r * area));
    }
    // The cap/complement: exactly one wire, one non-degenerate
    // closed section circle.
    let single = if face.wires.len() == 1 && face.wires[0].edges.len() == 1 {
        let we = &face.wires[0].edges[0];
        let edge = &brep.edges[we.edge];
        match &edge.curve {
            Curve::Circle {
                center,
                axis,
                radius,
                ..
            } if edge.start == edge.end => Some((*center, *axis, *radius, we.forward)),
            _ => None,
        }
    } else {
        None
    };
    let (cc, axis, rho, forward) = match single {
        Some(x) if x.2 > 0.0 && !brep.is_degenerate(face.wires[0].edges[0].edge) => x,
        _ => {
            // The band: exactly two single-use closed section circles
            // (the two latitude trims), with the kept sides facing
            // each other; everything else non-degenerate is the seam
            // chain, used once each way within the face (it carries no
            // region semantics).
            return sphere_band(ctx, c, r);
        }
    };
    // The kept side (the module-doc convention): kept on the +axis
    // side iff (forward ⟺ σ = +1).
    let k_sign = if forward == !face.reversed { 1.0 } else { -1.0 };
    let k_hat = v3(axis.x * k_sign, axis.y * k_sign, axis.z * k_sign);
    let off = v3(cc.x - c.x, cc.y - c.y, cc.z - c.z);
    let s = off.dot(&k_hat);
    // The circle is a section of THIS sphere: its center is the foot
    // of the sphere's center on its plane, and ρ² + s² = r².
    let residual = v3(
        off.x - k_hat.x * s,
        off.y - k_hat.y * s,
        off.z - k_hat.z * s,
    );
    if residual.length() > tol || (rho * rho + s * s - r * r).abs() > tol * r.max(1.0) {
        return Err(format!(
            "face {i}: the sphere trim circle is not a section of the face's sphere"
        ));
    }
    let area = 2.0 * pi * r * (r - s);
    let flux = sigma * (r * area + c.dot(&k_hat) * pi * (r * r - s * s));
    Ok((area, flux))
}

/// The sphere band between two latitude circles (the K3e composed
/// trims): exactly two single-use closed section circles whose kept
/// sides face each other — the lower kept above its own plane, the
/// upper kept below — plus a seam chain (any remaining non-degenerate
/// edges used once each way within the face; the meridian arcs carry
/// no region semantics). The area is the spherical zone
/// `2πr(s_hi − s_lo)`; the flux adds the vector-area term
/// `c·k̂·π(s_hi² − s_lo²)` (the cap formula with both lids).
fn sphere_band(ctx: FaceRef<'_>, c: Vector3, r: f64) -> Result<(f64, f64), String> {
    let FaceRef {
        brep,
        face,
        i,
        sigma,
        tol,
    } = ctx;
    let pi = std::f64::consts::PI;
    // Partition the face's non-degenerate edge uses: the boundary
    // circles (used once) and the seam (used exactly twice, once each
    // way).
    let mut uses: Vec<usize> = Vec::new();
    for wire in &face.wires {
        for we in &wire.edges {
            if !brep.is_degenerate(we.edge) {
                uses.push(we.edge);
            }
        }
    }
    let mut boundary: Vec<(usize, bool)> = Vec::new();
    for e in &uses {
        if boundary.iter().any(|(b, _)| b == e) {
            continue;
        }
        let count = uses.iter().filter(|u| *u == e).count();
        let forwards: Vec<bool> = face
            .wires
            .iter()
            .flat_map(|w| &w.edges)
            .filter(|we| we.edge == *e)
            .map(|we| we.forward)
            .collect();
        if count == 1 {
            boundary.push((*e, forwards[0]));
        } else if count == 2 {
            if forwards[0] == forwards[1] {
                return Err(format!(
                    "face {i}: a sphere seam edge used twice in the same direction"
                ));
            }
        } else {
            return Err(format!(
                "face {i}: a sphere band edge used {count} times in the face"
            ));
        }
    }
    if boundary.len() != 2 {
        return Err(format!(
            "face {i}: a sphere face with an unrecognized trim — the band reading \
             needs exactly two single-use section circles"
        ));
    }
    // Both boundary circles must be sections of THIS sphere, and
    // their kept sides (the cap rule) must face each other.
    let mut bands: Vec<(Vector3, f64, Vector3)> = Vec::new(); // (center, ρ, k̂)
    for (e, forward) in &boundary {
        let edge = &brep.edges[*e];
        let (cc, axis, rho) = match &edge.curve {
            Curve::Circle {
                center,
                axis,
                radius,
                ..
            } if edge.start == edge.end && *radius > 0.0 => (*center, *axis, *radius),
            _ => {
                return Err(format!(
                    "face {i}: a sphere band trim that is not a closed circle"
                ))
            }
        };
        let k_sign = if *forward == !face.reversed {
            1.0
        } else {
            -1.0
        };
        let k_hat = v3(axis.x * k_sign, axis.y * k_sign, axis.z * k_sign);
        let off = v3(cc.x - c.x, cc.y - c.y, cc.z - c.z);
        let s = off.dot(&k_hat);
        let residual = v3(
            off.x - k_hat.x * s,
            off.y - k_hat.y * s,
            off.z - k_hat.z * s,
        );
        if residual.length() > tol || (rho * rho + s * s - r * r).abs() > tol * r.max(1.0) {
            return Err(format!(
                "face {i}: the sphere band trim circle is not a section of the face's sphere"
            ));
        }
        bands.push((cc, rho, k_hat));
    }
    // The kept directions face each other: the lower circle's k̂
    // points up (into the band), the upper's down.
    if bands[0].2.dot(&bands[1].2) > -(1.0 - REL_TOL) {
        return Err(format!(
            "face {i}: the sphere band's trim circles do not face each other"
        ));
    }
    // Order the pair: the band runs from the kept-above circle (the
    // lower) toward the kept-below one (the upper), along the lower's
    // own kept direction.
    let (lo, hi) = {
        let a = (bands[0].0, bands[0].2);
        let b = (bands[1].0, bands[1].2);
        let s_b = v3(b.0.x - c.x, b.0.y - c.y, b.0.z - c.z).dot(&a.1);
        if s_b > 0.0 {
            (a, b)
        } else {
            (b, a)
        }
    };
    let d = lo.1; // points from the lower circle toward the upper
    let s_lo = v3(lo.0.x - c.x, lo.0.y - c.y, lo.0.z - c.z).dot(&d);
    let s_hi = v3(hi.0.x - c.x, hi.0.y - c.y, hi.0.z - c.z).dot(&d);
    if s_hi <= s_lo {
        return Err(format!(
            "face {i}: the sphere band has no width ({s_lo} to {s_hi})"
        ));
    }
    let area = 2.0 * pi * r * (s_hi - s_lo);
    let flux = sigma * (r * area + c.dot(&d) * pi * (s_hi * s_hi - s_lo * s_lo));
    Ok((area, flux))
}

// ---------------------------------------------------------------------------
// cylinder faces — the two-circle strip
// ---------------------------------------------------------------------------

/// An elliptical rim recognized on a strip face — the K3a section
/// curve, stored with its own plane (the axis is the plane normal;
/// `δ = axis·center` is the plane's offset).
struct StripEllipse {
    center: Vector3,
    axis: Vector3,
    major_dir: Vector3,
    major_radius: f64,
    minor_radius: f64,
}

/// One section-circle rim: `(center, radius)`.
type CircleRim = (Vector3, f64);

/// A circular arc recognized from a rational quadratic B-spline (the
/// K3d arc representation — the classical NURBS circle arc, see
/// `brep_boolean`): its circle's center, the rotation axis the sweep
/// runs CCW around, the radius, and the sweep in `(0, 2π)`.
struct ConicArc {
    center: Vector3,
    normal: Vector3,
    radius: f64,
    sweep: f64,
}

/// Recognize the rational quadratic circular arc: degree 2, clamped
/// knots `[0,0,0,1,1,1]`, three control points, weights `[1, w, 1]`
/// with `0 < w ≤ 1`. The circle is re-derived from the control
/// polygon — the center at the intersection of the end tangents'
/// perpendiculars — and verified circular (equidistant ends, and the
/// weight equal to `cos(sweep/2)`; a genuine circular arc satisfies
/// both exactly). Anything else is not an arc this reader knows.
fn conic_arc_of(curve: &Curve) -> Result<Option<ConicArc>, String> {
    let Curve::Bspline {
        degree: 2,
        knots,
        points,
        weights: Some(w),
    } = curve
    else {
        return Ok(None);
    };
    if points.len() != 3
        || knots.as_slice() != [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
        || w[0] != 1.0
        || w[2] != 1.0
        || !(w[1] > 0.0 && w[1] <= 1.0)
    {
        return Ok(None);
    }
    let (p0, pm, p2) = (points[0], points[1], points[2]);
    // The in-plane frame: e₁ along the first tangent (toward the
    // middle control point), n̂ the plane normal, e₂ = n̂ × e₁.
    let t0 = v3(pm.x - p0.x, pm.y - p0.y, pm.z - p0.z);
    let t0_len = t0.length();
    if t0_len <= 0.0 {
        return Ok(None);
    }
    let e1 = t0 * (1.0 / t0_len);
    let d2 = v3(p2.x - p0.x, p2.y - p0.y, p2.z - p0.z);
    let mut n_hat = t0.cross(&d2);
    let n_len = n_hat.length();
    if n_len <= 0.0 {
        return Ok(None); // collinear control polygon — not an arc
    }
    n_hat = n_hat * (1.0 / n_len);
    let mut e2 = n_hat.cross(&e1);
    let mut q = d2.dot(&e2);
    if q < 0.0 {
        // The same plane, the other side: flip the frame so p₂ sits
        // off the tangent line toward +e₂.
        n_hat = v3(-n_hat.x, -n_hat.y, -n_hat.z);
        e2 = v3(-e2.x, -e2.y, -e2.z);
        q = -q;
    }
    if q <= f64::EPSILON * t0_len.max(1.0) {
        return Ok(None); // p₂ on the tangent line: degenerate
    }
    // The center: on the perpendicular to the tangent at p₀ and on
    // the perpendicular to the tangent at p₂ (both in-plane) — two
    // linear conditions. In the (e₁, e₂) frame with p₀ at the origin:
    // C = (0, c_y) with c_y from the p₂ condition.
    let p = d2.dot(&e1);
    let m1 = t0_len;
    let cy = (q * q + p * (p - m1)) / q;
    let center = v3(p0.x + e2.x * cy, p0.y + e2.y * cy, p0.z + e2.z * cy);
    let r0 = v3(p0.x - center.x, p0.y - center.y, p0.z - center.z).length();
    let r2l = v3(p2.x - center.x, p2.y - center.y, p2.z - center.z).length();
    if (r0 - r2l).abs() > REL_TOL * r0.max(1.0) {
        return Ok(None); // not circular
    }
    // The sweep: the angle of p₂ around the center from p₀, running
    // CCW around the axis n̂ — for a genuine arc (equidistant ends,
    // checked above) the axis from t₀ × d₂ with q > 0 is exactly that
    // rotation axis: the center lies on the +e₂ side (cy > 0, else the
    // ends cannot be equidistant), and the tangent at p₀ toward m is
    // the CCW tangent around n̂.
    let u0 = v3(
        (p0.x - center.x) / r0,
        (p0.y - center.y) / r0,
        (p0.z - center.z) / r0,
    );
    let y0 = n_hat.cross(&u0);
    let u2 = v3(p2.x - center.x, p2.y - center.y, p2.z - center.z);
    let theta2 = u2.dot(&y0).atan2(u2.dot(&u0));
    let sweep = if theta2 > 0.0 {
        theta2
    } else {
        theta2 + std::f64::consts::TAU
    };
    // The circularity check: a rational quadratic through these
    // points with these tangents is the circular arc exactly when the
    // weight is cos(sweep/2).
    if (w[1] - (sweep / 2.0).cos()).abs() > REL_TOL {
        return Ok(None);
    }
    Ok(Some(ConicArc {
        center,
        normal: n_hat,
        radius: r0,
        sweep,
    }))
}

/// One recognized arc trim: `(edge index, the arc)`.
type ArcRim = (usize, ConicArc);

/// Everything a strip face's wires carry: the section-circle rims,
/// at most one ellipse (the composed oblique section), and the K3d
/// conic arcs.
type StripTrims = (Vec<CircleRim>, Option<StripEllipse>, Vec<ArcRim>);

/// The rims a strip face carries: its section circles (checked
/// coaxial with `axis` and — when `radius` is `Some` — of the
/// surface's radius; the cone passes `None` and checks against its
/// own radius function instead) and at most one ellipse (the
/// composed oblique section). Seam edges are glue with no region
/// semantics: a `Line`, or the degree-1 two-point B-spline the
/// composed strips carry (its de Boor endpoints are exact — see
/// `brep_boolean`). The count checks live at the call sites (the
/// ellipse-trimmed strip has ONE circle rim). The K3d strips add the
/// rational quadratic arcs (collected; the cylinder's arc-strip
/// reading consumes them).
fn strip_rims(
    ctx: &FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: Option<f64>,
    allow_degenerate: bool,
) -> Result<StripTrims, String> {
    let FaceRef {
        brep, face, i, tol, ..
    } = *ctx;
    let mut rims: Vec<(Vector3, f64)> = Vec::new();
    let mut ellipse: Option<StripEllipse> = None;
    let mut arcs: Vec<ArcRim> = Vec::new();
    for wire in &face.wires {
        for we in &wire.edges {
            let edge = &brep.edges[we.edge];
            match &edge.curve {
                Curve::Circle {
                    center,
                    radius: rho,
                    ..
                } if edge.start == edge.end => {
                    let degen = *rho == 0.0;
                    if degen && !allow_degenerate {
                        return Err(format!(
                            "face {i}: a degenerate rim on this surface — unrecognized trim"
                        ));
                    }
                    if let Some(r) = radius {
                        if !degen && (*rho - r).abs() > tol {
                            return Err(format!(
                                "face {i}: a rim circle of radius {rho} on a radius-{r} \
                                 surface — not a section circle"
                            ));
                        }
                    }
                    let off = v3(center.x - o.x, center.y - o.y, center.z - o.z);
                    let along = off.dot(&axis);
                    let residual = v3(
                        off.x - axis.x * along,
                        off.y - axis.y * along,
                        off.z - axis.z * along,
                    );
                    if residual.length() > tol {
                        return Err(format!(
                            "face {i}: a rim circle not coaxial with the surface"
                        ));
                    }
                    rims.push((*center, *rho));
                }
                Curve::Circle {
                    center,
                    axis: c_axis,
                    radius: rho,
                    ..
                } if edge.start != edge.end => {
                    // An OPEN circle edge — an arc rim read verbatim
                    // from a real conic (the OCCT reader's trimmed
                    // circles): same coaxial/radius contract as the
                    // closed rims, sweep straight from the range (the
                    // curve parameter runs CCW around its axis).
                    if *rho <= 0.0 {
                        return Err(format!("face {i}: an arc rim of radius {rho}"));
                    }
                    if let Some(r) = radius {
                        if (*rho - r).abs() > tol {
                            return Err(format!(
                                "face {i}: an arc of radius {rho} on a radius-{r} surface — \
                                 not a section circle"
                            ));
                        }
                    }
                    let off = v3(center.x - o.x, center.y - o.y, center.z - o.z);
                    let along = off.dot(&axis);
                    let residual = v3(
                        off.x - axis.x * along,
                        off.y - axis.y * along,
                        off.z - axis.z * along,
                    );
                    if residual.length() > tol {
                        return Err(format!("face {i}: an arc rim not coaxial with the surface"));
                    }
                    if c_axis.cross(&axis).length() > 1e-9 {
                        return Err(format!(
                            "face {i}: an arc rim whose plane is not perpendicular to the \
                             surface axis"
                        ));
                    }
                    let sweep = edge.range.1 - edge.range.0;
                    if !(sweep > 0.0 && sweep <= 2.0 * std::f64::consts::PI + tol) {
                        return Err(format!("face {i}: an arc rim of sweep {sweep}"));
                    }
                    arcs.push((
                        we.edge,
                        ConicArc {
                            center: *center,
                            normal: *c_axis,
                            radius: *rho,
                            sweep,
                        },
                    ));
                }
                Curve::Ellipse {
                    center,
                    axis: e_axis,
                    major_dir,
                    major_radius,
                    minor_radius,
                } if edge.start == edge.end => {
                    if ellipse.is_some() {
                        return Err(format!(
                            "face {i}: two elliptical rims — a strip carries one section \
                             curve"
                        ));
                    }
                    if *major_radius < *minor_radius || *minor_radius <= 0.0 {
                        return Err(format!(
                            "face {i}: a malformed elliptical rim (a = {major_radius}, \
                             b = {minor_radius})"
                        ));
                    }
                    ellipse = Some(StripEllipse {
                        center: *center,
                        axis: *e_axis,
                        major_dir: *major_dir,
                        major_radius: *major_radius,
                        minor_radius: *minor_radius,
                    });
                }
                Curve::Bspline { degree: 2, .. } => match conic_arc_of(&edge.curve)? {
                    Some(arc) => arcs.push((we.edge, arc)),
                    None => {
                        return Err(format!(
                            "face {i}: a degree-2 B-spline that is not a circular arc \
                             this reader knows"
                        ))
                    }
                },
                // Seam glue — no region semantics. The composed strips
                // carry the seam as a degree-1 two-point B-spline.
                Curve::Line { .. } => {}
                Curve::Bspline {
                    degree: 1,
                    points,
                    weights: None,
                    ..
                } if points.len() == 2 => {}
                _ => {
                    return Err(format!(
                        "face {i}: an unrecognized rim ({} edges are not section circles, \
                         ellipses, arcs or seam glue)",
                        brep.edges[we.edge].curve.kind_name()
                    ))
                }
            }
        }
    }
    Ok((rims, ellipse, arcs))
}

fn cylinder_face(
    ctx: FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: f64,
) -> Result<(f64, f64), String> {
    let FaceRef {
        face,
        i,
        sigma,
        tol,
        ..
    } = ctx;
    if face.wires.is_empty() {
        return Err(format!(
            "face {i}: an untrimmed cylinder face — an unbounded region"
        ));
    }
    let (rims, ellipse, arcs) = strip_rims(&ctx, o, axis, Some(radius), false)?;
    if !arcs.is_empty() {
        if ellipse.is_some() {
            return Err(format!(
                "face {i}: a mixed ellipse/arc cylinder strip — not a trim this \
                 reader knows"
            ));
        }
        // The K3d arc-trimmed strips: the standalone side face (arcs
        // only) or the notched mantle (two circle rims + the window's
        // arcs as a hole wire).
        if rims.is_empty() {
            return cylinder_arc_strip(ctx, o, axis, radius, arcs);
        }
        return cylinder_notched_strip(ctx, o, axis, radius, &rims, &arcs);
    }
    if let Some(e) = ellipse {
        // The composed oblique section: ONE circle rim + the elliptical
        // rim, whose plane reduces — in the cylinder's own frame — to
        // the sinusoid v(u) = z0 + A·cos u + B·sin u, exact from the
        // stored vectors (the rim's own plane; never the UV box).
        if rims.len() != 1 {
            return Err(format!(
                "face {i}: an ellipse-trimmed cylinder strip needs exactly one circle \
                 rim, found {}",
                rims.len()
            ));
        }
        return cylinder_ellipse_strip(ctx, o, axis, radius, e, rims[0]);
    }
    if rims.len() != 2 {
        return Err(format!(
            "face {i}: a cylinder face needs exactly two section-circle rims, found {}",
            rims.len()
        ));
    }
    let v: Vec<f64> = rims
        .iter()
        .map(|(cc, _)| v3(cc.x - o.x, cc.y - o.y, cc.z - o.z).dot(&axis))
        .collect();
    let (v0, v1) = (v[0].min(v[1]), v[0].max(v[1]));
    if v1 - v0 <= tol {
        return Err(format!(
            "face {i}: a degenerate (zero-height) cylinder strip"
        ));
    }
    let h = v1 - v0;
    // ∫r̂(u) du = 0 over the full turn, so the origin drops out.
    let area = 2.0 * std::f64::consts::PI * radius * h;
    let flux = sigma * 2.0 * std::f64::consts::PI * radius * radius * h;
    Ok((area, flux))
}

/// Verify a cylinder face's conic-arc trims and derive the swept
/// window they bound: the `(θ_start, total signed sweep, w_lo, w_hi)`
/// of the angular interval × height window the arcs chain to (the
/// crossing boolean's notch or side face). The arcs' circles are
/// re-derived (`conic_arc_of`), verified coaxial and of the surface's
/// radius; the two heights' chains must cover the same interval; the
/// remaining non-arc edges must be generator glue — the window's
/// verticals at the interval's ends (and, with
/// `allow_full_generators`, the seam of a notched mantle, a
/// full-height generator at any angle).
fn analyze_arc_trims(
    ctx: &FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: f64,
    arcs: &[ArcRim],
    allow_full_generators: bool,
) -> Result<(f64, f64, f64, f64), String> {
    let FaceRef {
        brep, face, i, tol, ..
    } = *ctx;
    let x_hat = match &face.surface {
        Surface::Cylinder { ref_dir, .. } => *ref_dir,
        _ => unreachable!("checked by the caller"),
    };
    let y_hat = axis.cross(&x_hat);
    // Verify each arc against the surface, and collect its height and
    // its signed sweep around the surface axis.
    let mut info: Vec<(f64, f64, usize)> = Vec::with_capacity(arcs.len());
    for (ei, arc) in arcs {
        let ei = *ei;
        if (arc.radius - radius).abs() > tol {
            return Err(format!(
                "face {i}: an arc of radius {} on a radius-{radius} cylinder",
                arc.radius
            ));
        }
        let off = v3(arc.center.x - o.x, arc.center.y - o.y, arc.center.z - o.z);
        let along = off.dot(&axis);
        let residual = v3(
            off.x - axis.x * along,
            off.y - axis.y * along,
            off.z - axis.z * along,
        );
        if residual.length() > tol {
            return Err(format!(
                "face {i}: an arc circle not coaxial with the cylinder"
            ));
        }
        // The arc's endpoints lie on the surface and in the circle's
        // plane ⊥ axis (the tangent-intersection control point is off
        // the surface BY DESIGN — at R/cos(sweep/2) — so only the
        // endpoints are checked; the chain's interior junctions are
        // exact circle evaluations).
        if let Curve::Bspline { points, .. } = &brep.edges[ei].curve {
            for p in [&points[0], &points[points.len() - 1]] {
                let off = v3(p.x - o.x, p.y - o.y, p.z - o.z);
                let along = off.dot(&axis);
                let radial = v3(
                    off.x - axis.x * along,
                    off.y - axis.y * along,
                    off.z - axis.z * along,
                )
                .length();
                if (radial - radius).abs() > tol {
                    return Err(format!(
                        "face {i}: an arc endpoint off the cylinder surface"
                    ));
                }
            }
        }
        let side = arc.normal.dot(&axis).signum();
        info.push((along, side * arc.sweep, ei));
    }
    // Group the arcs by height (the two boundary planes).
    let mut heights: Vec<f64> = Vec::with_capacity(2);
    for &(v, _, _) in &info {
        if !heights.iter().any(|&hv| (hv - v).abs() <= tol) {
            heights.push(v);
        }
    }
    if heights.len() != 2 {
        return Err(format!(
            "face {i}: arc trims at {} height(s) — the strip needs exactly two",
            heights.len()
        ));
    }
    let (w_lo, w_hi) = (heights[0].min(heights[1]), heights[0].max(heights[1]));
    if w_hi - w_lo <= tol {
        return Err(format!(
            "face {i}: a degenerate (zero-height) arc-trimmed strip"
        ));
    }
    let group_of = |v: f64| -> usize {
        if (v - heights[0]).abs() <= (v - heights[1]).abs() {
            0
        } else {
            1
        }
    };
    let mut group_a: Vec<usize> = Vec::new();
    let mut group_b: Vec<usize> = Vec::new();
    for (k, &(v, _, _)) in info.iter().enumerate() {
        if group_of(v) == 0 {
            group_a.push(k);
        } else {
            group_b.push(k);
        }
    }

    // Chain one group's arcs by their shared vertices (the stored
    // direction — every arc of the chain runs the same way, from the
    // crossing generator at the interval's start to the one at its
    // end). Returns (θ_start, total signed sweep).
    let chain_of = |group: &[usize]| -> Result<(f64, f64), String> {
        let head = (0..group.len())
            .find(|&h| {
                let sv = brep.edges[info[group[h]].2].start;
                !group
                    .iter()
                    .enumerate()
                    .any(|(a, &k)| a != h && brep.edges[info[k].2].end == sv)
            })
            .ok_or_else(|| format!("face {i}: arc trims that do not chain"))?;
        let mut order = Vec::with_capacity(group.len());
        let mut current = group[head];
        loop {
            order.push(current);
            let end_vx = brep.edges[info[current].2].end;
            match group
                .iter()
                .find(|&&k| k != current && brep.edges[info[k].2].start == end_vx)
            {
                Some(&next) => current = next,
                None => break,
            }
        }
        if order.len() != group.len() {
            return Err(format!("face {i}: arc trims that do not chain"));
        }
        let start_pt = brep.vertices[brep.edges[info[order[0]].2].start].point;
        let off = v3(start_pt.x - o.x, start_pt.y - o.y, start_pt.z - o.z);
        let theta = off.dot(&y_hat).atan2(off.dot(&x_hat));
        let total: f64 = order.iter().map(|&k| info[k].1).sum();
        Ok((theta, total))
    };
    let (th1, total1) = chain_of(&group_a)?;
    let (th2, total2) = chain_of(&group_b)?;
    let wrap = |d: f64| {
        let mut d = d;
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        d
    };
    if wrap(th2 - th1).abs() > 1e-7 || (total2 - total1).abs() > 1e-7 {
        return Err(format!(
            "face {i}: the two heights' arc chains do not cover the same interval"
        ));
    }
    let theta = th1;
    let total = total1;
    if total.abs() <= 1e-12 {
        return Err(format!("face {i}: a zero-sweep arc-trimmed strip"));
    }

    // The remaining edges: generator glue — the window's verticals at
    // the interval's two ends, plus (for a notched mantle) full-height
    // seam generators.
    let arc_set: Vec<usize> = arcs.iter().map(|p| p.0).collect();
    let end_angles = [theta, theta + total];
    for wire in &face.wires {
        for we in &wire.edges {
            if arc_set.contains(&we.edge) {
                continue;
            }
            let edge = &brep.edges[we.edge];
            match &edge.curve {
                // The notched mantle's outer boundary: closed circle
                // rims (checked coaxial and of the surface's radius
                // by the caller's rim reading).
                Curve::Circle { .. } if edge.start == edge.end => {
                    if !allow_full_generators {
                        return Err(format!("face {i}: a closed rim on the arc strip"));
                    }
                }
                Curve::Line { .. } | Curve::Bspline { degree: 1, .. } => {
                    let mut angles = Vec::with_capacity(2);
                    let mut vs = Vec::with_capacity(2);
                    for &vx in &[edge.start, edge.end] {
                        let p = brep.vertices[vx].point;
                        let off = v3(p.x - o.x, p.y - o.y, p.z - o.z);
                        vs.push(off.dot(&axis));
                        angles.push(off.dot(&y_hat).atan2(off.dot(&x_hat)));
                    }
                    let same_generator = wrap(angles[0] - angles[1]).abs() <= 1e-7;
                    if !same_generator {
                        return Err(format!("face {i}: a non-arc trim not on a generator"));
                    }
                    let at_interval_end = end_angles
                        .iter()
                        .any(|&e| wrap(angles[0] - e).abs() <= 1e-7);
                    let window_vertical = at_interval_end
                        && (vs[0] - w_lo).abs() <= tol
                        && (vs[1] - w_hi).abs() <= tol;
                    let full_generator = allow_full_generators && (vs[0] - w_lo).abs() > tol;
                    if !window_vertical && !full_generator {
                        return Err(format!(
                            "face {i}: a non-arc edge off the window's generators"
                        ));
                    }
                }
                _ => {
                    return Err(format!(
                        "face {i}: an unrecognized non-arc trim on the arc strip"
                    ))
                }
            }
        }
    }
    Ok((theta, total, w_lo, w_hi))
}

/// The arc window's area and flux contributions:
/// `A = R·|Δθ|·Δv`, `F = R·Δv·(R|Δθ| + o·W)` with
/// `W = ∫_I r̂ dθ` — `p·r̂ = o·r̂(u) + R` on the cylinder, the
/// origin's coupling with `r̂` surviving the partial turn as `W`.
fn arc_window_terms(
    o: Vector3,
    x_hat: Vector3,
    y_hat: Vector3,
    radius: f64,
    theta: f64,
    total: f64,
    dv: f64,
) -> (f64, f64) {
    let a = theta + total.min(0.0);
    let b = theta + total.max(0.0);
    let width = total.abs();
    let area = radius * width * dv;
    let w = v3(
        (b.sin() - a.sin()) * x_hat.x + (a.cos() - b.cos()) * y_hat.x,
        (b.sin() - a.sin()) * x_hat.y + (a.cos() - b.cos()) * y_hat.y,
        (b.sin() - a.sin()) * x_hat.z + (a.cos() - b.cos()) * y_hat.z,
    );
    let flux = radius * dv * (radius * width + o.dot(&w));
    (area, flux)
}

/// The standalone arc-trimmed cylinder strip (the K3d crossing
/// booleans' side faces): the swept region `u ∈ I × v ∈ [w_lo, w_hi]`.
fn cylinder_arc_strip(
    ctx: FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: f64,
    arcs: Vec<ArcRim>,
) -> Result<(f64, f64), String> {
    let FaceRef { face, sigma, .. } = ctx;
    let x_hat = match &face.surface {
        Surface::Cylinder { ref_dir, .. } => *ref_dir,
        _ => unreachable!("checked by the caller"),
    };
    let y_hat = axis.cross(&x_hat);
    let (theta, total, w_lo, w_hi) = analyze_arc_trims(&ctx, o, axis, radius, &arcs, false)?;
    let (area, flux) = arc_window_terms(o, x_hat, y_hat, radius, theta, total, w_hi - w_lo);
    Ok((area, sigma * flux))
}

/// The notched mantle (the K3d blind drill): the full strip between
/// two section-circle rims with the arc window cut out of its middle
/// — the outer wire the K1 mantle pattern, the hole wire the window's
/// arcs and verticals. Area and flux are the full strip minus the
/// window's.
fn cylinder_notched_strip(
    ctx: FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: f64,
    rims: &[CircleRim],
    arcs: &[ArcRim],
) -> Result<(f64, f64), String> {
    let FaceRef {
        face,
        i,
        sigma,
        tol,
        ..
    } = ctx;
    if rims.len() != 2 {
        return Err(format!(
            "face {i}: a notched mantle needs exactly two circle rims, found {}",
            rims.len()
        ));
    }
    let v: Vec<f64> = rims
        .iter()
        .map(|(cc, _)| v3(cc.x - o.x, cc.y - o.y, cc.z - o.z).dot(&axis))
        .collect();
    let (v0, v1) = (v[0].min(v[1]), v[0].max(v[1]));
    if v1 - v0 <= tol {
        return Err(format!(
            "face {i}: a degenerate (zero-height) notched mantle"
        ));
    }
    let x_hat = match &face.surface {
        Surface::Cylinder { ref_dir, .. } => *ref_dir,
        _ => unreachable!("checked by the caller"),
    };
    let y_hat = axis.cross(&x_hat);
    let (theta, total, w_lo, w_hi) = analyze_arc_trims(&ctx, o, axis, radius, arcs, true)?;
    // The window sits strictly inside the strip (a window touching a
    // rim is not the hole structure — that face would carry the
    // split-circle wire of a later slice).
    if w_lo - v0 <= tol || v1 - w_hi <= tol {
        return Err(format!(
            "face {i}: the notch reaches a rim — the split-circle mantle is a later slice"
        ));
    }
    let full_area = 2.0 * std::f64::consts::PI * radius * (v1 - v0);
    let full_flux = 2.0 * std::f64::consts::PI * radius * radius * (v1 - v0);
    let (win_area, win_flux) = arc_window_terms(o, x_hat, y_hat, radius, theta, total, w_hi - w_lo);
    Ok((full_area - win_area, sigma * (full_flux - win_flux)))
}

/// The ellipse-trimmed cylinder strip: the region between the rim
/// circle at `v_rim` and the sinusoid `v_e(u) = z0 + A·cos u +
/// B·sin u` — the oblique plane's height on the cylinder, derived
/// exactly from the stored rim (`ν = n_e·axis ≠ 0`).
///
/// * Area `= 2πR·|z0 − v_rim|` — Cavalieri: an oblique cut leaves
///   the mean height alone.
/// * Flux `= σR·(R·2πΔv̄ + π(A·oₓ + B·o_y))` — `p·r̂ = o·r̂(u) + R`,
///   the `z0` part of the sinusoid coupling with the origin's
///   radial part vanishing over the turn, the `(A, B)` part not.
fn cylinder_ellipse_strip(
    ctx: FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: f64,
    e: StripEllipse,
    rim: CircleRim,
) -> Result<(f64, f64), String> {
    let FaceRef {
        face,
        i,
        sigma,
        tol,
        ..
    } = ctx;
    let (rim_center, _rim_radius) = rim;
    // The rim must lie on the axis (coaxial, checked by strip_rims) —
    // its v-parameter:
    let v_rim = v3(rim_center.x - o.x, rim_center.y - o.y, rim_center.z - o.z).dot(&axis);
    // The ellipse's plane: axis n_e through its center. On the
    // cylinder, the generator at u meets the plane at
    // v_e(u) = (δ − n·o − R·m(u))/ν  with m(u) = (n·x̂)cos u + (n·ŷ)sin u.
    let nu = e.axis.dot(&axis);
    if nu == 0.0 {
        return Err(format!(
            "face {i}: an elliptical rim whose plane is parallel to the axis — not a \
             cylinder section"
        ));
    }
    // The section ellipse is centered on the axis — check it, then
    // sample the rim onto the surface.
    let off_c = v3(e.center.x - o.x, e.center.y - o.y, e.center.z - o.z);
    let along = off_c.dot(&axis);
    let residual = v3(
        off_c.x - axis.x * along,
        off_c.y - axis.y * along,
        off_c.z - axis.z * along,
    );
    if residual.length() > tol {
        return Err(format!(
            "face {i}: an elliptical rim not centered on the axis — not a cylinder \
             section"
        ));
    }
    let curve = Curve::Ellipse {
        center: e.center,
        axis: e.axis,
        major_dir: e.major_dir,
        major_radius: e.major_radius,
        minor_radius: e.minor_radius,
    };
    for t in [0.0, 1.1, 2.2, 3.3, 4.4, 5.5] {
        let p = curve.point(t);
        let d = v3(p.x - o.x, p.y - o.y, p.z - o.z);
        let a = d.dot(&axis);
        let r = v3(d.x - axis.x * a, d.y - axis.y * a, d.z - axis.z * a).length();
        if (r - radius).abs() > tol {
            return Err(format!(
                "face {i}: an elliptical rim not lying on the cylinder — not a section \
                 curve"
            ));
        }
    }
    let x_hat = match &face.surface {
        Surface::Cylinder { ref_dir, .. } => *ref_dir,
        _ => unreachable!("the cylinder face"),
    };
    let y_hat = axis.cross(&x_hat);
    let delta = e.axis.dot(&e.center);
    let z0 = (delta - e.axis.dot(&o)) / nu;
    let a_c = -radius * e.axis.dot(&x_hat) / nu;
    let b_c = -radius * e.axis.dot(&y_hat) / nu;
    // The sinusoid must sit strictly on one side of the rim.
    let gap = z0 - v_rim;
    if gap * gap <= a_c * a_c + b_c * b_c {
        return Err(format!(
            "face {i}: the elliptical rim's sinusoid is not strictly on one side of the \
             circle rim — a degenerate or self-overlapping strip"
        ));
    }
    let s = gap.signum();
    let area = 2.0 * std::f64::consts::PI * radius * (s * gap);
    let o_x = o.dot(&x_hat);
    let o_y = o.dot(&y_hat);
    let flux = sigma
        * radius
        * (radius * 2.0 * std::f64::consts::PI * (s * gap)
            + std::f64::consts::PI * s * (a_c * o_x + b_c * o_y));
    Ok((area, flux))
}

// ---------------------------------------------------------------------------
// cone faces — the frustum strip in the slant parameter
// ---------------------------------------------------------------------------

fn cone_face(
    ctx: FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    semi: f64,
    r1: f64,
) -> Result<(f64, f64), String> {
    let FaceRef {
        face,
        i,
        sigma,
        tol,
        ..
    } = ctx;
    if face.wires.is_empty() {
        return Err(format!(
            "face {i}: an untrimmed cone face — an unbounded region"
        ));
    }
    let (s, c) = semi.sin_cos();
    if c <= 0.0 {
        return Err(format!(
            "face {i}: a cone semi-angle out of range (cos = {c})"
        ));
    }
    // Rim check against the surface's own radius function w(v) = r1
    // + v·sin(semi), then read the slant parameters.
    let (rims, ellipse, _arcs) = strip_rims(&ctx, o, axis, None, true)?;
    if let Some(e) = ellipse {
        if rims.len() != 1 {
            return Err(format!(
                "face {i}: an ellipse-trimmed cone strip needs exactly one circle \
                 rim, found {}",
                rims.len()
            ));
        }
        return cone_ellipse_strip(ctx, o, axis, semi, r1, e, rims[0]);
    }
    let mut vs: Vec<f64> = Vec::new();
    for (cc, rho) in &rims {
        let off = v3(cc.x - o.x, cc.y - o.y, cc.z - o.z);
        let v = off.dot(&axis) / c;
        if (rho - (r1 + v * s)).abs() > tol {
            return Err(format!(
                "face {i}: a rim circle of radius {rho} that is not a section of this cone"
            ));
        }
        vs.push(v);
    }
    if rims.len() != 2 {
        return Err(format!(
            "face {i}: a cone face needs exactly two section-circle rims, found {}",
            rims.len()
        ));
    }
    let (v0, v1) = (vs[0].min(vs[1]), vs[0].max(vs[1]));
    if v1 - v0 <= tol {
        return Err(format!("face {i}: a degenerate (zero-slant) cone strip"));
    }
    let oz = o.dot(&axis);
    let d1 = v1 - v0;
    let d2 = v1 * v1 - v0 * v0;
    let d3 = v1.powi(3) - v0.powi(3);
    // The tangent planes all pass through the apex; the definite
    // integral of p·n̂ over the strip collapses (the Δ3 terms cancel
    // between the w² and v·w parts).
    let flux = sigma
        * 2.0
        * std::f64::consts::PI
        * (c * (r1 * r1 * d1 + r1 * s * d2 + s * s * d3 / 3.0)
            - s * oz * (r1 * d1 + s * d2 / 2.0)
            - s * c * (r1 * d2 / 2.0 + s * d3 / 3.0));
    let area = 2.0 * std::f64::consts::PI * (r1 * d1 + s * d2 / 2.0);
    Ok((area, flux))
}

/// The ellipse-trimmed cone strip: the region between the circle rim
/// at slant `v_rim` and the **Möbius trim** — the oblique plane's
/// height on the cone,
///
/// ```text
/// v_e(u) = (δ − n·o − r1·m(u)) / (cosφ·ν + sinφ·m(u)),
///   m(u) = (n·x̂)cos u + (n·ŷ)sin u,  ν = n·axis,
/// ```
///
/// a ratio of affine functions of the sinusoid m(u). Its moments
/// reduce — after the partial-fraction split `v_e = A₀ +
/// B₀/(κ + M̃·cosθ)` (κ = cosφ·ν, M̃ = sinφ·M) and the rotation
/// m(u) = M·cos θ — to the standard closed forms
///
/// ```text
/// I₀  = ∫du/(κ+M̃cosθ)   = sgn(κ)·2π/√q
/// I₀' = ∫du/(κ+M̃cosθ)² = 2π|κ|/√q³
/// I_c  = ∫cosθ·du/(κ+M̃cosθ)   = −2πM̃/(√q·(√q+|κ|))
/// I_c' = ∫cosθ·du/(κ+M̃cosθ)² = −2π·sgn(κ)·M̃/√q³
/// ```
///
/// with `√q = √(κ² − M̃²)` (the `I_c` form rationalized — the naive
/// `1 − |κ|/√q` cancels catastrophically as M̃ → 0; this one is
/// exact and vanishes smoothly). The sin-coupled moments vanish by
/// symmetry; the cos-coupled ones carry the origin's radial part
/// through `P` (its component along m's phase). Every tangent plane
/// of the cone passes through the apex, so `p·n̂ = cosφ·(o·r̂(u)) −
/// sinφ·(o·axis) + r1·cosφ` — constant along the rulings — and the
/// flux collapses onto the same moment families.
fn cone_ellipse_strip(
    ctx: FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    semi: f64,
    r1: f64,
    e: StripEllipse,
    rim: CircleRim,
) -> Result<(f64, f64), String> {
    let FaceRef {
        face,
        i,
        sigma,
        tol,
        ..
    } = ctx;
    let (s, c) = semi.sin_cos();
    if c <= 0.0 {
        return Err(format!(
            "face {i}: a cone semi-angle out of range (cos = {c})"
        ));
    }
    let (rim_center, rim_rho) = rim;
    let v_rim = v3(rim_center.x - o.x, rim_center.y - o.y, rim_center.z - o.z).dot(&axis) / c;
    if (rim_rho - (r1 + v_rim * s)).abs() > tol {
        return Err(format!(
            "face {i}: a rim circle of radius {rim_rho} that is not a section of this cone"
        ));
    }
    // The rim must lie on the cone (sampled at six parameters).
    let curve = Curve::Ellipse {
        center: e.center,
        axis: e.axis,
        major_dir: e.major_dir,
        major_radius: e.major_radius,
        minor_radius: e.minor_radius,
    };
    for t in [0.0, 1.1, 2.2, 3.3, 4.4, 5.5] {
        let p = curve.point(t);
        let d = v3(p.x - o.x, p.y - o.y, p.z - o.z);
        let along = d.dot(&axis);
        let v_p = along / c;
        let radial = v3(
            d.x - axis.x * along,
            d.y - axis.y * along,
            d.z - axis.z * along,
        )
        .length();
        if (radial - (r1 + v_p * s)).abs() > tol * (1.0 + v_p.abs()) {
            return Err(format!(
                "face {i}: an elliptical rim not lying on the cone — not a section curve"
            ));
        }
    }
    let x_hat = match &face.surface {
        Surface::Cone { ref_dir, .. } => *ref_dir,
        _ => unreachable!("the cone face"),
    };
    let y_hat = axis.cross(&x_hat);
    let m_c = e.axis.dot(&x_hat);
    let m_s = e.axis.dot(&y_hat);
    let m_amp = (m_c * m_c + m_s * m_s).sqrt();
    let nu = e.axis.dot(&axis);
    let kappa = c * nu;
    let m_tilde = s * m_amp;
    let q = kappa * kappa - m_tilde * m_tilde;
    if q <= 0.0 {
        return Err(format!(
            "face {i}: the elliptical rim's plane is too steep for this cone (κ² ≤ \
             (sinφ·M)²) — not a closed section"
        ));
    }
    let sqrt_q = q.sqrt();
    // v_e = A₀ + B₀/(κ + M̃·cos θ).
    let a0 = if s != 0.0 { -r1 / s } else { 0.0 };
    let delta = e.axis.dot(&e.center);
    let gamma0 = delta - e.axis.dot(&o);
    let b0 = gamma0 - a0 * kappa;
    let (i0, i0p) = (
        kappa.signum() * 2.0 * std::f64::consts::PI / sqrt_q,
        2.0 * kappa.abs() * std::f64::consts::PI / (sqrt_q * q),
    );
    let (ic, icp) = (
        -2.0 * std::f64::consts::PI * m_tilde / (sqrt_q * (sqrt_q + kappa.abs())),
        -kappa.signum() * 2.0 * std::f64::consts::PI * m_tilde / (sqrt_q * q),
    );
    // The origin's radial part along m's phase: P = (o·x̂, o·ŷ)·(m_c,
    // m_s)/M; the sin-coupled moments vanish.
    let o_x = o.dot(&x_hat);
    let o_y = o.dot(&y_hat);
    let p_c = if m_amp > 0.0 {
        (o_x * m_c + o_y * m_s) / m_amp
    } else {
        0.0
    };
    // The moments of v_e and v_e² over the turn.
    let int_v = 2.0 * std::f64::consts::PI * a0 + b0 * i0;
    let int_v2 = 2.0 * std::f64::consts::PI * a0 * a0 + 2.0 * a0 * b0 * i0 + b0 * b0 * i0p;
    let int_or_v = b0 * p_c * ic;
    let int_or_v2 = 2.0 * a0 * b0 * p_c * ic + b0 * b0 * p_c * icp;
    // The trim must sit strictly on one side of the rim: the Möbius
    // is monotone in cosθ, so its extremes are at cosθ = ±1 — the
    // interval [v_lo, v_hi] must not contain or touch v_rim.
    let v_minus = a0 + b0 / (kappa + m_tilde);
    let v_plus = a0 + b0 / (kappa - m_tilde);
    let (v_lo, v_hi) = (v_minus.min(v_plus), v_minus.max(v_plus));
    if !(v_rim < v_lo || v_rim > v_hi) {
        return Err(format!(
            "face {i}: the elliptical trim is not strictly on one side of the circle \
             rim — a degenerate or self-overlapping strip"
        ));
    }
    let side = if v_lo > v_rim { 1.0 } else { -1.0 };
    // Area = ∫∫ w du dv over the region (w = r1 + v·sinφ).
    let area = side
        * (r1 * (int_v - 2.0 * std::f64::consts::PI * v_rim)
            + s * (int_v2 - 2.0 * std::f64::consts::PI * v_rim * v_rim) / 2.0);
    // Flux = σ∫∫ p·n̂ dA with p·n̂ = cosφ·(o·r̂(u)) − sinφ·(o·axis) +
    // r1·cosφ (constant along the rulings — every tangent plane
    // passes through the apex).
    let oz = o.dot(&axis);
    let coupled = r1 * int_or_v + (s / 2.0) * int_or_v2;
    let flat = r1 * (int_v - 2.0 * std::f64::consts::PI * v_rim)
        + s * (int_v2 - 2.0 * std::f64::consts::PI * v_rim * v_rim) / 2.0;
    let flux = sigma * side * (c * coupled + (-s * oz + r1 * c) * flat);
    Ok((area, flux))
}

// ---------------------------------------------------------------------------
// torus faces — the full torus
// ---------------------------------------------------------------------------

fn torus_face(
    ctx: FaceRef<'_>,
    c: Vector3,
    r_major: f64,
    r_minor: f64,
) -> Result<(f64, f64), String> {
    let FaceRef {
        brep,
        face,
        i: _,
        sigma,
        tol,
    } = ctx;
    let pi = std::f64::consts::PI;
    let on_torus = |p: Vector3| -> bool {
        let q = v3(p.x - c.x, p.y - c.y, p.z - c.z);
        let rho = (q.x * q.x + q.y * q.y).sqrt();
        let z = q.z;
        ((rho - r_major) * (rho - r_major) + z * z - r_minor * r_minor).abs() <= tol * r_minor
    };
    let seam_ok = face.wires.iter().all(|wire| {
        wire.edges.iter().all(|we| {
            let edge = &brep.edges[we.edge];
            if brep.is_degenerate(we.edge) {
                return true;
            }
            // Used twice within the face, once each way, and lying
            // on the torus.
            let uses: Vec<&WireEdge> = face
                .wires
                .iter()
                .flat_map(|w| &w.edges)
                .filter(|u| u.edge == we.edge)
                .collect();
            if uses.len() != 2 || uses[0].forward == uses[1].forward {
                return false;
            }
            let _ = edge;
            [0.0, 1.5, 3.0, 4.5]
                .iter()
                .all(|&t| on_torus(edge.curve.point(t)))
        })
    });
    if face.wires.is_empty() || (seam_ok && face.wires.iter().any(|w| !w.edges.is_empty())) {
        let area = 4.0 * pi * pi * r_major * r_minor;
        let flux = sigma * 6.0 * pi * pi * r_major * r_minor * r_minor;
        Ok((area, flux))
    } else {
        torus_band(ctx, c, r_major, r_minor)
    }
}

/// A trimmed torus band — the fillet-class face: two closed rim
/// circles coaxial with the torus and lying ON it (each at its own
/// `v`), the rest seam glue. The area and the `p·n̂` flux integrate
/// in closed form over `[v0, v1] × [0, 2π]`:
///
/// * `|xu × xv| = r·(R + r·cos v)` — the area element;
/// * `x·n̂ = R·cos v + r` — the position–normal product (the outward
///   normal of the natural parameterization).
///
/// `v = 0..2π` reproduces the full torus (the check on the existing
/// path); a band is any subinterval with strictly increasing `v`.
fn torus_band(
    ctx: FaceRef<'_>,
    c: Vector3,
    r_major: f64,
    r_minor: f64,
) -> Result<(f64, f64), String> {
    let FaceRef {
        brep,
        face,
        i,
        sigma,
        tol,
    } = ctx;
    let axis = match &ctx.face.surface {
        Surface::Torus { axis, .. } => *axis,
        _ => unreachable!("checked by the caller"),
    };
    let on_torus = |p: Vector3| -> bool {
        let q = v3(p.x - c.x, p.y - c.y, p.z - c.z);
        let z = q.dot(&axis);
        let radial = v3(q.x - axis.x * z, q.y - axis.y * z, q.z - axis.z * z);
        let rho = (radial.x * radial.x + radial.y * radial.y).sqrt();
        ((rho - r_major) * (rho - r_major) + z * z - r_minor * r_minor).abs() <= tol * r_minor
    };
    let mut vs: Vec<f64> = Vec::new();
    for wire in &face.wires {
        for we in &wire.edges {
            let edge = &brep.edges[we.edge];
            match &edge.curve {
                Curve::Circle { center, radius, .. } if edge.start == edge.end => {
                    // The rim circle: coaxial (its center on the
                    // axis), at height `z` along it, with its own
                    // radius `rho` — on the torus exactly when
                    // (rho − R)² + z² = r².
                    let off = v3(center.x - c.x, center.y - c.y, center.z - c.z);
                    let z = off.dot(&axis);
                    let radial = v3(off.x - axis.x * z, off.y - axis.y * z, off.z - axis.z * z);
                    if radial.length() > tol {
                        return Err(format!(
                            "face {i}: a torus band rim not coaxial with the surface"
                        ));
                    }
                    let rho = *radius;
                    if ((rho - r_major) * (rho - r_major) + z * z - r_minor * r_minor).abs()
                        > tol * r_minor
                    {
                        return Err(format!(
                            "face {i}: a torus band rim not on the surface ((rho − R)² + z² \
                             ≠ r²)"
                        ));
                    }
                    let v = z.atan2(rho - r_major);
                    vs.push(v);
                }
                _ if on_torus(edge.curve.point(edge.range.0))
                    && on_torus(edge.curve.point((edge.range.0 + edge.range.1) / 2.0))
                    && on_torus(edge.curve.point(edge.range.1)) =>
                {
                    // Seam glue: any curve whose sampled points lie on
                    // the surface (the pinned NURBS seam arcs of the
                    // OCCT reader included).
                }
                _ => {
                    return Err(format!(
                        "face {i}: a torus face with an unrecognized trim ({} edges) — only \
                         the full torus and two-rim bands have a closed form here",
                        edge.curve.kind_name()
                    ))
                }
            }
        }
    }
    if vs.len() != 2 {
        return Err(format!(
            "face {i}: a torus band needs exactly two rim circles, found {}",
            vs.len()
        ));
    }
    let (v0, v1) = if vs[0] <= vs[1] {
        (vs[0], vs[1])
    } else {
        (vs[1], vs[0])
    };
    if v1 - v0 <= tol {
        return Err(format!("face {i}: a degenerate (zero-sweep) torus band"));
    }
    if v1 - v0 > 2.0 * std::f64::consts::PI + tol {
        return Err(format!(
            "face {i}: a torus band sweeping more than a full turn"
        ));
    }
    let pi = std::f64::consts::PI;
    let r = r_minor;
    let big_r = r_major;
    let dv = v1 - v0;
    let dsin = v1.sin() - v0.sin();
    let dsin2 = (2.0 * v1).sin() - (2.0 * v0).sin();
    let dsin_sq = v1.sin() * v1.sin() - v0.sin() * v0.sin();
    // Area: ∫∫ r(R + r·cos v) = 2π·r·(R·Δv + r·Δsin).
    let area = 2.0 * pi * r * (big_r * dv + r * dsin);
    // Flux: ∫∫ (x·n̂)·r(R + r·cos v) with x·n̂ = R·cos v + r + c·n̂
    // and c·n̂ = cos v·(c·ring u) + sin v·(c·â) — the ring term dies
    // over the full turn, the axial one stays:
    // = 2π·r·[R²·Δsin + Rr·(Δv/2 + Δsin2/4) + Rr·Δv + r²·Δsin
    //          + (c·â)·(R·(cos v0 − cos v1) + (r/2)·Δsin²)].
    let c_axis = c.dot(&axis);
    let flux = sigma
        * 2.0
        * pi
        * r
        * (big_r * big_r * dsin
            + big_r * r * (dv / 2.0 + dsin2 / 4.0)
            + big_r * r * dv
            + r * r * dsin
            + c_axis * (big_r * (v0.cos() - v1.cos()) + r * dsin_sq / 2.0));
    Ok((area, flux))
}
