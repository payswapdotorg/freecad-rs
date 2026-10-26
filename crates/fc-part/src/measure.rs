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

/// The rims a strip face carries: its section circles (checked
/// coaxial with `axis` and — when `radius` is `Some` — of the
/// surface's radius; the cone passes `None` and checks against its
/// own radius function instead) and at most one ellipse (the
/// composed oblique section). Seam edges are glue with no region
/// semantics: a `Line`, or the degree-1 two-point B-spline the
/// composed strips carry (its de Boor endpoints are exact — see
/// `brep_boolean`). The count checks live at the call sites (the
/// ellipse-trimmed strip has ONE circle rim).
fn strip_rims(
    ctx: &FaceRef<'_>,
    o: Vector3,
    axis: Vector3,
    radius: Option<f64>,
    allow_degenerate: bool,
) -> Result<(Vec<CircleRim>, Option<StripEllipse>), String> {
    let FaceRef {
        brep, face, i, tol, ..
    } = *ctx;
    let mut rims: Vec<(Vector3, f64)> = Vec::new();
    let mut ellipse: Option<StripEllipse> = None;
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
                         ellipses or seam glue)",
                        brep.edges[we.edge].curve.kind_name()
                    ))
                }
            }
        }
    }
    Ok((rims, ellipse))
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
    let (rims, ellipse) = strip_rims(&ctx, o, axis, Some(radius), false)?;
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
    let (rims, ellipse) = strip_rims(&ctx, o, axis, None, true)?;
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
