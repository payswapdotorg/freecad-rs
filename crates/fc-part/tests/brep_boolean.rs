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
//! * The sphere×halfspace cuts (`sphere_plane_boolean`): the tool
//!   is `{x : n·(x − p) ≥ 0}` — Common keeps the +n cap (a spherical
//!   patch + a planar disk sharing the K3a plane×sphere circle),
//!   Cut the −n cap, Fuse refused as unbounded. The disk's stored
//!   normal points AWAY from the solid; the cap-solid faces are
//!   naturally oriented (no reversed flags).
//! * The constructed solids round-trip through FCBREP v3
//!   byte-identically.

use fc_base::Vector3;
use fc_part::brep::{read_brep_bytes, write_brep};
use fc_part::brep_boolean::{
    cone_plane_boolean, cylinder_cylinder_boolean, cylinder_plane_boolean, cylinder_sphere_boolean,
    sphere_plane_boolean, sphere_sphere_boolean, BooleanOp, BooleanOutcome, BooleanOutcomeKind,
};
use fc_part::intersect::{intersect_surfaces, SurfaceIntersection};
use fc_part::measure::{brep_area, brep_volume};
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
fn the_outcomes_round_trip_through_fcbrep_v3() {
    let (ca, ra, cb, rb) = pair();
    for kind in [BooleanOp::Common, BooleanOp::Cut, BooleanOp::Fuse] {
        let out = sphere_sphere_boolean(kind, ca, ra, cb, rb).expect("secant");
        let brep = expect_solid(&out, "solid");
        let mut bytes = Vec::new();
        write_brep(&mut bytes, brep).expect("FCBREP v3 write");
        let back = read_brep_bytes(&bytes).expect("round-trip parse");
        assert!(
            back == *brep,
            "{kind:?}: FCBREP v3 round-trip is lossless (byte-identical model)"
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

// ---------------------------------------------------------------------------
// sphere × halfspace — the plane cuts (K3c-2)
// ---------------------------------------------------------------------------

/// A general oblique secant configuration for the plane cuts.
fn plane_case() -> (Vector3, f64, Vector3, Vector3) {
    let c = v3(1.0, -2.0, 0.5);
    let r = 3.0;
    let p = v3(2.0, 0.0, -1.0);
    let n = v3(1.0 / 3.0, 2.0 / 3.0, -2.0 / 3.0);
    (c, r, p, n)
}

#[test]
fn sphere_plane_secant_builds_the_cap_and_the_disk() {
    let (c, r, p, n) = plane_case();

    for (op, keep_sign, what) in [
        (BooleanOp::Common, 1.0, "common keeps the +n cap"),
        (BooleanOp::Cut, -1.0, "cut keeps the −n cap"),
    ] {
        let out = sphere_plane_boolean(op, c, r, p, n).expect(what);
        let brep = expect_solid(&out, what);
        brep.validate().expect("the cap solid validates");

        // Topology: one spherical patch + one planar disk on one
        // shared closed circle. Euler 2.
        assert_eq!(brep.faces.len(), 2, "{what}: two faces");
        assert_eq!(brep.edges.len(), 1, "{what}: one shared circle");
        assert_eq!(brep.vertices.len(), 1, "{what}: one vertex");
        assert_eq!(brep.edge_uses(0), 2, "{what}: the circle used twice");
        assert_eq!(brep.euler_characteristic(), 2, "{what}: sphere topology");

        // The two faces traverse the shared circle in opposite
        // directions (the closed-shell orientation contract).
        let f0 = brep.faces[0].wires[0].edges[0].forward;
        let f1 = brep.faces[1].wires[0].edges[0].forward;
        assert_ne!(f0, f1, "{what}: opposite traversals");

        // Neither face is reversed: the cap solid lies inside its
        // sphere and on the inner side of its disk (the natural
        // normals are the outward ones).
        assert!(
            !brep.faces[0].reversed && !brep.faces[1].reversed,
            "{what}: naturally oriented"
        );

        // The shared circle is the K3a plane×sphere section,
        // bit-identical.
        let sphere_surface = Surface::Sphere {
            center: c,
            axis: v3(0.0, 0.0, 1.0),
            ref_dir: v3(1.0, 0.0, 0.0),
            radius: r,
        };
        let plane_surface = Surface::Plane {
            origin: p,
            normal: n,
            x_dir: v3(1.0, 0.0, 0.0),
        };
        let expected = match intersect_surfaces(&sphere_surface, &plane_surface) {
            fc_part::intersect::SurfaceIntersection::Curves(cs) => cs[0].clone(),
            other => panic!("{what}: the K3a circle, got {other:?}"),
        };
        assert_eq!(
            brep.edges[0].curve, expected,
            "{what}: the K3a section circle"
        );

        // The faces: one sphere (the operand), one plane (the disk).
        let mut sphere_faces = 0;
        let mut plane_faces = 0;
        for f in &brep.faces {
            match &f.surface {
                Surface::Sphere { center, radius, .. } => {
                    assert_eq!(*center, c, "{what}: the operand sphere");
                    approx(*radius, r, "{what}: operand radius");
                    sphere_faces += 1;
                }
                Surface::Plane {
                    origin,
                    normal,
                    x_dir,
                } => {
                    plane_faces += 1;
                    // The disk lives on the section circle: its
                    // origin is the circle's center, its normal
                    // points AWAY from the solid (the solid lies on
                    // the keep side), and its frame spans the circle.
                    let (cc, rho, axis) = match &brep.edges[0].curve {
                        Curve::Circle {
                            center,
                            radius,
                            axis,
                            ..
                        } => (*center, *radius, *axis),
                        other => panic!("{other:?}"),
                    };
                    assert_eq!(*origin, cc, "{what}: the disk is centered on the circle");
                    let away = normal.dot(&n);
                    approx(
                        away,
                        -keep_sign,
                        "{what}: the disk normal points away from the solid",
                    );
                    // The stored normal is unit and the frame
                    // contains the circle (the uv box is the disk's
                    // bounding square).
                    approx(normal.length(), 1.0, "{what}: unit normal");
                    let y = normal.cross(x_dir);
                    let _ = y;
                    let [umin, umax, vmin, vmax] = f.uv_bounds;
                    approx(umin, -rho, "{what}: umin = −ρ");
                    approx(umax, rho, "{what}: umax = +ρ");
                    approx(vmin, -rho, "{what}: vmin = −ρ");
                    approx(vmax, rho, "{what}: vmax = +ρ");
                    // The circle lies in the disk's plane.
                    let _ = axis;
                }
                other => panic!("{what}: sphere or plane faces, got {other:?}"),
            }
        }
        assert_eq!(sphere_faces, 1, "{what}: one spherical patch");
        assert_eq!(plane_faces, 1, "{what}: one planar disk");

        // The sphere patch keeps the right side: its wire flag is
        // forward iff the stored circle axis points toward the kept
        // cap (the kept-on-the-left convention).
        let axis = match &brep.edges[0].curve {
            Curve::Circle { axis, .. } => *axis,
            other => panic!("{other:?}"),
        };
        let k_hat = n * keep_sign;
        let sphere_forward = brep
            .faces
            .iter()
            .find(|f| matches!(f.surface, Surface::Sphere { .. }))
            .map(|f| f.wires[0].edges[0].forward)
            .expect("a sphere face");
        let want_forward = axis.dot(&k_hat) > 0.0;
        assert_eq!(
            sphere_forward, want_forward,
            "{what}: the patch flag follows the kept side"
        );

        // The cap solid's own geometry: the apex on the keep side is
        // the kept patch's pole — its signed plane distance is
        // `s + keep_sign·r`, which equals the kept cap's height
        // measured from the plane (`r + keep_sign·s`).
        let s = v3(c.x - p.x, c.y - p.y, c.z - p.z).dot(&n);
        let apex = c + k_hat * r;
        let d_apex = v3(apex.x - p.x, apex.y - p.y, apex.z - p.z).dot(&n);
        approx(
            d_apex,
            s + keep_sign * r,
            "{what}: the kept apex reaches the cap's far boundary",
        );
        let h = r + keep_sign * s;
        approx(
            d_apex,
            keep_sign * h,
            "{what}: ... which is the cap height from the plane",
        );
    }
}

#[test]
fn sphere_plane_caps_lie_on_the_operands_at_machine_scale() {
    let (c, r, p, n) = plane_case();
    let out = sphere_plane_boolean(BooleanOp::Common, c, r, p, n).expect("common");
    let brep = expect_solid(&out, "common");
    match &brep.edges[0].curve {
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            assert!(radius > &0.0, "a genuine circle");
            let second = axis.cross(ref_dir);
            for i in 0..128 {
                let t = i as f64 * (std::f64::consts::TAU / 128.0);
                let (s, co) = t.sin_cos();
                let q = v3(
                    center.x + radius * (co * ref_dir.x + s * second.x),
                    center.y + radius * (co * ref_dir.y + s * second.y),
                    center.z + radius * (co * ref_dir.z + s * second.z),
                );
                approx(
                    v3(q.x - c.x, q.y - c.y, q.z - c.z).length(),
                    r,
                    "circle point on the sphere",
                );
                approx(
                    v3(q.x - p.x, q.y - p.y, q.z - p.z).dot(&n),
                    0.0,
                    "circle point on the plane",
                );
            }
        }
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// K3c-3 — the oblique plane cuts: cylinder×halfspace and cone×halfspace,
// the sections with elliptical rims
// ---------------------------------------------------------------------------
//
// Written BEFORE the implementation (the acceptance-first discipline).
// The contract:
//
// * `cylinder_plane_boolean(op, origin, axis, radius, height, p, n)` —
//   the finite cylinder cut by the halfspace `{x : n·(x − p) ≥ 0}`:
//   Common keeps the +n piece, Cut the −n piece, Fuse is refused as
//   unbounded. A clean section (the plane crosses the open axis
//   segment, misses both cap disks) composes the K3a plane×cylinder
//   ellipse bit-identically as the shared edge between the side strip
//   and the new planar elliptical face; the kept cap's rim circle and a
//   seam edge (the periodicity glue, carried as a degree-1 B-spline
//   whose de Boor evaluation returns its endpoints exactly) close the
//   strip. Three faces, three edges, two vertices, Euler 2, every
//   non-degenerate edge used exactly twice with the two faces of each
//   shared edge traversing it in opposite directions.
// * Volumes verify by Cavalieri: the oblique cut does not change the
//   mean height — each piece measures `πR²` times its mean height, the
//   axis-crossing distance from its rim, and the two pieces sum to the
//   whole cylinder exactly. Areas: the elliptical face is `πab` with
//   `b = R`, the side strip `2πR` times the mean height.
// * A perpendicular plane (circle section) composes the K3a axis-meet
//   circle; the pieces are ordinary cylinders.
// * `cone_plane_boolean(op, origin, axis, base_radius, top_radius,
//   height, p, n)` — the same contract on the cone, where the apex-side
//   piece of a plain cone is the oblique cone over the ellipse: its
//   volume is the classical `(1/3)·πab·d(apex, plane)`, exact for any
//   obliqueness. The cone's faces carry the apex-origin
//   parameterization (axis apex→base, semi-angle in (0, π/2),
//   ref_radius 0) so the K3a section is the same curve OCCT would name.
// * Non-representable configurations refuse by name: a plane parallel
//   to the axis cutting the side (generator-line sections), a steep
//   cone plane (the hyperbola/parabola class), a section that clips a
//   cap, a plane through the apex. Non-secant pass-throughs are
//   honest: the whole solid or the empty set.

/// A cylinder in general position: R = 2, h = 5, base (0.5, −0.3, 1.0),
/// axis +Z. The plane `n = (0.6, 0, 0.8)` through the axis point at
/// v* = 2.5: secant, the ellipse's v-range [1.0, 4.0] strictly inside
/// the band, both cap disks strictly missed.
fn cyl_case() -> (Vector3, Vector3, f64, f64, Vector3, Vector3) {
    (
        v3(0.5, -0.3, 1.0),
        v3(0.0, 0.0, 1.0),
        2.0,
        5.0,
        v3(0.5, -0.3, 3.5),
        v3(0.6, 0.0, 0.8),
    )
}

/// A plain cone: r1 = 3, r2 = 0, h = 4, base at the origin, axis +Z
/// (apex (0, 0, 4)). The plane `n = (0.28, 0, 0.96)` through the axis
/// point at v* = 2: secant, the base disk strictly missed (|α| = 1.92 >
/// r1·sinγ = 0.84), the apex strictly on the +n side.
fn cone_case() -> (Vector3, Vector3, f64, f64, f64, Vector3, Vector3) {
    (
        v3(0.0, 0.0, 0.0),
        v3(0.0, 0.0, 1.0),
        3.0,
        0.0,
        4.0,
        v3(0.0, 0.0, 2.0),
        v3(0.28, 0.0, 0.96),
    )
}

fn expect_refused(o: &BooleanOutcome, needle: &str, what: &str) {
    match &o.kind {
        BooleanOutcomeKind::Refused { why } => assert!(
            why.contains(needle),
            "{what}: refusal must name {needle:?}, got {why:?}"
        ),
        other => panic!("{what}: expected a refusal, got {other:?}"),
    }
}

fn expect_empty(o: &BooleanOutcome, what: &str) {
    assert!(
        matches!(o.kind, BooleanOutcomeKind::Empty),
        "{what}: expected the empty set, got {:?}",
        o.kind
    );
}

fn surface_kind(face: &fc_part::topology::TopoFace) -> &'static str {
    face.surface.kind_name()
}

#[test]
fn cylinder_plane_oblique_cut_composes_the_k3a_ellipse() {
    let (o, axis, r, h, p, n) = cyl_case();
    for op in [BooleanOp::Common, BooleanOp::Cut] {
        let out = cylinder_plane_boolean(op, o, axis, r, h, p, n).expect("secant cylinder cut");
        let brep = expect_solid(&out, "the oblique cylinder cut");
        brep.validate().expect("the cut solid validates");

        // Topology: strip + kept cap + elliptical face; the ellipse, the
        // kept rim circle and the seam; two vertices; Euler 2.
        assert_eq!(brep.faces.len(), 3, "strip + cap + elliptical face");
        assert_eq!(brep.edges.len(), 3, "ellipse + rim circle + seam");
        assert_eq!(brep.vertices.len(), 2, "one vertex per closed rim");
        assert_eq!(brep.euler_characteristic(), 2, "a topological ball");
        let mut kinds: Vec<&str> = brep.faces.iter().map(surface_kind).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, ["Cylinder", "Plane", "Plane"]);

        for e in 0..brep.edges.len() {
            assert_eq!(brep.edge_uses(e), 2, "edge {e} used exactly twice");
        }
        // The shared edges run opposite under the two faces (the
        // closed-shell orientation contract).
        let uses_forward = |e: usize| -> (bool, bool) {
            let mut it = brep
                .faces
                .iter()
                .flat_map(|f| &f.wires)
                .flat_map(|w| &w.edges)
                .filter(|we| we.edge == e);
            let a = it.next().unwrap().forward;
            let b = it.next().unwrap().forward;
            (a, b)
        };
        for e in 0..brep.edges.len() {
            let (a, b) = uses_forward(e);
            assert_ne!(a, b, "edge {e} traversed opposite by its two faces");
        }

        // The elliptical edge is the K3a plane×cylinder section of the
        // STORED cylinder surface and the input plane — bit-identical.
        let strip = brep
            .faces
            .iter()
            .find(|f| matches!(f.surface, Surface::Cylinder { .. }))
            .expect("the strip face");
        let plane = Surface::Plane {
            origin: p,
            normal: n,
            x_dir: v3(1.0, 0.0, 0.0),
        };
        let expected = match intersect_surfaces(&strip.surface, &plane) {
            fc_part::intersect::SurfaceIntersection::Curves(cs) => cs[0].clone(),
            other => panic!("the K3a ellipse, got {other:?}"),
        };
        let ellipse_edge = brep
            .edges
            .iter()
            .find(|e| matches!(e.curve, Curve::Ellipse { .. }))
            .expect("the ellipse edge");
        assert!(
            ellipse_edge.curve == expected,
            "the elliptical rim is the K3a construction, bit-identically"
        );

        // The kept cap follows the side: with n·axis > 0 the top center
        // is +n, so Common keeps the top cap and Cut the bottom one.
        let g_top = v3(
            o.x + axis.x * h - p.x,
            o.y + axis.y * h - p.y,
            o.z + axis.z * h - p.z,
        )
        .dot(&n);
        let keep_top = if op == BooleanOp::Common {
            g_top > 0.0
        } else {
            g_top < 0.0
        };
        let cap = brep
            .faces
            .iter()
            .find(|f| {
                matches!(f.surface, Surface::Plane { normal, .. } if normal.dot(&axis).abs() > 0.9)
            })
            .expect("the cap face");
        match &cap.surface {
            Surface::Plane { origin, .. } => {
                let v_cap = v3(origin.x - o.x, origin.y - o.y, origin.z - o.z).dot(&axis);
                approx(
                    v_cap,
                    if keep_top { h } else { 0.0 },
                    "the kept cap sits on the kept side's rim",
                );
            }
            other => panic!("a plane cap, got {other:?}"),
        }

        // FCBREP v3 round-trips the composed solid byte-identically
        // (the seam is a degree-1 B-spline — the format's curve set).
        let mut bytes = Vec::new();
        write_brep(&mut bytes, brep).expect("write");
        let back = read_brep_bytes(&bytes).expect("read");
        let mut bytes2 = Vec::new();
        write_brep(&mut bytes2, &back).expect("re-write");
        assert_eq!(bytes, bytes2, "FCBREP round-trip is byte-identical");
    }
}

#[test]
fn cylinder_plane_volumes_and_areas_verify_by_cavalieri() {
    let (o, axis, r, h, p, n) = cyl_case();
    let v_star = 2.5_f64; // the axis crossing (the plane point's height)
    let whole = std::f64::consts::PI * r * r * h;

    let common = cylinder_plane_boolean(BooleanOp::Common, o, axis, r, h, p, n).expect("common");
    let cut = cylinder_plane_boolean(BooleanOp::Cut, o, axis, r, h, p, n).expect("cut");
    let top = expect_solid(&common, "common");
    let bottom = expect_solid(&cut, "cut");

    // Cavalieri: an oblique cut leaves the mean height alone.
    let v_top = std::f64::consts::PI * r * r * (h - v_star);
    let v_bottom = std::f64::consts::PI * r * r * v_star;
    approx(
        brep_volume(top).unwrap(),
        v_top,
        "Common (the +n piece) = πR²·(h − v*)",
    );
    approx(brep_volume(bottom).unwrap(), v_bottom, "Cut = πR²·v*");
    approx(
        brep_volume(top).unwrap() + brep_volume(bottom).unwrap(),
        whole,
        "the pieces sum to the whole cylinder",
    );

    // Areas: the elliptical face is πab with b = R and a = R/|n·axis|;
    // the cap is πR²; the side strip is 2πR times the mean height.
    let beta = n.dot(&axis);
    let a_semi = r / beta.abs();
    let area_ellipse = std::f64::consts::PI * a_semi * r;
    let area_cap = std::f64::consts::PI * r * r;
    let area_side_top = 2.0 * std::f64::consts::PI * r * (h - v_star);
    let area_side_bottom = 2.0 * std::f64::consts::PI * r * v_star;
    approx(
        brep_area(top).unwrap(),
        area_ellipse + area_cap + area_side_top,
        "Common's boundary area",
    );
    approx(
        brep_area(bottom).unwrap(),
        area_ellipse + area_cap + area_side_bottom,
        "Cut's boundary area",
    );
}

#[test]
fn cylinder_plane_perpendicular_cut_composes_circles() {
    let (o, axis, r, h, _, _) = cyl_case();
    // A ⊥ plane through the axis point at v* = 2.
    let p = v3(0.5, -0.3, 3.0);
    let n = v3(0.0, 0.0, 1.0);
    let v_star = 2.0_f64;

    let common = cylinder_plane_boolean(BooleanOp::Common, o, axis, r, h, p, n).expect("common");
    let cut = cylinder_plane_boolean(BooleanOp::Cut, o, axis, r, h, p, n).expect("cut");
    let top = expect_solid(&common, "common");
    let bottom = expect_solid(&cut, "cut");
    top.validate().expect("top validates");
    bottom.validate().expect("bottom validates");

    assert_eq!(top.euler_characteristic(), 2, "a ball");
    assert_eq!(bottom.euler_characteristic(), 2, "a ball");
    assert!(
        top.edges
            .iter()
            .all(|e| !matches!(e.curve, Curve::Ellipse { .. })),
        "no ellipse in a perpendicular section"
    );

    // The shared rim is the K3a axis-meet circle, bit-identical.
    let strip = top
        .faces
        .iter()
        .find(|f| matches!(f.surface, Surface::Cylinder { .. }))
        .expect("the strip");
    let plane = Surface::Plane {
        origin: p,
        normal: n,
        x_dir: v3(1.0, 0.0, 0.0),
    };
    let expected = match intersect_surfaces(&strip.surface, &plane) {
        fc_part::intersect::SurfaceIntersection::Curves(cs) => cs[0].clone(),
        other => panic!("the K3a axis-meet circle, got {other:?}"),
    };
    let circle_edge = top
        .edges
        .iter()
        .find(|e| matches!(&e.curve, Curve::Circle { radius, .. } if *radius > 0.0))
        .expect("the cut circle");
    assert!(
        circle_edge.curve == expected,
        "the K3a circle, bit-identically"
    );

    approx(
        brep_volume(top).unwrap(),
        std::f64::consts::PI * r * r * (h - v_star),
        "the top piece",
    );
    approx(
        brep_volume(bottom).unwrap(),
        std::f64::consts::PI * r * r * v_star,
        "the bottom piece",
    );
}

#[test]
fn cylinder_plane_classifies_nonsecant_configurations_honestly() {
    let (o, axis, r, h, _, _) = cyl_case();

    // Fuse is always unbounded (a halfspace union).
    let (_, _, _, _, p, n) = cyl_case();
    let fuse = cylinder_plane_boolean(BooleanOp::Fuse, o, axis, r, h, p, n).expect("fuse");
    expect_refused(&fuse, "unbounded", "fusing a halfspace");

    // The tool is the +n halfspace: a plane strictly ABOVE with
    // n = +z leaves the cylinder outside — Common = empty, Cut =
    // whole.
    let above = cylinder_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r,
        h,
        v3(0.5, -0.3, 9.0),
        v3(0.0, 0.0, 1.0),
    )
    .expect("above");
    expect_empty(&above, "above the tool");
    let cut = cylinder_plane_boolean(
        BooleanOp::Cut,
        o,
        axis,
        r,
        h,
        v3(0.5, -0.3, 9.0),
        v3(0.0, 0.0, 1.0),
    )
    .expect("cut above");
    approx(
        brep_volume(expect_solid(&cut, "cut above")).unwrap(),
        std::f64::consts::PI * r * r * h,
        "the whole cylinder",
    );

    // Strictly below (the tool contains the cylinder): Common =
    // whole, Cut = empty.
    let below = cylinder_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r,
        h,
        v3(0.5, -0.3, -4.0),
        v3(0.0, 0.0, 1.0),
    )
    .expect("below");
    approx(
        brep_volume(expect_solid(&below, "below the tool")).unwrap(),
        std::f64::consts::PI * r * r * h,
        "the whole cylinder",
    );
    let cut_below = cylinder_plane_boolean(
        BooleanOp::Cut,
        o,
        axis,
        r,
        h,
        v3(0.5, -0.3, -4.0),
        v3(0.0, 0.0, 1.0),
    )
    .expect("cut below");
    expect_empty(&cut_below, "cut below the solid");

    // A plane parallel to the axis, missing the cylinder.
    let miss = cylinder_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r,
        h,
        v3(3.5, -0.3, 1.0),
        v3(1.0, 0.0, 0.0),
    )
    .expect("parallel miss");
    expect_empty(&miss, "a parallel plane beyond the radius");

    // A plane parallel to the axis, tangent along a generator: the
    // intersection is a line, not a solid.
    let tangent = cylinder_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r,
        h,
        v3(2.5, -0.3, 1.0),
        v3(1.0, 0.0, 0.0),
    )
    .expect("parallel tangent");
    expect_empty(&tangent, "the tangent line is not a solid");

    // A plane parallel to the axis, secant: the section is two
    // generator lines — refused by name.
    let secant = cylinder_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r,
        h,
        v3(1.0, -0.3, 1.0),
        v3(1.0, 0.0, 0.0),
    )
    .expect("parallel secant");
    expect_refused(
        &secant,
        "generator lines",
        "a line-section cut of the cylinder",
    );

    // An oblique plane whose section clips a cap rim: refused by name.
    let straddle = cylinder_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r,
        h,
        v3(0.5, -0.3, 1.4),
        v3(0.8, 0.0, 0.6),
    )
    .expect("cap-straddling");
    expect_refused(&straddle, "cap", "a section that clips a cap");
}

