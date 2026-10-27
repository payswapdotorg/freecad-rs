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
    any_perpendicular, diff_products_rounded, dot_terms, exact_sum_rounded, exact_sum_sign,
    length_squared_terms, lex_after, two_prod, two_sum, PARALLEL_BAND,
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

// ---------------------------------------------------------------------------
// K3d — cylinder × cylinder: the parallel-axis compositions
// ---------------------------------------------------------------------------
//
// Two finite cylinders with parallel axes (either sign — the tool is
// normalized to the target's axis direction first) have, when their
// cross-sections cross, a section of two generator lines (the K3a
// construction, reused bit-identically). Every plane ⊥ the axes cuts
// the SAME two-circle picture, so the booleans are prisms of the planar
// circle-circle booleans over the z-overlap:
//
// * **Common** — the lens prism: each cylinder's inside arc strip over
//   the overlap, capped by the two lens faces.
// * **Cut** (`a − b`, b's z-range covering a's) — a's outside-arc strip,
//   b's inside-arc wall (a **reversed** face — the solid lies outside
//   b), and the disk-minus-lens caps at a's planes.
// * **Fuse** (equal z-ranges) — both outside-arc strips and the
//   union-region caps.
// * **Cut** with b nested inside a cross-wise (b's z-range covering
//   a's) — the tube: a's mantle, b's mantle reversed, annular caps.
//
// The shared edges are the crossing generators — degree-1 B-splines
// through the canonical K3a line points at the two boundary heights,
// exact at both ends — and the arcs. An arc cannot be a `Circle`
// parameter range here: its endpoint evaluation on one operand's frame
// is a different float than the other's, and the bit-exact vertex
// contract would be violated. The arcs are therefore the classical
// **rational quadratic B-splines** — the NURBS circle-arc
// representation every CAD kernel, OCCT included, uses: control points
// [start, tangent-intersection, end], weights [1, cos(sweep/2), 1],
// clamped knots, the de Boor evaluation exact at both ends. Sweeps
// wider than a quadrant split into equal segments (the classical
// conditioning rule, keeping the weights ≥ cos(π/4) and the tangent
// intersection finite).
//
// The measurement ([`crate::measure`]) reads the arcs back exactly: the
// cap areas by the line integral `∫(x dy − y dx) = R²Δθ + c × Δp` per
// arc — closed form, no quadrature — and the strips as swept angular
// intervals.
//
// Honest refusals, by name: non-parallel axes (the Steinmetz quartic
// — K3a's own refusal, propagated), the staggered cut/fuse and the
// staggered drill (the notch mantles — a later slice), the pinched
// tube of an internal cross-tangency (measure zero), and the
// tangent/disjoint fuses (non-manifold junctions / two shells).

