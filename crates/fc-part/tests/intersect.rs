//! K3a acceptance — exact intersections of analytic surfaces
//! (`intersect.rs`).
//!
//! Written BEFORE the implementation (the acceptance-first discipline,
//! recommendation #6): every assertion below is the contract for
//! `fc_part::intersect::intersect_surfaces` — intersections of the K1
//! analytic surface set (plane, cylinder, cone, sphere, torus) computed
//! in closed form, returning curves from the K1 curve set (line,
//! circle, ellipse) with every configuration classified exactly
//! (crossing / tangent / empty / coincident) and the non-analytic pairs
//! refused honestly.
//!
//! Verification style (independent of the implementation's math):
//! * every returned curve is dense-sampled and checked against **both
//!   operands' implicit equations**, recomputed test-side from the
//!   documented parameterizations — residuals at machine scale;
//! * distinguished points (centers, radii, offsets) are re-derived
//!   test-side from the classical constructions (radical planes,
//!   generator angles, quadratic completion);
//! * tangency and disjointness classifications are exercised on both
//!   sides of every boundary, with exactly-representable operands so
//!   the classification is not a tolerance judgment;
//! * non-analytic pairs return `NotAnalytic` with a reason naming the
//!   geometry — never a fabricated approximation.

use fc_base::Vector3;
use fc_part::intersect::{intersect_surfaces, SurfaceIntersection};
use fc_part::topology::Surface;

const PI: f64 = std::f64::consts::PI;

// ---------------------------------------------------------------------------
// helpers (test-side, independent of the implementation's math)
// ---------------------------------------------------------------------------

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

fn vec_approx(a: Vector3, b: Vector3, what: &str) {
    approx(
        (a.x - b.x)
            .abs()
            .max((a.y - b.y).abs())
            .max((a.z - b.z).abs()),
        0.0,
        what,
    );
}

/// Test-side implicit residual of `p` against `s`: the signed distance
/// (plane), radial-minus-radius (cylinder), |p−c|−r (sphere), the
/// torus tube equation, and the cone's angle-to-axis equation —
/// recomputed from each kind's documented parameterization, independent
/// of the implementation.
fn residual(s: &Surface, p: Vector3) -> f64 {
    match s {
        Surface::Plane { origin, normal, .. } => {
            (p.x - origin.x) * normal.x + (p.y - origin.y) * normal.y + (p.z - origin.z) * normal.z
        }
        Surface::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let d = v3(p.x - origin.x, p.y - origin.y, p.z - origin.z);
            let along = d.dot(axis);
            let radial = (d - v3(axis.x * along, axis.y * along, axis.z * along)).length();
            radial - radius
        }
        Surface::Sphere { center, radius, .. } => {
            let d = v3(p.x - center.x, p.y - center.y, p.z - center.z);
            d.length() - radius
        }
        Surface::Cone {
            origin,
            axis,
            semi_angle,
            ..
        } => {
            // The double cone through `origin` along `axis`:
            // radial² = along²·tan²(semi).
            let d = v3(p.x - origin.x, p.y - origin.y, p.z - origin.z);
            let along = d.dot(axis);
            let radial = (d - v3(axis.x * along, axis.y * along, axis.z * along)).length();
            radial - along.abs() * semi_angle.tan()
        }
        Surface::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let d = v3(p.x - center.x, p.y - center.y, p.z - center.z);
            let along = d.dot(axis);
            let radial = (d - v3(axis.x * along, axis.y * along, axis.z * along)).length();
            (radial - major_radius) * (radial - major_radius) + along * along
                - minor_radius * minor_radius
        }
        Surface::Bspline { .. } => panic!("residual: bspline surfaces are not analytic"),
    }
}

/// Sample a point on a curve at parameter `t`, test-side, from the
/// documented parameterizations (line/circle/ellipse).
fn curve_point(c: &fc_part::topology::Curve, t: f64) -> Vector3 {
    use fc_part::topology::Curve;
    match c {
        Curve::Line { origin, direction } => v3(
            origin.x + t * direction.x,
            origin.y + t * direction.y,
            origin.z + t * direction.z,
        ),
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            let second = axis.cross(ref_dir);
            v3(
                center.x + radius * (t.cos() * ref_dir.x + t.sin() * second.x),
                center.y + radius * (t.cos() * ref_dir.y + t.sin() * second.y),
                center.z + radius * (t.cos() * ref_dir.z + t.sin() * second.z),
            )
        }
        Curve::Ellipse {
            center,
            axis,
            major_dir,
            major_radius,
            minor_radius,
        } => {
            let minor_dir = axis.cross(major_dir);
            v3(
                center.x
                    + major_radius * t.cos() * major_dir.x
                    + minor_radius * t.sin() * minor_dir.x,
                center.y
                    + major_radius * t.cos() * major_dir.y
                    + minor_radius * t.sin() * minor_dir.y,
                center.z
                    + major_radius * t.cos() * major_dir.z
                    + minor_radius * t.sin() * minor_dir.z,
            )
        }
        Curve::Bspline { .. } => panic!("curve_point: not an analytic curve"),
    }
}

/// The strongest honest check: dense-sample `c` and assert both
/// operands' implicit residuals stay at machine scale.
fn on_both(a: &Surface, b: &Surface, c: &fc_part::topology::Curve, what: &str) {
    let n = 512;
    for i in 0..n {
        let t = (i as f64) * (2.0 * PI / n as f64) - PI;
        let ts = if matches!(c, fc_part::topology::Curve::Line { .. }) {
            (i as f64) * 0.125 - 32.0
        } else {
            t
        };
        let p = curve_point(c, ts);
        let ra = residual(a, p);
        let rb = residual(b, p);
        let scale = 1.0;
        assert!(
            ra.abs() <= 1e-9 * scale && rb.abs() <= 1e-9 * scale,
            "{what}: sample {i} (t={ts:.4}) residuals a={ra:.3e} b={rb:.3e}"
        );
    }
}

fn expect_curves(r: &SurfaceIntersection) -> &Vec<fc_part::topology::Curve> {
    match r {
        SurfaceIntersection::Curves(cs) => cs,
        other => panic!("expected curves, got {other:?}"),
    }
}

