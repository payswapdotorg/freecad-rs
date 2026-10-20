//! The v1 constraint vocabulary and its validation.
//!
//! Constraints reference geometries by [`GeoId`] and, where a specific point
//! is meant, by [`PointPos`]. [`Sketch::add_constraint`] validates every
//! reference (geometry exists, point slot exists on that geometry kind,
//! geometry kinds match the constraint's needs, values are satisfiable) and
//! rejects malformed constraints with [`SketchError`].
//!
//! # Sign and angle conventions
//!
//! - `Distance` is unsigned (point-to-point Euclidean distance); the value
//!   must be `>= 0`.
//! - `DistanceX` / `DistanceY` are *signed* and fix the **second point
//!   minus the first**: `x2 - x1 = value` / `y2 - y1 = value`. Negative
//!   values are meaningful.
//! - `Angle` fixes the CCW rotation from line 1's direction
//!   (`end - start`) to line 2's direction, in radians, modulo `2*pi`. The
//!   residual is wrap-aware, so any `value + k*2*pi` is equivalent.
//! - `Radius` / `Diameter` values must be `>= 0`; `Diameter` fixes
//!   `2 * radius`.
//! - `Equal` means equal radii for two circles/arcs, or equal lengths for
//!   two line segments. Mixed line-vs-circle pairs are rejected (the
//!   length-equals-radius interpretation is not part of v1).
//! - `PointOnObject` puts a point onto a **line segment's (infinite) line**
//!   or onto a **circle's curve**. Arcs are rejected in v1 (a point on an
//!   arc's *circle* would silently ignore the angle range).

use crate::geometry::{GeoId, Geometry, Sketch};
use crate::SketchError;

/// Which point-slot of a geometry a constraint refers to.
///
/// Ordinals are stable and used by the XML format: `1` = Start, `2` = End,
/// `3` = Center. See [`Geometry::point_at`] for the slot map per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointPos {
    /// Start point of a line, the point itself for a bare point, or the
    /// angle-derived start point of an arc.
    Start,
    /// End point of a line, or the angle-derived end point of an arc.
    End,
    /// Center point of a circle or arc.
    Center,
}

impl PointPos {
    /// Stable ordinal used in the XML schema (1/2/3).
    pub fn ordinal(self) -> u32 {
        match self {
            PointPos::Start => 1,
            PointPos::End => 2,
            PointPos::Center => 3,
        }
    }

    /// Decodes an ordinal produced by [`PointPos::ordinal`].
    pub fn from_ordinal(o: u32) -> Option<PointPos> {
        match o {
            1 => Some(PointPos::Start),
            2 => Some(PointPos::End),
            3 => Some(PointPos::Center),
            _ => None,
        }
    }
}

/// Index of a constraint inside a [`Sketch`] (assigned sequentially by
/// `Sketch::add_constraint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConstraintId(pub u32);

/// A geometric constraint from the v1 numeric subset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Constraint {
    /// Two point-slots occupy the same coordinates. Contributes 2 scalar
    /// equations (`dx`, `dy`).
    Coincident {
        /// First geometry.
        g1: GeoId,
        /// Point slot on the first geometry.
        pos1: PointPos,
        /// Second geometry.
        g2: GeoId,
        /// Point slot on the second geometry.
        pos2: PointPos,
    },
    /// A line segment's endpoints have equal Y.
    Horizontal {
        /// The line.
        g: GeoId,
    },
    /// A line segment's endpoints have equal X.
    Vertical {
        /// The line.
        g: GeoId,
    },
    /// Two line segments are parallel (either orientation).
    Parallel {
        /// First line.
        g1: GeoId,
        /// Second line.
        g2: GeoId,
    },
    /// Two line segments are perpendicular.
    Perpendicular {
        /// First line.
        g1: GeoId,
        /// Second line.
        g2: GeoId,
    },
    /// Unsigned point-to-point Euclidean distance.
    Distance {
        /// First geometry.
        g1: GeoId,
        /// Point slot on the first geometry.
        pos1: PointPos,
        /// Second geometry.
        g2: GeoId,
        /// Point slot on the second geometry.
        pos2: PointPos,
        /// Required distance (`>= 0`).
        value: f64,
    },
    /// Signed horizontal distance: `x2 - x1 = value`.
    DistanceX {
        /// First geometry.
        g1: GeoId,
        /// Point slot on the first geometry.
        pos1: PointPos,
        /// Second geometry.
        g2: GeoId,
        /// Point slot on the second geometry.
        pos2: PointPos,
        /// Required signed delta.
        value: f64,
    },
    /// Signed vertical distance: `y2 - y1 = value`.
    DistanceY {
        /// First geometry.
        g1: GeoId,
        /// Point slot on the first geometry.
        pos1: PointPos,
        /// Second geometry.
        g2: GeoId,
        /// Point slot on the second geometry.
        pos2: PointPos,
        /// Required signed delta.
        value: f64,
    },
    /// Circle/arc radius equals `value`.
    Radius {
        /// The circle or arc.
        g: GeoId,
        /// Required radius (`>= 0`).
        value: f64,
    },
    /// Circle/arc diameter equals `value` (`2 * radius = value`).
    Diameter {
        /// The circle or arc.
        g: GeoId,
        /// Required diameter (`>= 0`).
        value: f64,
    },
    /// Equal radii (circles/arcs) or equal lengths (lines).
    Equal {
        /// First geometry.
        g1: GeoId,
        /// Second geometry.
        g2: GeoId,
    },
    /// CCW angle from line 1's direction to line 2's direction, radians,
    /// modulo `2*pi` (wrap-aware).
    Angle {
        /// First line.
        g1: GeoId,
        /// Second line.
        g2: GeoId,
        /// Required angle in radians.
        value: f64,
    },
    /// A point-slot lies on another geometry's curve (line's infinite line
    /// or circle's curve).
    PointOnObject {
        /// Geometry owning the point.
        g1: GeoId,
        /// Point slot on `g1`.
        pos1: PointPos,
        /// The line or circle the point must lie on.
        g2: GeoId,
    },
}

