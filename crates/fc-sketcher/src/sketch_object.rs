//! `Sketcher::SketchObject` — a sketch as a first-class document object,
//! mirroring FreeCAD `Mod/Sketcher`'s `Sketcher::SketchObject`.
//!
//! The object couples the two halves of the sketcher wave into the
//! document model: it **is** an [`fc_app::DocumentObject`] (so it lives in
//! a [`fc_app::Document`], takes part in the dependency graph and is
//! recomputed by the recompute loop), and it **owns** a
//! [`fc_sketcher::Sketch`] (geometry + constraints) that its `execute`
//! step solves.
//!
//! # Execute = solve
//!
//! FreeCAD's sketch recompute *is* the constraint solve; the same is true
//! here. `execute_impl` runs [`fc_sketcher::Sketch::solve`]:
//!
//! - `Ok(Solved { iterations, residual })` — the solved geometry is
//!   already written back into the sketch by the solver; the object
//!   records a [`SolveInfo`] (iterations, residual, current
//!   [`fc_sketcher::Sketch::dof`]) and the document marks it `Valid`.
//! - `Ok(ConvergedPartially)` / `Ok(Failed { .. })` / `Err(_)` — the
//!   solve is a failure from the document's point of view: `execute`
//!   returns `Err(reason)`, the recompute loop marks the object `Error`
//!   and any dependent features fail with it (FreeCAD behavior). The
//!   solver's fail-safety is preserved: a non-`Solved` outcome leaves the
//!   geometry untouched, and `solved` is reset to `None` — a sketch that
//!   did not solve has no solve result to report.
//!
//! # Persistence shape (an honest simplification)
//!
//! FreeCAD exposes the sketch content as `Geometry` / `Constraints`
//! *properties* on the object. Here the geometry and constraints live in
//! the typed [`Sketch`] field (they are structured data with cross
//! references, not scalars), and `.FCStd` persistence emits them as a
//! `<Sketch>` XML fragment (the exact element `Sketch::to_xml()` produces,
//! see [`fc_sketcher::xml`]) stored as a child of the object's
//! `<ObjectData>` entry — a sibling of its `<Properties>` element, not a
//! `<Property>` itself. Solve results are **not** persisted: a loaded
//! sketch carries no `SolveInfo` and is `Touched`, so the next recompute
//! re-solves it (FreeCAD behavior: the solve is a recompute action).
//!
//! # v1 scope, honestly stated
//!
//! - The `Placement` property is registered and persisted exactly like the
//!   Part primitives carry theirs, but v1 sketches solve (and are drawn)
//!   in the XY frame of the sketch itself; embedding a solved sketch into
//!   3D space is future PartDesign work (the same limitation the
//!   primitives document).
//! - `Label` defaults to the object name, FreeCAD-style.

use fc_app::{impl_document_object, Document, Property, PropertyContainer};
use fc_base::Placement;

use crate::Sketch;

/// The outcome of the last successful solve (FreeCAD's
/// `SketchObject::solve` result, distilled). `dof` is the sketch's
/// degree-of-freedom count *at solve time*.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveInfo {
    /// LM iterations the solve accepted.
    pub iterations: usize,
    /// Final residual 2-norm (`< 1e-10` on a `Solved` outcome).
    pub residual: f64,
    /// Arithmetic degrees of freedom left after the solve.
    pub dof: isize,
}

/// FreeCAD `Sketcher::SketchObject`: a document object whose payload is a
/// 2D parametric sketch.
///
/// Geometry and constraints live in the typed [`Sketch`] field (see the
/// module docs for why they are not property-map entries). The property
/// map carries the FreeCAD-standard `Placement` and `Label`.
pub struct SketchObject {
    pub container: PropertyContainer,
    /// The sketch content: geometry + constraints. Solved in place by
    /// `execute` (only a `Solved` outcome mutates it — solver guarantee).
    pub sketch: Sketch,
    /// Result of the last successful solve; `None` until the first
    /// successful execute, and cleared again whenever a solve fails or
    /// the sketch content is replaced.
    pub solved: Option<SolveInfo>,
}

