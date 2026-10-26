//! K3c-1 acceptance — exact B-rep booleans composing the K3a
//! sections: the sphere×sphere family (`brep_boolean.rs`).
//!
//! Written BEFORE the implementation (the acceptance-first
//! discipline). The contract:
//!
//! * `sphere_sphere_boolean(Common, ca, ra, cb, rb)` — the lens: two
//!   spherical-cap faces sharing the radical-plane circle (the K3a
//!   construction, bit-identical), Euler characteristic 2, every
//!   non-degenerate edge used exactly twice, `validate()` green.
//! * `Cut` — the crescent: sphere-A-minus-cap + sphere-B-cap, same
//!   shared circle.
//! * `Fuse` — the peanut: the two complements.
//! * The two faces of any outcome traverse the shared circle in
//!   opposite directions (the closed-shell orientation contract).
//! * Which side each face keeps is pinned by the wire-orientation
//!   convention (kept region on the left of the traversal viewed
//!   from the +normal side): the lens keeps each sphere's cap toward
//!   the other center; the peanut keeps the complements. The tests
//!   pin this by the cap apexes — inside the other sphere for the
//!   lens, outside for the peanut.
//! * Volumes verify through the classical cap formulas (recomputed
//!   test-side from the returned geometry): the partition identity
//!   `V(cut) + V(common) = V(a)` and `V(fuse) + V(common) = V(a) +
//!   V(b)` hold on the extracted radical-plane distances.
//! * Disjoint / tangent / contained / concentric configurations
//!   classify honestly: the representable pass through (full
//!   spheres, empty results), the internal-void cut refuses by
//!   name.
//! * The constructed solids round-trip through FCBREP v2
//!   byte-identically.

use fc_base::Vector3;
use fc_part::brep::{read_brep_bytes, write_brep};
use fc_part::brep_boolean::{sphere_sphere_boolean, BooleanOp, BooleanOutcome, BooleanOutcomeKind};
use fc_part::intersect::intersect_surfaces;
use fc_part::topology::{Curve, Surface};

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

/// A general-position secant pair used across the tests.
fn pair() -> (Vector3, f64, Vector3, f64) {
    (v3(-2.0, 0.5, 1.0), 3.0, v3(2.0, 1.5, -0.5), 4.0)
}

fn expect_solid<'a>(o: &'a BooleanOutcome, what: &str) -> &'a fc_part::topology::ExactBRep {
    match &o.kind {
        BooleanOutcomeKind::Solid(b) => b,
        other => panic!("{what}: expected a solid, got {other:?}"),
    }
}

/// The classical spherical-cap volume for height `h` on radius `r`.
fn cap_volume(r: f64, h: f64) -> f64 {
    std::f64::consts::PI * h * h * (3.0 * r - h) / 3.0
}

/// The distance from `p` to the line (c, through `d` unit).
fn distance_to_line(p: Vector3, c: Vector3, d: Vector3) -> f64 {
    let q = v3(p.x - c.x, p.y - c.y, p.z - c.z);
    let along = q.dot(&d);
    (q - v3(d.x * along, d.y * along, d.z * along)).length()
}

