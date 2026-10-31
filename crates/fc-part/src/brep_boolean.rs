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

// ---------------------------------------------------------------------------
// K4-2d/3 — box × sphere: the bite taxonomy on the box's boundary
// skeleton
// ---------------------------------------------------------------------------
//
// A loaded box-class operand (or any hexahedral box) meets a sphere
// whose center sits exactly on the box's boundary skeleton — at a
// corner (the bite is an octant ball), on an edge (a quarter ball
// spanning the two adjacent faces), or on a face (a half ball behind
// an interior circular hole). The three booleans are the face choices
// of each station:
//
// * **Cut** — the box with the bite removed: the bitten adjacent
//   faces (each losing its disk share, the boundary arc in the wire)
//   + the untouched faces + the REVERSED spherical patch (the solid
//   lies outside the sphere there);
// * **Fuse** — the box plus the bulge (the ball's complement share):
//   the SAME bitten faces (the disk shares are interior to the union)
//   + the untouched faces + the unreversed complement patch — Cut and
//   Fuse differ only in the patch's `reversed` flag and the region it
//   keeps, the K3c-2 discipline;
// * **Common** — the bite itself: the disk faces (quarter, half, or
//   the full circle's disk) + the unreversed patch.
//
// The arc-bounded patches read through the spherical polygon
// (measure's K4-2d/3 reading: Gauss–Bonnet excess area, the Stokes
// vector area); the circle-bounded one through the cap. The authored
// arcs are the classical NURBS arcs with canonical exact endpoints
// (`push_arc_chain`), every shared vertex comes from one canonical
// corner formula — the bit-exact contract — and the authored arc
// directions chain all-forward into the patch's POSITIVE cycle (the
// boundary orientation against the outward-of-sphere normal), a small
// theorem this construction leans on, verified for both box
// handednesses at every station.
//
// Honest refusals, by name, never approximated:
// * the center OFF the skeleton (an interior center whose ball crosses
//   faces, or a center outside the slabs) — the general spherical
//   overlap is not constructed;
// * the bite leaving its adjacent faces (r beyond an extent, or
//   beyond the along-edge span) — exact tangency at a bound included;
// * the center within recognition tolerance of a station but not
//   bit-exactly on it — the section circles would not be great
//   circles, and this kernel never silently snaps;
// * the contained cut (the internal void — a second shell) and the
//   disjoint fuse (a two-shell compound) — the family-wide refusals.
//
// The classification tolerances are recognition-scale (1e-9·scale:
// which station, or none); everything CONSTRUCTED is exact.

/// The box×sphere boolean. `o` is a corner; `a1`/`a2`/`a3` the unit
/// axes (mutually perpendicular, either handedness); `l1`/`l2`/`l3`
/// the extents along them; `cs`/`r` the sphere. Placements are
/// applied analytically by the caller (the feature dispatch).
#[allow(clippy::too_many_arguments)] // the box frame + dims + the sphere — the family's own vocabulary
pub fn box_sphere_boolean(
    op: BooleanOp,
    o: Vector3,
    a1: Vector3,
    a2: Vector3,
    a3: Vector3,
    l1: f64,
    l2: f64,
    l3: f64,
    cs: Vector3,
    r: f64,
) -> Result<BooleanOutcome, String> {
    require_unit(a1, "box axis a1")?;
    require_unit(a2, "box axis a2")?;
    require_unit(a3, "box axis a3")?;
    if (a1.dot(&a2)).abs() > 1e-9 || (a1.dot(&a3)).abs() > 1e-9 || (a2.dot(&a3)).abs() > 1e-9 {
        return Err("the box axes must be mutually perpendicular".into());
    }
    if !(l1 > 0.0 && l2 > 0.0 && l3 > 0.0) {
        return Err("the box extents must be positive".into());
    }
    if r <= 0.0 {
        return Err("the sphere radius must be positive".into());
    }
    let scale = l1
        .max(l2)
        .max(l3)
        .max(r)
        .max(o.x.abs().max(o.y.abs()).max(o.z.abs()))
        .max(cs.x.abs().max(cs.y.abs()).max(cs.z.abs()))
        .max(1.0);
    let eps = 1e-9 * scale;
    let axes = [a1, a2, a3];
    let dims = [l1, l2, l3];
    let d = [
        v3(cs.x - o.x, cs.y - o.y, cs.z - o.z).dot(&a1),
        v3(cs.x - o.x, cs.y - o.y, cs.z - o.z).dot(&a2),
        v3(cs.x - o.x, cs.y - o.y, cs.z - o.z).dot(&a3),
    ];

    // ---- engulf: the box entirely inside the ball -------------------
    // (convexity: all corners inside ⟹ the box inside; tangency
    // included on purpose — the touched remnant is measure-zero).
    let mut max_corner_d = 0.0f64;
    for s in 0..8u8 {
        let p = box_corner(o, axes, dims, [s & 1, (s >> 1) & 1, (s >> 2) & 1]);
        let dd = v3(p.x - cs.x, p.y - cs.y, p.z - cs.z).length();
        max_corner_d = max_corner_d.max(dd);
    }
    if max_corner_d <= r {
        return Ok(match op {
            BooleanOp::Cut => empty(),
            BooleanOp::Fuse => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(cs, r)),
            },
            BooleanOp::Common => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(whole_box_brep(o, axes, dims)),
            },
        });
    }

    // ---- disjoint (external tangency included) ----------------------
    let mut min_d2 = 0.0f64;
    for k in 0..3 {
        let excess = if d[k] < 0.0 {
            -d[k]
        } else if d[k] > dims[k] {
            d[k] - dims[k]
        } else {
            0.0
        };
        min_d2 += excess * excess;
    }
    if min_d2 >= r * r {
        return Ok(match op {
            BooleanOp::Cut => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(whole_box_brep(o, axes, dims)),
            },
            BooleanOp::Fuse => refused(
                "disjoint fuse — the result is a two-shell compound, not a single closed shell",
            ),
            BooleanOp::Common => empty(),
        });
    }

    // ---- contained: the ball entirely inside the box ----------------
    if (0..3).all(|k| d[k] >= r && dims[k] - d[k] >= r) {
        return Ok(match op {
            BooleanOp::Cut => refused(
                "contained cut — the internal void needs a second shell, not modeled in this \
                 family (the sphere is entirely inside the box)",
            ),
            BooleanOp::Fuse => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(whole_box_brep(o, axes, dims)),
            },
            BooleanOp::Common => BooleanOutcome {
                kind: BooleanOutcomeKind::Solid(sphere_brep_at(cs, r)),
            },
        });
    }

    // ---- the station classification ---------------------------------
    // Per axis: OnLow / OnHigh (the center on that face's plane), In
    // (strictly inside the slab), Out (outside the slabs).
    #[derive(Clone, Copy, PartialEq)]
    enum Side {
        OnLow,
        OnHigh,
        In,
        Out,
    }
    let side = |k: usize| -> Side {
        if d[k].abs() <= eps {
            Side::OnLow
        } else if (dims[k] - d[k]).abs() <= eps {
            Side::OnHigh
        } else if d[k] > 0.0 && d[k] < dims[k] {
            Side::In
        } else {
            Side::Out
        }
    };
    let sides = [side(0), side(1), side(2)];
    if sides.contains(&Side::Out) {
        return Ok(refused(
            "the sphere's center is off the box's boundary skeleton (outside a slab) — the \
             general spherical overlap is not constructed in this family",
        ));
    }
    // The stations demand bit-exactness: the section circles are great
    // circles of the operand sphere only when the center lies ON the
    // face planes exactly as the arithmetic reads it.
    for k in 0..3 {
        if sides[k] != Side::In {
            let off = if sides[k] == Side::OnHigh {
                dims[k] - d[k]
            } else {
                d[k]
            };
            if off != 0.0 {
                return Ok(refused(&format!(
                    "the sphere's center is {off:.3e} from the box's face-{k} plane — within \
                     recognition tolerance but not exactly on it; the section circles would not \
                     be great circles, and this kernel never silently snaps (place the operand \
                     exactly, or tessellate for the faceted workflow)"
                )));
            }
        }
    }
    let sig = |k: usize| -> u8 {
        if sides[k] == Side::OnHigh {
            1
        } else {
            0
        }
    };
    let on: Vec<usize> = (0..3).filter(|&k| sides[k] != Side::In).collect();
    match on.len() {
        3 => corner_station(op, o, axes, dims, cs, r, [sig(0), sig(1), sig(2)]),
        2 => edge_station(
            op,
            o,
            axes,
            dims,
            cs,
            r,
            [on[0], on[1]],
            [sig(on[0]), sig(on[1])],
        ),
        1 => face_station(op, o, axes, dims, cs, r, on[0], sig(on[0])),
        _ => {
            // All axes strictly inside the slabs. Count the face
            // crossings: axis k's low face is crossed iff the ball
            // reaches past it (d(k) < r), the high face likewise
            // (dims[k] − d(k) < r) — strict, tangency being the
            // contained arm's own boundary. Exactly ONE crossing is
            // the offset face station (the small-circle section,
            // K4-2d/3 slice 2); exactly TWO on ADJACENT faces whose
            // section disks cross each other's planes (a² + b² < r²)
            // is the offset EDGE station (the crossing-disk
            // side-clipped bite wrapping the shared edge, K4-2d/3
            // slice 7); anything else is a general overlap this
            // family does not construct, each refusing by name.
            let crossings: Vec<(usize, u8)> = (0..3)
                .flat_map(|k| {
                    let mut c = Vec::new();
                    if d[k] < r {
                        c.push((k, 0u8));
                    }
                    if dims[k] - d[k] < r {
                        c.push((k, 1u8));
                    }
                    c
                })
                .collect();
            match crossings.len() {
                1 => {
                    let (n, sig_n) = crossings[0];
                    offset_face_station(op, o, axes, dims, cs, r, n, sig_n)
                }
                2 => {
                    let (n0, s0) = crossings[0];
                    let (n1, s1) = crossings[1];
                    if n0 == n1 {
                        Ok(refused(
                            "the ball crosses both faces of the box's slab from an interior \
                             center (the through-going two-mouth section on one axis) — not \
                             constructed in this family",
                        ))
                    } else {
                        // Adjacent faces (two faces on distinct axes
                        // share a box edge). The crossing-disk test:
                        // the sections cross each other's planes iff
                        // a² + b² < r² — exact arithmetic, the tangent
                        // equality the degenerate lens's own boundary.
                        let depth = |(n, s): (usize, u8)| {
                            if s == 1 {
                                dims[n] - d[n]
                            } else {
                                d[n]
                            }
                        };
                        let a = depth((n0, s0));
                        let b = depth((n1, s1));
                        let rr = a * a + b * b;
                        if rr > r * r {
                            Ok(refused(&format!(
                                "the ball crosses two adjacent faces from an interior center \
                                 but the section disks do not cross each other's planes \
                                 (a² + b² = {rr:.6} > r² = {}) — the separate two-mouth \
                                 sections are not constructed in this family",
                                r * r
                            )))
                        } else if rr == r * r {
                            Ok(refused(
                                "the two section circles are tangent on the shared box edge \
                                 (a² + b² = r²) — the degenerate lens is not constructed, and \
                                 this kernel never silently snaps it to the edge bite",
                            ))
                        } else {
                            offset_edge_station(op, o, axes, dims, cs, r, [n0, n1], [s0, s1])
                        }
                    }
                }
                n => Ok(refused(&format!(
                    "the sphere crosses {n} of the box's faces from an interior center — the \
                     corner-ward general overlap is not constructed in this family (one crossed \
                     face is the offset bite; two crossing disks the edge wrap)",
                ))),
            }
        }
    }
}

/// The canonical corner `o + s1·(l1·a1) + s2·(l2·a2) + s3·(l3·a3)` in
/// one fixed accumulation order — every face references these exact
/// points, so shared corners are bit-identical across faces.
fn box_corner(o: Vector3, axes: [Vector3; 3], dims: [f64; 3], s: [u8; 3]) -> Vector3 {
    let mut p = o;
    for k in 0..3 {
        if s[k] == 1 {
            p = v3(
                p.x + dims[k] * axes[k].x,
                p.y + dims[k] * axes[k].y,
                p.z + dims[k] * axes[k].z,
            );
        }
    }
    p
}

/// The construction accumulators: vertices deduplicated by exact
/// equality, edges appended once and reused (the shared box edges
/// author once, every face reuses with its own traversal flag), faces
/// appended.
struct Draft {
    vertices: Vec<TopoVertex>,
    edges: Vec<TopoEdge>,
    faces: Vec<TopoFace>,
}

impl Draft {
    fn new() -> Self {
        Draft {
            vertices: Vec::new(),
            edges: Vec::new(),
            faces: Vec::new(),
        }
    }

    fn vertex(&mut self, p: Vector3) -> usize {
        match self.vertices.iter().position(|v| v.point == p) {
            Some(i) => i,
            None => {
                self.vertices.push(TopoVertex { point: p });
                self.vertices.len() - 1
            }
        }
    }

    /// One straight seam edge between exact canonical points,
    /// find-or-append by the unordered endpoint pair. Returns the edge
    /// index and the traversal flag for the a→b direction of THIS
    /// call.
    fn seam(&mut self, a: Vector3, b: Vector3) -> (usize, bool) {
        let ia = self.vertex(a);
        let ib = self.vertex(b);
        for (ei, e) in self.edges.iter().enumerate() {
            if let Curve::Bspline {
                degree: 1, points, ..
            } = &e.curve
            {
                if points.len() == 2
                    && (e.start == ia && e.end == ib || e.start == ib && e.end == ia)
                {
                    return (ei, e.start == ia);
                }
            }
        }
        self.edges.push(seam_between(a, b, ia, ib));
        (self.edges.len() - 1, true)
    }

    /// One arc chain (the classical NURBS arcs, canonical exact
    /// endpoints); the vertices for `p_start`/`p_end` are found or
    /// added. Returns the chain.
    #[allow(clippy::too_many_arguments)] // push_arc_chain's own vocabulary
    fn arc(
        &mut self,
        center: Vector3,
        x_hat: Vector3,
        y_hat: Vector3,
        radius: f64,
        p_start: Vector3,
        p_end: Vector3,
        theta_start: f64,
        sweep: f64,
    ) -> ArcChain {
        let v_start = self.vertex(p_start);
        let v_end = self.vertex(p_end);
        push_arc_chain(
            &mut self.vertices,
            &mut self.edges,
            center,
            x_hat,
            y_hat,
            radius,
            p_start,
            p_end,
            theta_start,
            sweep,
            v_start,
            v_end,
        )
    }

    /// One closed circle edge with its own seam vertex; the edge index.
    fn circle(&mut self, curve: Curve) -> usize {
        let vi = self.vertices.len();
        self.vertices.push(TopoVertex {
            point: curve.point(0.0),
        });
        self.edges.push(TopoEdge {
            curve,
            range: (0.0, std::f64::consts::TAU),
            start: vi,
            end: vi,
        });
        self.edges.len() - 1
    }

