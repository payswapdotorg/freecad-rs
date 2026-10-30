//! K4-2d — the NURBS measure acceptance: Gauss–Legendre on polynomial
//! integrands, exact, never an approximation.
//!
//! The claim under test: a closed shell whose boundary carries
//! non-rational B-spline faces measures **exactly** — the volume flux
//! `p·(∂u×∂v)` of a polynomial patch is a piecewise polynomial, and a
//! Gauss–Legendre rule with enough nodes per knot span integrates a
//! polynomial of degree `2n−1` with **zero quadrature error** (the
//! only residual is evaluation rounding, machine scale — the same
//! scale the closed forms themselves carry). The trims those faces
//! force on their plane neighbors — B-spline boundary curves — read
//! through the exact Green line integral `∮ (x dy − y dx)`, itself a
//! piecewise polynomial per span, itself GL-exact.
//!
//! Every fixture below is a hand-built "tent": the graph patch
//! `z = c·f(u)·g(v)` over a rectangle footprint, clamped, non-rational,
//! with its four boundary pole rows as the shared shell edges (the
//! representation real OCCT payloads carry — the K4-1 corpus pattern).
//! The closed forms are hand-derived in each test from the
//! spline-moment integrals of the chosen pole rows; the acceptance
//! pins the measured volume to machine scale, not to a tolerance a
//! tolerant quadrature could squeak through.
//!
//! Honest refusals, each named by a test: a **rational** B-spline
//! face (the flux is a ratio of polynomials — GL is exact only on
//! polynomial integrands), a B-spline face whose wire is **not its
//! natural boundary** (only the full-domain patch reads), and the
//! **area** of a B-spline face (the integrand `|∂u×∂v|` lives under a
//! square root — not polynomial, no exact rule, refused by name).

use fc_base::Vector3;
use fc_part::brep::{read_brep_bytes, write_brep};
use fc_part::measure::{brep_area, brep_volume};
use fc_part::topology::{
    Curve, ExactBRep, Surface, TopoEdge, TopoFace, TopoVertex, TopoWire, WireEdge,
};

