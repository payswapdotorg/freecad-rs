//! Integration tests for `fc-base`: vectors, rotations, matrices,
//! placements and bounding boxes — the whole `Base` math vocabulary.
//!
//! All comparisons are tolerance-based (deterministic, no time or
//! order dependence).

use fc_base::{BoundBox3, Matrix4, Placement, Rotation, Vector3};

/// Tolerance for algebra round-trips on well-scaled values.
const TOL: f64 = 1e-12;
/// Tolerance for general rotation math (sin/cos/atan2 chains).
const ROT_TOL: f64 = 1e-12;

fn assert_vec(a: Vector3, b: Vector3, ctx: &str) {
    assert!(
        (a.x - b.x).abs() <= TOL && (a.y - b.y).abs() <= TOL && (a.z - b.z).abs() <= TOL,
        "{ctx}: {a:?} != {b:?} (tol {TOL})"
    );
}

fn assert_rot(r: &Rotation, s: &Rotation, ctx: &str) {
    // Compare through the rotation matrix so q and -q count as equal.
    let (m, n) = (r.to_matrix(), s.to_matrix());
    for i in 0..4 {
        for j in 0..4 {
            assert!(
                (m.m[i][j] - n.m[i][j]).abs() <= ROT_TOL,
                "{ctx}: rotation matrices differ at ({i},{j}): {} vs {}",
                m.m[i][j],
                n.m[i][j]
            );
        }
    }
}

fn assert_mat(a: &Matrix4, b: &Matrix4, ctx: &str) {
    for i in 0..4 {
        for j in 0..4 {
            assert!(
                (a.m[i][j] - b.m[i][j]).abs() <= 1e-10,
                "{ctx}: matrices differ at ({i},{j}): {} vs {}",
                a.m[i][j],
                b.m[i][j]
            );
        }
    }
}

fn assert_identity(m: &Matrix4, ctx: &str) {
    assert_mat(m, &Matrix4::IDENTITY, ctx);
}

// ---------------------------------------------------------------------------
// Vector3
// ---------------------------------------------------------------------------