impl Constraint {
    /// Number of scalar equations the constraint contributes to the
    /// solver's residual vector (`Coincident` -> 2, everything else -> 1).
    pub fn dimension(&self) -> usize {
        match self {
            Constraint::Coincident { .. } => 2,
            _ => 1,
        }
    }

    /// The geometries whose parameters the constraint touches (its
    /// "targets"), in reference order. These are the geometries whose
    /// solver parameters enter the constraint's residual(s).
    pub fn targets(&self) -> Vec<GeoId> {
        match *self {
            Constraint::Coincident { g1, g2, .. }
            | Constraint::Distance { g1, g2, .. }
            | Constraint::DistanceX { g1, g2, .. }
            | Constraint::DistanceY { g1, g2, .. }
            | Constraint::Parallel { g1, g2 }
            | Constraint::Perpendicular { g1, g2 }
            | Constraint::Equal { g1, g2 }
            | Constraint::Angle { g1, g2, .. }
            | Constraint::PointOnObject { g1, g2, .. } => vec![g1, g2],
            Constraint::Horizontal { g }
            | Constraint::Vertical { g }
            | Constraint::Radius { g, .. }
            | Constraint::Diameter { g, .. } => vec![g],
        }
    }
}

impl Sketch {
    /// Validates and appends a constraint.
    ///
    /// Rejects, with a specific [`SketchError`]:
    /// - references to nonexistent geometries (`UnknownGeometry`),
    /// - point slots that do not exist on the referenced geometry kind
    ///   (`InvalidPointPos`),
    /// - geometry-kind combinations v1 cannot express
    ///   (`UnsupportedConstraint`), and
    /// - unsatisfiable values — non-finite, or negative where the quantity
    ///   is unsigned (`InvalidValue`).
    ///
    /// Note: adding constraints is allowed to drive the sketch's DOF
    /// negative; that is reported by `Sketch::solve` as
    /// [`SketchError::OverConstrained`], not here.
    pub fn add_constraint(&mut self, c: Constraint) -> Result<ConstraintId, SketchError> {
        validate(self.geometry(), &c)?;
        Ok(self.push_constraint(c))
    }
}