fn v(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// Add (or find) a vertex by bit-exact point equality.
fn vertex(brep: &mut ExactBRep, p: Vector3) -> usize {
    if let Some(i) = brep.vertices.iter().position(|q| q.point == p) {
        return i;
    }
    brep.vertices.push(TopoVertex { point: p });
    brep.vertices.len() - 1
}

/// A non-rational B-spline edge over its full knot domain, endpoints
/// pinned to shared vertices (clamped ends evaluate to their end
/// poles bit-exactly — the K1 reconciliation contract).
fn spline_edge(brep: &mut ExactBRep, degree: usize, knots: &[f64], pts: &[Vector3]) -> usize {
    let curve = Curve::Bspline {
        degree,
        knots: knots.to_vec(),
        points: pts.to_vec(),
        weights: None,
    };
    let lo = knots[degree];
    let hi = knots[pts.len()];
    let s = vertex(brep, curve.point(lo));
    let e = vertex(brep, curve.point(hi));
    brep.edges.push(TopoEdge {
        curve,
        range: (lo, hi),
        start: s,
        end: e,
    });
    brep.edges.len() - 1
}

fn line_edge(brep: &mut ExactBRep, a: Vector3, b: Vector3) -> usize {
    let curve = Curve::Line {
        origin: a,
        direction: b - a,
    };
    let s = vertex(brep, curve.point(0.0));
    let e = vertex(brep, curve.point(1.0));
    brep.edges.push(TopoEdge {
        curve,
        range: (0.0, 1.0),
        start: s,
        end: e,
    });
    brep.edges.len() - 1
}

fn wire(edges: &[(usize, bool)]) -> TopoWire {
    TopoWire {
        edges: edges
            .iter()
            .map(|&(e, f)| WireEdge {
                edge: e,
                forward: f,
            })
            .collect(),
    }
}

/// The hand-built "tent" shell: the graph patch
/// `z = zc·f(u)·g(v)` over the footprint `[0, x_poles.last()] ×
/// [0, y_poles.last()]`, with `f`/`g` given by their 1D pole rows
/// (`fu` over `u_knots`, `gv` over `v_knots` — the tensor net is
/// `points[i][j] = (x_i, y_j, zc·fu[i]·gv[j])`, so `x(u)` and `y(v)`
/// carry the linear-precision pole rows and `z` factors exactly).
///
/// The shell: the patch face (natural-domain wire — its four boundary
/// pole rows as the edges, the K4-1 corpus representation), the
/// bottom plane, and — when `f` does not close at the far end — the
/// vertical side wall, a plane face genuinely trimmed by the curved
/// B-spline boundary column (the Green-integral acceptance).
///
/// Boundary rows must close to `z = 0` at the footprint rim (the
/// poles' boundary values are multiplied by `gv`'s zero end poles),
/// so the solid is exactly the region under the graph and
/// `V = ∫∫ z dA` in closed form.
#[allow(clippy::too_many_arguments)]
fn tent(
    u_knots: &[f64],
    x_poles: &[f64],
    fu: &[f64],
    v_knots: &[f64],
    y_poles: &[f64],
    gv: &[f64],
    zc: f64,
) -> ExactBRep {
    let du = match u_knots.len().checked_sub(x_poles.len() + 1) {
        Some(d) => d,
        None => panic!("u knot count mismatch"),
    };
    let dv = match v_knots.len().checked_sub(y_poles.len() + 1) {
        Some(d) => d,
        None => panic!("v knot count mismatch"),
    };
    // The tensor net.
    let net: Vec<Vec<Vector3>> = (0..x_poles.len())
        .map(|i| {
            (0..y_poles.len())
                .map(|j| v(x_poles[i], y_poles[j], zc * fu[i] * gv[j]))
                .collect()
        })
        .collect();
    let (ulo, uhi) = (u_knots[du], u_knots[x_poles.len()]);
    let (vlo, vhi) = (v_knots[dv], v_knots[y_poles.len()]);
    let mut brep = ExactBRep::default();

    // The four boundary rows as full-domain B-spline edges.
    let v0_row: Vec<Vector3> = (0..x_poles.len()).map(|i| net[i][0]).collect();
    let v1_row: Vec<Vector3> = (0..x_poles.len())
        .map(|i| net[i][y_poles.len() - 1])
        .collect();
    let u0_col: Vec<Vector3> = net[0].clone();
    let u1_col: Vec<Vector3> = net[x_poles.len() - 1].clone();
    let e_v0 = spline_edge(&mut brep, du, u_knots, &v0_row);
    let e_v1 = spline_edge(&mut brep, du, u_knots, &v1_row);
    let e_u0 = spline_edge(&mut brep, dv, v_knots, &u0_col);
    let e_u1 = spline_edge(&mut brep, dv, v_knots, &u1_col);

    // Does the far column rise off the bottom plane? Then the shell
    // needs the side wall (and its bottom line edge).
    let wall_needed = fu[x_poles.len() - 1] != 0.0;
    let c00 = net[0][0];
    let c10 = net[x_poles.len() - 1][0];
    let c11 = net[x_poles.len() - 1][y_poles.len() - 1];
    let e_wall = if wall_needed {
        Some(line_edge(&mut brep, c10, c11))
    } else {
        None
    };

    // The patch face: the natural-domain wire, CCW in (u, v).
    let dome = TopoFace {
        surface: Surface::Bspline {
            u_degree: du,
            v_degree: dv,
            u_knots: u_knots.to_vec(),
            v_knots: v_knots.to_vec(),
            points: net,
            weights: None,
        },
        uv_bounds: [ulo, uhi, vlo, vhi],
        wires: vec![wire(&[
            (e_v0, true),
            (e_u1, true),
            (e_v1, false),
            (e_u0, false),
        ])],
        reversed: false,
    };
    // The bottom: outward −Z, wire CCW in its own (x_dir, y_dir)
    // frame (the K1 convention).
    let bottom = TopoFace {
        surface: Surface::Plane {
            origin: c00,
            normal: v(0.0, 0.0, -1.0),
            x_dir: v(1.0, 0.0, 0.0),
        },
        uv_bounds: [
            0.0,
            x_poles[x_poles.len() - 1],
            0.0,
            y_poles[y_poles.len() - 1],
        ],
        wires: vec![wire(&match e_wall {
            Some(w) => vec![(e_u0, true), (e_v1, true), (w, false), (e_v0, false)],
            None => vec![(e_u0, true), (e_v1, true), (e_u1, false), (e_v0, false)],
        })],
        reversed: false,
    };
    let mut faces = vec![bottom, dome];
    if let Some(w) = e_wall {
        // The side wall at the far column: outward +X, trimmed by the
        // curved B-spline column and the bottom line.
        let z_peak = u1_col.iter().map(|p| p.z.abs()).fold(0.0_f64, f64::max);
        faces.push(TopoFace {
            surface: Surface::Plane {
                origin: c10,
                normal: v(1.0, 0.0, 0.0),
                x_dir: v(0.0, 1.0, 0.0),
            },
            uv_bounds: [0.0, y_poles[y_poles.len() - 1], 0.0, z_peak],
            wires: vec![wire(&[(w, true), (e_u1, false)])],
            reversed: false,
        });
    }
    brep.faces = faces;
    brep
}

/// L=3, W=2 in every fixture; every closed form is stated per test.
const L: f64 = 3.0;
const W: f64 = 2.0;

fn assert_shell(brep: &ExactBRep) {
    brep.validate().unwrap_or_else(|e| panic!("validate: {e}"));
    assert_eq!(brep.euler_characteristic(), 2);
}

// ---------------------------------------------------------------------------
// the exact volume — one closed form per degree, machine scale
// ---------------------------------------------------------------------------

#[test]
fn tent_quadratic_single_span_measures_exactly() {
    // z = 9·2u(1−u)·2v(1−v) = 36u(1−u)v(1−v) on [0,1]²;
    // V = 36·(1/6)²·L·W = L·W = 6.
    let brep = tent(
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, L / 2.0, L],
        &[0.0, 1.0, 0.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, W / 2.0, W],
        &[0.0, 1.0, 0.0],
        9.0,
    );
    assert_shell(&brep);
    let vol = brep_volume(&brep).unwrap_or_else(|e| panic!("volume refused: {e}"));
    assert!(
        (vol - L * W).abs() <= 1e-12,
        "V = {vol:.17} vs the closed form {}",
        L * W
    );
    // The FCBREP v2 round-trip: the shell re-reads bit-identical and
    // measures bit-identically.
    let mut out = Vec::new();
    write_brep(&mut out, &brep).unwrap();
    let back = read_brep_bytes(&out).unwrap();
    assert_eq!(&back, &brep);
    let vol2 = brep_volume(&back).unwrap();
    assert_eq!(vol, vol2);
}

