//! Sketch geometry: points, line segments, circles and arcs of circle.
//!
//! # Coordinate frame
//!
//! v1 sketches live in the **XY plane** of their own 2D frame. A sketch
//! carries no `Plane`/placement: embedding a solved sketch into 3D space is
//! the job of the document/object layer (fc-app), not of the solver. This
//! keeps every coordinate below a plain `(x, y)` pair.
//!
//! # Parametric degrees of freedom
//!
//! Every geometry kind exposes its parametric DOF via [`Geometry::dof`],
//! which doubles as the number of solver parameters the geometry owns:
//!
//! | kind                      | params                             | DOF |
//! |---------------------------|------------------------------------|-----|
//! | [`Geometry::Point`]       | `x, y`                             | 2   |
//! | [`Geometry::LineSegment`] | `start.x, start.y, end.x, end.y`  | 4   |
//! | [`Geometry::Circle`]      | `center.x, center.y, radius`       | 3   |
//! | [`Geometry::ArcOfCircle`] | `center.x, center.y, radius, start_angle, end_angle` | 5 |
//!
//! Arc angles are radians, measured CCW from `start_angle` to `end_angle`;
//! the arc's start/end points are *derived* quantities
//! (`center + radius * (cos a, sin a)`), not independent parameters.

use crate::constraint::{Constraint, ConstraintId, PointPos};

/// A 2D point / vector in the sketch plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

impl Point {
    /// Constructs a point from coordinates.
    pub fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }
}

/// Index of a geometry inside a [`Sketch`] (the "GeoId" in FreeCAD terms).
///
/// Ids are handed out sequentially by [`Sketch::add_geometry`], so `GeoId(n)`
/// is also the position of the geometry in [`Sketch::geometry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeoId(pub u32);

impl GeoId {
    /// The geometry's position as a `usize` index.
    pub fn as_index(self) -> usize {
        self.0 as usize
    }
}

/// Identifies one parametric point of one geometry — the pair
/// `(GeoId, PointPos)` packed into a single `u32`.
///
/// Encoding (an implementation detail, but stable): `geo_index * 4 + pos`
/// where `pos` is the [`PointPos`] ordinal (1 = Start, 2 = End, 3 = Center).
/// The slot value `0` is never produced by [`PointId::new`]; decoding a
/// hand-made id with slot 0 yields `None` from [`PointId::pos`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointId(pub u32);

impl PointId {
    /// Packs a `(geometry, position)` pair into a point id.
    pub fn new(geo: GeoId, pos: PointPos) -> Self {
        PointId(geo.0 * 4 + pos.ordinal())
    }

    /// The geometry the id refers to.
    pub fn geo(self) -> GeoId {
        GeoId(self.0 / 4)
    }

    /// The point position on that geometry, or `None` for an id with an
    /// empty slot (only possible for ids not produced by [`PointId::new`]).
    pub fn pos(self) -> Option<PointPos> {
        PointPos::from_ordinal(self.0 % 4)
    }
}

/// A single sketchable 2D geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geometry {
    /// A bare point. Addressed with [`PointPos::Start`] in constraints
    /// (a point has exactly one point-slot, exposed as its "start").
    Point {
        /// X coordinate.
        x: f64,
        /// Y coordinate.
        y: f64,
    },
    /// A line segment between two points.
    LineSegment {
        /// Start point.
        start: Point,
        /// End point.
        end: Point,
    },
    /// A full circle.
    Circle {
        /// Center point.
        center: Point,
        /// Radius (kept `>= 0` by construction from XML; the in-memory API
        /// trusts the caller).
        radius: f64,
    },
    /// A circular arc from `start_angle` to `end_angle` (radians, CCW).
    ArcOfCircle {
        /// Center point.
        center: Point,
        /// Radius.
        radius: f64,
        /// Angle of the arc's start point, radians.
        start_angle: f64,
        /// Angle of the arc's end point, radians.
        end_angle: f64,
    },
}

impl Geometry {
    /// The geometry's parametric degrees of freedom (== number of solver
    /// parameters it owns, see the module table).
    pub fn dof(&self) -> usize {
        match self {
            Geometry::Point { .. } => 2,
            Geometry::LineSegment { .. } => 4,
            Geometry::Circle { .. } => 3,
            Geometry::ArcOfCircle { .. } => 5,
        }
    }

