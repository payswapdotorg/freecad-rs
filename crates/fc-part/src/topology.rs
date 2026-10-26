//! K1 — the exact topology model: vertices, edges and faces carrying
//! parametric curves and surfaces.
//!
//! This is the kernel's BRep layer (FreeCAD/OCCT `TopoDS_*` +
//! `BRep_TFace/TEdge/TVertex`, distilled to an exact core). The mesh
//! pipeline ([`crate::shape`]) stays the tessellated representation;
//! this module carries the *analytic* structure that exact
//! intersections (K3) will operate on:
//!
//! * [`Curve`] — line, circle, ellipse, B-spline (rational allowed)
//! * [`Surface`] — plane, cylinder, cone, sphere, torus, B-spline
//!   surface (rational allowed)
//! * [`ExactBRep`] — the topology: vertices, edges (curve + parameter
//!   range + vertex bounds), faces (surface + UV bounds + wires)
//!
//! # Conventions (ours — asserted by `tests/topology.rs`)
//!
//! * A **closed** edge (a full circle) carries ONE vertex:
//!   `start == end == v`.
//! * A **degenerate** edge maps its whole parameter range to a single
//!   point — a `Circle { radius: 0 }` (sphere poles, plain-cone apex)
//!   or a zero-direction line. Degenerate edges are excluded from the
//!   Euler count: `chi = V − E_nondegenerate + F`.
//! * Edge endpoints are **bit-exact** against their vertex table:
//!   `curve.point(range.0) == vertices[start].point`. The generators
//!   below build vertices *from* curve evaluations, so this holds by
//!   construction; [`ExactBRep::validate`] enforces it for everyone.
//! * Wires are closed vertex loops; a face with no wires is an
//!   untrimmed (e.g. fully periodic) surface.
//! * Surface parameterizations follow the analytic kinds' frames
//!   (`axis` the pole direction, `ref_dir` the u = 0 direction, the
//!   implicit second direction `axis × ref_dir`):
//!   * plane: `p(u,v) = origin + u·x + v·(n × x)`
//!   * cylinder: `p(u,v) = origin + r·ring(u) + v·axis`
//!   * cone (OCCT `Geom_ConicalSurface`): `p(u,v) = origin +
//!     (ref_radius + v·sin(semi))·ring(u) + v·cos(semi)·axis`
//!   * sphere (OCCT latitude convention): `p(u,v) = center + r·(cos v·ring(u) + sin v·axis)`, `v ∈ [−π/2, π/2]`
//!   * torus: `p(u,v) = center + (R + r·cos v)·ring(u) +
//!     r·sin v·axis`
//!
//!   where `ring(u) = cos u·ref_dir + sin u·(axis × ref_dir)`.
//!
//! # Exactness limits, honestly
//!
//! The generators share vertices across edges only when the shared
//! points unify bit-exactly. For axis-aligned analytic geometry with
//! exactly-representable dimensions (every committed fixture) they
//! do, by the arithmetic of `0`-additions and `1`-multiplications;
//! the sphere's poles carry trigonometry's last-ulp noise by
//! construction (the degenerate pole circles are centered on the
//! seam's derived endpoints, so the topology stays bit-consistent).
//! A tolerance-based snapping doctrine is deliberately deferred to
//! K3, where computed intersection endpoints force the question.

use fc_base::Vector3;

use crate::shape::TopoShape;

/// `τ = 2π` — the full-turn parameter range.
pub const TAU: f64 = std::f64::consts::TAU;

/// `π/2` — the sphere's colatitude bounds.
pub const FRAC_PI_2: f64 = std::f64::consts::FRAC_PI_2;

// ---------------------------------------------------------------------------
// Curves
// ---------------------------------------------------------------------------

/// A parametric 3D curve.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// `p(t) = origin + t · direction`.
    Line { origin: Vector3, direction: Vector3 },
    /// `p(t) = center + radius · (cos t · ref_dir + sin t · (axis × ref_dir))`.
    /// `radius == 0` is the legal degenerate case (a point-circle: the
    /// sphere poles and cone apex use it).
    Circle {
        center: Vector3,
        axis: Vector3,
        ref_dir: Vector3,
        radius: f64,
    },
    /// `p(t) = center + major · cos t · major_dir + minor · sin t · (axis × major_dir)`.
    Ellipse {
        center: Vector3,
        axis: Vector3,
        major_dir: Vector3,
        major_radius: f64,
        minor_radius: f64,
    },
    /// Clamped B-spline; `knots.len() == points.len() + degree + 1`,
    /// non-decreasing. Rational when `weights` is `Some` (all > 0).
    Bspline {
        degree: usize,
        knots: Vec<f64>,
        points: Vec<Vector3>,
        weights: Option<Vec<f64>>,
    },
}

