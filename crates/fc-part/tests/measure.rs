//! K3c-2 acceptance — exact measure on composed B-reps (`measure.rs`),
//! the sphere×halfspace plane cuts (`brep_boolean.rs`), and the
//! face-orientation model both forced.
//!
//! Written BEFORE the implementation (the acceptance-first
//! discipline). The contract:
//!
//! * `brep_volume` / `brep_area` compute the divergence-theorem
//!   volume and the total boundary area of a closed `ExactBRep` in
//!   closed form, per face kind — no tessellation, no quadrature.
//! * Every K1 primitive's `ExactBRep::from_shape` measures exactly
//!   what `TopoShape::volume()` / `surface_area()` report: two
//!   independent computations agreeing.
//! * The K3c-1 outcomes measure their classical volumes: the lens is
//!   the two caps, the crescent is `V(a) − V(lens)`, the peanut is
//!   `V(a) + V(b) − V(lens)`. **The last two cannot both hold unless
//!   the crescent and the peanut are different B-reps** — this is
//!   the acceptance test that forced the face-orientation
//!   (`reversed`) field: before it, Cut and Fuse of a secant pair
//!   produced byte-identical face lists, so no consumer could tell
//!   a removed cap from a kept one.
//! * `sphere_plane_boolean` cuts a sphere with the halfspace tool
//!   `{x : n·(x − p) ≥ 0}`: Common keeps the +normal cap (a
//!   spherical patch + a planar disk sharing the K3a section
//!   circle), Cut keeps the −normal cap. Volumes and areas verify
//!   through the classical cap formulas; non-secant configurations
//!   classify exactly (expansion-arithmetic signs, no tolerances)
//!   and Fuse is refused by name — an unbounded solid.
//! * Measure refuses what it cannot close, by name, never by
//!   approximation: B-spline faces, untrimmed planes, cylinder faces
//!   with rims it cannot read.
//! * FCBREP v3 round-trips the orientation flag byte-identically;
//!   v2 payloads still read (faces default unreversed).

use fc_base::Vector3;
use fc_part::brep::{read_brep_bytes, write_brep};
use fc_part::brep_boolean::{
    sphere_plane_boolean, sphere_sphere_boolean, BooleanOp, BooleanOutcome, BooleanOutcomeKind,
};
use fc_part::measure::{brep_area, brep_volume};
use fc_part::shape::TopoShape;
use fc_part::topology::{
    Curve, ExactBRep, Surface, TopoEdge, TopoFace, TopoVertex, TopoWire, WireEdge,
};

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

fn approx(a: f64, b: f64, what: &str) {
    let scale = a.abs().max(b.abs()).max(1.0);
    assert!(
        (a - b).abs() <= 1e-12 * scale,
        "{what}: {a} vs {b} (diff {})",
        (a - b).abs()
    );
}

fn expect_solid<'a>(o: &'a BooleanOutcome, what: &str) -> &'a ExactBRep {
    match &o.kind {
        BooleanOutcomeKind::Solid(b) => b,
        other => panic!("{what}: expected a solid, got {other:?}"),
    }
}

/// The classical spherical-cap volume for height `h` on radius `r`.
fn cap_volume(r: f64, h: f64) -> f64 {
    std::f64::consts::PI * h * h * (3.0 * r - h) / 3.0
}

// ---------------------------------------------------------------------------
// primitives — two independent computations agreeing
// ---------------------------------------------------------------------------

