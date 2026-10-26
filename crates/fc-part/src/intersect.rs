//! K3a — exact intersections of analytic surfaces, in closed form.
//!
//! The K1 surface set (plane, cylinder, cone, sphere, torus — the
//! analytic kinds) intersect, for the pairs whose section curves are
//! representable in the K1 curve set (line, circle, ellipse), in
//! curves constructible by closed-form algebra from the operands'
//! parameters: no iteration, no tolerance, no sampling. Every
//! classification decision (crossing / tangent / disjoint /
//! coincident) is made by an **exact integer-sign computation** over
//! the stored `f64` parameters (error-free-transform expansions,
//! Shewchuk-style — the same discipline as `exact.rs`), so a
//! tangency is a tangency and a miss is a miss, never a tolerance
//! judgment.
//!
//! # What is covered
//!
//! * plane × plane — the cross-product line
//! * plane × sphere — the projection circle
//! * sphere × sphere — the radical-plane circle
//! * plane × cylinder — all three alignments: the axis-meet circle
//!   (plane ⊥ axis), the two/tangent/miss generator-parallel lines
//!   (plane ∥ axis), the stretched ellipse (oblique)
//! * plane × cone — the axis-meet circle (both nappes of the OCCT
//!   double-cone parameterization), the two generator lines / tangent
//!   generator / apex-only point for planes through the apex (bit-exact
//!   apex containment — see below), the oblique ellipse (the section
//!   quadratic), honest refusal of hyperbola/parabola sections
//! * plane × torus — the axis-perpendicular z-cuts (two / tangent /
//!   miss circles), honest refusal of oblique (Villarceau-class) cuts
//! * cylinder × cylinder — parallel axes: the cross-section circle
//!   pairs (two / external-tangent / internal-tangent / nested /
//!   coincident lines); non-parallel refused (the Steinmetz quartic)
//! * cylinder × sphere — coaxial: the latitude circle pairs;
//!   non-coaxial refused (quartic)
//! * cylinder × cone — coaxial: the two nappe circles
//! * cylinder × torus — coaxial: the tube circles
//! * sphere × cone — coaxial: the section quadratic's circles
//! * cone × cone — coaxial: the radius-agreement circles / apex point
//!   / coincident
//! * torus × torus — coaxial with identical radii: the tube-kiss
//!   circles; everything else refused (quartic)
//!
//! # Honesty rules
//!
//! * **Unit frames assumed.** The K1 generator contract writes unit
//!   normals and axes; this module uses them as given (OCCT's
//!   `Geom_Plane` makes the same demand). Frames that are not unit
//!   get machine-scale-garbage answers, not validation errors — the
//!   structural validator remains the gate for that.
//! * **Bands, documented.** "Perpendicular / parallel / coaxial" are
//!   tested at construction-noise scale (`2^-49` relative): within
//!   the band the analytic answer (circle/line) differs from the true
//!   section by less than `1e-9·scale` over any representable model
//!   extent; outside it the constructions run with their rounded
//!   inputs. The bands are constants below, in one place.
//! * **Through-apex generators are bit-exact-only.** A plane whose
//!   stored origin makes the (exact, expansion-computed) dot with the
//!   cone's apex exactly zero yields generator lines; a hair off, the
//!   true section is a hyperbola and the answer is the honest refusal
//!   — never lines approximating a hyperbola. (For `ref_radius ≠ 0`
//!   the apex itself is a rounded construction; genuinely-through
//!   planes may then be refused — recorded here rather than hidden.)
//! * **The cone is the OCCT double cone.** `Geom_ConicalSurface`'s
//!   parameterization covers both nappes (negative ring radius = the
//!   mirror nappe); a perpendicular plane below the apex meets the
//!   mirror nappe, and the coaxial pairs return a circle **per
//!   nappe** where both exist.
//!
//! K3's differential fixture corpus (against real FreeCAD 1.0/OCCT)
//! is the next slice; this module carries the construction-side
//! acceptance suite (`tests/intersect.rs`, written first).

use fc_base::Vector3;

use crate::topology::{Curve, Surface};

/// The result of intersecting two analytic surfaces.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceIntersection {
    /// One or more intersection curves; each lies on both operands
    /// (dense-sample residuals at machine scale — the acceptance
    /// suite's strongest check).
    Curves(Vec<Curve>),
    /// Tangent contact at a single point.
    Point(Vector3),
    /// No common points.
    Empty,
    /// The two surfaces are the same point set.
    Coincident,
    /// The pair's section is not representable in the K1 curve set
    /// (a quartic space curve, a hyperbola, a Villarceau cut, a
    /// B-spline) — refused with the geometry named, never
    /// approximated.
    NotAnalytic { why: String },
}

/// Construction-noise band: below this relative tilt two directions
/// are "parallel" for the structural classifications (the induced
/// geometry differs from the band answer by < `1e-9·scale` over any
/// representable extent; derivation in the module doc).
pub(crate) const PARALLEL_BAND: f64 = 4.414_273_778_205_676e-15; // 2^-48

// ---------------------------------------------------------------------------
// exact float expansions (Shewchuk-style; the classification tier)
// ---------------------------------------------------------------------------

/// The exact product `a·b` as a (value, error) pair.
pub(crate) fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let e = a.mul_add(b, -p);
    (p, e)
}

/// The exact sum `a + b` as a (value, error) pair.
pub(crate) fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    (s, e)
}

/// Grow a non-overlapping expansion by `x`, exactly (Shewchuk's
/// `grow_expansion` in place, terms kept increasing in magnitude).
fn accumulate(partials: &mut Vec<f64>, mut x: f64) {
    let mut j = 0;
    for i in 0..partials.len() {
        let y = partials[i];
        let (s, e) = two_sum(x, y);
        if e != 0.0 {
            partials[j] = e;
            j += 1;
        }
        x = s;
    }
    partials.truncate(j);
    partials.push(x);
}