impl Curve {
    /// `"Line"`, `"Circle"`, `"Ellipse"`, `"Bspline"`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Curve::Line { .. } => "Line",
            Curve::Circle { .. } => "Circle",
            Curve::Ellipse { .. } => "Ellipse",
            Curve::Bspline { .. } => "Bspline",
        }
    }

    /// Evaluate at parameter `t`.
    pub fn point(&self, t: f64) -> Vector3 {
        match self {
            Curve::Line { origin, direction } => *origin + *direction * t,
            Curve::Circle {
                center,
                axis,
                ref_dir,
                radius,
            } => {
                let y = axis.cross(ref_dir);
                let (s, c) = t.sin_cos();
                *center + (*ref_dir * (radius * c) + y * (radius * s))
            }
            Curve::Ellipse {
                center,
                axis,
                major_dir,
                major_radius,
                minor_radius,
            } => {
                let n = axis.cross(major_dir);
                let (s, c) = t.sin_cos();
                *center + (*major_dir * (major_radius * c) + n * (minor_radius * s))
            }
            Curve::Bspline {
                degree,
                knots,
                points,
                weights,
            } => match weights {
                None => bspline_eval(*degree, knots, points, t),
                Some(w) => {
                    let weighted: Vec<Vector3> =
                        points.iter().zip(w).map(|(&p, &w)| p * w).collect();
                    let num = bspline_eval(*degree, knots, &weighted, t);
                    let den = bspline_eval_f64(*degree, knots, w, t);
                    num * (1.0 / den)
                }
            },
        }
    }

    /// Evaluate `dp/dt`.
    ///
    /// Lines: the constant direction. Circles/ellipses: the analytic
    /// derivative. B-splines: the derivative spline (degree − 1,
    /// control points `p·(P[i+1] − P[i]) / (knots[i+p+1] − knots[i+1])`),
    /// with the rational quotient rule when weighted.
    pub fn derivative(&self, t: f64) -> Vector3 {
        match self {
            Curve::Line { direction, .. } => *direction,
            Curve::Circle {
                axis,
                ref_dir,
                radius,
                ..
            } => {
                let y = axis.cross(ref_dir);
                let (s, c) = t.sin_cos();
                *ref_dir * (-*radius * s) + y * (*radius * c)
            }
            Curve::Ellipse {
                axis,
                major_dir,
                major_radius,
                minor_radius,
                ..
            } => {
                let n = axis.cross(major_dir);
                let (s, c) = t.sin_cos();
                *major_dir * (-*major_radius * s) + n * (*minor_radius * c)
            }
            Curve::Bspline {
                degree,
                knots,
                points,
                weights,
            } => {
                let (dknots, dnum, dden) =
                    derivative_ctrl(*degree, knots, points, weights.as_deref());
                match (weights, dden) {
                    (None, _) => bspline_eval(degree - 1, &dknots, &dnum, t),
                    (Some(w), Some(dd)) => {
                        let num = self.point(t);
                        let den = bspline_eval_f64(*degree, knots, w, t);
                        let num_d = bspline_eval(degree - 1, &dknots, &dnum, t);
                        let den_d = bspline_eval_f64(degree - 1, &dknots, &dd, t);
                        (num_d * den - num * den_d) * (1.0 / (den * den))
                    }
                    (Some(_), None) => unreachable!("rational always yields a denominator"),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------

/// A parametric surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    /// `p(u,v) = origin + u·x_dir + v·(normal × x_dir)`.
    Plane {
        origin: Vector3,
        normal: Vector3,
        x_dir: Vector3,
    },
    /// `p(u,v) = origin + radius·ring(u) + v·axis` — `u` radians, `v`
    /// length along the axis.
    Cylinder {
        origin: Vector3,
        axis: Vector3,
        ref_dir: Vector3,
        radius: f64,
    },
    /// OCCT `Geom_ConicalSurface`: `p(u,v) = origin + (ref_radius +
    /// v·sin(semi_angle))·ring(u) + v·cos(semi_angle)·axis` — `v` is
    /// distance along the surface (slant), so a frustum of height `h`
    /// spans `v ∈ [0, h / cos(semi)]`.
    Cone {
        origin: Vector3,
        axis: Vector3,
        ref_dir: Vector3,
        semi_angle: f64,
        ref_radius: f64,
    },
    /// `p(u,v) = center + radius·(cos v·axis + sin v·ring(u))` with
    /// `v ∈ [−π/2, π/2]` the colatitude.
    Sphere {
        center: Vector3,
        axis: Vector3,
        ref_dir: Vector3,
        radius: f64,
    },
    /// `p(u,v) = center + (major_radius + minor_radius·cos v)·ring(u) +
    /// minor_radius·sin v·axis` — both parameters full turns.
    Torus {
        center: Vector3,
        axis: Vector3,
        ref_dir: Vector3,
        major_radius: f64,
        minor_radius: f64,
    },
    /// Tensor-product B-spline surface over a `points[u][v]` control
    /// net; rational when `weights[u][v]` is `Some`.
    Bspline {
        u_degree: usize,
        v_degree: usize,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        points: Vec<Vec<Vector3>>,
        weights: Option<Vec<Vec<f64>>>,
    },
}

impl Surface {
    /// `"Plane"`, `"Cylinder"`, `"Cone"`, `"Sphere"`, `"Torus"`, `"Bspline"`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "Plane",
            Surface::Cylinder { .. } => "Cylinder",
            Surface::Cone { .. } => "Cone",
            Surface::Sphere { .. } => "Sphere",
            Surface::Torus { .. } => "Torus",
            Surface::Bspline { .. } => "Bspline",
        }
    }

    /// Evaluate at surface parameters `(u, v)`.
    pub fn point(&self, u: f64, v: f64) -> Vector3 {
        match self {
            Surface::Plane {
                origin,
                normal,
                x_dir,
            } => {
                let y = normal.cross(x_dir);
                *origin + *x_dir * u + y * v
            }
            Surface::Cylinder {
                origin,
                axis,
                ref_dir,
                radius,
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                *origin + (*ref_dir * (radius * cu) + y * (radius * su)) + *axis * v
            }
            Surface::Cone {
                origin,
                axis,
                ref_dir,
                semi_angle,
                ref_radius,
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                let r = ref_radius + v * semi_angle.sin();
                *origin + (*ref_dir * (r * cu) + y * (r * su)) + *axis * (v * semi_angle.cos())
            }
            Surface::Sphere {
                center,
                axis,
                ref_dir,
                radius,
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let ring = *ref_dir * (radius * cu * cv) + y * (radius * su * cv);
                *center + ring + *axis * (radius * sv)
            }
            Surface::Torus {
                center,
                axis,
                ref_dir,
                major_radius,
                minor_radius,
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let ring = *ref_dir * cu + y * su;
                *center + ring * (major_radius + minor_radius * cv) + *axis * (minor_radius * sv)
            }
            Surface::Bspline {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                points,
                weights,
            } => match weights {
                None => eval_net(*u_degree, u_knots, *v_degree, v_knots, points, u, v),
                Some(w) => {
                    let mut wnum = vec![Vec::with_capacity(points[0].len()); points.len()];
                    for (i, row) in points.iter().enumerate() {
                        for (j, p) in row.iter().enumerate() {
                            wnum[i].push(*p * w[i][j]);
                        }
                    }
                    let num = eval_net(*u_degree, u_knots, *v_degree, v_knots, &wnum, u, v);
                    let den = eval_scalar_net(*u_degree, u_knots, *v_degree, v_knots, w, u, v);
                    num * (1.0 / den)
                }
            },
        }
    }

    /// Evaluate the partial derivatives `(∂p/∂u, ∂p/∂v)` at `(u, v)`.
    pub fn derivatives(&self, u: f64, v: f64) -> (Vector3, Vector3) {
        match self {
            Surface::Plane { normal, x_dir, .. } => (*x_dir, normal.cross(x_dir)),
            Surface::Cylinder {
                axis,
                ref_dir,
                radius,
                ..
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                (*ref_dir * (-*radius * su) + y * (*radius * cu), *axis)
            }
            Surface::Cone {
                axis,
                ref_dir,
                semi_angle,
                ref_radius,
                ..
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                let r = ref_radius + v * semi_angle.sin();
                let du = *ref_dir * (-r * su) + y * (r * cu);
                let dv = (*ref_dir * (semi_angle.sin() * cu) + y * (semi_angle.sin() * su))
                    + *axis * semi_angle.cos();
                (du, dv)
            }
            Surface::Sphere {
                axis,
                ref_dir,
                radius,
                ..
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let du = *ref_dir * (-radius * su * cv) + y * (radius * cu * cv);
                let dv = *ref_dir * (-radius * cu * sv)
                    + y * (-radius * su * sv)
                    + *axis * (radius * cv);
                (du, dv)
            }
            Surface::Torus {
                axis,
                ref_dir,
                major_radius,
                minor_radius,
                ..
            } => {
                let y = axis.cross(ref_dir);
                let (su, cu) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let r = major_radius + minor_radius * cv;
                let du = *ref_dir * (-r * su) + y * (r * cu);
                let dv = *ref_dir * (-minor_radius * cu * sv)
                    + y * (-minor_radius * su * sv)
                    + *axis * (minor_radius * cv);
                (du, dv)
            }
            Surface::Bspline {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                points,
                weights,
            } => {
                // du: the u-derivative control net (degree u−1, trimmed
                // u-knots), evaluated at (u, v).
                let du_net = derivative_net_u(*u_degree, u_knots, points, None, false);
                let du = eval_net(
                    u_degree - 1,
                    &u_knots[1..u_knots.len() - 1],
                    *v_degree,
                    v_knots,
                    &du_net,
                    u,
                    v,
                );
                // dv: the v-derivative net (degree v−1, trimmed v-knots).
                let mut dv_net = Vec::with_capacity(points.len());
                for row in points {
                    let (_, dnum, _) = derivative_ctrl(*v_degree, v_knots, row, None);
                    dv_net.push(dnum);
                }
                let dv = eval_net(
                    *u_degree,
                    u_knots,
                    v_degree - 1,
                    &v_knots[1..v_knots.len() - 1],
                    &dv_net,
                    u,
                    v,
                );
                if let Some(w) = weights {
                    // Rational: the quotient rule in both parameters.
                    // p = num/den over the weighted numerator.
                    let num = self.point(u, v);
                    let den = eval_scalar_net(*u_degree, u_knots, *v_degree, v_knots, w, u, v);
                    // u-direction: numerator and denominator derivatives.
                    let wnum_net = derivative_net_u(*u_degree, u_knots, points, Some(w), true);
                    let num_u = eval_net(
                        u_degree - 1,
                        &u_knots[1..u_knots.len() - 1],
                        *v_degree,
                        v_knots,
                        &wnum_net,
                        u,
                        v,
                    );
                    // den_u: the u-derivative of the weight net.
                    let mut wden_net = Vec::with_capacity(w.len() - 1);
                    for i in 0..w.len() - 1 {
                        let denom = u_knots[i + u_degree + 1] - u_knots[i + 1];
                        let scale = if denom > 0.0 {
                            *u_degree as f64 / denom
                        } else {
                            0.0
                        };
                        let row: Vec<f64> = w[i + 1]
                            .iter()
                            .zip(&w[i])
                            .map(|(a, b)| (a - b) * scale)
                            .collect();
                        wden_net.push(row);
                    }
                    let den_u = eval_scalar_net(
                        u_degree - 1,
                        &u_knots[1..u_knots.len() - 1],
                        *v_degree,
                        v_knots,
                        &wden_net,
                        u,
                        v,
                    );
                    let du = (num_u * den - num * den_u) * (1.0 / (den * den));
                    // v-direction.
                    let mut num_v_net = Vec::with_capacity(points.len());
                    let mut den_v_net = Vec::with_capacity(points.len());
                    for (row, wrow) in points.iter().zip(w) {
                        let mut nrow = Vec::with_capacity(row.len() - 1);
                        let mut drow = Vec::with_capacity(row.len() - 1);
                        for j in 0..row.len() - 1 {
                            let denom = v_knots[j + v_degree + 1] - v_knots[j + 1];
                            let scale = if denom > 0.0 {
                                *v_degree as f64 / denom
                            } else {
                                0.0
                            };
                            nrow.push((row[j + 1] * wrow[j + 1] - row[j] * wrow[j]) * scale);
                            drow.push((wrow[j + 1] - wrow[j]) * scale);
                        }
                        num_v_net.push(nrow);
                        den_v_net.push(drow);
                    }
                    let num_v = eval_net(
                        *u_degree,
                        u_knots,
                        v_degree - 1,
                        &v_knots[1..v_knots.len() - 1],
                        &num_v_net,
                        u,
                        v,
                    );
                    let den_v = eval_scalar_net(
                        *u_degree,
                        u_knots,
                        v_degree - 1,
                        &v_knots[1..v_knots.len() - 1],
                        &den_v_net,
                        u,
                        v,
                    );
                    let dv = (num_v * den - num * den_v) * (1.0 / (den * den));
                    (du, dv)
                } else {
                    (du, dv)
                }
            }
        }
    }

    /// Unit normal `∂p/∂u × ∂p/∂v`, normalized. At degenerate
    /// parameter points (poles, apexes) the cross product can be the
    /// zero vector, which cannot be normalized — the zero vector is
    /// returned honestly and callers must avoid those points.
    pub fn normal(&self, u: f64, v: f64) -> Vector3 {
        let (du, dv) = self.derivatives(u, v);
        let n = du.cross(&dv);
        n.normalized().unwrap_or(n)
    }
}

// ---------------------------------------------------------------------------
// B-spline machinery (de Boor)
// ---------------------------------------------------------------------------

/// De Boor evaluation of a non-rational B-spline curve at `t` (the
/// NURBS Book A2.2 recursion; exact `0`/`1` weights at clamped ends,
/// so the domain endpoints hit their control points bit-exactly).
fn bspline_eval(degree: usize, knots: &[f64], pts: &[Vector3], t: f64) -> Vector3 {
    let ncp = pts.len();
    if ncp == 0 {
        return Vector3::ZERO;
    }
    let mut k = degree;
    while k < ncp - 1 && t >= knots[k + 1] {
        k += 1;
    }
    let mut d: Vec<Vector3> = pts[k - degree..=k].to_vec();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let lo = knots[j + k - degree];
            let hi = knots[j + k - r + 1];
            let alpha = if hi > lo { (t - lo) / (hi - lo) } else { 0.0 };
            d[j] = d[j - 1] * (1.0 - alpha) + d[j] * alpha;
        }
    }
    d[degree]
}

