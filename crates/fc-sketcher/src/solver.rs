//! A small planar geometric constraint solver:
//! Gauss-Newton with Levenberg-Marquardt damping.
//!
//! # Model
//!
//! A sketch is flattened into a parameter vector `p` (each geometry owns a
//! contiguous block, laid out as documented in [`crate::geometry`]) and a
//! residual vector `r(p)` assembled from the constraints:
//!
//! | constraint | residual(s) |
//! |------------|-------------|
//! | `Coincident` | `x1 - x2`, `y1 - y2` (2 equations) |
//! | `Horizontal` | `y_end - y_start` |
//! | `Vertical` | `x_end - x_start` |
//! | `Parallel` | `dx1*dy2 - dy1*dx2` (cross product) |
//! | `Perpendicular` | `dx1*dx2 + dy1*dy2` (dot product) |
//! | `Distance` | `hypot(x2-x1, y2-y1) - value` |
//! | `DistanceX` / `DistanceY` | `(x2-x1) - value` / `(y2-y1) - value` |
//! | `Radius` / `Diameter` | `r - value` / `2r - value` |
//! | `Equal` | `len1 - len2` (lines) or `r1 - r2` (circles/arcs) |
//! | `Angle` | `wrap_pi(theta2 - theta1 - value)` with `theta = atan2(dy, dx)` |
//! | `PointOnObject` (line) | signed point-to-line distance |
//! | `PointOnObject` (circle) | `(px-cx)^2 + (py-cy)^2 - r^2` |
//!
//! # Algorithm
//!
//! Levenberg-Marquardt: iterate `(J^T J + lambda * diag) * delta = -J^T r`,
//! accepting a step only when it decreases `||r||_2`, shrinking `lambda`
//! by 3 on success and growing it by 10 on rejection (capped at `1e12`).
//! The Jacobian is built by **central finite differences** with a relative
//! step of `1e-7` — a deliberate v1 choice: it is uniform across all
//! residual types, immune to hand-derivation sign errors, and avoids
//! hand-coded singularities (e.g. the gradient of a distance at distance
//! zero); at v1 problem sizes (tens of parameters) the extra residual
//! evaluations are irrelevant. Analytic Jacobians are the obvious v2
//! upgrade.
//!
//! Termination: `||r||_2 < 1e-10` (success), `||delta||_2 < 1e-10`
//! (stalled), or 200 accepted iterations (budget exhausted) — the latter
//! two are then classified by the final residual norm.
//!
//! # Semantics
//!
//! - `Sketch::solve` first checks the degree-of-freedom arithmetic; a
//!   negative DOF aborts with [`SketchError::OverConstrained`] *before*
//!   any numerical work.
//! - The solver runs on a copy of the parameters. **Only a `Solved`
//!   outcome writes the solved parameters back into the geometry.**
//!   `ConvergedPartially` and `Failed` leave the sketch bit-identical.
//! - Conflicting-but-consistent-DOF systems converge to a least-squares
//!   compromise and are reported honestly: `Solved` requires
//!   `||r||_2 < 1e-10`, never less.
//! - Under-constrained sketches solve to *some* configuration that
//!   satisfies the constraints. Parameters that appear in no residual keep
//!   their initial values exactly (their step is pinned to zero by the
//!   damping term), so free geometry drifts minimally.
//!
//! # Known pitfalls (documented, not hidden)
//!
//! - A `Distance`/`DistanceX`/`DistanceY` between two points that start
//!   *coincident* has a degenerate gradient; the solver will honestly
//!   stall and report failure. Start from a non-degenerate guess.
//! - Degenerate zero-length lines make direction-based residuals
//!   (`Parallel`, `Angle`, ...) meaningless; the solver fails honestly.

use crate::constraint::{Constraint, PointPos};
use crate::geometry::{GeoId, Geometry, Sketch};
use crate::SketchError;

/// Success tolerance on the residual 2-norm.
const RESIDUAL_TOL: f64 = 1e-10;
/// Termination tolerance on the step 2-norm (spec).
const STEP_TOL: f64 = 1e-10;
/// Maximum accepted LM iterations (spec).
const MAX_ITERATIONS: usize = 200;
/// A stalled solve whose residual lands below this is a "compromise"
/// (partially converged) rather than an outright failure.
const PARTIAL_TOL: f64 = 1e-6;
/// Initial LM damping.
const INITIAL_LAMBDA: f64 = 1e-3;
/// Smallest lambda we shrink to (quasi-Gauss-Newton regime).
const MIN_LAMBDA: f64 = 1e-14;
/// Lambda at which a step attempt is considered hopeless.
const MAX_LAMBDA: f64 = 1e12;
/// Relative step for the central-difference Jacobian.
const FD_STEP: f64 = 1e-7;
/// Floor for the damping diagonal; guarantees a positive pivot for
/// parameters that appear in no residual, pinning their step to zero.
const DIAGONAL_FLOOR: f64 = 1e-12;

/// Outcome of `Sketch::solve`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolveResult {
    /// All constraints are satisfied: final `||r||_2 < 1e-10`. The solved
    /// parameters have been written back into the geometry.
    Solved {
        /// Accepted LM iterations (0 if the input already satisfied every
        /// constraint).
        iterations: usize,
        /// Final residual 2-norm.
        residual: f64,
    },
    /// The solver stalled (step/lambda exhausted) with a residual in
    /// `[1e-10, 1e-6)`: a near-miss compromise, not a success. Geometry
    /// is unchanged.
    ConvergedPartially,
    /// The solver ran out of iterations or stalled with a residual
    /// `>= 1e-6`. Geometry is unchanged.
    Failed {
        /// Accepted LM iterations.
        iterations: usize,
        /// Final residual 2-norm.
        residual: f64,
    },
}

impl Sketch {
    /// Degrees of freedom: total geometry parameters minus total
    /// constraint dimensions. Negative means over-constrained.
    ///
    /// Note this is *arithmetic* DOF (FreeCAD-style); it does not detect
    /// redundant-but-consistent constraint sets.
    pub fn dof(&self) -> isize {
        let params: usize = self.geometry().iter().map(|g| g.dof()).sum();
        let dims: usize = self.constraints().iter().map(|c| c.dimension()).sum();
        params as isize - dims as isize
    }

    /// `dof() == 0`. True for consistent redundant systems too (see the
    /// module notes on translation invariance: without an origin geometry,
    /// any *consistent* non-empty sketch keeps `dof >= 2`).
    pub fn is_fully_constrained(&self) -> bool {
        self.dof() == 0
    }

