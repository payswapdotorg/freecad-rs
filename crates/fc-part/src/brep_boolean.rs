//! K3c — exact B-rep booleans composing the K3a sections.
//!
//! First slice: the **sphere×sphere family**. Two secant spheres'
//! boundary consists of the radical-plane circle (the K3a
//! construction, reused bit-identically) separating each sphere into
//! a cap (toward the other center) and its complement. The three
//! booleans are the four possible face choices:
//!
//! * **Common** (the lens): both caps.
//! * **Cut** (the crescent): the tool's cap + the target's
//!   complement.
//! * **Fuse** (the peanut): both complements.
//!
//! Orientation convention (the OCCT-style contract, documented here
//! as part of the model): a face with wires keeps the region on the
//! **left of the outer wire's traversal viewed from the face's
//! OUTWARD normal** (away from the solid; the K1 sphere's `∂u×∂v` is
//! outward, so faces whose solid lies inside their sphere use it
//! directly). K3c-1 believed the wire flag alone sufficed — it does
//! not: Cut and Fuse of a secant pair emitted byte-identical face
//! lists (the crescent's cut wall and the peanut's complement face
//! share surface, wire, and flag, and differ only in which side of
//! the surface the solid lies on). K3c-2 therefore carries the
//! orientation explicitly on [`TopoFace::reversed`] — what OCCT
//! calls `TopoDS_REVERSED` — and the wire flags are set against the
//! OUTWARD normal, `reversed` telling the reader which normal that
//! is. The two faces of any outcome still traverse the shared
//! circle in opposite directions — the closed-shell edge contract
//! `validate()` checks.
//!
//! Honest refusals, by name:
//! * a **contained cut** leaves an internal void — the K1 model has
//!   no face-orientation flag for untrimmed periodic faces yet;
//! * a **disjoint or tangent fuse** is a multi-shell compound or a
//!   point-junction — not a single closed shell;
//! * a **halfspace fuse** (`sphere_plane_boolean`) is an unbounded
//!   solid.
//!
//! Non-secant but representable configurations pass through: the
//! whole-sphere generators (translated to their centers) and empty
//! results.
//!
//! The plane cuts (K3c-2): `sphere_plane_boolean` composes the K3a
//! plane×sphere section circle into a cap solid — a spherical patch
//! and a planar disk sharing the circle (Common keeps the +normal
//! cap, Cut the −normal one). The disk's plane frame is the
//! circle's own (`x_dir` = the circle's `ref_dir`), so its UV box is
//! exactly `[−ρ, ρ]²`. Both faces are naturally oriented — the cap
//! solid lies inside its sphere and on the inner side of its disk.

use fc_base::Vector3;

use crate::intersect::{intersect_surfaces, SurfaceIntersection};
use crate::topology::{
    sphere_topology, Curve, ExactBRep, Surface, TopoEdge, TopoFace, TopoVertex, TopoWire, WireEdge,
};

/// The boolean operation. `Cut` is `a − b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Common,
    Cut,
    Fuse,
}

/// What a boolean produced.
#[derive(Debug, Clone, PartialEq)]
pub enum BooleanOutcomeKind {
    /// A closed solid (validated at construction).
    Solid(ExactBRep),
    /// The result is the empty set.
    Empty,
    /// The configuration's result is not representable in the K1
    /// model — refused with the geometry named, never approximated.
    Refused { why: String },
}

/// The outcome of a B-rep boolean: a solid, the empty set, or an
/// honest refusal.
#[derive(Debug, Clone, PartialEq)]
pub struct BooleanOutcome {
    pub kind: BooleanOutcomeKind,
}

fn refused(why: &str) -> BooleanOutcome {
    BooleanOutcome {
        kind: BooleanOutcomeKind::Refused {
            why: why.to_string(),
        },
    }
}

fn empty() -> BooleanOutcome {
    BooleanOutcome {
        kind: BooleanOutcomeKind::Empty,
    }
}