/// Scalar twin of [`bspline_eval`] (rational denominators, weight
/// nets).
fn bspline_eval_f64(degree: usize, knots: &[f64], vals: &[f64], t: f64) -> f64 {
    let ncp = vals.len();
    if ncp == 0 {
        return 0.0;
    }
    let mut k = degree;
    while k < ncp - 1 && t >= knots[k + 1] {
        k += 1;
    }
    let mut d: Vec<f64> = vals[k - degree..=k].to_vec();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let lo = knots[j + k - degree];
            let hi = knots[j + k - r + 1];
            let alpha = if hi > lo { (t - lo) / (hi - lo) } else { 0.0 };
            d[j] = d[j - 1] * (1.0 - alpha) + d[j] * alpha;
        }
    }
    d[degree]
}

/// Evaluate a (non-rational) control net: each u-row's v-spline at
/// `v`, then the resulting u-spline at `u`.
fn eval_net(
    u_degree: usize,
    u_knots: &[f64],
    v_degree: usize,
    v_knots: &[f64],
    net: &[Vec<Vector3>],
    u: f64,
    v: f64,
) -> Vector3 {
    let rows: Vec<Vector3> = net
        .iter()
        .map(|row| bspline_eval(v_degree, v_knots, row, v))
        .collect();
    bspline_eval(u_degree, u_knots, &rows, u)
}