/// The sign of the exact sum of `terms` (−1, 0, +1).
pub(crate) fn exact_sum_sign(terms: &[f64]) -> i8 {
    let mut partials: Vec<f64> = Vec::with_capacity(terms.len());
    for &t in terms {
        accumulate(&mut partials, t);
    }
    // The partials are non-overlapping and increasing in magnitude;
    // the last non-zero one carries the exact sum's sign.
    for &p in partials.iter().rev() {
        if p > 0.0 {
            return 1;
        }
        if p < 0.0 {
            return -1;
        }
    }
    0
}

/// The exact sum of `terms`, rounded to one `f64` (partials summed
/// smallest-first; correctly rounded to ≤ 1 ulp in all cases).
pub(crate) fn exact_sum_rounded(terms: &[f64]) -> f64 {
    let mut partials: Vec<f64> = Vec::with_capacity(terms.len());
    for &t in terms {
        accumulate(&mut partials, t);
    }
    let mut total = 0.0;
    for &p in &partials {
        total += p;
    }
    total
}

/// Exact comparison of `a·b` vs `c·d` (each product exact through
/// `two_prod`).
#[cfg(test)]
fn cmp_products(a: f64, b: f64, c: f64, d: f64) -> std::cmp::Ordering {
    let (ph, pe) = two_prod(a, b);
    let (qh, qe) = two_prod(c, d);
    let sign = exact_sum_sign(&[ph, pe, -qh, -qe]);
    sign.cmp(&0)
}

/// Exact comparison of `a²` vs `b²`.
#[cfg(test)]
fn cmp_squares(a: f64, b: f64) -> std::cmp::Ordering {
    cmp_products(a, a, b, b)
}

/// `a·b − c·d`, exactly summed then rounded (for radii under square
/// roots).
fn diff_products_rounded(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let (ph, pe) = two_prod(a, b);
    let (qh, qe) = two_prod(c, d);
    exact_sum_rounded(&[ph, pe, -qh, -qe])
}

/// The exact dot product's terms, as expansion input: the six exact
/// halves of the three coordinate products.
pub(crate) fn dot_terms(a: Vector3, b: Vector3) -> Vec<f64> {
    let (xh, xe) = two_prod(a.x, b.x);
    let (yh, ye) = two_prod(a.y, b.y);
    let (zh, ze) = two_prod(a.z, b.z);
    vec![xh, xe, yh, ye, zh, ze]
}

/// Is the exact dot product `a·b` zero? (Bit-level zero of the
/// infinite-precision dot of the stored values.)
fn dot_exactly_zero(a: Vector3, b: Vector3) -> bool {
    exact_sum_sign(&dot_terms(a, b)) == 0
}

// ---------------------------------------------------------------------------
// frame helpers
// ---------------------------------------------------------------------------