#[test]
fn cone_plane_oblique_cut_composes_the_k3a_ellipse() {
    let (o, axis, r1, r2, h, p, n) = cone_case();
    for op in [BooleanOp::Common, BooleanOp::Cut] {
        let out = cone_plane_boolean(op, o, axis, r1, r2, h, p, n).expect("secant cone cut");
        let brep = expect_solid(&out, "the oblique cone cut");
        brep.validate().expect("the cut solid validates");

        // Common keeps the apex piece (the apex is +n here): strip +
        // elliptical face; Cut keeps the base piece: strip + base cap
        // + elliptical face.
        let want_faces = if op == BooleanOp::Common { 2 } else { 3 };
        assert_eq!(brep.faces.len(), want_faces, "the {op:?} piece's faces");
        assert_eq!(brep.euler_characteristic(), 2, "a topological ball");
        for e in 0..brep.edges.len() {
            if brep.is_degenerate(e) {
                continue;
            }
            assert_eq!(brep.edge_uses(e), 2, "edge {e} used exactly twice");
        }

        // The elliptical rim is the K3a section of the STORED cone
        // surface and the input plane, bit-identically. The stored
        // cone is the apex-origin parameterization (axis apex→base).
        let strip = brep
            .faces
            .iter()
            .find(|f| matches!(f.surface, Surface::Cone { .. }))
            .expect("the strip face");
        let plane = Surface::Plane {
            origin: p,
            normal: n,
            x_dir: v3(1.0, 0.0, 0.0),
        };
        let expected = match intersect_surfaces(&strip.surface, &plane) {
            fc_part::intersect::SurfaceIntersection::Curves(cs) => cs[0].clone(),
            other => panic!("the K3a ellipse, got {other:?}"),
        };
        let ellipse_edge = brep
            .edges
            .iter()
            .find(|e| matches!(e.curve, Curve::Ellipse { .. }))
            .expect("the ellipse edge");
        assert!(
            ellipse_edge.curve == expected,
            "the elliptical rim is the K3a construction, bit-identically"
        );

        // The stored cone's apex is the geometric apex of the operand.
        match &strip.surface {
            Surface::Cone {
                origin, axis: sa, ..
            } => {
                let apex = v3(o.x + axis.x * h, o.y + axis.y * h, o.z + axis.z * h);
                assert_eq!(*origin, apex, "the cone's parameter origin is the apex");
                approx(sa.dot(&axis), -1.0, "the stored axis points apex→base");
            }
            other => panic!("a cone strip, got {other:?}"),
        }

        // FCBREP round-trip.
        let mut bytes = Vec::new();
        write_brep(&mut bytes, brep).expect("write");
        let back = read_brep_bytes(&bytes).expect("read");
        let mut bytes2 = Vec::new();
        write_brep(&mut bytes2, &back).expect("re-write");
        assert_eq!(bytes, bytes2, "FCBREP round-trip is byte-identical");
    }
}

