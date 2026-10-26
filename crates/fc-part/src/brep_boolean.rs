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
    any_perpendicular, dot_terms, exact_sum_rounded, exact_sum_sign, length_squared_terms,
    two_prod, two_sum, PARALLEL_BAND,
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

// ---------------------------------------------------------------------------
// K3c-3 — the oblique plane cuts: cylinder×halfspace and cone×halfspace
// ---------------------------------------------------------------------------
//
// The plane cuts a *finite* primitive; the tool is the halfspace
// `{x : n·(x − p) ≥ 0}`. A clean section — the plane crosses the open
// axis segment between the end references and strictly misses every
// end disk — is a single closed curve on the side surface: the K3a
// ellipse (oblique) or the axis-meet circle (perpendicular), reused
// bit-identically as the shared edge between the side strip and the
// new planar face. The kept piece's boundary:
//
// * the **side strip** between the section curve and the kept rim,
//   carried as ONE wire of four legs — seam, rim, seam back, curve —
//   mirroring the K1 mantle. The seam is the periodicity glue the
//   Euler count needs; it runs along a generator (both endpoints sit
//   at the same parameter angle) and is stored as a degree-1 B-spline
//   whose de Boor evaluation returns the endpoints exactly, which a
//   `Line`'s origin+direction arithmetic cannot guarantee at arbitrary
//   placements (`fl(a + fl(b − a)) ≠ b` in general).
// * the **kept cap** where the piece reaches an original rim (none for
//   a plain cone's apex piece — the apex is a degenerate rim), and
// * the **section face**: the elliptical or circular disk on the
//   cutting plane.
//
// The orientation contract (K3c-2): no face is `reversed` — the solid
// lies inside the side surface and on the inner side of every plane
// face, so the stored normals ARE the outward ones. The wire flags
// follow kept-on-the-left against the outward normal, which makes the
// two faces of every shared edge traverse it in opposite directions
// (the closed-shell orientation contract); the section-curve leg of
// the strip comes out `(op == Common)` and the section disk's own leg
// `(op == Cut)` — always opposed.
//
// The classification tier is exact throughout (the `exact.rs`
// discipline): the end-center g-values and the axis tilt β are
// expansion sums; the disk-miss tests compare `g²` against
// `r²·(1 − β²)` through EFT products, so one-ulp-off tangencies
// classify honestly. Tangent-to-a-rim sections (the ellipse touching
// a cap circle) refuse by name — a non-manifold vertex junction, the
// same measure-zero honesty as the tangent fuse refusal.

/// The seam edge — the periodicity glue of a composed strip face — as
/// a degree-1 B-spline through the two rim vertices: the de Boor
/// evaluation returns the control points exactly at both ends (a
/// `Line`'s `origin + direction` cannot, in general). Both endpoints
/// sit on the same generator of the surface, so the seam lies on it
/// to machine scale; it carries no region semantics.
fn seam_between(a: Vector3, b: Vector3, ia: usize, ib: usize) -> TopoEdge {
    TopoEdge {
        curve: Curve::Bspline {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![a, b],
            weights: None,
        },
        range: (0.0, 1.0),
        start: ia,
        end: ib,
    }
}

fn unbounded_fuse() -> BooleanOutcome {
    refused(
        "fusing a halfspace — the result is an unbounded solid, not representable in \
         the K1 model",
    )
}

fn require_unit(v: Vector3, what: &str) -> Result<(), String> {
    let l = v.length();
    if (l - 1.0).abs() > 1e-9 {
        return Err(format!(
            "the {what} must be unit (the K1 frame contract), |v| = {l}"
        ));
    }
    Ok(())
}

/// The exact sign of `s² − r²·(1 − β²)` where `s` is an exact
/// expansion (the plane's signed distance of a disk's center) and `r`
/// is the disk radius, `β = n·axis` with both unit. Positive: the
/// plane strictly misses the disk; zero: tangent to its rim;
/// negative: the disk is cut. `1 − β²` is exact (`two_sum` over the
/// `two_prod` halves) and every product below is an EFT product —
/// the tangency classification is exact, not tolerant.
fn cmp_disk_miss(s_terms: &[f64], r: f64, beta: f64) -> i8 {
    // s² : every pairwise product of the expansion's terms.
    let mut terms: Vec<f64> = Vec::with_capacity(s_terms.len() * s_terms.len() + 12);
    for &a in s_terms {
        for &b in s_terms {
            let (h, e) = two_prod(a, b);
            terms.push(h);
            terms.push(e);
        }
    }
    // r²·(1 − β²) : r² exact, 1 − β² exact as (oh + oe − be).
    let (rh, re) = two_prod(r, r);
    let (bh, be) = two_prod(beta, beta);
    let (oh, oe) = two_sum(1.0, -bh);
    for x in [oh, oe, -be] {
        for y in [rh, re] {
            let (h, e) = two_prod(y, x);
            terms.push(-h);
            terms.push(-e);
        }
    }
    exact_sum_sign(&terms)
}

/// One end disk's extremes against the plane: the exact signs of
/// `min g = g_c − r·sinγ` and `max g = g_c + r·sinγ` over the disk
/// (`g_c` the center's g; the oblique branch has `sinγ > 0`).
fn disk_extremes(g_terms: &[f64], r: f64, beta: f64) -> (i8, i8) {
    let g_sign = exact_sum_sign(g_terms);
    if g_sign > 0 {
        (cmp_disk_miss(g_terms, r, beta), 1)
    } else if g_sign < 0 {
        (-1, -cmp_disk_miss(g_terms, r, beta))
    } else {
        // The center on the plane with sinγ > 0: straddling.
        (-1, 1)
    }
}