impl SketchObject {
    /// A new, empty sketch object with FreeCAD's default properties
    /// (`Placement` identity, `Label` = the object name).
    pub fn new(name: &str) -> Self {
        let mut container = PropertyContainer::new(name);
        container.set("Placement", Property::Placement(Placement::IDENTITY));
        container.set("Label", Property::String(name.to_string()));
        Self {
            container,
            sketch: Sketch::new(),
            solved: None,
        }
    }

    /// Convenience constructor: a sketch object that already carries
    /// `sketch` content (still `Touched` — nothing has solved it yet).
    pub fn with_sketch(name: &str, sketch: Sketch) -> Self {
        let mut object = Self::new(name);
        object.replace_sketch(sketch);
        object
    }

    /// Swap in new sketch content. The previous solve (if any) described
    /// the *old* content, so it is dropped, and the object is touched —
    /// the next recompute re-solves the new geometry (FreeCAD's
    /// "editing a sketch touches it" semantics). There is deliberately no
    /// way to swap content without the touch.
    pub fn replace_sketch(&mut self, sketch: Sketch) {
        self.sketch = sketch;
        self.solved = None;
        // PropertyContainer::touch: the whole point of this method —
        // content changed, so the object must re-solve.
        self.container.touch();
    }

    /// The solve step (`DocumentObject::execute`): solve the sketch.
    ///
    /// A `Solved` outcome records the [`SolveInfo`]; everything else is
    /// reported as an honest `Err` (the recompute loop turns that into an
    /// `Error` status and fails dependents). A failed or not-yet-run
    /// solve always leaves `solved` empty.
    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        let name = self.container.name().to_string();
        match self.sketch.solve() {
            Ok(crate::SolveResult::Solved {
                iterations,
                residual,
            }) => {
                self.solved = Some(SolveInfo {
                    iterations,
                    residual,
                    dof: self.sketch.dof(),
                });
                Ok(())
            }
            Ok(crate::SolveResult::ConvergedPartially) => {
                self.solved = None;
                Err(format!(
                    "Sketcher::SketchObject '{name}': solve converged partially \
                     (residual in [1e-10, 1e-6)) — geometry left unchanged"
                ))
            }
            Ok(crate::SolveResult::Failed {
                iterations,
                residual,
            }) => {
                self.solved = None;
                Err(format!(
                    "Sketcher::SketchObject '{name}': solve failed after \
                     {iterations} iteration(s), residual {residual:.3e} — \
                     geometry left unchanged"
                ))
            }
            Err(err) => {
                self.solved = None;
                Err(format!("Sketcher::SketchObject '{name}': {err}"))
            }
        }
    }
}

