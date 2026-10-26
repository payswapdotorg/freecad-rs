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
            return Err(format!(
                "face {i}: a sphere face with an unrecognized trim — the cap/complement \
                 reading needs a single section circle"
            ))
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

// ---------------------------------------------------------------------------
// cylinder faces — the two-circle strip
// ---------------------------------------------------------------------------

/// The two section-circle rims of a strip face: every closed circle
/// edge used by the face, checked coaxial with `axis` and — when
/// `radius` is `Some` — of the surface's radius (the cone passes
/// `None` and checks against its own radius function instead).
/// Returns the rim circles as `(center, radius)` pairs. Line edges
/// are seam glue (no region semantics); anything else is an
/// unrecognized rim.
fn strip_rims(
    ctx: &FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: Option<f64>,
    allow_degenerate: bool,
) -> Result<Vec<(Vector3, f64)>, String> {
    let FaceRef {
        brep, face, i, tol, ..
    } = *ctx;
    let mut rims: Vec<(Vector3, f64)> = Vec::new();
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
                Curve::Line { .. } => {} // seam glue — no region semantics
                _ => {
                    return Err(format!(
                        "face {i}: an unrecognized rim ({} edges are not section circles \
                         or seam lines) — e.g. an oblique elliptical cut is not yet \
                         composed (K3c)",
                        brep.edges[we.edge].curve.kind_name()
                    ))
                }
            }
        }
    }
    if rims.len() != 2 {
        return Err(format!(
            "face {i}: a cylinder/cone face needs exactly two section-circle rims, found {}",
            rims.len()
        ));
    }
    Ok(rims)
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
    let rims = strip_rims(&ctx, o, axis, Some(radius), false)?;
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
    let rims = strip_rims(&ctx, o, axis, None, true)?;
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
        i,
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
        Err(format!(
            "face {i}: a torus face with an unrecognized trim — only the full torus has a \
             closed form here"
        ))
    }
}