/// The pass-through outcome when the whole solid lies on one side
/// (`side > 0`: inside the tool).
fn whole_or_empty(op: BooleanOp, side: i8, whole: ExactBRep) -> BooleanOutcome {
    let in_tool = side > 0;
    let kind = match (op, in_tool) {
        (BooleanOp::Common, true) | (BooleanOp::Cut, false) => BooleanOutcomeKind::Solid(whole),
        _ => BooleanOutcomeKind::Empty,
    };
    BooleanOutcome { kind }
}

/// A closed cylinder piece between two coaxial rim circles at heights
/// `v0 < v1` along `axis`, capped at both ends — the pass-through
/// whole. The seam runs along the generator at parameter angle 0
/// (both rims share `ref_dir`).
fn cylinder_piece(origin: Vector3, axis: Vector3, radius: f64, v0: f64, v1: f64) -> ExactBRep {
    let x_hat = any_perpendicular(axis);
    let at = |v: f64| {
        v3(
            origin.x + axis.x * v,
            origin.y + axis.y * v,
            origin.z + axis.z * v,
        )
    };
    let c0 = Curve::Circle {
        center: at(v0),
        axis,
        ref_dir: x_hat,
        radius,
    };
    let c1 = Curve::Circle {
        center: at(v1),
        axis,
        ref_dir: x_hat,
        radius,
    };
    let v_lo = c0.point(0.0);
    let v_hi = c1.point(0.0);
    ExactBRep {
        vertices: vec![TopoVertex { point: v_lo }, TopoVertex { point: v_hi }],
        edges: vec![
            TopoEdge {
                curve: c0,
                range: (0.0, std::f64::consts::TAU),
                start: 0,
                end: 0,
            },
            TopoEdge {
                curve: c1,
                range: (0.0, std::f64::consts::TAU),
                start: 1,
                end: 1,
            },
            seam_between(v_lo, v_hi, 0, 1),
        ],
        faces: vec![
            // The strip: seam up, top rim, seam down, bottom rim —
            // kept-on-the-left against the outward (natural) normal.
            TopoFace {
                surface: Surface::Cylinder {
                    origin,
                    axis,
                    ref_dir: x_hat,
                    radius,
                },
                uv_bounds: [0.0, std::f64::consts::TAU, v0, v1],
                wires: vec![TopoWire {
                    edges: vec![
                        WireEdge {
                            edge: 2,
                            forward: true,
                        },
                        WireEdge {
                            edge: 1,
                            forward: false,
                        },
                        WireEdge {
                            edge: 2,
                            forward: false,
                        },
                        WireEdge {
                            edge: 0,
                            forward: true,
                        },
                    ],
                }],
                reversed: false,
            },
            // The lower cap: outward −axis.
            TopoFace {
                surface: Surface::Plane {
                    origin: at(v0),
                    normal: v3(-axis.x, -axis.y, -axis.z),
                    x_dir: x_hat,
                },
                uv_bounds: [-radius, radius, -radius, radius],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: 0,
                        forward: false,
                    }],
                }],
                reversed: false,
            },
            // The upper cap: outward +axis.
            TopoFace {
                surface: Surface::Plane {
                    origin: at(v1),
                    normal: axis,
                    x_dir: x_hat,
                },
                uv_bounds: [-radius, radius, -radius, radius],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: 1,
                        forward: true,
                    }],
                }],
                reversed: false,
            },
        ],
    }
}

/// Assemble a cut piece from its ingredients:
///
/// * `side` — the strip's surface (Cylinder or Cone);
/// * `curve` — the closed section curve (the K3a ellipse or circle,
///   reused bit-identically) with `curve_v_range` its extent in the
///   surface's v parameter;
/// * the kept rim: `rim_center`/`rim_radius` on `rim_axis`, its v
///   parameter `rim_v`, and whether it is the HIGH-v end
///   (`rim_is_base` — the cone's base side / the cylinder's top);
/// * `cap` — the kept cap plane (`None` for a plain cone's apex
///   piece: the apex is a degenerate rim, not a face);
/// * the cutting plane (`plane_point`, `plane_normal`) and
///   `keep_sign` (+1 Common, −1 Cut).
///
/// The section curve's vertex is its own parameterization's start;
/// the rim's vertex is the rim point at the section curve's frame
/// angle — the same generator, so the seam lies on the surface.
/// The kept rim of a cut piece: the original end the piece reaches
/// (the base circle, the top circle, or the degenerate apex circle of
/// a plain cone's apex piece) plus the cap face it carries, if any.
struct KeptRim {
    center: Vector3,
    radius: f64,
    /// The rim's v parameter in the side surface's own frame.
    v: f64,
    /// Is the rim the HIGH-v end (the cone's base side / the
    /// cylinder's top)? Decides the strip wire's rim-leg flag.
    is_base: bool,
    /// The cap plane (origin, outward normal); `None` for a plain
    /// cone's apex piece — the apex is a degenerate rim, not a face.
    cap: Option<(Vector3, Vector3)>,
}