/// The K1 sphere generator, translated to `center`.
fn sphere_brep_at(center: Vector3, radius: f64) -> ExactBRep {
    let mut b = sphere_topology(radius);
    let shift = |p: &mut Vector3| {
        *p = v3(p.x + center.x, p.y + center.y, p.z + center.z);
    };
    for v in &mut b.vertices {
        shift(&mut v.point);
    }
    for e in &mut b.edges {
        if let Curve::Circle { ref mut center, .. } = e.curve {
            shift(center);
        }
    }
    for f in &mut b.faces {
        if let Surface::Sphere { ref mut center, .. } = f.surface {
            shift(center);
        }
    }
    b
}

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// Exact helpers re-used from the K3a classification tier.
use crate::intersect::{
    dot_terms, exact_sum_rounded, exact_sum_sign, length_squared_terms, two_prod,
};

/// The sign of `d² − (r1 + r2)²` exactly (both squares exact).
fn cmp_exterior(c1: Vector3, c2: Vector3, r1: f64, r2: f64) -> i8 {
    let mut terms = length_squared_terms(v3(c2.x - c1.x, c2.y - c1.y, c2.z - c1.z));
    let (sh, se) = two_prod(r1 + r2, r1 + r2);
    terms.push(-sh);
    terms.push(-se);
    exact_sum_sign(&terms)
}

/// The sign of `d² − (r1 − r2)²` exactly.
fn cmp_interior(c1: Vector3, c2: Vector3, r1: f64, r2: f64) -> i8 {
    let mut terms = length_squared_terms(v3(c2.x - c1.x, c2.y - c1.y, c2.z - c1.z));
    let (ih, ie) = two_prod(r1 - r2, r1 - r2);
    terms.push(-ih);
    terms.push(-ie);
    exact_sum_sign(&terms)
}