    /// The chain's uses for the traversal `from → to` (the chain must
    /// connect exactly those vertices).
    fn chain_use_between(&self, chain: &ArcChain, from: usize, to: usize) -> Vec<WireEdge> {
        let s = self.edges[chain.edges[0]].start;
        let e = self.edges[chain.edges[chain.edges.len() - 1]].end;
        if s == from && e == to {
            chain_uses(chain, true)
        } else if s == to && e == from {
            chain_uses(chain, false)
        } else {
            panic!("the arc chain does not connect the walk's vertices")
        }
    }

    fn finish(self) -> Result<ExactBRep, String> {
        let brep = ExactBRep {
            vertices: self.vertices,
            edges: self.edges,
            faces: self.faces,
        };
        brep.validate()
            .map_err(|e| format!("the constructed solid failed validation: {e}"))?;
        Ok(brep)
    }
}

/// A full rectangle face: the four canonical corners in CCW order
/// against the outward normal, the frame `(e_a, e_b)` right-handed
/// against it, the walk four seams (flags per call — shared edges
/// oppose), `reversed = false`.
#[allow(clippy::too_many_arguments)] // the frame + corners + extents
fn rect_face(
    draft: &mut Draft,
    c00: Vector3,
    c10: Vector3,
    c11: Vector3,
    c01: Vector3,
    n_out: Vector3,
    e_a: Vector3,
    _e_b: Vector3,
    la: f64,
    lb: f64,
) {
    let w = [
        draft.seam(c00, c10),
        draft.seam(c10, c11),
        draft.seam(c11, c01),
        draft.seam(c01, c00),
    ];
    draft.faces.push(TopoFace {
        surface: Surface::Plane {
            origin: c00,
            normal: n_out,
            x_dir: e_a,
        },
        uv_bounds: [0.0, la, 0.0, lb],
        wires: vec![TopoWire {
            edges: w
                .iter()
                .map(|&(e, f)| WireEdge {
                    edge: e,
                    forward: f,
                })
                .collect(),
        }],
        reversed: false,
    });
}

/// The OFFSET face station (K4-2d/3 slice 2): the center strictly
/// inside the box, the ball crossing EXACTLY ONE face — the bite is
/// the spherical cap the face plane cuts from the ball, a **small
/// circle** section (radius `ρ = √(r² − δ²)` at center depth `δ ∈
/// (0, r)` from the crossed face). The single-crossing guarantee is
/// the caller's (the classification's crossing count): the lateral
/// extent of the cap is the ball's own (≤ r, inside by the count) and
/// the far face is unreached (or tangent — measure-zero, the
/// construction unaffected).
///
/// * **Cut** — the box with the cap-shaped bite removed: the five
///   untouched faces + the station face with the disk hole + the
///   REVERSED deep cap (the solid lies outside the sphere there);
/// * **Fuse** — the box plus the outside part of the ball: the SAME
///   five faces + the holed face + the unreversed SHALLOW cap (the
///   part beyond the face plane, height `r − δ`);
/// * **Common** — the bite itself: the flat disk (`ρ`) + the
///   unreversed deep cap (height `δ + r`).
///
/// Every patch is a single-circle-trimmed sphere face — the CAP
/// class the measure's cap reader already reads exactly (`s = ∓δ`
/// with `ρ² + s² = r²`); the disk hole is the plane reader's own.
#[allow(clippy::too_many_arguments)] // the box frame is the vocabulary
fn offset_face_station(
    op: BooleanOp,
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    cs: Vector3,
    r: f64,
    n: usize,
    sig_n: u8,
) -> Result<BooleanOutcome, String> {
    let d = |k: usize| v3(cs.x - o.x, cs.y - o.y, cs.z - o.z).dot(&axes[k]);
    let others: Vec<usize> = (0..3).filter(|&m| m != n).collect();
    let (i, j) = (others[0], others[1]);
    // The inward direction of the crossed face and the center's depth
    // from it — both exact arithmetic on the input parameters.
    let u_n = if sig_n == 1 {
        v3(-axes[n].x, -axes[n].y, -axes[n].z)
    } else {
        axes[n]
    };
    let delta = if sig_n == 1 { dims[n] - d(n) } else { d(n) };
    // The caller guarantees 0 < δ < r (a strict crossing from an
    // interior center); defend it anyway — the construction's own
    // contract, never assumed from afar.
    if !(delta > 0.0 && delta < r) {
        return Err(format!(
            "the offset face station demands 0 < depth < r, got δ = {delta} (r = {r})"
        ));
    }
    let rho = (r * r - delta * delta).sqrt();
    if rho <= 0.0 {
        return Err("the offset section circle degenerated (ρ = 0)".into());
    }
    let n_out = v3(-u_n.x, -u_n.y, -u_n.z);
    // The face's frame: (e_a, e_b) from {a_i, a_j}, right-handed
    // against n_out.
    let (ia, ib) = if axes[i].cross(&axes[j]).dot(&n_out) > 0.0 {
        (i, j)
    } else {
        (j, i)
    };
    let e_a = axes[ia];
    // The foot of the sphere's center on the crossed face plane —
    // the section circle's center, canonical.
    let foot = v3(
        cs.x - delta * u_n.x,
        cs.y - delta * u_n.y,
        cs.z - delta * u_n.z,
    );
    // The section circle: a small circle of the sphere ON the face
    // plane, stored CCW around the inward normal u_n.
    let circle_curve = Curve::Circle {
        center: foot,
        axis: u_n,
        ref_dir: e_a,
        radius: rho,
    };
    let sphere_surface = Surface::Sphere {
        center: cs,
        axis: u_n,
        ref_dir: e_a,
        radius: r,
    };
    // The station face's side entry (the crossed face's plane).
    let s_n = if u_n == axes[n] { 0u8 } else { 1 };
    let s_face = |a: u8, b: u8| -> [u8; 3] {
        let mut s = [0u8, 0, 0];
        s[n] = s_n;
        s[ia] = a;
        s[ib] = b;
        s
    };

    let brep = match op {
        BooleanOp::Cut | BooleanOp::Fuse => {
            let mut draft = Draft::new();
            let circle_edge = draft.circle(circle_curve.clone());
            // The five untouched faces.
            for k in 0..3 {
                for high in [false, true] {
                    // Skip the crossed face (holed below).
                    if k == n && ((u_n == axes[n] && !high) || (u_n != axes[n] && high)) {
                        continue;
                    }
                    let n_out_k = if high {
                        axes[k]
                    } else {
                        v3(-axes[k].x, -axes[k].y, -axes[k].z)
                    };
                    let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
                    let (ka, kb) = if axes[span[0]].cross(&axes[span[1]]).dot(&n_out_k) > 0.0 {
                        (span[0], span[1])
                    } else {
                        (span[1], span[0])
                    };
                    let s_of = |a: u8, b: u8| -> [u8; 3] {
                        let mut s = [0u8, 0, 0];
                        s[k] = high as u8;
                        s[ka] = a;
                        s[kb] = b;
                        s
                    };
                    rect_face(
                        &mut draft,
                        box_corner(o, axes, dims, s_of(0, 0)),
                        box_corner(o, axes, dims, s_of(1, 0)),
                        box_corner(o, axes, dims, s_of(1, 1)),
                        box_corner(o, axes, dims, s_of(0, 1)),
                        n_out_k,
                        axes[ka],
                        axes[kb],
                        dims[ka],
                        dims[kb],
                    );
                }
            }
            // The holed face: the outer rectangle CCW + the small
            // circle hole forward (CCW around the inward normal —
            // the region-on-left rule for holes).
            let c00 = box_corner(o, axes, dims, s_face(0, 0));
            let c10 = box_corner(o, axes, dims, s_face(1, 0));
            let c11 = box_corner(o, axes, dims, s_face(1, 1));
            let c01 = box_corner(o, axes, dims, s_face(0, 1));
            let outer = [
                draft.seam(c00, c10),
                draft.seam(c10, c11),
                draft.seam(c11, c01),
                draft.seam(c01, c00),
            ];
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: c00,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [0.0, dims[ia], 0.0, dims[ib]],
                wires: vec![
                    TopoWire {
                        edges: outer
                            .iter()
                            .map(|&(ed, f)| WireEdge {
                                edge: ed,
                                forward: f,
                            })
                            .collect(),
                    },
                    TopoWire {
                        edges: vec![WireEdge {
                            edge: circle_edge,
                            forward: true,
                        }],
                    },
                ],
                reversed: false,
            });
            // The cap patch: Cut keeps the DEEP cap (into the box,
            // height δ + r — the boundary of the removed bite;
            // reversed, the solid outside the sphere), Fuse the
            // SHALLOW cap (beyond the face plane, height r − δ;
            // unreversed, the solid inside the ball there). Both
            // traverse the circle backward — the kept side follows
            // from the cap rule against the stored axis u_n.
            let reversed = op == BooleanOp::Cut;
            draft.faces.push(TopoFace {
                surface: sphere_surface,
                uv_bounds: [
                    0.0,
                    std::f64::consts::TAU,
                    -std::f64::consts::FRAC_PI_2,
                    std::f64::consts::FRAC_PI_2,
                ],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: circle_edge,
                        forward: false,
                    }],
                }],
                reversed,
            });
            draft.finish()?
        }
        BooleanOp::Common => {
            // The bite: the flat disk (ρ) + the deep cap (height
            // δ + r), sharing the circle in opposite directions.
            let mut draft = Draft::new();
            let circle_edge = draft.circle(circle_curve);
            // The disk (outward −u_n, in the crossed face plane): the
            // circle backward.
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: foot,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [-rho, rho, -rho, rho],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: circle_edge,
                        forward: false,
                    }],
                }],
                reversed: false,
            });
            // The deep cap (the +u_n side, height δ + r, unreversed):
            // the circle forward.
            draft.faces.push(TopoFace {
                surface: sphere_surface,
                uv_bounds: [
                    0.0,
                    std::f64::consts::TAU,
                    -std::f64::consts::FRAC_PI_2,
                    std::f64::consts::FRAC_PI_2,
                ],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: circle_edge,
                        forward: true,
                    }],
                }],
                reversed: false,
            });
            draft.finish()?
        }
    };
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