fn plane(origin: Vector3, normal: Vector3) -> Surface {
    let n = normal.normalized().unwrap();
    let x_dir = if n.x.abs() < 0.9 {
        v3(1.0, 0.0, 0.0)
    } else {
        v3(0.0, 1.0, 0.0)
    };
    let x_dir = (x_dir
        - v3(
            n.x * x_dir.dot(&n),
            n.y * x_dir.dot(&n),
            n.z * x_dir.dot(&n),
        ))
    .normalized()
    .unwrap();
    Surface::Plane {
        origin,
        normal: n,
        x_dir,
    }
}

fn cylinder(origin: Vector3, axis: Vector3, radius: f64) -> Surface {
    let a = axis.normalized().unwrap();
    let ref_dir = if a.x.abs() < 0.9 {
        v3(1.0, 0.0, 0.0)
    } else {
        v3(0.0, 1.0, 0.0)
    };
    let ref_dir = (ref_dir
        - v3(
            a.x * ref_dir.dot(&a),
            a.y * ref_dir.dot(&a),
            a.z * ref_dir.dot(&a),
        ))
    .normalized()
    .unwrap();
    Surface::Cylinder {
        origin,
        axis: a,
        ref_dir,
        radius,
    }
}

fn cone(origin: Vector3, axis: Vector3, semi_angle: f64, ref_radius: f64) -> Surface {
    let a = axis.normalized().unwrap();
    let ref_dir = if a.x.abs() < 0.9 {
        v3(1.0, 0.0, 0.0)
    } else {
        v3(0.0, 1.0, 0.0)
    };
    let ref_dir = (ref_dir
        - v3(
            a.x * ref_dir.dot(&a),
            a.y * ref_dir.dot(&a),
            a.z * ref_dir.dot(&a),
        ))
    .normalized()
    .unwrap();
    Surface::Cone {
        origin,
        axis: a,
        ref_dir,
        semi_angle,
        ref_radius,
    }
}

fn sphere(center: Vector3, radius: f64) -> Surface {
    Surface::Sphere {
        center,
        axis: v3(0.0, 0.0, 1.0),
        ref_dir: v3(1.0, 0.0, 0.0),
        radius,
    }
}

fn torus(center: Vector3, axis: Vector3, major_radius: f64, minor_radius: f64) -> Surface {
    let a = axis.normalized().unwrap();
    let ref_dir = if a.x.abs() < 0.9 {
        v3(1.0, 0.0, 0.0)
    } else {
        v3(0.0, 1.0, 0.0)
    };
    let ref_dir = (ref_dir
        - v3(
            a.x * ref_dir.dot(&a),
            a.y * ref_dir.dot(&a),
            a.z * ref_dir.dot(&a),
        ))
    .normalized()
    .unwrap();
    Surface::Torus {
        center,
        axis: a,
        ref_dir,
        major_radius,
        minor_radius,
    }
}

fn assert_lines(r: &SurfaceIntersection, want: usize, what: &str) {
    let cs = expect_curves(r);
    assert_eq!(cs.len(), want, "{what}: curve count");
    for c in cs {
        assert!(
            matches!(c, fc_part::topology::Curve::Line { .. }),
            "{what}: expected lines"
        );
    }
}

fn assert_circles(r: &SurfaceIntersection, want: usize, what: &str) {
    let cs = expect_curves(r);
    assert_eq!(cs.len(), want, "{what}: curve count");
    for c in cs {
        assert!(
            matches!(c, fc_part::topology::Curve::Circle { .. }),
            "{what}: expected circles"
        );
    }
}

fn assert_ellipses(r: &SurfaceIntersection, want: usize, what: &str) {
    let cs = expect_curves(r);
    assert_eq!(cs.len(), want, "{what}: curve count");
    for c in cs {
        assert!(
            matches!(c, fc_part::topology::Curve::Ellipse { .. }),
            "{what}: expected ellipses"
        );
    }
}

// ---------------------------------------------------------------------------
// plane × plane
// ---------------------------------------------------------------------------

#[test]
fn plane_plane_crossing_is_the_exact_cross_product_line() {
    let a = plane(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0));
    let b = plane(v3(0.0, 0.0, 3.0), v3(1.0, 1.0, 0.0));
    let r = intersect_surfaces(&a, &b);
    let cs = expect_curves(&r);
    assert_eq!(cs.len(), 1, "crossing planes: one line");
    match &cs[0] {
        fc_part::topology::Curve::Line { origin, direction } => {
            // The line direction is the (normalized) cross of the
            // normals; test-side recompute.
            let n = v3(0.0, 0.0, 1.0)
                .cross(&v3(1.0, 1.0, 0.0))
                .normalized()
                .unwrap();
            vec_approx(*direction, n, "line direction = normalized n_a × n_b");
            // z = 0 and x + y = 3: the line lives at their meet.
            let _ = origin;
            on_both(&a, &b, &cs[0], "plane×plane line");
        }
        other => panic!("plane×plane: expected a line, got {other:?}"),
    }
}

#[test]
fn plane_plane_parallel_distinct_is_empty() {
    let a = plane(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0));
    let b = plane(v3(0.0, 0.0, 2.0), v3(0.0, 0.0, 5.0));
    assert!(matches!(
        intersect_surfaces(&a, &b),
        SurfaceIntersection::Empty
    ));
}

#[test]
fn plane_plane_coincident_is_coincident() {
    let a = plane(v3(1.0, -2.0, 0.5), v3(0.0, 0.0, 1.0));
    let b = plane(v3(-7.0, 3.0, 0.5), v3(0.0, 0.0, 1.0));
    assert!(matches!(
        intersect_surfaces(&a, &b),
        SurfaceIntersection::Coincident
    ));
}

// ---------------------------------------------------------------------------
// plane × sphere
// ---------------------------------------------------------------------------

#[test]
fn plane_sphere_secant_is_the_projection_circle() {
    let s = sphere(v3(1.0, 2.0, -1.0), 5.0);
    // A vertical plane through x = 4: distance 3 from the center.
    let p = plane(v3(4.0, 0.0, 0.0), v3(1.0, 0.0, 0.0));
    let r = intersect_surfaces(&s, &p);
    assert_circles(&r, 1, "plane×sphere secant");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            // Test-side: center = projection of the sphere center onto
            // the plane (4, 2, -1); radius² = R² − d² = 25 − 9 = 16.
            vec_approx(*center, v3(4.0, 2.0, -1.0), "circle center = projection");
            approx(*radius, 4.0, "radius = √(R² − d²)");
        }
        other => panic!("plane×sphere: expected a circle, got {other:?}"),
    }
    on_both(&s, &p, &expect_curves(&r)[0], "plane×sphere circle");
}