#[test]
fn vector_add_sub_scale_negate() {
    let a = Vector3::new(1.0, -2.0, 3.0);
    let b = Vector3::new(-4.0, 5.0, 0.5);

    assert_vec(a + b, Vector3::new(-3.0, 3.0, 3.5), "add");
    assert_vec(a - b, Vector3::new(5.0, -7.0, 2.5), "sub");
    assert_vec(a * 2.5, Vector3::new(2.5, -5.0, 7.5), "scale");
    assert_vec(-a, Vector3::new(-1.0, 2.0, -3.0), "negate");
    assert_eq!(Vector3::ZERO, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(Vector3::X, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(Vector3::Y, Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(Vector3::Z, Vector3::new(0.0, 0.0, 1.0));

    // (a + b) - b == a: subtraction inverts addition.
    assert_vec((a + b) - b, a, "add-sub cancel");
}

#[test]
fn vector_dot_cross_and_length() {
    let a = Vector3::new(1.0, 2.0, 3.0);
    let b = Vector3::new(-2.0, 0.5, 4.0);

    assert!((a.dot(&b) - 11.0).abs() <= TOL, "dot value");
    // Commutative.
    assert!((a.dot(&b) - b.dot(&a)).abs() <= TOL, "dot symmetry");

    // Right-handed cross products of the basis.
    assert_vec(Vector3::X.cross(&Vector3::Y), Vector3::Z, "X x Y = Z");
    assert_vec(Vector3::Y.cross(&Vector3::Z), Vector3::X, "Y x Z = X");
    assert_vec(Vector3::Z.cross(&Vector3::X), Vector3::Y, "Z x X = Y");
    // Anti-commutative and self-annihilating.
    assert_vec(a.cross(&b), -(b.cross(&a)), "cross anti-commutative");
    assert_vec(a.cross(&a), Vector3::ZERO, "a x a = 0");
    // Lagrange identity: |a x b|^2 = |a|^2|b|^2 - (a.b)^2
    let lhs = a.cross(&b).length_squared();
    let rhs = a.length_squared() * b.length_squared() - a.dot(&b) * a.dot(&b);
    assert!((lhs - rhs).abs() <= 1e-12 * rhs.abs().max(1.0), "Lagrange");

    assert!((a.length() - (14.0f64).sqrt()).abs() <= TOL, "length");
    assert!((a.length_squared() - 14.0).abs() <= TOL, "length squared");
    let p = Vector3::new(0.0, 0.0, 3.0);
    let q = Vector3::new(0.0, 4.0, 0.0);
    assert!((Vector3::distance(&p, &q) - 5.0).abs() <= TOL, "distance");
}

#[test]
fn vector_normalized_and_zero_vector_behavior() {
    let a = Vector3::new(3.0, 0.0, 4.0);
    let n = a.normalized().expect("non-zero vector normalizes");
    assert_vec(n, Vector3::new(0.6, 0.0, 0.8), "normalize 3-4-5");
    assert!((n.length() - 1.0).abs() <= TOL, "unit length");

    // Direction preserved: normalized is parallel to the original.
    let b = Vector3::new(-1.25, 7.5, 0.5);
    let m = b.normalized().expect("non-zero vector normalizes");
    assert!(
        (m.dot(&b) - b.length()).abs() <= 1e-12,
        "parallel to original"
    );
    assert!(
        (b.cross(&m)).length() <= 1e-12 * b.length(),
        "no perpendicular component"
    );

    // Zero-vector normalize is an explicit `None` (documented deviation
    // from FreeCAD's silent no-op `Normalize`).
    assert_eq!(Vector3::ZERO.normalized(), None);
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

#[test]
fn rotation_from_axis_angle_quaternion_components() {
    let q = Rotation::around_z(std::f64::consts::FRAC_PI_2);
    let expect = std::f64::consts::FRAC_PI_4;
    assert!((q.x - 0.0).abs() <= TOL, "q.x");
    assert!((q.y - 0.0).abs() <= TOL, "q.y");
    assert!((q.z - expect.sin()).abs() <= TOL, "q.z = sin(theta/2)");
    assert!((q.w - expect.cos()).abs() <= TOL, "q.w = cos(theta/2)");

    // A non-unit axis is normalized internally: same rotation.
    let by_unit = Rotation::from_axis_angle(Vector3::Z, 0.7).expect("unit axis");
    let by_long = Rotation::from_axis_angle(Vector3::Z * 8.0, 0.7).expect("scaled axis");
    assert_rot(&by_unit, &by_long, "non-unit axis gives same rotation");

    // A zero axis is rejected.
    assert_eq!(
        Rotation::from_axis_angle(Vector3::ZERO, 0.7),
        Err(fc_base::DegenerateAxis)
    );
}

#[test]
fn rotation_rotates_basis_by_90_degrees() {
    // Pure 90-degree cases: the gimbal-trouble spots of Euler angles,
    // trivial for quaternions.
    let half_pi = std::f64::consts::FRAC_PI_2;
    let rz = Rotation::around_z(half_pi);
    assert_vec(rz.rotate_vector(&Vector3::X), Vector3::Y, "Rz90: X -> Y");
    assert_vec(rz.rotate_vector(&Vector3::Y), -Vector3::X, "Rz90: Y -> -X");
    assert_vec(rz.rotate_vector(&Vector3::Z), Vector3::Z, "Rz90: Z -> Z");

    let rx = Rotation::around_x(half_pi);
    assert_vec(rx.rotate_vector(&Vector3::Y), Vector3::Z, "Rx90: Y -> Z");
    assert_vec(rx.rotate_vector(&Vector3::Z), -Vector3::Y, "Rx90: Z -> -Y");
    assert_vec(rx.rotate_vector(&Vector3::X), Vector3::X, "Rx90: X -> X");

    let ry = Rotation::around_y(half_pi);
    assert_vec(ry.rotate_vector(&Vector3::Z), Vector3::X, "Ry90: Z -> X");
    assert_vec(ry.rotate_vector(&Vector3::X), -Vector3::Z, "Ry90: X -> -Z");
    assert_vec(ry.rotate_vector(&Vector3::Y), Vector3::Y, "Ry90: Y -> Y");

    // Compositions of 90-degree rotations (right-handed, active).
    // Rz90 then Rx90: X -> Y -> Z, Y -> -X -> -X, Z -> Z -> -Y.
    let rz_then_rx = rz.multiply(&rx);
    assert_vec(
        rz_then_rx.rotate_vector(&Vector3::X),
        Vector3::Z,
        "Rz90.Rx90: X -> Z",
    );
    assert_vec(
        rz_then_rx.rotate_vector(&Vector3::Z),
        -Vector3::Y,
        "Rz90.Rx90: Z -> -Y",
    );
    // Rx90 then Rz90 cycles the basis X -> Y -> Z -> X.
    let rx_then_rz = rx.multiply(&rz);
    assert_vec(
        rx_then_rz.rotate_vector(&Vector3::X),
        Vector3::Y,
        "Rx90.Rz90: X -> Y",
    );
    assert_vec(
        rx_then_rz.rotate_vector(&Vector3::Y),
        Vector3::Z,
        "Rx90.Rz90: Y -> Z",
    );
    assert_vec(
        rx_then_rz.rotate_vector(&Vector3::Z),
        Vector3::X,
        "Rx90.Rz90: Z -> X",
    );
}

#[test]
fn rotation_preserves_lengths_angles_and_cross_products() {
    let r = Rotation::from_axis_angle(Vector3::new(2.0, -3.0, 4.0), 0.823).expect("non-zero axis");
    let a = Vector3::new(1.5, -0.5, 2.0);
    let b = Vector3::new(-3.0, 0.25, 1.75);

    let ra = r.rotate_vector(&a);
    let rb = r.rotate_vector(&b);

    // Isometry: lengths preserved.
    assert!(
        (ra.length() - a.length()).abs() <= 1e-12,
        "length preserved"
    );
    // Inner product preserved.
    assert!(
        (ra.dot(&rb) - a.dot(&b)).abs() <= 1e-12,
        "dot product preserved"
    );
    // Cross product rotates with the vectors (rotation is an algebra
    // homomorphism).
    let rc = r.rotate_vector(&a.cross(&b));
    assert!(
        (rc - ra.cross(&rb)).length() <= 1e-12,
        "cross product rotates with operands"
    );
    // Twice the same rotation equals the doubled angle.
    let doubled =
        Rotation::from_axis_angle(Vector3::new(2.0, -3.0, 4.0), 1.646).expect("non-zero axis");
    assert_rot(&r.multiply(&r), &doubled, "r * r == rotate by 2*theta");
}

#[test]
fn rotation_inverse_round_trip() {
    let samples = [
        Rotation::IDENTITY,
        Rotation::around_z(std::f64::consts::FRAC_PI_2),
        Rotation::around_x(2.1),
        Rotation::around_y(-0.75),
        Rotation::from_axis_angle(Vector3::new(1.0, 1.0, 1.0), 1.234).expect("non-zero axis"),
    ];
    for r in &samples {
        let inv = r.inverse();
        assert_rot(&r.multiply(&inv), &Rotation::IDENTITY, "r * r^-1 = 1");
        assert_rot(&inv.multiply(r), &Rotation::IDENTITY, "r^-1 * r = 1");
        assert_rot(&inv.inverse(), r, "(r^-1)^-1 = r");

        // Rotating then un-rotating a vector restores it.
        let v = Vector3::new(0.3, -2.0, 5.5);
        assert_vec(
            inv.rotate_vector(&r.rotate_vector(&v)),
            v,
            "rotate + unrotate",
        );

        // Identity is a two-sided unit for composition.
        assert_rot(&r.multiply(&Rotation::IDENTITY), r, "r * 1 = r");
        assert_rot(&Rotation::IDENTITY.multiply(r), r, "1 * r = r");
    }
}

#[test]
fn rotation_composition_matches_sequential_application() {
    // `a.multiply(&b)` must mean: apply `a` first, then `b` — consistent
    // with `a.to_matrix().multiply(&b.to_matrix())` under the row-vector
    // convention.
    let a = Rotation::around_z(0.6);
    let b = Rotation::from_axis_angle(Vector3::new(1.0, -1.0, 0.5), 1.1).expect("non-zero axis");
    let composed = a.multiply(&b);
    let v = Vector3::new(2.0, 3.0, -4.0);

    let sequential = b.rotate_vector(&a.rotate_vector(&v));
    let via_quaternion = composed.rotate_vector(&v);
    assert!(
        (sequential - via_quaternion).length() <= 1e-12,
        "composition == sequential application"
    );

    let matrix_product = a.to_matrix().multiply(&b.to_matrix());
    let via_matrix = matrix_product.transform_direction(&v);
    assert!(
        (via_quaternion - via_matrix).length() <= 1e-12,
        "quaternion composition == matrix product"
    );
}

#[test]
fn rotation_matrix_agrees_with_rotate_vector() {
    let samples = [
        Rotation::around_x(0.9),
        Rotation::around_y(-2.2),
        Rotation::around_z(std::f64::consts::FRAC_PI_3),
        Rotation::from_axis_angle(Vector3::new(0.0, 4.0, -3.0), 2.7).expect("non-zero axis"),
    ];
    for r in &samples {
        let m = r.to_matrix();
        // The 3x3 block must be orthonormal (rigid).
        for (i, row) in m.m.iter().take(3).enumerate() {
            let mut norm = 0.0;
            for v in row.iter().take(3) {
                norm += v * v;
            }
            assert!((norm - 1.0).abs() <= 1e-12, "row {i} unit length");
        }
        // No translation in a pure rotation matrix.
        assert!(
            m.m[3][0] == 0.0 && m.m[3][1] == 0.0 && m.m[3][2] == 0.0 && m.m[3][3] == 1.0,
            "no translation"
        );
        for v in [
            Vector3::X,
            Vector3::Y,
            Vector3::Z,
            Vector3::new(1.0, -2.0, 0.5),
        ] {
            let rotated = r.rotate_vector(&v);
            let transformed = m.transform_direction(&v);
            assert!(
                (rotated - transformed).length() <= 1e-12,
                "to_matrix agrees with rotate_vector"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Matrix4
// ---------------------------------------------------------------------------

#[test]
fn matrix_multiply_and_row_vector_convention() {
    // Hand-composed case: translate by (1,2,3), then rotate 90 degrees
    // about Z. Row-vector convention: p' = p * M(t) * M(r).
    let t = Matrix4::from_translation(&Vector3::new(1.0, 2.0, 3.0));
    let r = Rotation::around_z(std::f64::consts::FRAC_PI_2).to_matrix();

    // Translation applies to points, not directions.
    assert_vec(
        t.transform_point(&Vector3::new(1.0, 1.0, 1.0)),
        Vector3::new(2.0, 3.0, 4.0),
        "translate point",
    );
    assert_vec(
        t.transform_direction(&Vector3::new(1.0, 1.0, 1.0)),
        Vector3::new(1.0, 1.0, 1.0),
        "translate leaves directions",
    );

    // (1,0,0) -> t -> (2,2,3) -> Rz90 -> (-2,2,3).
    let combined = t.multiply(&r);
    assert_vec(
        combined.transform_point(&Vector3::new(1.0, 0.0, 0.0)),
        Vector3::new(-2.0, 2.0, 3.0),
        "translate then rotate",
    );
    // Composition order: applying `t` first then `r` equals the product.
    let stepwise = r.transform_point(&t.transform_point(&Vector3::new(1.0, 0.0, 0.0)));
    assert_vec(stepwise, Vector3::new(-2.0, 2.0, 3.0), "stepwise");

    // The Mul operator is `multiply`.
    assert_mat(&(t * r), &combined, "operator * == multiply");

    // Identity is a two-sided unit.
    assert_mat(&Matrix4::IDENTITY.multiply(&t), &t, "1 * t");
    assert_mat(&t.multiply(&Matrix4::IDENTITY), &t, "t * 1");

    // Transpose is an involution.
    assert_mat(&t.transpose().transpose(), &t, "transpose twice");
}

#[test]
fn matrix_multiply_is_associative() {
    let a = Matrix4::from_translation(&Vector3::new(3.0, -1.0, 2.0));
    let b = Rotation::around_x(0.8).to_matrix();
    let mut c = Matrix4::IDENTITY;
    c.m[0][1] = 0.4; // a shear to make the product genuinely mixed
    c.m[2][0] = -0.2;

    let left = (a.multiply(&b)).multiply(&c);
    let right = a.multiply(&b.multiply(&c));
    assert_mat(&left, &right, "(A*B)*C == A*(B*C)");
}

#[test]
fn matrix_inverse_round_trip() {
    let rot = Rotation::from_axis_angle(Vector3::new(1.0, 2.0, 3.0), 0.9)
        .expect("non-zero axis")
        .to_matrix();
    let mut scale = Matrix4::IDENTITY;
    scale.m[0][0] = 2.0;
    scale.m[1][1] = 3.0;
    scale.m[2][2] = 4.0;
    let mut shear = Matrix4::IDENTITY;
    shear.m[0][1] = 0.5;
    shear.m[1][2] = 0.2;
    shear.m[2][0] = 0.3;

    let cases = [
        Matrix4::from_translation(&Vector3::new(5.0, -6.0, 7.0)),
        rot,
        scale,
        shear,
        rot.multiply(&Matrix4::from_translation(&Vector3::new(-2.0, 0.5, 9.0))),
    ];
    for (i, m) in cases.iter().enumerate() {
        let inv = m
            .invert()
            .unwrap_or_else(|| panic!("case {i}: expected invertible"));
        assert_identity(&m.multiply(&inv), "case {i}: M * M^-1");
        assert_identity(&inv.multiply(m), "case {i}: M^-1 * M");

        // The inverse undoes the transform on sample points.
        let p = Vector3::new(1.25, -3.5, 2.0);
        assert_vec(
            inv.transform_point(&m.transform_point(&p)),
            p,
            "case {i}: undo",
        );
    }

    // The inverse of a pure rotation is its transpose.
    assert_mat(
        &rot.invert().expect("rotation invertible"),
        &rot.transpose(),
        "rot^-1 = rot^T",
    );
}

#[test]
fn matrix_inverse_rejects_singular_matrices() {
    let zero = Matrix4 { m: [[0.0; 4]; 4] };
    assert_eq!(zero.invert(), None, "zero matrix");

    let mut duplicate_rows = Matrix4::IDENTITY;
    duplicate_rows.m[1] = duplicate_rows.m[0]; // rank 3
    assert_eq!(duplicate_rows.invert(), None, "duplicate rows");

    let mut collapsed = Matrix4::IDENTITY;
    collapsed.m[3][3] = 0.0; // bottom-right zero -> singular
    assert_eq!(collapsed.invert(), None, "zero diagonal");
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

#[test]
fn placement_compose_matches_matrix_product_and_hand_case() {
    // Hand case: P = translate (1,0,0) + Rz90; Q = translate (0,1,0) +
    // Rx90. Applying P then Q to the origin: (0,0,0) -> (1,0,0) ->
    // (1,1,0). To (1,0,0): -> (1,1,0) -> (1,1,1).
    let p = Placement::new(
        Vector3::new(1.0, 0.0, 0.0),
        Rotation::around_z(std::f64::consts::FRAC_PI_2),
    );
    let q = Placement::new(
        Vector3::new(0.0, 1.0, 0.0),
        Rotation::around_x(std::f64::consts::FRAC_PI_2),
    );
    let combined = p.multiply(&q);
    assert_vec(
        combined.position,
        Vector3::new(1.0, 1.0, 0.0),
        "combined position",
    );
    assert_vec(
        combined.to_matrix().transform_point(&Vector3::ZERO),
        Vector3::new(1.0, 1.0, 0.0),
        "origin maps to combined position",
    );
    assert_vec(
        combined.to_matrix().transform_point(&Vector3::X),
        Vector3::new(1.0, 1.0, 1.0),
        "hand case: (1,0,0) -> (1,1,1)",
    );

    // Generic consistency: placement composition == matrix product.
    let samples = [
        (
            Placement::new(Vector3::new(1.0, 2.0, 3.0), Rotation::around_y(0.7)),
            Placement::new(Vector3::new(-4.0, 0.5, 2.0), Rotation::around_z(-1.3)),
        ),
        (
            Placement::IDENTITY,
            Placement::new(Vector3::new(1.0, 1.0, 1.0), Rotation::around_x(2.9)),
        ),
        (p, q),
    ];
    for (i, (a, b)) in samples.iter().enumerate() {
        let composed = a.multiply(b);
        let via_matrix = a.to_matrix().multiply(&b.to_matrix());
        for point in [
            Vector3::ZERO,
            Vector3::new(1.0, -2.0, 3.0),
            Vector3::new(-0.5, 0.5, -0.5),
        ] {
            let by_placement = composed.to_matrix().transform_point(&point);
            let by_matrix = via_matrix.transform_point(&point);
            assert!(
                (by_placement - by_matrix).length() <= 1e-12,
                "case {i}: placement composition == matrix product"
            );
        }
        // Stepwise application gives the same result.
        let step = b
            .to_matrix()
            .transform_point(&a.to_matrix().transform_point(&Vector3::new(0.3, 0.4, 0.5)));
        let direct = composed
            .to_matrix()
            .transform_point(&Vector3::new(0.3, 0.4, 0.5));
        assert!(
            (step - direct).length() <= 1e-12,
            "case {i}: compose == apply sequentially"
        );
    }
}

#[test]
fn placement_inverse_round_trip() {
    let samples = [
        Placement::IDENTITY,
        Placement::new(
            Vector3::new(3.0, -4.0, 5.0),
            Rotation::around_z(std::f64::consts::FRAC_PI_2),
        ),
        Placement::new(
            Vector3::new(-1.25, 0.5, 2.0),
            Rotation::from_axis_angle(Vector3::new(1.0, 1.0, 2.0), 2.05).expect("non-zero axis"),
        ),
    ];
    for (i, pl) in samples.iter().enumerate() {
        let inv = pl.inverse();
        // Both composition orders collapse to the identity transform.
        for combined in [pl.multiply(&inv), inv.multiply(pl)] {
            assert_vec(
                combined.position,
                Vector3::ZERO,
                "case {i}: inverse composition position",
            );
            assert_rot(
                &combined.rotation,
                &Rotation::IDENTITY,
                "case {i}: inverse composition rotation",
            );
        }
        // Transform then un-transform restores sample points.
        for point in [Vector3::ZERO, Vector3::new(1.0, 2.0, -3.0)] {
            let there = pl.to_matrix().transform_point(&point);
            let back = inv.to_matrix().transform_point(&there);
            assert!(
                (back - point).length() <= 1e-12,
                "case {i}: round-trip restores point"
            );
        }
    }
}

#[test]
fn placement_matrix_agrees_with_rotate_then_translate() {
    let pl = Placement::new(Vector3::new(10.0, -20.0, 30.0), Rotation::around_y(1.234));
    let m = pl.to_matrix();
    let p = Vector3::new(1.0, 2.0, 3.0);
    let manual = pl.rotation.rotate_vector(&p) + pl.position;
    assert_vec(m.transform_point(&p), manual, "rotate then translate");
}

// ---------------------------------------------------------------------------
// BoundBox3
// ---------------------------------------------------------------------------

#[test]
fn bbox_from_points_and_contains() {
    assert_eq!(BoundBox3::from_points(&[]), None, "no points -> None");

    let single =
        BoundBox3::from_points(&[Vector3::new(2.0, -1.0, 5.0)]).expect("one point gives a box");
    assert_vec(single.min, Vector3::new(2.0, -1.0, 5.0), "single min");
    assert_vec(single.max, Vector3::new(2.0, -1.0, 5.0), "single max");

    let cloud = [
        Vector3::new(-1.0, -2.0, -3.0),
        Vector3::new(5.0, 0.0, 7.0),
        Vector3::new(2.0, 1.0, 0.0),
        Vector3::new(4.0, -2.0, 1.0),
    ];
    let b = BoundBox3::from_points(&cloud).expect("points give a box");
    assert_vec(b.min, Vector3::new(-1.0, -2.0, -3.0), "cloud min");
    assert_vec(b.max, Vector3::new(5.0, 1.0, 7.0), "cloud max");

    // Contains is inclusive on all six faces.
    for p in [
        b.min,
        b.max,
        Vector3::new(-1.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
    ] {
        assert!(b.contains_point(&p), "contains {p:?}");
    }
    for p in [
        Vector3::new(-1.0001, 0.0, 0.0),
        Vector3::new(0.0, 1.0001, 0.0),
        Vector3::new(0.0, 0.0, 7.0001),
    ] {
        assert!(!b.contains_point(&p), "excludes {p:?}");
    }
}

#[test]
fn bbox_add_point_and_union_of_boxes() {
    let mut b =
        BoundBox3::from_points(&[Vector3::ZERO, Vector3::new(1.0, 1.0, 1.0)]).expect("two points");
    b.add_point(&Vector3::new(2.0, -1.0, 0.5));
    assert_vec(b.min, Vector3::new(0.0, -1.0, 0.0), "grown min");
    assert_vec(b.max, Vector3::new(2.0, 1.0, 1.0), "grown max");

    // Union with a disjoint box covers both.
    let disjoint = BoundBox3::new(Vector3::new(2.0, 2.0, 2.0), Vector3::new(3.0, 3.0, 3.0));
    b.add_box(&disjoint);
    assert_vec(b.min, Vector3::new(0.0, -1.0, 0.0), "union min");
    assert_vec(b.max, Vector3::new(3.0, 3.0, 3.0), "union max");
    assert!(
        b.contains_point(&Vector3::new(0.5, 0.5, 0.5)),
        "union covers first"
    );
    assert!(
        b.contains_point(&Vector3::new(2.5, 2.5, 2.5)),
        "union covers second"
    );

    // Union with an empty box is a no-op (the default box is empty).
    let one = Vector3::new(1.0, 1.0, 1.0);
    let mut untouched = BoundBox3::new(one, one);
    untouched.add_box(&BoundBox3::default());
    assert_vec(untouched.min, one, "empty union: min unchanged");
    assert_vec(untouched.max, one, "empty union: max unchanged");

    // An empty box initializes from the first box added.
    let mut fresh = BoundBox3::default();
    assert!(fresh.is_empty(), "default box is empty");
    fresh.add_box(&disjoint);
    assert_vec(fresh.min, Vector3::new(2.0, 2.0, 2.0), "fresh min from box");
    assert_vec(fresh.max, Vector3::new(3.0, 3.0, 3.0), "fresh max from box");

    // add_point on an empty box initializes from the point.
    let mut from_point = BoundBox3::default();
    from_point.add_point(&Vector3::new(7.0, 8.0, 9.0));
    assert_vec(
        from_point.min,
        Vector3::new(7.0, 8.0, 9.0),
        "point init min",
    );
    assert_vec(
        from_point.max,
        Vector3::new(7.0, 8.0, 9.0),
        "point init max",
    );
}

#[test]
fn bbox_measures_center_size_volume_empty() {
    let b = BoundBox3::new(Vector3::ZERO, Vector3::new(2.0, 4.0, 6.0));
    assert_vec(b.center(), Vector3::new(1.0, 2.0, 3.0), "center");
    assert_vec(b.size(), Vector3::new(2.0, 4.0, 6.0), "size");
    assert!((b.volume() - 48.0).abs() <= TOL, "volume");
    assert!(!b.is_empty(), "well-formed box is not empty");

    // The default box is empty and reports volume 0, not a negative
    // product of its degenerate extents.
    let e = BoundBox3::default();
    assert!(e.is_empty(), "default is empty");
    assert_eq!(e.volume(), 0.0, "empty volume is 0");

    // A degenerate single-point box is valid but flat.
    let flat = BoundBox3::new(Vector3::new(1.0, 1.0, 1.0), Vector3::new(1.0, 1.0, 1.0));
    assert!(!flat.is_empty(), "single point is not empty");
    assert_eq!(flat.volume(), 0.0, "flat volume is 0");
    assert!(
        flat.contains_point(&Vector3::new(1.0, 1.0, 1.0)),
        "contains itself"
    );
}