    /// Solves the sketch numerically.
    ///
    /// Returns [`SketchError::OverConstrained`] if `dof() < 0` (checked
    /// before any numerical work). Otherwise runs Levenberg-Marquardt on a
    /// copy of the parameters and, on [`SolveResult::Solved`], writes the
    /// solved parameters back into the sketch's geometry. Any non-`Solved`
    /// outcome leaves the geometry untouched.
    pub fn solve(&mut self) -> Result<SolveResult, SketchError> {
        let dof = self.dof();
        if dof < 0 {
            return Err(SketchError::OverConstrained {
                excess: (-dof) as usize,
            });
        }
        let (params, offsets) = build_params(self);
        let (result, solved_params) = run_lm(self, &offsets, params);
        if let (SolveResult::Solved { .. }, Some(p)) = (&result, solved_params) {
            write_back(self, &offsets, &p);
        }
        Ok(result)
    }
}

/// Flattens the sketch's geometry into the solver parameter vector, plus
/// each geometry's block offset.
fn build_params(sk: &Sketch) -> (Vec<f64>, Vec<usize>) {
    let mut params = Vec::new();
    let mut offsets = Vec::with_capacity(sk.geometry().len());
    for g in sk.geometry() {
        offsets.push(params.len());
        params.extend(g.to_params());
    }
    (params, offsets)
}

/// Writes solved parameters back into the sketch's geometry, block by
/// block.
fn write_back(sk: &mut Sketch, offsets: &[usize], params: &[f64]) {
    for (i, g) in sk.geometries_mut().iter_mut().enumerate() {
        let start = offsets[i];
        let end = start + g.dof();
        g.set_from_params(&params[start..end]);
    }
}

/// Runs Levenberg-Marquardt. Returns the result plus the solved parameter
/// vector when (and only when) the run succeeded.
fn run_lm(sk: &Sketch, offsets: &[usize], p0: Vec<f64>) -> (SolveResult, Option<Vec<f64>>) {
    let mut p = p0;
    let mut r = residual_vector(sk, offsets, &p);
    let mut r_norm = norm2(&r);
    if r_norm < RESIDUAL_TOL {
        return (
            SolveResult::Solved {
                iterations: 0,
                residual: r_norm,
            },
            Some(p),
        );
    }
    let n = p.len();
    let m = r.len();
    let mut lambda = INITIAL_LAMBDA;
    let mut iterations = 0usize;
    while iterations < MAX_ITERATIONS {
        iterations += 1;

        let j = fd_jacobian(sk, offsets, &p, m, n);
        // A = J^T J, g = J^T r (normal equations).
        let mut a = vec![0.0; n * n];
        let mut g = vec![0.0; n];
        for k in 0..m {
            for i in 0..n {
                let jki = j[k * n + i];
                if jki == 0.0 {
                    continue;
                }
                g[i] += jki * r[k];
                for l in 0..n {
                    a[i * n + l] += jki * j[k * n + l];
                }
            }
        }

        // Step attempts with escalating damping until one improves r_norm.
        let mut accepted = false;
        let mut stalled = false;
        loop {
            let mut sys = a.clone();
            for i in 0..n {
                let d = sys[i * n + i].max(DIAGONAL_FLOOR);
                sys[i * n + i] += lambda * d;
            }
            let mut delta: Vec<f64> = g.iter().map(|&gv| -gv).collect();
            if gauss_solve(n, &mut sys, &mut delta) {
                let p2: Vec<f64> = p
                    .iter()
                    .zip(delta.iter())
                    .map(|(&pv, &dv)| pv + dv)
                    .collect();
                let r2 = residual_vector(sk, offsets, &p2);
                let r2_norm = norm2(&r2);
                if r2_norm < r_norm {
                    let step_norm = norm2(&delta);
                    p = p2;
                    r = r2;
                    r_norm = r2_norm;
                    accepted = true;
                    lambda = (lambda / 3.0).max(MIN_LAMBDA);
                    if r_norm < RESIDUAL_TOL {
                        return (
                            SolveResult::Solved {
                                iterations,
                                residual: r_norm,
                            },
                            Some(p),
                        );
                    }
                    if step_norm < STEP_TOL {
                        stalled = true;
                    }
                    break;
                }
            }
            lambda *= 10.0;
            if lambda > MAX_LAMBDA {
                break;
            }
        }
        if !accepted || stalled {
            break;
        }
    }
    (classify(iterations, r_norm), None)
}

/// Classifies a finished-but-unsolved run by its final residual norm.
fn classify(iterations: usize, r_norm: f64) -> SolveResult {
    if r_norm < RESIDUAL_TOL {
        SolveResult::Solved {
            iterations,
            residual: r_norm,
        }
    } else if r_norm < PARTIAL_TOL {
        SolveResult::ConvergedPartially
    } else {
        SolveResult::Failed {
            iterations,
            residual: r_norm,
        }
    }
}

/// Resolves a point slot against the *parametric* state, honouring the
/// arc's angle-derived endpoints.
///
/// Invariant: `add_constraint` has validated the `(geometry, slot)` pair,
/// so the fallback arm is unreachable through the public API.
fn point_at_params(
    geos: &[Geometry],
    offsets: &[usize],
    p: &[f64],
    g: GeoId,
    pos: PointPos,
) -> (f64, f64) {
    let i = g.as_index();
    let o = offsets[i];
    match (&geos[i], pos) {
        (Geometry::Point { .. }, PointPos::Start) => (p[o], p[o + 1]),
        (Geometry::LineSegment { .. }, PointPos::Start) => (p[o], p[o + 1]),
        (Geometry::LineSegment { .. }, PointPos::End) => (p[o + 2], p[o + 3]),
        (Geometry::Circle { .. }, PointPos::Center) => (p[o], p[o + 1]),
        (Geometry::ArcOfCircle { .. }, PointPos::Center) => (p[o], p[o + 1]),
        (Geometry::ArcOfCircle { .. }, PointPos::Start) => (
            p[o] + p[o + 2] * p[o + 3].cos(),
            p[o + 1] + p[o + 2] * p[o + 3].sin(),
        ),
        (Geometry::ArcOfCircle { .. }, PointPos::End) => (
            p[o] + p[o + 2] * p[o + 4].cos(),
            p[o + 1] + p[o + 2] * p[o + 4].sin(),
        ),
        _ => unreachable!("(geometry, PointPos) pairs are validated by add_constraint"),
    }
}