#[test]
fn primitive_breps_measure_what_the_shape_module_reports() {
    let primitives: Vec<(&str, TopoShape)> = vec![
        (
            "box",
            TopoShape::Box {
                length: 3.0,
                width: 2.0,
                height: 5.0,
            },
        ),
        ("sphere", TopoShape::Sphere { radius: 2.5 }),
        (
            "cylinder",
            TopoShape::Cylinder {
                radius: 2.0,
                height: 7.0,
            },
        ),
        (
            "plain cone",
            TopoShape::Cone {
                radius1: 3.0,
                radius2: 0.0,
                height: 4.0,
            },
        ),
        (
            "frustum (widening)",
            TopoShape::Cone {
                radius1: 1.5,
                radius2: 3.0,
                height: 4.0,
            },
        ),
        // Contracting frustum: negative semi-angle — the signed-slope
        // case the K3b differential caught being wrong elsewhere.
        (
            "contracting frustum",
            TopoShape::Cone {
                radius1: 3.0,
                radius2: 1.5,
                height: 4.0,
            },
        ),
        (
            "torus",
            TopoShape::Torus {
                radius1: 3.0,
                radius2: 1.0,
            },
        ),
    ];
    for (name, shape) in &primitives {
        let brep = ExactBRep::from_shape(shape).expect("primitive topology");
        let v = brep_volume(&brep).unwrap_or_else(|e| panic!("{name}: {e}"));
        let a = brep_area(&brep).unwrap_or_else(|e| panic!("{name}: {e}"));
        approx(v, shape.volume(), &format!("{name} volume vs TopoShape"));
        approx(
            a,
            shape.surface_area(),
            &format!("{name} area vs TopoShape"),
        );
        assert!(
            v > 0.0,
            "{name}: a correctly oriented shell measures positive"
        );
    }
}

#[test]
fn primitive_volumes_match_the_classical_closed_forms() {
    let pi = std::f64::consts::PI;
    let cases: Vec<(&str, TopoShape, f64)> = vec![
        (
            "box",
            TopoShape::Box {
                length: 3.0,
                width: 2.0,
                height: 5.0,
            },
            3.0 * 2.0 * 5.0,
        ),
        (
            "sphere",
            TopoShape::Sphere { radius: 2.5 },
            4.0 / 3.0 * pi * 2.5_f64.powi(3),
        ),
        (
            "cylinder",
            TopoShape::Cylinder {
                radius: 2.0,
                height: 7.0,
            },
            pi * 4.0 * 7.0,
        ),
        (
            "plain cone",
            TopoShape::Cone {
                radius1: 3.0,
                radius2: 0.0,
                height: 4.0,
            },
            pi * 4.0 * 9.0 / 3.0,
        ),
        (
            "frustum",
            TopoShape::Cone {
                radius1: 3.0,
                radius2: 1.5,
                height: 4.0,
            },
            pi * 4.0 * (9.0 + 4.5 + 2.25) / 3.0,
        ),
        (
            "torus",
            TopoShape::Torus {
                radius1: 3.0,
                radius2: 1.0,
            },
            2.0 * pi * pi * 3.0 * 1.0,
        ),
    ];
    for (name, shape, want) in &cases {
        let brep = ExactBRep::from_shape(shape).expect("primitive topology");
        let v = brep_volume(&brep).unwrap_or_else(|e| panic!("{name}: {e}"));
        approx(v, *want, &format!("{name} closed form"));
    }
}

// ---------------------------------------------------------------------------
// the K3c-1 outcomes — and the orientation defect the volumes expose
// ---------------------------------------------------------------------------

/// A general-position secant pair used across the tests.
fn pair() -> (Vector3, f64, Vector3, f64) {
    (v3(-2.0, 0.5, 1.0), 3.0, v3(2.0, 1.5, -0.5), 4.0)
}