/// The exact B-rep boolean of two spheres at arbitrary centers.
///
/// Secant configurations compose the K3a radical-plane circle with
/// the cap/complement face selection (see the module doc); the
/// non-secant classifications are exact (`d²` vs `(r1±r2)²` through
/// expansion arithmetic) and their outcomes honest.
pub fn sphere_sphere_boolean(
    op: BooleanOp,
    ca: Vector3,
    ra: f64,
    cb: Vector3,
    rb: f64,
) -> Result<BooleanOutcome, String> {
    let dvec = v3(cb.x - ca.x, cb.y - ca.y, cb.z - ca.z);
    let d2_terms = length_squared_terms(dvec);
    let concentric = exact_sum_sign(&d2_terms) == 0;

    // ---- non-secant classifications --------------------------------
    let contained = |inner_r: f64, outer_r: f64| -> bool {
        // d ≤ |outer − inner| — the smaller sphere entirely inside
        // (internal tangency at the boundary included): the exact
        // sign of d² − (outer − inner)² is ≤ 0.
        let sign = cmp_interior(ca, cb, outer_r, inner_r);
        sign <= 0
    };

    if concentric {
        if ra == rb {
            // The same sphere.
            return Ok(match op {
                BooleanOp::Common | BooleanOp::Fuse => BooleanOutcome {
                    kind: BooleanOutcomeKind::Solid(sphere_brep_at(ca, ra)),
                },
                BooleanOp::Cut => empty(),
            });
        }
        let (r_out, c_out) = if ra > rb { (ra, ca) } else { (rb, cb) };
        let (r_in, c_in) = if ra > rb { (rb, cb) } else { (ra, ca) };
        return Ok(match op {
            BooleanOp::Common => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(c_in, r_in)),
            },
            BooleanOp::Fuse => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(c_out, r_out)),
            },
            BooleanOp::Cut => {
                let _ = (r_in, c_in);
                refused(
                    "fully contained cut — the internal void needs a face-orientation \
                     flag the K1 model does not carry yet",
                )
            }
        });
    }

    let ext = cmp_exterior(ca, cb, ra, rb);
    if ext > 0 {
        // Disjoint.
        return Ok(match op {
            BooleanOp::Common => empty(),
            BooleanOp::Cut => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(ca, ra)),
            },
            BooleanOp::Fuse => {
                refused("disjoint fuse — a two-shell compound, not a single closed shell")
            }
        });
    }
    if ext == 0 {
        // Externally tangent: the common is a point (not a solid).
        return Ok(match op {
            BooleanOp::Common => empty(),
            BooleanOp::Cut => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(ca, ra)),
            },
            BooleanOp::Fuse => {
                refused("tangent fuse — the shared point is not an edge-manifold junction")
            }
        });
    }
    if ra != rb && contained(ra.max(rb), ra.min(rb)) {
        // One sphere entirely inside the other (internal tangency
        // included: the void touches the boundary at one point).
        let (r_out, c_out) = if ra > rb { (ra, ca) } else { (rb, cb) };
        let (r_in, c_in) = if ra > rb { (rb, cb) } else { (ra, ca) };
        return Ok(match op {
            BooleanOp::Common => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(c_in, r_in)),
            },
            BooleanOp::Fuse => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(c_out, r_out)),
            },
            BooleanOp::Cut => refused(
                "contained cut (internal tangency included) — the internal void \
                 needs a face-orientation flag the K1 model does not carry yet",
            ),
        });
    }

    // ---- the secant construction ------------------------------------
    let d = exact_sum_rounded(&d2_terms).sqrt();
    let n_hat = v3(dvec.x / d, dvec.y / d, dvec.z / d);

    // The radical-plane circle, exactly as K3a constructs it.
    let sa = Surface::Sphere {
        center: ca,
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius: ra,
    };
    let sb = Surface::Sphere {
        center: cb,
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius: rb,
    };
    let circle = match intersect_surfaces(&sa, &sb) {
        SurfaceIntersection::Curves(cs) if cs.len() == 1 => match &cs[0] {
            Curve::Circle { .. } => cs[0].clone(),
            other => return Err(format!("K3a returned a non-circle: {other:?}")),
        },
        other => return Err(format!("K3a radical circle missing: {other:?}")),
    };
    let circle_axis = match &circle {
        Curve::Circle { axis, .. } => *axis,
        _ => unreachable!(),
    };
    // Does the stored parameterization run CCW viewed from the A→B
    // side? (K3a canonicalizes operand order; the cap orientation
    // must follow the geometry, not the canonical order.)
    let sigma = circle_axis.dot(&n_hat);
    if sigma == 0.0 {
        return Err("the radical circle's axis is perpendicular to the center line".into());
    }
    let a_forward = sigma > 0.0;

    // One vertex on the circle; one closed edge (start == end).
    let p0 = circle_point(&circle, 0.0);
    let vertex = TopoVertex { point: p0 };
    let edge = TopoEdge {
        curve: circle,
        range: (0.0, std::f64::consts::TAU),
        start: 0,
        end: 0,
    };

    let face_of = |surface: Surface, forward: bool, reversed: bool| TopoFace {
        surface,
        uv_bounds: [
            0.0,
            std::f64::consts::TAU,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        ],
        wires: vec![TopoWire {
            edges: vec![WireEdge { edge: 0, forward }],
        }],
        reversed,
    };
    let surface_a = Surface::Sphere {
        center: ca,
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius: ra,
    };
    let surface_b = Surface::Sphere {
        center: cb,
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius: rb,
    };

    // Wire flags (the module-doc orientation convention). The kept
    // region lies on the LEFT of the traversal viewed from the face's
    // OUTWARD normal (away from the solid). Four face-uses exist; the
    // tool's cap in a cut is the one REVERSED face — the solid lies
    // on the OUTSIDE of the tool sphere there, so the outward normal
    // is the natural one negated and the traversal is set against
    // THAT (the `reversed` flag is what makes the pair readable;
    // before it, Cut and Fuse were byte-identical — see K3c-2).
    let cap_a = a_forward; // A's cap, solid inside A
    let cap_b = !a_forward; // B's cap, solid inside B
    let hole_a = !cap_a; // A's complement, solid inside A
    let cut_wall_b = !cap_b; // B's cap as a cut wall: solid OUTSIDE B

    let faces = match op {
        BooleanOp::Common => vec![
            face_of(surface_a, cap_a, false),
            face_of(surface_b, cap_b, false),
        ],
        BooleanOp::Cut => vec![
            face_of(surface_a, hole_a, false),
            face_of(surface_b, cut_wall_b, true),
        ],
        BooleanOp::Fuse => vec![
            face_of(surface_a, hole_a, false),
            face_of(surface_b, !cap_b, false),
        ],
    };

    let brep = ExactBRep {
        vertices: vec![vertex],
        edges: vec![edge],
        faces,
    };
    brep.validate()
        .map_err(|e| format!("the constructed solid failed validation: {e}"))?;
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