#[test]
fn tent_quadratic_two_u_spans_measures_exactly() {
    // u-knots [0,0,0,1,2,2,2]: f(u) = u(2−u) (poles [0,1,1,0]),
    // g(v) = v(1−v) (poles [0,1/2,0]), z = 9·f·g;
    // V = (L/2)·W·9·(4/3)·(1/6) = L·W = 6 — through two u-spans,
    // so the per-span decomposition is genuinely exercised.
    let brep = tent(
        &[0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        &[0.0, L / 4.0, 3.0 * L / 4.0, L],
        &[0.0, 1.0, 1.0, 0.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, W / 2.0, W],
        &[0.0, 0.5, 0.0],
        9.0,
    );
    assert_shell(&brep);
    let vol = brep_volume(&brep).unwrap_or_else(|e| panic!("volume refused: {e}"));
    assert!(
        (vol - L * W).abs() <= 1e-12,
        "V = {vol:.17} vs the closed form {}",
        L * W
    );
}

#[test]
fn tent_cubic_measures_exactly() {
    // Degree (3,3), single span: f = g = u(1−u) (cubic poles
    // [0,1/3,1/3,0]), z = 36·f·g; V = 36·(1/6)²·L·W = 6.
    // The flux integrand is degree (8,8) — the 5-node rule.
    let brep = tent(
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, L / 3.0, 2.0 * L / 3.0, L],
        &[0.0, 1.0 / 3.0, 1.0 / 3.0, 0.0],
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, W / 3.0, 2.0 * W / 3.0, W],
        &[0.0, 1.0 / 3.0, 1.0 / 3.0, 0.0],
        36.0,
    );
    assert_shell(&brep);
    let vol = brep_volume(&brep).unwrap_or_else(|e| panic!("volume refused: {e}"));
    assert!(
        (vol - L * W).abs() <= 1e-12,
        "V = {vol:.17} vs the closed form {}",
        L * W
    );
}