/// Assemble a cut piece from its ingredients: the strip's `surface`,
/// the closed section `curve` (the K3a ellipse or circle, reused
/// bit-identically) with its v-extent, the kept `rim`, the cutting
/// plane's normal, and `keep_sign` (+1 Common, −1 Cut).
///
/// The section curve's vertex is its own parameterization's start;
/// the rim's vertex is the rim point at the section curve's frame
/// angle — the same generator, so the seam lies on the surface.
fn assemble_cut(
    surface: Surface,
    curve: Curve,
    curve_v_range: (f64, f64),
    rim: KeptRim,
    plane_normal: Vector3,
    keep_sign: f64,
) -> Result<ExactBRep, String> {
    let KeptRim {
        center: rim_center,
        radius: rim_radius,
        v: rim_v,
        is_base: rim_is_base,
        cap,
    } = rim;
    let v_curve = curve.point(0.0);
    let x_hat = any_perpendicular(curve_axis_or(&curve, plane_normal));
    let (surface_origin, axis) = match &surface {
        Surface::Cylinder { origin, axis, .. } | Surface::Cone { origin, axis, .. } => {
            (*origin, *axis)
        }
        other => {
            return Err(format!(
                "the strip surface must be a cylinder or cone: {other:?}"
            ))
        }
    };
    // The rim circle: its range starts at the section curve's frame
    // angle so both vertices share a generator.
    let theta = {
        let y_hat = axis.cross(&x_hat);
        let d = v3(
            v_curve.x - surface_origin.x,
            v_curve.y - surface_origin.y,
            v_curve.z - surface_origin.z,
        );
        d.dot(&y_hat).atan2(d.dot(&x_hat))
    };
    let rim = Curve::Circle {
        center: rim_center,
        axis,
        ref_dir: x_hat,
        radius: rim_radius,
    };
    let v_rim = rim.point(theta);

    let mut brep = ExactBRep {
        vertices: vec![TopoVertex { point: v_curve }, TopoVertex { point: v_rim }],
        edges: vec![
            TopoEdge {
                curve: curve.clone(),
                range: (0.0, std::f64::consts::TAU),
                start: 0,
                end: 0,
            },
            TopoEdge {
                curve: rim,
                range: (theta, theta + std::f64::consts::TAU),
                start: 1,
                end: 1,
            },
            seam_between(v_curve, v_rim, 0, 1),
        ],
        faces: Vec::new(),
    };

    // ---- the side strip: seam, rim, seam back, curve -----------------
    // The rim leg: the region lies between the rim and the curve —
    // +u at the low-v rim, −u at the high-v one (kept-on-the-left
    // against the outward natural normal). The curve leg: the region
    // beyond the curve for Common, under it for Cut — which the
    // case analysis in the module doc pins to exactly `(op ==
    // Common)`, keeping the shared traversal opposite the section
    // disk's own `(op == Cut)`.
    let strip_rim_forward = !rim_is_base;
    let strip_curve_forward = keep_sign > 0.0;
    let v_lo = curve_v_range.0.min(rim_v);
    let v_hi = curve_v_range.1.max(rim_v);
    brep.faces.push(TopoFace {
        surface,
        uv_bounds: [0.0, std::f64::consts::TAU, v_lo, v_hi],
        wires: vec![TopoWire {
            edges: vec![
                WireEdge {
                    edge: 2,
                    forward: true,
                },
                WireEdge {
                    edge: 1,
                    forward: strip_rim_forward,
                },
                WireEdge {
                    edge: 2,
                    forward: false,
                },
                WireEdge {
                    edge: 0,
                    forward: strip_curve_forward,
                },
            ],
        }],
        reversed: false,
    });

    // ---- the kept cap --------------------------------------------------
    if let Some((cap_origin, cap_normal)) = cap {
        let cap_forward = cap_normal.dot(&axis) > 0.0;
        brep.faces.push(TopoFace {
            surface: Surface::Plane {
                origin: cap_origin,
                normal: cap_normal,
                x_dir: x_hat,
            },
            uv_bounds: [-rim_radius, rim_radius, -rim_radius, rim_radius],
            wires: vec![TopoWire {
                edges: vec![WireEdge {
                    edge: 1,
                    forward: cap_forward,
                }],
            }],
            reversed: false,
        });
    }

    // ---- the section face on the cutting plane -------------------------
    // The outward normal points away from the kept side: −keep_sign·n.
    let out_n = v3(
        plane_normal.x * -keep_sign,
        plane_normal.y * -keep_sign,
        plane_normal.z * -keep_sign,
    );
    let curve_axis = match &curve {
        Curve::Ellipse { axis, .. } | Curve::Circle { axis, .. } => *axis,
        other => {
            return Err(format!(
                "the section curve must be an ellipse or circle: {other:?}"
            ))
        }
    };
    let (origin_c, x_dir_c, u_lo, u_hi, v_lo_c, v_hi_c) = match &curve {
        Curve::Ellipse {
            center,
            major_dir,
            major_radius,
            minor_radius,
            ..
        } => (
            *center,
            *major_dir,
            -*major_radius,
            *major_radius,
            -*minor_radius,
            *minor_radius,
        ),
        Curve::Circle {
            center,
            ref_dir,
            radius,
            ..
        } => (*center, *ref_dir, -*radius, *radius, -*radius, *radius),
        other => {
            return Err(format!(
                "the section curve must be an ellipse or circle: {other:?}"
            ))
        }
    };
    let disk_forward = out_n.dot(&curve_axis) > 0.0;
    brep.faces.push(TopoFace {
        surface: Surface::Plane {
            origin: origin_c,
            normal: out_n,
            x_dir: x_dir_c,
        },
        uv_bounds: [u_lo, u_hi, v_lo_c, v_hi_c],
        wires: vec![TopoWire {
            edges: vec![WireEdge {
                edge: 0,
                forward: disk_forward,
            }],
        }],
        reversed: false,
    });

    brep.validate()
        .map_err(|e| format!("the constructed cut solid failed validation: {e}"))?;
    Ok(brep)
}