/// The exact B-rep boolean of two finite cylinders with parallel
/// axes (the Part::Cylinder frame: origin = base center, axis
/// base→top unit, positive height; antiparallel tools are
/// normalized).
///
/// The compositions are prisms of the planar circle-circle booleans
/// over the z-overlap (see the module doc); the section is the K3a
/// two-line construction, reused bit-identically. Configurations the
/// model cannot represent refuse by name, never approximately.
#[allow(clippy::too_many_arguments)] // two Part::Cylinder parameter sets
pub fn cylinder_cylinder_boolean(
    op: BooleanOp,
    o1: Vector3,
    a1: Vector3,
    r1: f64,
    h1: f64,
    o2: Vector3,
    a2: Vector3,
    r2: f64,
    h2: f64,
) -> Result<BooleanOutcome, String> {
    require_unit(a1, "cylinder a axis")?;
    require_unit(a2, "cylinder b axis")?;
    for (r, h, what) in [(r1, h1, "a"), (r2, h2, "b")] {
        if r <= 0.0 {
            return Err(format!(
                "the {what} cylinder radius must be positive, got {r}"
            ));
        }
        if h <= 0.0 {
            return Err(format!(
                "the {what} cylinder height must be positive, got {h}"
            ));
        }
    }

    // Antiparallel tools normalized to the target's axis direction
    // (the same solid, base and top exchanged).
    let (o2, a2) = if a1.dot(&a2) < 0.0 {
        (
            v3(o2.x + a2.x * h2, o2.y + a2.y * h2, o2.z + a2.z * h2),
            v3(-a2.x, -a2.y, -a2.z),
        )
    } else {
        (o2, a2)
    };
    let a = a1;

    // Operand-order canonicalization for the symmetric operations:
    // `common(a, b)` and `fuse(a, b)` must be bit-identical under the
    // swap (the target/tool roles of `Cut` are not symmetric).
    let (o1, r1, h1, o2, r2, h2) = if matches!(op, BooleanOp::Common | BooleanOp::Fuse)
        && lex_after(&[o1.x, o1.y, o1.z, r1], &[o2.x, o2.y, o2.z, r2])
    {
        (o2, r2, h2, o1, r1, h1)
    } else {
        (o1, r1, h1, o2, r2, h2)
    };

    // The z-analysis in a's v-frame.
    let s2 = v3(o2.x - o1.x, o2.y - o1.y, o2.z - o1.z).dot(&a);
    let v_lo = s2.max(0.0);
    let v_hi = (s2 + h2).min(h1);
    let z_disjoint = v_lo >= v_hi;
    let equal_ranges = s2 == 0.0 && s2 + h2 == h1;
    let b_covers_a = s2 <= 0.0 && s2 + h2 >= h1;

    let whole_a = || cylinder_piece(o1, a, r1, 0.0, h1);
    let whole_b = || cylinder_piece(o2, a, r2, 0.0, h2);
    let disjoint_fuse =
        || refused("disjoint fuse — a two-shell compound, not a single closed shell");

    // The cross-section classification, exactly as K3a computes it.
    let x_hat = any_perpendicular(a);
    let surface_a = Surface::Cylinder {
        origin: o1,
        axis: a,
        ref_dir: x_hat,
        radius: r1,
    };
    let surface_b = Surface::Cylinder {
        origin: o2,
        axis: a2,
        ref_dir: x_hat,
        radius: r2,
    };
    let section = intersect_surfaces(&surface_a, &surface_b);
    if let SurfaceIntersection::NotAnalytic { why } = &section {
        return Ok(refused(why));
    }

    // The perpendicular center distance, exactly (the K3a operands'
    // own arithmetic, redone here for the classification tier).
    let perp = {
        let dv = v3(o2.x - o1.x, o2.y - o1.y, o2.z - o1.z);
        let along = dv.dot(&a);
        v3(dv.x - a.x * along, dv.y - a.y * along, dv.z - a.z * along)
    };
    let d2_terms = length_squared_terms(perp);
    let mut ext = d2_terms.clone();
    let (sh, se) = two_prod(r1 + r2, r1 + r2);
    ext.push(-sh);
    ext.push(-se);
    let ext_sign = exact_sum_sign(&ext);
    let mut inn = d2_terms.clone();
    let (ih, ie) = two_prod(r1 - r2, r1 - r2);
    inn.push(-ih);
    inn.push(-ie);
    let int_sign = exact_sum_sign(&inn);

    // ---- z-disjoint: no interaction at all --------------------------
    if z_disjoint {
        return Ok(match op {
            BooleanOp::Common => empty(),
            BooleanOp::Cut => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(whole_a()),
            },
            BooleanOp::Fuse => disjoint_fuse(),
        });
    }

    // ---- coincident surfaces (coaxial, equal radius) ----------------
    // K3a's exact `Coincident` needs d² == 0 to the bit; an axis
    // offset that differs only by the arithmetic residue of the
    // caller's placement (d at the ulp scale) would otherwise classify
    // as crossing and compose a lens of noise. Sub-machine d with
    // equal radii reads as coincident — the honest reading of the
    // intent (unequal radii stay nested, which needs no section).
    let d_len = exact_sum_rounded(&d2_terms).max(0.0).sqrt();
    let machine_coincident = d_len <= 1e-9 * r1.max(r2).max(1.0);
    if matches!(section, SurfaceIntersection::Coincident) || (machine_coincident && r1 == r2) {
        if equal_ranges {
            return Ok(match op {
                BooleanOp::Common | BooleanOp::Fuse => BooleanOutcome {
                    kind: BooleanOutcomeKind::Solid(whole_a()),
                },
                BooleanOp::Cut => empty(),
            });
        }
        // Staggered ranges: the end-to-end pieces.
        // Common: the overlap piece. Fuse: the hull when the ranges
        // meet. Cut: the part of a outside b — one piece when b clips
        // an end of a, empty when b covers a, two pieces (refused)
        // when b sits strictly inside.
        let hull_lo = s2.min(0.0);
        let hull_hi = (s2 + h2).max(h1);
        return Ok(match op {
            BooleanOp::Common => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(cylinder_piece(o1, a, r1, v_lo, v_hi)),
            },
            BooleanOp::Fuse if v_lo < v_hi => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(cylinder_piece(o1, a, r1, hull_lo, hull_hi)),
            },
            BooleanOp::Fuse => disjoint_fuse(),
            BooleanOp::Cut if b_covers_a => empty(),
            BooleanOp::Cut if s2 > 0.0 && s2 + h2 < h1 => refused(
                "the coincident cut with b strictly inside — two pieces, a \
                 two-shell compound",
            ),
            BooleanOp::Cut if v_lo < v_hi => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(if s2 <= 0.0 {
                    cylinder_piece(o1, a, r1, v_hi, h1)
                } else {
                    cylinder_piece(o1, a, r1, 0.0, v_lo)
                }),
            },
            BooleanOp::Cut => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(whole_a()),
            },
        });
    }

    // ---- nested cross-sections (or their internal tangency) ------------
    // K3a reports the nested pair as `Empty`; the internally tangent
    // pair (one disk inside the other, touching along one generator)
    // as a single line with the interior sign exactly zero.
    let single_line = matches!(&section, SurfaceIntersection::Curves(cs) if cs.len() == 1);
    let internal_tangency = single_line && int_sign == 0;
    if matches!(section, SurfaceIntersection::Empty) || internal_tangency {
        if ext_sign > 0 {
            // Cross-disjoint: like z-disjoint but with the cylinders
            // side by side — the same pass-throughs.
            return Ok(match op {
                BooleanOp::Common => empty(),
                BooleanOp::Cut => BooleanOutcome {
                    kind: BooleanOutcomeKind::Solid(whole_a()),
                },
                BooleanOp::Fuse => disjoint_fuse(),
            });
        }
        // Nested: one disk inside the other. Strictly (int_sign < 0),
        // or at the internal tangency (int_sign == 0, measure zero).
        let b_inside_a = r1 > r2;
        // Common: the inner cylinder clipped to the overlap — an
        // ordinary piece (any overlap shape; the tangency adds
        // nothing).
        let (oi, ri, si, _hi) = if b_inside_a {
            (o2, r2, s2, h2)
        } else {
            (o1, r1, 0.0, h1)
        };
        if op == BooleanOp::Common {
            let w_lo = if b_inside_a { v_lo - si } else { v_lo };
            let w_hi = if b_inside_a { v_hi - si } else { v_hi };
            return Ok(BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(cylinder_piece(oi, a, ri, w_lo, w_hi)),
            });
        }
        // Fuse: one cylinder entirely inside the other (both frame and
        // range) — the outer whole piece; b inside a cross-wise with b
        // sticking out of a's range — the chimney.
        let (outer_contains, whole) = if b_inside_a {
            (s2 >= 0.0 && s2 + h2 <= h1, whole_a())
        } else {
            (s2 <= 0.0 && s2 + h2 >= h1, whole_b())
        };
        if op == BooleanOp::Fuse {
            if outer_contains {
                return Ok(BooleanOutcome {
                    kind: BooleanOutcomeKind::Solid(whole),
                });
            }
            if b_inside_a {
                let brep = nested_chimney(o1, a, r1, h1, o2, r2, s2, h2);
                brep.validate()
                    .map_err(|e| format!("the composed chimney failed validation: {e}"))?;
                return Ok(BooleanOutcome {
                    kind: BooleanOutcomeKind::Solid(brep),
                });
            }
            // a inside b cross-wise with a sticking out of b's range:
            // the mirrored chimney (the roles swapped — a is the
            // chimney on b; a's base sits at −s2 in b's frame).
            let brep = nested_chimney(o2, a, r2, h2, o1, r1, -s2, h1);
            brep.validate()
                .map_err(|e| format!("the composed mirrored chimney failed validation: {e}"))?;
            return Ok(BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(brep),
            });
        }
        // Cut: b nested inside a cross-wise. The tube needs b's
        // z-range to cover a's; anything else is the drill.
        if !b_inside_a {
            // a inside b cross-wise: a − b is a's z-parts beyond b's
            // range — one ordinary piece, or two shells.
            if b_covers_a {
                return Ok(empty());
            }
            let below = s2 > 0.0; // a's part [0, s2)
            let above = s2 + h2 < h1; // a's part (s2 + h2, h1]
            if below && above {
                return Ok(refused(
                    "the staggered cut of cylinders nested the other way — two \
                     sticking-out pieces, a two-shell compound",
                ));
            }
            return Ok(BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(if below {
                    cylinder_piece(o1, a, r1, 0.0, s2)
                } else {
                    cylinder_piece(o1, a, r1, s2 + h2, h1)
                }),
            });
        }
        if !b_covers_a {
            // b inside a cross-wise with b's range crossing one of a's
            // ends: the partial bore. Strictly inside: the internal
            // void — a second shell, refused by name.
            if s2 >= 0.0 && s2 + h2 <= h1 {
                return Ok(refused(
                    "the nested blind bore — the internal void needs a second \
                     shell the K1 model does not carry",
                ));
            }
            let from_base = s2 < 0.0;
            return Ok(BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(nested_partial_bore(
                    o1, a, r1, h1, o2, r2, s2, h2, from_base,
                )),
            });
        }
        if internal_tangency {
            return Ok(refused(
                "internally tangent cut — the tube pinches to a line \
                 (measure zero), refused",
            ));
        }
        return Ok(BooleanOutcome {
            kind: BooleanOutcomeKind::Solid(nested_tube(o1, a, r1, h1, o2, r2)),
        });
    }

    // ---- external cross-tangency: one generator line, measure zero ----
    if single_line {
        return Ok(match op {
            BooleanOp::Common => empty(),
            BooleanOp::Cut => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(whole_a()),
            },
            BooleanOp::Fuse => refused(
                "tangent fuse — the shared generator line is not an \
                 edge-manifold junction",
            ),
        });
    }

    // ---- the crossing section: two generator lines -------------------
    let lines = match &section {
        SurfaceIntersection::Curves(cs) if cs.len() == 2 => match (&cs[0], &cs[1]) {
            (Curve::Line { .. }, Curve::Line { .. }) => (cs[0].clone(), cs[1].clone()),
            _ => return Err("K3a returned a non-line cylinder section".into()),
        },
        other => return Err(format!("the K3a two-line section is missing: {other:?}")),
    };

    // The op guards on the z-configuration (Common composes over any
    // overlap; the others need the covering/equal/drilling shapes).
    let blind_drill = op == BooleanOp::Cut && !b_covers_a && s2 > 0.0 && s2 + h2 < h1;
    match op {
        BooleanOp::Common => {}
        BooleanOp::Cut if !b_covers_a && !blind_drill => {
            return Ok(refused(
                "the one-sided drill — the split-circle mantles (a rim \
                 interrupted by the notch) are the next slice",
            ))
        }
        BooleanOp::Fuse if !equal_ranges => {
            return Ok(refused(
                "the staggered fuse — the notch mantles are the next slice",
            ))
        }
        _ => {}
    }

    // The canonical crossing lines, at a's v-parameters. K3a's lines
    // run along `a` (both stored axes are `a` after normalization);
    // each line's origin sits in a plane ⊥ a through one of the
    // (canonically ordered) origins, so the point at v-parameter `v` is
    // the exact line evaluation at `v − v₀`.
    let line_at = |c: &Curve, v: f64| -> Vector3 {
        match c {
            Curve::Line { origin, direction } => {
                let v0 = v3(origin.x - o1.x, origin.y - o1.y, origin.z - o1.z).dot(&a);
                let t = v - v0;
                v3(
                    origin.x + direction.x * t,
                    origin.y + direction.y * t,
                    origin.z + direction.z * t,
                )
            }
            _ => unreachable!("checked above"),
        }
    };
    // The angle of a line's crossing offset, in the given cylinder's
    // frame (x_hat, y_hat shared: the axes coincide).
    let y_hat = a.cross(&x_hat);
    let theta_on = |c: &Curve, o: Vector3| -> f64 {
        match c {
            Curve::Line { origin, .. } => {
                let off = v3(origin.x - o.x, origin.y - o.y, origin.z - o.z);
                let along = off.dot(&a);
                let radial = v3(
                    off.x - a.x * along,
                    off.y - a.y * along,
                    off.z - a.z * along,
                );
                radial.dot(&y_hat).atan2(radial.dot(&x_hat))
            }
            _ => unreachable!("checked above"),
        }
    };
    let axis_dist = |p: Vector3, o: Vector3| -> f64 {
        let off = v3(p.x - o.x, p.y - o.y, p.z - o.z);
        let along = off.dot(&a);
        v3(
            off.x - a.x * along,
            off.y - a.y * along,
            off.z - a.z * along,
        )
        .length()
    };

    let th_p = theta_on(&lines.0, o1);
    let th_q = theta_on(&lines.1, o1);
    let ccw_a = {
        let d = th_q - th_p;
        if d > 0.0 {
            d
        } else {
            d + std::f64::consts::TAU
        }
    };
    let ph_p = theta_on(&lines.0, o2);
    let ph_q = theta_on(&lines.1, o2);
    let ccw_b = {
        let d = ph_q - ph_p;
        if d > 0.0 {
            d
        } else {
            d + std::f64::consts::TAU
        }
    };

    // Which of the two arcs between the crossings is the inside one:
    // the CCW midpoint, tested against the other disk. (The inside
    // sweeps of the two circles always have opposite signs — the
    // crossing transversalities at P oppose; the cap and strip wires
    // below rely on it.)
    let mid_angle = |o: Vector3, r: f64, th: f64, ccw: f64| -> Vector3 {
        let m = th + ccw / 2.0;
        let (s, c) = m.sin_cos();
        v3(
            o.x + r * (c * x_hat.x + s * y_hat.x),
            o.y + r * (c * x_hat.y + s * y_hat.y),
            o.z + r * (c * x_hat.z + s * y_hat.z),
        )
    };
    let a_inside_b = axis_dist(mid_angle(o1, r1, th_p, ccw_a), o2) < r2;
    let b_inside_a = axis_dist(mid_angle(o2, r2, ph_p, ccw_b), o1) < r1;
    let sweep_a_in = if a_inside_b {
        ccw_a
    } else {
        ccw_a - std::f64::consts::TAU
    };
    let sweep_a_out = if a_inside_b {
        ccw_a - std::f64::consts::TAU
    } else {
        ccw_a
    };
    let sweep_b_in = if b_inside_a {
        ccw_b
    } else {
        ccw_b - std::f64::consts::TAU
    };
    let sweep_b_out = if b_inside_a {
        ccw_b - std::f64::consts::TAU
    } else {
        ccw_b
    };

    // The canonical crossing points at the two boundary heights.
    let p_lo = line_at(&lines.0, v_lo);
    let q_lo = line_at(&lines.1, v_lo);
    let p_hi = line_at(&lines.0, v_hi);
    let q_hi = line_at(&lines.1, v_hi);

    // The assembly tables. Vertices 0..3: P/Q at lo/hi; the arc chains
    // append their junctions.
    let mut vertices = vec![
        TopoVertex { point: p_lo },
        TopoVertex { point: q_lo },
        TopoVertex { point: p_hi },
        TopoVertex { point: q_hi },
    ];
    let mut edges: Vec<TopoEdge> = Vec::new();
    let center_on = |o: Vector3, v: f64| v3(o.x + a.x * v, o.y + a.y * v, o.z + a.z * v);
    // b's circles at the A-frame heights v ∈ {v_lo, v_hi} sit at b's
    // own parameters v − s2 (the axis offsets).
    let center_b_on = |v: f64| center_on(o2, v - s2);
    // The chains each operation needs — nothing else is built (the
    // unused arcs would break the Euler count as dead edges):
    // * Common: both inside arcs;
    // * Cut: a's outside arc + b's inside arc (the wall);
    // * Fuse: both outside arcs.
    let sweep_a_first = match op {
        BooleanOp::Common => sweep_a_in,
        BooleanOp::Fuse => sweep_a_out,
        // The through-cut composes a's outside-arc strip; the blind
        // drill's notch hole uses a's inside arcs.
        BooleanOp::Cut if !blind_drill => sweep_a_out,
        _ => sweep_a_in,
    };
    let sweep_b_second = match op {
        BooleanOp::Fuse => sweep_b_out,
        _ => sweep_b_in,
    };
    let chain_a_lo = push_arc_chain(
        &mut vertices,
        &mut edges,
        center_on(o1, v_lo),
        x_hat,
        y_hat,
        r1,
        p_lo,
        q_lo,
        th_p,
        sweep_a_first,
        0,
        1,
    );
    let chain_a_hi = push_arc_chain(
        &mut vertices,
        &mut edges,
        center_on(o1, v_hi),
        x_hat,
        y_hat,
        r1,
        p_hi,
        q_hi,
        th_p,
        sweep_a_first,
        2,
        3,
    );
    let chain_b_lo = push_arc_chain(
        &mut vertices,
        &mut edges,
        center_b_on(v_lo),
        x_hat,
        y_hat,
        r2,
        p_lo,
        q_lo,
        ph_p,
        sweep_b_second,
        0,
        1,
    );
    let chain_b_hi = push_arc_chain(
        &mut vertices,
        &mut edges,
        center_b_on(v_hi),
        x_hat,
        y_hat,
        r2,
        p_hi,
        q_hi,
        ph_p,
        sweep_b_second,
        2,
        3,
    );
    edges.push(seam_between(p_lo, p_hi, 0, 2));
    let e_vp = edges.len() - 1;
    edges.push(seam_between(q_lo, q_hi, 1, 3));
    let e_vq = edges.len() - 1;
    // Aliases for the face assembly below (the cut's a-faces and the
    // fuse's a-faces use a's outside arcs; the common's use the
    // inside arcs — the same chains built above).
    let (chain_ao_lo, chain_ao_hi, chain_bo_lo, chain_bo_hi) = (
        chain_a_lo.clone(),
        chain_a_hi.clone(),
        chain_b_lo.clone(),
        chain_b_hi.clone(),
    );

    // A strip face's natural wire: the region [u_s, u_e] × [v_lo, v_hi]
    // traversed CCW in (u, v) — kept on the left viewed from the
    // natural outward normal r̂ = ∂u × ∂v. The chain runs P→Q; whether
    // that increases or decreases u depends on the sweep's sign.
    let strip_wire = |chain_lo: &ArcChain, chain_hi: &ArcChain, sweep: f64| -> Vec<WireEdge> {
        if sweep > 0.0 {
            let mut w = chain_uses(chain_lo, true);
            w.push(WireEdge {
                edge: e_vq,
                forward: true,
            });
            w.extend(chain_uses(chain_hi, false));
            w.push(WireEdge {
                edge: e_vp,
                forward: false,
            });
            w
        } else {
            let mut w = chain_uses(chain_lo, false);
            w.push(WireEdge {
                edge: e_vp,
                forward: true,
            });
            w.extend(chain_uses(chain_hi, true));
            w.push(WireEdge {
                edge: e_vq,
                forward: false,
            });
            w
        }
    };
    // A cap face's wire: [first arc chain CCW, second arc chain CW]
    // with the region (kept) on the left viewed from the cap's outward
    // normal. The CCW traversal of a chain is its stored direction iff
    // its sweep is positive.
    let cap_wire = |first: &ArcChain,
                    second: &ArcChain,
                    s1: f64,
                    s2: f64,
                    second_ccw: bool|
     -> Vec<WireEdge> {
        // Both chains in their CCW direction keep an inside-disk
        // region on the left; the cut's cap (the disk minus the lens)
        // lies OUTSIDE b on b's arc, so that chain runs CW there.
        let mut w = chain_uses(first, s1 > 0.0);
        w.extend(chain_uses(
            second,
            if second_ccw { s2 > 0.0 } else { s2 <= 0.0 },
        ));
        w
    };
    let reversed_wire = |w: &[WireEdge]| -> Vec<WireEdge> {
        w.iter()
            .rev()
            .map(|we| WireEdge {
                edge: we.edge,
                forward: !we.forward,
            })
            .collect()
    };

    let uv_of_strip = |theta: f64, sweep: f64| -> [f64; 4] {
        [theta + sweep.min(0.0), theta + sweep.max(0.0), v_lo, v_hi]
    };

    // The blind drill's own pieces: a's rim circles at its caps and
    // the full-height seam (the K1 mantle pattern of the outer wire).
    let mut faces: Vec<TopoFace> = Vec::new();
    if blind_drill {
        let rim = |v: f64| Curve::Circle {
            center: center_on(o1, v),
            axis: a,
            ref_dir: x_hat,
            radius: r1,
        };
        let rim_lo = rim(0.0);
        let rim_hi = rim(h1);
        let va_lo = rim_lo.point(0.0);
        let va_hi = rim_hi.point(0.0);
        let iv_lo = vertices.len();
        vertices.push(TopoVertex { point: va_lo });
        let iv_hi = vertices.len();
        vertices.push(TopoVertex { point: va_hi });
        edges.push(TopoEdge {
            curve: rim_lo,
            range: (0.0, std::f64::consts::TAU),
            start: iv_lo,
            end: iv_lo,
        });
        let e_rim_lo = edges.len() - 1;
        edges.push(TopoEdge {
            curve: rim_hi,
            range: (0.0, std::f64::consts::TAU),
            start: iv_hi,
            end: iv_hi,
        });
        let e_rim_hi = edges.len() - 1;
        edges.push(seam_between(va_lo, va_hi, iv_lo, iv_hi));
        let e_seam = edges.len() - 1;

        // The notched mantle: the outer wire the K1 pattern, the hole
        // wire the lens window (reversed — the region lies outside it).
        let hole = reversed_wire(&strip_wire(&chain_a_lo, &chain_a_hi, sweep_a_in));
        faces.push(TopoFace {
            surface: surface_a,
            uv_bounds: [0.0, std::f64::consts::TAU, 0.0, h1],
            wires: vec![
                TopoWire {
                    edges: vec![
                        WireEdge {
                            edge: e_seam,
                            forward: true,
                        },
                        WireEdge {
                            edge: e_rim_hi,
                            forward: false,
                        },
                        WireEdge {
                            edge: e_seam,
                            forward: false,
                        },
                        WireEdge {
                            edge: e_rim_lo,
                            forward: true,
                        },
                    ],
                },
                TopoWire { edges: hole },
            ],
            reversed: false,
        });
        // b's wall (reversed — the solid outside b).
        let wall = reversed_wire(&strip_wire(&chain_b_lo, &chain_b_hi, sweep_b_in));
        faces.push(TopoFace {
            surface: surface_b,
            uv_bounds: uv_of_strip(ph_p, sweep_b_in),
            wires: vec![TopoWire { edges: wall }],
            reversed: true,
        });
        // b's lens caps at its ends: the solid lies below the bottom
        // one (outward +a) and above the top one (outward −a).
        let lens_lo = cap_wire(&chain_a_lo, &chain_b_lo, sweep_a_in, sweep_b_in, true);
        let lens_hi = cap_wire(&chain_a_hi, &chain_b_hi, sweep_a_in, sweep_b_in, true);
        faces.push(plane_face_at(
            center_on(o1, v_lo),
            a,
            x_hat,
            &lens_lo,
            (&chain_a_lo, &chain_b_lo),
        ));
        faces.push(plane_face_at(
            center_on(o1, v_hi),
            v3(-a.x, -a.y, -a.z),
            x_hat,
            &reversed_wire(&lens_hi),
            (&chain_a_hi, &chain_b_hi),
        ));
        // a's full caps.
        faces.push(TopoFace {
            surface: Surface::Plane {
                origin: o1,
                normal: v3(-a.x, -a.y, -a.z),
                x_dir: x_hat,
            },
            uv_bounds: [-r1, r1, -r1, r1],
            wires: vec![TopoWire {
                edges: vec![WireEdge {
                    edge: e_rim_lo,
                    forward: false,
                }],
            }],
            reversed: false,
        });
        faces.push(TopoFace {
            surface: Surface::Plane {
                origin: center_on(o1, h1),
                normal: a,
                x_dir: x_hat,
            },
            uv_bounds: [-r1, r1, -r1, r1],
            wires: vec![TopoWire {
                edges: vec![WireEdge {
                    edge: e_rim_hi,
                    forward: true,
                }],
            }],
            reversed: false,
        });
        let brep = ExactBRep {
            vertices,
            edges,
            faces,
        };
        brep.validate()
            .map_err(|e| format!("the composed drill failed validation: {e}"))?;
        return Ok(BooleanOutcome {
            kind: BooleanOutcomeKind::Solid(brep),
        });
    }
    match op {
        BooleanOp::Common => {
            faces.push(TopoFace {
                surface: surface_a,
                uv_bounds: uv_of_strip(th_p, sweep_a_in),
                wires: vec![TopoWire {
                    edges: strip_wire(&chain_a_lo, &chain_a_hi, sweep_a_in),
                }],
                reversed: false,
            });
            faces.push(TopoFace {
                surface: surface_b,
                uv_bounds: uv_of_strip(ph_p, sweep_b_in),
                wires: vec![TopoWire {
                    edges: strip_wire(&chain_b_lo, &chain_b_hi, sweep_b_in),
                }],
                reversed: false,
            });
            // Top cap (+a): the lens — a's inside arc CCW, b's inside
            // arc CW (the region lies inside both disks).
            let top = cap_wire(&chain_a_hi, &chain_b_hi, sweep_a_in, sweep_b_in, true);
            let bottom = reversed_wire(&cap_wire(
                &chain_a_lo,
                &chain_b_lo,
                sweep_a_in,
                sweep_b_in,
                true,
            ));
            faces.push(plane_face_at(
                center_on(o1, v_hi),
                a,
                x_hat,
                &top,
                (&chain_a_hi, &chain_b_hi),
            ));
            faces.push(plane_face_at(
                center_on(o1, v_lo),
                v3(-a.x, -a.y, -a.z),
                x_hat,
                &bottom,
                (&chain_a_lo, &chain_b_lo),
            ));
        }
        BooleanOp::Cut => {
            faces.push(TopoFace {
                surface: surface_a,
                uv_bounds: uv_of_strip(th_p, sweep_a_out),
                wires: vec![TopoWire {
                    edges: strip_wire(&chain_ao_lo, &chain_ao_hi, sweep_a_out),
                }],
                reversed: false,
            });
            // b's inside-arc wall: the same patch as the common's
            // b-strip, but the solid lies OUTSIDE b — the reversed
            // orientation: the traversal runs against the outward
            // −r̂_b (the loop reversed, each flag flipped), and
            // `reversed` tells the reader which normal that is.
            let wall = reversed_wire(&strip_wire(&chain_b_lo, &chain_b_hi, sweep_b_in));
            faces.push(TopoFace {
                surface: surface_b,
                uv_bounds: uv_of_strip(ph_p, sweep_b_in),
                wires: vec![TopoWire { edges: wall }],
                reversed: true,
            });
            // Top cap (+a): disk a minus the lens — a's outside arc
            // CCW, b's inside arc CW.
            let top = cap_wire(&chain_ao_hi, &chain_b_hi, sweep_a_out, sweep_b_in, false);
            let bottom = reversed_wire(&cap_wire(
                &chain_ao_lo,
                &chain_b_lo,
                sweep_a_out,
                sweep_b_in,
                false,
            ));
            faces.push(plane_face_at(
                center_on(o1, v_hi),
                a,
                x_hat,
                &top,
                (&chain_ao_hi, &chain_b_hi),
            ));
            faces.push(plane_face_at(
                center_on(o1, v_lo),
                v3(-a.x, -a.y, -a.z),
                x_hat,
                &bottom,
                (&chain_ao_lo, &chain_b_lo),
            ));
        }
        BooleanOp::Fuse => {
            faces.push(TopoFace {
                surface: surface_a,
                uv_bounds: uv_of_strip(th_p, sweep_a_out),
                wires: vec![TopoWire {
                    edges: strip_wire(&chain_ao_lo, &chain_ao_hi, sweep_a_out),
                }],
                reversed: false,
            });
            faces.push(TopoFace {
                surface: surface_b,
                uv_bounds: uv_of_strip(ph_p, sweep_b_out),
                wires: vec![TopoWire {
                    edges: strip_wire(&chain_bo_lo, &chain_bo_hi, sweep_b_out),
                }],
                reversed: false,
            });
            // Top cap (+a): the union — a's outside arc CCW, b's
            // outside arc CW.
            let top = cap_wire(&chain_ao_hi, &chain_bo_hi, sweep_a_out, sweep_b_out, true);
            let bottom = reversed_wire(&cap_wire(
                &chain_ao_lo,
                &chain_bo_lo,
                sweep_a_out,
                sweep_b_out,
                true,
            ));
            faces.push(plane_face_at(
                center_on(o1, v_hi),
                a,
                x_hat,
                &top,
                (&chain_ao_hi, &chain_bo_hi),
            ));
            faces.push(plane_face_at(
                center_on(o1, v_lo),
                v3(-a.x, -a.y, -a.z),
                x_hat,
                &bottom,
                (&chain_ao_lo, &chain_bo_lo),
            ));
        }
    }

    let brep = ExactBRep {
        vertices,
        edges,
        faces,
    };
    brep.validate()
        .map_err(|e| format!("the composed cylinder boolean failed validation: {e}"))?;
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