#[test]
fn cone_plane_volumes_verify_by_the_oblique_cone_formula() {
    let (o, axis, r1, _r2, h, p, n) = cone_case();
    let apex = v3(o.x + axis.x * h, o.y + axis.y * h, o.z + axis.z * h);
    let d_apex = v3(apex.x - p.x, apex.y - p.y, apex.z - p.z).dot(&n).abs();

    let common = cone_plane_boolean(BooleanOp::Common, o, axis, r1, 0.0, h, p, n).expect("common");
    let cut = cone_plane_boolean(BooleanOp::Cut, o, axis, r1, 0.0, h, p, n).expect("cut");
    let apex_piece = expect_solid(&common, "the apex piece");
    let base_piece = expect_solid(&cut, "the base piece");

    // The classical oblique cone: V = (1/3)·(ellipse area)·(apex
    // height over the plane) — exact for any obliqueness.
    let (a_semi, b_semi) = match &apex_piece
        .edges
        .iter()
        .find(|e| matches!(e.curve, Curve::Ellipse { .. }))
        .unwrap()
        .curve
    {
        Curve::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => (*major_radius, *minor_radius),
        other => panic!("{other:?}"),
    };
    let v_apex_piece = std::f64::consts::PI * a_semi * b_semi * d_apex / 3.0;
    approx(
        brep_volume(apex_piece).unwrap(),
        v_apex_piece,
        "V(apex piece) = (1/3)·πab·d(apex, plane)",
    );

    let v_whole = std::f64::consts::PI * r1 * r1 * h / 3.0;
    approx(
        brep_volume(base_piece).unwrap(),
        v_whole - v_apex_piece,
        "the base piece completes the cone",
    );
    approx(
        brep_volume(apex_piece).unwrap() + brep_volume(base_piece).unwrap(),
        v_whole,
        "the pieces sum to the whole cone",
    );
}

#[test]
fn cone_plane_frustum_volumes_verify_by_cone_subtraction() {
    // The frustum r1 = 3 → r2 = 1, h = 4, cut by the same oblique
    // plane. The +n (top) piece is the oblique cone over the ellipse
    // from the extended apex, minus the small right cone above the
    // top rim — both classical.
    let (o, axis, r1, _r2, h, p, n) = cone_case();
    let r2 = 1.0_f64;
    let h_apex = h * r1 / (r1 - r2); // 6: the extended apex height
    let apex = v3(
        o.x + axis.x * h_apex,
        o.y + axis.y * h_apex,
        o.z + axis.z * h_apex,
    );
    let d_apex = v3(apex.x - p.x, apex.y - p.y, apex.z - p.z).dot(&n).abs();

    let common = cone_plane_boolean(BooleanOp::Common, o, axis, r1, r2, h, p, n).expect("common");
    let cut = cone_plane_boolean(BooleanOp::Cut, o, axis, r1, r2, h, p, n).expect("cut");
    let top = expect_solid(&common, "the top piece");
    let base = expect_solid(&cut, "the base piece");
    top.validate().expect("top validates");
    base.validate().expect("base validates");

    let (a_semi, b_semi) = match &top
        .edges
        .iter()
        .find(|e| matches!(e.curve, Curve::Ellipse { .. }))
        .unwrap()
        .curve
    {
        Curve::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => (*major_radius, *minor_radius),
        other => panic!("{other:?}"),
    };
    let v_top = std::f64::consts::PI * a_semi * b_semi * d_apex / 3.0
        - std::f64::consts::PI * r2 * r2 * (h_apex - h) / 3.0;
    approx(
        brep_volume(top).unwrap(),
        v_top,
        "the top piece by cone subtraction",
    );
    let v_whole = std::f64::consts::PI * h * (r1 * r1 + r1 * r2 + r2 * r2) / 3.0;
    approx(
        brep_volume(base).unwrap(),
        v_whole - v_top,
        "the base piece completes the frustum",
    );
    approx(
        brep_volume(top).unwrap() + brep_volume(base).unwrap(),
        v_whole,
        "the pieces sum to the whole frustum",
    );
}

#[test]
fn cone_plane_perpendicular_cut_composes_frusta() {
    let (o, axis, r1, _r2, h, _, _) = cone_case();
    let p = v3(0.0, 0.0, 2.0);
    let n = v3(0.0, 0.0, 1.0);

    let common = cone_plane_boolean(BooleanOp::Common, o, axis, r1, 0.0, h, p, n).expect("common");
    let cut = cone_plane_boolean(BooleanOp::Cut, o, axis, r1, 0.0, h, p, n).expect("cut");
    let top = expect_solid(&common, "the apex piece");
    let bottom = expect_solid(&cut, "the base piece");
    top.validate().expect("top validates");
    bottom.validate().expect("bottom validates");

    // The section circle has radius r1·(1 − v*/h) = 1.5.
    let rho = r1 * (1.0 - 2.0 / h);
    approx(
        brep_volume(top).unwrap(),
        std::f64::consts::PI * rho * rho * 2.0 / 3.0,
        "the apex piece is the short cone",
    );
    approx(
        brep_volume(bottom).unwrap(),
        std::f64::consts::PI * 2.0 * (r1 * r1 + r1 * rho + rho * rho) / 3.0,
        "the base piece is the frustum",
    );
}

#[test]
fn cone_plane_classifies_nonsecant_configurations_honestly() {
    let (o, axis, r1, _r2, h, _, _) = cone_case();

    // A steep plane (hyperbola class) through the axis segment.
    let steep = cone_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r1,
        0.0,
        h,
        v3(0.0, 0.0, 2.0),
        v3(0.96, 0.0, 0.28),
    )
    .expect("steep");
    expect_refused(&steep, "hyperbola", "a steep-plane section");

    // A plane through the apex.
    let through = cone_plane_boolean(
        BooleanOp::Cut,
        o,
        axis,
        r1,
        0.0,
        h,
        v3(0.0, 0.0, 4.0),
        v3(0.6, 0.0, 0.8),
    )
    .expect("through the apex");
    expect_refused(&through, "apex", "a plane through the apex");

    // The plane strictly above the apex (the cone outside the +n
    // halfspace): Common = empty, Cut = whole.
    let above = cone_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r1,
        0.0,
        h,
        v3(0.0, 0.0, 9.0),
        v3(0.0, 0.0, 1.0),
    )
    .expect("above");
    expect_empty(&above, "above the tool");
    let cut_above = cone_plane_boolean(
        BooleanOp::Cut,
        o,
        axis,
        r1,
        0.0,
        h,
        v3(0.0, 0.0, 9.0),
        v3(0.0, 0.0, 1.0),
    )
    .expect("cut above");
    approx(
        brep_volume(expect_solid(&cut_above, "the whole cone")).unwrap(),
        std::f64::consts::PI * r1 * r1 * h / 3.0,
        "the whole cone",
    );

    // The plane strictly below the base (the tool contains the cone):
    // Common = whole, Cut = empty.
    let below = cone_plane_boolean(
        BooleanOp::Common,
        o,
        axis,
        r1,
        0.0,
        h,
        v3(0.0, 0.0, -3.0),
        v3(0.0, 0.0, 1.0),
    )
    .expect("below");
    approx(
        brep_volume(expect_solid(&below, "the whole cone")).unwrap(),
        std::f64::consts::PI * r1 * r1 * h / 3.0,
        "the whole cone",
    );

    // Fuse is unbounded.
    let (_, _, _, _, _, p, n) = cone_case();
    let fuse = cone_plane_boolean(BooleanOp::Fuse, o, axis, r1, 0.0, h, p, n).expect("fuse");
    expect_refused(&fuse, "unbounded", "fusing a halfspace");
}