/// The OFFSET EDGE station (K4-2d/3 slice 7): the center strictly
/// inside the box, the ball crossing exactly TWO ADJACENT faces whose
/// section disks CROSS each other's planes (`a² + b² < r²`, the
/// depths `a`, `b` from the two crossed faces) — the crossing-disk
/// side-clipped bite wrapping the shared box edge. Each face's
/// section is a small circle (`ρ_p = √(r² − a²)` on face `p`) clipped
/// by the other face's plane into a segment-bounded footprint; the
/// two circles meet at the two points `P±` ON the shared edge (at
/// along-edge offsets `±h`, `h = √(r² − a² − b²)`); the sphere patch
/// is the **lens** `{p ≥ −a, q ≥ −b}` between the two MAJOR arcs —
/// the bigon the measure's polygon reader reads exactly by
/// Gauss–Bonnet with the small-circle geodesic curvature.
///
/// * **Cut** — the box with the bite removed: the four untouched
///   faces + the two faces NOTCHED at the shared edge (the rim
///   detouring through each face's major arc) + the REVERSED lens;
/// * **Fuse** — the box plus the outside part of the ball (the two
///   overlapping caps): the SAME notched faces + the unreversed
///   cap-union patch (the lens's complement on the same wire — Cut
///   and Fuse differ only in the flag, the K3c-2 discipline);
/// * **Common** — the bite itself: the two flat footprints (the
///   segment-clipped disks, sharing the edge chord in opposite
///   directions) + the unreversed lens.
///
/// The caller (the classification's crossing count) guarantees the
/// two crossings on distinct axes with `a² + b² < r²` strictly and no
/// crossing along the edge axis; the station defends its own
/// contract: the strict depths, the strict crossing, and the
/// along-edge pole clearance (the lens spans the ball's full
/// `±r` along the edge — the exact pole tangency on an end face is
/// refused by name, never silently emitted as a self-touching shell).
#[allow(clippy::too_many_arguments)] // the box frame is the vocabulary
fn offset_edge_station(
    op: BooleanOp,
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    cs: Vector3,
    r: f64,
    on: [usize; 2],
    sig_on: [u8; 2],
) -> Result<BooleanOutcome, String> {
    let [p, q] = on; // the two crossed axes
    let e = 3 - p - q; // the shared edge's axis
    let d = |k: usize| v3(cs.x - o.x, cs.y - o.y, cs.z - o.z).dot(&axes[k]);
    // The inward directions of the two crossed faces and the center's
    // depths from them — exact arithmetic on the input parameters.
    let u_p = if sig_on[0] == 1 {
        v3(-axes[p].x, -axes[p].y, -axes[p].z)
    } else {
        axes[p]
    };
    let u_q = if sig_on[1] == 1 {
        v3(-axes[q].x, -axes[q].y, -axes[q].z)
    } else {
        axes[q]
    };
    let a = if sig_on[0] == 1 { dims[p] - d(p) } else { d(p) };
    let b = if sig_on[1] == 1 { dims[q] - d(q) } else { d(q) };
    // The station's own contract, defended: strict crossings (the
    // caller's count guarantees them), the crossing disks, and the
    // along-edge pole clearance.
    if !(a > 0.0 && a < r && b > 0.0 && b < r) {
        return Err(format!(
            "the offset edge station demands 0 < depths < r, got a = {a}, b = {b} (r = {r})"
        ));
    }
    let rr = a * a + b * b;
    if rr >= r * r {
        return Err(format!(
            "the offset edge station demands crossing section disks (a² + b² < r²), got \
             a² + b² = {rr} vs r² = {r}²"
        ));
    }
    let t = d(e);
    if !(t > r && dims[e] - t > r) {
        return Ok(refused(
            "the bite touches the box's along-edge face at the sphere's pole — the exact \
             pole tangency is not constructed in this family (a self-touching shell, refused \
             by name rather than emitted)",
        ));
    }
    let a_e = axes[e];
    let neg_a_e = v3(-a_e.x, -a_e.y, -a_e.z);
    // The canonical station points: the feet of the sphere's center on
    // the two face planes (the section circles' centers), the section
    // radii, the half-chord on the shared edge, and the two crossing
    // points P± — one fixed accumulation order each, every face
    // referencing the same bits.
    let f_p = v3(cs.x - a * u_p.x, cs.y - a * u_p.y, cs.z - a * u_p.z);
    let f_q = v3(cs.x - b * u_q.x, cs.y - b * u_q.y, cs.z - b * u_q.z);
    let rho_p = (r * r - a * a).sqrt();
    let rho_q = (r * r - b * b).sqrt();
    let h = (r * r - a * a - b * b).sqrt();
    if !(rho_p > 0.0 && rho_q > 0.0 && h > 0.0) {
        return Err("the offset edge section degenerated (ρ or h = 0)".into());
    }
    let edge_foot = v3(
        cs.x - a * u_p.x - b * u_q.x,
        cs.y - a * u_p.y - b * u_q.y,
        cs.z - a * u_p.z - b * u_q.z,
    );
    let a_pt = v3(
        edge_foot.x - h * a_e.x,
        edge_foot.y - h * a_e.y,
        edge_foot.z - h * a_e.z,
    );
    let b_pt = v3(
        edge_foot.x + h * a_e.x,
        edge_foot.y + h * a_e.y,
        edge_foot.z + h * a_e.z,
    );
    // The local-to-global corner mapping: local (se along the edge,
    // sp along u_p, sq along u_q).
    let s_edge = |se: u8, sp: u8, sq: u8| -> [u8; 3] {
        let mut s = [0u8, 0, 0];
        s[e] = se;
        s[p] = if u_p == axes[p] { sp } else { 1 - sp };
        s[q] = if u_q == axes[q] { sq } else { 1 - sq };
        s
    };
    let corner = |se: u8, sp: u8, sq: u8| box_corner(o, axes, dims, s_edge(se, sp, sq));
    // The frame parity: (a_e × u_p) · u_q = ±1 — the two face frames'
    // handedness against their outward normals, both signs constructed.
    let par = a_e.cross(&u_p).dot(&u_q);
    // The sphere patch's own parameterization: the edge direction as
    // the axis — both section circles then read as bounded-u curves
    // (no latitude wraparound), the lens as the exterior of the walk's
    // UV image and its complement as the bounded interior, each tiling
    // through the trim-domain classifier's own arms.
    let sphere_surface = Surface::Sphere {
        center: cs,
        axis: a_e,
        ref_dir: u_p,
        radius: r,
    };
    // The two MAJOR arcs, authored in the directions the bitten faces'
    // walks use them (the parity swap keeps the all-forward cycle the
    // lens's region-on-left walk for both box handednesses — the
    // slice-1 small theorem, extended): arc_p through the +u_q side of
    // circle_p (a_pt → b_pt when par > 0), arc_q through the +u_p side
    // of circle_q (b_pt → a_pt when par > 0). Each sweep is the major
    // one, π + 2·atan2(depth, h) — the multi-segment NURBS chain the
    // classical arc authoring spells for sweeps past π.
    let arcs = |draft: &mut Draft| -> [ArcChain; 2] {
        let sweep_p = std::f64::consts::PI + 2.0 * b.atan2(h);
        let arc_p = draft.arc(
            f_p,
            u_q,
            if par > 0.0 { a_e } else { neg_a_e },
            rho_p,
            if par > 0.0 { a_pt } else { b_pt },
            if par > 0.0 { b_pt } else { a_pt },
            (-h).atan2(-b),
            sweep_p,
        );
        let sweep_q = std::f64::consts::PI + 2.0 * a.atan2(h);
        let arc_q = draft.arc(
            f_q,
            if par > 0.0 { a_e } else { neg_a_e },
            u_p,
            rho_q,
            if par > 0.0 { b_pt } else { a_pt },
            if par > 0.0 { a_pt } else { b_pt },
            (-a).atan2(h),
            sweep_q,
        );
        [arc_p, arc_q]
    };
    // The four untouched faces: far-p, far-q, and the two e-ends.
    let untouched = |draft: &mut Draft| {
        for (k, high) in [
            (p, u_p == axes[p]),
            (q, u_q == axes[q]),
            (e, false),
            (e, true),
        ] {
            let n_out = if high {
                axes[k]
            } else {
                v3(-axes[k].x, -axes[k].y, -axes[k].z)
            };
            let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
            let (ia, ib) = if axes[span[0]].cross(&axes[span[1]]).dot(&n_out) > 0.0 {
                (span[0], span[1])
            } else {
                (span[1], span[0])
            };
            let s_of = |a: u8, b: u8| -> [u8; 3] {
                let mut s = [0u8, 0, 0];
                s[k] = high as u8;
                s[ia] = a;
                s[ib] = b;
                s
            };
            rect_face(
                draft,
                box_corner(o, axes, dims, s_of(0, 0)),
                box_corner(o, axes, dims, s_of(1, 0)),
                box_corner(o, axes, dims, s_of(1, 1)),
                box_corner(o, axes, dims, s_of(0, 1)),
                n_out,
                axes[ia],
                axes[ib],
                dims[ia],
                dims[ib],
            );
        }
    };

    // ---- Cut / Fuse ------------------------------------------------
    let bite_solid = |negate: bool, reversed: bool| -> Result<ExactBRep, String> {
        let mut draft = Draft::new();
        let [arc_p, arc_q] = arcs(&mut draft);
        // F_p: the notched face — the CCW rim walk detouring through
        // the major arc on the shared-edge side.
        {
            let n_out = v3(-u_p.x, -u_p.y, -u_p.z);
            let c00 = corner(0, 0, 0);
            let c10 = corner(1, 0, 0);
            let c11 = corner(1, 0, 1);
            let c01 = corner(0, 0, 1);
            let (e_a, la, lb) = if par > 0.0 {
                (a_e, dims[e], dims[q])
            } else {
                (u_q, dims[q], dims[e])
            };
            let mut uses: Vec<WireEdge> = Vec::new();
            let mut walk = |from: Vector3, to: Vector3, uses: &mut Vec<WireEdge>| {
                let s = draft.seam(from, to);
                uses.push(WireEdge {
                    edge: s.0,
                    forward: s.1,
                });
            };
            if par > 0.0 {
                // c00 → a_pt → arc → b_pt → c10 → c11 → c01 → c00.
                walk(c00, a_pt, &mut uses);
                uses.extend(chain_uses(&arc_p, true));
                walk(b_pt, c10, &mut uses);
                walk(c10, c11, &mut uses);
                walk(c11, c01, &mut uses);
                walk(c01, c00, &mut uses);
            } else {
                // c00 → c01 → c11 → c10 → b_pt → arc → a_pt → c00.
                walk(c00, c01, &mut uses);
                walk(c01, c11, &mut uses);
                walk(c11, c10, &mut uses);
                walk(c10, b_pt, &mut uses);
                uses.extend(chain_uses(&arc_p, true));
                walk(a_pt, c00, &mut uses);
            }
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: c00,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [0.0, la, 0.0, lb],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        // F_q: the notched face, the mirrored walk.
        {
            let n_out = v3(-u_q.x, -u_q.y, -u_q.z);
            let c00 = corner(0, 0, 0);
            let c10 = corner(0, 1, 0);
            let c11 = corner(1, 1, 0);
            let c01 = corner(1, 0, 0);
            let (e_a, la, lb) = if par > 0.0 {
                (u_p, dims[p], dims[e])
            } else {
                (a_e, dims[e], dims[p])
            };
            let mut uses: Vec<WireEdge> = Vec::new();
            let mut walk = |from: Vector3, to: Vector3, uses: &mut Vec<WireEdge>| {
                let s = draft.seam(from, to);
                uses.push(WireEdge {
                    edge: s.0,
                    forward: s.1,
                });
            };
            if par > 0.0 {
                // c00 → c10 → c11 → c01 → b_pt → arc → a_pt → c00.
                walk(c00, c10, &mut uses);
                walk(c10, c11, &mut uses);
                walk(c11, c01, &mut uses);
                walk(c01, b_pt, &mut uses);
                uses.extend(chain_uses(&arc_q, true));
                walk(a_pt, c00, &mut uses);
            } else {
                // c00 → a_pt → arc → b_pt → c01 → c11 → c10 → c00.
                walk(c00, a_pt, &mut uses);
                uses.extend(chain_uses(&arc_q, true));
                walk(b_pt, c01, &mut uses);
                walk(c01, c11, &mut uses);
                walk(c11, c10, &mut uses);
                walk(c10, c00, &mut uses);
            }
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: c00,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [0.0, la, 0.0, lb],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        untouched(&mut draft);
        // The patch on the NEGATIVE cycle (the lens's region-on-left
        // walk is the all-forward positive cycle; Cut and Fuse share
        // the negative cycle and differ only in the flag).
        let chains = vec![arc_p, arc_q];
        let v_start = draft.vertex(if par > 0.0 { a_pt } else { b_pt });
        let order = arc_cycle_order(&draft, &chains, v_start);
        let cycle: Vec<&ArcChain> = order.iter().map(|&ci| &chains[ci]).collect();
        draft
            .faces
            .push(patch_face(sphere_surface.clone(), &cycle, negate, reversed));
        draft.finish()
    };

    // ---- Common: the bite itself (two footprints + the lens) ------
    let bite_itself = || -> Result<ExactBRep, String> {
        let mut draft = Draft::new();
        let [arc_p, arc_q] = arcs(&mut draft);
        let v_a = draft.vertex(a_pt);
        let v_b = draft.vertex(b_pt);
        // The footprint on face p: the segment-clipped disk — the edge
        // chord and the major arc, the region on the left against
        // −u_p (the chord toward +u_q's side, the arc closing around).
        // The UV box: the disk's own ±ρ square in the face's frame —
        // the rational arcs lie in their control hulls, both inside.
        {
            let e_a = if par > 0.0 { a_e } else { u_q };
            let mut uses: Vec<WireEdge> = Vec::new();
            if par > 0.0 {
                // a_pt → chord → b_pt → arc → a_pt.
                let (ce, cf) = draft.seam(a_pt, b_pt);
                uses.push(WireEdge {
                    edge: ce,
                    forward: cf,
                });
                uses.extend(draft.chain_use_between(&arc_p, v_b, v_a));
            } else {
                // b_pt → chord → a_pt → arc → b_pt.
                let (ce, cf) = draft.seam(b_pt, a_pt);
                uses.push(WireEdge {
                    edge: ce,
                    forward: cf,
                });
                uses.extend(draft.chain_use_between(&arc_p, v_a, v_b));
            }
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: f_p,
                    normal: v3(-u_p.x, -u_p.y, -u_p.z),
                    x_dir: e_a,
                },
                uv_bounds: [-rho_p, rho_p, -rho_p, rho_p],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        // The footprint on face q: the mirrored walk.
        {
            let e_a = if par > 0.0 { u_p } else { a_e };
            let mut uses: Vec<WireEdge> = Vec::new();
            if par > 0.0 {
                // b_pt → chord → a_pt → arc → b_pt.
                let (ce, cf) = draft.seam(b_pt, a_pt);
                uses.push(WireEdge {
                    edge: ce,
                    forward: cf,
                });
                uses.extend(draft.chain_use_between(&arc_q, v_a, v_b));
            } else {
                // a_pt → chord → b_pt → arc → a_pt.
                let (ce, cf) = draft.seam(a_pt, b_pt);
                uses.push(WireEdge {
                    edge: ce,
                    forward: cf,
                });
                uses.extend(draft.chain_use_between(&arc_q, v_b, v_a));
            }
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: f_q,
                    normal: v3(-u_q.x, -u_q.y, -u_q.z),
                    x_dir: e_a,
                },
                uv_bounds: [-rho_q, rho_q, -rho_q, rho_q],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        // The lens on the POSITIVE cycle.
        let chains = vec![arc_p, arc_q];
        let v_start = draft.vertex(if par > 0.0 { a_pt } else { b_pt });
        let order = arc_cycle_order(&draft, &chains, v_start);
        let cycle: Vec<&ArcChain> = order.iter().map(|&ci| &chains[ci]).collect();
        draft
            .faces
            .push(patch_face(sphere_surface.clone(), &cycle, false, false));
        draft.finish()
    };

    let brep = match op {
        BooleanOp::Cut => bite_solid(true, true)?,
        BooleanOp::Fuse => bite_solid(true, false)?,
        BooleanOp::Common => bite_itself()?,
    };
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

/// The whole box: six rectangle faces over the canonical corners.
fn whole_box_brep(o: Vector3, axes: [Vector3; 3], dims: [f64; 3]) -> ExactBRep {
    let mut draft = Draft::new();
    for k in 0..3 {
        for &high in &[false, true] {
            let n_out = if high {
                axes[k]
            } else {
                v3(-axes[k].x, -axes[k].y, -axes[k].z)
            };
            let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
            let (ia, ib) = if axes[span[0]].cross(&axes[span[1]]).dot(&n_out) > 0.0 {
                (span[0], span[1])
            } else {
                (span[1], span[0])
            };
            let s_of = |a: u8, b: u8| -> [u8; 3] {
                let mut s = [0u8, 0, 0];
                s[k] = high as u8;
                s[ia] = a;
                s[ib] = b;
                s
            };
            rect_face(
                &mut draft,
                box_corner(o, axes, dims, s_of(0, 0)),
                box_corner(o, axes, dims, s_of(1, 0)),
                box_corner(o, axes, dims, s_of(1, 1)),
                box_corner(o, axes, dims, s_of(0, 1)),
                n_out,
                axes[ia],
                axes[ib],
                dims[ia],
                dims[ib],
            );
        }
    }
    draft
        .finish()
        .expect("the whole box validates by construction")
}

/// The sphere patch face over one closed cycle of arc chains: the
/// full natural UV box, the wire the chained uses (all forward, in
/// cycle order; the reversed cycle for `negate`), and the `reversed`
/// flag.
fn patch_face(surface: Surface, chains: &[&ArcChain], negate: bool, reversed: bool) -> TopoFace {
    let mut uses: Vec<WireEdge> = Vec::new();
    if negate {
        for chain in chains.iter().rev() {
            uses.extend(chain_uses(chain, false));
        }
    } else {
        for chain in chains {
            uses.extend(chain_uses(chain, true));
        }
    }
    TopoFace {
        surface,
        uv_bounds: [
            0.0,
            std::f64::consts::TAU,
            -std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        ],
        wires: vec![TopoWire { edges: uses }],
        reversed,
    }
}

/// Chain the arcs (each used forward) into the closed cycle starting
/// at the vertex `start`: follow each chain's arrival vertex to the
/// next chain's departure. Returns the chain indices in cycle order —
/// the POSITIVE cycle (the authored directions chain into it, both
/// box handednesses, every station).
fn arc_cycle_order(draft: &Draft, chains: &[ArcChain], start: usize) -> Vec<usize> {
    let span_of = |c: &ArcChain| -> (usize, usize) {
        let s = draft.edges[c.edges[0]].start;
        let e = draft.edges[c.edges[c.edges.len() - 1]].end;
        (s, e)
    };
    let mut order: Vec<usize> = Vec::new();
    let mut at = start;
    while order.len() < chains.len() {
        let next = (0..chains.len())
            .find(|&ci| !order.contains(&ci) && span_of(&chains[ci]).0 == at)
            .expect("the arcs chain into a closed cycle");
        at = span_of(&chains[next]).1;
        order.push(next);
    }
    order
}

// ---------------------------------------------------------------------------
// the corner station — the octant bite
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)] // the box frame is the vocabulary
fn corner_station(
    op: BooleanOp,
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    cs: Vector3,
    r: f64,
    sig: [u8; 3],
) -> Result<BooleanOutcome, String> {
    let p_corner = box_corner(o, axes, dims, sig);
    if cs != p_corner {
        return Ok(refused(&format!(
            "the sphere's center is off the corner point by ({:.3e}, {:.3e}, {:.3e}) — the \
             station demands the exact corner; this kernel never silently snaps",
            cs.x - p_corner.x,
            cs.y - p_corner.y,
            cs.z - p_corner.z
        )));
    }
    // The bite must stay within all three adjacent faces: the
    // quarter-disk on the face ⟂ u_k needs r below the two other
    // extents, and the three together bind at the minimum extent.
    if r >= dims[0].min(dims[1]).min(dims[2]) {
        return Ok(refused(
            "the corner bite leaves the box's adjacent faces (r is not below every extent) — \
             the octant bite is the only corner construction in this family",
        ));
    }
    // The interior directions and the far-side s-entries.
    let u: [Vector3; 3] = std::array::from_fn(|k| {
        if sig[k] == 1 {
            v3(-axes[k].x, -axes[k].y, -axes[k].z)
        } else {
            axes[k]
        }
    });
    let s_far = |k: usize| -> u8 {
        if sig[k] == 1 {
            0
        } else {
            1
        }
    };
    // The axis points on the three edges from the corner — canonical.
    let x_pt = |k: usize| {
        v3(
            p_corner.x + r * u[k].x,
            p_corner.y + r * u[k].y,
            p_corner.z + r * u[k].z,
        )
    };
    let sphere_surface = Surface::Sphere {
        center: cs,
        axis: u[2],
        ref_dir: u[0],
        radius: r,
    };
    // The face F_k's frame and corners (shared by the bitten and the
    // quarter-disk variants).
    let frame_of = |k: usize| -> (usize, usize) {
        let n_out = v3(-u[k].x, -u[k].y, -u[k].z);
        let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
        if u[span[0]].cross(&u[span[1]]).dot(&n_out) > 0.0 {
            (span[0], span[1])
        } else {
            (span[1], span[0])
        }
    };
    let corners_of = |ia: usize, ib: usize| -> (Vector3, Vector3, Vector3, Vector3) {
        let s_of = |a: u8, b: u8| -> [u8; 3] {
            let mut s = sig;
            s[ia] = if a == 1 { s_far(ia) } else { sig[ia] };
            s[ib] = if b == 1 { s_far(ib) } else { sig[ib] };
            s
        };
        (
            box_corner(o, axes, dims, s_of(0, 0)),
            box_corner(o, axes, dims, s_of(1, 0)),
            box_corner(o, axes, dims, s_of(1, 1)),
            box_corner(o, axes, dims, s_of(0, 1)),
        )
    };

    // ---- Cut / Fuse: the six box faces (the adjacent ones bitten) --
    let bite_solid = |negate: bool, reversed: bool| -> Result<ExactBRep, String> {
        let mut draft = Draft::new();
        let mut arcs: Vec<ArcChain> = Vec::new();
        for k in 0..3 {
            let n_out = v3(-u[k].x, -u[k].y, -u[k].z);
            let (ia, ib) = frame_of(k);
            let (c00, c10, c11, c01) = corners_of(ia, ib);
            // The bite arc: from X_{e_b} to X_{e_a}, CCW around
            // (e_b × e_a) — the INWARD normal — authored exactly in
            // the walk's direction.
            let chain = draft.arc(
                p_corner,
                u[ib],
                u[ia],
                r,
                x_pt(ib),
                x_pt(ia),
                0.0,
                std::f64::consts::FRAC_PI_2,
            );
            arcs.push(chain);
            // The CCW walk: X_ea → c10 → c11 → c01 → X_eb → arc → X_ea.
            let lines = [
                draft.seam(x_pt(ia), c10),
                draft.seam(c10, c11),
                draft.seam(c11, c01),
                draft.seam(c01, x_pt(ib)),
            ];
            let mut uses: Vec<WireEdge> = lines
                .iter()
                .map(|&(e, f)| WireEdge {
                    edge: e,
                    forward: f,
                })
                .collect();
            uses.extend(chain_uses(&arcs[k], true));
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: c00,
                    normal: n_out,
                    x_dir: u[ia],
                },
                uv_bounds: [0.0, dims[ia], 0.0, dims[ib]],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        // The three opposite faces: plain rectangles.
        for k in 0..3 {
            let n_out = u[k];
            let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
            let (ia, ib) = if u[span[0]].cross(&u[span[1]]).dot(&n_out) > 0.0 {
                (span[0], span[1])
            } else {
                (span[1], span[0])
            };
            let s_of = |a: u8, b: u8| -> [u8; 3] {
                let mut s = sig;
                s[k] = s_far(k);
                s[ia] = if a == 1 { s_far(ia) } else { sig[ia] };
                s[ib] = if b == 1 { s_far(ib) } else { sig[ib] };
                s
            };
            rect_face(
                &mut draft,
                box_corner(o, axes, dims, s_of(0, 0)),
                box_corner(o, axes, dims, s_of(1, 0)),
                box_corner(o, axes, dims, s_of(1, 1)),
                box_corner(o, axes, dims, s_of(0, 1)),
                n_out,
                u[ia],
                u[ib],
                dims[ia],
                dims[ib],
            );
        }
        // The patch on the NEGATIVE cycle (Cut's reversed bite patch,
        // Fuse's complement bulge).
        let v_x0 = draft.vertex(x_pt(0));
        let order = arc_cycle_order(&draft, &arcs, v_x0);
        let cycle: Vec<&ArcChain> = order.iter().map(|&ci| &arcs[ci]).collect();
        draft
            .faces
            .push(patch_face(sphere_surface.clone(), &cycle, negate, reversed));
        draft.finish()
    };

    // ---- Common: the octant ball (three quarter-disks + patch) ----
    let octant_ball = || -> Result<ExactBRep, String> {
        let mut draft = Draft::new();
        let mut arcs: Vec<ArcChain> = Vec::new();
        for k in 0..3 {
            let n_out = v3(-u[k].x, -u[k].y, -u[k].z);
            let (ia, ib) = frame_of(k);
            // The quarter-disk face: the walk X_ea → arc → X_eb → P →
            // X_ea (the arc backward — the authored direction is
            // X_eb → X_ea).
            let chain = draft.arc(
                p_corner,
                u[ib],
                u[ia],
                r,
                x_pt(ib),
                x_pt(ia),
                0.0,
                std::f64::consts::FRAC_PI_2,
            );
            arcs.push(chain);
            let v_xa = draft.vertex(x_pt(ia));
            let v_xb = draft.vertex(x_pt(ib));
            let mut uses = draft.chain_use_between(&arcs[k], v_xa, v_xb);
            let d1 = draft.seam(x_pt(ib), p_corner);
            uses.push(WireEdge {
                edge: d1.0,
                forward: d1.1,
            });
            let d2 = draft.seam(p_corner, x_pt(ia));
            uses.push(WireEdge {
                edge: d2.0,
                forward: d2.1,
            });
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: p_corner,
                    normal: n_out,
                    x_dir: u[ia],
                },
                uv_bounds: [0.0, r, 0.0, r],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        let v_x0 = draft.vertex(x_pt(0));
        let order = arc_cycle_order(&draft, &arcs, v_x0);
        let cycle: Vec<&ArcChain> = order.iter().map(|&ci| &arcs[ci]).collect();
        draft
            .faces
            .push(patch_face(sphere_surface.clone(), &cycle, false, false));
        draft.finish()
    };

    let brep = match op {
        BooleanOp::Cut => bite_solid(true, true)?,
        BooleanOp::Fuse => bite_solid(true, false)?,
        BooleanOp::Common => octant_ball()?,
    };
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

// ---------------------------------------------------------------------------
// the edge station — the quarter-ball bite
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)] // the box frame is the vocabulary
fn edge_station(
    op: BooleanOp,
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    cs: Vector3,
    r: f64,
    on: [usize; 2],
    sig_on: [u8; 2],
) -> Result<BooleanOutcome, String> {
    let [p, q] = on; // the two axes whose faces carry the center
    let e = 3 - p - q; // the along-edge axis
    let d = |k: usize| v3(cs.x - o.x, cs.y - o.y, cs.z - o.z).dot(&axes[k]);
    let t = d(e);
    // The interior directions of the two adjacent faces.
    let u_p = if sig_on[0] == 1 {
        v3(-axes[p].x, -axes[p].y, -axes[p].z)
    } else {
        axes[p]
    };
    let u_q = if sig_on[1] == 1 {
        v3(-axes[q].x, -axes[q].y, -axes[q].z)
    } else {
        axes[q]
    };
    // The bite must stay within the two faces and the edge span.
    if !(r < t && r < dims[e] - t && r < dims[p] && r < dims[q]) {
        return Ok(refused(
            "the edge bite leaves its adjacent faces or the edge span — the quarter-ball bite \
             is the only edge construction in this family",
        ));
    }
    let a_e = axes[e];
    let neg_a_e = v3(-a_e.x, -a_e.y, -a_e.z);
    // The station points, canonical: the two edge-axis points and the
    // two in-face apexes.
    let a_pt = v3(cs.x - r * a_e.x, cs.y - r * a_e.y, cs.z - r * a_e.z);
    let b_pt = v3(cs.x + r * a_e.x, cs.y + r * a_e.y, cs.z + r * a_e.z);
    // The local-to-global corner mapping: local (se along the edge,
    // sp along u_p, sq along u_q).
    let s_edge = |se: u8, sp: u8, sq: u8| -> [u8; 3] {
        let mut s = [0u8, 0, 0];
        s[e] = se;
        s[p] = if u_p == axes[p] { sp } else { 1 - sp };
        s[q] = if u_q == axes[q] { sq } else { 1 - sq };
        s
    };
    let corner = |se: u8, sp: u8, sq: u8| box_corner(o, axes, dims, s_edge(se, sp, sq));
    // The frame parity: (a_e × u_p) · u_q = ±1. par > 0: F_p spans
    // (a_e, u_q) with the bite on its e_a boundary (the walk a_pt →
    // arc → b_pt), F_q spans (u_p, a_e) with the bite on its e_b
    // boundary (the walk b_pt → arc → a_pt); par < 0 mirrors both.
    let par = a_e.cross(&u_p).dot(&u_q);
    let sphere_surface = Surface::Sphere {
        center: cs,
        axis: u_p,
        ref_dir: a_e,
        radius: r,
    };
    // The four untouched faces: far-p, far-q, and the two e-ends.
    let untouched = |draft: &mut Draft| {
        // The far sides: opposite the station's faces (u_p == axes[p]
        // means the station sits on the LOW face, so far is HIGH).
        for (k, high) in [
            (p, u_p == axes[p]),
            (q, u_q == axes[q]),
            (e, false),
            (e, true),
        ] {
            let n_out = if high {
                axes[k]
            } else {
                v3(-axes[k].x, -axes[k].y, -axes[k].z)
            };
            let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
            let (ia, ib) = if axes[span[0]].cross(&axes[span[1]]).dot(&n_out) > 0.0 {
                (span[0], span[1])
            } else {
                (span[1], span[0])
            };
            let s_of = |a: u8, b: u8| -> [u8; 3] {
                let mut s = [0u8, 0, 0];
                s[k] = high as u8;
                s[ia] = a;
                s[ib] = b;
                s
            };
            rect_face(
                draft,
                box_corner(o, axes, dims, s_of(0, 0)),
                box_corner(o, axes, dims, s_of(1, 0)),
                box_corner(o, axes, dims, s_of(1, 1)),
                box_corner(o, axes, dims, s_of(0, 1)),
                n_out,
                axes[ia],
                axes[ib],
                dims[ia],
                dims[ib],
            );
        }
    };

    // ---- Cut / Fuse ------------------------------------------------
    let bite_solid = |negate: bool, reversed: bool| -> Result<ExactBRep, String> {
        let mut draft = Draft::new();
        // F_p's bite arc: through x_q, authored a_pt → b_pt (par > 0)
        // or b_pt → a_pt (par < 0) — the walk's direction, CCW around
        // F_p's inward normal.
        let arc_p = draft.arc(
            cs,
            if par > 0.0 { neg_a_e } else { a_e },
            u_q,
            r,
            if par > 0.0 { a_pt } else { b_pt },
            if par > 0.0 { b_pt } else { a_pt },
            0.0,
            std::f64::consts::PI,
        );
        // F_q's bite arc: through x_p, authored b_pt → a_pt (par > 0)
        // or a_pt → b_pt (par < 0).
        let arc_q = draft.arc(
            cs,
            if par > 0.0 { a_e } else { neg_a_e },
            u_p,
            r,
            if par > 0.0 { b_pt } else { a_pt },
            if par > 0.0 { a_pt } else { b_pt },
            0.0,
            std::f64::consts::PI,
        );
        // F_p: the CCW walk with the bite on the a_e boundary.
        {
            let n_out = v3(-u_p.x, -u_p.y, -u_p.z);
            let c00 = corner(0, 0, 0);
            let c10 = corner(1, 0, 0);
            let c11 = corner(1, 0, 1);
            let c01 = corner(0, 0, 1);
            let (e_a, la, lb) = if par > 0.0 {
                (a_e, dims[e], dims[q])
            } else {
                (u_q, dims[q], dims[e])
            };
            let mut uses: Vec<WireEdge> = Vec::new();
            let mut walk = |from: Vector3, to: Vector3, uses: &mut Vec<WireEdge>| {
                let s = draft.seam(from, to);
                uses.push(WireEdge {
                    edge: s.0,
                    forward: s.1,
                });
            };
            if par > 0.0 {
                // c00 → a_pt → arc → b_pt → c10 → c11 → c01 → c00.
                walk(c00, a_pt, &mut uses);
                uses.extend(chain_uses(&arc_p, true));
                walk(b_pt, c10, &mut uses);
                walk(c10, c11, &mut uses);
                walk(c11, c01, &mut uses);
                walk(c01, c00, &mut uses);
            } else {
                // c00 → c01 → c11 → c10 → b_pt → arc → a_pt → c00 —
                // the (u_q, a_e) frame walks the shared edge LAST
                // (c10 → c00, the −a_e direction), the detour
                // splitting it at b_pt/a_pt.
                walk(c00, c01, &mut uses);
                walk(c01, c11, &mut uses);
                walk(c11, c10, &mut uses);
                walk(c10, b_pt, &mut uses);
                uses.extend(chain_uses(&arc_p, true));
                walk(a_pt, c00, &mut uses);
            }
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: c00,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [0.0, la, 0.0, lb],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        // F_q: the CCW walk with the bite on the a_e boundary
        // (par < 0) or the e_b boundary (par > 0).
        {
            let n_out = v3(-u_q.x, -u_q.y, -u_q.z);
            let c00 = corner(0, 0, 0);
            let c10 = corner(0, 1, 0);
            let c11 = corner(1, 1, 0);
            let c01 = corner(1, 0, 0);
            let (e_a, la, lb) = if par > 0.0 {
                (u_p, dims[p], dims[e])
            } else {
                (a_e, dims[e], dims[p])
            };
            let mut uses: Vec<WireEdge> = Vec::new();
            let mut walk = |from: Vector3, to: Vector3, uses: &mut Vec<WireEdge>| {
                let s = draft.seam(from, to);
                uses.push(WireEdge {
                    edge: s.0,
                    forward: s.1,
                });
            };
            if par > 0.0 {
                // c00 → c10 → c11 → c01 → b_pt → arc → a_pt → c00.
                walk(c00, c10, &mut uses);
                walk(c10, c11, &mut uses);
                walk(c11, c01, &mut uses);
                walk(c01, b_pt, &mut uses);
                uses.extend(chain_uses(&arc_q, true));
                walk(a_pt, c00, &mut uses);
            } else {
                // c00 → a_pt → arc → b_pt → c01 → c11 → c10 → c00 —
                // the (a_e, u_p) frame walks the shared edge FIRST
                // (c00 → c01, the +a_e direction), the detour
                // splitting it at a_pt/b_pt.
                walk(c00, a_pt, &mut uses);
                uses.extend(chain_uses(&arc_q, true));
                walk(b_pt, c01, &mut uses);
                walk(c01, c11, &mut uses);
                walk(c11, c10, &mut uses);
                walk(c10, c00, &mut uses);
            }
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: c00,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [0.0, la, 0.0, lb],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        untouched(&mut draft);
        // The patch on the NEGATIVE cycle: the arcs chained
        // all-forward from a_pt (the positive cycle), then negated.
        let chains = vec![arc_p, arc_q];
        let v_a = draft.vertex(a_pt);
        let order = arc_cycle_order(&draft, &chains, v_a);
        let cycle: Vec<&ArcChain> = order.iter().map(|&ci| &chains[ci]).collect();
        draft
            .faces
            .push(patch_face(sphere_surface.clone(), &cycle, negate, reversed));
        draft.finish()
    };

    // ---- Common: the quarter ball (two half-disks + patch) --------
    let quarter_ball = || -> Result<ExactBRep, String> {
        let mut draft = Draft::new();
        // The same authored directions as the bitten faces (through
        // x_q from a_pt when par > 0, etc.) — the walks below use
        // `chain_use_between`, so only connectivity matters.
        let arc_p = draft.arc(
            cs,
            if par > 0.0 { neg_a_e } else { a_e },
            u_q,
            r,
            if par > 0.0 { a_pt } else { b_pt },
            if par > 0.0 { b_pt } else { a_pt },
            0.0,
            std::f64::consts::PI,
        );
        let arc_q = draft.arc(
            cs,
            if par > 0.0 { a_e } else { neg_a_e },
            u_p,
            r,
            if par > 0.0 { b_pt } else { a_pt },
            if par > 0.0 { a_pt } else { b_pt },
            0.0,
            std::f64::consts::PI,
        );
        let v_a = draft.vertex(a_pt);
        let v_b = draft.vertex(b_pt);
        // F_p's half-disk (in F_p's plane, through x_q): the walk
        // a_pt → diameter → b_pt → arc → a_pt.
        {
            let (uses_dia, fwd_dia) = draft.seam(a_pt, b_pt);
            let mut uses = vec![WireEdge {
                edge: uses_dia,
                forward: fwd_dia,
            }];
            uses.extend(draft.chain_use_between(&arc_p, v_b, v_a));
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: cs,
                    normal: v3(-u_p.x, -u_p.y, -u_p.z),
                    x_dir: a_e,
                },
                uv_bounds: [-r, r, 0.0, r],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        // F_q's half-disk (in F_q's plane, through x_p): the walk
        // b_pt → diameter → a_pt → arc → b_pt.
        {
            let (uses_dia, fwd_dia) = draft.seam(b_pt, a_pt);
            let mut uses = vec![WireEdge {
                edge: uses_dia,
                forward: fwd_dia,
            }];
            uses.extend(draft.chain_use_between(&arc_q, v_a, v_b));
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: cs,
                    normal: v3(-u_q.x, -u_q.y, -u_q.z),
                    x_dir: u_p,
                },
                uv_bounds: [0.0, r, -r, r],
                wires: vec![TopoWire { edges: uses }],
                reversed: false,
            });
        }
        // The patch on the POSITIVE cycle.
        let chains = vec![arc_p, arc_q];
        let order = arc_cycle_order(&draft, &chains, v_a);
        let cycle: Vec<&ArcChain> = order.iter().map(|&ci| &chains[ci]).collect();
        draft
            .faces
            .push(patch_face(sphere_surface.clone(), &cycle, false, false));
        draft.finish()
    };

    let brep = match op {
        BooleanOp::Cut => bite_solid(true, true)?,
        BooleanOp::Fuse => bite_solid(true, false)?,
        BooleanOp::Common => quarter_ball()?,
    };
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