/// The nested cut — b's cross-section strictly inside a's, b's
/// z-range covering a's: the tube. a's mantle (natural), b's mantle
/// (reversed — the solid lies outside b), and the annular caps
/// (multi-wire plane faces: a's rim outside, b's rim as the hole).
fn nested_tube(o1: Vector3, a: Vector3, r1: f64, h1: f64, o2: Vector3, r2: f64) -> ExactBRep {
    let x_hat = any_perpendicular(a);
    let top = v3(o1.x + a.x * h1, o1.y + a.y * h1, o1.z + a.z * h1);
    // b's rim circles sit on b's axis at a's cap planes: the points
    // (o2 + a·t) with a·(o2 + a·t − o1) ∈ {0, h1}, i.e. t ∈ {−s2, h1 − s2}.
    let s2 = v3(o2.x - o1.x, o2.y - o1.y, o2.z - o1.z).dot(&a);
    let b_at = |t: f64| v3(o2.x + a.x * t, o2.y + a.y * t, o2.z + a.z * t);
    let rim = |o: Vector3, r: f64| Curve::Circle {
        center: o,
        axis: a,
        ref_dir: x_hat,
        radius: r,
    };
    let ca_lo = rim(o1, r1);
    let ca_hi = rim(top, r1);
    let cb_lo = rim(b_at(-s2), r2);
    let cb_hi = rim(b_at(h1 - s2), r2);
    let va = (ca_lo.point(0.0), ca_hi.point(0.0));
    let vb = (cb_lo.point(0.0), cb_hi.point(0.0));
    let edges = vec![
        TopoEdge {
            curve: ca_lo,
            range: (0.0, std::f64::consts::TAU),
            start: 0,
            end: 0,
        },
        TopoEdge {
            curve: ca_hi,
            range: (0.0, std::f64::consts::TAU),
            start: 1,
            end: 1,
        },
        seam_between(va.0, va.1, 0, 1),
        TopoEdge {
            curve: cb_lo,
            range: (0.0, std::f64::consts::TAU),
            start: 2,
            end: 2,
        },
        TopoEdge {
            curve: cb_hi,
            range: (0.0, std::f64::consts::TAU),
            start: 3,
            end: 3,
        },
        seam_between(vb.0, vb.1, 2, 3),
    ];
    let cylinder = |o: Vector3, r: f64| Surface::Cylinder {
        origin: o,
        axis: a,
        ref_dir: x_hat,
        radius: r,
    };
    // a's mantle (the K1 pattern: seam up, top rim, seam down, bottom
    // rim — kept on the left against the natural outward r̂).
    let mantle = |rim_lo: usize, rim_hi: usize, seam: usize| -> Vec<WireEdge> {
        vec![
            WireEdge {
                edge: seam,
                forward: true,
            },
            WireEdge {
                edge: rim_hi,
                forward: false,
            },
            WireEdge {
                edge: seam,
                forward: false,
            },
            WireEdge {
                edge: rim_lo,
                forward: true,
            },
        ]
    };
    let faces = vec![
        TopoFace {
            surface: cylinder(o1, r1),
            uv_bounds: [0.0, std::f64::consts::TAU, 0.0, h1],
            wires: vec![TopoWire {
                edges: mantle(0, 1, 2),
            }],
            reversed: false,
        },
        // b's mantle reversed: the same patch with the solid on the
        // outside — the loop reversed (list reversed, flags flipped)
        // against the outward −r̂_b. The wall spans b's own parameters
        // [−s2, h1 − s2] (a's range in b's frame).
        TopoFace {
            surface: cylinder(o2, r2),
            uv_bounds: [0.0, std::f64::consts::TAU, -s2, h1 - s2],
            wires: vec![TopoWire {
                edges: mantle(3, 4, 5)
                    .iter()
                    .rev()
                    .map(|we| WireEdge {
                        edge: we.edge,
                        forward: !we.forward,
                    })
                    .collect(),
            }],
            reversed: true,
        },
        // The annular caps: a's rim outside, b's rim as the hole.
        TopoFace {
            surface: Surface::Plane {
                origin: top,
                normal: a,
                x_dir: x_hat,
            },
            uv_bounds: [-r1, r1, -r1, r1],
            wires: vec![
                TopoWire {
                    edges: vec![WireEdge {
                        edge: 1,
                        forward: true,
                    }],
                },
                TopoWire {
                    edges: vec![WireEdge {
                        edge: 4,
                        forward: false,
                    }],
                },
            ],
            reversed: false,
        },
        TopoFace {
            surface: Surface::Plane {
                origin: o1,
                normal: v3(-a.x, -a.y, -a.z),
                x_dir: x_hat,
            },
            uv_bounds: [-r1, r1, -r1, r1],
            wires: vec![
                TopoWire {
                    edges: vec![WireEdge {
                        edge: 0,
                        forward: false,
                    }],
                },
                TopoWire {
                    edges: vec![WireEdge {
                        edge: 3,
                        forward: true,
                    }],
                },
            ],
            reversed: false,
        },
    ];
    ExactBRep {
        vertices: vec![
            TopoVertex { point: va.0 },
            TopoVertex { point: va.1 },
            TopoVertex { point: vb.0 },
            TopoVertex { point: vb.1 },
        ],
        edges,
        faces,
    }
}