/// The three outcomes' volumes against the classical cap
/// composition — recomputed from the geometry the boolean returned
/// (the radical-plane distance read off the shared circle).
#[test]
fn sphere_sphere_outcomes_measure_their_cap_compositions() {
    let (ca, ra, cb, rb) = pair();
    let pi = std::f64::consts::PI;

    let common = sphere_sphere_boolean(BooleanOp::Common, ca, ra, cb, rb).unwrap();
    let cut = sphere_sphere_boolean(BooleanOp::Cut, ca, ra, cb, rb).unwrap();
    let fuse = sphere_sphere_boolean(BooleanOp::Fuse, ca, ra, cb, rb).unwrap();

    // The radical-plane offset from A's center, from the returned
    // circle (the boolean's own geometry, not the test's).
    let lens = expect_solid(&common, "common");
    let (cc, _rho) = match &lens.edges[0].curve {
        Curve::Circle { center, radius, .. } => (*center, *radius),
        other => panic!("{other:?}"),
    };
    let d1 = v3(cc.x - ca.x, cc.y - ca.y, cc.z - ca.z).length();
    let d = v3(cb.x - ca.x, cb.y - ca.y, cb.z - ca.z).length();
    let h_a = ra - d1;
    let h_b = rb - (d - d1);
    let v_lens = cap_volume(ra, h_a) + cap_volume(rb, h_b);
    let v_a = 4.0 / 3.0 * pi * ra * ra * ra;
    let v_b = 4.0 / 3.0 * pi * rb * rb * rb;

    let v_common = brep_volume(lens).unwrap_or_else(|e| panic!("lens: {e}"));
    let v_cut = brep_volume(expect_solid(&cut, "cut")).unwrap_or_else(|e| panic!("cut: {e}"));
    let v_fuse = brep_volume(expect_solid(&fuse, "fuse")).unwrap_or_else(|e| panic!("fuse: {e}"));
    approx(v_common, v_lens, "V(lens) = the two caps");

    // The partition identities — and the direct proof that the
    // crescent and the peanut are different B-reps: their volumes
    // differ by exactly V(b) (before the orientation fix they were
    // byte-identical and this could not hold).
    approx(v_common + v_cut, v_a, "V(lens) + V(crescent) = V(a)");
    approx(
        v_common + v_fuse,
        v_a + v_b,
        "V(lens) + V(peanut) = V(a) + V(b)",
    );
    approx(v_fuse - v_cut, v_b, "V(peanut) − V(crescent) = V(b)");

    // Areas: the lens is two spherical patches (no disk); the
    // crescent and peanut reuse the same patches.
    let a_lens = brep_area(lens).unwrap_or_else(|e| panic!("lens area: {e}"));
    let want_lens_area = 2.0 * pi * ra * h_a + 2.0 * pi * rb * h_b;
    approx(a_lens, want_lens_area, "A(lens) = the two caps' patches");
}

// ---------------------------------------------------------------------------
// sphere × halfspace — the plane cuts
// ---------------------------------------------------------------------------

fn plane_pair() -> (Vector3, f64, Vector3, Vector3) {
    // Sphere at a general position; an oblique unit normal.
    let c = v3(1.0, -2.0, 0.5);
    let r = 3.0;
    let p = v3(2.0, 0.0, -1.0);
    let n = v3(1.0 / 3.0, 2.0 / 3.0, -2.0 / 3.0);
    (c, r, p, n)
}

#[test]
fn sphere_plane_common_and_cut_partition_the_sphere() {
    let (c, r, p, n) = plane_pair();
    let pi = std::f64::consts::PI;

    // The plane's signed offset of the center: s = (c − p)·n. The
    // +n cap's height is r + s (for s < 0, the small cap).
    let s = v3(c.x - p.x, c.y - p.y, c.z - p.z).dot(&n);
    let h_plus = r + s; // the +n cap's height
    let rho2 = r * r - s * s;

    let common = sphere_plane_boolean(BooleanOp::Common, c, r, p, n).expect("common");
    let cut = sphere_plane_boolean(BooleanOp::Cut, c, r, p, n).expect("cut");

    let v_common = brep_volume(expect_solid(&common, "common")).unwrap_or_else(|e| panic!("{e}"));
    let v_cut = brep_volume(expect_solid(&cut, "cut")).unwrap_or_else(|e| panic!("{e}"));
    approx(v_common, cap_volume(r, h_plus), "V(common) = the +n cap");
    approx(
        v_cut,
        4.0 / 3.0 * pi * r * r * r - cap_volume(r, h_plus),
        "V(cut) = the sphere − the cap",
    );
    approx(
        v_common + v_cut,
        4.0 / 3.0 * pi * r * r * r,
        "the two caps partition the sphere",
    );

    let a_common = brep_area(expect_solid(&common, "common")).unwrap_or_else(|e| panic!("{e}"));
    let a_cut = brep_area(expect_solid(&cut, "cut")).unwrap_or_else(|e| panic!("{e}"));
    approx(
        a_common,
        2.0 * pi * r * h_plus + pi * rho2,
        "A(common) = patch + disk",
    );
    approx(
        a_cut,
        2.0 * pi * r * (r - s) + pi * rho2,
        "A(cut) = patch + disk",
    );
}