#[test]
fn tent_quartic_measures_exactly() {
    // Degree (4,4): f = u(1−u) as the quartic poles
    // [0,1/4,1/3,1/4,0] (degree elevation of the cubic, verified by
    // the test's closed form); z = 36·f·g; V = 6. Integrand degree
    // (11,11) — the 6-node rule.
    let brep = tent(
        &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, L / 4.0, L / 2.0, 3.0 * L / 4.0, L],
        &[0.0, 0.25, 1.0 / 3.0, 0.25, 0.0],
        &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, W / 4.0, W / 2.0, 3.0 * W / 4.0, W],
        &[0.0, 0.25, 1.0 / 3.0, 0.25, 0.0],
        36.0,
    );
    assert_shell(&brep);
    let vol = brep_volume(&brep).unwrap_or_else(|e| panic!("volume refused: {e}"));
    assert!(
        (vol - L * W).abs() <= 1e-11,
        "V = {vol:.17} vs the closed form {}",
        L * W
    );
}

#[test]
fn tent_quintic_measures_exactly() {
    // Degree (5,5): f = u(1−u) as the quintic poles
    // [0,1/5,3/10,3/10,1/5,0]; z = 36·f·g; V = 6. Integrand degree
    // (14,14) — the 8-node rule, the table's end.
    let brep = tent(
        &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, L / 5.0, 2.0 * L / 5.0, 3.0 * L / 5.0, 4.0 * L / 5.0, L],
        &[0.0, 0.2, 0.3, 0.3, 0.2, 0.0],
        &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        &[0.0, W / 5.0, 2.0 * W / 5.0, 3.0 * W / 5.0, 4.0 * W / 5.0, W],
        &[0.0, 0.2, 0.3, 0.3, 0.2, 0.0],
        36.0,
    );
    assert_shell(&brep);
    let vol = brep_volume(&brep).unwrap_or_else(|e| panic!("volume refused: {e}"));
    assert!(
        (vol - L * W).abs() <= 1e-11,
        "V = {vol:.17} vs the closed form {}",
        L * W
    );
}