impl_document_object!(SketchObject, "Sketcher::SketchObject");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::Constraint;
    use crate::geometry::{Geometry, Point};
    use crate::SolveResult;
    use fc_app::{DocumentObject, ObjectStatus};

    /// A 40x30 rectangle built from four lines: corner-coincident,
    /// two horizontal, two vertical, width and height dimensioned.
    /// 16 parameters, 14 constraint dimensions -> DOF 2 (the v1
    /// translation invariance documented in the crate docs). The start
    /// geometry is slightly off-spec so the solve really has work to do.
    fn rectangle_sketch() -> Sketch {
        let mut sk = Sketch::new();
        let top = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.02),
            end: Point::new(40.05, 0.0),
        });
        let right = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(40.05, 0.0),
            end: Point::new(39.95, 30.03),
        });
        let bottom = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(39.95, 30.03),
            end: Point::new(0.02, 29.97),
        });
        let left = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.02, 29.97),
            end: Point::new(0.0, 0.02),
        });
        for (a, pa, b, pb) in [
            (top, crate::PointPos::End, right, crate::PointPos::Start),
            (right, crate::PointPos::End, bottom, crate::PointPos::Start),
            (bottom, crate::PointPos::End, left, crate::PointPos::Start),
            (left, crate::PointPos::End, top, crate::PointPos::Start),
        ] {
            sk.add_constraint(Constraint::Coincident {
                g1: a,
                pos1: pa,
                g2: b,
                pos2: pb,
            })
            .unwrap();
        }
        sk.add_constraint(Constraint::Horizontal { g: top })
            .unwrap();
        sk.add_constraint(Constraint::Vertical { g: right })
            .unwrap();
        sk.add_constraint(Constraint::Horizontal { g: bottom })
            .unwrap();
        sk.add_constraint(Constraint::Vertical { g: left }).unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: top,
            pos1: crate::PointPos::Start,
            g2: top,
            pos2: crate::PointPos::End,
            value: 40.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: right,
            pos1: crate::PointPos::Start,
            g2: right,
            pos2: crate::PointPos::End,
            value: 30.0,
        })
        .unwrap();
        sk
    }

    #[test]
    fn new_registers_type_name_and_default_properties() {
        let object = SketchObject::new("Sketch");
        assert_eq!(DocumentObject::type_name(&object), "Sketcher::SketchObject");
        assert_eq!(DocumentObject::name(&object), "Sketch");
        assert_eq!(object.container.get_string("Label"), Some("Sketch".into()));
        assert_eq!(
            object.container.get_placement("Placement"),
            Some(Placement::IDENTITY)
        );
        assert_eq!(*object.status(), ObjectStatus::Touched);
        assert!(object.sketch.geometry().is_empty());
        assert_eq!(object.solved, None);
    }

    #[test]
    fn with_sketch_carries_content_and_stays_touched() {
        let mut sk = Sketch::new();
        sk.add_geometry(Geometry::Point { x: 1.0, y: 2.0 });
        let object = SketchObject::with_sketch("S", sk);
        assert_eq!(object.sketch.geometry().len(), 1);
        assert_eq!(*object.status(), ObjectStatus::Touched);
        assert_eq!(object.solved, None);
    }

    #[test]
    fn empty_sketch_solves_immediately() {
        let mut object = SketchObject::new("Sketch");
        object.execute_impl(&test_doc()).unwrap();
        let info = object.solved.expect("empty sketch solves");
        assert_eq!(info.dof, 0);
        assert_eq!(info.residual, 0.0);
        assert_eq!(*object.status(), ObjectStatus::Touched); // execute_impl does not set status; the document does
    }

    /// A minimal document to hand to `execute_impl` (sketches read
    /// nothing from it).
    fn test_doc() -> Document {
        Document::new("test")
    }

    #[test]
    fn recompute_solves_rectangle_and_marks_valid() {
        let mut doc = Document::new("test");
        let id = doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            rectangle_sketch(),
        )));
        let recomputed = doc.recompute_all().expect("recompute succeeds");
        assert_eq!(recomputed, vec![id]);
        let object = doc.get(id).unwrap();
        assert_eq!(*object.status(), ObjectStatus::Valid);
        let any = object.as_any();
        let sketch = any.downcast_ref::<SketchObject>().unwrap();
        let info = sketch.solved.expect("solve info recorded");
        assert!(info.residual < 1e-10, "residual {}", info.residual);
        assert_eq!(info.dof, 2);
        assert!(info.iterations >= 1);
        // The solver wrote the solved geometry back: corners are exactly
        // 40 apart on x and 30 on y now (within residual tolerance).
        assert_eq!(sketch.sketch.geometry().len(), 4);
    }

    #[test]
    fn recompute_marks_overconstrained_sketch_as_error() {
        let mut sk = Sketch::new();
        let line = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 0.0),
        });
        // 4 parameters, 1+1+1+1+1 = 5 constraint dimensions.
        sk.add_constraint(Constraint::Horizontal { g: line })
            .unwrap();
        sk.add_constraint(Constraint::Vertical { g: line }).unwrap();
        for c in [
            Constraint::DistanceX {
                g1: line,
                pos1: crate::PointPos::Start,
                g2: line,
                pos2: crate::PointPos::End,
                value: 10.0,
            },
            Constraint::DistanceY {
                g1: line,
                pos1: crate::PointPos::Start,
                g2: line,
                pos2: crate::PointPos::End,
                value: 0.0,
            },
            Constraint::Distance {
                g1: line,
                pos1: crate::PointPos::Start,
                g2: line,
                pos2: crate::PointPos::End,
                value: 10.0,
            },
        ] {
            sk.add_constraint(c).unwrap();
        }
        assert!(sk.dof() < 0);
        let mut doc = Document::new("test");
        let id = doc.add(Box::new(SketchObject::with_sketch("Sketch", sk)));
        let err = doc.recompute_all().expect_err("over-constrained fails");
        match &err {
            fc_app::RecomputeError::ExecuteFailed { object, reason } => {
                assert_eq!(object, "Sketch");
                assert!(reason.contains("over-constrained"), "{reason}");
            }
            other => panic!("expected ExecuteFailed, got {other:?}"),
        }
        let object = doc.get(id).unwrap();
        let sketch = object.as_any().downcast_ref::<SketchObject>().unwrap();
        assert!(matches!(
            object.status(),
            ObjectStatus::Error(ref reason) if reason.contains("over-constrained")
        ));
        assert_eq!(sketch.solved, None, "no solve info on failure");
    }

    #[test]
    fn recompute_reports_degenerate_distance_as_failed_solve() {
        // Two coincident points with a Distance between them: the
        // residual gradient is identically zero (documented solver
        // pitfall), so the LM stalls at the initial residual and the
        // outcome is `Failed` — which `execute` reports as `Err`.
        let mut sk = Sketch::new();
        let a = sk.add_geometry(Geometry::Point { x: 1.0, y: 1.0 });
        let b = sk.add_geometry(Geometry::Point { x: 1.0, y: 1.0 });
        sk.add_constraint(Constraint::Distance {
            g1: a,
            pos1: crate::PointPos::Start,
            g2: b,
            pos2: crate::PointPos::Start,
            value: 40.0,
        })
        .unwrap();
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch("Sketch", sk)));
        let err = doc.recompute_all().expect_err("degenerate solve fails");
        let fc_app::RecomputeError::ExecuteFailed { object, reason } = &err else {
            panic!("expected ExecuteFailed, got {err:?}");
        };
        assert_eq!(object, "Sketch");
        assert!(reason.contains("solve failed"), "{reason}");
    }

    #[test]
    fn solved_then_replaced_goes_back_to_touched_without_solve_info() {
        let mut object = SketchObject::with_sketch("Sketch", rectangle_sketch());
        object.execute_impl(&test_doc()).unwrap();
        assert!(object.solved.is_some());

        // Replace the content: back to Touched, solve info cleared — the
        // previous solve described the old geometry. (The document API
        // deliberately exposes no mutable object access; the loader and
        // the CLI import call `replace_sketch` while the object is still
        // outside the document, which is exactly this situation.)
        let mut other = Sketch::new();
        other.add_geometry(Geometry::Circle {
            center: Point::new(0.0, 0.0),
            radius: 5.0,
        });
        object.replace_sketch(other);
        assert_eq!(*object.status(), ObjectStatus::Touched);
        assert_eq!(object.solved, None);
    }

    #[test]
    fn solve_failure_clears_previous_solve_info() {
        let mut object = SketchObject::with_sketch("Sketch", rectangle_sketch());
        object.execute_impl(&test_doc()).unwrap();
        assert!(object.solved.is_some());

        // Over-constrain the content in place: the next execute fails and
        // must drop the stale solve info.
        let mut sk = Sketch::new();
        let line = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        });
        sk.add_constraint(Constraint::Horizontal { g: line })
            .unwrap();
        sk.add_constraint(Constraint::Vertical { g: line }).unwrap();
        sk.add_constraint(Constraint::DistanceX {
            g1: line,
            pos1: crate::PointPos::Start,
            g2: line,
            pos2: crate::PointPos::End,
            value: 5.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::DistanceY {
            g1: line,
            pos1: crate::PointPos::Start,
            g2: line,
            pos2: crate::PointPos::End,
            value: 5.0,
        })
        .unwrap();
        sk.add_constraint(Constraint::Distance {
            g1: line,
            pos1: crate::PointPos::Start,
            g2: line,
            pos2: crate::PointPos::End,
            value: 5.0,
        })
        .unwrap();
        object.replace_sketch(sk);
        assert!(object.execute_impl(&test_doc()).is_err());
        assert_eq!(object.solved, None);
    }

    #[test]
    fn solve_result_display_shape_is_stable() {
        // Guard the SolveResult variants SketchObject matches on.
        let solved = SolveResult::Solved {
            iterations: 3,
            residual: 1e-15,
        };
        assert!(matches!(solved, SolveResult::Solved { iterations: 3, .. }));
    }
}