    /// Resolves one of the geometry's point slots to its current
    /// coordinates, or `None` if the geometry kind has no such slot.
    ///
    /// Slot map (mirrored by `add_constraint` validation):
    ///
    /// | kind       | Start              | End                | Center |
    /// |------------|--------------------|--------------------|--------|
    /// | Point      | the point itself   | —                  | —      |
    /// | Line       | segment start      | segment end        | —      |
    /// | Circle     | —                  | —                  | center |
    /// | Arc        | `center + r*(cos a_start, sin a_start)` | `... a_end ...` | center |
    ///
    /// Note the arc's `Start`/`End` are *derived* from the angle parameters,
    /// so they move when the arc's center/radius/angles move.
    pub fn point_at(&self, pos: PointPos) -> Option<Point> {
        match (self, pos) {
            (Geometry::Point { x, y }, PointPos::Start) => Some(Point { x: *x, y: *y }),
            (Geometry::LineSegment { start, .. }, PointPos::Start) => Some(*start),
            (Geometry::LineSegment { end, .. }, PointPos::End) => Some(*end),
            (Geometry::Circle { center, .. }, PointPos::Center) => Some(*center),
            (Geometry::ArcOfCircle { center, .. }, PointPos::Center) => Some(*center),
            (
                Geometry::ArcOfCircle {
                    center,
                    radius,
                    start_angle,
                    ..
                },
                PointPos::Start,
            ) => Some(Point {
                x: center.x + radius * start_angle.cos(),
                y: center.y + radius * start_angle.sin(),
            }),
            (
                Geometry::ArcOfCircle {
                    center,
                    radius,
                    end_angle,
                    ..
                },
                PointPos::End,
            ) => Some(Point {
                x: center.x + radius * end_angle.cos(),
                y: center.y + radius * end_angle.sin(),
            }),
            _ => None,
        }
    }

    /// Packs the geometry's parameters into the solver's flat layout
    /// (order documented in the module table). Crate-internal: the layout
    /// is a solver detail.
    pub(crate) fn to_params(self) -> Vec<f64> {
        match self {
            Geometry::Point { x, y } => vec![x, y],
            Geometry::LineSegment { start, end } => vec![start.x, start.y, end.x, end.y],
            Geometry::Circle { center, radius } => vec![center.x, center.y, radius],
            Geometry::ArcOfCircle {
                center,
                radius,
                start_angle,
                end_angle,
            } => vec![center.x, center.y, radius, start_angle, end_angle],
        }
    }

    /// Overwrites the geometry's parameters from a slice produced by
    /// [`Geometry::to_params`] (same order). Panics if the length does not
    /// match [`Geometry::dof`]. Crate-internal.
    pub(crate) fn set_from_params(&mut self, params: &[f64]) {
        debug_assert_eq!(
            params.len(),
            self.dof(),
            "parameter block length must match the geometry's DOF"
        );
        match self {
            Geometry::Point { x, y } => {
                *x = params[0];
                *y = params[1];
            }
            Geometry::LineSegment { start, end } => {
                start.x = params[0];
                start.y = params[1];
                end.x = params[2];
                end.y = params[3];
            }
            Geometry::Circle { center, radius } => {
                center.x = params[0];
                center.y = params[1];
                *radius = params[2];
            }
            Geometry::ArcOfCircle {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                center.x = params[0];
                center.y = params[1];
                *radius = params[2];
                *start_angle = params[3];
                *end_angle = params[4];
            }
        }
    }
}

/// A 2D sketch: geometry plus constraints, in the XY plane.
///
/// Build one with [`Sketch::new`] / [`Sketch::add_geometry`] /
/// `Sketch::add_constraint`, then inspect its degrees of freedom with
/// [`Sketch::dof`] and solve it with `Sketch::solve` (both defined in
/// [`crate::solver`] as inherent methods).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sketch {
    geometries: Vec<Geometry>,
    constraints: Vec<Constraint>,
}

impl Sketch {
    /// Creates an empty sketch.
    pub fn new() -> Self {
        Sketch::default()
    }

    /// Appends a geometry and returns its [`GeoId`].
    ///
    /// Note: `add_geometry` is infallible by design (it performs no
    /// validation); values such as a negative circle radius are the
    /// caller's responsibility in-memory and are rejected when loading
    /// from XML.
    pub fn add_geometry(&mut self, g: Geometry) -> GeoId {
        let id = GeoId(self.geometries.len() as u32);
        self.geometries.push(g);
        id
    }

    /// All geometries, in [`GeoId`] order.
    pub fn geometry(&self) -> &[Geometry] {
        &self.geometries
    }

    /// Looks a geometry up by id, or `None` if the id is out of range.
    pub fn geometry_by_id(&self, id: GeoId) -> Option<&Geometry> {
        self.geometries.get(id.as_index())
    }

    /// Resolves a packed [`PointId`] to current coordinates.
    ///
    /// Returns `None` if the geometry does not exist or the id's slot is
    /// empty/invalid for that geometry kind.
    pub fn point(&self, id: PointId) -> Option<Point> {
        let geo = self.geometry_by_id(id.geo())?;
        let pos = id.pos()?;
        geo.point_at(pos)
    }

    /// All constraints, in the order they were added.
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// Looks a constraint up by id, or `None` if the id is out of range.
    pub fn constraint_by_id(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.get(id.0 as usize)
    }

    /// Records a validated constraint (crate-internal helper used by
    /// `Sketch::add_constraint` after validation succeeded).
    pub(crate) fn push_constraint(&mut self, c: Constraint) -> ConstraintId {
        let id = ConstraintId(self.constraints.len() as u32);
        self.constraints.push(c);
        id
    }