// ---------------------------------------------------------------------------
// the face station — the half-ball bite behind a circular hole
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)] // the box frame is the vocabulary
fn face_station(
    op: BooleanOp,
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    cs: Vector3,
    r: f64,
    n: usize,
    sig_n: u8,
) -> Result<BooleanOutcome, String> {
    let d = |k: usize| v3(cs.x - o.x, cs.y - o.y, cs.z - o.z).dot(&axes[k]);
    let others: Vec<usize> = (0..3).filter(|&m| m != n).collect();
    let (i, j) = (others[0], others[1]);
    // The interior direction of the station's face.
    let u_n = if sig_n == 1 {
        v3(-axes[n].x, -axes[n].y, -axes[n].z)
    } else {
        axes[n]
    };
    // The bite must stay interior to the face and within the depth.
    if !(r < d(i) && r < dims[i] - d(i) && r < d(j) && r < dims[j] - d(j) && r < dims[n]) {
        return Ok(refused(
            "the face bite's circle leaves the face (or the half ball exceeds the depth) — the \
             interior-hole construction is the only face bite in this family",
        ));
    }
    let n_out = v3(-u_n.x, -u_n.y, -u_n.z);
    // The face's frame: (e_a, e_b) from {a_i, a_j}, right-handed
    // against n_out.
    let (ia, ib) = if axes[i].cross(&axes[j]).dot(&n_out) > 0.0 {
        (i, j)
    } else {
        (j, i)
    };
    let e_a = axes[ia];
    // The station face's side entry (the plane the center sits on).
    let s_n = if u_n == axes[n] { 0u8 } else { 1 };
    let s_face = |a: u8, b: u8| -> [u8; 3] {
        let mut s = [0u8, 0, 0];
        s[n] = s_n;
        s[ia] = a;
        s[ib] = b;
        s
    };
    // The section circle: a great circle of the sphere (the center
    // is ON the face plane, bit-exact by the station demand), stored
    // CCW around the INWARD normal u_n.
    let circle_curve = Curve::Circle {
        center: cs,
        axis: u_n,
        ref_dir: e_a,
        radius: r,
    };
    let sphere_surface = Surface::Sphere {
        center: cs,
        axis: u_n,
        ref_dir: e_a,
        radius: r,
    };

    let brep = match op {
        BooleanOp::Cut | BooleanOp::Fuse => {
            let mut draft = Draft::new();
            let circle_edge = draft.circle(circle_curve.clone());
            // The five untouched faces.
            for k in 0..3 {
                for high in [false, true] {
                    // Skip the station's own face (holed below).
                    if k == n && ((u_n == axes[n] && !high) || (u_n != axes[n] && high)) {
                        continue;
                    }
                    let n_out_k = if high {
                        axes[k]
                    } else {
                        v3(-axes[k].x, -axes[k].y, -axes[k].z)
                    };
                    let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
                    let (ka, kb) = if axes[span[0]].cross(&axes[span[1]]).dot(&n_out_k) > 0.0 {
                        (span[0], span[1])
                    } else {
                        (span[1], span[0])
                    };
                    let s_of = |a: u8, b: u8| -> [u8; 3] {
                        let mut s = [0u8, 0, 0];
                        s[k] = high as u8;
                        s[ka] = a;
                        s[kb] = b;
                        s
                    };
                    rect_face(
                        &mut draft,
                        box_corner(o, axes, dims, s_of(0, 0)),
                        box_corner(o, axes, dims, s_of(1, 0)),
                        box_corner(o, axes, dims, s_of(1, 1)),
                        box_corner(o, axes, dims, s_of(0, 1)),
                        n_out_k,
                        axes[ka],
                        axes[kb],
                        dims[ka],
                        dims[kb],
                    );
                }
            }
            // The holed face: the outer rectangle CCW + the hole
            // circle forward (CCW around the inward normal — the
            // region-on-left rule for holes).
            let c00 = box_corner(o, axes, dims, s_face(0, 0));
            let c10 = box_corner(o, axes, dims, s_face(1, 0));
            let c11 = box_corner(o, axes, dims, s_face(1, 1));
            let c01 = box_corner(o, axes, dims, s_face(0, 1));
            let outer = [
                draft.seam(c00, c10),
                draft.seam(c10, c11),
                draft.seam(c11, c01),
                draft.seam(c01, c00),
            ];
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: c00,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [0.0, dims[ia], 0.0, dims[ib]],
                wires: vec![
                    TopoWire {
                        edges: outer
                            .iter()
                            .map(|&(ed, f)| WireEdge {
                                edge: ed,
                                forward: f,
                            })
                            .collect(),
                    },
                    TopoWire {
                        edges: vec![WireEdge {
                            edge: circle_edge,
                            forward: true,
                        }],
                    },
                ],
                reversed: false,
            });
            // The hemisphere patch: the station's INWARD side for Cut
            // (reversed — the solid lies outside the sphere), the
            // OUTWARD side for Fuse (unreversed); both traverse the
            // circle backward (the negative orientation w.r.t. the
            // stored axis — the kept side follows from the cap rule).
            let reversed = op == BooleanOp::Cut;
            draft.faces.push(TopoFace {
                surface: sphere_surface,
                uv_bounds: [
                    0.0,
                    std::f64::consts::TAU,
                    -std::f64::consts::FRAC_PI_2,
                    std::f64::consts::FRAC_PI_2,
                ],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: circle_edge,
                        forward: false,
                    }],
                }],
                reversed,
            });
            draft.finish()?
        }
        BooleanOp::Common => {
            // The half ball: the flat disk + the cap, sharing the
            // circle in opposite directions.
            let mut draft = Draft::new();
            let circle_edge = draft.circle(circle_curve);
            // The disk (outward −u_n): the circle backward.
            draft.faces.push(TopoFace {
                surface: Surface::Plane {
                    origin: cs,
                    normal: n_out,
                    x_dir: e_a,
                },
                uv_bounds: [-r, r, -r, r],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: circle_edge,
                        forward: false,
                    }],
                }],
                reversed: false,
            });
            // The cap (the +u_n hemisphere, unreversed): the circle
            // forward.
            draft.faces.push(TopoFace {
                surface: sphere_surface,
                uv_bounds: [
                    0.0,
                    std::f64::consts::TAU,
                    -std::f64::consts::FRAC_PI_2,
                    std::f64::consts::FRAC_PI_2,
                ],
                wires: vec![TopoWire {
                    edges: vec![WireEdge {
                        edge: circle_edge,
                        forward: true,
                    }],
                }],
                reversed: false,
            });
            draft.finish()?
        }
    };
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