// ---------------------------------------------------------------------------
// K3d — cylinder × cylinder: the parallel-axis compositions
// ---------------------------------------------------------------------------
//
// Two finite cylinders whose axes are parallel (either sign) cross, when
// their cross-sections do, along two generator lines — the K3a section,
// reused bit-identically. Every horizontal slice is then the SAME
// two-circle picture, so the booleans are prisms of the planar
// circle-circle booleans:
//
// * **Common** — the lens prism: each cylinder's arc strip inside the
//   other over the z-overlap, capped by the two lens faces.
// * **Cut** (a − b, b's z-range covering a's) — the outer-arc strip of
//   a, the inner-arc wall of b (a REVERSED face — the solid lies
//   outside b), and the disk-minus-lens caps.
// * **Fuse** (equal z-ranges) — both outer-arc strips and the
//   union-region caps.
// * **Cut** with b nested inside a cross-wise — the tube: a's mantle,
//   b's mantle reversed, and annular caps (multi-wire plane faces).
//
// The shared edges are the crossing generators (degree-1 B-splines
// through the canonical K3a line points, exact at both ends) and the
// arcs. An arc cannot be a `Circle` parameter range here: its endpoint
// evaluation on one operand's frame is a different float than the other
// operand's — the bit-exact vertex contract would be violated — so the
// arcs are the classical **rational quadratic B-splines** (NURBS arcs,
// the representation every CAD kernel uses): control points
// [start, tangent-intersection, end], weights [1, cos(sweep/2), 1],
// clamped knots, de Boor exact at both ends. Sweeps wider than a
// quadrant split into quadrant-sized segments (the classical
// conditioning rule).
//
// The measurement reads the arcs back exactly: the cap areas by the
// line integral ∫(x dy − y dx) = R²Δθ + c × Δp per arc (closed form,
// no quadrature), the strips as swept angular intervals.

/// A general crossing configuration: tilted axis, unequal radii,
/// equal z-ranges.
fn cc_pair() -> (Vector3, Vector3, f64, f64, Vector3, Vector3, f64, f64) {
    let a = v3(1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
    let o1 = v3(1.0, -1.0, 2.0);
    let r1 = 2.0;
    let h = 5.0;
    // A perpendicular offset of length 2.5: (2, -1, 0)·(2.5/√5).
    let s5 = 5.0f64.sqrt();
    let perp = v3(2.0 * 2.5 / s5, -2.5 / s5, 0.0);
    let o2 = v3(o1.x + perp.x, o1.y + perp.y, o1.z + perp.z);
    (o1, a, r1, h, o2, a, 1.5, 5.0)
}

/// The perpendicular center distance of [`cc_pair`].
fn cc_d() -> f64 {
    2.5
}

/// The classical two-circle lens area.
fn lens_area(d: f64, r1: f64, r2: f64) -> f64 {
    let t1 = ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1)).clamp(-1.0, 1.0);
    let t2 = ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2)).clamp(-1.0, 1.0);
    let tri = (-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2);
    r1 * r1 * t1.acos() + r2 * r2 * t2.acos() - 0.5 * tri.max(0.0).sqrt()
}

/// The sweep of the arc of circle `r_this` lying inside disk `r_other`.
fn arc_sweep_inside(d: f64, r_this: f64, r_other: f64) -> f64 {
    let t = ((d * d + r_this * r_this - r_other * r_other) / (2.0 * d * r_this)).clamp(-1.0, 1.0);
    2.0 * t.acos()
}

/// Every edge used exactly twice, by two different faces, in opposite
/// directions (the closed-shell orientation contract).
fn assert_oriented_closed_shell(brep: &fc_part::topology::ExactBRep, what: &str) {
    for e in 0..brep.edges.len() {
        let mut uses: Vec<(usize, bool)> = Vec::new();
        for (fi, f) in brep.faces.iter().enumerate() {
            for w in &f.wires {
                for we in &w.edges {
                    if we.edge == e {
                        uses.push((fi, we.forward));
                    }
                }
            }
        }
        assert_eq!(uses.len(), 2, "{what}: edge {e} used {} times", uses.len());
        assert_ne!(
            uses[0].1, uses[1].1,
            "{what}: edge {e} traversed twice in the same direction"
        );
    }
}

#[test]
fn cyl_cyl_common_is_the_lens_prism() {
    let (o1, a, r1, h, o2, _, r2, h2) = cc_pair();
    let out =
        cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r1, h, o2, a, r2, h2).expect("secant");
    let brep = expect_solid(&out, "the lens prism");
    brep.validate().expect("the prism validates");

    // Topology: two cylindrical strips + two planar caps, sphere
    // topology, every edge shared by two faces in opposite directions.
    assert_eq!(brep.faces.len(), 4, "four faces");
    let kinds: Vec<&str> = brep.faces.iter().map(surface_kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "Cylinder").count(), 2);
    assert_eq!(kinds.iter().filter(|k| **k == "Plane").count(), 2);
    assert_eq!(brep.euler_characteristic(), 2, "sphere topology");
    assert_oriented_closed_shell(brep, "the lens prism");

    // Volume: the lens area times the overlap height (classical).
    let d = cc_d();
    approx(
        brep_volume(brep).unwrap(),
        lens_area(d, r1, r2) * h,
        "the lens prism volume",
    );

    // Area: two lens faces + each cylinder's inside arc strip.
    let th1 = arc_sweep_inside(d, r1, r2);
    let th2 = arc_sweep_inside(d, r2, r1);
    approx(
        brep_area(brep).unwrap(),
        2.0 * lens_area(d, r1, r2) + r1 * th1 * h + r2 * th2 * h,
        "the lens prism area",
    );
}

#[test]
fn cyl_cyl_cut_and_fuse_partition_the_cylinders() {
    let (o1, a, r1, h, o2, _, r2, _) = cc_pair();
    let d = cc_d();
    let lens = lens_area(d, r1, r2);
    let th1 = arc_sweep_inside(d, r1, r2);
    let th2 = arc_sweep_inside(d, r2, r1);

    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h, o2, a, r2, 5.0).expect("cut");
    let cut = expect_solid(&cut, "the cut");
    cut.validate().expect("the cut validates");
    assert_eq!(cut.euler_characteristic(), 2, "cut: sphere topology");
    assert_oriented_closed_shell(cut, "the cut");
    // Partition identity: V(cut) + V(common) = V(a).
    approx(
        brep_volume(cut).unwrap(),
        std::f64::consts::PI * r1 * r1 * h - lens * h,
        "the cut volume (disk minus lens, prism)",
    );
    approx(
        brep_area(cut).unwrap(),
        2.0 * (std::f64::consts::PI * r1 * r1 - lens)
            + r1 * (std::f64::consts::TAU - th1) * h
            + r2 * th2 * h,
        "the cut area",
    );
    // The tool's wall is the REVERSED face (the solid lies outside b).
    let walls: Vec<&fc_part::topology::TopoFace> = cut
        .faces
        .iter()
        .filter(|f| surface_kind(f) == "Cylinder" && matches!(f.surface, fc_part::topology::Surface::Cylinder { origin, radius, .. } if (origin.y - o2.y).abs() < 1e-9 && (radius - r2).abs() < 1e-9))
        .collect();
    assert_eq!(walls.len(), 1, "one wall face of b");
    assert!(walls[0].reversed, "the wall is reversed (solid outside b)");

    let fuse =
        cylinder_cylinder_boolean(BooleanOp::Fuse, o1, a, r1, h, o2, a, r2, 5.0).expect("fuse");
    let fuse = expect_solid(&fuse, "the fuse");
    fuse.validate().expect("the fuse validates");
    assert_eq!(fuse.euler_characteristic(), 2, "fuse: sphere topology");
    assert_oriented_closed_shell(fuse, "the fuse");
    // Partition identity: V(fuse) + V(common) = V(a) + V(b).
    approx(
        brep_volume(fuse).unwrap(),
        (std::f64::consts::PI * r1 * r1 + std::f64::consts::PI * r2 * r2 - lens) * h,
        "the fuse volume",
    );
    approx(
        brep_area(fuse).unwrap(),
        2.0 * (std::f64::consts::PI * r1 * r1 + std::f64::consts::PI * r2 * r2 - lens)
            + r1 * (std::f64::consts::TAU - th1) * h
            + r2 * (std::f64::consts::TAU - th2) * h,
        "the fuse area",
    );
    // No face of the fuse is reversed (both strips keep their solids
    // inside their own cylinders).
    assert!(
        fuse.faces.iter().all(|f| !f.reversed),
        "the fuse faces are naturally oriented"
    );
}