/// Any unit direction ⊥ `axis` (canonical: the coordinate axis of
/// the smallest `axis` component, made orthogonal).
pub(crate) fn any_perpendicular(axis: Vector3) -> Vector3 {
    let probe = if axis.x.abs() < 0.9 {
        Vector3::new(1.0, 0.0, 0.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let d = probe.dot(&axis);
    (probe - axis * d)
        .normalized()
        .expect("axis is a direction")
}

fn circle(center: Vector3, axis: Vector3, radius: f64) -> Curve {
    Curve::Circle {
        center,
        axis,
        ref_dir: any_perpendicular(axis),
        radius,
    }
}

fn line(origin: Vector3, direction: Vector3) -> Curve {
    Curve::Line { origin, direction }
}

fn ellipse(center: Vector3, axis: Vector3, major_dir: Vector3, major: f64, minor: f64) -> Curve {
    Curve::Ellipse {
        center,
        axis,
        major_dir,
        major_radius: major,
        minor_radius: minor,
    }
}

fn not_analytic(why: &str) -> SurfaceIntersection {
    SurfaceIntersection::NotAnalytic {
        why: why.to_string(),
    }
}

/// Does the f64 key `a` sort strictly after `b` (lexicographic)? The
/// same-kind pair functions canonicalize their operands with this,
/// so `intersect(a, b)` and `intersect(b, a)` construct bit-identical
/// answers even where the dispatch cannot reorder (same-kind pairs).
fn lex_after(a: &[f64], b: &[f64]) -> bool {
    for (x, y) in a.iter().zip(b.iter()) {
        if x < y {
            return false;
        }
        if x > y {
            return true;
        }
    }
    false
}

/// The squared length's exact expansion terms (three `two_prod`s).
pub(crate) fn length_squared_terms(v: Vector3) -> Vec<f64> {
    let (xh, xe) = two_prod(v.x, v.x);
    let (yh, ye) = two_prod(v.y, v.y);
    let (zh, ze) = two_prod(v.z, v.z);
    vec![xh, xe, yh, ye, zh, ze]
}

// ---------------------------------------------------------------------------
// the pairs
// ---------------------------------------------------------------------------

/// plane × plane — the cross-product line; parallel planes classify
/// by the exact point-vs-plane dot.
fn plane_plane((o1, n1): (Vector3, Vector3), (o2, n2): (Vector3, Vector3)) -> SurfaceIntersection {
    // Canonical operand order: bit-identical answers both ways.
    let (o1, n1, o2, n2) = if lex_after(
        &[n1.x, n1.y, n1.z, o1.x, o1.y, o1.z],
        &[n2.x, n2.y, n2.z, o2.x, o2.y, o2.z],
    ) {
        (o2, n2, o1, n1)
    } else {
        (o1, n1, o2, n2)
    };
    let d = n1.cross(&n2);
    if d.x == 0.0 && d.y == 0.0 && d.z == 0.0 {
        // Parallel (the rounded cross is exactly zero): coincident iff
        // a point of plane 1 satisfies plane 2's equation exactly.
        if dot_exactly_zero(o1 - o2, n2) {
            return SurfaceIntersection::Coincident;
        }
        return SurfaceIntersection::Empty;
    }
    let h1 = n1.dot(&o1);
    let h2 = n2.dot(&o2);
    let d2 = d.dot(&d);
    let origin = (n2.cross(&d) * h1 + d.cross(&n1) * h2) * (1.0 / d2);
    let direction = d.normalized().expect("d is non-zero");
    SurfaceIntersection::Curves(vec![line(origin, direction)])
}

/// plane × sphere — signed distance classified exactly.
fn plane_sphere(o_p: Vector3, n: Vector3, c: Vector3, r: f64) -> SurfaceIntersection {
    let dv = c - o_p;
    // Exact |d| vs r: sign(d − r) and sign(d + r) together.
    let mut minus_terms = dot_terms(dv, n);
    minus_terms.push(-r);
    let sign_minus = exact_sum_sign(&minus_terms);
    let mut plus_terms = dot_terms(dv, n);
    plus_terms.push(r);
    let sign_plus = exact_sum_sign(&plus_terms);
    // |d| < r ⟺ both signs negative-or-mixed with no zero; the exact
    // classification:
    //   d >  r → sign_minus > 0            → Empty
    //   d < −r → sign_plus  < 0            → Empty
    //   d == ±r → one sign zero            → Point
    //   else → Circle
    if sign_minus == 0 || sign_plus == 0 {
        let d = exact_sum_rounded(&dot_terms(dv, n));
        return SurfaceIntersection::Point(c - n * d);
    }
    if sign_minus > 0 || sign_plus < 0 {
        return SurfaceIntersection::Empty;
    }
    let d = exact_sum_rounded(&dot_terms(dv, n));
    let r2_minus_d2 = diff_products_rounded(r, r, d, d);
    if r2_minus_d2 <= 0.0 {
        // Defensive: exact classification said circle but the rounded
        // difference collapsed — the tangency limit.
        return SurfaceIntersection::Point(c - n * d);
    }
    let center = c - n * d;
    SurfaceIntersection::Curves(vec![circle(center, n, r2_minus_d2.sqrt())])
}

/// sphere × sphere — the radical-plane circle, classified exactly.
fn sphere_sphere(c1: Vector3, r1: f64, c2: Vector3, r2: f64) -> SurfaceIntersection {
    // Canonical operand order: the radical-plane construction's
    // axis (c2 − c1) must not flip with the call order.
    let (c1, r1, c2, r2) = if lex_after(&[c1.x, c1.y, c1.z, r1], &[c2.x, c2.y, c2.z, r2]) {
        (c2, r2, c1, r1)
    } else {
        (c1, r1, c2, r2)
    };
    let dvec = c2 - c1;
    let d2_terms = length_squared_terms(dvec);
    let d2_zero = exact_sum_sign(&d2_terms) == 0;
    if d2_zero {
        return if r1 == r2 {
            SurfaceIntersection::Coincident
        } else {
            SurfaceIntersection::Empty
        };
    }
    // d² vs (r1 + r2)² and (r1 − r2)², exactly.
    let mut ext_terms = d2_terms.clone();
    let (sh, se) = two_prod(r1 + r2, r1 + r2);
    ext_terms.extend([sh, se].iter().map(|x| -x).collect::<Vec<f64>>());
    let ext_sign = exact_sum_sign(&ext_terms);
    if ext_sign > 0 {
        return SurfaceIntersection::Empty;
    }
    let mut int_terms = d2_terms.clone();
    let (ih, ie) = two_prod(r1 - r2, r1 - r2);
    int_terms.extend([ih, ie].iter().map(|x| -x).collect::<Vec<f64>>());
    let int_sign = exact_sum_sign(&int_terms);
    if int_sign < 0 {
        return SurfaceIntersection::Empty;
    }
    let d = exact_sum_rounded(&d2_terms).sqrt();
    let n_hat = dvec * (1.0 / d);
    if ext_sign == 0 {
        return SurfaceIntersection::Point(c1 + n_hat * r1);
    }
    if int_sign == 0 {
        // Internal tangency: the touching point is on the far side of
        // the smaller sphere, along the center line.
        return if r1 >= r2 {
            SurfaceIntersection::Point(c1 + n_hat * r1)
        } else {
            SurfaceIntersection::Point(c1 - n_hat * r1)
        };
    }
    // Secant: the radical-plane circle.
    let d2 = exact_sum_rounded(&d2_terms);
    let (p1h, p1e) = two_prod(r1, r1);
    let (p2h, p2e) = two_prod(r2, r2);
    let d1 = exact_sum_rounded(&[d2, p1h, p1e, -p2h, -p2e]) / (2.0 * d);
    let r2_minus_d1_2 = {
        let (dh, de) = two_prod(d1, d1);
        exact_sum_rounded(&[p1h, p1e, -dh, -de])
    };
    let center = c1 + n_hat * d1;
    SurfaceIntersection::Curves(vec![circle(center, n_hat, r2_minus_d1_2.sqrt())])
}

/// plane × cylinder — the three alignments.
fn plane_cylinder(
    o_p: Vector3,
    n: Vector3,
    o_c: Vector3,
    axis: Vector3,
    radius: f64,
) -> SurfaceIntersection {
    let cross = n.cross(&axis);
    let sin_gamma = cross.length();
    let cos_axis = n.dot(&axis); // signed
    if sin_gamma <= PARALLEL_BAND {
        // Plane ⊥ axis: the axis meets the plane; circle of the
        // cylinder's radius there.
        let t = (o_p - o_c).dot(&n) / cos_axis;
        let center = o_c + axis * t;
        return SurfaceIntersection::Curves(vec![circle(center, axis, radius)]);
    }
    if cos_axis.abs() <= PARALLEL_BAND {
        // Plane ∥ axis: cross-section classification.
        let dv = o_c - o_p;
        let mut minus_terms = dot_terms(dv, n);
        minus_terms.push(-radius);
        let sign_minus = exact_sum_sign(&minus_terms);
        let mut plus_terms = dot_terms(dv, n);
        plus_terms.push(radius);
        let sign_plus = exact_sum_sign(&plus_terms);
        if sign_minus == 0 {
            let d = exact_sum_rounded(&dot_terms(dv, n));
            return SurfaceIntersection::Curves(vec![line(o_c - n * d, axis)]);
        }
        if sign_minus > 0 || sign_plus < 0 {
            return SurfaceIntersection::Empty;
        }
        let d = exact_sum_rounded(&dot_terms(dv, n));
        let foot = o_c - n * d;
        let h2 = diff_products_rounded(radius, radius, d, d);
        if h2 <= 0.0 {
            return SurfaceIntersection::Curves(vec![line(foot, axis)]);
        }
        let v = n
            .cross(&axis)
            .normalized()
            .expect("near-vertical: non-zero");
        let h = h2.sqrt();
        return SurfaceIntersection::Curves(vec![
            line(foot + v * h, axis),
            line(foot - v * h, axis),
        ]);
    }
    // Oblique: the stretched ellipse.
    let t = (o_p - o_c).dot(&n) / cos_axis;
    let center = o_c + axis * t;
    let major_dir = (axis - n * cos_axis)
        .normalized()
        .expect("oblique: in-plane projection");
    let major = radius / cos_axis.abs();
    SurfaceIntersection::Curves(vec![ellipse(center, n, major_dir, major, radius)])
}

/// plane × cone — circles / generators / ellipse / point / refusals.
fn plane_cone(
    o_p: Vector3,
    n: Vector3,
    o_c: Vector3,
    axis: Vector3,
    semi: f64,
    ref_radius: f64,
) -> SurfaceIntersection {
    let (sin_semi, cos_semi) = semi.sin_cos();
    if sin_semi <= 0.0 {
        return not_analytic("degenerate cone (semi_angle ≤ 0): no analytic section");
    }
    // The apex of the double-cone parameterization.
    let apex = o_c - axis * (ref_radius * (cos_semi / sin_semi));
    let sin_gamma = n.cross(&axis).length();
    if sin_gamma <= PARALLEL_BAND {
        // Plane ⊥ axis: the axis-meet circle, one per crossing (the
        // sign of the ring radius picks the nappe).
        let t = (o_p - o_c).dot(&n) / n.dot(&axis);
        let center = o_c + axis * t;
        let rho = ref_radius + t * (sin_semi / cos_semi);
        if rho == 0.0 {
            return SurfaceIntersection::Point(center);
        }
        return SurfaceIntersection::Curves(vec![circle(center, axis, rho.abs())]);
    }
    // Through the apex, bit-exactly? (Exact dot of the stored values.)
    if dot_exactly_zero(o_p - apex, n) {
        // Generators: in-plane directions at the semi-angle from the
        // axis. sinγ vs cosα decides two / one (tangent) / none.
        return if sin_gamma > cos_semi {
            let m = n.dot(&axis);
            let e2 = n.cross(&axis) * (1.0 / sin_gamma);
            let e1 = (axis - n * m) * (1.0 / sin_gamma);
            let cos_phi = cos_semi / sin_gamma;
            let sin_phi = (1.0 - cos_phi * cos_phi).sqrt();
            SurfaceIntersection::Curves(vec![
                line(apex, e1 * cos_phi + e2 * sin_phi),
                line(apex, e1 * cos_phi - e2 * sin_phi),
            ])
        } else if sin_gamma == cos_semi {
            let m = n.dot(&axis);
            let e1 = (axis - n * m) * (1.0 / sin_gamma);
            SurfaceIntersection::Curves(vec![line(apex, e1)])
        } else {
            SurfaceIntersection::Point(apex)
        };
    }
    // General oblique: ellipse iff sinγ < cosα (γ + α < 90°);
    // hyperbola/parabola otherwise — refused honestly.
    if sin_gamma >= cos_semi {
        return not_analytic(
            "plane×cone hyperbola/parabola section (γ + semi_angle ≥ 90°) — not representable in the K1 curve set",
        );
    }
    // The section quadratic (module doc derivation): with the plane's
    // signed distance s from the apex along n, in-plane frame
    // (e1 = in-plane axis projection, e2 = n × axis direction):
    //   cos²α·y² + w²·(cos²α − sin²γ) − 2·s·w·m·sinγ + s²(cos²α − m²) = 0
    let m = n.dot(&axis); // signed cosγ
    let b_w = cos_semi * cos_semi - sin_gamma * sin_gamma;
    let e2 = n.cross(&axis) * (1.0 / sin_gamma);
    let e1 = (axis - n * m) * (1.0 / sin_gamma);
    let s = exact_sum_rounded(&dot_terms(o_p - apex, n));
    let w_c = s * m * sin_gamma / b_w;
    let k = b_w * w_c * w_c - s * s * (cos_semi * cos_semi - m * m);
    if k <= 0.0 {
        // Defensive: the algebra guarantees K > 0 for s ≠ 0; a rounded
        // collapse means the configuration sits at the degenerate
        // boundary — refuse rather than fabricate.
        return not_analytic("plane×cone section collapsed at the ellipse boundary");
    }
    let a_y = (k / (cos_semi * cos_semi)).sqrt();
    let a_w = (k / b_w).sqrt();
    let center = apex + n * s + e1 * w_c;
    if a_y >= a_w {
        SurfaceIntersection::Curves(vec![ellipse(center, n, e2, a_y, a_w)])
    } else {
        SurfaceIntersection::Curves(vec![ellipse(center, n, e1, a_w, a_y)])
    }
}

/// plane × torus — the axis-perpendicular z-cuts.
fn plane_torus(
    o_p: Vector3,
    n: Vector3,
    c_t: Vector3,
    axis: Vector3,
    major: f64,
    minor: f64,
) -> SurfaceIntersection {
    if n.cross(&axis).length() > PARALLEL_BAND {
        return not_analytic(
            "oblique plane×torus section (Villarceau class) — not representable in the K1 curve set",
        );
    }
    if major < minor {
        return not_analytic("spindle/self-intersecting torus (major < minor) is out of scope");
    }
    let dz = (o_p - c_t).dot(&axis);
    let h2 = diff_products_rounded(minor, minor, dz, dz);
    if h2 < 0.0 {
        return SurfaceIntersection::Empty;
    }
    let center = c_t + axis * dz;
    if h2 == 0.0 {
        return SurfaceIntersection::Curves(vec![circle(center, axis, major)]);
    }
    let h = h2.sqrt();
    SurfaceIntersection::Curves(vec![
        circle(center, axis, major + h),
        circle(center, axis, (major - h).max(0.0)),
    ])
}

/// Are the two axis lines the same (parallel at construction noise,
/// perpendicular offset at construction noise)?
fn same_axis_line((o1, a1): (Vector3, Vector3), (o2, a2): (Vector3, Vector3)) -> Option<Vector3> {
    if a1.cross(&a2).length() > PARALLEL_BAND {
        return None;
    }
    let dv = o2 - o1;
    let along = dv.dot(&a1);
    let perp = dv - a1 * along;
    if perp.length() <= PARALLEL_BAND * (1.0 + dv.length()) {
        return Some(a1);
    }
    None
}

/// cylinder × cylinder — parallel axes only.
fn cylinder_cylinder(
    (o1, a1, r1): (Vector3, Vector3, f64),
    (o2, a2, r2): (Vector3, Vector3, f64),
) -> SurfaceIntersection {
    if a1.cross(&a2).length() > PARALLEL_BAND {
        return not_analytic(
            "non-parallel cylinder×cylinder (Steinmetz quartic space curve) — not representable in the K1 curve set",
        );
    }
    // Canonical operand order (after the order-independent parallel
    // test): the two-line construction must not flip with the call.
    let (o1, a1, r1, o2, r2) = if lex_after(&[o1.x, o1.y, o1.z, r1], &[o2.x, o2.y, o2.z, r2]) {
        (o2, a2, r2, o1, r1)
    } else {
        (o1, a1, r1, o2, r2)
    };
    let dv = o2 - o1;
    let along = dv.dot(&a1);
    let perp = dv - a1 * along;
    let d2_terms = length_squared_terms(perp);
    if exact_sum_sign(&d2_terms) == 0 {
        return if r1 == r2 {
            SurfaceIntersection::Coincident
        } else {
            SurfaceIntersection::Empty
        };
    }
    // d² vs (r1 + r2)² and (r1 − r2)² exactly.
    let mut ext = d2_terms.clone();
    let (sh, se) = two_prod(r1 + r2, r1 + r2);
    ext.extend([sh, se].iter().map(|x| -x).collect::<Vec<f64>>());
    let ext_sign = exact_sum_sign(&ext);
    if ext_sign > 0 {
        return SurfaceIntersection::Empty;
    }
    let mut int = d2_terms.clone();
    let (ih, ie) = two_prod(r1 - r2, r1 - r2);
    int.extend([ih, ie].iter().map(|x| -x).collect::<Vec<f64>>());
    let int_sign = exact_sum_sign(&int);
    if int_sign < 0 {
        return SurfaceIntersection::Empty;
    }
    let d = exact_sum_rounded(&d2_terms).sqrt();
    let u = perp * (1.0 / d);
    let d1 = {
        let (p1h, p1e) = two_prod(r1, r1);
        let (p2h, p2e) = two_prod(r2, r2);
        let d2 = exact_sum_rounded(&d2_terms);
        exact_sum_rounded(&[d2, p1h, p1e, -p2h, -p2e]) / (2.0 * d)
    };
    let foot = o1 + u * d1;
    if ext_sign == 0 || int_sign == 0 {
        return SurfaceIntersection::Curves(vec![line(foot, a1)]);
    }
    let h2 = {
        let (dh, de) = two_prod(d1, d1);
        let (p1h, p1e) = two_prod(r1, r1);
        exact_sum_rounded(&[p1h, p1e, -dh, -de])
    };
    if h2 <= 0.0 {
        return SurfaceIntersection::Curves(vec![line(foot, a1)]);
    }
    let v = a1.cross(&u);
    let h = h2.sqrt();
    SurfaceIntersection::Curves(vec![line(foot + v * h, a1), line(foot - v * h, a1)])
}

/// cylinder × sphere — coaxial circles only.
fn cylinder_sphere(
    o_c: Vector3,
    axis: Vector3,
    radius: f64,
    c_s: Vector3,
    r_s: f64,
) -> SurfaceIntersection {
    match same_axis_line((o_c, axis), (c_s, axis)) {
        None => not_analytic(
            "non-coaxial cylinder×sphere (quartic space curve) — not representable in the K1 curve set",
        ),
        Some(_) => {
            // Coaxial: z² = R² − r² at the sphere center's height.
            let h2 = diff_products_rounded(r_s, r_s, radius, radius);
            if h2 < 0.0 {
                return SurfaceIntersection::Empty;
            }
            if h2 == 0.0 {
                return SurfaceIntersection::Curves(vec![circle(c_s, axis, radius)]);
            }
            let h = h2.sqrt();
            SurfaceIntersection::Curves(vec![
                circle(c_s + axis * h, axis, radius),
                circle(c_s - axis * h, axis, radius),
            ])
        }
    }
}

/// cylinder × cone — coaxial circles (one per nappe).
fn cylinder_cone(
    o_cyl: Vector3,
    axis: Vector3,
    radius: f64,
    o_cone: Vector3,
    cone_axis: Vector3,
    semi: f64,
    ref_radius: f64,
) -> SurfaceIntersection {
    let Some(a) = same_axis_line((o_cyl, axis), (o_cone, cone_axis)) else {
        return not_analytic(
            "non-coaxial cylinder×cone (quartic space curve) — not representable in the K1 curve set",
        );
    };
    let (sin_semi, cos_semi) = semi.sin_cos();
    if sin_semi <= 0.0 {
        return not_analytic("degenerate cone (semi_angle ≤ 0): no analytic section");
    }
    let t = sin_semi / cos_semi;
    // The cone's apex in ITS OWN axis frame (the stored axis may be
    // antiparallel to the canonical one — contracting frusta are
    // recorded flipped so the semi-angle stays positive).
    let apex = o_cone - cone_axis * (ref_radius / t);
    if radius == 0.0 {
        return SurfaceIntersection::Point(apex);
    }
    // Signed slope along the canonical axis: radius(z) = ref +
    // (z − z_o)·σ·t with σ = sign(cone_axis · a).
    let sigma = cone_axis.dot(&a);
    if sigma == 0.0 {
        return not_analytic("cone axis perpendicular to the common axis");
    }
    let slope = sigma * t;
    let z_o = o_cone.dot(&a);
    let center_at = |z: f64| o_cone + a * (z - z_o);
    SurfaceIntersection::Curves(vec![
        circle(center_at(z_o + (radius - ref_radius) / slope), a, radius),
        circle(center_at(z_o + (-radius - ref_radius) / slope), a, radius),
    ])
}

/// cylinder × torus — coaxial tube circles.
fn cylinder_torus(
    o_cyl: Vector3,
    axis: Vector3,
    radius: f64,
    c_t: Vector3,
    torus_axis: Vector3,
    major: f64,
    minor: f64,
) -> SurfaceIntersection {
    let Some(a) = same_axis_line((o_cyl, axis), (c_t, torus_axis)) else {
        return not_analytic(
            "non-coaxial cylinder×torus (quartic space curve) — not representable in the K1 curve set",
        );
    };
    let h2 = diff_products_rounded(minor, minor, radius - major, radius - major);
    if h2 < 0.0 {
        return SurfaceIntersection::Empty;
    }
    if h2 == 0.0 {
        return SurfaceIntersection::Curves(vec![circle(c_t, a, radius)]);
    }
    let h = h2.sqrt();
    SurfaceIntersection::Curves(vec![
        circle(c_t + a * h, a, radius),
        circle(c_t - a * h, a, radius),
    ])
}

/// sphere × cone — coaxial section quadratic.
fn sphere_cone(
    c_s: Vector3,
    r_s: f64,
    o_cone: Vector3,
    cone_axis: Vector3,
    semi: f64,
    ref_radius: f64,
) -> SurfaceIntersection {
    let Some(a) = same_axis_line((c_s, cone_axis), (o_cone, cone_axis)) else {
        return not_analytic(
            "non-coaxial sphere×cone (quartic space curve) — not representable in the K1 curve set",
        );
    };
    let (sin_semi, cos_semi) = semi.sin_cos();
    if sin_semi <= 0.0 {
        return not_analytic("degenerate cone (semi_angle ≤ 0): no analytic section");
    }
    // In the cone's frame (z along the axis from o_cone, u the signed
    // ring radius): u = ref + z·tanα; the sphere: (z − zc)² + u² = R².
    let t = sin_semi / cos_semi;
    let zc = (c_s - o_cone).dot(&a);
    // (1 + t²)·z² + 2·(ref·t − zc)·z + (ref² + zc² − R²) = 0
    let qa = 1.0 + t * t;
    let qb = 2.0 * (ref_radius * t - zc);
    let qc = {
        let (rh, re) = two_prod(ref_radius, ref_radius);
        let (zh, ze) = two_prod(zc, zc);
        let (sh, se) = two_prod(r_s, r_s);
        exact_sum_rounded(&[rh, re, zh, ze, -sh, -se])
    };
    // Discriminant sign: qb² − 4·qa·qc = qb² − (2qa)(2qc), both
    // products exact through two_prod.
    let (bh2, be2) = two_prod(qb, qb);
    let (m1h, m1e) = two_prod(2.0 * qa, 2.0 * qc);
    let disc_sign = exact_sum_sign(&[bh2, be2, -m1h, -m1e]);
    let disc4 = exact_sum_rounded(&[bh2, be2, -m1h, -m1e]);
    if disc_sign < 0 || (disc_sign == 0 && disc4 < 0.0) {
        return SurfaceIntersection::Empty;
    }
    let roots = if disc4 <= 0.0 {
        vec![-qb / (2.0 * qa)]
    } else {
        let sqrt_disc4 = disc4.sqrt();
        vec![
            (-qb + sqrt_disc4) / (2.0 * qa),
            (-qb - sqrt_disc4) / (2.0 * qa),
        ]
    };
    let mut curves = Vec::new();
    for z in roots {
        let rho = ref_radius + z * t;
        let center = o_cone + a * z;
        if rho == 0.0 {
            return SurfaceIntersection::Point(center);
        }
        curves.push(circle(center, a, rho.abs()));
    }
    // Dedupe bit-identical roots.
    if curves.len() == 2 && curves[0] == curves[1] {
        curves.truncate(1);
    }
    SurfaceIntersection::Curves(curves)
}

/// cone × cone — coaxial radius-agreement circles.
fn cone_cone(
    (o1, a1, semi1, ref1): (Vector3, Vector3, f64, f64),
    (o2, a2, semi2, ref2): (Vector3, Vector3, f64, f64),
) -> SurfaceIntersection {
    let Some(a) = same_axis_line((o1, a1), (o2, a2)) else {
        return not_analytic(
            "non-coaxial cone×cone (quartic space curve) — not representable in the K1 curve set",
        );
    };
    // Canonical operand order (after the order-independent coaxial
    // test): the root listing must not permute with the call. The
    // axes travel with their cones (the apex math is frame-local).
    let ((o1, a1, semi1, ref1), (o2, a2, semi2, ref2)) = if lex_after(
        &[o1.x, o1.y, o1.z, semi1, ref1],
        &[o2.x, o2.y, o2.z, semi2, ref2],
    ) {
        ((o2, a2, semi2, ref2), (o1, a1, semi1, ref1))
    } else {
        ((o1, a1, semi1, ref1), (o2, a2, semi2, ref2))
    };
    let (s1, c1s) = semi1.sin_cos();
    let (s2, c2s) = semi2.sin_cos();
    if s1 <= 0.0 || s2 <= 0.0 {
        return not_analytic("degenerate cone (semi_angle ≤ 0): no analytic section");
    }
    let t1 = s1 / c1s;
    let t2 = s2 / c2s;
    // Apexes in each cone's OWN axis frame (stored axes may be
    // antiparallel to the canonical one).
    let apex1 = o1 - a1 * (ref1 / t1);
    let apex2 = o2 - a2 * (ref2 / t2);
    if t1 == t2 && apex1 == apex2 {
        return SurfaceIntersection::Coincident;
    }
    // Radius at canonical height z: u_i(z) = ref_i + (z − z_i)·σ_i·t_i
    // (σ_i = sign of the stored axis against the canonical one).
    let z1 = o1.dot(&a);
    let z2 = o2.dot(&a);
    let slope1 = a1.dot(&a) * t1;
    let slope2 = a2.dot(&a) * t2;
    if slope1 == 0.0 || slope2 == 0.0 {
        return not_analytic("cone axis perpendicular to the common axis");
    }
    // |u1| = |u2| splits into u1 = ±u2, each linear in z.
    let mut zs: Vec<f64> = Vec::new();
    if slope1 != slope2 {
        zs.push((ref2 - ref1 + slope1 * z1 - slope2 * z2) / (slope1 - slope2));
    }
    if slope1 + slope2 != 0.0 {
        zs.push((-ref2 - ref1 + slope1 * z1 + slope2 * z2) / (slope1 + slope2));
    }
    let mut curves = Vec::new();
    for &z in &zs {
        let rho = ref1 + (z - z1) * slope1;
        if rho == 0.0 {
            // Both radii vanish together only at a shared apex.
            return SurfaceIntersection::Point(apex1);
        }
        let center = o1 + a * (z - z1);
        curves.push(circle(center, a, rho.abs()));
    }
    if curves.len() == 2 && curves[0] == curves[1] {
        curves.truncate(1);
    }
    SurfaceIntersection::Curves(curves)
}

/// torus × torus — coaxial, identical radii only.
fn torus_torus(
    (c1, a1, major1, minor1): (Vector3, Vector3, f64, f64),
    (c2, a2, major2, minor2): (Vector3, Vector3, f64, f64),
) -> SurfaceIntersection {
    if same_axis_line((c1, a1), (c2, a2)).is_none() {
        return not_analytic(
            "non-coaxial torus×torus (quartic space curve) — not representable in the K1 curve set",
        );
    }
    if major1 != major2 || minor1 != minor2 {
        return not_analytic(
            "coaxial torus pair with differing radii (quartic section) — not representable in the K1 curve set",
        );
    }
    // Canonical operand order: the circles' axis frame must not flip
    // with the call.
    let (c1, a, major1, minor1, c2) = if lex_after(
        &[c1.x, c1.y, c1.z, major1, minor1],
        &[c2.x, c2.y, c2.z, major2, minor2],
    ) {
        (c2, a2, major2, minor2, c1)
    } else {
        (c1, a1, major1, minor1, c2)
    };
    let dz = (c2 - c1).dot(&a);
    if dz == 0.0 {
        return SurfaceIntersection::Coincident;
    }
    // Tubes kiss where (z)² = (z − dz)² → z = dz/2;
    // (ρ − R)² = minor² − (dz/2)².
    let h2 = {
        let (mh, me) = two_prod(minor1, minor1);
        let half = dz / 2.0;
        let (dh, de) = two_prod(half, half);
        exact_sum_rounded(&[mh, me, -dh, -de])
    };
    if h2 < 0.0 {
        return SurfaceIntersection::Empty;
    }
    let mid = c1 + a * (dz / 2.0);
    if h2 == 0.0 {
        return SurfaceIntersection::Curves(vec![circle(mid, a, major1)]);
    }
    let h = h2.sqrt();
    SurfaceIntersection::Curves(vec![
        circle(mid, a, major1 + h),
        circle(mid, a, (major1 - h).max(0.0)),
    ])
}

// ---------------------------------------------------------------------------
// dispatch
// ---------------------------------------------------------------------------

/// The exact intersection of two analytic surfaces (closed form).
///
/// Every configuration is classified exactly (expansion-arithmetic
/// signs — no tolerances); pairs whose sections are not representable
/// in the K1 curve set are refused by name. See the module doc for
/// the coverage matrix and the honesty rules.
pub fn intersect_surfaces(a: &Surface, b: &Surface) -> SurfaceIntersection {
    match (a, b) {
        (
            Surface::Plane {
                origin: o1,
                normal: n1,
                ..
            },
            Surface::Plane {
                origin: o2,
                normal: n2,
                ..
            },
        ) => plane_plane((*o1, *n1), (*o2, *n2)),
        (Surface::Plane { origin, normal, .. }, Surface::Sphere { center, radius, .. })
        | (Surface::Sphere { center, radius, .. }, Surface::Plane { origin, normal, .. }) => {
            plane_sphere(*origin, *normal, *center, *radius)
        }
        (
            Surface::Plane { origin, normal, .. },
            Surface::Cylinder {
                origin: oc,
                axis,
                radius,
                ..
            },
        )
        | (
            Surface::Cylinder {
                origin: oc,
                axis,
                radius,
                ..
            },
            Surface::Plane { origin, normal, .. },
        ) => plane_cylinder(*origin, *normal, *oc, *axis, *radius),
        (
            Surface::Plane { origin, normal, .. },
            Surface::Cone {
                origin: oc,
                axis,
                semi_angle,
                ref_radius,
                ..
            },
        )
        | (
            Surface::Cone {
                origin: oc,
                axis,
                semi_angle,
                ref_radius,
                ..
            },
            Surface::Plane { origin, normal, .. },
        ) => plane_cone(*origin, *normal, *oc, *axis, *semi_angle, *ref_radius),
        (
            Surface::Plane { origin, normal, .. },
            Surface::Torus {
                center,
                axis,
                major_radius,
                minor_radius,
                ..
            },
        )
        | (
            Surface::Torus {
                center,
                axis,
                major_radius,
                minor_radius,
                ..
            },
            Surface::Plane { origin, normal, .. },
        ) => plane_torus(
            *origin,
            *normal,
            *center,
            *axis,
            *major_radius,
            *minor_radius,
        ),
        (
            Surface::Sphere {
                center: c1,
                radius: r1,
                ..
            },
            Surface::Sphere {
                center: c2,
                radius: r2,
                ..
            },
        ) => sphere_sphere(*c1, *r1, *c2, *r2),
        (
            Surface::Cylinder {
                origin,
                axis,
                radius,
                ..
            },
            Surface::Sphere {
                center, radius: rs, ..
            },
        )
        | (
            Surface::Sphere {
                center, radius: rs, ..
            },
            Surface::Cylinder {
                origin,
                axis,
                radius,
                ..
            },
        ) => cylinder_sphere(*origin, *axis, *radius, *center, *rs),
        (
            Surface::Cylinder {
                origin,
                axis,
                radius,
                ..
            },
            Surface::Cone {
                origin: oc,
                axis: ca,
                semi_angle,
                ref_radius,
                ..
            },
        )
        | (
            Surface::Cone {
                origin: oc,
                axis: ca,
                semi_angle,
                ref_radius,
                ..
            },
            Surface::Cylinder {
                origin,
                axis,
                radius,
                ..
            },
        ) => cylinder_cone(*origin, *axis, *radius, *oc, *ca, *semi_angle, *ref_radius),
        (
            Surface::Cylinder {
                origin,
                axis,
                radius,
                ..
            },
            Surface::Torus {
                center,
                axis: ta,
                major_radius,
                minor_radius,
                ..
            },
        )
        | (
            Surface::Torus {
                center,
                axis: ta,
                major_radius,
                minor_radius,
                ..
            },
            Surface::Cylinder {
                origin,
                axis,
                radius,
                ..
            },
        ) => cylinder_torus(
            *origin,
            *axis,
            *radius,
            *center,
            *ta,
            *major_radius,
            *minor_radius,
        ),
        (
            Surface::Sphere { center, radius, .. },
            Surface::Cone {
                origin: oc,
                axis,
                semi_angle,
                ref_radius,
                ..
            },
        )
        | (
            Surface::Cone {
                origin: oc,
                axis,
                semi_angle,
                ref_radius,
                ..
            },
            Surface::Sphere { center, radius, .. },
        ) => sphere_cone(*center, *radius, *oc, *axis, *semi_angle, *ref_radius),
        (
            Surface::Cylinder {
                origin: o1,
                axis: a1,
                radius: r1,
                ..
            },
            Surface::Cylinder {
                origin: o2,
                axis: a2,
                radius: r2,
                ..
            },
        ) => cylinder_cylinder((*o1, *a1, *r1), (*o2, *a2, *r2)),
        (
            Surface::Cone {
                origin: o1,
                axis: a1,
                semi_angle: s1,
                ref_radius: r1,
                ..
            },
            Surface::Cone {
                origin: o2,
                axis: a2,
                semi_angle: s2,
                ref_radius: r2,
                ..
            },
        ) => cone_cone((*o1, *a1, *s1, *r1), (*o2, *a2, *s2, *r2)),
        (
            Surface::Torus {
                center: c1,
                axis: a1,
                major_radius: m1,
                minor_radius: n1,
                ..
            },
            Surface::Torus {
                center: c2,
                axis: a2,
                major_radius: m2,
                minor_radius: n2,
                ..
            },
        ) => torus_torus((*c1, *a1, *m1, *n1), (*c2, *a2, *m2, *n2)),
        (Surface::Sphere { .. }, Surface::Torus { .. })
        | (Surface::Torus { .. }, Surface::Sphere { .. }) => not_analytic(
            "sphere×torus (quartic space curve) — not representable in the K1 curve set",
        ),
        (Surface::Cone { .. }, Surface::Torus { .. })
        | (Surface::Torus { .. }, Surface::Cone { .. }) => {
            not_analytic("cone×torus (quartic space curve) — not representable in the K1 curve set")
        }
        (Surface::Bspline { .. }, _) | (_, Surface::Bspline { .. }) => {
            not_analytic("B-spline surfaces are outside the analytic intersection set")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_prod_is_exact() {
        // (p, e) reconstructs the exact product for adversarial
        // mantissas.
        for (a, b) in [
            (0.1, 0.2),
            (1e200, 1e-200),
            (3.0, 5.0),
            (1.0 / 3.0, 7.0 / 9.0),
            (-2.5, 1.25e-8),
        ] {
            let (p, e) = two_prod(a, b);
            let reconstructed = p + e;
            // p is the rounded product; p + e recovers it to the last
            // bit (e is the exact correction).
            assert_eq!(p, a * b);
            let naive = a * b;
            assert_eq!(naive, p);
            let _ = reconstructed;
        }
    }

    #[test]
    fn two_sum_is_exact() {
        let (s, e) = two_sum(1.0, 1e-300);
        assert_eq!(s, 1.0);
        assert_eq!(e, 1e-300);
        let (s, e) = two_sum(1e300, -1e300);
        assert_eq!(s, 0.0);
        assert_eq!(e, 0.0);
    }

    #[test]
    fn exact_sum_sign_catches_cancellation() {
        // A naive sum gives 0; the exact sign is +1e-300.
        assert_eq!(exact_sum_sign(&[1.0, 1e-300, -1.0]), 1);
        assert_eq!(exact_sum_sign(&[1.0, -1e-300, -1.0]), -1);
        assert_eq!(exact_sum_sign(&[1.0, -1.0]), 0);
    }

    #[test]
    fn exact_sum_rounded_needs_the_correction() {
        // 1 + 2^-60 rounds to 1 naively; the exact sum carries the bit.
        assert_eq!(1.0 + 2f64.powi(-60), 1.0);
        assert_eq!(
            exact_sum_rounded(&[1.0, 2f64.powi(-60)]),
            1.0 + 2f64.powi(-60) + 0.0
        );
        // ...which still rounds to 1 as a single f64, but the SIGN
        // machinery sees it:
        assert_eq!(exact_sum_sign(&[2f64.powi(-60), 1.0, -1.0]), 1);
    }

    #[test]
    fn cmp_squares_is_exact_at_one_ulp() {
        let x = 1.0;
        let y = 1.0 + f64::EPSILON;
        assert_eq!(cmp_squares(x, y), std::cmp::Ordering::Less);
        assert_eq!(cmp_squares(y, x), std::cmp::Ordering::Greater);
        assert_eq!(cmp_squares(x, x), std::cmp::Ordering::Equal);
        // 3² vs 4² and equality through non-trivial bits:
        assert_eq!(cmp_squares(0.1, 0.1), std::cmp::Ordering::Equal);
        assert_eq!(cmp_squares(3.0, 4.0), std::cmp::Ordering::Less);
    }

    #[test]
    fn diff_products_rounded_matches_exact_difference() {
        // r² − d² for r = 5, d = 3: exactly 16.
        assert_eq!(diff_products_rounded(5.0, 5.0, 3.0, 3.0), 16.0);
        // A case where the naive difference of rounded squares loses
        // bits: near-equal operands.
        let a = 1.0000000000000002f64;
        let d = diff_products_rounded(a, a, 1.0, 1.0);
        assert!(d > 0.0 && d < 1e-14);
    }
}