/// Scalar-net twin of [`eval_net`].
fn eval_scalar_net(
    u_degree: usize,
    u_knots: &[f64],
    v_degree: usize,
    v_knots: &[f64],
    net: &[Vec<f64>],
    u: f64,
    v: f64,
) -> f64 {
    let rows: Vec<f64> = net
        .iter()
        .map(|row| bspline_eval_f64(v_degree, v_knots, row, v))
        .collect();
    bspline_eval_f64(u_degree, u_knots, &rows, u)
}

/// The derivative control data of a curve spline:
/// `(trimmed knots, numerator control points, denominator control
/// values)`. The numerator carries `w·P` differences when rational
/// (callers apply the quotient rule with the denominator values);
/// plain point differences otherwise.
fn derivative_ctrl(
    degree: usize,
    knots: &[f64],
    points: &[Vector3],
    weights: Option<&[f64]>,
) -> (Vec<f64>, Vec<Vector3>, Option<Vec<f64>>) {
    let ncp = points.len();
    let dknots: Vec<f64> = knots[1..knots.len() - 1].to_vec();
    let mut dnum = Vec::with_capacity(ncp.saturating_sub(1));
    let mut dden = weights.map(|_| Vec::with_capacity(ncp - 1));
    for i in 0..ncp - 1 {
        let denom = knots[i + degree + 1] - knots[i + 1];
        let scale = if denom > 0.0 {
            degree as f64 / denom
        } else {
            0.0
        };
        match weights {
            Some(w) => {
                dnum.push((points[i + 1] * w[i + 1] - points[i] * w[i]) * scale);
                if let Some(d) = dden.as_mut() {
                    d.push((w[i + 1] - w[i]) * scale);
                }
            }
            None => dnum.push((points[i + 1] - points[i]) * scale),
        }
    }
    (dknots, dnum, dden)
}

/// The u-direction derivative control net of a surface: one row per
/// u-index, `degree_u` scaled differences. With `weighted = true`
/// the numerator rows carry `w·P` differences (for the rational
/// quotient rule).
fn derivative_net_u(
    u_degree: usize,
    u_knots: &[f64],
    net: &[Vec<Vector3>],
    weights: Option<&[Vec<f64>]>,
    weighted: bool,
) -> Vec<Vec<Vector3>> {
    let npu = net.len();
    let npv = net[0].len();
    let mut out = Vec::with_capacity(npu - 1);
    for i in 0..npu - 1 {
        let denom = u_knots[i + u_degree + 1] - u_knots[i + 1];
        let scale = if denom > 0.0 {
            u_degree as f64 / denom
        } else {
            0.0
        };
        let mut row = Vec::with_capacity(npv);
        for j in 0..npv {
            let d = if weighted {
                let w = weights.expect("weighted needs weights");
                net[i + 1][j] * w[i + 1][j] - net[i][j] * w[i][j]
            } else {
                net[i + 1][j] - net[i][j]
            };
            row.push(d * scale);
        }
        out.push(row);
    }
    out
}

// ---------------------------------------------------------------------------
// Topology
// ---------------------------------------------------------------------------

/// A topology vertex: a 3D point.
#[derive(Debug, Clone, PartialEq)]
pub struct TopoVertex {
    pub point: Vector3,
}

/// A topology edge: a parametric curve over a parameter range, bounded
/// by vertices. A **closed** edge (full circle) carries one vertex
/// (`start == end`).
#[derive(Debug, Clone, PartialEq)]
pub struct TopoEdge {
    pub curve: Curve,
    /// `range.0 < range.1`; the active parameter interval.
    pub range: (f64, f64),
    pub start: usize,
    pub end: usize,
}

/// One edge use inside a wire: the edge index and its traversal
/// orientation. `forward = false` traverses the curve backwards
/// (from `end` to `start`).
#[derive(Debug, Clone, PartialEq)]
pub struct WireEdge {
    pub edge: usize,
    pub forward: bool,
}

/// An ordered, closed loop of oriented edge uses.
#[derive(Debug, Clone, PartialEq)]
pub struct TopoWire {
    pub edges: Vec<WireEdge>,
}

/// A topology face: a parametric surface trimmed by UV bounds and
/// wires. `wires[0]` is the outer boundary, the rest are holes; a face
/// with no wires is an untrimmed (fully periodic) surface.
#[derive(Debug, Clone, PartialEq)]
pub struct TopoFace {
    pub surface: Surface,
    /// `[umin, umax, vmin, vmax]`.
    pub uv_bounds: [f64; 4],
    pub wires: Vec<TopoWire>,
}

/// The exact boundary representation: vertices, edges, faces.
///
/// Built from the analytic primitives via [`ExactBRep::from_shape`],
/// or by hand (then run [`ExactBRep::validate`]). Serialized by
/// [`crate::brep::write_brep`] as FCBREP v2.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExactBRep {
    pub vertices: Vec<TopoVertex>,
    pub edges: Vec<TopoEdge>,
    pub faces: Vec<TopoFace>,
}

impl ExactBRep {
    /// The exact topology of an analytic primitive. Mesh shapes have
    /// no exact topology — that is the honest refusal.
    pub fn from_shape(shape: &TopoShape) -> Result<ExactBRep, String> {
        match shape {
            TopoShape::Box {
                length,
                width,
                height,
            } => Ok(box_topology(*length, *width, *height)),
            TopoShape::Sphere { radius } => Ok(sphere_topology(*radius)),
            TopoShape::Cylinder { radius, height } => Ok(cylinder_topology(*radius, *height)),
            TopoShape::Cone {
                radius1,
                radius2,
                height,
            } => Ok(cone_topology(*radius1, *radius2, *height)),
            TopoShape::Torus { radius1, radius2 } => Ok(torus_topology(*radius1, *radius2)),
            TopoShape::Mesh(_) => Err(
                "mesh shapes carry no exact topology — the exact model covers the analytic primitives".to_string(),
            ),
        }
    }

    /// How many times face wires reference edge `e`.
    pub fn edge_uses(&self, edge: usize) -> usize {
        self.faces
            .iter()
            .flat_map(|f| &f.wires)
            .flat_map(|w| &w.edges)
            .filter(|we| we.edge == edge)
            .count()
    }

    /// A degenerate edge maps its whole range to one point: a
    /// zero-radius circle (sphere poles, cone apex) or a zero-length
    /// line. Excluded from the Euler count.
    pub fn is_degenerate(&self, edge: usize) -> bool {
        match &self.edges[edge].curve {
            Curve::Circle { radius, .. } => *radius == 0.0,
            Curve::Line { direction, .. } => {
                direction.x == 0.0 && direction.y == 0.0 && direction.z == 0.0
            }
            _ => false,
        }
    }