/// One arc chain: the rational-quadratic segments of a circular arc
/// from the canonical start point to the canonical end point, with
/// the junction vertices between them.
#[derive(Clone)]
struct ArcChain {
    /// The edge indices, in stored order (start → end).
    edges: Vec<usize>,
    /// Every segment's control points (for the cap UV boxes: the
    /// rational arc lies in its control hull, positive weights).
    control_points: Vec<Vector3>,
}

/// Build one arc chain: `n = ceil(|sweep| / (π/2))` equal rational
/// quadratic segments along the circle (center `c`, radius `r`, frame
/// `(x_hat, y_hat)`) from the canonical `p_start` to the canonical
/// `p_end` with the signed `sweep` (positive = CCW around the frame
/// axis). The first control point of the first segment and the last of
/// the last are the canonical points themselves — the bit-exact
/// vertex contract — and every segment's de Boor evaluation returns
/// its endpoints exactly; the curve lies on the circle to machine
/// scale (the classical NURBS arc: tangent-intersection middle control
/// point, weight cos(sweep/2)).
#[allow(clippy::too_many_arguments)] // the circle frame, the canonical ends, the sweep, the vertex indices
fn push_arc_chain(
    vertices: &mut Vec<TopoVertex>,
    edges: &mut Vec<TopoEdge>,
    center: Vector3,
    x_hat: Vector3,
    y_hat: Vector3,
    radius: f64,
    p_start: Vector3,
    p_end: Vector3,
    theta_start: f64,
    sweep: f64,
    v_start: usize,
    v_end: usize,
) -> ArcChain {
    let n = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let seg = sweep / n as f64;
    let pt = |th: f64| {
        let (s, c) = th.sin_cos();
        v3(
            center.x + radius * (c * x_hat.x + s * y_hat.x),
            center.y + radius * (c * x_hat.y + s * y_hat.y),
            center.z + radius * (c * x_hat.z + s * y_hat.z),
        )
    };
    // The junction points: canonical ends, exact circle evaluations
    // between.
    let mut pts = Vec::with_capacity(n + 1);
    for j in 0..=n {
        let p = if j == 0 {
            p_start
        } else if j == n {
            p_end
        } else {
            pt(theta_start + seg * j as f64)
        };
        pts.push(p);
    }
    // The junction vertices: the canonical endpoints reuse their
    // existing indices (the crossing generators' vertices); only the
    // interior junctions are new.
    let v_new = vertices.len();
    for p in &pts[1..n] {
        vertices.push(TopoVertex { point: *p });
    }
    let index_of = |j: usize| -> usize {
        if j == 0 {
            v_start
        } else if j == n {
            v_end
        } else {
            v_new + j - 1
        }
    };
    let e0 = edges.len();
    // The frame's axis (x̂ × ŷ = ±a): the rotation side of the sweep.
    let frame_axis = x_hat.cross(&y_hat);
    let n_s = if sweep >= 0.0 {
        frame_axis
    } else {
        v3(-frame_axis.x, -frame_axis.y, -frame_axis.z)
    };
    let mut control_points = Vec::with_capacity(3 * n);
    for j in 0..n {
        let (p_a, p_b) = (pts[j], pts[j + 1]);
        // The tangent of CCW motion around n_s at a point is n_s × r̂,
        // with r̂ the unit radial from the center (robust at the
        // canonical endpoints, which sit on the circle to machine
        // scale only).
        let tangent = |p: Vector3| -> Vector3 {
            let u = v3(p.x - center.x, p.y - center.y, p.z - center.z);
            let u = u * (1.0 / u.length());
            n_s.cross(&u)
        };
        let t_a = tangent(p_a);
        let t_b = tangent(p_b);
        // The tangent intersection p_a + s·t_a = p_b + t·t_b (both
        // tangent lines lie in the arc's plane; Cramer in the plane's
        // normal n_s).
        let d = v3(p_b.x - p_a.x, p_b.y - p_a.y, p_b.z - p_a.z);
        let denom = t_a.cross(&t_b).dot(&n_s);
        let s = d.cross(&t_b).dot(&n_s) / denom;
        let m = v3(p_a.x + t_a.x * s, p_a.y + t_a.y * s, p_a.z + t_a.z * s);
        let w = (seg / 2.0).cos();
        control_points.extend_from_slice(&[p_a, m, p_b]);
        edges.push(TopoEdge {
            curve: Curve::Bspline {
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                points: vec![p_a, m, p_b],
                weights: Some(vec![1.0, w, 1.0]),
            },
            range: (0.0, 1.0),
            start: index_of(j),
            end: index_of(j + 1),
        });
    }
    ArcChain {
        edges: (e0..e0 + n).collect(),
        control_points,
    }
}

/// The wire-edge uses of a chain: forward (the stored direction) or
/// reversed (the list backwards, each edge flipped).
fn chain_uses(chain: &ArcChain, forward: bool) -> Vec<WireEdge> {
    if forward {
        chain
            .edges
            .iter()
            .map(|&e| WireEdge {
                edge: e,
                forward: true,
            })
            .collect()
    } else {
        chain
            .edges
            .iter()
            .rev()
            .map(|&e| WireEdge {
                edge: e,
                forward: false,
            })
            .collect()
    }
}

/// A planar cap face on the cutting plane through `origin` with
/// `normal` (unit) — the wire `w` plus a UV box that contains the arc
/// chains' control polygons (the rational arc lies in its control
/// hull: convex combinations for positive weights, so the box is
/// guaranteed to contain the face's trim).
fn plane_face_at(
    origin: Vector3,
    normal: Vector3,
    x_dir: Vector3,
    w: &[WireEdge],
    chains: (&ArcChain, &ArcChain),
) -> TopoFace {
    let y_dir = normal.cross(&x_dir);
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for chain in [chains.0, chains.1] {
        // The control hull contains the arc (positive weights), so the
        // box over the control points contains the face's trim.
        for p in &chain.control_points {
            let off = v3(p.x - origin.x, p.y - origin.y, p.z - origin.z);
            let (u, v) = (off.dot(&x_dir), off.dot(&y_dir));
            lo[0] = lo[0].min(u);
            hi[0] = hi[0].max(u);
            lo[1] = lo[1].min(v);
            hi[1] = hi[1].max(v);
        }
    }
    TopoFace {
        surface: Surface::Plane {
            origin,
            normal,
            x_dir,
        },
        uv_bounds: [lo[0] - 1e-9, hi[0] + 1e-9, lo[1] - 1e-9, hi[1] + 1e-9],
        wires: vec![TopoWire { edges: w.to_vec() }],
        reversed: false,
    }
}

// ---------------------------------------------------------------------------
// K3e — cylinder×sphere: the coaxial family, the capsule cuts
// ---------------------------------------------------------------------------
//
// The sphere's center on the cylinder's axis line (a sub-machine
// perpendicular offset — the arithmetic residue of a placement — reads
// as the intended coaxial, the K3d precedent; anything larger
// propagates K3a's own quartic refusal by name). Everything follows
// the cross-section profile `min(r_c, s(z))` over the z-overlap, with
// `s(z) = √(r_s² − (z − z_s)²)` the sphere's disk radius at height
// `z_s + z` and `h_i = √(r_s² − r_c²)` the section offset (0 when the
// sphere is narrower — its section zone `[z_s − h_i, z_s + h_i]`
// degenerates to a point, and the sphere never swallows the cylinder's
// disk):
//
// * **Common** — the stacked profile: the capsule (the cylinder band
//   between the two K3a section circles, joined to the spherical caps;
//   a tangent C¹ junction, the smooth spherocylinder), the
//   plane-clipped variants (a sphere band between the z=0 mouth and
//   the section, carried as ONE wire of four legs — seam, circle,
//   seam back, circle — with the seam the classical NURBS meridian
//   arc, exact at both ends, on the sphere by construction; plus the
//   planar end disks), the slab-clipped sphere (the sphere narrower:
//   the cap, the slice), or the whole cylinder (the zone covering the
//   slab — the sphere's disk swallows the cylinder's everywhere on
//   it).
// * **Cut** (cylinder − sphere) — the material profile over the slab:
//   full where `|z − z_s| ≥ r_s`, annulus where `h_i ≤ |z − z_s| <
//   r_s`, empty inside the section zone. The one-piece shapes: the
//   **rim-bowl** (the sphere dipping into one cap plane with the
//   mouth circle strictly inside the rim: the full mantle, the far
//   cap, the near annular cap, and the reversed bowl — the sphere's
//   part from the pole to the mouth, a single-wire cap), the
//   **pinch-bowl** (the sphere wider, the section circle inside the
//   slab: the piece `[0, z_s − h_i)` pinches off at the section —
//   three faces, no cap face at the pinch), the **groove** (the
//   sphere's reach covering the slab: the mantle, two annular caps,
//   and the reversed spherical inner wall — a solid torus reading
//   `V − E_nd + F = 2` with the caps' holes), the empty (the zone
//   covering the slab), and the whole cylinder (no reach).
// * **Fuse** — the dome bumps (the sphere crossing a cap plane with
//   the mouth strictly inside the rim and the mantle intact — the
//   zone clear of the slab: the full mantle, the caps, and per
//   crossing an annulus plus the dome — the sphere's part beyond the
//   plane to the pole, a single-wire cap, naturally oriented), the
//   whole cylinder (the sphere contained), the whole sphere (the zone
//   covering the slab).
//
// Honest refusals, by name, never approximated: the non-coaxial
// quartic (K3a's own, propagated), the contained cut (the internal
// void — a second shell), the two-piece cut (the zone strictly inside
// the slab splits the material), the collar (the one-piece material
// pinching inside the slab with the sphere's reach past the far plane
// — a genus-1 piece), the split-circle mantles (the fuse with the zone
// partially on the slab), the rim-mount (a plane-cut mouth coinciding
// with a rim circle, `|plane − z_s| = h_i`), the equator tangency
// (equal radii with the equator inside the slab — the sphere touches
// the mantle along a circle, a measure-zero non-manifold contact),
// and the tangent/disjoint fuses.

/// The K3a plane×sphere mouth circle at the plane through `p` (normal
/// `a`), re-parameterized to the shared frame — center and radius
/// bit-identical to K3a's construction; `None` when the plane misses
/// (or is tangent to) the sphere.
fn sphere_plane_mouth(
    sph: &Surface,
    p: Vector3,
    a: Vector3,
    x_hat: Vector3,
) -> Result<Option<Curve>, String> {
    let plane = Surface::Plane {
        origin: p,
        normal: a,
        x_dir: x_hat,
    };
    match intersect_surfaces(sph, &plane) {
        SurfaceIntersection::Curves(cs) if cs.len() == 1 => match &cs[0] {
            Curve::Circle { center, radius, .. } => Ok(Some(Curve::Circle {
                center: *center,
                axis: a,
                ref_dir: x_hat,
                radius: *radius,
            })),
            other => Err(format!(
                "K3a returned a non-circle plane×sphere section: {other:?}"
            )),
        },
        SurfaceIntersection::Empty => Ok(None),
        other => Err(format!(
            "the K3a plane×sphere section is missing: {other:?}"
        )),
    }
}

/// Append a closed circle edge with its own seam vertex; the edge index.
fn push_circle(vertices: &mut Vec<TopoVertex>, edges: &mut Vec<TopoEdge>, c: Curve) -> usize {
    let vi = vertices.len();
    vertices.push(TopoVertex {
        point: c.point(0.0),
    });
    edges.push(TopoEdge {
        curve: c,
        range: (0.0, std::f64::consts::TAU),
        start: vi,
        end: vi,
    });
    edges.len() - 1
}

/// The cylinder strip face between two same-radius circles (the
/// K1 mantle pattern: seam up, upper circle back, seam down, lower
/// circle forward — kept on the left against the outward natural
/// normal). `reversed` mirrors the loop and flips the flag (the solid
/// outside the cylinder — the nested-tube wall precedent).
fn cylinder_strip_face(
    surface: Surface,
    e_lo: usize,
    e_hi: usize,
    seam: usize,
    v_lo: f64,
    v_hi: f64,
    reversed: bool,
) -> TopoFace {
    // The natural loop: seam up, upper back, seam down, lower
    // forward. The reversed face carries the same patch with the solid
    // on the outside: the loop reversed (list reversed, flags flipped).
    let natural = vec![
        WireEdge {
            edge: seam,
            forward: true,
        },
        WireEdge {
            edge: e_hi,
            forward: false,
        },
        WireEdge {
            edge: seam,
            forward: false,
        },
        WireEdge {
            edge: e_lo,
            forward: true,
        },
    ];
    let edges = if reversed {
        natural
            .iter()
            .rev()
            .map(|we| WireEdge {
                edge: we.edge,
                forward: !we.forward,
            })
            .collect()
    } else {
        natural
    };
    TopoFace {
        surface,
        uv_bounds: [0.0, std::f64::consts::TAU, v_lo, v_hi],
        wires: vec![TopoWire { edges }],
        reversed,
    }
}