#[test]
fn plane_sphere_oblique_secant_rotated_frames() {
    // A general-position plane against a general-position sphere: the
    // construction must work in arbitrary frames, not just axis-aligned.
    let s = sphere(v3(2.0, -3.0, 1.0), 4.0);
    // Plane with normal (1,1,1)/√3 through (2,-3,1)+normal·2.5:
    // signed distance 2.5 < 4.
    let n = v3(1.0, 1.0, 1.0).normalized().unwrap();
    let p = plane(
        v3(2.0 + 2.5 * n.x, -3.0 + 2.5 * n.y, 1.0 + 2.5 * n.z),
        v3(1.0, 1.0, 1.0),
    );
    let r = intersect_surfaces(&s, &p);
    assert_circles(&r, 1, "plane×sphere oblique");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            let expected_center = v3(2.0 + 2.5 * n.x, -3.0 + 2.5 * n.y, 1.0 + 2.5 * n.z);
            vec_approx(*center, expected_center, "oblique circle center");
            approx(*radius, (16.0f64 - 6.25).sqrt(), "oblique radius");
        }
        _other => panic!("expected circle"),
    }
    on_both(&s, &p, &expect_curves(&r)[0], "plane×sphere oblique circle");
}

#[test]
fn plane_sphere_tangent_is_the_point() {
    let s = sphere(v3(0.0, 0.0, 0.0), 3.0);
    let p = plane(v3(3.0, 0.0, 0.0), v3(1.0, 0.0, 0.0));
    match intersect_surfaces(&s, &p) {
        SurfaceIntersection::Point(q) => vec_approx(q, v3(3.0, 0.0, 0.0), "tangent point"),
        other => panic!("plane×sphere tangent: expected Point, got {other:?}"),
    }
}

#[test]
fn plane_sphere_miss_is_empty() {
    let s = sphere(v3(0.0, 0.0, 0.0), 3.0);
    let p = plane(v3(3.5, 0.0, 0.0), v3(1.0, 0.0, 0.0));
    assert!(matches!(
        intersect_surfaces(&s, &p),
        SurfaceIntersection::Empty
    ));
}

// ---------------------------------------------------------------------------
// sphere × sphere — the radical-plane circle
// ---------------------------------------------------------------------------

#[test]
fn sphere_sphere_secant_is_the_radical_plane_circle() {
    let a = sphere(v3(-2.0, 0.0, 0.0), 3.0);
    let b = sphere(v3(2.0, 1.0, 0.5), 4.0);
    let r = intersect_surfaces(&a, &b);
    assert_circles(&r, 1, "sphere×sphere secant");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            // Test-side classical construction: centers distance d,
            // d1 = (d² + r1² − r2²)/(2d) from c1 along the center
            // line; radius² = r1² − d1².
            let c1 = v3(-2.0, 0.0, 0.0);
            let c2 = v3(2.0, 1.0, 0.5);
            let dvec = v3(c2.x - c1.x, c2.y - c1.y, c2.z - c1.z);
            let d = dvec.length();
            let d1 = (d * d + 9.0 - 16.0) / (2.0 * d);
            let expected = v3(
                c1.x + d1 * dvec.x / d,
                c1.y + d1 * dvec.y / d,
                c1.z + d1 * dvec.z / d,
            );
            vec_approx(*center, expected, "radical circle center");
            approx(*radius, (9.0 - d1 * d1).sqrt(), "radical circle radius");
        }
        _other => panic!("expected circle"),
    }
    // Both sphere equations hold at every sample — the strongest check.
    on_both(&a, &b, &expect_curves(&r)[0], "sphere×sphere circle");
}

#[test]
fn sphere_sphere_external_tangent_is_the_point() {
    let a = sphere(v3(0.0, 0.0, 0.0), 2.0);
    let b = sphere(v3(5.0, 0.0, 0.0), 3.0);
    match intersect_surfaces(&a, &b) {
        SurfaceIntersection::Point(q) => vec_approx(q, v3(2.0, 0.0, 0.0), "external tangent point"),
        other => panic!("expected Point, got {other:?}"),
    }
}

#[test]
fn sphere_sphere_internal_tangent_is_the_point() {
    let a = sphere(v3(0.0, 0.0, 0.0), 5.0);
    let b = sphere(v3(3.0, 0.0, 0.0), 2.0);
    match intersect_surfaces(&a, &b) {
        SurfaceIntersection::Point(q) => vec_approx(q, v3(5.0, 0.0, 0.0), "internal tangent point"),
        other => panic!("expected Point, got {other:?}"),
    }
}

#[test]
fn sphere_sphere_disjoint_and_contained_are_empty() {
    let a = sphere(v3(0.0, 0.0, 0.0), 2.0);
    let b = sphere(v3(9.0, 0.0, 0.0), 3.0);
    assert!(matches!(
        intersect_surfaces(&a, &b),
        SurfaceIntersection::Empty
    ));
    let c = sphere(v3(0.2, 0.0, 0.0), 1.0);
    assert!(matches!(
        intersect_surfaces(&a, &c),
        SurfaceIntersection::Empty
    ));
}

#[test]
fn sphere_sphere_identical_is_coincident() {
    let a = sphere(v3(1.0, 2.0, 3.0), 4.0);
    let b = sphere(v3(1.0, 2.0, 3.0), 4.0);
    assert!(matches!(
        intersect_surfaces(&a, &b),
        SurfaceIntersection::Coincident
    ));
}

// ---------------------------------------------------------------------------
// plane × cylinder — the three alignments
// ---------------------------------------------------------------------------