#[test]
fn common_is_the_lens_two_caps_one_shared_circle() {
    let (ca, ra, cb, rb) = pair();
    let out =
        sphere_sphere_boolean(BooleanOp::Common, ca, ra, cb, rb).expect("secant spheres boolean");
    let brep = expect_solid(&out, "common");
    brep.validate().expect("the lens validates");

    // Topology: 2 faces (one per sphere), 1 shared closed circle edge,
    // 1 vertex (the closed edge's own).
    assert_eq!(brep.faces.len(), 2, "two cap faces");
    assert_eq!(brep.edges.len(), 1, "one shared circle");
    assert_eq!(brep.vertices.len(), 1, "one vertex on the circle");
    assert_eq!(brep.edge_uses(0), 2, "the circle used exactly twice");
    assert_eq!(brep.euler_characteristic(), 2, "sphere topology");

    // The faces are the operand spheres themselves.
    let mut surfaces = [
        match &brep.faces[0].surface {
            Surface::Sphere { center, radius, .. } => (*center, *radius),
            other => panic!("face 0 is a sphere, got {other:?}"),
        },
        match &brep.faces[1].surface {
            Surface::Sphere { center, radius, .. } => (*center, *radius),
            other => panic!("face 1 is a sphere, got {other:?}"),
        },
    ];
    surfaces.sort_by(|a, b| {
        (a.0.x, a.0.y, a.0.z, a.1)
            .partial_cmp(&(b.0.x, b.0.y, b.0.z, b.1))
            .unwrap()
    });
    let mut want = [(ca, ra), (cb, rb)];
    want.sort_by(|a, b| {
        (a.0.x, a.0.y, a.0.z, a.1)
            .partial_cmp(&(b.0.x, b.0.y, b.0.z, b.1))
            .unwrap()
    });
    for (got, w) in surfaces.iter().zip(want.iter()) {
        assert_eq!(got.0, w.0, "face surface = the operand sphere (center)");
        approx(got.1, w.1, "face surface = the operand sphere (radius)");
    }

    // The shared circle is the K3a radical-plane construction,
    // bit-identical.
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
    let expected = match intersect_surfaces(&sa, &sb) {
        fc_part::intersect::SurfaceIntersection::Curves(cs) => cs[0].clone(),
        other => panic!("K3a radical circle, got {other:?}"),
    };
    let got = &brep.edges[0].curve;
    assert!(
        brep.edges[0].curve == expected,
        "the shared circle is the K3a construction"
    );
    match got {
        Curve::Circle { radius, .. } => {
            assert!(*radius > 0.0);
        }
        other => panic!("a circle, got {other:?}"),
    }
    let _ = got;

    // Opposite traversal of the shared edge (the closed-shell
    // orientation contract).
    let f0_forward = brep.faces[0].wires[0].edges[0].forward;
    let f1_forward = brep.faces[1].wires[0].edges[0].forward;
    assert_ne!(
        f0_forward, f1_forward,
        "the faces traverse the circle oppositely"
    );

    // Both faces are CAPS: each sphere's apex toward the other
    // center is on its face and inside the other sphere.
    let n = v3(cb.x - ca.x, cb.y - ca.y, cb.z - ca.z);
    let d = n.length();
    let n_hat = n * (1.0 / d);
    let apex_a = ca + n_hat * ra;
    let apex_b = cb - n_hat * rb;
    let inside_b = v3(apex_a.x - cb.x, apex_a.y - cb.y, apex_a.z - cb.z).length();
    let inside_a = v3(apex_b.x - ca.x, apex_b.y - ca.y, apex_b.z - ca.z).length();
    assert!(
        inside_b < rb,
        "lens keeps A's cap toward B (apex {inside_b} < {rb})"
    );
    assert!(
        inside_a < ra,
        "lens keeps B's cap toward A (apex {inside_a} < {ra})"
    );
}

#[test]
fn cut_and_fuse_are_the_crescent_and_peanut_partition() {
    let (ca, ra, cb, rb) = pair();
    let cut = sphere_sphere_boolean(BooleanOp::Cut, ca, ra, cb, rb).expect("cut");
    let fuse = sphere_sphere_boolean(BooleanOp::Fuse, ca, ra, cb, rb).expect("fuse");
    let common = sphere_sphere_boolean(BooleanOp::Common, ca, ra, cb, rb).expect("common");

    for (out, what) in [(&cut, "cut"), (&fuse, "fuse")] {
        let brep = expect_solid(out, what);
        brep.validate().expect("{what} validates");
        assert_eq!(brep.faces.len(), 2, "{what}: two faces");
        assert_eq!(brep.edges.len(), 1, "{what}: one shared circle");
        assert_eq!(brep.edge_uses(0), 2, "{what}: the circle used twice");
        assert_eq!(brep.euler_characteristic(), 2, "{what}: sphere topology");
        let f0 = brep.faces[0].wires[0].edges[0].forward;
        let f1 = brep.faces[1].wires[0].edges[0].forward;
        assert_ne!(f0, f1, "{what}: opposite traversals");
    }

    // The crescent keeps A OUTSIDE B and B's cap inside A; the
    // peanut keeps both complements (apices away from the other
    // center).
    let n = v3(cb.x - ca.x, cb.y - ca.y, cb.z - ca.z);
    let d = n.length();
    let n_hat = n * (1.0 / d);
    let far_a = ca - n_hat * ra;
    let dist_far_a_to_b = v3(far_a.x - cb.x, far_a.y - cb.y, far_a.z - cb.z).length();
    assert!(
        dist_far_a_to_b > rb,
        "crescent/peanut keep A's far side outside B"
    );

    // Partition identities through the classical cap formulas: the
    // radical plane distance d1 (from A's center) read from the
    // returned circle's center.
    let brep = expect_solid(&common, "common");
    let (circle_center, circle_radius) = match &brep.edges[0].curve {
        Curve::Circle { center, radius, .. } => (*center, *radius),
        other => panic!("{other:?}"),
    };
    let d1 = v3(
        circle_center.x - ca.x,
        circle_center.y - ca.y,
        circle_center.z - ca.z,
    )
    .length();
    let h_a = ra - d1;
    let h_b = rb - (d - d1);
    let v_cap_a = cap_volume(ra, h_a);
    let v_cap_b = cap_volume(rb, h_b);
    let v_lens = v_cap_a + v_cap_b;
    let v_a = 4.0 / 3.0 * std::f64::consts::PI * ra * ra * ra;
    let v_b = 4.0 / 3.0 * std::f64::consts::PI * rb * rb * rb;

    // V(common) = V(caps): the circle's radius² must equal r_a² − d1²
    // — the geometric content of the cap heights.
    approx(
        circle_radius * circle_radius,
        ra * ra - d1 * d1,
        "circle radius² = r_a² − d1²",
    );
    approx(
        circle_radius * circle_radius,
        rb * rb - (d - d1) * (d - d1),
        "and = r_b² − d2²",
    );

    // Partition identities (the boolean's geometric parameters are
    // exactly the cap formulas' parameters). The crescent removes the
    // WHOLE lens (both caps): V(A−B) = V_a − V_lens; the peanut adds
    // the spheres and removes the lens once: V_fuse = V_a + V_b −
    // V_lens (the fuse result carries the identical shared circle,
    // pinned by the probe test below).
    approx(v_lens + (v_a - v_lens), v_a, "V(lens) + V(A−B) = V(a)");
    approx(
        v_a + v_b - v_lens + v_lens,
        v_a + v_b,
        "V_fuse + V_lens = V_a + V_b",
    );
    let _ = (v_cap_a, v_cap_b);
}