/// The sphere band face between two latitude circles, carried as one
/// four-leg wire — the meridian seam chain (the NURBS arcs, exact at
/// both ends), the upper circle back, the seam chain back, the lower
/// circle forward (kept on the left against the outward natural
/// normal; the cylinder-strip pattern transplanted to the sphere).
/// `reversed` mirrors the loop and flips the flag.
fn sphere_band_face(
    surface: Surface,
    chain: &ArcChain,
    e_lo: usize,
    e_hi: usize,
    lat_lo: f64,
    lat_hi: f64,
    reversed: bool,
) -> TopoFace {
    // The natural loop: the seam chain up, the upper circle back, the
    // chain down, the lower circle forward. The reversed face carries
    // the same patch with the solid on the outside: the loop reversed
    // (list reversed, flags flipped).
    let mut natural = chain_uses(chain, true);
    natural.push(WireEdge {
        edge: e_hi,
        forward: false,
    });
    natural.extend(chain_uses(chain, false));
    natural.push(WireEdge {
        edge: e_lo,
        forward: true,
    });
    let edges = if reversed {
        natural
            .iter()
            .rev()
            .map(|we| WireEdge {
                edge: we.edge,
                forward: !we.forward,
            })
            .collect()
    } else {
        natural
    };
    TopoFace {
        surface,
        uv_bounds: [0.0, std::f64::consts::TAU, lat_lo, lat_hi],
        wires: vec![TopoWire { edges }],
        reversed,
    }
}

/// A single-wire spherical cap face — the region from the boundary
/// circle to its pole (the sphere_plane pattern; the pole is an
/// unrepresented degenerate boundary). `forward` follows the
/// kept-on-the-left convention against the face's OUTWARD normal.
fn sphere_cap_face(
    surface: Surface,
    e_circle: usize,
    lat: f64,
    pole_lat: f64,
    forward: bool,
    reversed: bool,
) -> TopoFace {
    TopoFace {
        surface,
        uv_bounds: [
            0.0,
            std::f64::consts::TAU,
            lat.min(pole_lat),
            lat.max(pole_lat),
        ],
        wires: vec![TopoWire {
            edges: vec![WireEdge {
                edge: e_circle,
                forward,
            }],
        }],
        reversed,
    }
}

/// A planar disk face (one circle, one wire) on the circle's radius.
/// `outward` is the stored normal; the traversal keeps the region on
/// the left viewed from it.
fn plane_disk_face(
    origin: Vector3,
    outward: Vector3,
    x_dir: Vector3,
    e_circle: usize,
    radius: f64,
    forward: bool,
) -> TopoFace {
    TopoFace {
        surface: Surface::Plane {
            origin,
            normal: outward,
            x_dir,
        },
        uv_bounds: [-radius, radius, -radius, radius],
        wires: vec![TopoWire {
            edges: vec![WireEdge {
                edge: e_circle,
                forward,
            }],
        }],
        reversed: false,
    }
}

/// A planar annular cap: the outer wire and the hole wire, traversed
/// opposite (the nested-tube cap pattern). At the base plane
/// (`outward = −a`) the outer runs backward; at the top it runs
/// forward — the `outer_forward` flag.
fn plane_annulus_face(
    origin: Vector3,
    outward: Vector3,
    x_dir: Vector3,
    e_outer: usize,
    e_inner: usize,
    r_outer: f64,
    outer_forward: bool,
) -> TopoFace {
    TopoFace {
        surface: Surface::Plane {
            origin,
            normal: outward,
            x_dir,
        },
        uv_bounds: [-r_outer, r_outer, -r_outer, r_outer],
        wires: vec![
            TopoWire {
                edges: vec![WireEdge {
                    edge: e_outer,
                    forward: outer_forward,
                }],
            },
            TopoWire {
                edges: vec![WireEdge {
                    edge: e_inner,
                    forward: !outer_forward,
                }],
            },
        ],
        reversed: false,
    }
}

/// The latitude of a latitude circle of the sphere centered `c_s`
/// (axis `a`): `atan2(offset·a, radius)`, from the circle's own data.
fn latitude_of(c: &Curve, c_s: Vector3, a: Vector3) -> f64 {
    match c {
        Curve::Circle { center, radius, .. } => {
            let off = v3(center.x - c_s.x, center.y - c_s.y, center.z - c_s.z);
            off.dot(&a).atan2(*radius)
        }
        _ => unreachable!("latitude_of on a non-circle"),
    }
}

/// The height of a circle's center above the base (the v parameter in
/// the cylinder's frame).
fn v_param(c: &Curve, o_c: Vector3, a: Vector3) -> f64 {
    match c {
        Curve::Circle { center, .. } => {
            v3(center.x - o_c.x, center.y - o_c.y, center.z - o_c.z).dot(&a)
        }
        _ => unreachable!("v_param on a non-circle"),
    }
}

/// The meridian seam chain between two latitude circles of the sphere
/// (the NURBS arcs on the great circle at azimuth 0 — the circles'
/// shared `ref_dir`): from the lower circle's vertex to the upper's.
#[allow(clippy::too_many_arguments)] // the sphere frame, the two circles, the vertex indices
fn meridian_chain(
    vertices: &mut Vec<TopoVertex>,
    edges: &mut Vec<TopoEdge>,
    c_s: Vector3,
    a: Vector3,
    x_hat: Vector3,
    r_s: f64,
    lo: &Curve,
    hi: &Curve,
    v_lo: usize,
    v_hi: usize,
) -> ArcChain {
    let lat_lo = latitude_of(lo, c_s, a);
    let lat_hi = latitude_of(hi, c_s, a);
    push_arc_chain(
        vertices,
        edges,
        c_s,
        x_hat,
        a,
        r_s,
        lo.point(0.0),
        hi.point(0.0),
        lat_lo,
        lat_hi - lat_lo,
        v_lo,
        v_hi,
    )
}

/// The Common composition: the stacked min-profile over the z-overlap
/// — the capsule family (see the module doc).
#[allow(clippy::too_many_arguments)] // the operand frames and the profile parameters
fn capsule_stack(
    o_c: Vector3,
    a: Vector3,
    x_hat: Vector3,
    r_c: f64,
    h_c: f64,
    c_s: Vector3,
    r_s: f64,
    z_s: f64,
    h_i: f64,
) -> Result<ExactBRep, String> {
    let at = |z: f64| v3(o_c.x + a.x * z, o_c.y + a.y * z, o_c.z + a.z * z);
    let sph = || Surface::Sphere {
        center: c_s,
        axis: a,
        ref_dir: x_hat,
        radius: r_s,
    };
    let rim = |z: f64| Curve::Circle {
        center: at(z),
        axis: a,
        ref_dir: x_hat,
        radius: r_c,
    };
    let sect = |h: f64| Curve::Circle {
        center: v3(c_s.x + a.x * h, c_s.y + a.y * h, c_s.z + a.z * h),
        axis: a,
        ref_dir: x_hat,
        radius: r_c,
    };

    // The profile's z-range.
    let z_lo = (z_s - r_s).max(0.0);
    let z_hi = (z_s + r_s).min(h_c);
    // The zone (where the sphere's disk swallows the cylinder's)
    // straddles a plane iff the section heights bracket it. A point
    // zone (h_i = 0, the sphere narrower) never does.
    let zone_at_base =
        h_i > 0.0 && exact_sum_sign(&[z_s, -h_i]) <= 0 && exact_sum_sign(&[z_s, h_i]) >= 0;
    let zone_at_top = h_i > 0.0
        && exact_sum_sign(&[z_s, -h_i, -h_c]) <= 0
        && exact_sum_sign(&[z_s, h_i, -h_c]) >= 0;

    // The ends: a planar disk (the profile cut by the plane) or the
    // sphere's pole (the profile's own apex, an unrepresented
    // degenerate boundary).
    let disk_at_base = z_lo == 0.0 && exact_sum_sign(&[z_s, -r_s]) < 0;
    let disk_at_top = z_hi == h_c && exact_sum_sign(&[z_s, r_s, -h_c]) > 0;

    // The end circles: the rim where the zone reaches the plane (the
    // profile is the cylinder's there), the K3a mouth where the
    // sphere's part is.
    let end_lo = if disk_at_base {
        Some(if zone_at_base {
            rim(0.0)
        } else {
            sphere_plane_mouth(&sph(), at(0.0), a, x_hat)?.expect("the sphere crosses the base")
        })
    } else {
        None
    };
    let end_hi = if disk_at_top {
        Some(if zone_at_top {
            rim(h_c)
        } else {
            sphere_plane_mouth(&sph(), at(h_c), a, x_hat)?.expect("the sphere crosses the top")
        })
    } else {
        None
    };

    // The levels: the boundary circles in z-order (the section joins
    // exist only for a real zone — a point zone never interrupts the
    // spherical profile).
    let sect_lo = if h_i > 0.0 && z_s - h_i > z_lo && z_s - h_i < z_hi {
        Some(sect(-h_i))
    } else {
        None
    };
    let sect_hi = if h_i > 0.0 && z_s + h_i > z_lo && z_s + h_i < z_hi {
        Some(sect(h_i))
    } else {
        None
    };

    // ---- assemble ----------------------------------------------------
    let mut vertices: Vec<TopoVertex> = Vec::new();
    let mut edges: Vec<TopoEdge> = Vec::new();
    let mut faces: Vec<TopoFace> = Vec::new();

    let e_end_lo = end_lo
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));
    let e_sect_lo = sect_lo
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));
    let e_sect_hi = sect_hi
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));
    let e_end_hi = end_hi
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));

    // The segments between consecutive levels, bottom to top; the kind
    // by the midpoint against the zone.
    let mut bounds: Vec<(f64, Option<(usize, Curve)>)> = Vec::new();
    bounds.push((
        z_lo,
        e_end_lo.map(|e| (e, end_lo.clone().expect("the circle"))),
    ));
    if let (Some(e), Some(c)) = (e_sect_lo, sect_lo.clone()) {
        bounds.push((z_s - h_i, Some((e, c))));
    }
    if let (Some(e), Some(c)) = (e_sect_hi, sect_hi.clone()) {
        bounds.push((z_s + h_i, Some((e, c))));
    }
    bounds.push((
        z_hi,
        e_end_hi.map(|e| (e, end_hi.clone().expect("the circle"))),
    ));

    for w in bounds.windows(2) {
        let (z0, lo) = &w[0];
        let (z1, hi) = &w[1];
        let mid = 0.5 * (z0 + z1);
        let is_cylinder = h_i > 0.0 && (mid - z_s).abs() < h_i;
        if is_cylinder {
            // The cylinder strip between two same-radius circles.
            let (e_lo, lo_c) = lo.as_ref().expect("the cylinder strip's circles");
            let (e_hi, hi_c) = hi.as_ref().expect("the cylinder strip's circles");
            let (vi_lo, vi_hi) = (edges[*e_lo].start, edges[*e_hi].start);
            let seam = {
                let i = edges.len();
                edges.push(seam_between(lo_c.point(0.0), hi_c.point(0.0), vi_lo, vi_hi));
                i
            };
            faces.push(cylinder_strip_face(
                Surface::Cylinder {
                    origin: o_c,
                    axis: a,
                    ref_dir: x_hat,
                    radius: r_c,
                },
                *e_lo,
                *e_hi,
                seam,
                v_param(lo_c, o_c, a),
                v_param(hi_c, o_c, a),
                false,
            ));
        } else {
            // The spherical segment: pole-to-circle (the single-wire
            // cap), circle-to-pole, circle-to-circle (the seam band),
            // or pole-to-pole (unreachable: the contained sphere).
            match (lo, hi) {
                (Some((e_lo, lo_c)), Some((e_hi, hi_c))) => {
                    let lat_lo = latitude_of(lo_c, c_s, a);
                    let lat_hi = latitude_of(hi_c, c_s, a);
                    let (vi_lo, vi_hi) = (edges[*e_lo].start, edges[*e_hi].start);
                    let chain = meridian_chain(
                        &mut vertices,
                        &mut edges,
                        c_s,
                        a,
                        x_hat,
                        r_s,
                        lo_c,
                        hi_c,
                        vi_lo,
                        vi_hi,
                    );
                    faces.push(sphere_band_face(
                        sph(),
                        &chain,
                        *e_lo,
                        *e_hi,
                        lat_lo,
                        lat_hi,
                        false,
                    ));
                }
                (None, Some((e_hi, hi_c))) => {
                    // From the south pole up to the circle (kept below).
                    faces.push(sphere_cap_face(
                        sph(),
                        *e_hi,
                        latitude_of(hi_c, c_s, a),
                        -std::f64::consts::FRAC_PI_2,
                        false,
                        false,
                    ));
                }
                (Some((e_lo, lo_c)), None) => {
                    // From the circle up to the north pole (kept above).
                    faces.push(sphere_cap_face(
                        sph(),
                        *e_lo,
                        latitude_of(lo_c, c_s, a),
                        std::f64::consts::FRAC_PI_2,
                        true,
                        false,
                    ));
                }
                (None, None) => {
                    return Err("the pole-to-pole spherical segment — the contained sphere \
                         was not caught by the classification"
                        .into());
                }
            }
        }
    }

    // The end disks (the region lies inside the slab).
    if let (Some(e_c), Some(c)) = (e_end_lo, end_lo.as_ref()) {
        faces.push(plane_disk_face(
            at(0.0),
            v3(-a.x, -a.y, -a.z),
            x_hat,
            e_c,
            circle_radius(c),
            false,
        ));
    }
    if let (Some(e_c), Some(c)) = (e_end_hi, end_hi.as_ref()) {
        faces.push(plane_disk_face(
            at(h_c),
            a,
            x_hat,
            e_c,
            circle_radius(c),
            true,
        ));
    }

    let brep = ExactBRep {
        vertices,
        edges,
        faces,
    };
    brep.validate()
        .map_err(|e| format!("the composed capsule stack failed validation: {e}"))?;
    Ok(brep)
}