#[test]
fn ruled_tent_mixed_degrees_and_curved_plane_trim_measure_exactly() {
    // Degree (1,2): z = 12·u·v(1−v) — the far column rises off the
    // bottom plane, so the shell carries the side wall: a plane face
    // genuinely trimmed by a curved B-spline boundary column (the
    // Green line integral on a real curved trim) plus a plain line.
    // V = 12·(1/2)·(1/6)·L·W = L·W = 6.
    let brep = tent(
        &[0.0, 0.0, 1.0, 1.0],
        &[0.0, L],
        &[0.0, 1.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, W / 2.0, W],
        &[0.0, 0.5, 0.0],
        12.0,
    );
    assert_shell(&brep);
    assert_eq!(brep.faces.len(), 3, "bottom + wall + dome");
    let vol = brep_volume(&brep).unwrap_or_else(|e| panic!("volume refused: {e}"));
    assert!(
        (vol - L * W).abs() <= 1e-12,
        "V = {vol:.17} vs the closed form {}",
        L * W
    );
    // The curved wall's Green integral is pinned through the flux:
    // the wall's own flux is σ·L·A_wall, and the divergence sum hits
    // the closed form only if A_wall = 12·∫v(1−v)dv·W-integral is
    // exact (a wrong wall area would miss V by L·ΔA/3). The dome's
    // own AREA refuses honestly — the dedicated refusal test pins
    // that.
}

#[test]
fn reversed_dome_negates_the_volume_bit_exactly() {
    let mut brep = tent(
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, L / 2.0, L],
        &[0.0, 1.0, 0.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, W / 2.0, W],
        &[0.0, 1.0, 0.0],
        9.0,
    );
    let v_up = brep_volume(&brep).unwrap();
    // An inside-out dome: the σ flag flips the face's flux sign —
    // the negation is exact in floating point.
    brep.faces[1].reversed = true;
    let v_down = brep_volume(&brep).unwrap();
    assert_eq!(v_down, -v_up);
    assert!((v_down + L * W).abs() <= 1e-12);
}

// ---------------------------------------------------------------------------
// honest refusals, by name
// ---------------------------------------------------------------------------

#[test]
fn rational_bspline_face_refuses_by_name() {
    let mut brep = tent(
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, L / 2.0, L],
        &[0.0, 1.0, 0.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, W / 2.0, W],
        &[0.0, 1.0, 0.0],
        9.0,
    );
    if let Surface::Bspline {
        points, weights, ..
    } = &mut brep.faces[1].surface
    {
        // A genuinely rational net: unequal weights.
        *weights = Some(
            (0..points.len())
                .map(|i| {
                    (0..points[0].len())
                        .map(|j| if i == 1 && j == 1 { 0.8 } else { 1.0 })
                        .collect()
                })
                .collect(),
        );
    }
    assert_shell(&brep);
    let err = brep_volume(&brep).unwrap_err();
    assert!(
        err.contains("rational"),
        "the refusal names the rational surface: {err}"
    );
}

#[test]
fn non_natural_wire_refuses_by_name() {
    let mut brep = tent(
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, L / 2.0, L],
        &[0.0, 1.0, 0.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, W / 2.0, W],
        &[0.0, 1.0, 0.0],
        9.0,
    );
    // Swap one boundary edge for a curve that is NOT the surface's
    // own boundary row (same endpoints — the shell stays valid and
    // closed — but a different middle pole, still in the bottom
    // plane so the neighbor keeps reading it).
    let idx = brep.faces[0].wires[0].edges[1].edge;
    if let Curve::Bspline { points, .. } = &mut brep.edges[idx].curve {
        points[1] = v(L / 2.0, W + 0.2, 0.0);
    }
    assert_shell(&brep);
    let err = brep_volume(&brep).unwrap_err();
    assert!(
        err.contains("natural boundary"),
        "the refusal names the trim: {err}"
    );
}

#[test]
fn bspline_face_area_refuses_by_name() {
    let brep = tent(
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, L / 2.0, L],
        &[0.0, 1.0, 0.0],
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[0.0, W / 2.0, W],
        &[0.0, 1.0, 0.0],
        9.0,
    );
    assert_shell(&brep);
    // The volume is exact; the area refuses: |∂u×∂v| lives under a
    // square root — not a polynomial integrand.
    let vol = brep_volume(&brep).unwrap();
    assert!((vol - L * W).abs() <= 1e-12);
    let err = brep_area(&brep).unwrap_err();
    assert!(
        err.contains("not polynomial") || err.contains("no exact"),
        "the refusal names the integrand: {err}"
    );
}