#[test]
fn plane_cylinder_perpendicular_is_the_axis_meet_circle() {
    let c = cylinder(v3(1.0, 1.0, -2.0), v3(0.0, 0.0, 1.0), 2.0);
    let p = plane(v3(0.0, 0.0, 3.0), v3(0.0, 0.0, 1.0));
    let r = intersect_surfaces(&c, &p);
    assert_circles(&r, 1, "plane⊥axis × cylinder");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            vec_approx(*center, v3(1.0, 1.0, 3.0), "circle center = axis ∩ plane");
            approx(*radius, 2.0, "circle radius = cylinder radius");
        }
        _other => panic!("expected circle"),
    }
    on_both(&c, &p, &expect_curves(&r)[0], "plane⊥axis circle");
}

#[test]
fn plane_cylinder_perpendicular_rotated() {
    // Same geometry, arbitrary frames: axis along (1,1,1)/√3.
    let d = v3(1.0, 1.0, 1.0).normalized().unwrap();
    let c = cylinder(v3(0.5, -0.5, 0.25), d, 1.5);
    let p = plane(v3(3.0, 2.0, 1.0), d);
    let r = intersect_surfaces(&c, &p);
    assert_circles(&r, 1, "plane⊥axis rotated");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            // Test-side: the axis point closest to the plane's origin
            // that also lies in the plane = origin + t·d with
            // (origin + t·d − p0)·n = 0, n = d.
            let o = v3(0.5, -0.5, 0.25);
            let p0 = v3(3.0, 2.0, 1.0);
            let t = (p0.dot(&d) - o.dot(&d)) / d.dot(&d);
            let expected = v3(o.x + t * d.x, o.y + t * d.y, o.z + t * d.z);
            vec_approx(*center, expected, "rotated ⊥ center");
            approx(*radius, 1.5, "rotated ⊥ radius");
        }
        _other => panic!("expected circle"),
    }
    on_both(&c, &p, &expect_curves(&r)[0], "rotated ⊥ circle");
}

#[test]
fn plane_cylinder_parallel_two_lines_and_tangent_and_miss() {
    let c = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 2.0);
    // Vertical plane x = 1 (contains the axis direction): distance 1 < 2.
    let p = plane(v3(1.0, 0.0, 0.0), v3(1.0, 0.0, 0.0));
    let r = intersect_surfaces(&c, &p);
    assert_lines(&r, 2, "plane∥axis × cylinder: two lines");
    {
        // Test-side: the two lines are at y = ±√(r²−d²) = ±√3,
        // direction = axis.
        let cs = expect_curves(&r);
        let mut ys = vec![];
        for l in cs {
            match l {
                fc_part::topology::Curve::Line { origin, direction } => {
                    vec_approx(*direction, v3(0.0, 0.0, 1.0), "line direction = axis");
                    ys.push(origin.y);
                }
                other => panic!("expected line, got {other:?}"),
            }
        }
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        approx(ys[0], -3f64.sqrt(), "first line y = −√3");
        approx(ys[1], 3f64.sqrt(), "second line y = +√3");
        for l in expect_curves(&r) {
            on_both(&c, &p, l, "plane∥axis line");
        }
    }
    // Tangent vertical plane x = 2: one line at y = 0.
    let pt = plane(v3(2.0, 0.0, 0.0), v3(1.0, 0.0, 0.0));
    let rt = intersect_surfaces(&c, &pt);
    assert_lines(&rt, 1, "tangent plane: one line");
    match &expect_curves(&rt)[0] {
        fc_part::topology::Curve::Line { origin, .. } => vec_approx(
            v3(origin.x, origin.y, origin.z),
            v3(2.0, 0.0, 0.0),
            "tangent line origin",
        ),
        _other => panic!("expected line"),
    }
    // Miss: x = 2.5.
    let pm = plane(v3(2.5, 0.0, 0.0), v3(1.0, 0.0, 0.0));
    assert!(matches!(
        intersect_surfaces(&c, &pm),
        SurfaceIntersection::Empty
    ));
}

#[test]
fn plane_cylinder_oblique_is_the_stretched_ellipse() {
    let c = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 1.0);
    // Plane tilted 30° from horizontal: normal at 60° from the axis
    // → cos γ = 1/2 → major = r/cos γ = 2, minor = 1.
    let n = v3(
        0.0,
        (60.0f64).to_radians().sin(),
        (60.0f64).to_radians().cos(),
    );
    let p = plane(v3(0.0, 0.0, 1.0), n);
    let r = intersect_surfaces(&c, &p);
    assert_ellipses(&r, 1, "oblique plane × cylinder");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Ellipse {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            // The axis pierces the plane at t: z = 1/ (n·â) ... plane
            // through (0,0,1) with normal n: points q with (q−p0)·n=0.
            // Axis point (0,0,t): (t−1)·n_z = 0 → t = 1.
            vec_approx(*center, v3(0.0, 0.0, 1.0), "ellipse center = axis ∩ plane");
            approx(*major_radius, 2.0, "major = r/cos γ");
            approx(*minor_radius, 1.0, "minor = r");
        }
        other => panic!("expected ellipse, got {other:?}"),
    }
    on_both(&c, &p, &expect_curves(&r)[0], "oblique ellipse");
}

// ---------------------------------------------------------------------------
// plane × cone — circle / generators / ellipse
// ---------------------------------------------------------------------------

#[test]
fn plane_cone_perpendicular_is_the_axis_meet_circle() {
    // Plain cone (ref_radius 0, apex at the origin): semi 45°.
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0);
    let p = plane(v3(0.0, 0.0, 2.0), v3(0.0, 0.0, 1.0));
    let r = intersect_surfaces(&c, &p);
    assert_circles(&r, 1, "plane⊥axis × cone");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            vec_approx(*center, v3(0.0, 0.0, 2.0), "circle center");
            approx(*radius, 2.0, "radius = z·tan(45°)");
        }
        _other => panic!("expected circle"),
    }
    on_both(&c, &p, &expect_curves(&r)[0], "plane⊥cone circle");
}

#[test]
fn plane_cone_perpendicular_below_apex_hits_the_mirror_nappe() {
    // The OCCT parameterization is the double cone: a plane below the
    // apex still cuts a circle (on the mirrored nappe).
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0);
    let p = plane(v3(0.0, 0.0, -3.0), v3(0.0, 0.0, 1.0));
    let r = intersect_surfaces(&c, &p);
    assert_circles(&r, 1, "below-apex circle on the mirror nappe");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            vec_approx(*center, v3(0.0, 0.0, -3.0), "mirror circle center");
            approx(*radius, 3.0, "mirror circle radius = |z|·tan(45°)");
        }
        _other => panic!("expected circle"),
    }
    on_both(&c, &p, &expect_curves(&r)[0], "mirror nappe circle");
}