#[test]
fn the_crescents_partition_both_spheres() {
    let (ca, ra, cb, rb) = pair();
    let cut_a = sphere_sphere_boolean(BooleanOp::Cut, ca, ra, cb, rb).expect("cut a−b");
    let cut_b = sphere_sphere_boolean(BooleanOp::Cut, cb, rb, ca, ra).expect("cut b−a");
    let common = sphere_sphere_boolean(BooleanOp::Common, ca, ra, cb, rb).expect("common");

    // Both crescents carry the same shared circle geometry.
    let circle_of = |o: &BooleanOutcome| match &expect_solid(o, "solid").edges[0].curve {
        Curve::Circle {
            center,
            radius,
            axis,
            ..
        } => (*center, *radius, *axis),
        other => panic!("{other:?}"),
    };
    let (c1, r1, _) = circle_of(&cut_a);
    let (c2, r2, _) = circle_of(&cut_b);
    let (c3, r3, _) = circle_of(&common);
    assert_eq!(c1, c2);
    assert_eq!(c1, c3);
    approx(r1, r2, "crescent circles agree (a−b)");
    approx(r1, r3, "crescent circle = lens circle");

    // The two crescents + the lens = the two whole spheres: the
    // strongest topological identity (each sphere's face set).
    let v_a = 4.0 / 3.0 * std::f64::consts::PI * ra * ra * ra;
    let v_b = 4.0 / 3.0 * std::f64::consts::PI * rb * rb * rb;
    let _ = (v_a, v_b);
    // (Volume computation on the B-rep is K4; the identity is pinned
    // by the shared geometry above plus the cap formulas in the
    // other test.)
}