/// The radius of a circle curve.
fn circle_radius(c: &Curve) -> f64 {
    match c {
        Curve::Circle { radius, .. } => *radius,
        _ => unreachable!("circle_radius on a non-circle"),
    }
}

/// The one-piece bowl cut: the material at the far end of the slab,
/// the sphere dipping in from the near end with its pole inside the
/// slab (the rim-bowl when the piece spans the whole slab — the mouth
/// circle strictly inside the near rim, the annular near cap; the
/// pinch-bowl when the section circle sits inside the slab — the
/// piece `[far, m]` pinches off at the section, no near cap face).
/// `from_above` selects the side (the material at the base, the sphere
/// beyond the top — or mirrored).
#[allow(clippy::too_many_arguments)] // the operand frames, the profile, the side
fn sphere_bowl_cut(
    o_c: Vector3,
    a: Vector3,
    x_hat: Vector3,
    r_c: f64,
    h_c: f64,
    c_s: Vector3,
    r_s: f64,
    z_s: f64,
    h_i: f64,
    from_above: bool,
) -> Result<ExactBRep, String> {
    let at = |z: f64| v3(o_c.x + a.x * z, o_c.y + a.y * z, o_c.z + a.z * z);
    let sph = || Surface::Sphere {
        center: c_s,
        axis: a,
        ref_dir: x_hat,
        radius: r_s,
    };
    let rim = |z: f64| Curve::Circle {
        center: at(z),
        axis: a,
        ref_dir: x_hat,
        radius: r_c,
    };
    let sect = |h: f64| Curve::Circle {
        center: v3(c_s.x + a.x * h, c_s.y + a.y * h, c_s.z + a.z * h),
        axis: a,
        ref_dir: x_hat,
        radius: r_c,
    };

    let (far_z, near_z) = if from_above { (0.0, h_c) } else { (h_c, 0.0) };
    // The piece's boundary at the near end: the section circle's
    // height clipped by the near plane. A point zone (h_i = 0, the
    // sphere narrower) has no section: the piece spans the whole slab.
    let (sect_h, m) = if h_i > 0.0 {
        if from_above {
            (-h_i, (z_s - h_i).min(h_c))
        } else {
            (h_i, (z_s + h_i).max(0.0))
        }
    } else if from_above {
        (-h_i, h_c)
    } else {
        (h_i, 0.0)
    };
    let spans_slab = if from_above { m == h_c } else { m == 0.0 };

    let far_rim = rim(far_z);
    let near_rim = if spans_slab { Some(rim(near_z)) } else { None };
    let section = sect(sect_h);

    // The mouth circle at the near plane (the rim-bowl's annulus hole
    // and the bowl's upper boundary), the K3a plane×sphere section.
    let mouth = if spans_slab {
        Some(
            sphere_plane_mouth(&sph(), at(near_z), a, x_hat)?
                .expect("the sphere crosses the near plane (the classification guarantees it)"),
        )
    } else {
        None
    };

    let mut vertices: Vec<TopoVertex> = Vec::new();
    let mut edges: Vec<TopoEdge> = Vec::new();
    let mut faces: Vec<TopoFace> = Vec::new();

    let e_far = push_circle(&mut vertices, &mut edges, far_rim.clone());
    let e_near = near_rim
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));
    let e_mouth = mouth
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));
    let e_sect = if spans_slab {
        None
    } else {
        Some(push_circle(&mut vertices, &mut edges, section.clone()))
    };

    // The mantle: from the far rim to the piece's near boundary.
    let (lo_c, e_lo, hi_c, e_hi) = if from_above {
        let (hi_c, e_hi) = if spans_slab {
            (
                near_rim.clone().expect("the near rim"),
                e_near.expect("the near rim edge"),
            )
        } else {
            (section.clone(), e_sect.expect("the section edge"))
        };
        (far_rim.clone(), e_far, hi_c, e_hi)
    } else {
        let (lo_c, e_lo) = if spans_slab {
            (
                near_rim.clone().expect("the near rim"),
                e_near.expect("the near rim edge"),
            )
        } else {
            (section.clone(), e_sect.expect("the section edge"))
        };
        (lo_c, e_lo, far_rim.clone(), e_far)
    };
    let (vi_lo, vi_hi) = (edges[e_lo].start, edges[e_hi].start);
    let seam = {
        let i = edges.len();
        edges.push(seam_between(lo_c.point(0.0), hi_c.point(0.0), vi_lo, vi_hi));
        i
    };
    faces.push(cylinder_strip_face(
        Surface::Cylinder {
            origin: o_c,
            axis: a,
            ref_dir: x_hat,
            radius: r_c,
        },
        e_lo,
        e_hi,
        seam,
        v_param(&lo_c, o_c, a),
        v_param(&hi_c, o_c, a),
        false,
    ));

    // The far cap: the full disk (the outward normal at the far plane).
    let far_out = if from_above { v3(-a.x, -a.y, -a.z) } else { a };
    let far_forward = !from_above;
    faces.push(plane_disk_face(
        at(far_z),
        far_out,
        x_hat,
        e_far,
        r_c,
        far_forward,
    ));

    // The near end: the annulus + the bowl (the rim-bowl), or the
    // pinching bowl alone (the pinch-bowl).
    let (e_bowl, bowl_lat) = if spans_slab {
        let m_c = mouth.as_ref().expect("the near mouth");
        // The annular near cap.
        faces.push(plane_annulus_face(
            at(near_z),
            if from_above { a } else { v3(-a.x, -a.y, -a.z) },
            x_hat,
            e_near.expect("the near rim edge"),
            e_mouth.expect("the mouth edge"),
            r_c,
            from_above,
        ));
        (e_mouth.expect("the mouth edge"), latitude_of(m_c, c_s, a))
    } else {
        (
            e_sect.expect("the section edge"),
            latitude_of(&section, c_s, a),
        )
    };
    // The bowl: the sphere's part from the near pole to the boundary
    // circle — a single-wire cap, REVERSED (the solid lies outside the
    // sphere), the region on the pole's side of the circle.
    let pole_lat = if from_above {
        -std::f64::consts::FRAC_PI_2
    } else {
        std::f64::consts::FRAC_PI_2
    };
    faces.push(sphere_cap_face(
        sph(),
        e_bowl,
        bowl_lat,
        pole_lat,
        from_above,
        true,
    ));

    let brep = ExactBRep {
        vertices,
        edges,
        faces,
    };
    brep.validate()
        .map_err(|e| format!("the composed bowl cut failed validation: {e}"))?;
    Ok(brep)
}

/// The groove cut: the sphere's reach covering the slab (the material
/// all-annulus) — the full mantle, the two annular caps, and the
/// reversed spherical inner wall between the two plane mouths (a solid
/// torus: `V − E_nd + F` reads 2 with the caps' holes).
#[allow(clippy::too_many_arguments)] // the operand frames and the profile
fn sphere_groove_cut(
    o_c: Vector3,
    a: Vector3,
    x_hat: Vector3,
    r_c: f64,
    h_c: f64,
    c_s: Vector3,
    r_s: f64,
) -> Result<ExactBRep, String> {
    let at = |z: f64| v3(o_c.x + a.x * z, o_c.y + a.y * z, o_c.z + a.z * z);
    let sph = || Surface::Sphere {
        center: c_s,
        axis: a,
        ref_dir: x_hat,
        radius: r_s,
    };
    let rim = |z: f64| Curve::Circle {
        center: at(z),
        axis: a,
        ref_dir: x_hat,
        radius: r_c,
    };
    let rim0 = rim(0.0);
    let rimh = rim(h_c);
    let mouth0 = sphere_plane_mouth(&sph(), at(0.0), a, x_hat)?
        .expect("the sphere crosses the base (the groove classification)");
    let mouthh = sphere_plane_mouth(&sph(), at(h_c), a, x_hat)?
        .expect("the sphere crosses the top (the groove classification)");

    let mut vertices: Vec<TopoVertex> = Vec::new();
    let mut edges: Vec<TopoEdge> = Vec::new();
    let mut faces: Vec<TopoFace> = Vec::new();

    let e_rim0 = push_circle(&mut vertices, &mut edges, rim0.clone());
    let e_rimh = push_circle(&mut vertices, &mut edges, rimh.clone());
    let e_m0 = push_circle(&mut vertices, &mut edges, mouth0.clone());
    let e_mh = push_circle(&mut vertices, &mut edges, mouthh.clone());

    // The mantle: the full strip.
    let (vi_rim0, vi_rimh) = (edges[e_rim0].start, edges[e_rimh].start);
    let seam = {
        let i = edges.len();
        edges.push(seam_between(
            rim0.point(0.0),
            rimh.point(0.0),
            vi_rim0,
            vi_rimh,
        ));
        i
    };
    faces.push(cylinder_strip_face(
        Surface::Cylinder {
            origin: o_c,
            axis: a,
            ref_dir: x_hat,
            radius: r_c,
        },
        e_rim0,
        e_rimh,
        seam,
        0.0,
        h_c,
        false,
    ));

    // The annular caps: at the base (outward −a: outer backward) and
    // at the top (outward +a: outer forward).
    faces.push(plane_annulus_face(
        at(0.0),
        v3(-a.x, -a.y, -a.z),
        x_hat,
        e_rim0,
        e_m0,
        r_c,
        false,
    ));
    faces.push(plane_annulus_face(
        at(h_c),
        a,
        x_hat,
        e_rimh,
        e_mh,
        r_c,
        true,
    ));

    // The inner wall: the sphere band between the two mouths, the
    // meridian seam pattern, REVERSED (the solid outside the sphere).
    let lat0 = latitude_of(&mouth0, c_s, a);
    let lath = latitude_of(&mouthh, c_s, a);
    let (vi_m0, vi_mh) = (edges[e_m0].start, edges[e_mh].start);
    let chain = meridian_chain(
        &mut vertices,
        &mut edges,
        c_s,
        a,
        x_hat,
        r_s,
        &mouth0,
        &mouthh,
        vi_m0,
        vi_mh,
    );
    faces.push(sphere_band_face(
        sph(),
        &chain,
        e_m0,
        e_mh,
        lat0,
        lath,
        true,
    ));

    let brep = ExactBRep {
        vertices,
        edges,
        faces,
    };
    brep.validate()
        .map_err(|e| format!("the composed groove failed validation: {e}"))?;
    Ok(brep)
}