// ---------------------------------------------------------------------------
// K4-2d/3 slice 3 — the box×box family: the rectilinear composition
// ---------------------------------------------------------------------------

/// The per-axis level intervals of one box of the arrangement, in
/// the base frame's coordinates (`t[k]` the parameter along
/// `axes[k]`).
struct Slab {
    lo: [f64; 3],
    hi: [f64; 3],
}

/// One rectilinear region of a face plane: the outer boundary CCW in
/// the plane's `(ia, ib)` level coordinates, and the optional hole
/// boundary CW.
struct PlaneRegion {
    outer: Vec<(f64, f64)>,
    hole: Option<Vec<(f64, f64)>>,
}

/// The canonical point at per-axis levels — ONE fixed accumulation
/// order (axis 0, then 1, then 2), so every shared vertex is
/// bit-identical across the faces that reference it (the exactness
/// discipline; the box-corner precedent).
fn point_at(o: Vector3, axes: [Vector3; 3], t: [f64; 3]) -> Vector3 {
    let mut p = o;
    for k in 0..3 {
        p = v3(
            p.x + t[k] * axes[k].x,
            p.y + t[k] * axes[k].y,
            p.z + t[k] * axes[k].z,
        );
    }
    p
}

fn rect_ccw(i0: f64, i1: f64, j0: f64, j1: f64) -> Vec<(f64, f64)> {
    vec![(i0, j0), (i1, j0), (i1, j1), (i0, j1)]
}

fn rect_region(i0: f64, i1: f64, j0: f64, j1: f64) -> PlaneRegion {
    PlaneRegion {
        outer: rect_ccw(i0, i1, j0, j1),
        hole: None,
    }
}

fn poly_region(outer: Vec<(f64, f64)>) -> PlaneRegion {
    PlaneRegion { outer, hole: None }
}