    /// `V − E_nondegenerate + F` — the Euler characteristic of the
    /// shell, with degenerate (zero-length) edges excluded.
    pub fn euler_characteristic(&self) -> i64 {
        let non_degenerate = (0..self.edges.len())
            .filter(|&e| !self.is_degenerate(e))
            .count();
        self.vertices.len() as i64 - non_degenerate as i64 + self.faces.len() as i64
    }

    /// Full structural validation:
    ///
    /// * finite geometry everywhere; positive radii where required;
    /// * B-spline knot/weight bookkeeping, ranges inside the domain;
    /// * `range.0 < range.1`; vertex indices in range;
    /// * **bit-exact** edge endpoints against the vertex table;
    /// * plane frames non-degenerate (normal and x_dir not parallel);
    /// * UV bounds strictly increasing;
    /// * wires are closed, vertex-continuous loops (closed edges
    ///   touch at their single vertex).
    pub fn validate(&self) -> Result<(), String> {
        for (i, v) in self.vertices.iter().enumerate() {
            if !v.point.x.is_finite() || !v.point.y.is_finite() || !v.point.z.is_finite() {
                return Err(format!("vertex {i} has non-finite coordinates"));
            }
        }
        for (i, e) in self.edges.iter().enumerate() {
            validate_curve(&e.curve).map_err(|msg| format!("edge {i}: {msg}"))?;
            if e.range.0 >= e.range.1 {
                return Err(format!(
                    "edge {i}: parameter range must satisfy t0 < t1 (got {}..{})",
                    e.range.0, e.range.1
                ));
            }
            if let Curve::Bspline {
                degree,
                knots,
                points,
                ..
            } = &e.curve
            {
                let lo = knots[*degree];
                let hi = knots[points.len()];
                if e.range.0 < lo || e.range.1 > hi {
                    return Err(format!(
                        "edge {i}: parameter range {}..{} outside the B-spline domain {}..{}",
                        e.range.0, e.range.1, lo, hi
                    ));
                }
            }
            for (label, vi) in [("start", e.start), ("end", e.end)] {
                if vi >= self.vertices.len() {
                    return Err(format!(
                        "edge {i}: {label} vertex {vi} out of range ({} vertices)",
                        self.vertices.len()
                    ));
                }
            }
            // Bit-exact endpoints (the exactness discipline). A CLOSED
            // edge (`start == end`) is periodic: the parameter at
            // `range.1` denotes the same point as `range.0` (the
            // closure), so only the start evaluation defines the
            // vertex — full-turn trig is not bit-reproducible at
            // `2π`, and the model says so.
            let p0 = e.curve.point(e.range.0);
            if p0 != self.vertices[e.start].point {
                return Err(format!(
                    "edge {i}: curve at range start {p0:?} does not match vertex {} {:#?}",
                    e.start, self.vertices[e.start].point
                ));
            }
            if e.start != e.end {
                let p1 = e.curve.point(e.range.1);
                if p1 != self.vertices[e.end].point {
                    return Err(format!(
                        "edge {i}: curve at range end {p1:?} does not match vertex {} {:#?}",
                        e.end, self.vertices[e.end].point
                    ));
                }
            }
        }
        for (fi, f) in self.faces.iter().enumerate() {
            validate_surface(&f.surface).map_err(|msg| format!("face {fi}: {msg}"))?;
            let [umin, umax, vmin, vmax] = f.uv_bounds;
            if umin >= umax || vmin >= vmax {
                return Err(format!(
                    "face {fi}: UV bounds must be increasing, got u {umin}..{umax}, v {vmin}..{vmax}"
                ));
            }
            for (wi, wire) in f.wires.iter().enumerate() {
                if wire.edges.is_empty() {
                    return Err(format!("face {fi} wire {wi}: no edges"));
                }
                for we in &wire.edges {
                    if we.edge >= self.edges.len() {
                        return Err(format!(
                            "face {fi} wire {wi}: edge {} out of range ({} edges)",
                            we.edge,
                            self.edges.len()
                        ));
                    }
                }
            }
        }
        // Wire continuity (after every index is known valid).
        for (fi, f) in self.faces.iter().enumerate() {
            for (wi, wire) in f.wires.iter().enumerate() {
                let mut prev_end: Option<usize> = None;
                for (ei, we) in wire.edges.iter().enumerate() {
                    let edge = &self.edges[we.edge];
                    let (s, t) = if we.forward {
                        (edge.start, edge.end)
                    } else {
                        (edge.end, edge.start)
                    };
                    if let Some(pe) = prev_end {
                        if s != pe {
                            return Err(format!(
                                "face {fi} wire {wi}: edge-use {ei} (edge {}, {}) starts at vertex {s} but the previous edge-use ended at vertex {pe}",
                                we.edge,
                                if we.forward { "forward" } else { "reversed" }
                            ));
                        }
                    }
                    prev_end = Some(t);
                }
                let first = &wire.edges[0];
                let first_edge = &self.edges[first.edge];
                let first_start = if first.forward {
                    first_edge.start
                } else {
                    first_edge.end
                };
                if prev_end != Some(first_start) {
                    return Err(format!(
                        "face {fi} wire {wi}: not a closed loop (ends at vertex {prev_end:?}, started at {first_start})"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Curve-level structural rules (shared by validation and parsing).
pub(crate) fn validate_curve(c: &Curve) -> Result<(), String> {
    let finite3 = |v: &Vector3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    match c {
        Curve::Line { origin, direction } => {
            if !finite3(origin) || !finite3(direction) {
                return Err("non-finite line parameters".to_string());
            }
        }
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            if !finite3(center) || !finite3(axis) || !finite3(ref_dir) || !radius.is_finite() {
                return Err("non-finite circle parameters".to_string());
            }
            if *radius < 0.0 {
                return Err("circle radius must be >= 0".to_string());
            }
        }
        Curve::Ellipse {
            center,
            axis,
            major_dir,
            major_radius,
            minor_radius,
        } => {
            if !finite3(center) || !finite3(axis) || !finite3(major_dir) {
                return Err("non-finite ellipse parameters".to_string());
            }
            if !(major_radius.is_finite() && minor_radius.is_finite()) {
                return Err("non-finite ellipse radii".to_string());
            }
            if *major_radius <= 0.0 || *minor_radius <= 0.0 {
                return Err("ellipse radii must be > 0".to_string());
            }
        }
        Curve::Bspline {
            degree,
            knots,
            points,
            weights,
        } => {
            if *degree < 1 {
                return Err("B-spline degree must be >= 1".to_string());
            }
            if points.len() < *degree + 1 {
                return Err(format!(
                    "B-spline needs at least {} control points (has {})",
                    degree + 1,
                    points.len()
                ));
            }
            if knots.len() != points.len() + degree + 1 {
                return Err(format!(
                    "B-spline needs {} knots for {} control points at degree {} (has {})",
                    points.len() + degree + 1,
                    points.len(),
                    degree,
                    knots.len()
                ));
            }
            for k in knots {
                if !k.is_finite() {
                    return Err("non-finite B-spline knot".to_string());
                }
            }
            for w in knots.windows(2) {
                if w[0] > w[1] {
                    return Err("B-spline knots must be non-decreasing".to_string());
                }
            }
            for p in points {
                if !finite3(p) {
                    return Err("non-finite B-spline control point".to_string());
                }
            }
            if let Some(w) = weights {
                if w.len() != points.len() {
                    return Err(format!(
                        "rational B-spline needs {} weights (has {})",
                        points.len(),
                        w.len()
                    ));
                }
                for x in w {
                    if !x.is_finite() || *x <= 0.0 {
                        return Err("B-spline weights must be finite and > 0".to_string());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Surface-level structural rules (shared by validation and parsing).
pub(crate) fn validate_surface(s: &Surface) -> Result<(), String> {
    let finite3 = |v: &Vector3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    match s {
        Surface::Plane {
            origin,
            normal,
            x_dir,
        } => {
            if !finite3(origin) || !finite3(normal) || !finite3(x_dir) {
                return Err("non-finite plane parameters".to_string());
            }
            if normal.length_squared() == 0.0 || x_dir.length_squared() == 0.0 {
                return Err("plane normal and x_dir must be non-zero".to_string());
            }
            if normal.cross(x_dir).length_squared() == 0.0 {
                return Err("plane normal and x_dir must not be parallel".to_string());
            }
        }
        Surface::Cylinder {
            origin,
            axis,
            ref_dir,
            radius,
        } => {
            if !finite3(origin) || !finite3(axis) || !finite3(ref_dir) {
                return Err("non-finite cylinder parameters".to_string());
            }
            if !radius.is_finite() || *radius <= 0.0 {
                return Err("cylinder radius must be > 0".to_string());
            }
        }
        Surface::Cone {
            origin,
            axis,
            ref_dir,
            semi_angle,
            ref_radius,
        } => {
            if !finite3(origin) || !finite3(axis) || !finite3(ref_dir) {
                return Err("non-finite cone parameters".to_string());
            }
            if !semi_angle.is_finite() || !ref_radius.is_finite() || *ref_radius < 0.0 {
                return Err("cone needs a finite semi-angle and ref_radius >= 0".to_string());
            }
        }
        Surface::Sphere {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            if !finite3(center) || !finite3(axis) || !finite3(ref_dir) {
                return Err("non-finite sphere parameters".to_string());
            }
            if !radius.is_finite() || *radius <= 0.0 {
                return Err("sphere radius must be > 0".to_string());
            }
        }
        Surface::Torus {
            center,
            axis,
            ref_dir,
            major_radius,
            minor_radius,
        } => {
            if !finite3(center) || !finite3(axis) || !finite3(ref_dir) {
                return Err("non-finite torus parameters".to_string());
            }
            if *major_radius <= 0.0 || *minor_radius <= 0.0 {
                return Err("torus radii must be > 0".to_string());
            }
        }
        Surface::Bspline {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            points,
            weights,
        } => {
            if *u_degree < 1 || *v_degree < 1 {
                return Err("B-spline surface degrees must be >= 1".to_string());
            }
            let (npu, npv) = (points.len(), points[0].len());
            if npu < *u_degree + 1 || npv < *v_degree + 1 {
                return Err("B-spline surface control net too small".to_string());
            }
            if u_knots.len() != npu + u_degree + 1 {
                return Err(format!(
                    "u knots must number {} (has {})",
                    npu + u_degree + 1,
                    u_knots.len()
                ));
            }
            if v_knots.len() != npv + v_degree + 1 {
                return Err(format!(
                    "v knots must number {} (has {})",
                    npv + v_degree + 1,
                    v_knots.len()
                ));
            }
            for w in u_knots.windows(2) {
                if w[0] > w[1] {
                    return Err("u knots must be non-decreasing".to_string());
                }
            }
            for w in v_knots.windows(2) {
                if w[0] > w[1] {
                    return Err("v knots must be non-decreasing".to_string());
                }
            }
            if let Some(w) = weights {
                if w.len() != npu || w.iter().any(|row| row.len() != npv) {
                    return Err("weights net shape mismatch".to_string());
                }
                for row in w {
                    for x in row {
                        if !x.is_finite() || *x <= 0.0 {
                            return Err("weights must be finite and > 0".to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Primitive generators
// ---------------------------------------------------------------------------

fn v(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// Push edges into a growing topology, deriving vertices from curve
/// evaluations (bit-exact endpoints by construction).
struct EdgeBuilder {
    vertices: Vec<TopoVertex>,
    edges: Vec<TopoEdge>,
}

impl EdgeBuilder {
    fn new() -> Self {
        EdgeBuilder {
            vertices: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Add an edge between existing vertex indices.
    fn add(&mut self, curve: Curve, range: (f64, f64), start: usize, end: usize) -> usize {
        let idx = self.edges.len();
        self.edges.push(TopoEdge {
            curve,
            range,
            start,
            end,
        });
        idx
    }

    /// Find or append a vertex at exactly `p`.
    fn vertex(&mut self, p: Vector3) -> usize {
        if let Some(i) = self.vertices.iter().position(|v| v.point == p) {
            return i;
        }
        self.vertices.push(TopoVertex { point: p });
        self.vertices.len() - 1
    }
}

fn wire(uses: &[(usize, bool)]) -> TopoWire {
    TopoWire {
        edges: uses
            .iter()
            .map(|&(edge, forward)| WireEdge { edge, forward })
            .collect(),
    }
}

fn face(surface: Surface, uv: [f64; 4], wires: Vec<TopoWire>) -> TopoFace {
    TopoFace {
        surface,
        uv_bounds: uv,
        wires,
    }
}

/// Box: 8 vertices, 12 line edges, 6 planar faces. Corner at the
/// origin like FreeCAD `Part::Box`.
fn box_topology(l: f64, w: f64, h: f64) -> ExactBRep {
    let mut b = EdgeBuilder::new();
    let p000 = v(0.0, 0.0, 0.0);
    let pl00 = v(l, 0.0, 0.0);
    let plw0 = v(l, w, 0.0);
    let p0w0 = v(0.0, w, 0.0);
    let p00h = v(0.0, 0.0, h);
    let pl0h = v(l, 0.0, h);
    let plwh = v(l, w, h);
    let p0wh = v(0.0, w, h);

    // Bottom ring (CCW seen from +Z; the bottom face uses it reversed).
    // The builder derives vertices from the curve evaluations, so each
    // edge's endpoints unify exactly at the corners.
    let mk_line = |bb: &mut EdgeBuilder, a: Vector3, c: Vector3| -> usize {
        let curve = Curve::Line {
            origin: a,
            direction: c - a,
        };
        let s = bb.vertex(curve.point(0.0));
        let e = bb.vertex(curve.point(1.0));
        bb.add(curve, (0.0, 1.0), s, e)
    };

    let e0 = mk_line(&mut b, p000, pl00);
    let e1 = mk_line(&mut b, pl00, plw0);
    let e2 = mk_line(&mut b, plw0, p0w0);
    let e3 = mk_line(&mut b, p0w0, p000);
    let e4 = mk_line(&mut b, p00h, pl0h);
    let e5 = mk_line(&mut b, pl0h, plwh);
    let e6 = mk_line(&mut b, plwh, p0wh);
    let e7 = mk_line(&mut b, p0wh, p00h);
    let e8 = mk_line(&mut b, p000, p00h);
    let e9 = mk_line(&mut b, pl00, pl0h);
    let e10 = mk_line(&mut b, plw0, plwh);
    let e11 = mk_line(&mut b, p0w0, p0wh);

    let faces = vec![
        // Bottom (outward −Z): frame x = +Y, y = n×x = +X.
        face(
            Surface::Plane {
                origin: p000,
                normal: v(0.0, 0.0, -1.0),
                x_dir: Vector3::Y,
            },
            [0.0, w, 0.0, l],
            vec![wire(&[(e3, false), (e2, false), (e1, false), (e0, false)])],
        ),
        // Top (outward +Z).
        face(
            Surface::Plane {
                origin: p00h,
                normal: Vector3::Z,
                x_dir: Vector3::X,
            },
            [0.0, l, 0.0, w],
            vec![wire(&[(e4, true), (e5, true), (e6, true), (e7, true)])],
        ),
        // Front y = 0 (outward −Y): frame x = +X, y = n×x = +Z.
        face(
            Surface::Plane {
                origin: p000,
                normal: v(0.0, -1.0, 0.0),
                x_dir: Vector3::X,
            },
            [0.0, l, 0.0, h],
            vec![wire(&[(e0, true), (e9, true), (e4, false), (e8, false)])],
        ),
        // Back y = w (outward +Y): frame x = −X from origin (l,w,0),
        // y = +Z.
        face(
            Surface::Plane {
                origin: plw0,
                normal: Vector3::Y,
                x_dir: v(-1.0, 0.0, 0.0),
            },
            [0.0, l, 0.0, h],
            vec![wire(&[(e2, true), (e11, true), (e6, false), (e10, false)])],
        ),
        // Left x = 0 (outward −X): frame x = +Z, y = n×x = +Y.
        face(
            Surface::Plane {
                origin: p000,
                normal: v(-1.0, 0.0, 0.0),
                x_dir: Vector3::Z,
            },
            [0.0, h, 0.0, w],
            vec![wire(&[(e8, true), (e7, false), (e11, false), (e3, true)])],
        ),
        // Right x = l (outward +X).
        face(
            Surface::Plane {
                origin: pl00,
                normal: Vector3::X,
                x_dir: Vector3::Y,
            },
            [0.0, w, 0.0, h],
            vec![wire(&[(e1, true), (e10, true), (e5, false), (e9, false)])],
        ),
    ];

    ExactBRep {
        vertices: b.vertices,
        edges: b.edges,
        faces,
    }
}

/// Cylinder: 2 vertices (seam base/top), 3 edges (base circle, top
/// circle, seam line), 3 faces (mantle + 2 caps).
fn cylinder_topology(r: f64, h: f64) -> ExactBRep {
    let mut b = EdgeBuilder::new();

    // Base circle: CCW seen from +Z, closing on the seam base vertex.
    let base_curve = Curve::Circle {
        center: v(0.0, 0.0, 0.0),
        axis: Vector3::Z,
        ref_dir: Vector3::X,
        radius: r,
    };
    let vbase = b.vertex(base_curve.point(0.0)); // (r, 0, 0)
    let e0 = b.add(base_curve, (0.0, TAU), vbase, vbase);

    // Top circle, closing on the seam top vertex.
    let top_curve = Curve::Circle {
        center: v(0.0, 0.0, h),
        axis: Vector3::Z,
        ref_dir: Vector3::X,
        radius: r,
    };
    let vtop = b.vertex(top_curve.point(0.0)); // (r, 0, h)
    let e1 = b.add(top_curve, (0.0, TAU), vtop, vtop);

    // Seam line from (r, 0, 0) up to (r, 0, h).
    let seam = Curve::Line {
        origin: v(r, 0.0, 0.0),
        direction: v(0.0, 0.0, h),
    };
    let s0 = b.vertex(seam.point(0.0));
    let s1 = b.vertex(seam.point(1.0));
    debug_assert_eq!(
        (s0, s1),
        (vbase, vtop),
        "seam endpoints unify with the circles"
    );
    let e2 = b.add(seam, (0.0, 1.0), s0, s1);

    let faces = vec![
        // Mantle: full turn, v along the axis; the wire walks the UV
        // rectangle: seam up, top circle, seam down, base circle.
        face(
            Surface::Cylinder {
                origin: v(0.0, 0.0, 0.0),
                axis: Vector3::Z,
                ref_dir: Vector3::X,
                radius: r,
            },
            [0.0, TAU, 0.0, h],
            vec![wire(&[(e2, true), (e1, true), (e2, false), (e0, false)])],
        ),
        // Bottom cap (outward −Z): the boundary runs CW seen from +Z.
        face(
            Surface::Plane {
                origin: v(0.0, 0.0, 0.0),
                normal: v(0.0, 0.0, -1.0),
                x_dir: Vector3::X,
            },
            [-r, r, -r, r],
            vec![wire(&[(e0, false)])],
        ),
        // Top cap (outward +Z).
        face(
            Surface::Plane {
                origin: v(0.0, 0.0, h),
                normal: Vector3::Z,
                x_dir: Vector3::X,
            },
            [-r, r, -r, r],
            vec![wire(&[(e1, true)])],
        ),
    ];

    ExactBRep {
        vertices: b.vertices,
        edges: b.edges,
        faces,
    }
}

/// Cone: frustum (r2 > 0) or plain cone (r2 == 0, apex degenerate
/// edge). Semi-angle `atan2(r2 − r1, h)`; the mantle's v spans the
/// slant.
fn cone_topology(r1: f64, r2: f64, h: f64) -> ExactBRep {
    let slant = (h * h + (r1 - r2) * (r1 - r2)).sqrt();
    let semi = (r2 - r1).atan2(h);
    let has_top = r2 > 0.0;

    let mut b = EdgeBuilder::new();

    let base_curve = Curve::Circle {
        center: v(0.0, 0.0, 0.0),
        axis: Vector3::Z,
        ref_dir: Vector3::X,
        radius: r1,
    };
    let vbase = b.vertex(base_curve.point(0.0));
    let e0 = b.add(base_curve, (0.0, TAU), vbase, vbase);

    // The apex/top circle: degenerate (radius 0) for the plain cone,
    // a full circle for the frustum.
    let (e1, vtop) = if has_top {
        let top_curve = Curve::Circle {
            center: v(0.0, 0.0, h),
            axis: Vector3::Z,
            ref_dir: Vector3::X,
            radius: r2,
        };
        let vt = b.vertex(top_curve.point(0.0));
        (b.add(top_curve, (0.0, TAU), vt, vt), vt)
    } else {
        let apex_curve = Curve::Circle {
            center: v(0.0, 0.0, h),
            axis: Vector3::Z,
            ref_dir: Vector3::X,
            radius: 0.0,
        };
        let va = b.vertex(apex_curve.point(0.0)); // (0, 0, h)
        (b.add(apex_curve, (0.0, TAU), va, va), va)
    };

    // Seam line along the slant: from (r1, 0, 0) to (r2, 0, h).
    let seam = Curve::Line {
        origin: v(r1, 0.0, 0.0),
        direction: v(r2 - r1, 0.0, h),
    };
    let s0 = b.vertex(seam.point(0.0));
    let s1 = b.vertex(seam.point(1.0));
    debug_assert_eq!(s0, vbase, "the seam base unifies with the base circle");
    debug_assert_eq!(s1, vtop, "the seam top unifies with the top ring");
    let e2 = b.add(seam, (0.0, 1.0), s0, s1);

    let mut faces = vec![
        // Mantle: full turn, v along the slant.
        face(
            Surface::Cone {
                origin: v(0.0, 0.0, 0.0),
                axis: Vector3::Z,
                ref_dir: Vector3::X,
                semi_angle: semi,
                ref_radius: r1,
            },
            [0.0, TAU, 0.0, slant],
            vec![wire(&[(e2, true), (e1, true), (e2, false), (e0, false)])],
        ),
        // Base cap (outward −Z).
        face(
            Surface::Plane {
                origin: v(0.0, 0.0, 0.0),
                normal: v(0.0, 0.0, -1.0),
                x_dir: Vector3::X,
            },
            [-r1, r1, -r1, r1],
            vec![wire(&[(e0, false)])],
        ),
    ];
    if has_top {
        faces.push(face(
            Surface::Plane {
                origin: v(0.0, 0.0, h),
                normal: Vector3::Z,
                x_dir: Vector3::X,
            },
            [-r2, r2, -r2, r2],
            vec![wire(&[(e1, true)])],
        ));
    }

    ExactBRep {
        vertices: b.vertices,
        edges: b.edges,
        faces,
    }
}

/// Sphere: 2 pole vertices, 3 edges (meridian seam arc + 2 degenerate
/// pole circles), 1 face with UV `[0, 2π] × [−π/2, π/2]`.
///
/// The pole vertices are the seam's endpoint evaluations, and the
/// degenerate pole circles are centered on exactly those points, so
/// every endpoint stays bit-exact (the poles carry trigonometry's
/// last-ulp noise by construction — see the module docs).
pub(crate) fn sphere_topology(r: f64) -> ExactBRep {
    let mut b = EdgeBuilder::new();

    // The seam runs south → north through +X (axis −Y so increasing
    // t sweeps that way).
    let seam = Curve::Circle {
        center: v(0.0, 0.0, 0.0),
        axis: v(0.0, -1.0, 0.0),
        ref_dir: Vector3::X,
        radius: r,
    };
    let vsouth = b.vertex(seam.point(-FRAC_PI_2));
    let vnorth = b.vertex(seam.point(FRAC_PI_2));
    let (north_point, south_point) = (seam.point(FRAC_PI_2), seam.point(-FRAC_PI_2));
    let e0 = b.add(seam, (-FRAC_PI_2, FRAC_PI_2), vsouth, vnorth);

    // Degenerate pole circles centered ON the derived pole vertices.
    let north_degen = Curve::Circle {
        center: north_point,
        axis: Vector3::Z,
        ref_dir: Vector3::X,
        radius: 0.0,
    };
    let e1 = b.add(north_degen, (0.0, TAU), vnorth, vnorth);

    let south_degen = Curve::Circle {
        center: south_point,
        axis: Vector3::Z,
        ref_dir: Vector3::X,
        radius: 0.0,
    };
    let e2 = b.add(south_degen, (0.0, TAU), vsouth, vsouth);

    let faces = vec![face(
        Surface::Sphere {
            center: v(0.0, 0.0, 0.0),
            axis: Vector3::Z,
            ref_dir: Vector3::X,
            radius: r,
        },
        [0.0, TAU, -FRAC_PI_2, FRAC_PI_2],
        vec![wire(&[(e0, true), (e1, true), (e0, false), (e2, false)])],
    )];

    ExactBRep {
        vertices: b.vertices,
        edges: b.edges,
        faces,
    }
}

/// Torus: 1 vertex (the seam crossing at `(R + r, 0, 0)`), 2 closed
/// seam edges (the minor circle at u = 0 and the major circle at
/// v = 0), 1 face with both parameters full turns.
fn torus_topology(r1: f64, r2: f64) -> ExactBRep {
    let mut b = EdgeBuilder::new();

    // The u=0 minor circle: center on the ring at +X, in the XZ
    // plane (axis −Y), passing through the seam crossing.
    let minor = Curve::Circle {
        center: v(r1, 0.0, 0.0),
        axis: v(0.0, -1.0, 0.0),
        ref_dir: Vector3::X,
        radius: r2,
    };
    let v0 = b.vertex(minor.point(0.0)); // (r1 + r2, 0, 0)
    let e0 = b.add(minor, (0.0, TAU), v0, v0);

    // The v=0 major circle: radius r1 + r2 in the z = 0 plane, through
    // the same point.
    let major = Curve::Circle {
        center: v(0.0, 0.0, 0.0),
        axis: Vector3::Z,
        ref_dir: Vector3::X,
        radius: r1 + r2,
    };
    let v1 = b.vertex(major.point(0.0));
    debug_assert_eq!(v1, v0, "both torus seams cross at one vertex");
    let e1 = b.add(major, (0.0, TAU), v0, v0);

    let faces = vec![face(
        Surface::Torus {
            center: v(0.0, 0.0, 0.0),
            axis: Vector3::Z,
            ref_dir: Vector3::X,
            major_radius: r1,
            minor_radius: r2,
        },
        [0.0, TAU, 0.0, TAU],
        vec![wire(&[(e0, true), (e1, true), (e0, false), (e1, false)])],
    )];

    ExactBRep {
        vertices: b.vertices,
        edges: b.edges,
        faces,
    }
}