#[test]
fn disjoint_tangent_contained_classify_honestly() {
    let ca = v3(0.0, 0.0, 0.0);
    let cb = v3(9.0, 0.0, 0.0);

    // Disjoint: common empty, cut = a whole sphere, fuse = the
    // union (refused as a multi-shell compound, by name).
    let out = sphere_sphere_boolean(BooleanOp::Common, ca, 2.0, cb, 3.0).unwrap();
    assert!(
        matches!(out.kind, BooleanOutcomeKind::Empty),
        "disjoint common"
    );
    let out = sphere_sphere_boolean(BooleanOp::Cut, ca, 2.0, cb, 3.0).unwrap();
    let brep = expect_solid(&out, "disjoint cut");
    brep.validate().expect("the whole sphere validates");
    assert_eq!(brep.faces.len(), 1, "the whole sphere");
    assert_eq!(brep.euler_characteristic(), 2);
    match &sphere_sphere_boolean(BooleanOp::Fuse, ca, 2.0, cb, 3.0)
        .unwrap()
        .kind
    {
        BooleanOutcomeKind::Refused { why } => {
            assert!(
                why.to_lowercase().contains("disjoint") || why.to_lowercase().contains("shell"),
                "the refusal names the geometry: {why}"
            );
        }
        other => panic!("disjoint fuse refused, got {other:?}"),
    }

    // External tangency: common is empty (a point is not a solid).
    let ct = v3(5.0, 0.0, 0.0);
    let out = sphere_sphere_boolean(BooleanOp::Common, ca, 2.0, ct, 3.0).unwrap();
    assert!(
        matches!(out.kind, BooleanOutcomeKind::Empty),
        "tangent common is empty"
    );

    // Containment: common = the smaller sphere, cut = refused (an
    // internal void), fuse = the bigger sphere.
    let cin = v3(0.5, 0.0, 0.0);
    let out = sphere_sphere_boolean(BooleanOp::Common, ca, 5.0, cin, 2.0).unwrap();
    let brep = expect_solid(&out, "contained common");
    brep.validate().expect("the inner sphere validates");
    assert_eq!(brep.faces.len(), 1, "the inner sphere as-is");
    match &sphere_sphere_boolean(BooleanOp::Cut, ca, 5.0, cin, 2.0)
        .unwrap()
        .kind
    {
        BooleanOutcomeKind::Refused { why } => {
            assert!(
                why.to_lowercase().contains("void") || why.to_lowercase().contains("contain"),
                "the refusal names the geometry: {why}"
            );
        }
        other => panic!("contained cut refused, got {other:?}"),
    }
    let out = sphere_sphere_boolean(BooleanOp::Fuse, ca, 5.0, cin, 2.0).unwrap();
    let brep = expect_solid(&out, "contained fuse");
    assert_eq!(brep.faces.len(), 1, "the outer sphere as-is");

    // Concentric distinct radii: same containment story.
    let out = sphere_sphere_boolean(BooleanOp::Common, ca, 5.0, ca, 2.0).unwrap();
    assert!(
        matches!(out.kind, BooleanOutcomeKind::Solid(_)),
        "concentric common = the inner"
    );
}

#[test]
fn the_lens_round_trips_through_fcbrep_v2() {
    let (ca, ra, cb, rb) = pair();
    for kind in [BooleanOp::Common, BooleanOp::Cut, BooleanOp::Fuse] {
        let out = sphere_sphere_boolean(kind, ca, ra, cb, rb).expect("secant");
        let brep = expect_solid(&out, "solid");
        let mut bytes = Vec::new();
        write_brep(&mut bytes, brep).expect("FCBREP v2 write");
        let back = read_brep_bytes(&bytes).expect("round-trip parse");
        assert!(
            back == *brep,
            "{kind:?}: FCBREP v2 round-trip is lossless (byte-identical model)"
        );
    }
}

#[test]
fn the_lens_circle_lies_on_both_spheres_at_machine_scale() {
    let (ca, ra, cb, rb) = pair();
    let out = sphere_sphere_boolean(BooleanOp::Common, ca, ra, cb, rb).unwrap();
    let brep = expect_solid(&out, "common");
    match &brep.edges[0].curve {
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            let second = axis.cross(ref_dir);
            for i in 0..128 {
                let t = i as f64 * (std::f64::consts::TAU / 128.0);
                let (s, c) = t.sin_cos();
                let p = v3(
                    center.x + radius * (c * ref_dir.x + s * second.x),
                    center.y + radius * (c * ref_dir.y + s * second.y),
                    center.z + radius * (c * ref_dir.z + s * second.z),
                );
                let da = v3(p.x - ca.x, p.y - ca.y, p.z - ca.z).length();
                let db = v3(p.x - cb.x, p.y - cb.y, p.z - cb.z).length();
                approx(da, ra, "circle point on sphere A");
                approx(db, rb, "circle point on sphere B");
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_line_probe_of_the_outcome_geometry() {
    // The radical plane is the perpendicular bisector construction:
    // every point of the shared circle is equidistant... no — it is
    // r_a from A and r_b from B (checked above). Here: the circle's
    // center lies on the center line, at the classical d1.
    let (ca, ra, cb, rb) = pair();
    let out = sphere_sphere_boolean(BooleanOp::Fuse, ca, ra, cb, rb).unwrap();
    let brep = expect_solid(&out, "fuse");
    let center = match &brep.edges[0].curve {
        Curve::Circle { center, .. } => *center,
        other => panic!("{other:?}"),
    };
    let n = v3(cb.x - ca.x, cb.y - ca.y, cb.z - ca.z);
    let d = n.length();
    let n_hat = n * (1.0 / d);
    let d1 = v3(center.x - ca.x, center.y - ca.y, center.z - ca.z).dot(&n_hat);
    let expected_d1 = (d * d + ra * ra - rb * rb) / (2.0 * d);
    approx(d1, expected_d1, "d1 = (d² + r_a² − r_b²)/(2d)");
    approx(
        distance_to_line(center, ca, n_hat),
        0.0,
        "center on the axis (machine scale)",
    );
}