/// Assembles the constraint residual vector at parameter state `p`.
fn residual_vector(sk: &Sketch, offsets: &[usize], p: &[f64]) -> Vec<f64> {
    let geos = sk.geometry();
    let mut out = Vec::with_capacity(sk.constraints().iter().map(|c| c.dimension()).sum());
    let line = |g: GeoId| -> (f64, f64, f64, f64) {
        let (sx, sy) = point_at_params(geos, offsets, p, g, PointPos::Start);
        let (ex, ey) = point_at_params(geos, offsets, p, g, PointPos::End);
        (sx, sy, ex, ey)
    };
    let pt = |g: GeoId, pos: PointPos| point_at_params(geos, offsets, p, g, pos);
    let radius_of = |g: GeoId| p[offsets[g.as_index()] + 2];

    for c in sk.constraints() {
        match *c {
            Constraint::Coincident { g1, pos1, g2, pos2 } => {
                let (x1, y1) = pt(g1, pos1);
                let (x2, y2) = pt(g2, pos2);
                out.push(x1 - x2);
                out.push(y1 - y2);
            }
            Constraint::Horizontal { g } => {
                let (_, sy, _, ey) = line(g);
                out.push(ey - sy);
            }
            Constraint::Vertical { g } => {
                let (sx, _, ex, _) = line(g);
                out.push(ex - sx);
            }
            Constraint::Parallel { g1, g2 } => {
                let (sx1, sy1, ex1, ey1) = line(g1);
                let (sx2, sy2, ex2, ey2) = line(g2);
                out.push((ex1 - sx1) * (ey2 - sy2) - (ey1 - sy1) * (ex2 - sx2));
            }
            Constraint::Perpendicular { g1, g2 } => {
                let (sx1, sy1, ex1, ey1) = line(g1);
                let (sx2, sy2, ex2, ey2) = line(g2);
                out.push((ex1 - sx1) * (ex2 - sx2) + (ey1 - sy1) * (ey2 - sy2));
            }
            Constraint::Distance {
                g1,
                pos1,
                g2,
                pos2,
                value,
            } => {
                let (x1, y1) = pt(g1, pos1);
                let (x2, y2) = pt(g2, pos2);
                out.push((x2 - x1).hypot(y2 - y1) - value);
            }
            Constraint::DistanceX {
                g1,
                pos1,
                g2,
                pos2,
                value,
            } => {
                let (x1, _) = pt(g1, pos1);
                let (x2, _) = pt(g2, pos2);
                out.push(x2 - x1 - value);
            }
            Constraint::DistanceY {
                g1,
                pos1,
                g2,
                pos2,
                value,
            } => {
                let (_, y1) = pt(g1, pos1);
                let (_, y2) = pt(g2, pos2);
                out.push(y2 - y1 - value);
            }
            Constraint::Radius { g, value } => out.push(radius_of(g) - value),
            Constraint::Diameter { g, value } => out.push(2.0 * radius_of(g) - value),
            Constraint::Equal { g1, g2 } => {
                match (&geos[g1.as_index()], &geos[g2.as_index()]) {
                    (Geometry::LineSegment { .. }, Geometry::LineSegment { .. }) => {
                        let (sx1, sy1, ex1, ey1) = line(g1);
                        let (sx2, sy2, ex2, ey2) = line(g2);
                        out.push((ex1 - sx1).hypot(ey1 - sy1) - (ex2 - sx2).hypot(ey2 - sy2));
                    }
                    // Circles/arcs: equal radii (validated by add_constraint).
                    _ => out.push(radius_of(g1) - radius_of(g2)),
                }
            }
            Constraint::Angle { g1, g2, value } => {
                let (sx1, sy1, ex1, ey1) = line(g1);
                let (sx2, sy2, ex2, ey2) = line(g2);
                let th1 = (ey1 - sy1).atan2(ex1 - sx1);
                let th2 = (ey2 - sy2).atan2(ex2 - sx2);
                out.push(wrap_pi(th2 - th1 - value));
            }
            Constraint::PointOnObject { g1, pos1, g2 } => {
                let (px, py) = pt(g1, pos1);
                match &geos[g2.as_index()] {
                    Geometry::LineSegment { .. } => {
                        let (sx, sy, ex, ey) = line(g2);
                        let (dx, dy) = (ex - sx, ey - sy);
                        let len = dx.hypot(dy).max(1e-12);
                        out.push(((px - sx) * dy - (py - sy) * dx) / len);
                    }
                    Geometry::Circle { .. } => {
                        let (cx, cy) = pt(g2, PointPos::Center);
                        let r = radius_of(g2);
                        out.push((px - cx) * (px - cx) + (py - cy) * (py - cy) - r * r);
                    }
                    _ => unreachable!("add_constraint rejects PointOnObject onto non-line/circle"),
                }
            }
        }
    }
    out
}

/// Central-difference Jacobian, row-major `m x n`.
fn fd_jacobian(sk: &Sketch, offsets: &[usize], p: &[f64], m: usize, n: usize) -> Vec<f64> {
    let mut j = vec![0.0; m * n];
    let mut pp = p.to_vec();
    let mut pm = p.to_vec();
    for i in 0..n {
        let h = FD_STEP * p[i].abs().max(1.0);
        pp[i] = p[i] + h;
        pm[i] = p[i] - h;
        let rp = residual_vector(sk, offsets, &pp);
        let rm = residual_vector(sk, offsets, &pm);
        for k in 0..m {
            j[k * n + i] = (rp[k] - rm[k]) / (2.0 * h);
        }
        pp[i] = p[i];
        pm[i] = p[i];
    }
    j
}

/// Dense Gaussian elimination with partial pivoting; solves `a * x = b` in
/// place. Returns false when a pivot collapses (treated as a failed step
/// attempt by the LM loop).
fn gauss_solve(n: usize, a: &mut [f64], b: &mut [f64]) -> bool {
    for col in 0..n {
        let mut pivot_row = col;
        let mut pivot_val = a[col * n + col].abs();
        for row in (col + 1)..n {
            let v = a[row * n + col].abs();
            if v > pivot_val {
                pivot_val = v;
                pivot_row = row;
            }
        }
        // NaN or a collapsed pivot means the damped system is unusable;
        // the LM loop treats that as a failed step attempt.
        if pivot_val.is_nan() || pivot_val <= 1e-300 {
            return false;
        }
        if pivot_row != col {
            for c in col..n {
                a.swap(col * n + c, pivot_row * n + c);
            }
            b.swap(col, pivot_row);
        }
        let inv_pivot = 1.0 / a[col * n + col];
        for row in (col + 1)..n {
            let f = a[row * n + col] * inv_pivot;
            if f == 0.0 {
                continue;
            }
            for c in (col + 1)..n {
                a[row * n + c] -= f * a[col * n + c];
            }
            b[row] -= f * b[col];
        }
    }
    for row in (0..n).rev() {
        let mut acc = b[row];
        for c in (row + 1)..n {
            acc -= a[row * n + c] * b[c];
        }
        b[row] = acc / a[row * n + row];
    }
    true
}