/// A point on a circle at parameter `t` (the documented
/// parameterization, evaluated independently of the topology module's
/// own `point` for construction-local clarity).
fn circle_point(c: &Curve, t: f64) -> Vector3 {
    match c {
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            let second = axis.cross(ref_dir);
            let (s, co) = t.sin_cos();
            v3(
                center.x + radius * (co * ref_dir.x + s * second.x),
                center.y + radius * (co * ref_dir.y + s * second.y),
                center.z + radius * (co * ref_dir.z + s * second.z),
            )
        }
        other => panic!("circle_point on a non-circle {other:?}"),
    }
}

/// The exact B-rep boolean of a sphere with the halfspace tool
/// `{x : n·(x − p) ≥ 0}` (`n` must be unit — the K1 frame contract).
///
/// * `Common` keeps the `+n` cap: the spherical patch beyond the
///   plane plus the planar disk the section circle cuts out — a
///   two-face solid sharing the K3a plane×sphere circle.
/// * `Cut` keeps the `−n` cap (the sphere minus the tool).
/// * `Fuse` is refused by name: the union with a halfspace is an
///   unbounded solid.
///
/// The secant/tangent/disjoint classification is exact (the sign of
/// `d − r` and `d + r` through expansion arithmetic, `d` the signed
/// distance of the sphere's center from the plane). Tangency is
/// honest at measure zero: the tangent point is not a solid, so
/// `Common` there is empty and `Cut` is the whole sphere.
pub fn sphere_plane_boolean(
    op: BooleanOp,
    center: Vector3,
    radius: f64,
    plane_point: Vector3,
    plane_normal: Vector3,
) -> Result<BooleanOutcome, String> {
    let n_len = plane_normal.length();
    if (n_len - 1.0).abs() > 1e-9 {
        return Err(format!(
            "the halfspace normal must be unit (the K1 frame contract), |n| = {n_len}"
        ));
    }
    let whole_sphere = || BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(sphere_brep_at(center, radius)),
    };
    let unbounded = || {
        refused(
            "fusing a halfspace — the result is an unbounded solid, not representable in \
             the K1 model",
        )
    };

    // The exact classification: sign(d − r) and sign(d + r), with d
    // = (c − p)·n accumulated in expansion arithmetic.
    let dv = v3(
        center.x - plane_point.x,
        center.y - plane_point.y,
        center.z - plane_point.z,
    );
    let mut minus_terms = dot_terms(dv, plane_normal);
    minus_terms.push(-radius);
    let sign_minus = exact_sum_sign(&minus_terms);
    let mut plus_terms = dot_terms(dv, plane_normal);
    plus_terms.push(radius);
    let sign_plus = exact_sum_sign(&plus_terms);

    if sign_minus == 0 {
        // d == r: tangent with the sphere wholly on the tool side —
        // the closed tool contains every point of it.
        return Ok(match op {
            BooleanOp::Common => whole_sphere(),
            BooleanOp::Cut => empty(),
            BooleanOp::Fuse => unbounded(),
        });
    }
    if sign_plus == 0 {
        // d == −r: tangent with the sphere wholly outside the tool
        // (the tangent point lies on the tool boundary and is not a
        // solid).
        return Ok(match op {
            BooleanOp::Common => empty(),
            BooleanOp::Cut => whole_sphere(),
            BooleanOp::Fuse => unbounded(),
        });
    }
    if sign_minus > 0 {
        // d > r: disjoint, the sphere entirely inside the tool's
        // halfspace.
        return Ok(match op {
            BooleanOp::Common => whole_sphere(),
            BooleanOp::Cut => empty(),
            BooleanOp::Fuse => unbounded(),
        });
    }
    if sign_plus < 0 {
        // d < −r: disjoint, the sphere entirely outside the tool.
        return Ok(match op {
            BooleanOp::Common => empty(),
            BooleanOp::Cut => whole_sphere(),
            BooleanOp::Fuse => unbounded(),
        });
    }

    // Secant: compose the K3a plane×sphere section circle,
    // bit-identical.
    let sphere_surface = Surface::Sphere {
        center,
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius,
    };
    let plane_surface = Surface::Plane {
        origin: plane_point,
        normal: plane_normal,
        x_dir: crate::intersect::any_perpendicular(plane_normal),
    };
    let circle = match intersect_surfaces(&sphere_surface, &plane_surface) {
        SurfaceIntersection::Curves(cs) if cs.len() == 1 => cs[0].clone(),
        other => return Err(format!("the K3a plane×sphere circle is missing: {other:?}")),
    };
    let (circle_center, circle_axis, circle_ref, rho) = match &circle {
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => (*center, *axis, *ref_dir, *radius),
        other => return Err(format!("a non-circle section: {other:?}")),
    };

    // The kept cap: +n for Common, −n for Cut.
    let keep_sign = match op {
        BooleanOp::Common => 1.0,
        BooleanOp::Cut => -1.0,
        BooleanOp::Fuse => return Ok(unbounded()),
    };
    let k_hat = v3(
        plane_normal.x * keep_sign,
        plane_normal.y * keep_sign,
        plane_normal.z * keep_sign,
    );

    // One vertex on the circle; one closed edge.
    let vertex = TopoVertex {
        point: circle_point(&circle, 0.0),
    };
    let edge = TopoEdge {
        curve: circle,
        range: (0.0, std::f64::consts::TAU),
        start: 0,
        end: 0,
    };

    // The spherical patch keeps the cap on the k̂ side: forward iff
    // the stored circle axis points that way (the kept-on-the-left
    // convention, σ = +1 — the cap solid lies inside its sphere).
    let patch_forward = circle_axis.dot(&k_hat) > 0.0;
    // The disk's outward normal points away from the solid: −k̂. Its
    // frame is the circle's own, so the UV box is exactly [−ρ, ρ]².
    let disk_normal = v3(-k_hat.x, -k_hat.y, -k_hat.z);
    let disk_forward = !patch_forward;

    let faces = vec![
        TopoFace {
            surface: sphere_surface,
            uv_bounds: [
                0.0,
                std::f64::consts::TAU,
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            ],
            wires: vec![TopoWire {
                edges: vec![WireEdge {
                    edge: 0,
                    forward: patch_forward,
                }],
            }],
            reversed: false,
        },
        TopoFace {
            surface: Surface::Plane {
                origin: circle_center,
                normal: disk_normal,
                x_dir: circle_ref,
            },
            uv_bounds: [-rho, rho, -rho, rho],
            wires: vec![TopoWire {
                edges: vec![WireEdge {
                    edge: 0,
                    forward: disk_forward,
                }],
            }],
            reversed: false,
        },
    ];

    let brep = ExactBRep {
        vertices: vec![vertex],
        edges: vec![edge],
        faces,
    };
    brep.validate()
        .map_err(|e| format!("the constructed cap solid failed validation: {e}"))?;
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}