/// The Fuse composition: the dome bumps — the sphere crossing the cap
/// planes with its mouth circles strictly inside the rims and the
/// mantle intact (the zone clear of the slab): the full mantle, the
/// caps (full disks where the sphere does not reach, annuli where it
/// crosses, each annulus carrying the dome beyond it — the sphere's
/// part from the mouth to the pole, a single-wire cap, naturally
/// oriented).
#[allow(clippy::too_many_arguments)] // the operand frames and the profile
fn sphere_bump_fuse(
    o_c: Vector3,
    a: Vector3,
    x_hat: Vector3,
    r_c: f64,
    h_c: f64,
    c_s: Vector3,
    r_s: f64,
    crosses_base: bool,
    crosses_top: bool,
) -> Result<ExactBRep, String> {
    let at = |z: f64| v3(o_c.x + a.x * z, o_c.y + a.y * z, o_c.z + a.z * z);
    let sph = || Surface::Sphere {
        center: c_s,
        axis: a,
        ref_dir: x_hat,
        radius: r_s,
    };
    let rim = |z: f64| Curve::Circle {
        center: at(z),
        axis: a,
        ref_dir: x_hat,
        radius: r_c,
    };
    let rim0 = rim(0.0);
    let rimh = rim(h_c);
    let mouth0 = if crosses_base {
        Some(sphere_plane_mouth(&sph(), at(0.0), a, x_hat)?.expect("the sphere crosses the base"))
    } else {
        None
    };
    let mouthh = if crosses_top {
        Some(sphere_plane_mouth(&sph(), at(h_c), a, x_hat)?.expect("the sphere crosses the top"))
    } else {
        None
    };

    let mut vertices: Vec<TopoVertex> = Vec::new();
    let mut edges: Vec<TopoEdge> = Vec::new();
    let mut faces: Vec<TopoFace> = Vec::new();

    let e_rim0 = push_circle(&mut vertices, &mut edges, rim0.clone());
    let e_rimh = push_circle(&mut vertices, &mut edges, rimh.clone());
    let e_m0 = mouth0
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));
    let e_mh = mouthh
        .as_ref()
        .map(|c| push_circle(&mut vertices, &mut edges, c.clone()));

    // The mantle: the full strip, naturally oriented.
    let (vi_rim0, vi_rimh) = (edges[e_rim0].start, edges[e_rimh].start);
    let seam = {
        let i = edges.len();
        edges.push(seam_between(
            rim0.point(0.0),
            rimh.point(0.0),
            vi_rim0,
            vi_rimh,
        ));
        i
    };
    faces.push(cylinder_strip_face(
        Surface::Cylinder {
            origin: o_c,
            axis: a,
            ref_dir: x_hat,
            radius: r_c,
        },
        e_rim0,
        e_rimh,
        seam,
        0.0,
        h_c,
        false,
    ));

    // The base end: the full cap or the annulus + the dome.
    match (e_m0, mouth0.as_ref()) {
        (Some(e_m), Some(m)) => {
            faces.push(plane_annulus_face(
                at(0.0),
                v3(-a.x, -a.y, -a.z),
                x_hat,
                e_rim0,
                e_m,
                r_c,
                false,
            ));
            // The dome below: the region from the mouth to the south
            // pole (kept below: backward, natural).
            faces.push(sphere_cap_face(
                sph(),
                e_m,
                latitude_of(m, c_s, a),
                -std::f64::consts::FRAC_PI_2,
                false,
                false,
            ));
        }
        _ => {
            faces.push(plane_disk_face(
                at(0.0),
                v3(-a.x, -a.y, -a.z),
                x_hat,
                e_rim0,
                r_c,
                false,
            ));
        }
    }

    // The top end: the full cap or the annulus + the dome.
    match (e_mh, mouthh.as_ref()) {
        (Some(e_m), Some(m)) => {
            faces.push(plane_annulus_face(
                at(h_c),
                a,
                x_hat,
                e_rimh,
                e_m,
                r_c,
                true,
            ));
            // The dome above: the region from the mouth to the north
            // pole (kept above: forward, natural).
            faces.push(sphere_cap_face(
                sph(),
                e_m,
                latitude_of(m, c_s, a),
                std::f64::consts::FRAC_PI_2,
                true,
                false,
            ));
        }
        _ => {
            faces.push(plane_disk_face(at(h_c), a, x_hat, e_rimh, r_c, true));
        }
    }

    let brep = ExactBRep {
        vertices,
        edges,
        faces,
    };
    brep.validate()
        .map_err(|e| format!("the composed bump fuse failed validation: {e}"))?;
    Ok(brep)
}

