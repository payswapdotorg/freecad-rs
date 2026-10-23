//! `fc-sketcher` — the 2D parametric sketcher, mirroring FreeCAD's `Mod/Sketcher`.
//!
//! A sketch is a set of 2D geometries (points, line segments, circles, arcs)
//! tied together by geometric constraints (coincidence, tangency-free v1
//! subset: horizontal/vertical, parallel/perpendicular, distances, radii,
//! equality, angles, point-on-object). The constraint system is assembled
//! into a vector of scalar residuals and solved numerically with a
//! Gauss-Newton / Levenberg-Marquardt solver, so the geometry snaps to
//! whatever shape satisfies the constraints while staying as close as
//! possible to the drawn input.
//!
//! Clean-room note: this crate re-implements the *behavior* described in the
//! WOG work order from scratch. It contains no FreeCAD code and shares no
//! source with the C++ original.
//!
//! # Module map
//!
//! - [`geometry`] — [`Sketch`], [`Geometry`] and the id newtypes
//!   ([`GeoId`], [`PointId`])
//! - [`constraint`] — the [`Constraint`] vocabulary, `add_constraint` and
//!   its validation
//! - [`solver`] — `Sketch::solve`, `Sketch::dof`, [`SolveResult`]
//! - [`xml`] — FreeCAD-style XML fragments (`Sketch::to_xml`,
//!   [`xml::from_xml`])
//! - [`sketch_object`] — the document-object wrapper
//!   ([`SketchObject`], mirroring FreeCAD `Sketcher::SketchObject`;
//!   depends on `fc-app`, just like Mod/Sketcher depends on App)
//! - [`preservation`] — the raw round-trip shadow of a loaded sketch
//!   ([`SketchPreservation`])
//!
//! # v1 scope, honestly stated
//!
//! - Sketches live in the **XY plane**. There is no `Plane` placement in v1:
//!   every coordinate is a plain 2D `(x, y)` in the sketch's own frame.
//! - There is **no origin point / axes geometry** in v1, and every
//!   constraint in the v1 vocabulary is invariant under a rigid translation
//!   of the whole sketch. Consequence: any *consistent* sketch that contains
//!   geometry keeps at least 2 degrees of freedom (global translation), so
//!   `Sketch::dof()` reports `>= 2` for solvable non-empty sketches. A
//!   rectangle constrained by width/height therefore reports
//!   `dof == 2`, not `0` — FreeCAD reaches `0` by anchoring a corner to the
//!   sketch origin, which is out of v1 scope.
//!
//! # Example
//!
//! ```
//! use fc_sketcher::constraint::{Constraint, PointPos};
//! use fc_sketcher::geometry::{Geometry, Point, Sketch};
//! use fc_sketcher::SolveResult;
//!
//! let mut sk = Sketch::new();
//! let line = sk.add_geometry(Geometry::LineSegment {
//!     start: Point { x: 0.0, y: 0.0 },
//!     end: Point { x: 1.0, y: 1.0 },
//! });
//! sk.add_constraint(Constraint::Horizontal { g: line }).unwrap();
//!
//! match sk.solve().unwrap() {
//!     SolveResult::Solved { .. } => {}
//!     other => panic!("expected solve to succeed, got {other:?}"),
//! }
//! let geo = &sk.geometry()[0];
//! let (a, b) = (
//!     geo.point_at(PointPos::Start).unwrap(),
//!     geo.point_at(PointPos::End).unwrap(),
//! );
//! assert!((a.y - b.y).abs() < 1e-8); // now horizontal
//! assert_eq!(sk.dof(), 3); // 4 params - 1 equation
//! ```

pub mod constraint;
pub mod geometry;
pub mod preservation;
pub mod sketch_object;
pub mod solver;
pub mod xml;

pub use constraint::{Constraint, ConstraintId, PointPos};
pub use geometry::{GeoId, Geometry, Point, PointId, Sketch};
pub use preservation::SketchPreservation;
pub use sketch_object::{SketchObject, SolveInfo};
pub use solver::SolveResult;

use std::fmt;

/// Errors produced while building, solving, or parsing a [`Sketch`].
#[derive(Debug, Clone, PartialEq)]
pub enum SketchError {
    /// A constraint refers to a geometry id that does not exist.
    UnknownGeometry(GeoId),
    /// A constraint refers to a point position that the geometry kind does
    /// not have (e.g. `Center` of a line segment).
    InvalidPointPos {
        /// The offending geometry.
        geo: GeoId,
        /// The point position that does not exist on it.
        pos: PointPos,
    },
    /// The constraint is well-formed but combines geometry kinds that the
    /// v1 solver does not support (e.g. `Equal` between a line and a
    /// circle).
    UnsupportedConstraint(String),
    /// A constraint carries a value that can never be satisfied
    /// (non-finite, or negative where the quantity is unsigned).
    InvalidValue {
        /// Name of the constraint kind, e.g. `"Distance"`.
        constraint: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// The sketch has more constraint equations than parameters.
    /// `excess` is by how many.
    OverConstrained {
        /// Number of redundant constraint dimensions (params - equations,
        /// negated).
        excess: usize,
    },
    /// A sketch XML fragment could not be parsed.
    Xml(String),
}

impl fmt::Display for SketchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SketchError::UnknownGeometry(g) => {
                write!(f, "constraint refers to unknown geometry {g:?}")
            }
            SketchError::InvalidPointPos { geo, pos } => {
                write!(f, "geometry {geo:?} has no {pos:?} point")
            }
            SketchError::UnsupportedConstraint(msg) => {
                write!(f, "unsupported constraint: {msg}")
            }
            SketchError::InvalidValue { constraint, value } => {
                write!(f, "invalid value {value} for a {constraint} constraint")
            }
            SketchError::OverConstrained { excess } => write!(
                f,
                "sketch is over-constrained by {excess} constraint dimension(s)"
            ),
            SketchError::Xml(msg) => write!(f, "invalid sketch XML: {msg}"),
        }
    }
}

impl std::error::Error for SketchError {}