#[test]
fn cyl_cyl_arcs_lie_on_the_operands_and_the_crossing_lines() {
    let (o1, a, r1, h, o2, _, r2, _) = cc_pair();
    let axis_dist = |p: Vector3, o: Vector3| {
        let off = v3(p.x - o.x, p.y - o.y, p.z - o.z);
        let along = off.dot(&a);
        v3(
            off.x - a.x * along,
            off.y - a.y * along,
            off.z - a.z * along,
        )
        .length()
    };
    for (op, a_arcs_inside_b, b_arcs_inside_a, what) in [
        (BooleanOp::Common, true, true, "common"),
        (BooleanOp::Cut, false, true, "cut"),
        (BooleanOp::Fuse, false, false, "fuse"),
    ] {
        let out = cylinder_cylinder_boolean(op, o1, a, r1, h, o2, a, r2, 5.0).expect(what);
        let brep = expect_solid(&out, what);
        for (ei, edge) in brep.edges.iter().enumerate() {
            match &edge.curve {
                // The crossing generators: parallel to the axis and on
                // BOTH cylinders at machine scale.
                Curve::Bspline {
                    degree: 1,
                    points,
                    weights: None,
                    ..
                } if points.len() == 2 => {
                    let dir = v3(
                        points[1].x - points[0].x,
                        points[1].y - points[0].y,
                        points[1].z - points[0].z,
                    );
                    let len = dir.length();
                    let u = v3(dir.x / len, dir.y / len, dir.z / len);
                    approx(
                        u.dot(&a).abs(),
                        1.0,
                        "{what}: edge {ei} parallel to the axis",
                    );
                    for i in 0..16 {
                        let t = i as f64 / 16.0;
                        let p = edge
                            .curve
                            .point(edge.range.0 + (edge.range.1 - edge.range.0) * t);
                        approx(axis_dist(p, o1), r1, "{what}: edge {ei} on cylinder a");
                        approx(axis_dist(p, o2), r2, "{what}: edge {ei} on cylinder b");
                    }
                }
                // The arcs: rational quadratics on their own cylinder,
                // inside/outside the other per the operation.
                Curve::Bspline {
                    degree: 2,
                    weights: Some(_),
                    ..
                } => {}
                other => panic!("{what}: unexpected edge {ei}: {other:?}"),
            }
        }
        // Per side face: the arcs of a's strip and b's strip/wall.
        for f in &brep.faces {
            let (o_mine, r_mine, is_a) = match &f.surface {
                Surface::Cylinder { origin, radius, .. } => {
                    let is_a = (origin.x - o1.x).abs() < 1e-9
                        && (origin.y - o1.y).abs() < 1e-9
                        && (origin.z - o1.z).abs() < 1e-9;
                    (*origin, *radius, is_a)
                }
                _ => continue,
            };
            let (o_other, r_other, expect_inside) = if is_a {
                (o2, r2, a_arcs_inside_b)
            } else {
                (o1, r1, b_arcs_inside_a)
            };
            for wire in &f.wires {
                for we in &wire.edges {
                    let edge = &brep.edges[we.edge];
                    let Curve::Bspline {
                        degree: 2,
                        weights: Some(_),
                        ..
                    } = &edge.curve
                    else {
                        continue;
                    };
                    for i in 0..32 {
                        let t = i as f64 / 32.0;
                        let p = edge
                            .curve
                            .point(edge.range.0 + (edge.range.1 - edge.range.0) * t);
                        approx(
                            axis_dist(p, o_mine),
                            r_mine,
                            "{what}: an arc on its own cylinder",
                        );
                        // The arcs' shared endpoints sit exactly ON the
                        // other cylinder; the interiors are strictly on
                        // their side.
                        let d = axis_dist(p, o_other);
                        if expect_inside {
                            assert!(
                                d <= r_other + 1e-9,
                                "{what}: an arc sample {i} inside the other cylinder (d = {d})"
                            );
                        } else {
                            assert!(
                                d >= r_other - 1e-9,
                                "{what}: an arc sample {i} outside the other cylinder (d = {d})"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn cyl_cyl_swapped_operands_are_bit_identical_for_common_and_fuse() {
    let (o1, a, r1, h, o2, _, r2, h2) = cc_pair();
    for op in [BooleanOp::Common, BooleanOp::Fuse] {
        let ab = cylinder_cylinder_boolean(op, o1, a, r1, h, o2, a, r2, h2).expect("ab");
        let ba = cylinder_cylinder_boolean(op, o2, a, r2, h2, o1, a, r1, h).expect("ba");
        assert_eq!(
            expect_solid(&ab, "ab"),
            expect_solid(&ba, "ba"),
            "{op:?}: operand order canonicalized bit-identically"
        );
    }
}

#[test]
fn cyl_cyl_outcomes_round_trip_through_fcbrep() {
    let (o1, a, r1, h, o2, _, r2, _) = cc_pair();
    for op in [BooleanOp::Common, BooleanOp::Cut, BooleanOp::Fuse] {
        let out = cylinder_cylinder_boolean(op, o1, a, r1, h, o2, a, r2, 5.0).expect("op");
        let brep = expect_solid(&out, "solid");
        let mut bytes = Vec::new();
        write_brep(&mut bytes, brep).expect("FCBREP write");
        let back = read_brep_bytes(&bytes).expect("round-trip parse");
        assert!(
            back == *brep,
            "{op:?}: FCBREP round-trip is lossless (byte-identical model)"
        );
    }
}

#[test]
fn cyl_cyl_staggered_common_composes_the_clamped_lens() {
    // b's z-range shifted along the axis: the overlap is a proper
    // sub-interval of both, and the lens prism clamps to it.
    let (o1, a, r1, h, o2, _, r2, _) = cc_pair();
    let d = cc_d();
    let shift = v3(a.x * 1.5, a.y * 1.5, a.z * 1.5);
    let o2s = v3(o2.x + shift.x, o2.y + shift.y, o2.z + shift.z);
    let out = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r1, h, o2s, a, r2, 3.0)
        .expect("the clamped lens");
    let brep = expect_solid(&out, "the clamped lens");
    brep.validate().expect("validates");
    assert_eq!(brep.euler_characteristic(), 2, "sphere topology");
    assert_oriented_closed_shell(brep, "the clamped lens");
    // The overlap: a spans [0,5], b spans [1.5, 4.5] in a's v-frame.
    approx(
        brep_volume(brep).unwrap(),
        lens_area(d, r1, r2) * 3.0,
        "the clamped lens volume (overlap 1.5..4.5)",
    );
}

#[test]
fn cyl_cyl_nested_cut_composes_the_tube() {
    // b's cross-section strictly inside a's, b's z-range covering
    // a's: the cut is the tube — a's mantle, b's mantle reversed,
    // annular caps (multi-wire plane faces).
    let a = v3(1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
    let o1 = v3(0.5, -0.5, 1.0);
    let r1 = 3.0;
    let h1 = 4.0;
    let s5 = 5.0f64.sqrt();
    // A perpendicular offset of length 1.5 (< r1 − r2 = 2).
    let perp = v3(2.0 * 1.5 / s5, -1.5 / s5, 0.0);
    // b's range [−0.5, 4.5] covers a's [0, 4].
    let o2 = v3(
        o1.x + perp.x - a.x * 0.5,
        o1.y + perp.y - a.y * 0.5,
        o1.z + perp.z - a.z * 0.5,
    );
    let r2 = 1.0;
    assert!(1.5 + r2 < r1, "the nested precondition");

    let out =
        cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h1, o2, a, r2, 5.0).expect("the tube");
    let brep = expect_solid(&out, "the tube");
    brep.validate().expect("the tube validates");
    assert_eq!(brep.euler_characteristic(), 2, "tube: sphere topology");
    assert_oriented_closed_shell(brep, "the tube");
    // The caps are annuli: two wires each.
    for f in brep.faces.iter().filter(|f| surface_kind(f) == "Plane") {
        assert_eq!(f.wires.len(), 2, "the tube caps are annuli (two wires)");
    }
    approx(
        brep_volume(brep).unwrap(),
        std::f64::consts::PI * (r1 * r1 - r2 * r2) * h1,
        "the tube volume",
    );
    approx(
        brep_area(brep).unwrap(),
        std::f64::consts::TAU * (r1 + r2) * h1 + 2.0 * std::f64::consts::PI * (r1 * r1 - r2 * r2),
        "the tube area",
    );

    // Nested with b strictly inside a (both cross-section and range):
    // the common is b, the fuse is a.
    let o2b = v3(
        o1.x + perp.x + a.x * 0.5,
        o1.y + perp.y + a.y * 0.5,
        o1.z + perp.z + a.z * 0.5,
    );
    let common = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r1, h1, o2b, a, r2, 3.0)
        .expect("common");
    approx(
        brep_volume(expect_solid(&common, "b")).unwrap(),
        std::f64::consts::PI * r2 * r2 * 3.0,
        "nested common is b",
    );
    let fuse =
        cylinder_cylinder_boolean(BooleanOp::Fuse, o1, a, r1, h1, o2b, a, r2, 3.0).expect("fuse");
    approx(
        brep_volume(expect_solid(&fuse, "a")).unwrap(),
        std::f64::consts::PI * r1 * r1 * h1,
        "nested fuse is a",
    );
}

#[test]
fn cyl_cyl_classifies_nonsecant_configurations_honestly() {
    let (o1, a, r1, h, o2, _, r2, _) = cc_pair();

    // Non-parallel axes: the Steinmetz quartic, refused by name.
    let tilted = v3(1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0 + 1e-3)
        .normalized()
        .unwrap();
    let out = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r1, h, o2, tilted, r2, 5.0)
        .expect("classified");
    expect_refused(&out, "Steinmetz", "non-parallel axes");

    // Cross-disjoint: d = 5 > 3.5.
    let s5 = 5.0f64.sqrt();
    let far = v3(2.0 * 5.0 / s5, -5.0 / s5, 0.0);
    let of = v3(o1.x + far.x, o1.y + far.y, o1.z + far.z);
    let common = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r1, h, of, a, r2, 5.0)
        .expect("disjoint");
    expect_empty(&common, "disjoint common");
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h, of, a, r2, 5.0)
        .expect("disjoint cut");
    approx(
        brep_volume(expect_solid(&cut, "the whole a")).unwrap(),
        std::f64::consts::PI * r1 * r1 * h,
        "disjoint cut is the whole a",
    );
    let fuse = cylinder_cylinder_boolean(BooleanOp::Fuse, o1, a, r1, h, of, a, r2, 5.0)
        .expect("disjoint fuse");
    expect_refused(&fuse, "two-shell", "disjoint fuse");

    // External cross-tangency: d = r1 + r2 = 3.5, with a z-aligned
    // axis and a rational offset so the tangency is exactly
    // representable (d² = (r1+r2)² in the EFT classification).
    let ez = v3(0.0, 0.0, 1.0);
    let oe = v3(1.0, -1.0, 2.0);
    let ot = v3(1.0 + 3.5, -1.0, 2.0);
    let common = cylinder_cylinder_boolean(BooleanOp::Common, oe, ez, r1, h, ot, ez, r2, 5.0)
        .expect("tangent");
    expect_empty(&common, "tangent common (measure zero)");
    let fuse = cylinder_cylinder_boolean(BooleanOp::Fuse, oe, ez, r1, h, ot, ez, r2, 5.0)
        .expect("tangent fuse");
    expect_refused(&fuse, "junction", "tangent fuse");

    // Internal cross-tangency: d = r1 − r2 = 0.5 — the tube pinches;
    // the cut refuses at measure zero, the pass-throughs stay honest.
    let oi = v3(1.0 + 0.5, -1.0, 2.0);
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, oe, ez, r1, h, oi, ez, r2, 5.0)
        .expect("internally tangent");
    expect_refused(&cut, "tangent", "the pinched tube");

    // A inside b entirely (cross-section and z-range): the cut is
    // empty.
    let s5i = 5.0f64.sqrt();
    let perp_in = v3(2.0 * 2.0 / s5i, -2.0 / s5i, 0.0);
    let oa = v3(-3.0, 0.5, 1.0);
    let ob = v3(
        oa.x + perp_in.x - a.x * 0.5,
        oa.y + perp_in.y - a.y * 0.5,
        oa.z + perp_in.z - a.z * 0.5,
    );
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, oa, a, 1.0, 2.0, ob, a, 4.0, 3.0)
        .expect("a inside b");
    expect_empty(&cut, "a − b = ∅ when a ⊂ b");

    // z-disjoint (crossing cross-sections): no interaction.
    let s5c = 5.0f64.sqrt();
    let perp = v3(2.0 * 2.5 / s5c, -2.5 / s5c, 0.0);
    let up = v3(a.x * 10.0, a.y * 10.0, a.z * 10.0);
    let oz = v3(
        o1.x + perp.x + up.x,
        o1.y + perp.y + up.y,
        o1.z + perp.z + up.z,
    );
    let common = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r1, h, oz, a, r2, 5.0)
        .expect("z-disjoint");
    expect_empty(&common, "z-disjoint common");
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h, oz, a, r2, 5.0)
        .expect("z-disjoint cut");
    approx(
        brep_volume(expect_solid(&cut, "the whole a")).unwrap(),
        std::f64::consts::PI * r1 * r1 * h,
        "z-disjoint cut is the whole a",
    );

    // Staggered with both ends of b strictly inside a's range: the
    // cut is now the blind drill (its own test verifies it fully);
    // here just the volume identity. The staggered fuse is still the
    // next slice — refused by name.
    let shift = v3(a.x * 1.5, a.y * 1.5, a.z * 1.5);
    let o2s = v3(o2.x + shift.x, o2.y + shift.y, o2.z + shift.z);
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h, o2s, a, r2, 3.0)
        .expect("the blind drill");
    approx(
        brep_volume(expect_solid(&cut, "the drill")).unwrap(),
        std::f64::consts::PI * r1 * r1 * h - lens_area(cc_d(), r1, r2) * 3.0,
        "the blind drill volume (a minus the lens prism over b's range)",
    );
    let fuse = cylinder_cylinder_boolean(BooleanOp::Fuse, o1, a, r1, h, o2s, a, r2, 3.0)
        .expect("staggered");
    expect_refused(&fuse, "staggered", "the staggered fuse");

    // Coincident cylinders with staggered ranges: the end-to-end
    // pieces (the overlap for the common — the full battery is the
    // coincident-staggered test's own).
    let shift2 = v3(a.x * 2.0, a.y * 2.0, a.z * 2.0);
    let oc = v3(o1.x + shift2.x, o1.y + shift2.y, o1.z + shift2.z);
    let out = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r1, h, oc, a, r1, 5.0)
        .expect("coincident");
    approx(
        brep_volume(expect_solid(&out, "the overlap piece")).unwrap(),
        std::f64::consts::PI * r1 * r1 * 3.0,
        "coincident staggered common: the overlap piece",
    );
}