/// A closed section curve's stored axis (the ellipse's plane normal /
/// the circle's axis), for anchoring the rim frame.
fn curve_axis_or(c: &Curve, fallback: Vector3) -> Vector3 {
    match c {
        Curve::Ellipse { axis, .. } | Curve::Circle { axis, .. } => *axis,
        _ => fallback,
    }
}

/// The exact B-rep boolean of a finite cylinder with the halfspace
/// tool `{x : n·(x − p) ≥ 0}` (`axis` and `n` must be unit — the K1
/// frame contract).
///
/// * `Common` keeps the `+n` piece, `Cut` the `−n` piece — each a
///   three-face solid: the side strip between the kept rim and the
///   section curve, the kept cap, and the section disk, sharing the
///   K3a curve bit-identically.
/// * A perpendicular plane (`n ∥ axis`) sections a circle; the pieces
///   are ordinary cylinders.
/// * A plane parallel to the axis sections generator lines — refused
///   by name when secant, an honest pass-through when missing or
///   tangent along one generator.
/// * An oblique section that clips a cap rim refuses by name (mixed
///   line/arc trims); tangency to a rim is a non-manifold junction —
///   refused at measure zero, like the tangent fuse.
/// * `Fuse` is refused by name: the union with a halfspace is
///   unbounded.
///
/// The configuration classifications are exact (expansion signs;
/// `g²` vs `r²(1 − β²)` through EFT products) — no tolerances.
pub fn cylinder_plane_boolean(
    op: BooleanOp,
    origin: Vector3,
    axis: Vector3,
    radius: f64,
    height: f64,
    plane_point: Vector3,
    plane_normal: Vector3,
) -> Result<BooleanOutcome, String> {
    require_unit(axis, "cylinder axis")?;
    require_unit(plane_normal, "halfspace normal")?;
    if radius <= 0.0 {
        return Err(format!(
            "the cylinder radius must be positive, got {radius}"
        ));
    }
    if height <= 0.0 {
        return Err(format!(
            "the cylinder height must be positive, got {height}"
        ));
    }
    if op == BooleanOp::Fuse {
        return Ok(unbounded_fuse());
    }

    let dv = v3(
        origin.x - plane_point.x,
        origin.y - plane_point.y,
        origin.z - plane_point.z,
    );
    let alpha_terms = dot_terms(dv, plane_normal);
    let alpha = exact_sum_rounded(&alpha_terms);
    let alpha_sign = exact_sum_sign(&alpha_terms);
    let beta = exact_sum_rounded(&dot_terms(axis, plane_normal));
    let (hb_h, hb_e) = two_prod(height, beta);
    let mut top_terms = alpha_terms.clone();
    top_terms.push(hb_h);
    top_terms.push(hb_e);
    let top_sign = exact_sum_sign(&top_terms);
    let sin_gamma = plane_normal.cross(&axis).length();
    let whole = || cylinder_piece(origin, axis, radius, 0.0, height);

    // The K3a branch order: perpendicular, parallel, oblique.
    if sin_gamma <= PARALLEL_BAND {
        // Plane ⊥ axis: the axis-meet circle at v* = −α/β (β ≈ ±1).
        if beta == 0.0 {
            return Err("a unit normal cannot be both parallel and perpendicular".into());
        }
        if alpha_sign == top_sign {
            // The crossing outside the open segment: the whole solid
            // one side (min g has that sign).
            let side = if beta > 0.0 { alpha_sign } else { top_sign };
            return Ok(whole_or_empty(op, side, whole()));
        }
        // Secant: compose the pieces around the K3a circle.
        let v_star = -alpha / beta;
        let x_hat = any_perpendicular(axis);
        let cyl = Surface::Cylinder {
            origin,
            axis,
            ref_dir: x_hat,
            radius,
        };
        let plane = Surface::Plane {
            origin: plane_point,
            normal: plane_normal,
            x_dir: any_perpendicular(plane_normal),
        };
        let curve = match intersect_surfaces(&cyl, &plane) {
            SurfaceIntersection::Curves(cs) if cs.len() == 1 => match &cs[0] {
                Curve::Circle { radius: rho, .. } if *rho > 0.0 => cs[0].clone(),
                other => return Err(format!("K3a returned a non-circle section: {other:?}")),
            },
            other => return Err(format!("the K3a axis-meet circle is missing: {other:?}")),
        };
        let keep_sign = if op == BooleanOp::Common { 1.0 } else { -1.0 };
        let keep_top = (beta > 0.0) == (op == BooleanOp::Common);
        let v_kept = if keep_top { height } else { 0.0 };
        let rim_center = v3(
            origin.x + axis.x * v_kept,
            origin.y + axis.y * v_kept,
            origin.z + axis.z * v_kept,
        );
        let cap_normal = if keep_top {
            axis
        } else {
            v3(-axis.x, -axis.y, -axis.z)
        };
        let brep = assemble_cut(
            cyl,
            curve,
            (v_star, v_star),
            KeptRim {
                center: rim_center,
                radius,
                v: v_kept,
                is_base: keep_top,
                cap: Some((rim_center, cap_normal)),
            },
            plane_normal,
            keep_sign,
        )?;
        return Ok(BooleanOutcome {
            kind: BooleanOutcomeKind::Solid(brep),
        });
    }
    if beta.abs() <= PARALLEL_BAND {
        // Plane ∥ axis: the section is generator lines (secant), one
        // tangent line, or empty — classified exactly on |α| vs R.
        let mut minus_terms = alpha_terms.clone();
        minus_terms.push(-radius);
        let sign_minus = exact_sum_sign(&minus_terms);
        let mut plus_terms = alpha_terms.clone();
        plus_terms.push(radius);
        let sign_plus = exact_sum_sign(&plus_terms);
        if sign_minus == 0 {
            // Tangent along one generator: the solid lies in the
            // closed tool.
            return Ok(whole_or_empty(op, 1, whole()));
        }
        if sign_plus == 0 {
            return Ok(whole_or_empty(op, -1, whole()));
        }
        if sign_minus > 0 {
            return Ok(whole_or_empty(op, 1, whole()));
        }
        if sign_plus < 0 {
            return Ok(whole_or_empty(op, -1, whole()));
        }
        return Ok(refused(
            "the section is two generator lines — a line-trimmed cylindrical strip is \
             not representable in the K1 model",
        ));
    }

    // Oblique. The two cap-center signs decide the frame.
    if alpha_sign == top_sign {
        if alpha_sign == 0 {
            // Both centers on the plane: β ≠ 0 forces distinct
            // g-values, so this cannot happen — refuse defensively.
            return Err("both cap centers on the plane with a tilted normal".into());
        }
        // The plane misses the open axis segment: it can only cut the
        // solid by clipping cap rims. Does it cut at all?
        let (lo0, hi0) = disk_extremes(&alpha_terms, radius, beta);
        let (lo1, hi1) = disk_extremes(&top_terms, radius, beta);
        let min_sign = lo0.min(lo1);
        let max_sign = hi0.max(hi1);
        if min_sign >= 0 {
            return Ok(whole_or_empty(op, 1, whole()));
        }
        if max_sign <= 0 {
            return Ok(whole_or_empty(op, -1, whole()));
        }
        return Ok(refused(
            "the section meets the cap rims — the cut face would need mixed line/arc \
             trims, not representable in the K1 model",
        ));
    }
    if alpha_sign == 0 || top_sign == 0 {
        return Ok(refused(
            "the plane passes through a cap center — the section clips the cap",
        ));
    }
    // Both cap disks strictly missed, or the section is not a clean
    // ellipse: the exact disk-miss signs (≤ 0 covers the rim-tangency
    // non-manifold junction too).
    if cmp_disk_miss(&alpha_terms, radius, beta) <= 0
        || cmp_disk_miss(&top_terms, radius, beta) <= 0
    {
        return Ok(refused(
            "the section clips a cap rim — the elliptical cut is not representable in \
             the K1 model",
        ));
    }

    // The clean oblique section: the K3a plane×cylinder ellipse,
    // bit-identical.
    let x_hat = any_perpendicular(axis);
    let cyl = Surface::Cylinder {
        origin,
        axis,
        ref_dir: x_hat,
        radius,
    };
    let plane = Surface::Plane {
        origin: plane_point,
        normal: plane_normal,
        x_dir: any_perpendicular(plane_normal),
    };
    let curve = match intersect_surfaces(&cyl, &plane) {
        SurfaceIntersection::Curves(cs) if cs.len() == 1 => match &cs[0] {
            Curve::Ellipse { .. } => cs[0].clone(),
            other => return Err(format!("K3a returned a non-ellipse section: {other:?}")),
        },
        other => {
            return Err(format!(
                "the K3a plane×cylinder ellipse is missing: {other:?}"
            ))
        }
    };
    let (a_semi, b_semi, c_center) = match &curve {
        Curve::Ellipse {
            center,
            major_radius,
            minor_radius,
            ..
        } => (*major_radius, *minor_radius, *center),
        _ => unreachable!("checked above"),
    };
    // The ellipse's v-extent: its center is the axis crossing, the
    // amplitude √(a² − b²) (b = R on a cylinder section).
    let z0 = v3(
        c_center.x - origin.x,
        c_center.y - origin.y,
        c_center.z - origin.z,
    )
    .dot(&axis);
    let amp = (a_semi * a_semi - b_semi * b_semi).max(0.0).sqrt();

    let keep_sign = if op == BooleanOp::Common { 1.0 } else { -1.0 };
    let keep_top = (beta > 0.0) == (op == BooleanOp::Common);
    let v_kept = if keep_top { height } else { 0.0 };
    let rim_center = v3(
        origin.x + axis.x * v_kept,
        origin.y + axis.y * v_kept,
        origin.z + axis.z * v_kept,
    );
    let cap_normal = if keep_top {
        axis
    } else {
        v3(-axis.x, -axis.y, -axis.z)
    };
    let brep = assemble_cut(
        cyl,
        curve,
        (z0 - amp, z0 + amp),
        KeptRim {
            center: rim_center,
            radius,
            v: v_kept,
            is_base: keep_top,
            cap: Some((rim_center, cap_normal)),
        },
        plane_normal,
        keep_sign,
    )?;
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

/// The exact B-rep boolean of a cone — plain (`top_radius == 0`, the
/// apex) or frustum — with the halfspace tool `{x : n·(x − p) ≥ 0}`
/// (`axis` base→top and `n` unit).
///
/// The composed faces carry the **apex-origin parameterization**
/// (origin at the apex, axis apex→base, semi-angle in `(0, π/2)`,
/// ref_radius 0): the K3a `plane×cone` section of THAT surface is
/// the same curve OCCT's parameterization would name, and it is
/// reused bit-identically as the section edge.
///
/// * A clean section composes the kept piece: the side strip between
///   the kept rim (the base circle, the top circle, or the degenerate
///   apex circle of a plain cone's apex piece), the kept cap where
///   one exists, and the elliptical section disk.
/// * The apex piece of a plain cone is the classical **oblique cone
///   over the ellipse** — `V = (1/3)·πab·d(apex, plane)`.
/// * A perpendicular plane sections the axis-meet circle; the pieces
///   are a shorter cone / frusta.
/// * Steep planes (the hyperbola/parabola class, `sinγ ≥ cosφ`),
///   sections clipping a cap rim, and a plane through the apex refuse
///   by name; non-secant pass-throughs are honest.
/// * `Fuse` is refused as unbounded.
#[allow(clippy::too_many_arguments)] // the cone's Part::Cone parameters + the halfspace tool
pub fn cone_plane_boolean(
    op: BooleanOp,
    origin: Vector3,
    axis: Vector3,
    base_radius: f64,
    top_radius: f64,
    height: f64,
    plane_point: Vector3,
    plane_normal: Vector3,
) -> Result<BooleanOutcome, String> {
    require_unit(axis, "cone axis")?;
    require_unit(plane_normal, "halfspace normal")?;
    if base_radius <= 0.0 {
        return Err(format!(
            "the base radius must be positive, got {base_radius}"
        ));
    }
    if top_radius < 0.0 {
        return Err(format!("the top radius must be >= 0, got {top_radius}"));
    }
    if height <= 0.0 {
        return Err(format!("the cone height must be positive, got {height}"));
    }
    if (base_radius - top_radius).abs() <= f64::EPSILON * base_radius.max(1.0) {
        return Err(
            "equal end radii make a cylinder, not a cone — use cylinder_plane_boolean".into(),
        );
    }
    if op == BooleanOp::Fuse {
        return Ok(unbounded_fuse());
    }
    let r1 = base_radius;
    let r2 = top_radius;
    let has_top = r2 > 0.0;

    // The apex of the extended cone and the apex-origin frame.
    let apex = if has_top {
        let h_apex = height * r1 / (r1 - r2);
        v3(
            origin.x + axis.x * h_apex,
            origin.y + axis.y * h_apex,
            origin.z + axis.z * h_apex,
        )
    } else {
        v3(
            origin.x + axis.x * height,
            origin.y + axis.y * height,
            origin.z + axis.z * height,
        )
    };
    let a_prime = if has_top {
        v3(origin.x - apex.x, origin.y - apex.y, origin.z - apex.z)
            .normalized()
            .expect("apex ≠ base center")
    } else {
        v3(-axis.x, -axis.y, -axis.z)
    };
    let d_apex = v3(origin.x - apex.x, origin.y - apex.y, origin.z - apex.z).length();
    let semi = r1.atan2(d_apex);
    let (sin_semi, cos_semi) = semi.sin_cos();
    let x_hat = any_perpendicular(a_prime);
    let cone = Surface::Cone {
        origin: apex,
        axis: a_prime,
        ref_dir: x_hat,
        semi_angle: semi,
        ref_radius: 0.0,
    };
    // The rim v-parameters (slant): the base at v_b, the top at v_t.
    let v_base = d_apex / cos_semi;
    let v_top = if has_top {
        (d_apex - height) / cos_semi
    } else {
        0.0
    };

    // The end-reference g-values: the base disk center and the top
    // disk center (the apex for a plain cone — the same point as
    // origin + h·axis).
    let top_center = v3(
        origin.x + axis.x * height,
        origin.y + axis.y * height,
        origin.z + axis.z * height,
    );
    let g_base_terms = dot_terms(
        v3(
            origin.x - plane_point.x,
            origin.y - plane_point.y,
            origin.z - plane_point.z,
        ),
        plane_normal,
    );
    let g_top_terms = dot_terms(
        v3(
            top_center.x - plane_point.x,
            top_center.y - plane_point.y,
            top_center.z - plane_point.z,
        ),
        plane_normal,
    );
    let base_sign = exact_sum_sign(&g_base_terms);
    let top_sign = exact_sum_sign(&g_top_terms);
    let beta = exact_sum_rounded(&dot_terms(a_prime, plane_normal));
    let sin_gamma = plane_normal.cross(&a_prime).length();
    let whole = || {
        placed_cone_brep(
            apex, a_prime, x_hat, semi, origin, r1, top_center, r2, v_base, v_top, has_top,
        )
    };

    // The K3a branch order: perpendicular, parallel, oblique.
    if sin_gamma <= PARALLEL_BAND {
        // Plane ⊥ axis: the axis-meet circle.
        if beta == 0.0 {
            return Err("a unit normal cannot be both parallel and perpendicular".into());
        }
        if base_sign == top_sign {
            let side = if beta > 0.0 { base_sign } else { top_sign };
            return Ok(whole_or_empty(op, side, whole()));
        }
        // Secant: the crossing height along the axis from the base.
        let g_base = exact_sum_rounded(&g_base_terms);
        let v_star_z = -g_base / beta;
        // The crossing's slant parameter (from the apex).
        let v_star = (d_apex - v_star_z) / cos_semi;
        let plane = Surface::Plane {
            origin: plane_point,
            normal: plane_normal,
            x_dir: any_perpendicular(plane_normal),
        };
        let curve = match intersect_surfaces(&cone, &plane) {
            SurfaceIntersection::Curves(cs) if cs.len() == 1 => match &cs[0] {
                Curve::Circle { radius: rho, .. } if *rho > 0.0 => cs[0].clone(),
                other => return Err(format!("K3a returned a non-circle section: {other:?}")),
            },
            other => return Err(format!("the K3a axis-meet circle is missing: {other:?}")),
        };
        let keep_sign = if op == BooleanOp::Common { 1.0 } else { -1.0 };
        let keep_apex = (beta < 0.0) == (op == BooleanOp::Common);
        let rim = kept_cone_rim(
            keep_apex, has_top, top_center, r2, v_top, apex, origin, r1, v_base, a_prime,
        );
        let brep = assemble_cut(cone, curve, (v_star, v_star), rim, plane_normal, keep_sign)?;
        return Ok(BooleanOutcome {
            kind: BooleanOutcomeKind::Solid(brep),
        });
    }
    if beta.abs() <= PARALLEL_BAND {
        // Plane ∥ axis: the hyperbola/parabola class when it cuts.
        let (lo0, hi0) = disk_extremes(&g_base_terms, r1, beta);
        let extremes = if has_top {
            let (lo1, hi1) = disk_extremes(&g_top_terms, r2, beta);
            (lo0.min(lo1), hi0.max(hi1))
        } else {
            let apex_sign = top_sign;
            (lo0.min(apex_sign), hi0.max(apex_sign))
        };
        if extremes.0 >= 0 {
            return Ok(whole_or_empty(op, 1, whole()));
        }
        if extremes.1 <= 0 {
            return Ok(whole_or_empty(op, -1, whole()));
        }
        return Ok(refused(
            "the plane×cone section is the hyperbola/parabola class (γ + semi_angle ≥ \
             90°) — not representable in the K1 curve set",
        ));
    }

    // Oblique.
    if base_sign == top_sign {
        if base_sign == 0 {
            return Err("both end centers on the plane with a tilted normal".into());
        }
        let (lo0, hi0) = disk_extremes(&g_base_terms, r1, beta);
        let extremes = if has_top {
            let (lo1, hi1) = disk_extremes(&g_top_terms, r2, beta);
            (lo0.min(lo1), hi0.max(hi1))
        } else {
            (lo0.min(top_sign), hi0.max(top_sign))
        };
        if extremes.0 >= 0 {
            return Ok(whole_or_empty(op, 1, whole()));
        }
        if extremes.1 <= 0 {
            return Ok(whole_or_empty(op, -1, whole()));
        }
        if sin_gamma >= cos_semi {
            return Ok(refused(
                "the plane×cone section is the hyperbola/parabola class (γ + \
                 semi_angle ≥ 90°) — not representable in the K1 curve set",
            ));
        }
        return Ok(refused(
            "the section meets the cap rims — the cut face would need mixed line/arc \
             trims, not representable in the K1 model",
        ));
    }
    if !has_top && top_sign == 0 {
        return Ok(refused(
            "the plane passes through the apex — generator-line sections are not \
             representable in the K1 model",
        ));
    }
    if base_sign == 0 || (has_top && top_sign == 0) {
        return Ok(refused(
            "the plane passes through an end center — the section clips the cap",
        ));
    }
    if sin_gamma >= cos_semi {
        return Ok(refused(
            "the plane×cone section is the hyperbola/parabola class (γ + semi_angle \
             ≥ 90°) — not representable in the K1 curve set",
        ));
    }
    if cmp_disk_miss(&g_base_terms, r1, beta) <= 0
        || (has_top && cmp_disk_miss(&g_top_terms, r2, beta) <= 0)
    {
        return Ok(refused(
            "the section clips a cap rim — the elliptical cut is not representable \
             in the K1 model",
        ));
    }

    // The clean oblique section: the K3a plane×cone ellipse of the
    // apex-origin surface, bit-identical.
    let plane = Surface::Plane {
        origin: plane_point,
        normal: plane_normal,
        x_dir: any_perpendicular(plane_normal),
    };
    let curve = match intersect_surfaces(&cone, &plane) {
        SurfaceIntersection::Curves(cs) if cs.len() == 1 => match &cs[0] {
            Curve::Ellipse { .. } => cs[0].clone(),
            other => return Err(format!("K3a returned a non-ellipse section: {other:?}")),
        },
        other => return Err(format!("the K3a plane×cone ellipse is missing: {other:?}")),
    };
    // The ellipse's v-extent in the slant parameter: the Möbius trim
    // v_e(u) = γ0/(κ0 + sinφ·m(u)) takes its extremes at m = ±M.
    let (c_center, n_e) = match &curve {
        Curve::Ellipse { center, axis, .. } => (*center, *axis),
        _ => unreachable!("checked above"),
    };
    let y_hat = a_prime.cross(&x_hat);
    let m_c = n_e.dot(&x_hat);
    let m_s = n_e.dot(&y_hat);
    let m_amp = (m_c * m_c + m_s * m_s).sqrt();
    let gamma0 = v3(
        c_center.x - apex.x,
        c_center.y - apex.y,
        c_center.z - apex.z,
    )
    .dot(&n_e);
    let kappa0 = cos_semi * n_e.dot(&a_prime);
    let v_minus = gamma0 / (kappa0 + sin_semi * m_amp);
    let v_plus = gamma0 / (kappa0 - sin_semi * m_amp);

    let keep_sign = if op == BooleanOp::Common { 1.0 } else { -1.0 };
    let keep_apex = (beta < 0.0) == (op == BooleanOp::Common);
    let rim = kept_cone_rim(
        keep_apex, has_top, top_center, r2, v_top, apex, origin, r1, v_base, a_prime,
    );
    let brep = assemble_cut(
        cone,
        curve,
        (v_minus.min(v_plus), v_minus.max(v_plus)),
        rim,
        plane_normal,
        keep_sign,
    )?;
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

/// The kept rim of a cone piece: the apex side keeps the top circle
/// (frustum) or the degenerate apex circle (plain cone, no cap face);
/// the base side keeps the base circle.
#[allow(clippy::too_many_arguments)] // the two ends' ingredients, mirroring the primitive's own parameters
fn kept_cone_rim(
    keep_apex: bool,
    has_top: bool,
    top_center: Vector3,
    r2: f64,
    v_top: f64,
    apex: Vector3,
    origin: Vector3,
    r1: f64,
    v_base: f64,
    a_prime: Vector3,
) -> KeptRim {
    if keep_apex {
        if has_top {
            KeptRim {
                center: top_center,
                radius: r2,
                v: v_top,
                is_base: false,
                cap: Some((top_center, v3(-a_prime.x, -a_prime.y, -a_prime.z))),
            }
        } else {
            KeptRim {
                center: apex,
                radius: 0.0,
                v: 0.0,
                is_base: false,
                cap: None,
            }
        }
    } else {
        KeptRim {
            center: origin,
            radius: r1,
            v: v_base,
            is_base: true,
            cap: Some((origin, a_prime)),
        }
    }
}

/// A closed cone piece — the pass-through whole — placed directly in
/// the apex-origin parameterization: the strip between the base rim
/// and the top rim (or the degenerate apex circle), plus the cap
/// faces. The seam runs along the generator at parameter angle 0.
#[allow(clippy::too_many_arguments)]
fn placed_cone_brep(
    apex: Vector3,
    a_prime: Vector3,
    x_hat: Vector3,
    semi: f64,
    base_center: Vector3,
    r1: f64,
    top_center: Vector3,
    r2: f64,
    v_base: f64,
    v_top: f64,
    has_top: bool,
) -> ExactBRep {
    let base_curve = Curve::Circle {
        center: base_center,
        axis: a_prime,
        ref_dir: x_hat,
        radius: r1,
    };
    let (top_curve, v_hi_point) = if has_top {
        (
            Curve::Circle {
                center: top_center,
                axis: a_prime,
                ref_dir: x_hat,
                radius: r2,
            },
            top_center,
        )
    } else {
        (
            Curve::Circle {
                center: apex,
                axis: a_prime,
                ref_dir: x_hat,
                radius: 0.0,
            },
            apex,
        )
    };
    let v_base_pt = base_curve.point(0.0);
    let v_top_pt = top_curve.point(0.0);
    let surface = Surface::Cone {
        origin: apex,
        axis: a_prime,
        ref_dir: x_hat,
        semi_angle: semi,
        ref_radius: 0.0,
    };
    let mut faces = vec![
        // The strip: base circle, seam up, top/apex circle, seam
        // back — kept-on-the-left against the outward natural normal
        // (the region below the base rim and above the top rim).
        TopoFace {
            surface: surface.clone(),
            uv_bounds: [0.0, std::f64::consts::TAU, v_top, v_base],
            wires: vec![TopoWire {
                edges: vec![
                    WireEdge {
                        edge: 0,
                        forward: false,
                    },
                    WireEdge {
                        edge: 2,
                        forward: true,
                    },
                    WireEdge {
                        edge: 1,
                        forward: true,
                    },
                    WireEdge {
                        edge: 2,
                        forward: false,
                    },
                ],
            }],
            reversed: false,
        },
        // The base cap: outward +a' (away from the cone).
        TopoFace {
            surface: Surface::Plane {
                origin: base_center,
                normal: a_prime,
                x_dir: x_hat,
            },
            uv_bounds: [-r1, r1, -r1, r1],
            wires: vec![TopoWire {
                edges: vec![WireEdge {
                    edge: 0,
                    forward: true,
                }],
            }],
            reversed: false,
        },
    ];
    if has_top {
        faces.push(TopoFace {
            surface: Surface::Plane {
                origin: top_center,
                normal: v3(-a_prime.x, -a_prime.y, -a_prime.z),
                x_dir: x_hat,
            },
            uv_bounds: [-r2, r2, -r2, r2],
            wires: vec![TopoWire {
                edges: vec![WireEdge {
                    edge: 1,
                    forward: false,
                }],
            }],
            reversed: false,
        });
    }
    let _ = v_hi_point;
    ExactBRep {
        vertices: vec![
            TopoVertex { point: v_base_pt },
            TopoVertex { point: v_top_pt },
        ],
        edges: vec![
            TopoEdge {
                curve: base_curve,
                range: (0.0, std::f64::consts::TAU),
                start: 0,
                end: 0,
            },
            TopoEdge {
                curve: top_curve,
                range: (0.0, std::f64::consts::TAU),
                start: 1,
                end: 1,
            },
            seam_between(v_base_pt, v_top_pt, 0, 1),
        ],
        faces,
    }
}