/// The rect `r1 = [i0, i1]×[j0, j1]` minus the rect `r2` (both
/// axis-aligned in level coordinates, `r2` clipped into `r1` by the
/// caller): the full rect, the L (two adjacent touches, 6 vertices),
/// the step/C (one touch, 8 vertices), the ring (no touch — outer
/// plus hole), or the one-two bands (a spanning touch) — every
/// region a face the measure's polygon and annulus paths read
/// exactly, and the hole-free ones the tessellator's planar ear-clip
/// renders watertight.
fn rect_minus_rect(r1: [f64; 4], r2: [f64; 4]) -> Vec<PlaneRegion> {
    let [i0, i1, j0, j1] = r1;
    let [p0, p1, q0, q1] = r2;
    // No effective overlap: the face keeps its full rect.
    if !(p0 < p1 && q0 < q1) || p1 <= i0 || p0 >= i1 || q1 <= j0 || q0 >= j1 {
        return vec![rect_region(i0, i1, j0, j1)];
    }
    // Full cover: the face is consumed.
    if p0 <= i0 && p1 >= i1 && q0 <= j0 && q1 >= j1 {
        return Vec::new();
    }
    let touch = (p0 == i0, p1 == i1, q0 == j0, q1 == j1);
    match touch {
        // Spans the i axis: the two j-bands.
        (true, true, false, false) => {
            vec![rect_region(i0, i1, j0, q0), rect_region(i0, i1, q1, j1)]
        }
        (true, true, true, false) => vec![rect_region(i0, i1, q1, j1)],
        (true, true, false, true) => vec![rect_region(i0, i1, j0, q0)],
        (true, true, true, true) => Vec::new(),
        // Spans the j axis: the two i-bands.
        (false, false, true, true) => {
            vec![rect_region(i0, p0, j0, j1), rect_region(p1, i1, j0, j1)]
        }
        (true, false, true, true) => vec![rect_region(p1, i1, j0, j1)],
        (false, true, true, true) => vec![rect_region(i0, p0, j0, j1)],
        // Touches nothing: the ring — outer CCW, hole CW.
        (false, false, false, false) => {
            let mut hole = rect_ccw(p0, p1, q0, q1);
            hole.reverse();
            vec![PlaneRegion {
                outer: rect_ccw(i0, i1, j0, j1),
                hole: Some(hole),
            }]
        }
        // Touches one side: the step (C) polygon, 8 vertices.
        (true, false, false, false) => vec![poly_region(vec![
            (i0, j0),
            (i1, j0),
            (i1, j1),
            (i0, j1),
            (i0, q1),
            (p1, q1),
            (p1, q0),
            (i0, q0),
        ])],
        (false, true, false, false) => vec![poly_region(vec![
            (i0, j0),
            (i1, j0),
            (i1, q0),
            (p0, q0),
            (p0, q1),
            (i1, q1),
            (i1, j1),
            (i0, j1),
        ])],
        (false, false, true, false) => vec![poly_region(vec![
            (i0, j0),
            (p0, j0),
            (p0, q1),
            (p1, q1),
            (p1, j0),
            (i1, j0),
            (i1, j1),
            (i0, j1),
        ])],
        (false, false, false, true) => vec![poly_region(vec![
            (i0, j0),
            (i1, j0),
            (i1, j1),
            (p1, j1),
            (p1, q0),
            (p0, q0),
            (p0, j1),
            (i0, j1),
        ])],
        // Touches two adjacent sides: the L polygon, 6 vertices.
        (true, false, true, false) => vec![poly_region(vec![
            (p1, j0),
            (i1, j0),
            (i1, j1),
            (i0, j1),
            (i0, q1),
            (p1, q1),
        ])],
        (false, true, true, false) => vec![poly_region(vec![
            (i0, j0),
            (p0, j0),
            (p0, q1),
            (p1, q1),
            (i1, j1),
            (i0, j1),
        ])],
        (true, false, false, true) => vec![poly_region(vec![
            (i0, j0),
            (i1, j0),
            (i1, j1),
            (p1, j1),
            (p1, q0),
            (i0, q0),
        ])],
        (false, true, false, true) => vec![poly_region(vec![
            (i0, j0),
            (i1, j0),
            (i1, q0),
            (p0, q0),
            (p0, j1),
            (i0, j1),
        ])],
    }
}

/// Emit the faces on one plane of the arrangement: the owner box's
/// face rect (its other two axes' `[lo, hi]`), minus the footprint
/// slab where present (the overlap box's own other-extents — the
/// caller's condition says whether it bites this plane). The plane
/// is frame axis `k` at level `t[k] = level`, the solid's outward
/// normal `outward·axes[k]`. The `(ia, ib)` reading order is chosen
/// right-handed against that normal (both frame handednesses carry);
/// every region becomes one `TopoFace` — a plane trimmed by its
/// rectilinear wire, the outer wire CCW viewed from the outward
/// normal, holes CW — with the vertices canonical (one accumulation
/// formula) and the edges shared by exact endpoint equality.
#[allow(clippy::too_many_arguments)] // the plane's own vocabulary
fn emit_plane_faces(
    draft: &mut Draft,
    oa: Vector3,
    axes: [Vector3; 3],
    k: usize,
    level: f64,
    outward: f64,
    owner: &Slab,
    footprint: Option<&Slab>,
) {
    let n_out = v3(
        axes[k].x * outward,
        axes[k].y * outward,
        axes[k].z * outward,
    );
    let mut others = [0usize; 2];
    let mut m = 0;
    for a in 0..3 {
        if a != k {
            others[m] = a;
            m += 1;
        }
    }
    let (ia, ib) = if axes[others[0]].cross(&axes[others[1]]).dot(&n_out) > 0.0 {
        (others[0], others[1])
    } else {
        (others[1], others[0])
    };
    let r1 = [owner.lo[ia], owner.hi[ia], owner.lo[ib], owner.hi[ib]];
    let regions = match footprint {
        None => vec![rect_region(r1[0], r1[1], r1[2], r1[3])],
        Some(fp) => rect_minus_rect(r1, [fp.lo[ia], fp.hi[ia], fp.lo[ib], fp.hi[ib]]),
    };
    if regions.is_empty() {
        return; // the face is fully consumed
    }
    // The plane's canonical origin: the owner rect's low corner.
    let mut origin_levels = [0.0f64; 3];
    origin_levels[k] = level;
    origin_levels[ia] = r1[0];
    origin_levels[ib] = r1[2];
    let origin = point_at(oa, axes, origin_levels);
    let x_dir = axes[ia];
    for region in regions {
        let point_of = |u: f64, v: f64| -> Vector3 {
            let mut t = [0.0f64; 3];
            t[k] = level;
            t[ia] = u;
            t[ib] = v;
            point_at(oa, axes, t)
        };
        // The wire: the outer boundary's segments in CCW order.
        let pts: Vec<Vector3> = region.outer.iter().map(|&(u, v)| point_of(u, v)).collect();
        let n = pts.len();
        let mut edges = Vec::with_capacity(n);
        for p in 0..n {
            let (e, f) = draft.seam(pts[p], pts[(p + 1) % n]);
            edges.push(WireEdge {
                edge: e,
                forward: f,
            });
        }
        let mut wires = vec![TopoWire { edges }];
        // The hole boundary, CW: the annulus faces.
        if let Some(hole) = &region.hole {
            let hpts: Vec<Vector3> = hole.iter().map(|&(u, v)| point_of(u, v)).collect();
            let hn = hpts.len();
            let mut hedges = Vec::with_capacity(hn);
            for p in 0..hn {
                let (e, f) = draft.seam(hpts[p], hpts[(p + 1) % hn]);
                hedges.push(WireEdge {
                    edge: e,
                    forward: f,
                });
            }
            wires.push(TopoWire { edges: hedges });
        }
        // The UV bounds: the region's extents in the plane's own
        // (u, v) reading (exact level differences against the
        // origin's levels).
        let umin = region
            .outer
            .iter()
            .map(|&(u, _)| u - origin_levels[ia])
            .fold(f64::INFINITY, f64::min);
        let umax = region
            .outer
            .iter()
            .map(|&(u, _)| u - origin_levels[ia])
            .fold(f64::NEG_INFINITY, f64::max);
        let vmin = region
            .outer
            .iter()
            .map(|&(_, v)| v - origin_levels[ib])
            .fold(f64::INFINITY, f64::min);
        let vmax = region
            .outer
            .iter()
            .map(|&(_, v)| v - origin_levels[ib])
            .fold(f64::NEG_INFINITY, f64::max);
        draft.faces.push(TopoFace {
            surface: Surface::Plane {
                origin,
                normal: n_out,
                x_dir,
            },
            uv_bounds: [umin, umax, vmin, vmax],
            wires,
            reversed: false,
        });
    }
}

/// A whole box from per-axis level pairs — the classification tier's
/// whole-shape answers and Common's overlap box, built by the SAME
/// emission the partial construction uses (one construction, one
/// discipline).
fn framed_box_brep(o: Vector3, axes: [Vector3; 3], lo: [f64; 3], hi: [f64; 3]) -> ExactBRep {
    let slab = Slab { lo, hi };
    let mut draft = Draft::new();
    for k in 0..3 {
        for &high in &[false, true] {
            let level = if high { hi[k] } else { lo[k] };
            let outward = if high { 1.0 } else { -1.0 };
            emit_plane_faces(&mut draft, o, axes, k, level, outward, &slab, None);
        }
    }
    draft
        .finish()
        .expect("the whole box validates by construction")
}

/// The number of edge-connected face components — the structural
/// shell count. Every edge being used twice (the measure's
/// closed-shell rule) says each component is closed; CONNECTIVITY is
/// what separates one solid from a compound, and the split cut is
/// refused on it (by name, with the count).
fn shell_components(brep: &ExactBRep) -> usize {
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            let grandparent = parent[parent[x]];
            parent[x] = grandparent;
            x = grandparent;
        }
        x
    }
    let mut parent: Vec<usize> = (0..brep.faces.len()).collect();
    for e in 0..brep.edges.len() {
        if brep.is_degenerate(e) {
            continue;
        }
        let mut users: Vec<usize> = Vec::new();
        for (fi, f) in brep.faces.iter().enumerate() {
            if f.wires
                .iter()
                .any(|w| w.edges.iter().any(|we| we.edge == e))
            {
                users.push(fi);
            }
        }
        if let Some(&first) = users.first() {
            for &f2 in &users[1..] {
                let a = find(&mut parent, first);
                let b = find(&mut parent, f2);
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut roots = std::collections::HashSet::new();
    for f in 0..brep.faces.len() {
        roots.insert(find(&mut parent, f));
    }
    roots.len()
}

/// Finish a construction as a single-shell outcome: validated, then
/// the structural shell count — a compound (the split cut) refuses
/// by name with the count, never a plausible two-solid answer.
fn finish_single_shell(draft: Draft, what: &str) -> Result<BooleanOutcome, String> {
    if draft.faces.is_empty() {
        return Ok(empty());
    }
    let brep = draft
        .finish()
        .map_err(|e| format!("{what}: the construction failed validation: {e}"))?;
    let shells = shell_components(&brep);
    if shells > 1 {
        return Ok(refused(&format!(
            "{what} splits the base into {shells} shells — a compound of closed shells, not a \
             single-solid result in this family"
        )));
    }
    Ok(BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    })
}

/// The cut `A ∖ O` as the rectilinear composition: the base's six
/// faces (each minus the overlap's footprint wherever the overlap
/// touches that face), plus the overlap's own faces at levels
/// strictly interior to the base's slabs (the bite's walls, outward
/// normals pointing INTO the removed region). The construction reads
/// the overlap box only — flush tool faces contribute nothing, which
/// is the input's own arithmetic, not a snap.
fn box_box_cut(
    oa: Vector3,
    axes: [Vector3; 3],
    dims_a: [f64; 3],
    o_lo: [f64; 3],
    o_hi: [f64; 3],
) -> Result<BooleanOutcome, String> {
    let base = Slab {
        lo: [0.0; 3],
        hi: dims_a,
    };
    let over = Slab { lo: o_lo, hi: o_hi };
    let mut draft = Draft::new();
    for k in 0..3 {
        for &high in &[false, true] {
            let level = if high { dims_a[k] } else { 0.0 };
            let outward = if high { 1.0 } else { -1.0 };
            let touches = if high {
                o_hi[k] == dims_a[k]
            } else {
                o_lo[k] == 0.0
            };
            emit_plane_faces(
                &mut draft,
                oa,
                axes,
                k,
                level,
                outward,
                &base,
                if touches { Some(&over) } else { None },
            );
        }
    }
    for k in 0..3 {
        for &high in &[false, true] {
            let (level, interior) = if high {
                (o_hi[k], o_hi[k] < dims_a[k])
            } else {
                (o_lo[k], o_lo[k] > 0.0)
            };
            if interior {
                // The bite wall: the solid lies on the far side, the
                // outward normal points INTO the overlap.
                let outward = if high { -1.0 } else { 1.0 };
                emit_plane_faces(&mut draft, oa, axes, k, level, outward, &over, None);
            }
        }
    }
    finish_single_shell(draft, "the cut")
}

/// The fuse `A ∪ B` as the rectilinear composition: the base's six
/// faces (each minus the overlap's footprint wherever the tool
/// STRICTLY straddles the face's plane), plus the tool's six faces
/// (each minus the footprint wherever its level lies strictly
/// interior to the base's slab). Flush faces were refused by name
/// before this point — the coplanar merge is the named next slice,
/// never silent.
#[allow(clippy::too_many_arguments)] // the arrangement's own vocabulary
fn box_box_fuse(
    oa: Vector3,
    axes: [Vector3; 3],
    dims_a: [f64; 3],
    b_lo: [f64; 3],
    b_hi: [f64; 3],
    o_lo: [f64; 3],
    o_hi: [f64; 3],
) -> Result<BooleanOutcome, String> {
    let base = Slab {
        lo: [0.0; 3],
        hi: dims_a,
    };
    let tool = Slab { lo: b_lo, hi: b_hi };
    let over = Slab { lo: o_lo, hi: o_hi };
    let mut draft = Draft::new();
    for k in 0..3 {
        for &high in &[false, true] {
            let level = if high { dims_a[k] } else { 0.0 };
            let outward = if high { 1.0 } else { -1.0 };
            let straddles = if high {
                b_hi[k] > dims_a[k]
            } else {
                b_lo[k] < 0.0
            };
            emit_plane_faces(
                &mut draft,
                oa,
                axes,
                k,
                level,
                outward,
                &base,
                if straddles { Some(&over) } else { None },
            );
        }
    }
    for k in 0..3 {
        for &high in &[false, true] {
            let level = if high { b_hi[k] } else { b_lo[k] };
            let outward = if high { 1.0 } else { -1.0 };
            let interior = 0.0 < level && level < dims_a[k];
            emit_plane_faces(
                &mut draft,
                oa,
                axes,
                k,
                level,
                outward,
                &tool,
                if interior { Some(&over) } else { None },
            );
        }
    }
    finish_single_shell(draft, "the fuse")
}

/// K4-2d/3 slice 3 — the **box×box family**: two frame-parallel
/// boxes' three booleans as exact rectilinear compositions. The
/// frames must be EXACTLY parallel (each tool axis ±-aligned with a
/// base axis, the dots bit-exact ±1 — a rotated or near-parallel
/// frame refuses by name; this kernel never silently snaps). The
/// tool's extents are recomputed in the base's frame, the overlap
/// box `O = A ∩ B` is the arrangement's vocabulary, and every result
/// face is a plane trimmed by an axis-aligned rectilinear wire —
/// the polygon and annulus paths the measure's plane reader already
/// reads exactly; no new measure.
///
/// **The stations** (how `O` touches the base's boundary): the
/// corner (one corner — the 9-face notch cut, the 12-face corner
/// union), the edge (one edge — the through-notch, the
/// octagonal-prism fuse), the face (one face — a plain slab cut, the
/// stepped-box fuse with the annular shoulder), the slot (through on
/// one axis, interior on two — the genus-1 cut), the groove (through,
/// touching one face, interior on one). **Common is always the
/// overlap box** — the intersection of two co-oriented boxes is a
/// box, for every overlap topology at once.
///
/// **Cut** reads the overlap box only (flush tool faces contribute
/// nothing — the construction is the input's own arithmetic);
/// **Fuse** demands strict straddling (a flush tool face would need
/// the coplanar merge — the named next slice, refused by name).
/// Both close through the structural shell count: a construction
/// that splits the base into several shells (the through-two-axes
/// cut) refuses by name with the count.
///
/// The classification tier answers the whole shapes: disjoint (the
/// cut is the base, the common empty, the fuse a refused two-shell
/// compound), contained (the cut empty, the fuse the tool, the
/// common the base), engulf (the fuse the base, the common the tool,
/// and the strictly-interior cut the refused internal void — a flush
/// containment cut is a legitimate notch through the construction).
pub fn box_box_boolean(
    op: BooleanOp,
    oa: Vector3,
    axes_a: [Vector3; 3],
    dims_a: [f64; 3],
    ob: Vector3,
    axes_b: [Vector3; 3],
    dims_b: [f64; 3],
) -> Result<BooleanOutcome, String> {
    for (what, axes) in [("base", axes_a), ("tool", axes_b)] {
        require_unit(axes[0], &format!("the {what} box's axis 1"))?;
        require_unit(axes[1], &format!("the {what} box's axis 2"))?;
        require_unit(axes[2], &format!("the {what} box's axis 3"))?;
        if (axes[0].dot(&axes[1])).abs() > 1e-9
            || (axes[0].dot(&axes[2])).abs() > 1e-9
            || (axes[1].dot(&axes[2])).abs() > 1e-9
        {
            return Err(format!(
                "the {what} box's axes must be mutually perpendicular"
            ));
        }
    }
    if dims_a.iter().any(|&d| d <= 0.0) || dims_b.iter().any(|&d| d <= 0.0) {
        return Err("the box extents must be positive".into());
    }
    // The frame map: tool axis i → (base axis k, sign), the dot
    // EXACTLY ±1. A rotated or near-parallel frame refuses by name.
    let mut map: [Option<(usize, f64)>; 3] = [None; 3];
    for (i, b) in axes_b.iter().enumerate() {
        for (k, a) in axes_a.iter().enumerate() {
            let d = b.dot(a);
            if d == 1.0 || d == -1.0 {
                map[i] = Some((k, d));
                break;
            }
        }
        if map[i].is_none() {
            return Ok(refused(
                "the boxes' frames are not exactly parallel — the rotated box×box is outside \
                 this family (the extents would be a snap's, not the input's); align the frames \
                 exactly, or tessellate for the faceted workflow — this kernel never silently \
                 snaps",
            ));
        }
    }
    // The bijection (each base axis claimed once): orthogonality of
    // both frames guarantees it — asserted, not assumed.
    let mut claimed = [false; 3];
    for m in &map {
        let (k, _) = m.expect("every tool axis mapped");
        if claimed[k] {
            return Err(
                "the tool frame's axes map onto one base axis twice — not a box frame".into(),
            );
        }
        claimed[k] = true;
    }
    // The tool's extents in the base frame: one offset projection
    // per axis plus the signed span.
    let off = v3(ob.x - oa.x, ob.y - oa.y, ob.z - oa.z);
    let mut b_lo = [0.0f64; 3];
    let mut b_hi = [0.0f64; 3];
    for i in 0..3 {
        let (k, s) = map[i].expect("every tool axis mapped");
        let d = off.dot(&axes_a[k]);
        let span = s * dims_b[i];
        b_lo[k] = d + span.min(0.0);
        b_hi[k] = d + span.max(0.0);
    }
    // The overlap box (the arrangement's vocabulary).
    let mut o_lo = [0.0f64; 3];
    let mut o_hi = [0.0f64; 3];
    for k in 0..3 {
        o_lo[k] = b_lo[k].max(0.0);
        o_hi[k] = b_hi[k].min(dims_a[k]);
    }
    let solid = |brep: ExactBRep| BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    };
    // ---- disjoint (external tangency included) ----------------------
    if !(0..3).all(|k| o_lo[k] < o_hi[k]) {
        return Ok(match op {
            BooleanOp::Cut => solid(framed_box_brep(oa, axes_a, [0.0; 3], dims_a)),
            BooleanOp::Fuse => refused(
                "disjoint fuse — the result is a two-shell compound, not a single closed shell",
            ),
            BooleanOp::Common => empty(),
        });
    }
    // ---- contained: the base inside the tool -------------------------
    if (0..3).all(|k| b_lo[k] <= 0.0 && b_hi[k] >= dims_a[k]) {
        return Ok(match op {
            BooleanOp::Cut => empty(),
            BooleanOp::Fuse => solid(framed_box_brep(oa, axes_a, b_lo, b_hi)),
            BooleanOp::Common => solid(framed_box_brep(oa, axes_a, [0.0; 3], dims_a)),
        });
    }
    // ---- engulf: the tool inside the base ----------------------------
    if (0..3).all(|k| b_lo[k] >= 0.0 && b_hi[k] <= dims_a[k]) {
        return Ok(match op {
            BooleanOp::Fuse => solid(framed_box_brep(oa, axes_a, [0.0; 3], dims_a)),
            BooleanOp::Common => solid(framed_box_brep(oa, axes_a, b_lo, b_hi)),
            BooleanOp::Cut => {
                if (0..3).all(|k| b_lo[k] > 0.0 && b_hi[k] < dims_a[k]) {
                    refused(
                        "the internal void — the tool box is strictly inside the base, and the \
                         void needs a second shell, not modeled in this family",
                    )
                } else {
                    // Flush containment: the complement is a
                    // legitimate single-shell notch — the
                    // construction's own path.
                    return box_box_cut(oa, axes_a, dims_a, o_lo, o_hi);
                }
            }
        });
    }
    // ---- the partial tier --------------------------------------------
    match op {
        BooleanOp::Common => Ok(solid(framed_box_brep(oa, axes_a, o_lo, o_hi))),
        BooleanOp::Cut => box_box_cut(oa, axes_a, dims_a, o_lo, o_hi),
        BooleanOp::Fuse => {
            if (0..3).any(|k| b_lo[k] == 0.0 || b_hi[k] == dims_a[k]) {
                return Ok(refused(
                    "a coplanar fuse — the tool's face lies flush on the base's face plane, and \
                     the union would need the coplanar face merge (the named next slice); this \
                     kernel never silently merges",
                ));
            }
            box_box_fuse(oa, axes_a, dims_a, b_lo, b_hi, o_lo, o_hi)
        }
    }
}