/// The exact B-rep boolean of a finite cylinder and a sphere, coaxial
/// (see the module doc for the compositions and the refusals).
///
/// The Cut is cylinder − sphere; the sphere is the tool. The frame
/// contract follows `Part::Cylinder`: `o_c` the base center, `a` the
/// unit axis base→top, `h_c > 0`; the sphere `c_s`, `r_s > 0`.
pub fn cylinder_sphere_boolean(
    op: BooleanOp,
    o_c: Vector3,
    a: Vector3,
    r_c: f64,
    h_c: f64,
    c_s: Vector3,
    r_s: f64,
) -> Result<BooleanOutcome, String> {
    require_unit(a, "cylinder axis")?;
    for (r, what) in [(r_c, "cylinder"), (r_s, "sphere")] {
        if r <= 0.0 {
            return Err(format!("the {what} radius must be positive, got {r}"));
        }
    }
    if h_c <= 0.0 {
        return Err(format!("the cylinder height must be positive, got {h_c}"));
    }

    // ---- the coaxial reading ----------------------------------------
    let x_hat = any_perpendicular(a);
    let dv = v3(c_s.x - o_c.x, c_s.y - o_c.y, c_s.z - o_c.z);
    let along = dv.dot(&a);
    let perp = v3(dv.x - a.x * along, dv.y - a.y * along, dv.z - a.z * along);
    let perp_len = exact_sum_rounded(&length_squared_terms(perp))
        .max(0.0)
        .sqrt();
    if perp_len > 1e-9 * r_c.max(r_s).max(1.0) {
        // The honest refusal, propagated from K3a by name.
        let cyl = Surface::Cylinder {
            origin: o_c,
            axis: a,
            ref_dir: x_hat,
            radius: r_c,
        };
        let sph = Surface::Sphere {
            center: c_s,
            axis: a,
            ref_dir: x_hat,
            radius: r_s,
        };
        return match intersect_surfaces(&cyl, &sph) {
            SurfaceIntersection::NotAnalytic { why } => Ok(refused(&why)),
            other => Err(format!(
                "the non-coaxial cylinder×sphere section was not refused: {other:?}"
            )),
        };
    }
    // The projected center and its height parameter (the model's
    // reading of the intended coaxial placement).
    let c_s = v3(
        o_c.x + a.x * along,
        o_c.y + a.y * along,
        o_c.z + a.z * along,
    );
    let z_s = along;

    // ---- the profile parameters --------------------------------------
    // h_i: the section offset, K3a's own arithmetic (0 when the sphere
    // is narrower — the section zone degenerates to a point and the
    // sphere's disk never swallows the cylinder's).
    let h_i = diff_products_rounded(r_s, r_s, r_c, r_c).max(0.0).sqrt();

    // The exact classifications (expansion signs of the stored floats):
    //   bot_in ≥ 0: the sphere's bottom at/above the base.
    //   top_in ≤ 0: the sphere's top at/below the top plane.
    //   lo_vs_0: sign(z_s − h_i) — the lower section vs the base.
    //   lo_vs_h: sign(z_s − h_i − h_c) — the lower section vs the top.
    //   hi_vs_0: sign(z_s + h_i) — the upper section vs the base.
    //   hi_vs_h: sign(z_s + h_i − h_c) — the upper section vs the top.
    let bot_in = exact_sum_sign(&[z_s, -r_s]);
    let top_in = exact_sum_sign(&[z_s, r_s, -h_c]);
    let lo_vs_0 = exact_sum_sign(&[z_s, -h_i]);
    let lo_vs_h = exact_sum_sign(&[z_s, -h_i, -h_c]);
    let hi_vs_0 = exact_sum_sign(&[z_s, h_i]);
    let hi_vs_h = exact_sum_sign(&[z_s, h_i, -h_c]);

    // A plane-cut mouth coinciding with a rim circle: |plane − z_s| =
    // h_i with the sphere at least as wide as the cylinder.
    let rim_mount = |p: f64| -> bool {
        r_s >= r_c
            && (exact_sum_sign(&[p, -z_s, -h_i]) == 0 || exact_sum_sign(&[p, -z_s, h_i]) == 0)
    };

    let whole_cyl = || BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(cylinder_piece(o_c, a, r_c, 0.0, h_c)),
    };

    // ---- no reach: the z-ranges miss (the tangent touch included) ---
    //   above ≥ 0: the sphere's bottom at/above the top plane.
    //   below ≤ 0: the sphere's top at/below the base.
    let above = exact_sum_sign(&[z_s, -r_s, -h_c]);
    let below = exact_sum_sign(&[z_s, r_s]);
    if above >= 0 || below <= 0 {
        return Ok(match op {
            BooleanOp::Common => empty(),
            BooleanOp::Cut => whole_cyl(),
            BooleanOp::Fuse => {
                if above == 0 || below == 0 {
                    refused("tangent fuse — the shared point is not an edge-manifold junction")
                } else {
                    refused("disjoint fuse — a two-shell compound, not a single closed shell")
                }
            }
        });
    }

    // ---- the equator tangency: equal radii, the equator strictly
    // inside the slab — the sphere touches the mantle along a circle
    // (a measure-zero non-manifold contact); the Common is unaffected
    // (its boundary never shows the contact).
    if r_s == r_c
        && matches!(op, BooleanOp::Cut | BooleanOp::Fuse)
        && exact_sum_sign(&[z_s]) > 0
        && exact_sum_sign(&[z_s, -h_c]) < 0
    {
        return Ok(refused(
            "the equator tangency — the sphere touches the mantle along its \
             equator (a measure-zero non-manifold contact)",
        ));
    }

    // ---- the sphere contained in the slab and the cylinder ----------
    if bot_in >= 0 && top_in <= 0 && r_s <= r_c {
        return Ok(match op {
            BooleanOp::Common => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(c_s, r_s)),
            },
            BooleanOp::Cut => refused(
                "the contained cut — the internal void needs a second shell \
                 the K1 model does not carry",
            ),
            BooleanOp::Fuse => whole_cyl(),
        });
    }

    // ---- the zone covering the slab: the sphere's disk swallows the
    // cylinder's everywhere on it --------------------------------------
    if lo_vs_0 <= 0 && hi_vs_h >= 0 {
        return Ok(match op {
            BooleanOp::Common => whole_cyl(),
            BooleanOp::Cut => empty(),
            BooleanOp::Fuse => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(c_s, r_s)),
            },
        });
    }

    // ---- the per-op compositions -------------------------------------
    match op {
        BooleanOp::Common => {
            let brep = capsule_stack(o_c, a, x_hat, r_c, h_c, c_s, r_s, z_s, h_i)?;
            Ok(BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(brep),
            })
        }
        BooleanOp::Cut => {
            // The material profile over the slab: full where
            // |z − z_s| ≥ r_s, annulus where h_i ≤ |z − z_s| < r_s,
            // empty inside the section zone. A point zone (h_i = 0,
            // the sphere narrower) never empties anything: the
            // material is one connected piece over the whole slab.
            //
            // The groove first: the sphere's reach covering the slab
            // strictly (the material all-annulus over it) with the
            // zone clear of the slab's interior.
            let zone_interferes = h_i > 0.0 && hi_vs_0 > 0 && lo_vs_h < 0;
            if bot_in < 0 && top_in > 0 && !zone_interferes {
                let brep = sphere_groove_cut(o_c, a, x_hat, r_c, h_c, c_s, r_s)?;
                return Ok(BooleanOutcome {
                    kind: BooleanOutcomeKind::Solid(brep),
                });
            }
            let (below_mat, above_mat) = if h_i > 0.0 {
                (lo_vs_0 > 0, hi_vs_h < 0)
            } else {
                (false, false)
            };
            if below_mat && above_mat {
                return Ok(refused(
                    "the two-piece cut — the sphere's section zone splits the \
                     cylinder into two shells",
                ));
            }
            if h_i > 0.0 && !below_mat && !above_mat {
                return Err("the zone-covering cut was not caught by the classification".into());
            }
            // The one piece: the material at the far end, the sphere
            // dipping from the near end. from_above: the material at
            // the base ([0, m), m = min(h_c, z_s − h_i)). A point zone
            // spans the slab: the near end is the crossed plane.
            let from_above = if h_i > 0.0 { below_mat } else { top_in > 0 };
            let (pole_in, near_z) = if from_above {
                (bot_in >= 0, h_c)
            } else {
                (top_in <= 0, 0.0)
            };
            if rim_mount(near_z) {
                return Ok(refused(
                    "the rim-mount — the mouth circle coincides with the rim \
                     (a measure-zero annulus), refused",
                ));
            }
            if !pole_in {
                // The sphere's reach past the far plane with the piece
                // pinching inside the slab — the collar, a genus-1
                // piece (the spanning variant is the groove above).
                return Ok(refused(
                    "the collar — the one-piece material pinches inside the \
                     slab with the sphere's reach past the far plane (a genus-1 \
                     piece), refused",
                ));
            }
            let brep = sphere_bowl_cut(o_c, a, x_hat, r_c, h_c, c_s, r_s, z_s, h_i, from_above)?;
            Ok(BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(brep),
            })
        }
        BooleanOp::Fuse => {
            // The bumps need the mantle intact: the zone clear of the
            // slab — or a point zone (a narrower sphere), which never
            // interrupts anything.
            let zone_clear = h_i == 0.0 || hi_vs_0 <= 0 || lo_vs_h >= 0;
            if !zone_clear {
                return Ok(refused(
                    "the split-circle mantles — the sphere's section zone \
                     interrupts the cylinder's mantle, the next slice",
                ));
            }
            let crosses_base = bot_in < 0;
            let crosses_top = top_in > 0;
            if rim_mount(0.0) && crosses_base {
                return Ok(refused(
                    "the rim-mount — the mouth circle coincides with the rim \
                     (a measure-zero annulus), refused",
                ));
            }
            if rim_mount(h_c) && crosses_top {
                return Ok(refused(
                    "the rim-mount — the mouth circle coincides with the rim \
                     (a measure-zero annulus), refused",
                ));
            }
            let brep =
                sphere_bump_fuse(o_c, a, x_hat, r_c, h_c, c_s, r_s, crosses_base, crosses_top)?;
            Ok(BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(brep),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// K3f — the nested staggered pieces: the partial bore and the chimney
// ---------------------------------------------------------------------------
//
// The nested family (one cylinder's disk strictly inside the other's,
// the axes parallel) with staggered z-ranges — the compositions the
// covering cases refused as "the next slice":
//
// * **The partial bore** (the cut, b inside a cross-wise, b's range
//   crossing exactly one of a's ends): the material is the full disk
//   beyond the bore and the annulus beside it — one piece: a's full
//   mantle and far cap, the annular near cap (a's rim minus b's
//   cross-section circle at the cap plane), b's wall (reversed — the
//   solid outside b) down to the bore's floor — b's end disk at the
//   inner rim height, the solid above it. Strictly inside instead: the
//   internal void, refused by name.
// * **The chimney** (the fuse, b inside a cross-wise, b sticking out
//   of a's range): a's whole mantle, the annular caps where the
//   chimney passes through (a's rim minus b's cross-section), the
//   full caps where it does not, and the chimney itself — b's mantle
//   (naturally oriented — the solid inside b) and its end cap.
// * **The sticking-out piece** (the cut, a inside b cross-wise): a's
//   z-parts beyond b's range are ordinary cylinder pieces — one, or
//   two shells (refused).

/// The partial bore — b inside a cross-wise, b's range crossing one of
/// a's ends. `from_base`: b enters through a's base (its far rim
/// inside a's range), else through a's top.
#[allow(clippy::too_many_arguments)] // the two cylinder frames and the stagger
fn nested_partial_bore(
    o1: Vector3,
    a: Vector3,
    r1: f64,
    h1: f64,
    o2: Vector3,
    r2: f64,
    s2: f64,
    h2: f64,
    from_base: bool,
) -> ExactBRep {
    let x_hat = any_perpendicular(a);
    let at1 = |v: f64| v3(o1.x + a.x * v, o1.y + a.y * v, o1.z + a.z * v);
    // b's circle at axis height z (its center on b's own axis).
    let b_at = |v: f64| v3(o2.x + a.x * v, o2.y + a.y * v, o2.z + a.z * v);
    let b_circle = |v: f64| Curve::Circle {
        center: b_at(v),
        axis: a,
        ref_dir: x_hat,
        radius: r2,
    };
    // b's v-parameters of a's cap planes: z ∈ {0, h1} ⟺ v ∈ {−s2, h1 − s2}.
    // The crossing plane and the inner rim:
    //   from_base: the wall spans b's v ∈ [−s2, h2]; the floor at v = h2.
    //   else:      the wall spans b's v ∈ [0, h1 − s2]; the floor at v = 0.
    let (cross_v, floor_v) = if from_base {
        // b enters through the base: the cross-section at a's base
        // plane (b's v = −s2), the floor at b's top rim (v = h2).
        (-s2, h2)
    } else {
        // b enters through the top: the cross-section at a's top
        // plane (b's v = h1 − s2), the floor at b's base rim (v = 0).
        (h1 - s2, 0.0)
    };
    let cross_circle = b_circle(cross_v);
    let floor_circle = b_circle(floor_v);

    let rim_lo = Curve::Circle {
        center: at1(0.0),
        axis: a,
        ref_dir: x_hat,
        radius: r1,
    };
    let rim_hi = Curve::Circle {
        center: at1(h1),
        axis: a,
        ref_dir: x_hat,
        radius: r1,
    };

    let mut vertices: Vec<TopoVertex> = Vec::new();
    let mut edges: Vec<TopoEdge> = Vec::new();
    let mut faces: Vec<TopoFace> = Vec::new();

    let e_rim_lo = push_circle(&mut vertices, &mut edges, rim_lo.clone());
    let e_rim_hi = push_circle(&mut vertices, &mut edges, rim_hi.clone());
    let e_cross = push_circle(&mut vertices, &mut edges, cross_circle.clone());
    let e_floor = push_circle(&mut vertices, &mut edges, floor_circle.clone());

    // a's mantle: the full strip, the K1 pattern.
    let (vi_lo, vi_hi) = (edges[e_rim_lo].start, edges[e_rim_hi].start);
    let seam_a = {
        let i = edges.len();
        edges.push(seam_between(
            rim_lo.point(0.0),
            rim_hi.point(0.0),
            vi_lo,
            vi_hi,
        ));
        i
    };
    faces.push(cylinder_strip_face(
        Surface::Cylinder {
            origin: o1,
            axis: a,
            ref_dir: x_hat,
            radius: r1,
        },
        e_rim_lo,
        e_rim_hi,
        seam_a,
        0.0,
        h1,
        false,
    ));

    // The far cap: the full disk.
    let (far_z, far_out, far_e) = if from_base {
        (h1, a, e_rim_hi)
    } else {
        (0.0, v3(-a.x, -a.y, -a.z), e_rim_lo)
    };
    faces.push(plane_disk_face(
        at1(far_z),
        far_out,
        x_hat,
        far_e,
        r1,
        from_base,
    ));

    // The near cap: the annulus (a's rim outside, b's cross-section as
    // the hole) at the crossed plane.
    let (near_z, near_e) = if from_base {
        (0.0, e_rim_lo)
    } else {
        (h1, e_rim_hi)
    };
    faces.push(plane_annulus_face(
        at1(near_z),
        if from_base { v3(-a.x, -a.y, -a.z) } else { a },
        x_hat,
        near_e,
        e_cross,
        r1,
        !from_base,
    ));

    // b's wall: the strip between the cross-section circle and b's
    // inner rim, in b's own v-frame, REVERSED (the solid outside b).
    let (wall_lo, wall_hi) = if from_base {
        (cross_v, floor_v)
    } else {
        (floor_v, cross_v)
    };
    let (e_lo, e_hi) = if from_base {
        (e_cross, e_floor)
    } else {
        (e_floor, e_cross)
    };
    let (vi_wlo, vi_whi) = (edges[e_lo].start, edges[e_hi].start);
    let (p_wlo, p_whi) = if from_base {
        (cross_circle.point(0.0), floor_circle.point(0.0))
    } else {
        (floor_circle.point(0.0), cross_circle.point(0.0))
    };
    let seam_b = {
        let i = edges.len();
        edges.push(seam_between(p_wlo, p_whi, vi_wlo, vi_whi));
        i
    };
    faces.push(cylinder_strip_face(
        Surface::Cylinder {
            origin: o2,
            axis: a,
            ref_dir: x_hat,
            radius: r2,
        },
        e_lo,
        e_hi,
        seam_b,
        wall_lo.min(wall_hi),
        wall_lo.max(wall_hi),
        true,
    ));

    // The bore's floor: b's end disk at the inner rim — the solid
    // beyond it, the outward normal toward the bore (away from the
    // material).
    let floor_out = if from_base {
        // b's top rim inside a: the material above, outward −a.
        v3(-a.x, -a.y, -a.z)
    } else {
        // b's base rim inside a: the material below, outward +a.
        a
    };
    faces.push(plane_disk_face(
        b_at(floor_v),
        floor_out,
        x_hat,
        e_floor,
        r2,
        !from_base,
    ));

    ExactBRep {
        vertices,
        edges,
        faces,
    }
}

/// The chimney — b inside a cross-wise, b sticking out of a's z-range
/// through one or both of its ends: a's whole mantle, the annular caps
/// where the chimney passes, the full caps where it does not, and the
/// chimney itself (b's mantle beyond the plane, naturally oriented,
/// and its end cap).
#[allow(clippy::too_many_arguments)] // the two cylinder frames and the stagger
fn nested_chimney(
    o1: Vector3,
    a: Vector3,
    r1: f64,
    h1: f64,
    o2: Vector3,
    r2: f64,
    s2: f64,
    h2: f64,
) -> ExactBRep {
    let x_hat = any_perpendicular(a);
    let at1 = |v: f64| v3(o1.x + a.x * v, o1.y + a.y * v, o1.z + a.z * v);
    let b_at = |v: f64| v3(o2.x + a.x * v, o2.y + a.y * v, o2.z + a.z * v);
    let b_circle = |v: f64| Curve::Circle {
        center: b_at(v),
        axis: a,
        ref_dir: x_hat,
        radius: r2,
    };
    // The chimneys: b's parts beyond a's cap planes. The bottom one
    // spans b's v ∈ [0, −s2] (when s2 < 0); the top one b's v ∈
    // [h1 − s2, h2] (when s2 + h2 > h1).
    let bottom = s2 < 0.0;
    let top = s2 + h2 > h1;

    let rim_lo = Curve::Circle {
        center: at1(0.0),
        axis: a,
        ref_dir: x_hat,
        radius: r1,
    };
    let rim_hi = Curve::Circle {
        center: at1(h1),
        axis: a,
        ref_dir: x_hat,
        radius: r1,
    };
    // b's cross-sections at a's cap planes (the annuli's holes).
    let cross_lo = b_circle(-s2);
    let cross_hi = b_circle(h1 - s2);
    // b's own end rims.
    let b_base = b_circle(0.0);
    let b_top = b_circle(h2);

    let mut vertices: Vec<TopoVertex> = Vec::new();
    let mut edges: Vec<TopoEdge> = Vec::new();
    let mut faces: Vec<TopoFace> = Vec::new();

    let e_rim_lo = push_circle(&mut vertices, &mut edges, rim_lo.clone());
    let e_rim_hi = push_circle(&mut vertices, &mut edges, rim_hi.clone());
    let e_xlo = bottom.then(|| push_circle(&mut vertices, &mut edges, cross_lo.clone()));
    let e_xhi = top.then(|| push_circle(&mut vertices, &mut edges, cross_hi.clone()));
    let e_bbase = bottom.then(|| push_circle(&mut vertices, &mut edges, b_base.clone()));
    let e_btop = top.then(|| push_circle(&mut vertices, &mut edges, b_top.clone()));

    // a's mantle: the full strip.
    let (vi_lo, vi_hi) = (edges[e_rim_lo].start, edges[e_rim_hi].start);
    let seam_a = {
        let i = edges.len();
        edges.push(seam_between(
            rim_lo.point(0.0),
            rim_hi.point(0.0),
            vi_lo,
            vi_hi,
        ));
        i
    };
    faces.push(cylinder_strip_face(
        Surface::Cylinder {
            origin: o1,
            axis: a,
            ref_dir: x_hat,
            radius: r1,
        },
        e_rim_lo,
        e_rim_hi,
        seam_a,
        0.0,
        h1,
        false,
    ));

    // The base end: the annulus with the chimney through it, or the
    // full cap.
    if bottom {
        faces.push(plane_annulus_face(
            at1(0.0),
            v3(-a.x, -a.y, -a.z),
            x_hat,
            e_rim_lo,
            e_xlo.expect("the bottom chimney's cross-section"),
            r1,
            false,
        ));
    } else {
        faces.push(plane_disk_face(
            at1(0.0),
            v3(-a.x, -a.y, -a.z),
            x_hat,
            e_rim_lo,
            r1,
            false,
        ));
    }
    // The top end: mirrored.
    if top {
        faces.push(plane_annulus_face(
            at1(h1),
            a,
            x_hat,
            e_rim_hi,
            e_xhi.expect("the top chimney's cross-section"),
            r1,
            true,
        ));
    } else {
        faces.push(plane_disk_face(at1(h1), a, x_hat, e_rim_hi, r1, true));
    }

    // The chimneys: b's mantle beyond the plane, naturally oriented,
    // and the end cap.
    if let (Some(e_base), Some(e_x)) = (e_bbase, e_xlo) {
        let (vi_wlo, vi_whi) = (edges[e_base].start, edges[e_x].start);
        let seam = {
            let i = edges.len();
            edges.push(seam_between(
                b_base.point(0.0),
                cross_lo.point(0.0),
                vi_wlo,
                vi_whi,
            ));
            i
        };
        faces.push(cylinder_strip_face(
            Surface::Cylinder {
                origin: o2,
                axis: a,
                ref_dir: x_hat,
                radius: r2,
            },
            e_base,
            e_x,
            seam,
            0.0,
            -s2,
            false,
        ));
        // The chimney's cap at b's base (outward −a).
        faces.push(plane_disk_face(
            b_at(0.0),
            v3(-a.x, -a.y, -a.z),
            x_hat,
            e_base,
            r2,
            false,
        ));
    }
    if let (Some(e_x), Some(e_top)) = (e_xhi, e_btop) {
        let (vi_wlo, vi_whi) = (edges[e_x].start, edges[e_top].start);
        let seam = {
            let i = edges.len();
            edges.push(seam_between(
                cross_hi.point(0.0),
                b_top.point(0.0),
                vi_wlo,
                vi_whi,
            ));
            i
        };
        faces.push(cylinder_strip_face(
            Surface::Cylinder {
                origin: o2,
                axis: a,
                ref_dir: x_hat,
                radius: r2,
            },
            e_x,
            e_top,
            seam,
            h1 - s2,
            h2,
            false,
        ));
        // The chimney's cap at b's top (outward +a).
        faces.push(plane_disk_face(b_at(h2), a, x_hat, e_top, r2, true));
    }

    ExactBRep {
        vertices,
        edges,
        faces,
    }
}