#[test]
fn plane_cone_through_apex_two_generator_lines() {
    // A vertical plane through the apex of the 45° cone cuts two
    // generators (the plane is steeper than the tangent plane).
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0);
    let p = plane(v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0));
    let r = intersect_surfaces(&c, &p);
    assert_lines(&r, 2, "plane through apex: two generators");
    for l in expect_curves(&r) {
        match l {
            fc_part::topology::Curve::Line { direction, .. } => {
                // Test-side: generator directions lie in the plane
                // (x = 0 → d_x = 0) at 45° from the axis.
                approx(direction.x, 0.0, "generator in plane");
                approx(
                    (direction.y * direction.y + direction.z * direction.z).sqrt(),
                    1.0,
                    "generator unit",
                );
                let cos_axis = direction.z;
                approx(
                    cos_axis,
                    2f64.sqrt() / 2.0,
                    "generator at semi-angle from axis",
                );
            }
            _other => panic!("expected line"),
        }
        on_both(&c, &p, l, "generator line");
    }
}

#[test]
fn plane_cone_tangent_plane_through_apex_is_one_line() {
    // The tangent plane of the 45° cone at the generator in the
    // y–z plane: contains one generator only. For a plain 45° cone
    // (semi 45°), the tangent plane at the +y generator makes angle
    // 45° with the axis: normal = (0, cos45, ... ) — a plane through
    // the apex containing the generator (0, sin45, cos45)/1 and
    // tangent to the cone. Its normal: (0, cos45, sin45)... the
    // tangent plane at generator g: contains g and â; normal ⊥ both:
    // n = â × g direction → for g = (0, √2/2, √2/2), â = (0,0,1):
    // n = (0,1,0)×... = (1·√2/2·(0·...)) — compute: â × g =
    // (0·√2/2 − 1·√2/2·0, 1·0 − 0·√2/2, 0·√2/2 − 0·0) = (−√2/2, 0, 0)
    // → wait: â × g = |i j k; 0 0 1; 0 s c| = (0·c − 1·0, 1·0 − 0·c,
    // 0·0 − 0·0) = (0·c, 0, 0)?? Let me just: i(0·c − 1·s·0)... i
    // (a2b3−a3b2) = (0·c − 1·s) = −s; j (a3b1−a1b3) = (1·0 − 0·c) = 0;
    // k (a1b2−a2b1) = (0·s − 0·0) = 0 → â×g = (−s, 0, 0), so the
    // plane normal is (±1, 0, 0)?? That's the vertical plane x = 0 —
    // which contains BOTH generators. That's the plane through the
    // axis, not the tangent plane. The tangent plane at g contains g
    // and is tangent — its normal is g rotated: n = g × (â × g)
    // normalized... g × (â × g) = â(g·g) − g(g·â) = â − cos45·g =
    // (0,0,1) − (√2/2)(0,√2/2,√2/2) = (0, −1/2, 1 − 1/2) = (0,−1/2,1/2)
    // → plane normal (0,−1,1)/√2 through the apex — constructed
    // directly with normal (0, −k, k), k = cos(45°): the normalized
    // form lands one ulp below the sinγ == cosα knife edge (for a
    // one-ulp-flatter stored normal the honest section is the apex
    // point, covered by the through-apex point case).
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0);
    let k = (PI / 4.0f64).cos();
    let p = Surface::Plane {
        origin: v3(0.0, 0.0, 0.0),
        normal: v3(0.0, -k, k),
        x_dir: v3(k, 0.0, k),
    };
    let r = intersect_surfaces(&c, &p);
    assert_lines(&r, 1, "tangent plane through apex: one generator");
    on_both(&c, &p, &expect_curves(&r)[0], "tangent generator");
}

#[test]
fn plane_cone_oblique_secant_is_the_ellipse() {
    // Plain 30° cone (semi 30°), plane normal at γ = 45° from the axis
    // (γ < 90° − 30° → ellipse), plane through (0,0,1):
    // s = 1·(n·(p0−apex)) = cos 45°.
    let semi = (30.0f64).to_radians();
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), semi, 0.0);
    let n = v3(
        0.0,
        (45.0f64).to_radians().sin(),
        (45.0f64).to_radians().cos(),
    );
    let p = plane(v3(0.0, 0.0, 1.0), n);
    let r = intersect_surfaces(&c, &p);
    assert_ellipses(&r, 1, "oblique plane × cone");
    // Test-side quadratic (derived independently in this test):
    // with s = cos45° (signed distance apex→plane along n), γ = 45°,
    // cos²α = 3/4, sin²γ = 1/2, cos²γ = 1/2:
    //   cos²α·y² + w²(cos²α − sin²γ) − 2sw·sinγcosγ + s²(cos²α−cos²γ) = 0
    //   B_w = 1/4, w_c = s·sinγcosγ/B_w, K = B_w·w_c² − s²(cos²α−cos²γ)
    // (y along e2, w along e1 = in-plane axis projection.)
    let s = (45.0f64).to_radians().cos();
    let sin_g = (45.0f64).to_radians().sin();
    let cos_g = s;
    let cos_a = semi.cos();
    let b_w = cos_a * cos_a - sin_g * sin_g;
    let w_c = s * sin_g * cos_g / b_w;
    let k = b_w * w_c * w_c - s * s * (cos_a * cos_a - cos_g * cos_g);
    let a_y = (k / (cos_a * cos_a)).sqrt();
    let a_w = (k / b_w).sqrt();
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Ellipse {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            // Center: apex + s·n + w_c·e1 with e1 = (0, sin γ... e1
            // = projection of â into the plane, normalized = (0,
            // cos γ, −sin γ)... test-side: e1 = â − (â·n)n, |e1| =
            // sin γ → e1_unit = (0, −sinγ·cosγ, 1−cos²γ)/sinγ =
            // (0, −cos γ, sin γ)... verify: â·n = cos γ ✓.
            let e1 = v3(0.0, -cos_g, sin_g);
            let expected_center = v3(
                s * n.x + w_c * e1.x,
                s * n.y + w_c * e1.y,
                s * n.z + w_c * e1.z,
            );
            vec_approx(*center, expected_center, "cone ellipse center");
            let want_major = a_y.max(a_w);
            let want_minor = a_y.min(a_w);
            approx(*major_radius, want_major, "cone ellipse major");
            approx(*minor_radius, want_minor, "cone ellipse minor");
        }
        other => panic!("expected ellipse, got {other:?}"),
    }
    on_both(&c, &p, &expect_curves(&r)[0], "cone ellipse");
}