// ---------------------------------------------------------------------------
// K4-2d/3 slice 4 — the box×cylinder family
// ---------------------------------------------------------------------------

/// The exact B-rep boolean of a box and an axis-parallel cylinder
/// (K4-2d/3 slice 4 — the pairing table's third amendment).
///
/// **The frame contract**: the cylinder's axis dot with exactly one
/// box axis is bit-exactly `±1` — never a snapped parallel. A
/// rotated, oblique, or near-parallel axis refuses by name: the
/// section circles would not lie in the box's face planes, and their
/// extents would be a snap's, not the input's. The cylinder is then
/// read in the box's own frame: the cross-section disk
/// `D = {(u,v) : (u−ci)² + (v−cj)² ≤ rc²}` against the rectangle
/// `R = [0,li]×[0,lj]`, and the axial span `[t_lo, t_hi]` against
/// the slab `[0, ln]` — the span covering the slab exactly when the
/// axis points along `+an` (`[dn, dn+hc]`) or `−an` (`[dn−hc, dn]`).
///
/// **The interior-cross-section contract** (the constructive
/// stations): `D` strictly inside `R`, so every annular face the
/// stations author is a rectangle with a strictly-interior circular
/// hole — a regular region. A disk that CROSSES `R` (the hole
/// breaking the box's side faces) is the trim-domain slice, refused
/// by name; a TANGENT disk (the hole touching a side face) would
/// pinch the annulus at a point that is no vertex of either wire —
/// refused by name, never a silently degenerate face. Common alone
/// is unaffected by a tangency (the clipped cylinder is the full-disk
/// column either way) and answers for the whole closed disk.
///
/// **The stations** (the overlap `[ta, tb] = [max(t_lo,0),
/// min(t_hi,ln)]` against the slab):
///
/// * **Through** (`t_lo ≤ 0 ∧ t_hi ≥ ln`): the cut is the box with
///   the cylindrical hole through it — four plain faces, two annuli
///   (the hole's openings), the REVERSED wall band `[0, ln]` (the
///   solid outside the cylinder — the nested-tube wall precedent),
///   the hole-adjusted Euler characteristic 0; the fuse is the box
///   with the axle — the sticking-out wall bands with their caps,
///   both crossed faces annular.
/// * **Blind** (one end inside the slab, one out): the cut is the
///   blind bore — one annulus (the entry face), the wall band to the
///   floor, the floor disk (outward INTO the hole); the fuse is the
///   boss — the wall band beyond the crossed face with its cap. A
///   flush far end (the cap coplanar with the entry plane) is the
///   SAME construction: the cap lies interior to the union, covered
///   by the box's own full face — the honest minimal shell, where
///   OCCT keeps the coplanar disk as its own redundant ninth face.
/// * **Common** is always the clipped cylinder `D × [ta, tb]` — a
///   plain cylinder piece (three faces).
///
/// **The classification tier** answers the whole shapes first:
/// engulf (the box inside the cylinder — every cross-section corner
/// within `rc` AND the span covering the slab: the cut empty, the
/// fuse the cylinder, the common the box; tangency included, the
/// touched generators measure-zero), lateral disjoint (the disk not
/// reaching the rectangle, external tangency included), axial
/// disjoint (the span missing the slab — a strictly separate
/// two-shell fuse refusal, and an exactly-flush LANDING refused by
/// name as the coplanar-merge boundary: a strict overhang composes,
/// a landing merges), contained (the cylinder inside the box: the
/// common the cylinder, the fuse the box, and a strictly-interior
/// cut the refused internal void — a flush containment is the
/// legitimate blind/through bore through the construction).
#[allow(clippy::too_many_arguments)] // the box and cylinder frames are the vocabulary
pub fn box_cylinder_boolean(
    op: BooleanOp,
    o: Vector3,
    a1: Vector3,
    a2: Vector3,
    a3: Vector3,
    l1: f64,
    l2: f64,
    l3: f64,
    oc: Vector3,
    ac: Vector3,
    rc: f64,
    hc: f64,
) -> Result<BooleanOutcome, String> {
    require_unit(a1, "box axis a1")?;
    require_unit(a2, "box axis a2")?;
    require_unit(a3, "box axis a3")?;
    if (a1.dot(&a2)).abs() > 1e-9 || (a1.dot(&a3)).abs() > 1e-9 || (a2.dot(&a3)).abs() > 1e-9 {
        return Err("the box axes must be mutually perpendicular".into());
    }
    if !(l1 > 0.0 && l2 > 0.0 && l3 > 0.0) {
        return Err("the box extents must be positive".into());
    }
    require_unit(ac, "cylinder axis")?;
    if rc <= 0.0 {
        return Err("the cylinder radius must be positive".into());
    }
    if hc <= 0.0 {
        return Err("the cylinder height must be positive".into());
    }
    let axes = [a1, a2, a3];
    let dims = [l1, l2, l3];
    // The frame contract: exactly one box axis claims the cylinder
    // with a bit-exact ±1 dot — the uniqueness is orthogonality's
    // own guarantee, asserted anyway.
    let mut frame: Option<(usize, f64)> = None;
    for (k, a) in axes.iter().enumerate() {
        let d = ac.dot(a);
        if d == 1.0 || d == -1.0 {
            if frame.is_some() {
                return Err(
                    "the cylinder's axis is parallel to two box axes at once — not a frame".into(),
                );
            }
            frame = Some((k, d));
        }
    }
    let Some((n, s)) = frame else {
        return Ok(refused(
            "the cylinder's axis is not bit-exactly parallel to a box axis — the rotated, \
             oblique, or near-parallel box×cylinder is outside this family (the section \
             circles would not lie in the box's face planes, and their extents would be a \
             snap's, not the input's); align the axis exactly, or tessellate for the faceted \
             workflow — this kernel never silently snaps",
        ));
    };
    let others: Vec<usize> = (0..3).filter(|&m| m != n).collect();
    let (i, j) = (others[0], others[1]);
    let off = v3(oc.x - o.x, oc.y - o.y, oc.z - o.z);
    let dn = off.dot(&axes[n]);
    let ci = off.dot(&axes[i]);
    let cj = off.dot(&axes[j]);
    let (li, lj, ln) = (dims[i], dims[j], dims[n]);
    // The cylinder's axial span in the box's n-coordinate.
    let (t_lo, t_hi) = if s > 0.0 {
        (dn, dn + hc)
    } else {
        (dn - hc, dn)
    };
    let solid = |brep: ExactBRep| BooleanOutcome {
        kind: BooleanOutcomeKind::Solid(brep),
    };
    // ---- engulf: the box inside the cylinder -------------------------
    // Convexity: all four cross-section corners within rc of the
    // axis point puts the rectangle inside the disk; the span
    // covering the slab closes the containment.
    let mut max_corner_d2 = 0.0f64;
    for (u, v) in [(0.0, 0.0), (li, 0.0), (0.0, lj), (li, lj)] {
        let du = u - ci;
        let dv = v - cj;
        max_corner_d2 = max_corner_d2.max(du * du + dv * dv);
    }
    if max_corner_d2 <= rc * rc && t_lo <= 0.0 && t_hi >= ln {
        return Ok(match op {
            BooleanOp::Cut => empty(),
            BooleanOp::Fuse => solid(cylinder_piece(oc, ac, rc, 0.0, hc)),
            BooleanOp::Common => solid(whole_box_brep(o, axes, dims)),
        });
    }
    // ---- lateral disjoint (external tangency included) ---------------
    let ex_i = if ci < 0.0 {
        -ci
    } else if ci > li {
        ci - li
    } else {
        0.0
    };
    let ex_j = if cj < 0.0 {
        -cj
    } else if cj > lj {
        cj - lj
    } else {
        0.0
    };
    if ex_i * ex_i + ex_j * ex_j >= rc * rc {
        return Ok(match op {
            BooleanOp::Cut => solid(whole_box_brep(o, axes, dims)),
            BooleanOp::Fuse => refused(
                "disjoint fuse — the result is a two-shell compound, not a single closed shell",
            ),
            BooleanOp::Common => empty(),
        });
    }
    // ---- axial disjoint, and the flush landing ------------------------
    if t_hi <= 0.0 || t_lo >= ln {
        let flush = t_hi == 0.0 || t_lo == ln;
        return Ok(match op {
            BooleanOp::Cut => solid(whole_box_brep(o, axes, dims)),
            BooleanOp::Common => empty(),
            BooleanOp::Fuse => {
                if flush {
                    refused(
                        "the flush landing — the cylinder's cap lies exactly on the box's face \
                         plane, and the landing union is the coplanar-merge family (a strict \
                         overhang composes exactly; a landing merges), refused by name — this \
                         kernel never silently merges",
                    )
                } else {
                    refused(
                        "disjoint fuse — the result is a two-shell compound, not a single \
                         closed shell",
                    )
                }
            }
        });
    }
    // ---- contained: the cylinder inside the box -----------------------
    let disk_within = ci - rc >= 0.0 && ci + rc <= li && cj - rc >= 0.0 && cj + rc <= lj;
    let disk_strict = ci - rc > 0.0 && ci + rc < li && cj - rc > 0.0 && cj + rc < lj;
    if disk_within && t_lo >= 0.0 && t_hi <= ln {
        return Ok(match op {
            BooleanOp::Common => solid(cylinder_piece(oc, ac, rc, 0.0, hc)),
            BooleanOp::Fuse => solid(whole_box_brep(o, axes, dims)),
            BooleanOp::Cut => {
                if t_lo > 0.0 && t_hi < ln {
                    refused(
                        "the internal void — the cylinder is strictly inside the box, and the \
                         void needs a second shell, not modeled in this family",
                    )
                } else {
                    // Flush containment: the bore is a legitimate
                    // blind or through hole — the construction's own
                    // path.
                    return box_cylinder_cut(o, axes, dims, oc, ac, rc, dn, s, n, t_lo, t_hi);
                }
            }
        });
    }
    // ---- the constructive tier -----------------------------------------
    let ta = t_lo.max(0.0);
    let tb = t_hi.min(ln);
    match op {
        BooleanOp::Common => {
            if !disk_within {
                return Ok(refused(
                    "the section disk crosses the box's cross-section — the side-clipped \
                     cylindrical section is the trim-domain slice, not constructed in this \
                     family",
                ));
            }
            // The clipped cylinder: the overlap column, a plain
            // piece in the cylinder's own v-frame.
            let v_a = (ta - dn) * s;
            let v_b = (tb - dn) * s;
            Ok(solid(cylinder_piece(
                oc,
                ac,
                rc,
                v_a.min(v_b),
                v_a.max(v_b),
            )))
        }
        BooleanOp::Cut => {
            if !disk_strict {
                return Ok(refused(if disk_within {
                    "the section disk is tangent to the box's cross-section — the annular face \
                     would pinch at a point that is no vertex of either wire, and the tangency \
                     is the boundary of the edge-crossing family (the trim-domain slice); this \
                     kernel never silently degenerates"
                } else {
                    "the section disk crosses the box's cross-section — the hole breaking the \
                     box's side faces is the trim-domain slice, not constructed in this family"
                }));
            }
            box_cylinder_cut(o, axes, dims, oc, ac, rc, dn, s, n, t_lo, t_hi)
        }
        BooleanOp::Fuse => {
            if !disk_strict {
                return Ok(refused(if disk_within {
                    "the section disk is tangent to the box's cross-section — the annular face \
                     would pinch at a point that is no vertex of either wire, and the tangency \
                     is the boundary of the edge-crossing family (the trim-domain slice); this \
                     kernel never silently degenerates"
                } else {
                    "the section disk crosses the box's cross-section — the column breaking \
                     the box's side faces is the trim-domain slice, not constructed in this \
                     family"
                }));
            }
            // The parts beyond the slab compose; a flush far end
            // contributes nothing (its cap lies interior, covered by
            // the box's own full face) — the input's own arithmetic,
            // not a merge.
            box_cylinder_fuse(o, axes, dims, oc, ac, rc, dn, s, n, t_lo, t_hi)
        }
    }
}

