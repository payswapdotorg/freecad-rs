//! Sketch CLI commands — `freecad-rs sketch-solve` / `sketch-show`.
//!
//! Thin, honest surfaces over [`fc_sketcher`]: they load a
//! FreeCAD-style `<Sketch>` XML fragment (geometry + constraints),
//! solve it (or not — the DOF report and `SolveResult` are printed as
//! they are), and print the geometry in a compact table. The sketcher
//! is not yet wired into `Document` objects (that is the PartDesign
//! wave); these commands make the crate reachable and demo-able from
//! the product surface.

use std::fmt::Write as _;

use fc_sketcher::xml::from_xml;
use fc_sketcher::{Sketch, SolveResult};

/// `sketch-solve FILE`: solve the sketch, print the outcome + geometry.
pub fn run_solve(path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let mut sk = from_xml(&text).map_err(|e| format!("parse error: {e}"))?;
    let dof = sk.dof();
    let result = sk.solve().map_err(|e| format!("sketch error: {e}"))?;
    println!(
        "sketch: {} geometries, {} constraints, DOF {} ({})",
        sk.geometry().len(),
        sk.constraints().len(),
        dof,
        if dof == 0 {
            "fully constrained"
        } else if dof > 0 {
            "underconstrained"
        } else {
            "overconstrained"
        }
    );
    match result {
        SolveResult::Solved {
            iterations,
            residual,
        } => {
            println!("solve: Solved in {iterations} iteration(s), residual {residual:.3e}");
        }
        SolveResult::ConvergedPartially => {
            println!(
                "solve: CONVERGED PARTIALLY — residual in [1e-10, 1e-6), geometry left unchanged"
            );
        }
        SolveResult::Failed {
            iterations,
            residual,
        } => {
            println!(
                "solve: FAILED after {iterations} iteration(s), residual {residual:.3e} \
                 — geometry left unchanged"
            );
        }
    }
    print_geometry(&sk);
    Ok(())
}

/// `sketch-show FILE`: print geometry + constraints without solving.
pub fn run_show(path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let sk: Sketch = from_xml(&text).map_err(|e| format!("parse error: {e}"))?;
    println!(
        "sketch: {} geometries, {} constraints, DOF {}",
        sk.geometry().len(),
        sk.constraints().len(),
        sk.dof()
    );
    print_geometry(&sk);
    print_constraints(&sk);
    Ok(())
}

fn print_geometry(sk: &Sketch) {
    println!("geometry:");
    for (i, g) in sk.geometry().iter().enumerate() {
        let mut line = String::new();
        let _ = write!(line, "  [{i}] ");
        match *g {
            fc_sketcher::Geometry::Point { x, y } => {
                let _ = write!(line, "point   ({x:.6}, {y:.6})");
            }
            fc_sketcher::Geometry::LineSegment { start, end } => {
                let _ = write!(
                    line,
                    "line   ({:.6}, {:.6}) -> ({:.6}, {:.6}), len {:.6}",
                    start.x,
                    start.y,
                    end.x,
                    end.y,
                    ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt()
                );
            }
            fc_sketcher::Geometry::Circle { center, radius } => {
                let _ = write!(
                    line,
                    "circle ({:.6}, {:.6}) r {:.6}",
                    center.x, center.y, radius
                );
            }
            fc_sketcher::Geometry::ArcOfCircle {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let _ = write!(
                    line,
                    "arc    ({:.6}, {:.6}) r {:.6} [{:.6}..{:.6}] rad",
                    center.x, center.y, radius, start_angle, end_angle
                );
            }
        }
        println!("{line}");
    }
}

fn print_constraints(sk: &Sketch) {
    println!("constraints:");
    for (i, c) in sk.constraints().iter().enumerate() {
        println!("  [{i}] {c:?}");
    }
}