#[test]
fn sphere_plane_hemispheres_when_the_plane_cuts_the_center() {
    let c = v3(-1.0, 2.0, 0.25);
    let r = 2.0;
    // A plane through the center (p = c: the offset vector is the
    // zero vector, so s == 0 exactly).
    let p = c;
    let n = v3(0.0, 0.6, 0.8);
    let pi = std::f64::consts::PI;

    let common = sphere_plane_boolean(BooleanOp::Common, c, r, p, n).expect("hemisphere");
    let v = brep_volume(expect_solid(&common, "hemisphere")).unwrap_or_else(|e| panic!("{e}"));
    approx(v, 2.0 / 3.0 * pi * r * r * r, "V(hemisphere)");
    let a = brep_area(expect_solid(&common, "hemisphere")).unwrap_or_else(|e| panic!("{e}"));
    approx(
        a,
        2.0 * pi * r * r + pi * r * r,
        "A(hemisphere) = dome + disk",
    );
}

#[test]
fn sphere_plane_classifications_are_exact() {
    let c = v3(0.0, 0.0, 0.0);
    let r = 2.0;
    let n = v3(0.0, 0.0, 1.0);

    // Bit-exact tangency (integer arithmetic: s == ±r exactly).
    // Tool {z ≥ 2}: the sphere sits below, tangent at the north pole —
    // the common is a point (empty), the cut is the whole sphere.
    {
        let p = v3(0.0, 0.0, 2.0);
        let common = sphere_plane_boolean(BooleanOp::Common, c, r, p, n).unwrap();
        assert!(
            matches!(common.kind, BooleanOutcomeKind::Empty),
            "tangent below: common is empty"
        );
        let cut = sphere_plane_boolean(BooleanOp::Cut, c, r, p, n).unwrap();
        let brep = expect_solid(&cut, "tangent cut");
        brep.validate().expect("the whole sphere validates");
        approx(
            brep_volume(brep).unwrap(),
            4.0 / 3.0 * std::f64::consts::PI * r * r * r,
            "tangent below: cut = the sphere",
        );
    }
    // Tool {z ≥ −2}: the sphere sits inside, tangent at the south
    // pole — the common is the whole sphere, the cut is empty.
    {
        let p = v3(0.0, 0.0, -2.0);
        let cut = sphere_plane_boolean(BooleanOp::Cut, c, r, p, n).unwrap();
        assert!(
            matches!(cut.kind, BooleanOutcomeKind::Empty),
            "tangent inside: cut is empty"
        );
        let common = sphere_plane_boolean(BooleanOp::Common, c, r, p, n).unwrap();
        let brep = expect_solid(&common, "tangent common");
        brep.validate().expect("the whole sphere validates");
        approx(
            brep_volume(brep).unwrap(),
            4.0 / 3.0 * std::f64::consts::PI * r * r * r,
            "tangent inside: common = the sphere",
        );
    }

    // Disjoint, sphere entirely outside the tool: common empty,
    // cut = the whole sphere.
    let p_out = v3(0.0, 0.0, 10.0);
    assert!(matches!(
        sphere_plane_boolean(BooleanOp::Common, c, r, p_out, n)
            .unwrap()
            .kind,
        BooleanOutcomeKind::Empty
    ));
    let far_cut = sphere_plane_boolean(BooleanOp::Cut, c, r, p_out, n).unwrap();
    let brep = expect_solid(&far_cut, "far cut");
    brep.validate().expect("whole sphere");

    // Disjoint, sphere entirely inside the tool: common = the whole
    // sphere, cut empty.
    let p_in = v3(0.0, 0.0, -10.0);
    let deep_common = sphere_plane_boolean(BooleanOp::Common, c, r, p_in, n).unwrap();
    let brep = expect_solid(&deep_common, "deep common");
    brep.validate().expect("whole sphere");
    let v = brep_volume(brep).unwrap();
    approx(
        v,
        4.0 / 3.0 * std::f64::consts::PI * r * r * r,
        "whole-sphere common",
    );
    assert!(matches!(
        sphere_plane_boolean(BooleanOp::Cut, c, r, p_in, n)
            .unwrap()
            .kind,
        BooleanOutcomeKind::Empty
    ));

    // Fuse is refused by name: an unbounded solid.
    match sphere_plane_boolean(BooleanOp::Fuse, c, r, v3(0.0, 0.0, 1.0), n)
        .unwrap()
        .kind
    {
        BooleanOutcomeKind::Refused { why } => {
            assert!(
                why.to_lowercase().contains("unbounded"),
                "the refusal names the geometry: {why}"
            );
        }
        other => panic!("fuse refused, got {other:?}"),
    }

    // A non-unit normal is refused (the K1 frame contract).
    let bad = sphere_plane_boolean(
        BooleanOp::Common,
        c,
        r,
        v3(0.0, 0.0, 1.0),
        v3(1.0, 1.0, 1.0),
    );
    assert!(bad.is_err() || matches!(bad.unwrap().kind, BooleanOutcomeKind::Refused { .. }));
}