/// The box×cylinder cut at a constructive station: the overlap
/// `[ta, tb]` open at `ta == 0` (the low face), at `tb == ln` (the
/// high face), or both — the through hole or the blind bore. Exactly
/// TWO rim circles are authored (at `ta` and `tb`), each shared by
/// the wall band and its other consumer (the annulus of an opening,
/// the disk of a floor) — the closed-shell edge contract by
/// construction.
#[allow(clippy::too_many_arguments)] // the box and cylinder frames are the vocabulary
fn box_cylinder_cut(
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    oc: Vector3,
    ac: Vector3,
    rc: f64,
    dn: f64,
    s: f64,
    n: usize,
    t_lo: f64,
    t_hi: f64,
) -> Result<BooleanOutcome, String> {
    let ln = dims[n];
    let ta = t_lo.max(0.0);
    let tb = t_hi.min(ln);
    let e = any_perpendicular(ac);
    let axis_point = |t: f64| {
        v3(
            oc.x + (t - dn) * axes[n].x,
            oc.y + (t - dn) * axes[n].y,
            oc.z + (t - dn) * axes[n].z,
        )
    };
    let rim = |t: f64| Curve::Circle {
        center: axis_point(t),
        axis: ac,
        ref_dir: e,
        radius: rc,
    };
    let v_of = |t: f64| (t - dn) * s;
    let open_low = t_lo <= 0.0;
    let open_high = t_hi >= ln;
    let mut draft = Draft::new();
    // The two shared rim circles: at the wall's ends.
    let e_ta = draft.circle(rim(ta));
    let e_tb = draft.circle(rim(tb));
    // The box's faces: the ⊥n faces annular where the hole opens,
    // everything else plain.
    for k in 0..3 {
        for high in [false, true] {
            let holed = k == n && ((high && open_high) || (!high && open_low));
            if holed {
                let n_out = if high {
                    axes[n]
                } else {
                    v3(-axes[n].x, -axes[n].y, -axes[n].z)
                };
                let hole = if high { e_tb } else { e_ta };
                holed_rect_face(&mut draft, o, axes, dims, n, n_out, hole, ac);
                continue;
            }
            plain_box_face(&mut draft, o, axes, dims, k, high);
        }
    }
    // The wall band [ta, tb], REVERSED — the solid outside the
    // cylinder (the nested-tube wall precedent).
    let surface = Surface::Cylinder {
        origin: oc,
        axis: ac,
        ref_dir: e,
        radius: rc,
    };
    if s > 0.0 {
        wall_band(&mut draft, surface, e_ta, e_tb, v_of(ta), v_of(tb), true);
    } else {
        wall_band(&mut draft, surface, e_tb, e_ta, v_of(tb), v_of(ta), true);
    }
    // The blind bore's floor: the disk at the closed end, outward
    // INTO the hole (away from the material beyond it).
    if !(open_low && open_high) {
        let (t_floor, outward) = if open_low {
            (tb, v3(-axes[n].x, -axes[n].y, -axes[n].z))
        } else {
            (ta, axes[n])
        };
        let e_floor = if open_low { e_tb } else { e_ta };
        draft.faces.push(plane_disk_face(
            axis_point(t_floor),
            outward,
            e,
            e_floor,
            rc,
            outward.dot(&ac) > 0.0,
        ));
    }
    finish_single_shell(draft, "the box×cylinder cut")
}

/// The box×cylinder fuse at a constructive station: the cylinder's
/// parts strictly beyond the slab — `[t_lo, 0]` below, `[ln, t_hi]`
/// above — each a wall band with its far cap, and each strictly
/// straddling part's crossed face an annulus. A flush far end (the
/// cap coplanar with the face plane) contributes NOTHING: the cap
/// lies interior to the union, covered by the box's own full face.
#[allow(clippy::too_many_arguments)] // the box and cylinder frames are the vocabulary
fn box_cylinder_fuse(
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    oc: Vector3,
    ac: Vector3,
    rc: f64,
    dn: f64,
    s: f64,
    n: usize,
    t_lo: f64,
    t_hi: f64,
) -> Result<BooleanOutcome, String> {
    let ln = dims[n];
    let e = any_perpendicular(ac);
    let axis_point = |t: f64| {
        v3(
            oc.x + (t - dn) * axes[n].x,
            oc.y + (t - dn) * axes[n].y,
            oc.z + (t - dn) * axes[n].z,
        )
    };
    let rim = |t: f64| Curve::Circle {
        center: axis_point(t),
        axis: ac,
        ref_dir: e,
        radius: rc,
    };
    let v_of = |t: f64| (t - dn) * s;
    let below = t_lo < 0.0;
    let above = t_hi > ln;
    let mut draft = Draft::new();
    // The rim circles actually consumed: the crossed-face sections
    // and the far caps' rims — pushed only when their part exists.
    let e_low = if below {
        Some(draft.circle(rim(0.0)))
    } else {
        None
    };
    let e_high = if above {
        Some(draft.circle(rim(ln)))
    } else {
        None
    };
    let e_cap_lo = if below {
        Some(draft.circle(rim(t_lo)))
    } else {
        None
    };
    let e_cap_hi = if above {
        Some(draft.circle(rim(t_hi)))
    } else {
        None
    };
    // The box's faces: the crossed faces annular, the flush ones
    // plain and full.
    for k in 0..3 {
        for high in [false, true] {
            let holed = k == n && ((high && above) || (!high && below));
            if holed {
                let n_out = if high {
                    axes[n]
                } else {
                    v3(-axes[n].x, -axes[n].y, -axes[n].z)
                };
                let hole = if high { e_high } else { e_low };
                let hole = hole.expect("the holed face's part exists");
                holed_rect_face(&mut draft, o, axes, dims, n, n_out, hole, ac);
                continue;
            }
            plain_box_face(&mut draft, o, axes, dims, k, high);
        }
    }
    let surface = || Surface::Cylinder {
        origin: oc,
        axis: ac,
        ref_dir: e,
        radius: rc,
    };
    // The below part: the wall band [t_lo, 0] (natural — the solid
    // inside the column there) and the cap at t_lo, outward −an.
    if let (Some(e_low), Some(e_cap_lo)) = (e_low, e_cap_lo) {
        if s > 0.0 {
            wall_band(
                &mut draft,
                surface(),
                e_cap_lo,
                e_low,
                v_of(t_lo),
                v_of(0.0),
                false,
            );
        } else {
            wall_band(
                &mut draft,
                surface(),
                e_low,
                e_cap_lo,
                v_of(0.0),
                v_of(t_lo),
                false,
            );
        }
        let outward = v3(-axes[n].x, -axes[n].y, -axes[n].z);
        draft.faces.push(plane_disk_face(
            axis_point(t_lo),
            outward,
            e,
            e_cap_lo,
            rc,
            outward.dot(&ac) > 0.0,
        ));
    }
    // The above part: the wall band [ln, t_hi] and the cap at t_hi,
    // outward +an.
    if let (Some(e_high), Some(e_cap_hi)) = (e_high, e_cap_hi) {
        if s > 0.0 {
            wall_band(
                &mut draft,
                surface(),
                e_high,
                e_cap_hi,
                v_of(ln),
                v_of(t_hi),
                false,
            );
        } else {
            wall_band(
                &mut draft,
                surface(),
                e_cap_hi,
                e_high,
                v_of(t_hi),
                v_of(ln),
                false,
            );
        }
        draft.faces.push(plane_disk_face(
            axis_point(t_hi),
            axes[n],
            e,
            e_cap_hi,
            rc,
            axes[n].dot(&ac) > 0.0,
        ));
    }
    finish_single_shell(draft, "the box×cylinder fuse")
}

/// One plain box face (the whole-box pattern): the four canonical
/// corners CCW against the outward normal, the frame right-handed
/// against it.
fn plain_box_face(
    draft: &mut Draft,
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    k: usize,
    high: bool,
) {
    let n_out = if high {
        axes[k]
    } else {
        v3(-axes[k].x, -axes[k].y, -axes[k].z)
    };
    let span: Vec<usize> = (0..3).filter(|&m| m != k).collect();
    let (ia, ib) = if axes[span[0]].cross(&axes[span[1]]).dot(&n_out) > 0.0 {
        (span[0], span[1])
    } else {
        (span[1], span[0])
    };
    let s_of = |a: u8, b: u8| -> [u8; 3] {
        let mut s = [0u8, 0, 0];
        s[k] = high as u8;
        s[ia] = a;
        s[ib] = b;
        s
    };
    rect_face(
        draft,
        box_corner(o, axes, dims, s_of(0, 0)),
        box_corner(o, axes, dims, s_of(1, 0)),
        box_corner(o, axes, dims, s_of(1, 1)),
        box_corner(o, axes, dims, s_of(0, 1)),
        n_out,
        axes[ia],
        axes[ib],
        dims[ia],
        dims[ib],
    );
}

/// One annular box face: the ⊥n face's rectangle with the
/// cylinder's rim circle as a strictly-interior hole. The outer wire
/// is the canonical rectangle CCW against the outward normal; the
/// hole runs CW against it — forward exactly when the circle's own
/// axis points against the outward normal (the region-on-left rule
/// for holes, the offset-station pattern).
#[allow(clippy::too_many_arguments)] // the box frame + the hole's own partners
fn holed_rect_face(
    draft: &mut Draft,
    o: Vector3,
    axes: [Vector3; 3],
    dims: [f64; 3],
    n: usize,
    n_out: Vector3,
    hole: usize,
    ac: Vector3,
) {
    let span: Vec<usize> = (0..3).filter(|&m| m != n).collect();
    let (ia, ib) = if axes[span[0]].cross(&axes[span[1]]).dot(&n_out) > 0.0 {
        (span[0], span[1])
    } else {
        (span[1], span[0])
    };
    let high = n_out == axes[n];
    let s_of = |a: u8, b: u8| -> [u8; 3] {
        let mut s = [0u8, 0, 0];
        s[n] = high as u8;
        s[ia] = a;
        s[ib] = b;
        s
    };
    let c00 = box_corner(o, axes, dims, s_of(0, 0));
    let c10 = box_corner(o, axes, dims, s_of(1, 0));
    let c11 = box_corner(o, axes, dims, s_of(1, 1));
    let c01 = box_corner(o, axes, dims, s_of(0, 1));
    let outer = [
        draft.seam(c00, c10),
        draft.seam(c10, c11),
        draft.seam(c11, c01),
        draft.seam(c01, c00),
    ];
    draft.faces.push(TopoFace {
        surface: Surface::Plane {
            origin: c00,
            normal: n_out,
            x_dir: axes[ia],
        },
        uv_bounds: [0.0, dims[ia], 0.0, dims[ib]],
        wires: vec![
            TopoWire {
                edges: outer
                    .iter()
                    .map(|&(ed, f)| WireEdge {
                        edge: ed,
                        forward: f,
                    })
                    .collect(),
            },
            TopoWire {
                edges: vec![WireEdge {
                    edge: hole,
                    forward: ac.dot(&n_out) < 0.0,
                }],
            },
        ],
        reversed: false,
    });
}

/// One cylindrical wall band between two already-authored rim
/// circles (`e_lo` at the smaller cylinder v, `e_hi` at the larger):
/// the seam between their seam points and the strip face — the walk
/// [seam up, upper back, seam down, lower forward] against the
/// natural normal, `reversed` carrying which side the solid lies on.
fn wall_band(
    draft: &mut Draft,
    surface: Surface,
    e_lo: usize,
    e_hi: usize,
    v_lo: f64,
    v_hi: f64,
    reversed: bool,
) {
    let p_lo = draft.edges[e_lo].curve.point(0.0);
    let p_hi = draft.edges[e_hi].curve.point(0.0);
    let (seam, _) = draft.seam(p_lo, p_hi);
    draft.faces.push(cylinder_strip_face(
        surface, e_lo, e_hi, seam, v_lo, v_hi, reversed,
    ));
}