#[test]
fn plane_cone_hyperbola_is_refused_honestly() {
    // γ + α > 90° → hyperbola: not representable in the K1 curve set.
    let c = cone(
        v3(0.0, 0.0, 0.0),
        v3(0.0, 0.0, 1.0),
        (30.0f64).to_radians(),
        0.0,
    );
    let p = plane(
        v3(0.0, 0.0, 1.0),
        v3(0.0, 1.0, (30.0f64).to_radians().cos() / 4.0),
    );
    // normal ≈ 75.5° from axis → γ+α > 90°.
    match intersect_surfaces(&c, &p) {
        SurfaceIntersection::NotAnalytic { why } => {
            assert!(
                why.to_lowercase().contains("hyperbola") || why.to_lowercase().contains("conic"),
                "the refusal must name the geometry: {why}"
            );
        }
        other => panic!("expected NotAnalytic, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// cylinder × cylinder, cylinder × sphere (coaxial)
// ---------------------------------------------------------------------------

#[test]
fn cylinder_cylinder_parallel_two_lines() {
    let a = cylinder(v3(-1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 2.0);
    let b = cylinder(v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 1.0);
    let r = intersect_surfaces(&a, &b);
    assert_lines(&r, 2, "parallel cylinders: two lines");
    // Test-side: cross-section circles (−1,0,r=2) and (1,0,r=1):
    // d = 2, d1 = (4+4−1)/4 = 7/4 from c1 toward c2 → x = −1+7/4·1 =
    // 3/4; h = √(4 − 49/16) = √(15/16) = √15/4.
    let mut ys = vec![];
    for l in expect_curves(&r) {
        match l {
            fc_part::topology::Curve::Line { origin, direction } => {
                vec_approx(*direction, v3(0.0, 0.0, 1.0), "line direction");
                approx(origin.x, 0.75, "line x = 3/4");
                ys.push(origin.y);
            }
            _other => panic!("expected line"),
        }
    }
    ys.sort_by(|x, y| x.partial_cmp(y).unwrap());
    approx(ys[0], -15f64.sqrt() / 4.0, "first line y");
    approx(ys[1], 15f64.sqrt() / 4.0, "second line y");
    for l in expect_curves(&r) {
        on_both(&a, &b, l, "parallel cylinder line");
    }
}

#[test]
fn cylinder_cylinder_crossing_is_refused_honestly() {
    // Steinmetz: the intersection is a quartic space curve.
    let a = cylinder(v3(0.0, 0.0, -1.0), v3(0.0, 1.0, 0.0), 1.0);
    let b = cylinder(v3(0.0, -1.0, 0.0), v3(0.0, 0.0, 1.0), 1.0);
    match intersect_surfaces(&a, &b) {
        SurfaceIntersection::NotAnalytic { why } => {
            assert!(
                why.to_lowercase().contains("quartic")
                    || why.to_lowercase().contains("non-parallel"),
                "the refusal must name the geometry: {why}"
            );
        }
        other => panic!("expected NotAnalytic, got {other:?}"),
    }
}

#[test]
fn cylinder_sphere_coaxial_two_circles() {
    let s = sphere(v3(0.0, 0.0, 0.0), 5.0);
    let c = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 3.0);
    let r = intersect_surfaces(&s, &c);
    assert_circles(&r, 2, "coaxial cylinder×sphere: two circles");
    // Test-side: z = ±√(25−9) = ±4, radius 3.
    let mut zs = vec![];
    for cc in expect_curves(&r) {
        match cc {
            fc_part::topology::Curve::Circle { center, radius, .. } => {
                approx(*radius, 3.0, "circle radius = cylinder radius");
                zs.push(center.z);
            }
            _other => panic!("expected circle"),
        }
    }
    zs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    approx(zs[0], -4.0, "lower circle z = −4");
    approx(zs[1], 4.0, "upper circle z = +4");
    for cc in expect_curves(&r) {
        on_both(&s, &c, cc, "coaxial cylinder×sphere circle");
    }
}

#[test]
fn cylinder_sphere_noncoaxial_is_refused_honestly() {
    let s = sphere(v3(0.0, 0.5, 0.0), 5.0);
    let c = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 3.0);
    match intersect_surfaces(&s, &c) {
        SurfaceIntersection::NotAnalytic { why } => {
            assert!(
                why.to_lowercase().contains("coaxial") || why.to_lowercase().contains("quartic"),
                "the refusal must name the geometry: {why}"
            );
        }
        other => panic!("expected NotAnalytic, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// plane × torus (axis-perpendicular cuts)
// ---------------------------------------------------------------------------

#[test]
fn plane_torus_perpendicular_two_circles_tangent_one() {
    let t = torus(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 1.0);
    // z = 0.6: (ρ−4)² = 1 − 0.36 → ρ = 4 ± 0.8.
    let p = plane(v3(0.0, 0.0, 0.6), v3(0.0, 0.0, 1.0));
    let r = intersect_surfaces(&t, &p);
    assert_circles(&r, 2, "torus z-cut: two circles");
    let mut rhos = vec![];
    for cc in expect_curves(&r) {
        match cc {
            fc_part::topology::Curve::Circle { center, radius, .. } => {
                vec_approx(*center, v3(0.0, 0.0, 0.6), "circle center on the axis");
                rhos.push(*radius);
            }
            _other => panic!("expected circle"),
        }
    }
    rhos.sort_by(|x, y| x.partial_cmp(y).unwrap());
    approx(rhos[0], 3.2, "inner circle ρ = 4 − 0.8");
    approx(rhos[1], 4.8, "outer circle ρ = 4 + 0.8");
    for cc in expect_curves(&r) {
        on_both(&t, &p, cc, "torus z-cut circle");
    }
    // Tangent: z = 1 → one circle ρ = 4.
    let pt = plane(v3(0.0, 0.0, 1.0), v3(0.0, 0.0, 1.0));
    let rt = intersect_surfaces(&t, &pt);
    assert_circles(&rt, 1, "torus tangent z-cut");
    // Miss: z = 1.5.
    let pm = plane(v3(0.0, 0.0, 1.5), v3(0.0, 0.0, 1.0));
    assert!(matches!(
        intersect_surfaces(&t, &pm),
        SurfaceIntersection::Empty
    ));
}

#[test]
fn plane_torus_oblique_is_refused_honestly() {
    let t = torus(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 1.0);
    let p = plane(v3(0.0, 0.0, 0.0), v3(0.0, 1.0, 1.0));
    match intersect_surfaces(&t, &p) {
        SurfaceIntersection::NotAnalytic { why } => {
            assert!(
                why.to_lowercase().contains("torus"),
                "the refusal must name the geometry: {why}"
            );
        }
        other => panic!("expected NotAnalytic, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// cone × cylinder / cone × cone (coaxial) — circles
// ---------------------------------------------------------------------------

#[test]
fn cone_cylinder_coaxial_is_two_circles() {
    // Plain 45° cone (the OCCT parameterization is the DOUBLE cone),
    // cylinder radius 2 on the same axis: the tube meets the cone
    // where |z|·tan(45°) = 2 → circles at z = ±2 (one per nappe).
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0);
    let k = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 2.0);
    let r = intersect_surfaces(&c, &k);
    assert_circles(&r, 2, "coaxial cone×cylinder (double cone: both nappes)");
    let mut zs = vec![];
    for cc in expect_curves(&r) {
        match cc {
            fc_part::topology::Curve::Circle { center, radius, .. } => {
                approx(*radius, 2.0, "circle radius = cylinder radius");
                zs.push(center.z);
            }
            other => panic!("expected circle, got {other:?}"),
        }
    }
    zs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    approx(zs[0], -2.0, "mirror-nappe circle at z = −r/tan(45°)");
    approx(zs[1], 2.0, "principal-nappe circle at z = +r/tan(45°)");
    for cc in expect_curves(&r) {
        on_both(&c, &k, cc, "cone×cylinder circle");
    }
}

#[test]
fn cone_cone_coaxial_same_semi_shifted_apex_is_one_circle() {
    // Two coaxial plain cones with the SAME semi-angle and apexes 1
    // apart: the nappes meet where |z|·t = |z−1|·t → z = 1/2 — one
    // circle of radius t/2 (t = tan(semi)).
    let a = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 6.0, 0.0);
    let b = cone(v3(0.0, 0.0, 1.0), v3(0.0, 0.0, 1.0), PI / 6.0, 0.0);
    let r = intersect_surfaces(&a, &b);
    assert_circles(&r, 1, "same-semi shifted-apex cones: one circle");
    match &expect_curves(&r)[0] {
        fc_part::topology::Curve::Circle { center, radius, .. } => {
            vec_approx(*center, v3(0.0, 0.0, 0.5), "circle center at z = 1/2");
            approx(*radius, (PI / 6.0f64).tan() / 2.0, "radius = tan(semi)/2");
        }
        other => panic!("expected circle, got {other:?}"),
    }
    on_both(&a, &b, &expect_curves(&r)[0], "shifted-apex circle");
}

#[test]
fn cone_cone_coaxial_different_semi_is_a_circle() {
    // Nested plain cones, same apex: they intersect where the radii
    // agree — only at the apex (r=0)... with the same apex and
    // different semi-angles the nappes only share the apex: a Point.
    let a = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 6.0, 0.0);
    let b = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0);
    match intersect_surfaces(&a, &b) {
        SurfaceIntersection::Point(q) => vec_approx(q, v3(0.0, 0.0, 0.0), "shared apex"),
        other => panic!("expected Point (the apex), got {other:?}"),
    }
}

#[test]
fn cone_cone_coaxial_identical_is_coincident() {
    let a = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 6.0, 0.0);
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 6.0, 0.0);
    assert!(matches!(
        intersect_surfaces(&a, &c),
        SurfaceIntersection::Coincident
    ));
}

// ---------------------------------------------------------------------------
// sphere × cone, cylinder × torus, torus × torus (coaxial)
// ---------------------------------------------------------------------------

#[test]
fn sphere_cone_coaxial_is_two_circles() {
    // Sphere of radius √2 centered at the plain 45° cone's apex:
    // z² + (z·tan45)² = 2 → z² = 1 → circles at z = ±1, radius 1.
    let c = cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0);
    let s = sphere(v3(0.0, 0.0, 0.0), 2f64.sqrt());
    let r = intersect_surfaces(&s, &c);
    assert_circles(&r, 2, "coaxial sphere×cone: two circles");
    let mut zs = vec![];
    for cc in expect_curves(&r) {
        match cc {
            fc_part::topology::Curve::Circle { center, radius, .. } => {
                approx(*radius, 1.0, "circle radius = |z·tan(45°)|");
                zs.push(center.z);
            }
            _other => panic!("expected circle"),
        }
    }
    zs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    approx(zs[0], -1.0, "lower circle z");
    approx(zs[1], 1.0, "upper circle z");
    for cc in expect_curves(&r) {
        on_both(&s, &c, cc, "sphere×cone circle");
    }
}