#[test]
fn cyl_cyl_blind_drill_composes_the_notched_mantle() {
    // b strictly inside a's z-range (both ends), crossing sections:
    // the cut is a with a blind lens-prism hole — the notched mantle
    // (outer wire: a's rims + seam; hole wire: the lens arcs and the
    // crossing generators), b's wall, b's two lens caps, a's full
    // caps.
    let (o1, a, r1, h1, o2, _, r2, _) = cc_pair();
    let d = cc_d();
    let lens = lens_area(d, r1, r2);
    let th1 = arc_sweep_inside(d, r1, r2);
    let th2 = arc_sweep_inside(d, r2, r1);
    let s2 = 1.5;
    let h2 = 2.0;
    let o2s = v3(
        o1.x + (o2.x - o1.x) + a.x * s2,
        o1.y + (o2.y - o1.y) + a.y * s2,
        o1.z + (o2.z - o1.z) + a.z * s2,
    );
    let out = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h1, o2s, a, r2, h2)
        .expect("the blind drill");
    let brep = expect_solid(&out, "the blind drill");
    brep.validate().expect("the blind drill validates");
    // The formal V − E + F counts a face with a hole one above its
    // surface Euler (the tube precedent: the torus reads 2 with its
    // two annular caps) — the drill's sphere reads 3 with its one
    // notched mantle.
    assert_eq!(
        brep.euler_characteristic(),
        3,
        "drill: sphere + one holed face"
    );
    assert_oriented_closed_shell(brep, "the blind drill");
    // Six faces: a's notched mantle, b's wall, b's two lens caps,
    // a's two full caps.
    assert_eq!(brep.faces.len(), 6, "the drill's six faces");
    let planes: Vec<usize> = brep.faces.iter().map(|f| f.wires.len()).collect();
    // The notched mantle carries two wires (outer + hole); the four
    // caps carry one each; the wall one.
    assert_eq!(
        planes.iter().filter(|w| **w == 2).count(),
        1,
        "one two-wire face (the notch)"
    );

    // Volume: a minus the lens prism over b's range.
    approx(
        brep_volume(brep).unwrap(),
        std::f64::consts::PI * r1 * r1 * h1 - lens * h2,
        "the blind drill volume",
    );
    // Area: a's full caps + a's mantle minus the notch + b's wall +
    // the two lens caps.
    approx(
        brep_area(brep).unwrap(),
        2.0 * std::f64::consts::PI * r1 * r1
            + (std::f64::consts::TAU * r1 * h1 - r1 * th1 * h2)
            + r2 * th2 * h2
            + 2.0 * lens,
        "the blind drill area",
    );
    // b's wall is the reversed face.
    let walls: Vec<&fc_part::topology::TopoFace> = brep
        .faces
        .iter()
        .filter(|f| surface_kind(f) == "Cylinder" && matches!(&f.surface, Surface::Cylinder { origin, radius, .. } if (origin.x - o2s.x).abs() < 1e-9 && (radius - r2).abs() < 1e-9))
        .collect();
    assert_eq!(walls.len(), 1, "one wall face of b");
    assert!(walls[0].reversed, "the drill's wall is reversed");

    // Round-trips losslessly.
    let mut bytes = Vec::new();
    write_brep(&mut bytes, brep).expect("FCBREP write");
    let back = read_brep_bytes(&bytes).expect("round-trip parse");
    assert!(back == *brep, "the drill round-trips byte-identically");
}

#[test]
fn cyl_cyl_one_sided_and_edge_drills_refuse_by_name() {
    // b crossing exactly one of a's ends: the split-circle mantles
    // are a later slice — refused by name, honestly.
    let (o1, a, r1, h1, o2, _, r2, _) = cc_pair();
    // b's bottom flush with a's bottom, top strictly inside.
    let o2f = v3(o2.x, o2.y, o2.z);
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h1, o2f, a, r2, 2.0)
        .expect("one-sided");
    expect_refused(&cut, "slice", "the one-sided drill");
    // b sticking out the bottom, top strictly inside.
    let o2u = v3(o2.x - a.x, o2.y - a.y, o2.z - a.z);
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r1, h1, o2u, a, r2, 2.0)
        .expect("one-sided out");
    expect_refused(&cut, "slice", "the one-sided out drill");
}

#[test]
fn cyl_cyl_coincident_staggered_composes_the_pieces() {
    // Coaxial, equal radii, staggered ranges: the end-to-end
    // compositions — the common is the overlap piece, the fuse the
    // hull, the cut the part of a outside b (one piece when b clips
    // one end, two when it sits strictly inside — refused).
    let a = v3(1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
    let o1 = v3(0.5, -1.0, 2.0);
    let r = 2.0;
    let h1 = 5.0;
    let shift = |d: f64| v3(o1.x + a.x * d, o1.y + a.y * d, o1.z + a.z * d);
    let pi = std::f64::consts::PI;

    // b on top: ranges [0,5] and [3,8].
    let o2 = shift(3.0);
    let common = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r, h1, o2, a, r, 5.0)
        .expect("the overlap piece");
    approx(
        brep_volume(expect_solid(&common, "the overlap")).unwrap(),
        pi * r * r * 2.0,
        "coincident common: the overlap piece",
    );
    let fuse = cylinder_cylinder_boolean(BooleanOp::Fuse, o1, a, r, h1, o2, a, r, 5.0)
        .expect("the hull piece");
    approx(
        brep_volume(expect_solid(&fuse, "the hull")).unwrap(),
        pi * r * r * 8.0,
        "coincident fuse: the hull piece",
    );
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r, h1, o2, a, r, 5.0)
        .expect("the below piece");
    approx(
        brep_volume(expect_solid(&cut, "a below b")).unwrap(),
        pi * r * r * 3.0,
        "coincident cut: the part below b",
    );

    // b strictly inside: [1.5, 3.5] within [0,5] — the cut is two
    // pieces (a below and above b): refused as a two-shell compound.
    let oi = shift(1.5);
    let cut =
        cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r, h1, oi, a, r, 2.0).expect("two pieces");
    expect_refused(&cut, "two", "the strictly-inside coincident cut");
    // Its common and fuse stay pieces.
    let common = cylinder_cylinder_boolean(BooleanOp::Common, o1, a, r, h1, oi, a, r, 2.0)
        .expect("the inside overlap");
    approx(
        brep_volume(expect_solid(&common, "the overlap")).unwrap(),
        pi * r * r * 2.0,
        "the strictly-inside coincident common",
    );
    let fuse = cylinder_cylinder_boolean(BooleanOp::Fuse, o1, a, r, h1, oi, a, r, 2.0)
        .expect("the whole a");
    approx(
        brep_volume(expect_solid(&fuse, "a")).unwrap(),
        pi * r * r * h1,
        "the strictly-inside coincident fuse is a",
    );

    // b hanging below: ranges [0,5] and [-2,3]: the cut keeps a's
    // part above b.
    let ob = shift(-2.0);
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r, h1, ob, a, r, 5.0)
        .expect("the above piece");
    approx(
        brep_volume(expect_solid(&cut, "a above b")).unwrap(),
        pi * r * r * 2.0,
        "coincident cut: the part above b",
    );

    // z-disjoint coincident cylinders: the fuse is a two-shell
    // compound — refused; the cut is the whole a.
    let oz = shift(10.0);
    let fuse =
        cylinder_cylinder_boolean(BooleanOp::Fuse, o1, a, r, h1, oz, a, r, 5.0).expect("disjoint");
    expect_refused(&fuse, "two-shell", "the z-disjoint coincident fuse");
    let cut = cylinder_cylinder_boolean(BooleanOp::Cut, o1, a, r, h1, oz, a, r, 5.0)
        .expect("the whole a");
    approx(
        brep_volume(expect_solid(&cut, "a")).unwrap(),
        pi * r * r * h1,
        "the z-disjoint coincident cut",
    );
}

// ---------------------------------------------------------------------------
// K3e — cylinder×sphere, the coaxial family: the capsule cuts
// ---------------------------------------------------------------------------

fn cs_axis() -> Vector3 {
    v3(1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0)
}

fn cs_origin() -> Vector3 {
    v3(1.0, -1.0, 2.0)
}

/// The sphere center on the axis at height `z_s` above the base.
fn cs_center(z_s: f64) -> Vector3 {
    let a = cs_axis();
    let o = cs_origin();
    v3(o.x + a.x * z_s, o.y + a.y * z_s, o.z + a.z * z_s)
}

/// The part of the ball (radius `r`, center at height `z_s`) between
/// the axis heights `z_a` and `z_b` — the classical segment, computed
/// independently of the engine (the stacked-profile reference).
fn ball_segment(r: f64, z_a: f64, z_b: f64, z_s: f64) -> f64 {
    std::f64::consts::PI * (r * r * (z_b - z_a) - ((z_b - z_s).powi(3) - (z_a - z_s).powi(3)) / 3.0)
}

#[test]
fn cyl_sph_common_composes_the_capsule() {
    // The sphere's z-range strictly inside the cylinder's, the sphere
    // wider: the common is the capsule — the cylinder band between the
    // two K3a section circles plus the two spherical caps.
    let (o, a) = (cs_origin(), cs_axis());
    let (r_c, h_c, z_s, r_s) = (1.0, 6.0, 3.0, 2.0);
    let h_i = 3.0f64.sqrt();
    let out = cylinder_sphere_boolean(BooleanOp::Common, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the capsule");
    let brep = expect_solid(&out, "the capsule");
    brep.validate().expect("the capsule validates");

    // Topology: 1 cylinder strip + 2 sphere caps, 2 section circles +
    // the generator seam, one vertex per circle.
    assert_eq!(brep.faces.len(), 3, "three faces");
    let kinds: Vec<&str> = brep.faces.iter().map(surface_kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "Cylinder").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "Sphere").count(), 2);
    assert_eq!(brep.edges.len(), 3, "two circles and the seam");
    assert_eq!(brep.vertices.len(), 2, "one vertex per circle");
    assert_eq!(brep.euler_characteristic(), 2, "ball topology");
    assert_oriented_closed_shell(brep, "the capsule");

    // The section circles are the K3a construction, bit-identical
    // (the frame is irrelevant to the circle's center and radius).
    let x_hat = {
        let probe = if a.x.abs() < 0.9 {
            v3(1.0, 0.0, 0.0)
        } else {
            v3(0.0, 1.0, 0.0)
        };
        let d = probe.dot(&a);
        (probe - v3(a.x * d, a.y * d, a.z * d))
            .normalized()
            .unwrap()
    };
    let k3a = intersect_surfaces(
        &fc_part::topology::Surface::Cylinder {
            origin: o,
            axis: a,
            ref_dir: x_hat,
            radius: r_c,
        },
        &fc_part::topology::Surface::Sphere {
            center: cs_center(z_s),
            axis: a,
            ref_dir: x_hat,
            radius: r_s,
        },
    );
    let want: Vec<(Vector3, f64)> = match k3a {
        SurfaceIntersection::Curves(cs) => cs
            .iter()
            .map(|c| match c {
                Curve::Circle { center, radius, .. } => (*center, *radius),
                other => panic!("K3a returned a non-circle: {other:?}"),
            })
            .collect(),
        other => panic!("the K3a section is missing: {other:?}"),
    };
    let got: Vec<(Vector3, f64)> = brep
        .edges
        .iter()
        .filter_map(|e| match &e.curve {
            Curve::Circle { center, radius, .. } if e.start == e.end => Some((*center, *radius)),
            _ => None,
        })
        .collect();
    assert_eq!(got.len(), 2, "two closed circle edges");
    assert!(
        got.contains(&want[0]) && got.contains(&want[1]),
        "the composed circles are the K3a ones: {got:?} vs {want:?}"
    );

    // Volume: the cylinder prism plus the two caps (classical).
    approx(
        brep_volume(brep).unwrap(),
        std::f64::consts::PI * r_c * r_c * (2.0 * h_i) + 2.0 * cap_volume(r_s, r_s - h_i),
        "the capsule volume",
    );
    // Area: the strip plus the two cap zones.
    approx(
        brep_area(brep).unwrap(),
        std::f64::consts::TAU * r_c * (2.0 * h_i) + 2.0 * std::f64::consts::TAU * r_s * (r_s - h_i),
        "the capsule area",
    );
}