// ---------------------------------------------------------------------------
// honest refusals
// ---------------------------------------------------------------------------

/// A closed two-face shell: `surface` trimmed by the circle, plus a
/// plane disk on the same circle (the cap-solid shape with the
/// spherical face swapped for an arbitrary surface).
fn shell_with_face(surface: Surface) -> ExactBRep {
    let circle = Curve::Circle {
        center: v3(0.0, 0.0, 1.0),
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    let vertex = TopoVertex {
        point: circle.point(0.0),
    };
    let edge = TopoEdge {
        curve: circle,
        range: (0.0, std::f64::consts::TAU),
        start: 0,
        end: 0,
    };
    let probe = |surface: Surface, forward: bool| TopoFace {
        surface,
        uv_bounds: [-1.0, 1.0, -1.0, 1.0],
        wires: vec![TopoWire {
            edges: vec![WireEdge { edge: 0, forward }],
        }],
        reversed: false,
    };
    ExactBRep {
        vertices: vec![vertex],
        edges: vec![edge],
        faces: vec![
            probe(surface, true),
            probe(
                Surface::Plane {
                    origin: v3(0.0, 0.0, 1.0),
                    normal: v3(0.0, 0.0, -1.0),
                    x_dir: v3(1.0, 0.0, 0.0),
                },
                false,
            ),
        ],
    }
}

#[test]
fn measure_refuses_what_it_cannot_close_by_name() {
    // A B-spline face: no closed form, refused with the surface named.
    let bspline = Surface::Bspline {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![
            vec![v3(-1.0, -1.0, 1.0), v3(-1.0, 1.0, 1.0)],
            vec![v3(1.0, -1.0, 1.0), v3(1.0, 1.0, 1.0)],
        ],
        weights: None,
    };
    let brep = shell_with_face(bspline);
    brep.validate().expect("the shell is structurally valid");
    for (err, what) in [(brep_volume(&brep), "volume"), (brep_area(&brep), "area")] {
        let err = err.expect_err(what);
        assert!(
            err.to_lowercase().contains("b-spline"),
            "{what} refusal names the surface: {err}"
        );
    }

    // An untrimmed plane face: an unbounded region.
    let unwired = ExactBRep {
        vertices: vec![],
        edges: vec![],
        faces: vec![TopoFace {
            surface: Surface::Plane {
                origin: v3(0.0, 0.0, 0.0),
                normal: v3(0.0, 0.0, 1.0),
                x_dir: v3(1.0, 0.0, 0.0),
            },
            uv_bounds: [-1.0, 1.0, -1.0, 1.0],
            wires: vec![],
            reversed: false,
        }],
    };
    unwired.validate().expect("structurally valid");
    let err = brep_volume(&unwired).expect_err("untrimmed plane");
    assert!(
        err.to_lowercase().contains("untrimmed") || err.to_lowercase().contains("unbounded"),
        "the refusal names the region: {err}"
    );

    // A cylinder face trimmed by a single circle: an unbounded
    // half-infinite mantle, not the two-circle strip measure reads.
    let cyl = shell_with_face(Surface::Cylinder {
        origin: v3(0.0, 0.0, 0.0),
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius: 1.0,
    });
    let err = brep_volume(&cyl).expect_err("circle-trimmed cylinder");
    assert!(
        err.to_lowercase().contains("cylinder"),
        "the refusal names the face kind: {err}"
    );
}

// ---------------------------------------------------------------------------
// FCBREP v3 — the orientation flag on the wire
// ---------------------------------------------------------------------------

#[test]
fn fcbrep_v3_round_trips_the_orientation_flag() {
    let (ca, ra, cb, rb) = pair();
    let cut = sphere_sphere_boolean(BooleanOp::Cut, ca, ra, cb, rb).unwrap();
    let crescent = expect_solid(&cut, "cut");
    let fuse = sphere_sphere_boolean(BooleanOp::Fuse, ca, ra, cb, rb).unwrap();
    let peanut = expect_solid(&fuse, "fuse");
    // The orientation defect is now visible in the data: the cut
    // wall is a reversed face; the peanut's is not.
    let reversed_of = |b: &ExactBRep| -> Vec<bool> { b.faces.iter().map(|f| f.reversed).collect() };
    assert_ne!(
        reversed_of(crescent),
        reversed_of(peanut),
        "cut ≠ fuse in the data"
    );

    let mut bytes = Vec::new();
    write_brep(&mut bytes, crescent).expect("v3 write");
    let back = read_brep_bytes(&bytes).expect("round-trip parse");
    assert_eq!(&back, crescent, "v3 round-trip is lossless");
    let mut again = Vec::new();
    write_brep(&mut again, &back).expect("v3 rewrite");
    assert_eq!(again, bytes, "v3 round-trip is byte-identical");

    // v2 payloads still read: strip the orientation token from the
    // face lines, relabel the header, and the lens (all faces
    // unreversed) comes back unchanged.
    let common = sphere_sphere_boolean(BooleanOp::Common, ca, ra, cb, rb).unwrap();
    let lens = expect_solid(&common, "common");
    let mut v3_bytes = Vec::new();
    write_brep(&mut v3_bytes, lens).expect("v3 write");
    let v3_text = String::from_utf8(v3_bytes.clone()).expect("utf8");
    let mut v2_lines: Vec<String> = Vec::new();
    for (i, line) in v3_text.lines().enumerate() {
        if i == 0 {
            v2_lines.push("FCBREP 2".to_string());
        } else if line.starts_with("f ") {
            // Drop the trailing orientation token (the last of the
            // face line's tail).
            let toks = line.split_whitespace().collect::<Vec<_>>();
            v2_lines.push(toks[..toks.len() - 1].join(" "));
        } else {
            v2_lines.push(line.to_string());
        }
    }
    let v2_payload = format!("{}\n", v2_lines.join("\n"));
    let from_v2 = read_brep_bytes(v2_payload.as_bytes()).expect("v2 still reads");
    assert_eq!(&from_v2, lens, "v2 faces default unreversed");

    // Strictness preserved: a v3 face line under a v2 header is
    // malformed.
    let mut strict = v3_text.replace("FCBREP 3", "FCBREP 2");
    strict.push('\n');
    assert!(
        read_brep_bytes(strict.as_bytes()).is_err(),
        "v2 rejects the orientation token"
    );
}