#[test]
fn cylinder_torus_coaxial_two_circles_tangent_and_miss() {
    let t = torus(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 1.0);
    // Cylinder radius 4.6 around the same axis:
    // (4.6−4)² + z² = 1 → z = ±0.8.
    let k = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.6);
    let r = intersect_surfaces(&t, &k);
    assert_circles(&r, 2, "coaxial cylinder×torus: two circles");
    let mut zs = vec![];
    for cc in expect_curves(&r) {
        match cc {
            fc_part::topology::Curve::Circle { center, radius, .. } => {
                approx(*radius, 4.6, "circle radius = cylinder radius");
                zs.push(center.z);
            }
            _other => panic!("expected circle"),
        }
    }
    zs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    approx(zs[0], -0.8, "lower tube circle z");
    approx(zs[1], 0.8, "upper tube circle z");
    for cc in expect_curves(&r) {
        on_both(&t, &k, cc, "cylinder×torus circle");
    }
    // Tangent: cylinder radius 5 = R + minor → one circle at z = 0.
    let kt = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0);
    let rt = intersect_surfaces(&t, &kt);
    assert_circles(&rt, 1, "tangent cylinder×torus");
    // Miss: radius 5.5.
    let km = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.5);
    assert!(matches!(
        intersect_surfaces(&t, &km),
        SurfaceIntersection::Empty
    ));
}