/// Euclidean norm of a vector.
fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// Wraps an angle into `(-pi, pi]` so angle residuals are wrap-aware.
fn wrap_pi(x: f64) -> f64 {
    let tau = 2.0 * std::f64::consts::PI;
    let mut y = x % tau;
    if y > std::f64::consts::PI {
        y -= tau;
    }
    if y < -std::f64::consts::PI {
        y += tau;
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    fn line(sk: &mut Sketch, sx: f64, sy: f64, ex: f64, ey: f64) -> GeoId {
        sk.add_geometry(Geometry::LineSegment {
            start: Point::new(sx, sy),
            end: Point::new(ex, ey),
        })
    }

    fn pt_coords(sk: &Sketch, g: GeoId, pos: PointPos) -> (f64, f64) {
        let p = sk.geometry_by_id(g).unwrap().point_at(pos).unwrap();
        (p.x, p.y)
    }

    // ---------- DOF arithmetic (the counter) ----------

    #[test]
    fn dof_empty_sketch_is_zero() {
        let sk = Sketch::new();
        assert_eq!(sk.dof(), 0);
        // Vacuously true: no parameters, no equations.
        assert!(sk.is_fully_constrained());
    }

    #[test]
    fn dof_bare_line_is_four() {
        let mut sk = Sketch::new();
        line(&mut sk, 0.0, 0.0, 1.0, 2.0);
        assert_eq!(sk.dof(), 4);
        assert!(!sk.is_fully_constrained());
    }

    #[test]
    fn dof_line_plus_horizontal_is_three() {
        let mut sk = Sketch::new();
        let l = line(&mut sk, 0.0, 0.0, 1.0, 2.0);
        sk.add_constraint(Constraint::Horizontal { g: l }).unwrap();
        assert_eq!(sk.dof(), 3);
    }

    #[test]
    fn dof_two_lines_one_coincident_end_is_six() {
        // 8 params - 2 equations (dx, dy) = 6.
        // (WOG's example text says "7-2=5"; the arithmetic under the spec's
        // own residual definition — Coincident contributes dx AND dy — is
        // 8-2 = 6. See the final report's deviation notes.)
        let mut sk = Sketch::new();
        let a = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        let b = line(&mut sk, 5.0, 5.0, 6.0, 1.0);
        sk.add_constraint(Constraint::Coincident {
            g1: a,
            pos1: PointPos::End,
            g2: b,
            pos2: PointPos::Start,
        })
        .unwrap();
        assert_eq!(sk.dof(), 6);
    }

    #[test]
    fn dof_closed_triangle_with_horizontal_is_five() {
        // 3 lines = 12 params; 3 coincidences = 6 equations;
        // 1 horizontal = 1 equation; 12 - 7 = 5.
        // (WOG's test list says "dof reported 3"; the correct count under
        // the spec's own residual definitions is 5. See deviation notes.)
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 0.0, 3.7, 0.25);
        let l2 = line(&mut sk, 4.1, 0.4, 2.2, 3.1);
        let l3 = line(&mut sk, 1.8, 3.3, 0.15, 0.2);
        for (g1, p1, g2, p2) in [
            (l1, PointPos::End, l2, PointPos::Start),
            (l2, PointPos::End, l3, PointPos::Start),
            (l3, PointPos::End, l1, PointPos::Start),
        ] {
            sk.add_constraint(Constraint::Coincident {
                g1,
                pos1: p1,
                g2,
                pos2: p2,
            })
            .unwrap();
        }
        sk.add_constraint(Constraint::Horizontal { g: l1 }).unwrap();
        assert_eq!(sk.dof(), 5);
    }

    #[test]
    fn dof_rectangle_with_dimensions_is_two() {
        // 4 lines = 16 params; 4 coincidences (8) + 2 H + 2 V + width +
        // height (2) = 14 equations; 16 - 14 = 2.
        // The remaining 2 DOF are global translation: the v1 constraint
        // vocabulary is translation-invariant and has no origin geometry to
        // anchor to (FreeCAD pins a corner to the origin for dof 0 — out of
        // v1 scope). See deviation notes.
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 3.0, 4.0, 3.0); // top, left -> right
        let l2 = line(&mut sk, 4.0, 3.0, 4.0, 0.0); // right, top -> bottom
        let l3 = line(&mut sk, 4.0, 0.0, 0.0, 0.0); // bottom, right -> left
        let l4 = line(&mut sk, 0.0, 0.0, 0.0, 3.0); // left, bottom -> top
        for (g1, p1, g2, p2) in [
            (l1, PointPos::End, l2, PointPos::Start),
            (l2, PointPos::End, l3, PointPos::Start),
            (l3, PointPos::End, l4, PointPos::Start),
            (l4, PointPos::End, l1, PointPos::Start),
        ] {
            sk.add_constraint(Constraint::Coincident {
                g1,
                pos1: p1,
                g2,
                pos2: p2,
            })
            .unwrap();
        }
        for g in [l1, l3] {
            sk.add_constraint(Constraint::Horizontal { g }).unwrap();
        }
        for g in [l2, l4] {
            sk.add_constraint(Constraint::Vertical { g }).unwrap();
        }
        sk.add_constraint(Constraint::DistanceX {
            g1: l1,
            pos1: PointPos::Start,
            g2: l1,
            pos2: PointPos::End,
            value: 4.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: l4,
            pos1: PointPos::Start,
            g2: l4,
            pos2: PointPos::End,
            value: 3.0,
        })
        .unwrap();
        assert_eq!(sk.dof(), 2);
    }

    #[test]
    fn dof_overconstrained_is_negative() {
        let mut sk = Sketch::new();
        let l = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        sk.add_constraint(Constraint::Horizontal { g: l }).unwrap();
        sk.add_constraint(Constraint::Vertical { g: l }).unwrap();
        sk.add_constraint(Constraint::Distance {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 5.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 3.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 1.0,
        })
        .unwrap();
        // 4 params - 5 equations.
        assert_eq!(sk.dof(), -1);
    }

    // ---------- Solving: mandated scenarios ----------

    #[test]
    fn solve_triangle_closes_and_flattens_base() {
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 0.0, 3.7, 0.25);
        let l2 = line(&mut sk, 4.1, 0.4, 2.2, 3.1);
        let l3 = line(&mut sk, 1.8, 3.3, 0.15, 0.2);
        for (g1, p1, g2, p2) in [
            (l1, PointPos::End, l2, PointPos::Start),
            (l2, PointPos::End, l3, PointPos::Start),
            (l3, PointPos::End, l1, PointPos::Start),
        ] {
            sk.add_constraint(Constraint::Coincident {
                g1,
                pos1: p1,
                g2,
                pos2: p2,
            })
            .unwrap();
        }
        sk.add_constraint(Constraint::Horizontal { g: l1 }).unwrap();

        match sk.solve().unwrap() {
            SolveResult::Solved { residual, .. } => assert!(residual < 1e-10),
            other => panic!("expected Solved, got {other:?}"),
        }
        // All three corners coincide to 1e-8.
        let pairs = [
            (l1, PointPos::End, l2, PointPos::Start),
            (l2, PointPos::End, l3, PointPos::Start),
            (l3, PointPos::End, l1, PointPos::Start),
        ];
        for (g1, p1, g2, p2) in pairs {
            let (x1, y1) = pt_coords(&sk, g1, p1);
            let (x2, y2) = pt_coords(&sk, g2, p2);
            assert!((x1 - x2).abs() < 1e-8, "corner x mismatch: {x1} vs {x2}");
            assert!((y1 - y2).abs() < 1e-8, "corner y mismatch: {y1} vs {y2}");
        }
        // First line is horizontal.
        let (_, sy, _, ey) = {
            let (a, b) = (
                pt_coords(&sk, l1, PointPos::Start),
                pt_coords(&sk, l1, PointPos::End),
            );
            (a.0, a.1, b.0, b.1)
        };
        assert!((ey - sy).abs() < 1e-8);
        assert_eq!(sk.dof(), 5);
    }

    #[test]
    fn solve_rectangle_from_sloppy_quad() {
        let mut sk = Sketch::new();
        // Mildly skewed quad around the 4x3 rectangle.
        let l1 = line(&mut sk, -0.05, 3.05, 4.05, 2.9); // top
        let l2 = line(&mut sk, 4.05, 2.9, 3.95, -0.1); // right
        let l3 = line(&mut sk, 3.95, -0.1, 0.1, 0.05); // bottom
        let l4 = line(&mut sk, 0.1, 0.05, -0.05, 3.05); // left
        for (g1, p1, g2, p2) in [
            (l1, PointPos::End, l2, PointPos::Start),
            (l2, PointPos::End, l3, PointPos::Start),
            (l3, PointPos::End, l4, PointPos::Start),
            (l4, PointPos::End, l1, PointPos::Start),
        ] {
            sk.add_constraint(Constraint::Coincident {
                g1,
                pos1: p1,
                g2,
                pos2: p2,
            })
            .unwrap();
        }
        for g in [l1, l3] {
            sk.add_constraint(Constraint::Horizontal { g }).unwrap();
        }
        for g in [l2, l4] {
            sk.add_constraint(Constraint::Vertical { g }).unwrap();
        }
        sk.add_constraint(Constraint::DistanceX {
            g1: l1,
            pos1: PointPos::Start,
            g2: l1,
            pos2: PointPos::End,
            value: 4.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: l4,
            pos1: PointPos::Start,
            g2: l4,
            pos2: PointPos::End,
            value: 3.0,
        })
        .unwrap();

        match sk.solve().unwrap() {
            SolveResult::Solved { residual, .. } => assert!(residual < 1e-10),
            other => panic!("expected Solved, got {other:?}"),
        }
        // Corners coincide.
        for (g1, p1, g2, p2) in [
            (l1, PointPos::End, l2, PointPos::Start),
            (l2, PointPos::End, l3, PointPos::Start),
            (l3, PointPos::End, l4, PointPos::Start),
            (l4, PointPos::End, l1, PointPos::Start),
        ] {
            let (x1, y1) = pt_coords(&sk, g1, p1);
            let (x2, y2) = pt_coords(&sk, g2, p2);
            assert!((x1 - x2).abs() < 1e-8 && (y1 - y2).abs() < 1e-8);
        }
        // Top-left / top-right / bottom-right corners.
        let (tlx, tly) = pt_coords(&sk, l1, PointPos::Start);
        let (trx, _) = pt_coords(&sk, l1, PointPos::End);
        let (_, bry) = pt_coords(&sk, l2, PointPos::End);
        // Exact shape: width 4, height 3, axis-aligned.
        assert!((trx - tlx - 4.0).abs() < 1e-8);
        assert!((tly - bry - 3.0).abs() < 1e-8);
        let (_, l4sy) = pt_coords(&sk, l4, PointPos::Start);
        let (l2ex, _) = pt_coords(&sk, l2, PointPos::End);
        assert!((l2ex - trx).abs() < 1e-8);
        assert!((l4sy - bry).abs() < 1e-8);
        // Position: near the drawn quad (min-movement), loosely checked.
        assert!((tlx - 0.0).abs() < 0.5 && (tly - 3.0).abs() < 0.5);
        // DOf arithmetic: 2 (translation), documented deviation.
        assert_eq!(sk.dof(), 2);
        assert!(!sk.is_fully_constrained());
    }

    #[test]
    fn solve_circle_radius_constraint() {
        let mut sk = Sketch::new();
        let c = sk.add_geometry(Geometry::Circle {
            center: Point::new(3.0, 4.0),
            radius: 5.0,
        });
        sk.add_constraint(Constraint::Radius { g: c, value: 2.0 })
            .unwrap();
        match sk.solve().unwrap() {
            SolveResult::Solved { .. } => {}
            other => panic!("expected Solved, got {other:?}"),
        }
        let Geometry::Circle { center, radius } = sk.geometry_by_id(c).unwrap() else {
            panic!("not a circle");
        };
        assert!((radius - 2.0).abs() < 1e-9);
        // The center is not touched by the residual: it stays exactly put.
        assert_eq!((center.x, center.y), (3.0, 4.0));
        assert_eq!(sk.dof(), 2);
    }

    #[test]
    fn solve_circle_diameter_is_twice_radius() {
        let mut sk = Sketch::new();
        let c = sk.add_geometry(Geometry::Circle {
            center: Point::new(0.0, 0.0),
            radius: 1.0,
        });
        sk.add_constraint(Constraint::Diameter { g: c, value: 6.0 })
            .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let Geometry::Circle { radius, .. } = sk.geometry_by_id(c).unwrap() else {
            panic!("not a circle");
        };
        assert!((radius - 3.0).abs() < 1e-9);
    }

    #[test]
    fn solve_equal_radii() {
        let mut sk = Sketch::new();
        let c1 = sk.add_geometry(Geometry::Circle {
            center: Point::new(0.0, 0.0),
            radius: 1.0,
        });
        let c2 = sk.add_geometry(Geometry::Circle {
            center: Point::new(5.0, 0.0),
            radius: 3.0,
        });
        sk.add_constraint(Constraint::Equal { g1: c1, g2: c2 })
            .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (r1, r2) = match (
            sk.geometry_by_id(c1).unwrap(),
            sk.geometry_by_id(c2).unwrap(),
        ) {
            (Geometry::Circle { radius: a, .. }, Geometry::Circle { radius: b, .. }) => (*a, *b),
            _ => panic!("not circles"),
        };
        assert!((r1 - r2).abs() < 1e-8);
        // Minimum-norm move splits the difference.
        assert!((r1 - 2.0).abs() < 1e-6 && (r2 - 2.0).abs() < 1e-6);
    }

    #[test]
    fn solve_equal_line_lengths() {
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 0.0, 3.0, 0.0);
        let l2 = line(&mut sk, 0.0, 1.0, 0.0, 2.0);
        sk.add_constraint(Constraint::Equal { g1: l1, g2: l2 })
            .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (s1, e1) = (
            pt_coords(&sk, l1, PointPos::Start),
            pt_coords(&sk, l1, PointPos::End),
        );
        let (s2, e2) = (
            pt_coords(&sk, l2, PointPos::Start),
            pt_coords(&sk, l2, PointPos::End),
        );
        let len1 = (e1.0 - s1.0).hypot(e1.1 - s1.1);
        let len2 = (e2.0 - s2.0).hypot(e2.1 - s2.1);
        assert!((len1 - len2).abs() < 1e-8);
        assert!((len1 - 2.0).abs() < 1e-2); // both near the midpoint length
    }

    #[test]
    fn solve_angle_constraint_right_angle() {
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        let l2 = line(&mut sk, 0.2, 0.1, 1.2, 0.45);
        sk.add_constraint(Constraint::Angle {
            g1: l1,
            g2: l2,
            value: std::f64::consts::FRAC_PI_2,
        })
        .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (s1, e1) = (
            pt_coords(&sk, l1, PointPos::Start),
            pt_coords(&sk, l1, PointPos::End),
        );
        let (s2, e2) = (
            pt_coords(&sk, l2, PointPos::Start),
            pt_coords(&sk, l2, PointPos::End),
        );
        let (dx1, dy1) = (e1.0 - s1.0, e1.1 - s1.1);
        let (dx2, dy2) = (e2.0 - s2.0, e2.1 - s2.1);
        assert!((dx1 * dx2 + dy1 * dy2).abs() < 1e-8);
    }

    #[test]
    fn solve_angle_constraint_generic_and_wrap_aware() {
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        let l2 = line(&mut sk, 0.2, 0.1, 1.2, 0.405); // direction ~ 0.3 rad
        let target = 2.5_f64; // far from the initial 0.3 -> tests wrap + rotation
        sk.add_constraint(Constraint::Angle {
            g1: l1,
            g2: l2,
            value: target,
        })
        .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        // Min-norm LM may rotate both lines; only the *difference* is
        // constrained, so assert on the wrap-aware direction difference.
        let (s1, e1) = (
            pt_coords(&sk, l1, PointPos::Start),
            pt_coords(&sk, l1, PointPos::End),
        );
        let (s2, e2) = (
            pt_coords(&sk, l2, PointPos::Start),
            pt_coords(&sk, l2, PointPos::End),
        );
        let th1 = (e1.1 - s1.1).atan2(e1.0 - s1.0);
        let th2 = (e2.1 - s2.1).atan2(e2.0 - s2.0);
        assert!(wrap_pi(th2 - th1 - target).abs() < 1e-6);
    }

    #[test]
    fn solve_parallel_constraint() {
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        let l2 = line(&mut sk, 0.0, 1.0, 1.0, 0.8);
        sk.add_constraint(Constraint::Parallel { g1: l1, g2: l2 })
            .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (s1, e1) = (
            pt_coords(&sk, l1, PointPos::Start),
            pt_coords(&sk, l1, PointPos::End),
        );
        let (s2, e2) = (
            pt_coords(&sk, l2, PointPos::Start),
            pt_coords(&sk, l2, PointPos::End),
        );
        let (dx1, dy1) = (e1.0 - s1.0, e1.1 - s1.1);
        let (dx2, dy2) = (e2.0 - s2.0, e2.1 - s2.1);
        assert!((dx1 * dy2 - dy1 * dx2).abs() < 1e-8);
    }

    #[test]
    fn solve_perpendicular_constraint() {
        let mut sk = Sketch::new();
        let l1 = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        let l2 = line(&mut sk, 0.0, 1.0, 1.0, 1.5);
        sk.add_constraint(Constraint::Perpendicular { g1: l1, g2: l2 })
            .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (s1, e1) = (
            pt_coords(&sk, l1, PointPos::Start),
            pt_coords(&sk, l1, PointPos::End),
        );
        let (s2, e2) = (
            pt_coords(&sk, l2, PointPos::Start),
            pt_coords(&sk, l2, PointPos::End),
        );
        let (dx1, dy1) = (e1.0 - s1.0, e1.1 - s1.1);
        let (dx2, dy2) = (e2.0 - s2.0, e2.1 - s2.1);
        assert!((dx1 * dx2 + dy1 * dy2).abs() < 1e-8);
    }

    #[test]
    fn solve_point_on_line() {
        let mut sk = Sketch::new();
        let p = sk.add_geometry(Geometry::Point { x: 1.0, y: 1.0 });
        let l = line(&mut sk, 0.0, 0.0, 2.0, 0.0);
        sk.add_constraint(Constraint::PointOnObject {
            g1: p,
            pos1: PointPos::Start,
            g2: l,
        })
        .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        // The constraint puts the point on the line's *infinite* line; the
        // min-norm solver may move the point, tilt the line, or both. The
        // honest invariant is zero distance to the solved line.
        let Geometry::Point { x, y } = sk.geometry_by_id(p).unwrap() else {
            panic!("not a point");
        };
        let (ls, le) = (
            pt_coords(&sk, l, PointPos::Start),
            pt_coords(&sk, l, PointPos::End),
        );
        let (dx, dy) = (le.0 - ls.0, le.1 - ls.1);
        let len = dx.hypot(dy);
        let dist = ((x - ls.0) * dy - (y - ls.1) * dx).abs() / len;
        assert!(dist < 1e-9, "distance to line: {dist}");
        // Min-movement: nothing wandered far from the drawn input.
        assert!((x - 1.0).hypot(y - 1.0) < 1.0);
        assert_eq!(sk.dof(), 5); // 2 + 4 - 1
    }

    #[test]
    fn solve_point_on_circle() {
        let mut sk = Sketch::new();
        let p = sk.add_geometry(Geometry::Point { x: 0.5, y: 0.5 });
        let c = sk.add_geometry(Geometry::Circle {
            center: Point::new(2.0, 0.0),
            radius: 1.0,
        });
        sk.add_constraint(Constraint::PointOnObject {
            g1: p,
            pos1: PointPos::Start,
            g2: c,
        })
        .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let Geometry::Point { x, y } = sk.geometry_by_id(p).unwrap() else {
            panic!("not a point");
        };
        let Geometry::Circle { center, radius } = sk.geometry_by_id(c).unwrap() else {
            panic!("not a circle");
        };
        let d = (x - center.x).hypot(y - center.y);
        assert!((d - radius).abs() < 1e-8);
        assert_eq!(sk.dof(), 4); // 2 + 3 - 1
    }

    #[test]
    fn solve_arc_endpoint_coincident_with_line() {
        // Exercises the arc's angle-derived start point inside residuals.
        let mut sk = Sketch::new();
        let arc = sk.add_geometry(Geometry::ArcOfCircle {
            center: Point::new(1.0, 2.0),
            radius: 2.0,
            start_angle: 0.3,
            end_angle: 2.2,
        });
        let l = line(&mut sk, 5.0, 5.0, 6.0, 6.0);
        sk.add_constraint(Constraint::Coincident {
            g1: l,
            pos1: PointPos::Start,
            g2: arc,
            pos2: PointPos::Start,
        })
        .unwrap();
        match sk.solve().unwrap() {
            SolveResult::Solved { residual, .. } => assert!(residual < 1e-10),
            other => panic!("expected Solved, got {other:?}"),
        }
        let (lx, ly) = pt_coords(&sk, l, PointPos::Start);
        let (ax, ay) = pt_coords(&sk, arc, PointPos::Start);
        assert!((lx - ax).abs() < 1e-8 && (ly - ay).abs() < 1e-8);
        assert_eq!(sk.dof(), 7); // 5 + 4 - 2
    }

    #[test]
    fn solve_distance_between_two_points() {
        let mut sk = Sketch::new();
        let a = sk.add_geometry(Geometry::Point { x: 0.0, y: 0.0 });
        let b = sk.add_geometry(Geometry::Point { x: 1.0, y: 0.0 });
        sk.add_constraint(Constraint::Distance {
            g1: a,
            pos1: PointPos::Start,
            g2: b,
            pos2: PointPos::Start,
            value: 5.0,
        })
        .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (ax, ay) = pt_coords(&sk, a, PointPos::Start);
        let (bx, by) = pt_coords(&sk, b, PointPos::Start);
        assert!(((bx - ax).hypot(by - ay) - 5.0).abs() < 1e-9);
        assert_eq!(sk.dof(), 3); // 4 - 1
    }

    #[test]
    fn solve_signed_distance_x_and_y() {
        let mut sk = Sketch::new();
        let a = sk.add_geometry(Geometry::Point { x: 0.0, y: 0.0 });
        let b = sk.add_geometry(Geometry::Point { x: 1.0, y: 1.0 });
        sk.add_constraint(Constraint::DistanceX {
            g1: a,
            pos1: PointPos::Start,
            g2: b,
            pos2: PointPos::Start,
            value: 4.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: a,
            pos1: PointPos::Start,
            g2: b,
            pos2: PointPos::Start,
            value: -2.0,
        })
        .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (ax, ay) = pt_coords(&sk, a, PointPos::Start);
        let (bx, by) = pt_coords(&sk, b, PointPos::Start);
        assert!((bx - ax - 4.0).abs() < 1e-9);
        assert!((by - ay - -2.0).abs() < 1e-9);
        assert_eq!(sk.dof(), 2); // 4 - 2
    }

    #[test]
    fn solve_coincident_two_points_meet() {
        let mut sk = Sketch::new();
        let a = sk.add_geometry(Geometry::Point { x: 0.0, y: 0.0 });
        let b = sk.add_geometry(Geometry::Point { x: 1.0, y: 1.0 });
        sk.add_constraint(Constraint::Coincident {
            g1: a,
            pos1: PointPos::Start,
            g2: b,
            pos2: PointPos::Start,
        })
        .unwrap();
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (ax, ay) = pt_coords(&sk, a, PointPos::Start);
        let (bx, by) = pt_coords(&sk, b, PointPos::Start);
        assert!((ax - bx).abs() < 1e-10 && (ay - by).abs() < 1e-10);
        // Minimum-norm: both near the midpoint, loosely checked.
        assert!((ax - 0.5).abs() < 0.1 && (ay - 0.5).abs() < 0.1);
    }

    // ---------- Honesty: under/over-constrained, failure ----------

    #[test]
    fn solve_underconstrained_reports_solved_with_positive_dof() {
        let mut sk = Sketch::new();
        let l = line(&mut sk, 1.0, 2.0, 4.0, 6.0);
        sk.add_constraint(Constraint::Horizontal { g: l }).unwrap();
        match sk.solve().unwrap() {
            SolveResult::Solved { .. } => {}
            other => panic!("expected Solved, got {other:?}"),
        }
        assert_eq!(sk.dof(), 3);
        assert!(!sk.is_fully_constrained());
        let (sx, sy) = pt_coords(&sk, l, PointPos::Start);
        let (ex, ey) = pt_coords(&sk, l, PointPos::End);
        assert!((ey - sy).abs() < 1e-10);
        // x-coordinates are invisible to the residual: exactly unchanged.
        assert_eq!((sx, ex), (1.0, 4.0));
    }

    #[test]
    fn solve_unconstrained_sketch_is_trivially_solved() {
        let mut sk = Sketch::new();
        sk.add_geometry(Geometry::Circle {
            center: Point::new(1.0, 2.0),
            radius: 3.0,
        });
        let before = format!("{:?}", sk.geometry());
        match sk.solve().unwrap() {
            SolveResult::Solved {
                iterations,
                residual,
            } => {
                assert_eq!(iterations, 0);
                assert_eq!(residual, 0.0);
            }
            other => panic!("expected Solved, got {other:?}"),
        }
        assert_eq!(before, format!("{:?}", sk.geometry()));
        assert_eq!(sk.dof(), 3);
    }

    #[test]
    fn solve_overconstrained_errors_before_solving() {
        let mut sk = Sketch::new();
        let l = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        sk.add_constraint(Constraint::Horizontal { g: l }).unwrap();
        sk.add_constraint(Constraint::Vertical { g: l }).unwrap();
        sk.add_constraint(Constraint::Distance {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 5.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 3.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 1.0,
        })
        .unwrap();
        let before = format!("{:?}", sk.geometry());
        let err = sk.solve().unwrap_err();
        assert_eq!(err, SketchError::OverConstrained { excess: 1 });
        // Nothing moved: the error fires before any numerical work.
        assert_eq!(before, format!("{:?}", sk.geometry()));
    }

    #[test]
    fn solve_failure_leaves_geometry_unchanged() {
        // H + V force a zero-length line, but Distance wants 5: inconsistent
        // with dof == 1, so the solver must fail honestly.
        let mut sk = Sketch::new();
        let l = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        sk.add_constraint(Constraint::Horizontal { g: l }).unwrap();
        sk.add_constraint(Constraint::Vertical { g: l }).unwrap();
        sk.add_constraint(Constraint::Distance {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 5.0,
        })
        .unwrap();
        assert_eq!(sk.dof(), 1);
        let before = format!("{:?}", sk.geometry());
        match sk.solve().unwrap() {
            SolveResult::Failed { residual, .. } => assert!(residual > 1e-6),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(before, format!("{:?}", sk.geometry()));
    }

    #[test]
    fn solve_conflicting_dof_zero_reports_failure() {
        // 4 equations on 4 params, but H (dy = 0) contradicts DistanceY
        // (dy = 3): consistent DOF arithmetic, no solution.
        let mut sk = Sketch::new();
        let l = line(&mut sk, 0.0, 0.0, 1.0, 0.0);
        sk.add_constraint(Constraint::Horizontal { g: l }).unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 5.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 3.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::Vertical { g: l }).unwrap();
        assert_eq!(sk.dof(), 0);
        let before = format!("{:?}", sk.geometry());
        let result = sk.solve().unwrap();
        assert!(
            !matches!(result, SolveResult::Solved { .. }),
            "must not fabricate success: {result:?}"
        );
        assert_eq!(before, format!("{:?}", sk.geometry()));
    }

    #[test]
    fn solve_redundant_consistent_dof_zero_solves_and_reports_fully_constrained() {
        // H and DistanceY(0) say the same thing; Distance(4) is implied by
        // DistanceX(4) + H. Redundant but consistent: dof 0, solved.
        let mut sk = Sketch::new();
        let l = line(&mut sk, 0.1, 0.2, 3.9, -0.1);
        sk.add_constraint(Constraint::Horizontal { g: l }).unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 4.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 0.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::Distance {
            g1: l,
            pos1: PointPos::Start,
            g2: l,
            pos2: PointPos::End,
            value: 4.0,
        })
        .unwrap();
        assert_eq!(sk.dof(), 0);
        assert!(sk.is_fully_constrained());
        assert!(matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        let (sx, sy) = pt_coords(&sk, l, PointPos::Start);
        let (ex, ey) = pt_coords(&sk, l, PointPos::End);
        assert!((ey - sy).abs() < 1e-8);
        assert!((ex - sx - 4.0).abs() < 1e-8);
    }

    #[test]
    fn solve_conflicting_dof_positive_reports_failure() {
        // Triangle inequality violated: |AC| <= |AB| + |BC| cannot hold.
        let mut sk = Sketch::new();
        let a = sk.add_geometry(Geometry::Point { x: 0.0, y: 0.0 });
        let b = sk.add_geometry(Geometry::Point { x: 1.0, y: 0.0 });
        let c = sk.add_geometry(Geometry::Point { x: 2.0, y: 0.0 });
        for (g1, g2, v) in [(a, b, 1.0), (b, c, 1.0), (a, c, 2.5)] {
            sk.add_constraint(Constraint::Distance {
                g1,
                pos1: PointPos::Start,
                g2,
                pos2: PointPos::Start,
                value: v,
            })
            .unwrap();
        }
        assert_eq!(sk.dof(), 3);
        let before = format!("{:?}", sk.geometry());
        assert!(!matches!(sk.solve().unwrap(), SolveResult::Solved { .. }));
        assert_eq!(before, format!("{:?}", sk.geometry()));
    }

    // ---------- Unit-level helpers ----------

    #[test]
    fn wrap_pi_brings_angles_into_minus_pi_pi() {
        let pi = std::f64::consts::PI;
        assert!((wrap_pi(0.3) - 0.3).abs() < 1e-15);
        assert!((wrap_pi(pi) - pi).abs() < 1e-15);
        assert!((wrap_pi(1.5 * pi) + 0.5 * pi).abs() < 1e-15);
        assert!((wrap_pi(-1.5 * pi) - 0.5 * pi).abs() < 1e-15);
        assert!((wrap_pi(2.0 * pi + 0.1) - 0.1).abs() < 1e-12);
        assert!((wrap_pi(-2.0 * pi - 0.1) + 0.1).abs() < 1e-12);
    }

    #[test]
    fn classify_thresholds() {
        assert_eq!(
            classify(3, 5e-11),
            SolveResult::Solved {
                iterations: 3,
                residual: 5e-11
            }
        );
        assert_eq!(classify(3, 5e-8), SolveResult::ConvergedPartially);
        assert_eq!(
            classify(3, 5e-3),
            SolveResult::Failed {
                iterations: 3,
                residual: 5e-3
            }
        );
    }

    #[test]
    fn gauss_solve_small_dense_system() {
        // [4 1; 2 3] x = [1, 2]  =>  x = (1/10, 3/5)
        let mut a = vec![4.0, 1.0, 2.0, 3.0];
        let mut b = vec![1.0, 2.0];
        assert!(gauss_solve(2, &mut a, &mut b));
        assert!((b[0] - 0.1).abs() < 1e-12);
        assert!((b[1] - 0.6).abs() < 1e-12);
        // A singular matrix is reported, not silently "solved".
        let mut a = vec![1.0, 1.0, 1.0, 1.0];
        let mut b = vec![1.0, 2.0];
        assert!(!gauss_solve(2, &mut a, &mut b));
    }
}