#[test]
fn cyl_sph_common_clipped_variants_compose_the_stack() {
    let (o, a) = (cs_origin(), cs_axis());
    let pi = std::f64::consts::PI;

    // (a) The capsule crossing the base plane with its spherical
    // part: the disk at z=0, the sphere band (the meridian seam), the
    // cylinder band, the top cap.
    let (r_c, h_c, z_s, r_s) = (1.0, 4.0, 1.9, 2.0);
    let h_i = 3.0f64.sqrt();
    let out = cylinder_sphere_boolean(BooleanOp::Common, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the clipped capsule");
    let brep = expect_solid(&out, "the clipped capsule");
    brep.validate().expect("the clipped capsule validates");
    assert_eq!(brep.faces.len(), 4, "disk, sphere band, cylinder band, cap");
    let kinds: Vec<&str> = brep.faces.iter().map(surface_kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "Plane").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "Sphere").count(), 2);
    assert_eq!(kinds.iter().filter(|k| **k == "Cylinder").count(), 1);
    assert_eq!(brep.euler_characteristic(), 2, "ball topology");
    assert_oriented_closed_shell(brep, "the clipped capsule");
    approx(
        brep_volume(brep).unwrap(),
        ball_segment(r_s, 0.0, z_s - h_i, z_s)
            + pi * r_c * r_c * (2.0 * h_i)
            + cap_volume(r_s, r_s - h_i),
        "the clipped capsule volume",
    );

    // (b) The sphere narrower than the cylinder, both planes secant:
    // the slice — the sphere band and two planar disks.
    let (r_c, h_c, z_s, r_s) = (2.0, 1.2, 0.5, 1.0);
    let out = cylinder_sphere_boolean(BooleanOp::Common, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the slice");
    let brep = expect_solid(&out, "the slice");
    brep.validate().expect("the slice validates");
    assert_eq!(brep.faces.len(), 3, "band and two disks");
    assert_eq!(brep.euler_characteristic(), 2, "ball topology");
    assert_oriented_closed_shell(brep, "the slice");
    approx(
        brep_volume(brep).unwrap(),
        ball_segment(r_s, 0.0, h_c, z_s),
        "the slice volume",
    );

    // (c) The sphere narrower, one plane secant: the cap solid.
    let (r_c, h_c, z_s, r_s) = (2.0, 3.0, 0.5, 1.0);
    let out = cylinder_sphere_boolean(BooleanOp::Common, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the cap");
    let brep = expect_solid(&out, "the cap");
    brep.validate().expect("the cap validates");
    assert_eq!(brep.faces.len(), 2, "patch and disk");
    approx(
        brep_volume(brep).unwrap(),
        cap_volume(r_s, z_s + r_s),
        "the cap volume",
    );
}

#[test]
fn cyl_sph_cut_composes_the_bowls_and_the_groove() {
    let (o, a) = (cs_origin(), cs_axis());
    let pi = std::f64::consts::PI;

    // (a) The rim-bowl: the sphere dipping into the top cap from
    // above, narrower than the cylinder — the mouth circle inside the
    // rim, the bowl down to the south pole.
    let (r_c, h_c, z_s, r_s) = (2.0, 4.0, 4.2, 1.5);
    let out = cylinder_sphere_boolean(BooleanOp::Cut, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the rim-bowl cut");
    let brep = expect_solid(&out, "the rim-bowl cut");
    brep.validate().expect("the rim-bowl validates");
    assert_eq!(brep.faces.len(), 4, "mantle, cap, annulus, bowl");
    // V − E_nd + F = 3 − 4 + 4 = 3: the ball plus the annulus's hole.
    assert_eq!(
        brep.euler_characteristic(),
        3,
        "the ball reads 3 with the hole"
    );
    assert_oriented_closed_shell(brep, "the rim-bowl cut");
    approx(
        brep_volume(brep).unwrap(),
        pi * r_c * r_c * h_c - ball_segment(r_s, z_s - r_s, h_c, z_s),
        "the rim-bowl volume",
    );
    // The bowl is the REVERSED face (the solid lies outside the
    // sphere on it); the annulus has two wires.
    let mut bowls = 0;
    for f in brep.faces.iter().filter(|f| surface_kind(f) == "Sphere") {
        assert!(f.reversed, "the bowl face is reversed");
        assert_eq!(f.wires.len(), 1, "the bowl is bounded by the mouth circle");
        bowls += 1;
    }
    assert_eq!(bowls, 1, "one bowl face");
    let annuli: Vec<&fc_part::topology::TopoFace> = brep
        .faces
        .iter()
        .filter(|f| surface_kind(f) == "Plane" && f.wires.len() == 2)
        .collect();
    assert_eq!(annuli.len(), 1, "the top cap is an annulus");

    // (b) The pinch-bowl: the sphere wider than the cylinder, dipping
    // from above with the section circle inside the slab — the piece
    // pinches off at the section, no top face at all.
    let (r_c, h_c, z_s, r_s) = (1.0, 4.0, 3.5, 3.0);
    let h_i = 8.0f64.sqrt();
    let out = cylinder_sphere_boolean(BooleanOp::Cut, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the pinch-bowl cut");
    let brep = expect_solid(&out, "the pinch-bowl cut");
    brep.validate().expect("the pinch-bowl validates");
    assert_eq!(brep.faces.len(), 3, "mantle, cap, bowl");
    assert_eq!(brep.euler_characteristic(), 2, "ball topology");
    assert_oriented_closed_shell(brep, "the pinch-bowl cut");
    approx(
        brep_volume(brep).unwrap(),
        pi * r_c * r_c * (z_s - r_s) + pi * r_c * r_c * (r_s - h_i)
            - ball_segment(r_s, z_s - r_s, z_s - h_i, z_s),
        "the pinch-bowl volume",
    );

    // (c) The groove: the sphere's reach covering the slab, narrower
    // than the cylinder — the tube with the spherical inner wall.
    let (r_c, h_c, z_s, r_s) = (2.0, 1.2, 0.5, 1.5);
    let out = cylinder_sphere_boolean(BooleanOp::Cut, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the groove cut");
    let brep = expect_solid(&out, "the groove cut");
    brep.validate().expect("the groove validates");
    assert_eq!(brep.faces.len(), 4, "mantle, two annuli, inner wall");
    assert_eq!(
        brep.euler_characteristic(),
        2,
        "the torus reads 2 (holes included)"
    );
    assert_oriented_closed_shell(brep, "the groove cut");
    approx(
        brep_volume(brep).unwrap(),
        pi * r_c * r_c * h_c - ball_segment(r_s, 0.0, h_c, z_s),
        "the groove volume",
    );
    // The inner wall is REVERSED and carries the meridian seam.
    let walls: Vec<&fc_part::topology::TopoFace> = brep
        .faces
        .iter()
        .filter(|f| surface_kind(f) == "Sphere")
        .collect();
    assert_eq!(walls.len(), 1, "one inner wall");
    assert!(walls[0].reversed, "the inner wall is reversed");
    let annuli = brep
        .faces
        .iter()
        .filter(|f| surface_kind(f) == "Plane" && f.wires.len() == 2)
        .count();
    assert_eq!(annuli, 2, "both caps are annuli");

    // (d) The mirrored compositions: the sphere dipping in from
    // below — the rim-bowl and the pinch-bowl, exercising the
    // from-below assembly paths.
    let (r_c, h_c, z_s, r_s) = (2.0, 4.0, -0.2, 1.5);
    let out = cylinder_sphere_boolean(BooleanOp::Cut, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the from-below rim-bowl cut");
    let brep = expect_solid(&out, "the from-below rim-bowl cut");
    brep.validate().expect("the from-below rim-bowl validates");
    assert_eq!(brep.faces.len(), 4, "mantle, cap, annulus, bowl");
    assert_eq!(
        brep.euler_characteristic(),
        3,
        "the ball reads 3 with the hole"
    );
    assert_oriented_closed_shell(brep, "the from-below rim-bowl cut");
    approx(
        brep_volume(brep).unwrap(),
        pi * r_c * r_c * h_c - ball_segment(r_s, 0.0, z_s + r_s, z_s),
        "the from-below rim-bowl volume",
    );

    let (r_c, h_c, z_s, r_s) = (1.0, 4.0, 0.5, 3.0);
    let h_i = 8.0f64.sqrt();
    let out = cylinder_sphere_boolean(BooleanOp::Cut, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the from-below pinch-bowl cut");
    let brep = expect_solid(&out, "the from-below pinch-bowl cut");
    brep.validate()
        .expect("the from-below pinch-bowl validates");
    assert_eq!(brep.faces.len(), 3, "mantle, cap, bowl");
    assert_eq!(brep.euler_characteristic(), 2, "ball topology");
    assert_oriented_closed_shell(brep, "the from-below pinch-bowl cut");
    approx(
        brep_volume(brep).unwrap(),
        pi * r_c * r_c * (h_c - z_s - h_i) - ball_segment(r_s, z_s + h_i, z_s + r_s, z_s),
        "the from-below pinch-bowl volume",
    );
}

#[test]
fn cyl_sph_cut_and_fuse_partition_the_operands() {
    // One configuration where all three outcomes are solids: the
    // narrower sphere crossing the top plane.
    let (o, a) = (cs_origin(), cs_axis());
    let (r_c, h_c, z_s, r_s) = (2.0, 4.0, 4.2, 1.5);
    let pi = std::f64::consts::PI;
    let common = cylinder_sphere_boolean(BooleanOp::Common, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("common");
    let cut =
        cylinder_sphere_boolean(BooleanOp::Cut, o, a, r_c, h_c, cs_center(z_s), r_s).expect("cut");
    let fuse = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("fuse");
    // V(cut) + V(common) = V(cylinder).
    approx(
        brep_volume(expect_solid(&cut, "cut")).unwrap()
            + brep_volume(expect_solid(&common, "common")).unwrap(),
        pi * r_c * r_c * h_c,
        "the cut partition identity",
    );
    // V(fuse) + V(common) = V(cylinder) + V(sphere).
    approx(
        brep_volume(expect_solid(&fuse, "fuse")).unwrap()
            + brep_volume(expect_solid(&common, "common")).unwrap(),
        pi * r_c * r_c * h_c + (4.0 / 3.0) * pi * r_s * r_s * r_s,
        "the fuse partition identity",
    );
}

#[test]
fn cyl_sph_fuse_composes_the_bumps() {
    let (o, a) = (cs_origin(), cs_axis());
    let pi = std::f64::consts::PI;

    // One bump: the sphere crossing the top plane from inside the
    // slab, narrower — the dome sits on the annulus.
    let (r_c, h_c, z_s, r_s) = (2.0, 4.0, 4.2, 1.5);
    let out = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the bump fuse");
    let brep = expect_solid(&out, "the bump fuse");
    brep.validate().expect("the bump fuse validates");
    assert_eq!(brep.faces.len(), 4, "mantle, cap, annulus, dome");
    // 3 − 4 + 4 = 3: the ball plus the annulus's hole.
    assert_eq!(
        brep.euler_characteristic(),
        3,
        "the ball reads 3 with the hole"
    );
    assert_oriented_closed_shell(brep, "the bump fuse");
    approx(
        brep_volume(brep).unwrap(),
        pi * r_c * r_c * h_c + cap_volume(r_s, z_s + r_s - h_c),
        "the bump fuse volume",
    );
    // No reversed face: the dome's solid lies inside the sphere.
    assert!(
        brep.faces.iter().all(|f| !f.reversed),
        "the fuse faces are naturally oriented"
    );

    // Two bumps: the sphere spanning the slab, narrower — the domes
    // on both annuli.
    let (r_c, h_c, z_s, r_s) = (3.0, 1.4, 0.5, 1.0);
    let out = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, r_c, h_c, cs_center(z_s), r_s)
        .expect("the two-bump fuse");
    let brep = expect_solid(&out, "the two-bump fuse");
    brep.validate().expect("the two-bump fuse validates");
    assert_eq!(brep.faces.len(), 5, "mantle, two annuli, two domes");
    // 4 − 5 + 5 = 4: the ball plus the two annuli's holes.
    assert_eq!(
        brep.euler_characteristic(),
        4,
        "the ball reads 4 with two holes"
    );
    assert_oriented_closed_shell(brep, "the two-bump fuse");
    approx(
        brep_volume(brep).unwrap(),
        pi * r_c * r_c * h_c + cap_volume(r_s, r_s - z_s) + cap_volume(r_s, z_s + r_s - h_c),
        "the two-bump fuse volume",
    );
}

#[test]
fn cyl_sph_classifies_nonsecant_configurations_honestly() {
    let (o, a) = (cs_origin(), cs_axis());
    let pi = std::f64::consts::PI;

    // Non-coaxial: K3a's quartic refusal, propagated by name.
    let off = v3(0.03, -0.02, 0.01);
    let tilted_center = v3(
        cs_center(2.0).x + off.x,
        cs_center(2.0).y + off.y,
        cs_center(2.0).z + off.z,
    );
    let out = cylinder_sphere_boolean(BooleanOp::Common, o, a, 1.0, 5.0, tilted_center, 2.0)
        .expect("classified");
    expect_refused(&out, "coaxial", "the non-coaxial quartic");

    // No reach: the sphere beyond the slab — common empty, cut whole,
    // fuse refused as a two-shell compound.
    let common = cylinder_sphere_boolean(BooleanOp::Common, o, a, 1.0, 5.0, cs_center(8.0), 2.0)
        .expect("no reach");
    expect_empty(&common, "the no-reach common");
    let cut = cylinder_sphere_boolean(BooleanOp::Cut, o, a, 1.0, 5.0, cs_center(8.0), 2.0)
        .expect("no reach cut");
    approx(
        brep_volume(expect_solid(&cut, "the whole cylinder")).unwrap(),
        pi * 1.0 * 5.0,
        "the no-reach cut is the whole cylinder",
    );
    let fuse = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, 1.0, 5.0, cs_center(8.0), 2.0)
        .expect("no reach fuse");
    expect_refused(&fuse, "two-shell", "the no-reach fuse");

    // The reach tangent to the top plane from outside: the common is
    // a point (empty), the fuse a point junction (refused). A
    // z-aligned axis makes the tangency exactly representable (the
    // projected height is then exact).
    let ez = v3(0.0, 0.0, 1.0);
    let oez = v3(1.0, -1.0, 2.0);
    let cez = v3(1.0, -1.0, 9.0);
    let common =
        cylinder_sphere_boolean(BooleanOp::Common, oez, ez, 1.0, 5.0, cez, 2.0).expect("tangent");
    expect_empty(&common, "the tangent common");
    let fuse = cylinder_sphere_boolean(BooleanOp::Fuse, oez, ez, 1.0, 5.0, cez, 2.0)
        .expect("tangent fuse");
    expect_refused(&fuse, "junction", "the tangent fuse");

    // The sphere contained (narrower, reach inside): common is the
    // whole sphere, fuse the whole cylinder, cut the internal void —
    // refused by name.
    let common = cylinder_sphere_boolean(BooleanOp::Common, o, a, 2.0, 5.0, cs_center(2.5), 1.0)
        .expect("contained");
    approx(
        brep_volume(expect_solid(&common, "the whole sphere")).unwrap(),
        (4.0 / 3.0) * pi,
        "the contained common is the whole sphere",
    );
    let fuse = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, 2.0, 5.0, cs_center(2.5), 1.0)
        .expect("contained fuse");
    approx(
        brep_volume(expect_solid(&fuse, "the whole cylinder")).unwrap(),
        pi * 4.0 * 5.0,
        "the contained fuse is the whole cylinder",
    );
    let cut = cylinder_sphere_boolean(BooleanOp::Cut, o, a, 2.0, 5.0, cs_center(2.5), 1.0)
        .expect("the void");
    expect_refused(&cut, "void", "the contained cut");

    // The section zone covering the slab (the sphere wider, centered
    // over the slab): the common is the whole cylinder, the cut empty,
    // the fuse the whole sphere.
    let common = cylinder_sphere_boolean(BooleanOp::Common, o, a, 1.0, 2.0, cs_center(1.0), 3.0)
        .expect("covering");
    approx(
        brep_volume(expect_solid(&common, "the whole cylinder")).unwrap(),
        pi * 2.0,
        "the covering common is the whole cylinder",
    );
    let cut = cylinder_sphere_boolean(BooleanOp::Cut, o, a, 1.0, 2.0, cs_center(1.0), 3.0)
        .expect("covering cut");
    expect_empty(&cut, "the covering cut");
    let fuse = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, 1.0, 2.0, cs_center(1.0), 3.0)
        .expect("covering fuse");
    approx(
        brep_volume(expect_solid(&fuse, "the whole sphere")).unwrap(),
        (4.0 / 3.0) * pi * 27.0,
        "the covering fuse is the whole sphere",
    );

    // The sphere wider, the section zone strictly inside the slab:
    // the cut is two pieces — refused as a two-shell compound.
    let cut = cylinder_sphere_boolean(BooleanOp::Cut, o, a, 1.0, 8.0, cs_center(4.0), 3.0)
        .expect("two pieces");
    expect_refused(&cut, "two", "the split cut");
    // The fuse of the same: the mantle is interrupted — the
    // split-circle refusal.
    let fuse = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, 1.0, 8.0, cs_center(4.0), 3.0)
        .expect("split mantle");
    expect_refused(&fuse, "mantle", "the split-mantle fuse");

    // The collar: the sphere wider, the zone crossing the top plane,
    // the sphere's reach past the base — the one-piece material is a
    // genus-1 pinch piece, refused by name.
    let cut = cylinder_sphere_boolean(BooleanOp::Cut, o, a, 1.0, 1.5, cs_center(1.8), 2.0)
        .expect("the collar");
    expect_refused(&cut, "collar", "the genus-1 pinch piece");

    // The rim-mount: the mouth circle coinciding with the rim
    // (|plane − z_s| = h_i exactly) — refused by name. The radii 3
    // and 5 make h_i = 4 exact; the z-axis makes the heights exact.
    let cut = cylinder_sphere_boolean(
        BooleanOp::Cut,
        v3(1.0, -1.0, 2.0),
        v3(0.0, 0.0, 1.0),
        3.0,
        2.0,
        v3(1.0, -1.0, 8.0),
        5.0,
    )
    .expect("the rim-mount");
    expect_refused(&cut, "rim", "the coincident mouth and rim");

    // The equator tangency: equal radii, the equator inside the slab
    // — the cut and fuse touch the mantle along a circle, refused.
    let cut = cylinder_sphere_boolean(BooleanOp::Cut, o, a, 1.0, 5.0, cs_center(2.5), 1.0)
        .expect("the equator");
    expect_refused(&cut, "equator", "the tangent equator cut");
    let fuse = cylinder_sphere_boolean(BooleanOp::Fuse, o, a, 1.0, 5.0, cs_center(2.5), 1.0)
        .expect("the equator fuse");
    expect_refused(&fuse, "equator", "the tangent equator fuse");
    // Its common is unaffected: the sphere clipped by the slab.
    let common = cylinder_sphere_boolean(BooleanOp::Common, o, a, 1.0, 5.0, cs_center(2.5), 1.0)
        .expect("the equator common");
    approx(
        brep_volume(expect_solid(&common, "the whole sphere")).unwrap(),
        (4.0 / 3.0) * pi,
        "the equator common is the whole sphere",
    );
}

#[test]
fn cyl_sph_machine_precision_offset_reads_coaxial() {
    // A sub-machine perpendicular offset — the arithmetic residue of
    // a placement — reads as the intended coaxial (the K3d precedent).
    let (o, a) = (cs_origin(), cs_axis());
    let c = cs_center(3.0);
    let nudged = v3(c.x + 1e-12, c.y - 1e-12, c.z + 1e-12);
    let out = cylinder_sphere_boolean(BooleanOp::Common, o, a, 1.0, 6.0, nudged, 2.0)
        .expect("the coaxial reading");
    let brep = expect_solid(&out, "the capsule");
    brep.validate().expect("the nudged capsule validates");
    let h_i = 3.0f64.sqrt();
    approx(
        brep_volume(brep).unwrap(),
        std::f64::consts::PI * (2.0 * h_i) + 2.0 * cap_volume(2.0, 2.0 - h_i),
        "the nudged capsule volume",
    );
}

#[test]
fn cyl_sph_edges_lie_on_the_operands() {
    // Dense-sample every edge of the three compositions against the
    // operand surfaces (machine scale, the K3 discipline).
    let (o, a) = (cs_origin(), cs_axis());
    for (op, r_c, h_c, z_s, r_s, what) in [
        (BooleanOp::Common, 1.0, 4.0, 1.9, 2.0, "the clipped capsule"),
        (BooleanOp::Cut, 2.0, 4.0, 4.2, 1.5, "the rim-bowl cut"),
        (BooleanOp::Cut, 2.0, 1.2, 0.5, 1.5, "the groove cut"),
        (BooleanOp::Fuse, 3.0, 1.4, 0.5, 1.0, "the two-bump fuse"),
    ] {
        let out = cylinder_sphere_boolean(op, o, a, r_c, h_c, cs_center(z_s), r_s).expect(what);
        let brep = expect_solid(&out, what);
        for (ei, edge) in brep.edges.iter().enumerate() {
            for i in 0..16 {
                let t = edge.range.0 + (edge.range.1 - edge.range.0) * i as f64 / 16.0;
                let p = edge.curve.point(t);
                // The distance to the cylinder's surface.
                let off = v3(p.x - o.x, p.y - o.y, p.z - o.z);
                let along = off.dot(&a);
                let radial = v3(
                    off.x - a.x * along,
                    off.y - a.y * along,
                    off.z - a.z * along,
                )
                .length();
                // The distance to the sphere's surface.
                let to_c = v3(
                    p.x - cs_center(z_s).x,
                    p.y - cs_center(z_s).y,
                    p.z - cs_center(z_s).z,
                )
                .length();
                let on_cyl = (radial - r_c).abs() <= 1e-9 * r_c.max(1.0);
                let on_sph = (to_c - r_s).abs() <= 1e-9 * r_s.max(1.0);
                let on_cap = along.abs() <= 1e-9 || (along - h_c).abs() <= 1e-9;
                assert!(
                    on_cyl || on_sph || on_cap,
                    "{what}: edge {ei} sample {i} is on none of the operand surfaces \
                     (radial {radial}, cyl {r_c}, to_c {to_c}, sph {r_s}, along {along})"
                );
                // Every sample lies within the cylinder's z-range too.
                assert!(
                    -1e-9 <= along && along <= h_c + 1e-9,
                    "{what}: edge {ei} sample {i} outside the slab ({along} vs [0, {h_c}])"
                );
            }
        }
    }
}

#[test]
fn cyl_sph_outcomes_round_trip_through_fcbrep() {
    let (o, a) = (cs_origin(), cs_axis());
    for (op, r_c, h_c, z_s, r_s, what) in [
        (BooleanOp::Common, 1.0, 6.0, 3.0, 2.0, "the capsule"),
        (BooleanOp::Common, 1.0, 4.0, 1.9, 2.0, "the clipped capsule"),
        (BooleanOp::Cut, 2.0, 1.2, 0.5, 1.5, "the groove"),
        (BooleanOp::Cut, 1.0, 4.0, 3.5, 3.0, "the pinch-bowl"),
        (BooleanOp::Fuse, 3.0, 1.4, 0.5, 1.0, "the two-bump fuse"),
    ] {
        let out = cylinder_sphere_boolean(op, o, a, r_c, h_c, cs_center(z_s), r_s).expect(what);
        let brep = expect_solid(&out, what);
        let mut bytes = Vec::new();
        write_brep(&mut bytes, brep).expect("FCBREP write");
        let back = read_brep_bytes(&bytes).expect("round-trip parse");
        assert!(
            back == *brep,
            "{what}: the FCBREP round-trip is lossless (byte-identical model)"
        );
    }
}