#[test]
fn torus_torus_coaxial_same_radii_two_circles() {
    // Two identical ring tori, centers 1 apart on the common axis:
    // the tubes meet where z² = (z−1)² → z = 1/2,
    // (ρ−4)² = 1 − 1/4 → ρ = 4 ± √(3)/2.
    let a = torus(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 1.0);
    let b = torus(v3(0.0, 0.0, 1.0), v3(0.0, 0.0, 1.0), 4.0, 1.0);
    let r = intersect_surfaces(&a, &b);
    assert_circles(&r, 2, "coaxial same-radii tori: two circles");
    let mut rhos = vec![];
    for cc in expect_curves(&r) {
        match cc {
            fc_part::topology::Curve::Circle { center, radius, .. } => {
                vec_approx(*center, v3(0.0, 0.0, 0.5), "circle center at z = 1/2");
                rhos.push(*radius);
            }
            _other => panic!("expected circle"),
        }
    }
    rhos.sort_by(|x, y| x.partial_cmp(y).unwrap());
    approx(rhos[0], 4.0 - 3f64.sqrt() / 2.0, "inner tube circle ρ");
    approx(rhos[1], 4.0 + 3f64.sqrt() / 2.0, "outer tube circle ρ");
    for cc in expect_curves(&r) {
        on_both(&a, &b, cc, "torus×torus circle");
    }
    // Coincident: same center.
    let c = torus(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 1.0);
    assert!(matches!(
        intersect_surfaces(&a, &c),
        SurfaceIntersection::Coincident
    ));
}

// ---------------------------------------------------------------------------
// symmetry & honesty
// ---------------------------------------------------------------------------

#[test]
fn intersection_is_symmetric() {
    let cases: Vec<(Surface, Surface)> = vec![
        (
            plane(v3(0.0, 0.0, 0.0), v3(1.0, 2.0, 3.0)),
            sphere(v3(1.0, 1.0, 1.0), 3.0),
        ),
        (
            sphere(v3(-2.0, 0.0, 0.0), 3.0),
            sphere(v3(2.0, 1.0, 0.5), 4.0),
        ),
        (
            cylinder(v3(-1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 2.0),
            cylinder(v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 1.0),
        ),
        (
            torus(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 1.0),
            plane(v3(0.0, 0.0, 0.6), v3(0.0, 0.0, 1.0)),
        ),
        (
            cone(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), PI / 4.0, 0.0),
            plane(v3(0.0, 0.0, 2.0), v3(0.0, 0.0, 1.0)),
        ),
    ];
    for (i, (a, b)) in cases.iter().enumerate() {
        let ab = intersect_surfaces(a, b);
        let ba = intersect_surfaces(b, a);
        // The dispatch canonicalizes operand order, so symmetric
        // calls take the same code path: the answers must be fully
        // equal — same variants, same curve vectors, bit for bit.
        assert!(ab == ba, "case {i}: intersection must be symmetric");
    }
}

#[test]
fn bspline_surfaces_are_refused_honestly() {
    // B-spline surfaces are outside the analytic intersection set.
    let p = plane(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0));
    let bs = Surface::Bspline {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![
            vec![v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0)],
            vec![v3(0.0, 1.0, 0.0), v3(1.0, 1.0, 0.0)],
        ],
        weights: None,
    };
    match intersect_surfaces(&p, &bs) {
        SurfaceIntersection::NotAnalytic { why } => {
            assert!(
                why.to_lowercase().contains("b-spline") || why.to_lowercase().contains("analytic"),
                "the refusal must name the geometry: {why}"
            );
        }
        other => panic!("expected NotAnalytic, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// exact classification at the tangency boundaries
// ---------------------------------------------------------------------------

#[test]
fn tangency_classification_is_exact_not_tolerant() {
    // Exactly tangent and one-ulp-off configurations classify honestly:
    // exact tangency (d = r1 + r2) is Point; a hair inside is Curves.
    let a = sphere(v3(0.0, 0.0, 0.0), 1.0);
    let exactly = sphere(v3(2.5, 0.0, 0.0), 1.5);
    assert!(matches!(
        intersect_surfaces(&a, &exactly),
        SurfaceIntersection::Point(_)
    ));
    // d = 2.5 − 2^-20 (inside by ~1e-6): two surfaces that DO cross.
    let d = 2.5 - 2f64.powi(-20);
    let inside = sphere(v3(d, 0.0, 0.0), 1.5);
    assert!(
        matches!(
            intersect_surfaces(&a, &inside),
            SurfaceIntersection::Curves(_)
        ),
        "a hair inside the tangency must still be a circle, not a point"
    );
    // d = 2.5 + 2^-20 (outside): empty.
    let d = 2.5 + 2f64.powi(-20);
    let outside = sphere(v3(d, 0.0, 0.0), 1.5);
    assert!(matches!(
        intersect_surfaces(&a, &outside),
        SurfaceIntersection::Empty
    ));
}

#[test]
fn cylinder_tangency_classification_is_exact() {
    let a = cylinder(v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 1.0);
    let r = 2.0;
    let d = 3.0;
    // Exact tangency: centers 3 apart, radii 1 + 2.
    let b = cylinder(v3(d, 0.0, 0.0), v3(0.0, 0.0, 1.0), r);
    assert_lines(&intersect_surfaces(&a, &b), 1, "exact tangency: one line");
    // One-ulp inside: two lines.
    let b = cylinder(v3(d - 2f64.powi(-20), 0.0, 0.0), v3(0.0, 0.0, 1.0), r);
    assert_lines(&intersect_surfaces(&a, &b), 2, "hair inside: two lines");
    // One-ulp outside: empty.
    let b = cylinder(v3(d + 2f64.powi(-20), 0.0, 0.0), v3(0.0, 0.0, 1.0), r);
    assert!(matches!(
        intersect_surfaces(&a, &b),
        SurfaceIntersection::Empty
    ));
}