    /// Mutable access for the solver's write-back step (crate-internal).
    pub(crate) fn geometries_mut(&mut self) -> &mut [Geometry] {
        &mut self.geometries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_dof_table() {
        assert_eq!(Geometry::Point { x: 0.0, y: 0.0 }.dof(), 2);
        assert_eq!(
            Geometry::LineSegment {
                start: Point::new(0.0, 0.0),
                end: Point::new(1.0, 0.0)
            }
            .dof(),
            4
        );
        assert_eq!(
            Geometry::Circle {
                center: Point::new(0.0, 0.0),
                radius: 1.0
            }
            .dof(),
            3
        );
        assert_eq!(
            Geometry::ArcOfCircle {
                center: Point::new(0.0, 0.0),
                radius: 1.0,
                start_angle: 0.0,
                end_angle: std::f64::consts::FRAC_PI_2
            }
            .dof(),
            5
        );
    }

    #[test]
    fn add_geometry_assigns_sequential_ids() {
        let mut sk = Sketch::new();
        let a = sk.add_geometry(Geometry::Point { x: 1.0, y: 2.0 });
        let b = sk.add_geometry(Geometry::Point { x: 3.0, y: 4.0 });
        let c = sk.add_geometry(Geometry::Circle {
            center: Point::new(0.0, 0.0),
            radius: 2.5,
        });
        assert_eq!((a, b, c), (GeoId(0), GeoId(1), GeoId(2)));
        assert_eq!(sk.geometry().len(), 3);
        assert_eq!(
            sk.geometry()[2],
            Geometry::Circle {
                center: Point::new(0.0, 0.0),
                radius: 2.5,
            }
        );
    }

    #[test]
    fn geometry_by_id_lookup() {
        let mut sk = Sketch::new();
        let id = sk.add_geometry(Geometry::Point { x: 7.0, y: -7.0 });
        assert!(sk.geometry_by_id(id).is_some());
        assert!(sk.geometry_by_id(GeoId(99)).is_none());
        assert_eq!(
            sk.geometry_by_id(id),
            Some(&Geometry::Point { x: 7.0, y: -7.0 })
        );
    }

    #[test]
    fn point_at_resolves_every_valid_slot() {
        let pt = Geometry::Point { x: 1.0, y: 2.0 };
        assert_eq!(pt.point_at(PointPos::Start), Some(Point::new(1.0, 2.0)));
        assert_eq!(pt.point_at(PointPos::End), None);
        assert_eq!(pt.point_at(PointPos::Center), None);

        let line = Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(4.0, 3.0),
        };
        assert_eq!(line.point_at(PointPos::Start), Some(Point::new(0.0, 0.0)));
        assert_eq!(line.point_at(PointPos::End), Some(Point::new(4.0, 3.0)));
        assert_eq!(line.point_at(PointPos::Center), None);

        let circ = Geometry::Circle {
            center: Point::new(1.0, 1.0),
            radius: 2.0,
        };
        assert_eq!(circ.point_at(PointPos::Center), Some(Point::new(1.0, 1.0)));
        assert_eq!(circ.point_at(PointPos::Start), None);
        assert_eq!(circ.point_at(PointPos::End), None);

        let arc = Geometry::ArcOfCircle {
            center: Point::new(1.0, 2.0),
            radius: 2.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        };
        assert_eq!(arc.point_at(PointPos::Center), Some(Point::new(1.0, 2.0)));
        assert_eq!(arc.point_at(PointPos::Start), Some(Point::new(3.0, 2.0)));
        let end = arc.point_at(PointPos::End).unwrap();
        assert!((end.x - 1.0).abs() < 1e-15 && (end.y - 4.0).abs() < 1e-15);
    }

    #[test]
    fn point_id_pack_unpack_roundtrip() {
        for (geo, pos) in [
            (GeoId(0), PointPos::Start),
            (GeoId(3), PointPos::End),
            (GeoId(7), PointPos::Center),
        ] {
            let id = PointId::new(geo, pos);
            assert_eq!(id.geo(), geo);
            assert_eq!(id.pos(), Some(pos));
        }
        // Slot 0 decodes to None (never produced by PointId::new).
        assert_eq!(PointId(0).pos(), None);
        assert_eq!(PointId(0).geo(), GeoId(0));
    }

    #[test]
    fn sketch_point_lookup_via_point_id() {
        let mut sk = Sketch::new();
        let line = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(1.0, 2.0),
            end: Point::new(5.0, 7.0),
        });
        assert_eq!(
            sk.point(PointId::new(line, PointPos::End)),
            Some(Point::new(5.0, 7.0))
        );
        // Unknown geometry / empty slot resolve to None.
        assert_eq!(sk.point(PointId::new(GeoId(9), PointPos::Start)), None);
        assert_eq!(sk.point(PointId(4)), None); // geo 1 * 4 + 0: slot 0
    }
}