fn validate(geos: &[Geometry], c: &Constraint) -> Result<(), SketchError> {
    let lookup = |g: GeoId| geos.get(g.as_index());

    let require_point = |g: GeoId, pos: PointPos| -> Result<(), SketchError> {
        match lookup(g) {
            None => Err(SketchError::UnknownGeometry(g)),
            Some(geo) if geo.point_at(pos).is_none() => {
                Err(SketchError::InvalidPointPos { geo: g, pos })
            }
            Some(_) => Ok(()),
        }
    };
    let require_line = |g: GeoId| -> Result<(), SketchError> {
        match lookup(g) {
            None => Err(SketchError::UnknownGeometry(g)),
            Some(Geometry::LineSegment { .. }) => Ok(()),
            Some(_) => Err(SketchError::UnsupportedConstraint(format!(
                "constraint requires a line segment, but geometry {g:?} is not"
            ))),
        }
    };
    let require_circleish = |g: GeoId| -> Result<(), SketchError> {
        match lookup(g) {
            None => Err(SketchError::UnknownGeometry(g)),
            Some(Geometry::Circle { .. }) | Some(Geometry::ArcOfCircle { .. }) => Ok(()),
            Some(_) => Err(SketchError::UnsupportedConstraint(format!(
                "constraint requires a circle or arc, but geometry {g:?} is not"
            ))),
        }
    };
    let require_finite = |constraint: &'static str, v: f64| -> Result<(), SketchError> {
        if v.is_finite() {
            Ok(())
        } else {
            Err(SketchError::InvalidValue {
                constraint,
                value: v,
            })
        }
    };
    let require_nonnegative = |constraint: &'static str, v: f64| -> Result<(), SketchError> {
        if v.is_finite() && v >= 0.0 {
            Ok(())
        } else {
            Err(SketchError::InvalidValue {
                constraint,
                value: v,
            })
        }
    };

    match *c {
        Constraint::Coincident { g1, pos1, g2, pos2 } => {
            require_point(g1, pos1)?;
            require_point(g2, pos2)?;
        }
        Constraint::Horizontal { g } | Constraint::Vertical { g } => {
            require_line(g)?;
        }
        Constraint::Parallel { g1, g2 } | Constraint::Perpendicular { g1, g2 } => {
            require_line(g1)?;
            require_line(g2)?;
        }
        Constraint::Distance {
            g1,
            pos1,
            g2,
            pos2,
            value,
        } => {
            require_point(g1, pos1)?;
            require_point(g2, pos2)?;
            require_nonnegative("Distance", value)?;
        }
        Constraint::DistanceX {
            g1,
            pos1,
            g2,
            pos2,
            value,
        }
        | Constraint::DistanceY {
            g1,
            pos1,
            g2,
            pos2,
            value,
        } => {
            require_point(g1, pos1)?;
            require_point(g2, pos2)?;
            require_finite("DistanceX/DistanceY", value)?;
        }
        Constraint::Radius { g, value } => {
            require_circleish(g)?;
            require_nonnegative("Radius", value)?;
        }
        Constraint::Diameter { g, value } => {
            require_circleish(g)?;
            require_nonnegative("Diameter", value)?;
        }
        Constraint::Equal { g1, g2 } => {
            let a = geos
                .get(g1.as_index())
                .ok_or(SketchError::UnknownGeometry(g1))?;
            let b = geos
                .get(g2.as_index())
                .ok_or(SketchError::UnknownGeometry(g2))?;
            let both_lines = matches!(a, Geometry::LineSegment { .. })
                && matches!(b, Geometry::LineSegment { .. });
            let both_circleish = matches!(
                (a, b),
                (Geometry::Circle { .. }, Geometry::Circle { .. })
                    | (Geometry::Circle { .. }, Geometry::ArcOfCircle { .. })
                    | (Geometry::ArcOfCircle { .. }, Geometry::Circle { .. })
                    | (Geometry::ArcOfCircle { .. }, Geometry::ArcOfCircle { .. })
            );
            if !both_lines && !both_circleish {
                return Err(SketchError::UnsupportedConstraint(format!(
                    "Equal requires two lines or two circles/arcs, got {g1:?} and {g2:?}"
                )));
            }
        }
        Constraint::Angle { g1, g2, value } => {
            require_line(g1)?;
            require_line(g2)?;
            require_finite("Angle", value)?;
        }
        Constraint::PointOnObject { g1, pos1, g2 } => {
            require_point(g1, pos1)?;
            match geos
                .get(g2.as_index())
                .ok_or(SketchError::UnknownGeometry(g2))?
            {
                Geometry::LineSegment { .. } | Geometry::Circle { .. } => {}
                _ => {
                    return Err(SketchError::UnsupportedConstraint(format!(
                        "PointOnObject targets a line or circle in v1, but geometry {g2:?} is not"
                    )))
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    fn demo_sketch() -> Sketch {
        let mut sk = Sketch::new();
        sk.add_geometry(Geometry::Point { x: 0.0, y: 0.0 }); // 0
        sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(4.0, 3.0),
        }); // 1
        sk.add_geometry(Geometry::Circle {
            center: Point::new(0.0, 0.0),
            radius: 2.0,
        }); // 2
        sk.add_geometry(Geometry::ArcOfCircle {
            center: Point::new(0.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            end_angle: 1.0,
        }); // 3
        sk
    }

    #[test]
    fn point_pos_ordinals() {
        assert_eq!(PointPos::Start.ordinal(), 1);
        assert_eq!(PointPos::End.ordinal(), 2);
        assert_eq!(PointPos::Center.ordinal(), 3);
        assert_eq!(PointPos::from_ordinal(1), Some(PointPos::Start));
        assert_eq!(PointPos::from_ordinal(3), Some(PointPos::Center));
        assert_eq!(PointPos::from_ordinal(0), None);
        assert_eq!(PointPos::from_ordinal(4), None);
    }

    #[test]
    fn constraint_dimensions() {
        let c = Constraint::Coincident {
            g1: GeoId(0),
            pos1: PointPos::Start,
            g2: GeoId(1),
            pos2: PointPos::Start,
        };
        assert_eq!(c.dimension(), 2);
        let singles = [
            Constraint::Horizontal { g: GeoId(1) },
            Constraint::Vertical { g: GeoId(1) },
            Constraint::Parallel {
                g1: GeoId(1),
                g2: GeoId(1),
            },
            Constraint::Perpendicular {
                g1: GeoId(1),
                g2: GeoId(1),
            },
            Constraint::Distance {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(1),
                pos2: PointPos::End,
                value: 1.0,
            },
            Constraint::DistanceX {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(1),
                pos2: PointPos::End,
                value: 1.0,
            },
            Constraint::DistanceY {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(1),
                pos2: PointPos::End,
                value: -1.0,
            },
            Constraint::Radius {
                g: GeoId(2),
                value: 1.0,
            },
            Constraint::Diameter {
                g: GeoId(3),
                value: 2.0,
            },
            Constraint::Equal {
                g1: GeoId(2),
                g2: GeoId(3),
            },
            Constraint::Angle {
                g1: GeoId(1),
                g2: GeoId(1),
                value: 0.5,
            },
            Constraint::PointOnObject {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(1),
            },
        ];
        for c in &singles {
            assert_eq!(c.dimension(), 1, "{c:?}");
        }
    }

    #[test]
    fn constraint_targets() {
        let c = Constraint::Horizontal { g: GeoId(1) };
        assert_eq!(c.targets(), vec![GeoId(1)]);
        let c = Constraint::Radius {
            g: GeoId(2),
            value: 1.0,
        };
        assert_eq!(c.targets(), vec![GeoId(2)]);
        let c = Constraint::Coincident {
            g1: GeoId(0),
            pos1: PointPos::Start,
            g2: GeoId(1),
            pos2: PointPos::End,
        };
        assert_eq!(c.targets(), vec![GeoId(0), GeoId(1)]);
        let c = Constraint::PointOnObject {
            g1: GeoId(0),
            pos1: PointPos::Start,
            g2: GeoId(2),
        };
        assert_eq!(c.targets(), vec![GeoId(0), GeoId(2)]);
    }

    #[test]
    fn add_constraint_rejects_unknown_geometry() {
        let mut sk = demo_sketch();
        let err = sk
            .add_constraint(Constraint::Horizontal { g: GeoId(42) })
            .unwrap_err();
        assert_eq!(err, SketchError::UnknownGeometry(GeoId(42)));

        let err = sk
            .add_constraint(Constraint::Coincident {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(7),
                pos2: PointPos::Start,
            })
            .unwrap_err();
        assert_eq!(err, SketchError::UnknownGeometry(GeoId(7)));

        let err = sk
            .add_constraint(Constraint::PointOnObject {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(9),
            })
            .unwrap_err();
        assert_eq!(err, SketchError::UnknownGeometry(GeoId(9)));
    }

    #[test]
    fn add_constraint_rejects_invalid_point_positions() {
        let mut sk = demo_sketch();
        // Center of a line does not exist.
        let err = sk
            .add_constraint(Constraint::Coincident {
                g1: GeoId(1),
                pos1: PointPos::Center,
                g2: GeoId(0),
                pos2: PointPos::Start,
            })
            .unwrap_err();
        assert_eq!(
            err,
            SketchError::InvalidPointPos {
                geo: GeoId(1),
                pos: PointPos::Center
            }
        );
        // Start of a circle does not exist.
        let err = sk
            .add_constraint(Constraint::Distance {
                g1: GeoId(2),
                pos1: PointPos::Start,
                g2: GeoId(0),
                pos2: PointPos::Start,
                value: 1.0,
            })
            .unwrap_err();
        assert_eq!(
            err,
            SketchError::InvalidPointPos {
                geo: GeoId(2),
                pos: PointPos::Start
            }
        );
        // End of a bare point does not exist.
        let err = sk
            .add_constraint(Constraint::DistanceX {
                g1: GeoId(0),
                pos1: PointPos::End,
                g2: GeoId(1),
                pos2: PointPos::Start,
                value: 1.0,
            })
            .unwrap_err();
        assert_eq!(
            err,
            SketchError::InvalidPointPos {
                geo: GeoId(0),
                pos: PointPos::End
            }
        );
    }

    #[test]
    fn add_constraint_rejects_wrong_geometry_kinds() {
        let mut sk = demo_sketch();
        // Horizontal on a circle.
        assert!(matches!(
            sk.add_constraint(Constraint::Horizontal { g: GeoId(2) }),
            Err(SketchError::UnsupportedConstraint(_))
        ));
        // Radius on a line.
        assert!(matches!(
            sk.add_constraint(Constraint::Radius {
                g: GeoId(1),
                value: 1.0
            }),
            Err(SketchError::UnsupportedConstraint(_))
        ));
        // Parallel between a line and a circle.
        assert!(matches!(
            sk.add_constraint(Constraint::Parallel {
                g1: GeoId(1),
                g2: GeoId(2)
            }),
            Err(SketchError::UnsupportedConstraint(_))
        ));
        // Angle between a line and a bare point.
        assert!(matches!(
            sk.add_constraint(Constraint::Angle {
                g1: GeoId(1),
                g2: GeoId(0),
                value: 1.0
            }),
            Err(SketchError::UnsupportedConstraint(_))
        ));
        // Equal between a line and a circle.
        assert!(matches!(
            sk.add_constraint(Constraint::Equal {
                g1: GeoId(1),
                g2: GeoId(2)
            }),
            Err(SketchError::UnsupportedConstraint(_))
        ));
        // PointOnObject onto a bare point.
        assert!(matches!(
            sk.add_constraint(Constraint::PointOnObject {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(0)
            }),
            Err(SketchError::UnsupportedConstraint(_))
        ));
        // PointOnObject onto an arc (v1: line or circle only).
        assert!(matches!(
            sk.add_constraint(Constraint::PointOnObject {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(3)
            }),
            Err(SketchError::UnsupportedConstraint(_))
        ));
    }

    #[test]
    fn add_constraint_rejects_bad_values() {
        let mut sk = demo_sketch();
        for bad in [f64::NAN, f64::INFINITY, -1.0] {
            let err = sk
                .add_constraint(Constraint::Distance {
                    g1: GeoId(0),
                    pos1: PointPos::Start,
                    g2: GeoId(1),
                    pos2: PointPos::Start,
                    value: bad,
                })
                .unwrap_err();
            // Exact f64 equality is impossible for NaN, so match structurally.
            assert!(
                matches!(
                    err,
                    SketchError::InvalidValue {
                        constraint: "Distance",
                        ..
                    }
                ),
                "{err:?}"
            );
        }
        assert!(matches!(
            sk.add_constraint(Constraint::Radius {
                g: GeoId(2),
                value: -0.1
            }),
            Err(SketchError::InvalidValue { .. })
        ));
        assert!(matches!(
            sk.add_constraint(Constraint::Diameter {
                g: GeoId(2),
                value: f64::NAN
            }),
            Err(SketchError::InvalidValue { .. })
        ));
        assert!(matches!(
            sk.add_constraint(Constraint::Angle {
                g1: GeoId(1),
                g2: GeoId(1),
                value: f64::NAN
            }),
            Err(SketchError::InvalidValue { .. })
        ));
        // Signed distances accept negatives.
        assert!(sk
            .add_constraint(Constraint::DistanceX {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(1),
                pos2: PointPos::End,
                value: -4.0
            })
            .is_ok());
        // Zero distance is legal (coincidence-by-dimension).
        assert!(sk
            .add_constraint(Constraint::Distance {
                g1: GeoId(0),
                pos1: PointPos::Start,
                g2: GeoId(1),
                pos2: PointPos::End,
                value: 0.0
            })
            .is_ok());
    }

    #[test]
    fn add_constraint_happy_paths_and_sequential_ids() {
        let mut sk = demo_sketch();
        let a = sk
            .add_constraint(Constraint::Horizontal { g: GeoId(1) })
            .unwrap();
        let b = sk
            .add_constraint(Constraint::Coincident {
                g1: GeoId(1),
                pos1: PointPos::Start,
                g2: GeoId(3),
                pos2: PointPos::Start, // arc start point is a valid slot
            })
            .unwrap();
        let c = sk
            .add_constraint(Constraint::Equal {
                g1: GeoId(2),
                g2: GeoId(3),
            })
            .unwrap();
        assert_eq!(
            (a, b, c),
            (ConstraintId(0), ConstraintId(1), ConstraintId(2))
        );
        assert_eq!(sk.constraints().len(), 3);
        assert!(sk.constraint_by_id(b).is_some());
        assert!(sk.constraint_by_id(ConstraintId(99)).is_none());
    }
}
