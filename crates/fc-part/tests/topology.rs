//! K1 acceptance — the exact topology model (`topology.rs`) and the
//! structural BRep payload (FCBREP v2).
//!
//! Written BEFORE the implementation (the acceptance-first discipline):
//! every assertion below is the contract for `fc_part::topology` —
//! vertices/edges/faces carrying parametric curves & surfaces (line,
//! circle, ellipse, plane, cylinder, cone, sphere, torus, B-spline) —
//! and for `fc_part::brep`'s v2 payload round-trip.
//!
//! Conventions asserted here (ours, documented in the module):
//! * A **closed** edge (a full circle) carries ONE vertex:
//!   `start == end == Some(v)`.
//! * A **degenerate** edge (zero 3D length) is a `Circle { radius: 0 }`
//!   (sphere poles, plain-cone apex). Degenerate edges are excluded
//!   from the Euler count: `chi = V − E_nondegenerate + F`.
//! * Every generated primitive is a closed shell: every non-degenerate
//!   edge is used exactly twice by face wires (a seam counts twice).
//! * Surface parameterizations follow the analytic kinds' documented
//!   frames; the generators write axis-aligned unit frames.

use fc_base::Vector3;
use fc_part::brep::{read_brep_bytes, read_shape_bytes, write_brep, write_shape};
use fc_part::topology::{Curve, ExactBRep, Surface};
use fc_part::TopoShape;

const PI: f64 = std::f64::consts::PI;
const TAU: f64 = 2.0 * PI;

// ---------------------------------------------------------------------------
// helpers (test-side, independent of the implementation's math)
// ---------------------------------------------------------------------------

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

fn approx(a: f64, b: f64, what: &str) {
    let scale = a.abs().max(b.abs()).max(1.0);
    assert!(
        (a - b).abs() <= 1e-9 * scale,
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

/// Is `p` on the given analytic surface? Independent per-kind checks in
/// the shape's axis-aligned home coordinates (how the generators build it).
fn on_surface(s: &Surface, p: Vector3, what: &str) {
    match s {
        Surface::Plane { origin, normal, .. } => {
            // Distance along the (unit) normal is zero.
            let d = (p.x - origin.x) * normal.x
                + (p.y - origin.y) * normal.y
                + (p.z - origin.z) * normal.z;
            approx(d, 0.0, what);
        }
        Surface::Cylinder {
            origin,
            axis,
            ref_dir,
            radius,
        } => {
            // Radial distance from the axis equals the radius.
            let _ = (origin, ref_dir);
            let rho = if axis.z.abs() > 0.5 {
                (p.x * p.x + p.y * p.y).sqrt()
            } else {
                (p.x * p.x + p.z * p.z).sqrt()
            };
            approx(rho, *radius, what);
        }
        Surface::Sphere { center, radius, .. } => {
            let d =
                ((p.x - center.x).powi(2) + (p.y - center.y).powi(2) + (p.z - center.z).powi(2))
                    .sqrt();
            approx(d, *radius, what);
        }
        Surface::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            let rho = (p.x * p.x + p.y * p.y).sqrt();
            approx(
                (rho - major_radius).powi(2) + p.z * p.z,
                minor_radius * minor_radius,
                what,
            );
        }
        Surface::Cone {
            ref_radius,
            semi_angle,
            ..
        } => {
            // In the generator's frame: radius at parameter v grows from
            // ref_radius by v*sin(semi); z grows by v*cos(semi) — so the
            // radius at height z is ref_radius + z*tan(semi).
            let rho = (p.x * p.x + p.y * p.y).sqrt();
            let expected = ref_radius + p.z * semi_angle.tan();
            approx(rho, expected, what);
        }
        Surface::Bspline { .. } => { /* covered by the dedicated surface tests */ }
    }
}

/// Structural invariants every generated primitive must satisfy.
fn assert_closed_shell(brep: &ExactBRep, chi_expected: i64, what: &str) {
    // The full battery: structural validation, Euler, 2-use edges,
    // on-surface edge sampling.
    brep.validate().expect(what);

    assert_eq!(
        brep.euler_characteristic(),
        chi_expected,
        "{what}: Euler characteristic"
    );

    for e in 0..brep.edges.len() {
        if brep.is_degenerate(e) {
            // Degenerate edges (sphere poles, cone apex) are used at
            // least once but need no pairing.
            assert!(brep.edge_uses(e) >= 1, "{what}: degenerate edge {e} unused");
        } else {
            assert_eq!(
                brep.edge_uses(e),
                2,
                "{what}: non-degenerate edge {e} used {} times (want exactly 2)",
                brep.edge_uses(e)
            );
        }
    }

    // Every wire edge lies on its face's surface.
    for (fi, face) in brep.faces.iter().enumerate() {
        for wire in &face.wires {
            for we in &wire.edges {
                let edge = &brep.edges[we.edge];
                for k in 0..=6 {
                    let t = edge.range.0 + (edge.range.1 - edge.range.0) * (k as f64) / 6.0;
                    let p = edge.curve.point(t);
                    on_surface(
                        &face.surface,
                        p,
                        &format!("{what}: face {fi} edge {} sample {k}", we.edge),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Box
// ---------------------------------------------------------------------------

#[test]
fn box_topology_is_exact_and_closed() {
    let (l, w, h) = (30.0, 20.0, 10.0);
    let brep = ExactBRep::from_shape(&TopoShape::Box {
        length: l,
        width: w,
        height: h,
    })
    .expect("box topology");

    // 8 vertices — the corners, shared.
    assert_eq!(brep.vertices.len(), 8);
    // 12 line edges, 6 planar faces.
    assert_eq!(brep.edges.len(), 12);
    for e in &brep.edges {
        assert!(matches!(e.curve, Curve::Line { .. }), "box edges are lines");
    }
    let planes: Vec<&Surface> = brep
        .faces
        .iter()
        .map(|f| &f.surface)
        .filter(|s| matches!(s, Surface::Plane { .. }))
        .collect();
    assert_eq!(planes.len(), 6, "box faces are planes");

    // The outward normals are exactly ±X, ±Y, ±Z once each.
    let mut normals: Vec<Vector3> = brep
        .faces
        .iter()
        .map(|f| match &f.surface {
            Surface::Plane { normal, .. } => *normal,
            _ => panic!("box face not a plane"),
        })
        .collect();
    normals.sort_by(|a, b| (a.x, a.y, a.z).partial_cmp(&(b.x, b.y, b.z)).unwrap());
    let want = [
        v3(-1., 0., 0.),
        v3(0., -1., 0.),
        v3(0., 0., -1.),
        v3(0., 0., 1.),
        v3(0., 1., 0.),
        v3(1., 0., 0.),
    ];
    for (got, want) in normals.iter().zip(want) {
        vec_approx(*got, want, "box face normal set");
    }

    // Spot-check the +X face: plane through (l,0,0), spans Y then Z.
    let px = brep
        .faces
        .iter()
        .find(|f| matches!(&f.surface, Surface::Plane { normal, .. } if normal.x == 1.0))
        .expect("+X face");
    match &px.surface {
        Surface::Plane { origin, x_dir, .. } => {
            vec_approx(*origin, v3(l, 0., 0.), "+X face origin");
            // x_dir spans the width (Y) and the frame's second axis is Z.
            vec_approx(*x_dir, v3(0., 1., 0.), "+X face x_dir");
            assert_eq!(px.uv_bounds[1] - px.uv_bounds[0], w, "+X face u span");
            assert_eq!(px.uv_bounds[3] - px.uv_bounds[2], h, "+X face v span");
        }
        _ => unreachable!(),
    }

    // Endpoints are exact: every edge's curve hits its vertices bit-exactly.
    for e in &brep.edges {
        vec_approx(
            e.curve.point(e.range.0),
            brep.vertices[e.start].point,
            "box edge start",
        );
        vec_approx(
            e.curve.point(e.range.1),
            brep.vertices[e.end].point,
            "box edge end",
        );
    }

    assert_closed_shell(&brep, 2, "box");
}

// ---------------------------------------------------------------------------
// Cylinder
// ---------------------------------------------------------------------------

#[test]
fn cylinder_topology_is_exact_and_closed() {
    let (r, h) = (5.0, 12.0);
    let brep = ExactBRep::from_shape(&TopoShape::Cylinder {
        radius: r,
        height: h,
    })
    .expect("cylinder topology");

    // 2 vertices: the seam's base and top points.
    assert_eq!(brep.vertices.len(), 2);
    vec_approx(
        brep.vertices[0].point,
        v3(r, 0., 0.),
        "cylinder v0 (seam base)",
    );
    vec_approx(
        brep.vertices[1].point,
        v3(r, 0., h),
        "cylinder v1 (seam top)",
    );

    // 3 edges: base circle, top circle, seam line.
    assert_eq!(brep.edges.len(), 3);
    let mut circles = 0;
    let mut lines = 0;
    for e in &brep.edges {
        match &e.curve {
            Curve::Circle { center, radius, .. } => {
                circles += 1;
                approx(*radius, r, "cylinder circle radius");
                let _ = center;
            }
            Curve::Line { origin, direction } => {
                lines += 1;
                vec_approx(*origin, v3(r, 0., 0.), "seam origin");
                vec_approx(*direction, v3(0., 0., h), "seam direction");
                assert_eq!(e.start, 0);
                assert_eq!(e.end, 1);
            }
            other => panic!("unexpected cylinder edge kind: {}", other.kind_name()),
        }
    }
    assert_eq!((circles, lines), (2, 1));

    // The base circle is a full turn around +Z through (r, 0, 0),
    // CCW seen from +Z (the parameterization sweeps counterclockwise).
    let base = &brep.edges[0];
    match &base.curve {
        Curve::Circle { axis, ref_dir, .. } => {
            vec_approx(*axis, v3(0., 0., 1.), "base circle axis");
            vec_approx(*ref_dir, v3(1., 0., 0.), "base circle ref_dir");
        }
        _ => unreachable!(),
    }
    assert_eq!((base.range.0, base.range.1), (0.0, TAU));
    assert_eq!(base.start, base.end, "closed edge carries one vertex");
    assert_eq!(base.start, 0, "base circle closes on the seam base");

    // 3 faces: the mantle + two caps. The mantle spans the full turn.
    assert_eq!(brep.faces.len(), 3);
    let mantle = brep
        .faces
        .iter()
        .find(|f| matches!(&f.surface, Surface::Cylinder { .. }))
        .expect("mantle face");
    match &mantle.surface {
        Surface::Cylinder { radius, .. } => {
            approx(*radius, r, "mantle radius");
            assert_eq!(mantle.uv_bounds, [0.0, TAU, 0.0, h]);
        }
        _ => unreachable!(),
    }
    // The mantle's outer wire: seam up, top circle, seam down, base circle.
    assert_eq!(mantle.wires.len(), 1);
    assert_eq!(mantle.wires[0].edges.len(), 4);

    // The caps are planes at z = 0 (normal −Z) and z = h (normal +Z).
    for f in &brep.faces {
        if let Surface::Plane { origin, normal, .. } = &f.surface {
            if normal.z > 0.0 {
                vec_approx(*origin, v3(0., 0., h), "top cap origin");
            } else {
                vec_approx(*origin, v3(0., 0., 0.), "bottom cap origin");
            }
        }
    }

    assert_closed_shell(&brep, 2, "cylinder");
}

// ---------------------------------------------------------------------------
// Cone (frustum and plain)
// ---------------------------------------------------------------------------

#[test]
fn cone_frustum_topology_is_exact_and_closed() {
    let (r1, r2, h) = (6.0, 3.0, 9.0);
    let brep = ExactBRep::from_shape(&TopoShape::Cone {
        radius1: r1,
        radius2: r2,
        height: h,
    })
    .expect("frustum topology");

    assert_eq!(brep.vertices.len(), 2);
    vec_approx(brep.vertices[0].point, v3(r1, 0., 0.), "frustum seam base");
    vec_approx(brep.vertices[1].point, v3(r2, 0., h), "frustum seam top");

    assert_eq!(brep.edges.len(), 3);
    let mut radii = Vec::new();
    for e in &brep.edges {
        if let Curve::Circle { radius, .. } = &e.curve {
            radii.push(*radius);
        }
    }
    radii.sort_by(|a, b| a.partial_cmp(b).unwrap());
    approx(radii[0], r2, "frustum top circle radius");
    approx(radii[1], r1, "frustum base circle radius");

    // The seam line runs base→top along the slant.
    let seam = brep
        .edges
        .iter()
        .find(|e| matches!(e.curve, Curve::Line { .. }))
        .expect("frustum seam");
    match &seam.curve {
        Curve::Line { origin, direction } => {
            vec_approx(*origin, v3(r1, 0., 0.), "frustum seam origin");
            vec_approx(*direction, v3(r2 - r1, 0., h), "frustum seam direction");
        }
        _ => unreachable!(),
    }

    // The conical mantle: semi-angle atan2(r2−r1, h), ref radius r1,
    // v spans the slant length.
    let slant = (h * h + (r1 - r2) * (r1 - r2)).sqrt();
    assert_eq!(brep.faces.len(), 3);
    let mantle = brep
        .faces
        .iter()
        .find(|f| matches!(&f.surface, Surface::Cone { .. }))
        .expect("cone mantle");
    match &mantle.surface {
        Surface::Cone {
            semi_angle,
            ref_radius,
            ..
        } => {
            approx(*semi_angle, (r2 - r1).atan2(h), "frustum semi-angle");
            approx(*ref_radius, r1, "frustum ref radius");
            assert_eq!(mantle.uv_bounds, [0.0, TAU, 0.0, slant]);
        }
        _ => unreachable!(),
    }

    assert_closed_shell(&brep, 2, "cone frustum");
}

#[test]
fn plain_cone_topology_has_a_degenerate_apex() {
    let (r1, h) = (6.0, 9.0);
    let brep = ExactBRep::from_shape(&TopoShape::Cone {
        radius1: r1,
        radius2: 0.0,
        height: h,
    })
    .expect("plain cone topology");

    // 2 vertices: seam base + the apex. 3 edges: base circle, apex
    // degenerate circle (radius 0), seam line. 2 faces: mantle + base.
    assert_eq!(brep.vertices.len(), 2);
    vec_approx(brep.vertices[1].point, v3(0., 0., h), "cone apex");

    assert_eq!(brep.edges.len(), 3);
    let apex_idx = brep
        .edges
        .iter()
        .enumerate()
        .find(|(_, e)| matches!(&e.curve, Curve::Circle { radius, .. } if *radius == 0.0))
        .map(|(i, _)| i)
        .expect("the apex edge is a zero-radius circle");
    assert!(brep.is_degenerate(apex_idx), "the apex edge is degenerate");
    let apex = &brep.edges[apex_idx];
    match &apex.curve {
        Curve::Circle { center, radius, .. } => {
            approx(*radius, 0.0, "apex circle radius");
            vec_approx(*center, v3(0., 0., h), "apex circle center");
        }
        other => panic!("apex edge is a {}", other.kind_name()),
    }
    assert_eq!(apex.start, apex.end);
    assert_eq!(apex.start, 1, "apex edge closes on the apex vertex");

    assert_eq!(brep.faces.len(), 2);
    // Euler: V − E_nondegenerate + F = 2 − 2 + 2 = 2 (the degenerate
    // apex edge is excluded from the count).
    assert_closed_shell(&brep, 2, "plain cone");
}

// ---------------------------------------------------------------------------
// Sphere
// ---------------------------------------------------------------------------

#[test]
fn sphere_topology_has_a_seam_and_degenerate_poles() {
    let r = 4.5;
    let brep = ExactBRep::from_shape(&TopoShape::Sphere { radius: r }).expect("sphere topology");

    // 2 vertices (the poles), 3 edges (seam arc + 2 degenerate pole
    // circles), 1 face.
    assert_eq!(brep.vertices.len(), 2);
    vec_approx(brep.vertices[0].point, v3(0., 0., -r), "south pole");
    vec_approx(brep.vertices[1].point, v3(0., 0., r), "north pole");
    assert_eq!(brep.edges.len(), 3);
    assert_eq!(brep.faces.len(), 1);

    let face = &brep.faces[0];
    match &face.surface {
        Surface::Sphere { center, radius, .. } => {
            vec_approx(*center, v3(0., 0., 0.), "sphere center");
            approx(*radius, r, "sphere radius");
            assert_eq!(face.uv_bounds, [0.0, TAU, -PI / 2.0, PI / 2.0]);
        }
        _ => unreachable!(),
    }

    // The seam: a meridian arc from SOUTH to NORTH through +X.
    let seam = brep
        .edges
        .iter()
        .find(|e| matches!(&e.curve, Curve::Circle { radius, .. } if *radius > 0.0))
        .expect("seam edge");
    assert_eq!(seam.start, 0);
    assert_eq!(seam.end, 1);
    assert_eq!(seam.range, (-PI / 2.0, PI / 2.0));
    // Endpoints and midpoint hit the poles and +X exactly (they are the
    // same evaluations the vertex table was built from).
    vec_approx(
        seam.curve.point(seam.range.0),
        v3(0., 0., -r),
        "seam start = south",
    );
    vec_approx(
        seam.curve.point(seam.range.1),
        v3(0., 0., r),
        "seam end = north",
    );
    vec_approx(seam.curve.point(0.0), v3(r, 0., 0.), "seam passes +X");

    // The two pole edges are degenerate circles of radius 0 at the poles.
    let degen: Vec<&fc_part::topology::TopoEdge> = brep
        .edges
        .iter()
        .enumerate()
        .filter(|(i, _)| brep.is_degenerate(*i))
        .map(|(_, e)| e)
        .collect();
    assert_eq!(degen.len(), 2, "two degenerate pole edges");
    for d in &degen {
        match &d.curve {
            Curve::Circle { center, radius, .. } => {
                approx(*radius, 0.0, "pole circle radius");
                assert!(center.z.abs() - r.abs() < 1e-9, "pole circle at a pole");
            }
            other => panic!("pole edge is a {}", other.kind_name()),
        }
    }

    // The face's single wire: seam fwd, north pole, seam rev, south pole.
    assert_eq!(face.wires.len(), 1);
    assert_eq!(face.wires[0].edges.len(), 4);

    // Euler: V − E_nondegenerate + F = 2 − 1 + 1 = 2.
    assert_closed_shell(&brep, 2, "sphere");
}

// ---------------------------------------------------------------------------
// Torus
// ---------------------------------------------------------------------------

#[test]
fn torus_topology_has_two_periodic_seams() {
    let (r1, r2) = (8.0, 2.5);
    let brep = ExactBRep::from_shape(&TopoShape::Torus {
        radius1: r1,
        radius2: r2,
    })
    .expect("torus topology");

    // One vertex (where the seams cross), two closed seam edges, one face.
    assert_eq!(brep.vertices.len(), 1);
    vec_approx(
        brep.vertices[0].point,
        v3(r1 + r2, 0., 0.),
        "torus seam crossing",
    );
    assert_eq!(brep.edges.len(), 2);
    assert_eq!(brep.faces.len(), 1);
    for (i, _) in brep.edges.iter().enumerate() {
        assert!(!brep.is_degenerate(i), "torus edges are not degenerate");
    }

    let face = &brep.faces[0];
    match &face.surface {
        Surface::Torus {
            major_radius,
            minor_radius,
            ..
        } => {
            approx(*major_radius, r1, "torus major");
            approx(*minor_radius, r2, "torus minor");
            assert_eq!(face.uv_bounds, [0.0, TAU, 0.0, TAU]);
        }
        _ => unreachable!(),
    }

    // One seam is the minor circle at u=0 (center on the ring at +X),
    // the other is the major circle at v=0 (radius r1+r2 in the z=0
    // plane) — both full turns through the shared vertex.
    let minor: Vec<&fc_part::topology::TopoEdge> = brep
        .edges
        .iter()
        .filter(|e| match &e.curve {
            Curve::Circle { radius, .. } => (radius - r2).abs() < 1e-9,
            _ => false,
        })
        .collect();
    let major: Vec<&fc_part::topology::TopoEdge> = brep
        .edges
        .iter()
        .filter(|e| match &e.curve {
            Curve::Circle { radius, .. } => (radius - (r1 + r2)).abs() < 1e-9,
            _ => false,
        })
        .collect();
    assert_eq!(minor.len(), 1, "one minor seam");
    assert_eq!(major.len(), 1, "one major seam");
    if let Curve::Circle { center, .. } = &minor[0].curve {
        vec_approx(*center, v3(r1, 0., 0.), "minor seam center on the ring");
    }
    if let Curve::Circle { center, .. } = &major[0].curve {
        vec_approx(*center, v3(0., 0., 0.), "major seam centered");
    }

    // The face's wire uses each seam twice (fwd + rev) — a closed
    // shell where both edges pair with themselves.
    assert_eq!(face.wires.len(), 1);
    assert_eq!(face.wires[0].edges.len(), 4);

    // Euler: 1 − 2 + 1 = 0 (the torus).
    assert_closed_shell(&brep, 0, "torus");
}

// ---------------------------------------------------------------------------
// Evaluation — curves and surfaces
// ---------------------------------------------------------------------------

#[test]
fn surface_evaluation_matches_analytic_geometry() {
    let (r, h) = (5.0, 12.0);
    let brep = ExactBRep::from_shape(&TopoShape::Cylinder {
        radius: r,
        height: h,
    })
    .unwrap();
    let mantle = brep
        .faces
        .iter()
        .find(|f| matches!(&f.surface, Surface::Cylinder { .. }))
        .unwrap();
    let s = &mantle.surface;
    for (u, v) in [
        (0.0f64, 0.0f64),
        (PI / 3.0, 2.5),
        (PI, h),
        (3.0 * PI / 2.0, 7.0),
    ] {
        let p = s.point(u, v);
        vec_approx(p, v3(r * u.cos(), r * u.sin(), v), "cylinder point(u,v)");
        let n = s.normal(u, v);
        vec_approx(n, v3(u.cos(), u.sin(), 0.), "cylinder normal is radial");
    }

    let brep = ExactBRep::from_shape(&TopoShape::Sphere { radius: r }).unwrap();
    let s = &brep.faces[0].surface;
    let _ = s; // sphere evaluation is covered by its own test below
}

#[test]
fn sphere_evaluation_is_spherical_coordinates() {
    let r = 4.5;
    let brep = ExactBRep::from_shape(&TopoShape::Sphere { radius: r }).unwrap();
    let s = &brep.faces[0].surface;
    // p(u, v) = r * (cos v * (cos u * X + sin u * Y) + sin v * Z) — the
    // OCCT latitude convention: v = ±π/2 are the (degenerate) poles,
    // sampled away. ∂u×∂v = r²·cos v·p̂ — outward, consistent with the
    // cylinder.
    for (u, v) in [
        (0.4f64, 0.9f64),
        (PI / 2.0, PI / 4.0),
        (PI, -PI / 6.0),
        (5.0, 1.2),
    ] {
        let p = s.point(u, v);
        let want = v3(r * v.cos() * u.cos(), r * v.cos() * u.sin(), r * v.sin());
        vec_approx(p, want, "sphere point(u,v)");
        let n = s.normal(u, v);
        vec_approx(n, want.normalized().unwrap(), "sphere normal is radial");
    }
}

#[test]
fn curve_derivatives_have_the_right_magnitudes() {
    // Line: constant.
    let line = Curve::Line {
        origin: v3(1., 2., 3.),
        direction: v3(4., 0., 0.),
    };
    vec_approx(line.derivative(0.37), v3(4., 0., 0.), "line derivative");

    // Circle: |d/dt| = radius.
    let circle = Curve::Circle {
        center: v3(0., 0., 0.),
        axis: v3(0., 0., 1.),
        ref_dir: v3(1., 0., 0.),
        radius: 2.5,
    };
    for t in [0.0f64, 0.7, 2.1, 5.0] {
        approx(circle.derivative(t).length(), 2.5, "circle speed");
    }

    // Ellipse: |d/dt(0)| = minor radius, |d/dt(pi/2)| = major radius.
    let ellipse = Curve::Ellipse {
        center: v3(0., 0., 0.),
        axis: v3(0., 0., 1.),
        major_dir: v3(1., 0., 0.),
        major_radius: 2.0,
        minor_radius: 1.0,
    };
    approx(
        ellipse.derivative(0.0).length(),
        1.0,
        "ellipse speed at t=0",
    );
    approx(
        ellipse.derivative(PI / 2.0).length(),
        2.0,
        "ellipse speed at t=pi/2",
    );
    // The ellipse hits its axes at the quarter points.
    vec_approx(ellipse.point(0.0), v3(2., 0., 0.), "ellipse at t=0");
    vec_approx(ellipse.point(PI / 2.0), v3(0., 1., 0.), "ellipse at t=pi/2");
}

// ---------------------------------------------------------------------------
// B-splines — evaluation and round-trip
// ---------------------------------------------------------------------------

#[test]
fn bspline_curve_evaluation_is_de_boor() {
    // Degree 1, three control points, knots [0,0,1,2,2]: a polyline.
    let poly = Curve::Bspline {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        points: vec![v3(0., 0., 0.), v3(2., 0., 0.), v3(2., 3., 0.)],
        weights: None,
    };
    vec_approx(
        poly.point(0.5),
        v3(1., 0., 0.),
        "poly midpoint of first leg",
    );
    vec_approx(
        poly.point(1.5),
        v3(2., 1.5, 0.),
        "poly midpoint of second leg",
    );
    // Derivative along each leg is constant.
    vec_approx(
        poly.derivative(0.5),
        v3(2., 0., 0.),
        "poly first-leg direction",
    );
    vec_approx(
        poly.derivative(1.5),
        v3(0., 3., 0.),
        "poly second-leg direction",
    );

    // Rational quadratic Bezier (degree 2, knots [0,0,0,1,1,1]),
    // weights 1,1,2: the t=1/2 point is (1.2, 0.4, 0).
    let rat = Curve::Bspline {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points: vec![v3(0., 0., 0.), v3(1., 1., 0.), v3(2., 0., 0.)],
        weights: Some(vec![1.0, 1.0, 2.0]),
    };
    let p = rat.point(0.5);
    vec_approx(p, v3(1.2, 0.4, 0.), "rational Bezier midpoint");
}

#[test]
fn bspline_surface_bilinear_patch() {
    // Degree 1 x 1 over a 2x2 net: a bilinear patch whose midpoint is
    // the average of the four corners.
    let s = Surface::Bspline {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![
            vec![v3(0., 0., 0.), v3(0., 4., 0.)],
            vec![v3(6., 0., 0.), v3(6., 4., 8.)],
        ],
        weights: None,
    };
    let mid = s.point(0.5, 0.5);
    vec_approx(mid, v3(3., 2., 2.), "bilinear patch midpoint");
    // d/du at the midpoint is the v-average of the u-differences.
    let (du, dv) = s.derivatives(0.5, 0.5);
    vec_approx(du, v3(6., 0., 4.), "bilinear du");
    vec_approx(dv, v3(0., 4., 4.), "bilinear dv");
    let n = s.normal(0.5, 0.5);
    let want = v3(6., 0., 4.).cross(&v3(0., 4., 4.)).normalized().unwrap();
    vec_approx(n, want, "bilinear normal");
}

// ---------------------------------------------------------------------------
// Round-trip: FCBREP v2 structural payload
// ---------------------------------------------------------------------------

#[test]
fn brep_round_trip_all_primitives_is_byte_identical() {
    let shapes = [
        TopoShape::Box {
            length: 30.0,
            width: 20.0,
            height: 10.0,
        },
        TopoShape::Sphere { radius: 4.5 },
        TopoShape::Cylinder {
            radius: 5.0,
            height: 12.0,
        },
        TopoShape::Cone {
            radius1: 6.0,
            radius2: 3.0,
            height: 9.0,
        },
        TopoShape::Cone {
            radius1: 6.0,
            radius2: 0.0,
            height: 9.0,
        },
        TopoShape::Torus {
            radius1: 8.0,
            radius2: 2.5,
        },
    ];
    for shape in &shapes {
        let brep = ExactBRep::from_shape(shape).expect("topology");
        let mut buf = Vec::new();
        write_brep(&mut buf, &brep).expect("write v2");
        let back = read_brep_bytes(&buf).expect("read v2");
        assert_eq!(back, brep, "{}: structural round-trip", shape.kind_name());
        let mut buf2 = Vec::new();
        write_brep(&mut buf2, &back).expect("write again");
        assert_eq!(buf, buf2, "{}: byte-identical rewrite", shape.kind_name());
        // The reloaded topology still validates.
        back.validate().expect("reloaded validate");
    }
}

#[test]
fn bspline_payloads_round_trip() {
    let curve = Curve::Bspline {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        points: vec![
            v3(0., 0., 0.),
            v3(1., 2., 0.),
            v3(3., 2., 1.),
            v3(4., 0., 1.),
            v3(5., 0., 3.),
        ],
        weights: Some(vec![1.0, 1.0, 2.0, 1.0, 1.0]),
    };
    let surf = Surface::Bspline {
        u_degree: 2,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![
            vec![v3(0., 0., 0.), v3(0., 1., 0.)],
            vec![v3(1., 0., 2.), v3(1., 1., 2.)],
            vec![v3(2., 0., 0.), v3(2., 1., 0.)],
        ],
        weights: None,
    };
    let brep = ExactBRep {
        vertices: vec![
            fc_part::topology::TopoVertex {
                point: v3(0., 0., 0.),
            },
            fc_part::topology::TopoVertex {
                point: v3(5., 0., 3.),
            },
        ],
        edges: vec![fc_part::topology::TopoEdge {
            curve,
            // A clamped spline hits its first and last control points
            // exactly, so the vertices are the endpoints.
            range: (0.0, 1.0),
            start: 0,
            end: 1,
        }],
        faces: vec![fc_part::topology::TopoFace {
            surface: surf,
            uv_bounds: [0.0, 1.0, 0.0, 1.0],
            // A face with no wires is legal (an untrimmed surface).
            wires: vec![],
        }],
    };
    brep.validate().expect("hand-built validates");
    let mut buf = Vec::new();
    write_brep(&mut buf, &brep).expect("write");
    let back = read_brep_bytes(&buf).expect("read");
    assert_eq!(back, brep, "bspline round-trip");
    let mut buf2 = Vec::new();
    write_brep(&mut buf2, &back).expect("write again");
    assert_eq!(buf, buf2, "bspline byte-identical rewrite");
}

// ---------------------------------------------------------------------------
// Honest refusals
// ---------------------------------------------------------------------------

#[test]
fn mesh_shapes_have_no_exact_topology() {
    let mesh = TopoShape::Mesh(fc_part::Mesh::new(
        vec![v3(0., 0., 0.), v3(1., 0., 0.), v3(0., 1., 0.)],
        vec![[0, 1, 2]],
    ));
    let err = ExactBRep::from_shape(&mesh).expect_err("mesh has no exact topology");
    assert!(
        err.contains("mesh"),
        "error mentions what it refuses: {err}"
    );
}

#[test]
fn v1_and_v2_readers_reject_cross_versions() {
    // A v1 payload (primitive shape) is not a structural BRep.
    let mut v1 = Vec::new();
    write_shape(&mut v1, &TopoShape::Sphere { radius: 2.0 }).unwrap();
    let err = read_brep_bytes(&v1).expect_err("v1 is not a v2 payload");
    assert!(
        err.to_string().contains("v1"),
        "error names the version: {err}"
    );

    // A v2 payload is not a TopoShape.
    let brep = ExactBRep::from_shape(&TopoShape::Box {
        length: 1.,
        width: 1.,
        height: 1.,
    })
    .unwrap();
    let mut v2 = Vec::new();
    write_brep(&mut v2, &brep).unwrap();
    let err = read_shape_bytes(&v2).expect_err("v2 is not a v1 payload");
    assert!(
        err.to_string().contains("v2"),
        "error names the version: {err}"
    );
}

#[test]
fn malformed_v2_payloads_report_line_numbers() {
    let good = ExactBRep::from_shape(&TopoShape::Cylinder {
        radius: 5.0,
        height: 12.0,
    })
    .unwrap();
    let mut buf = Vec::new();
    write_brep(&mut buf, &good).unwrap();
    let text = String::from_utf8(buf).unwrap();

    // Empty payload: the format probe itself refuses it.
    let err = read_brep_bytes(b"").expect_err("empty");
    assert!(
        matches!(err, fc_part::BrpError::UnknownFormat),
        "empty: {err}"
    );

    // Unsupported version.
    let err = read_brep_bytes(b"FCBREP 3\n").expect_err("version");
    assert!(err.to_string().contains("version"), "{err}");

    // Non-finite float (the first vertex line is payload line 3).
    let mut bad = text.clone();
    let mut lines: Vec<&str> = bad.lines().collect();
    let mut replaced = false;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("v ") {
            lines[i] = "v 0 0 nan";
            replaced = true;
            break;
        }
    }
    assert!(replaced, "found a vertex line");
    bad = lines.join("\n");
    bad.push('\n');
    let err = read_brep_bytes(bad.as_bytes()).expect_err("nan");
    assert!(err.to_string().contains("finite"), "{err}");
    assert!(
        matches!(err, fc_part::BrpError::Malformed { line: 3, .. }),
        "line: {err}"
    );

    // Vertex index out of range in an edge.
    let mut bad = text.clone();
    let mut line_no = 0;
    for (i, line) in bad.lines().enumerate() {
        if line.starts_with("e line") || line.starts_with("e circle") {
            // Corrupt the trailing vertex indices.
            let mut parts: Vec<&str> = line.split_whitespace().collect();
            let n = parts.len();
            parts[n - 2] = "9";
            parts[n - 1] = "9";
            let rebuilt = parts.join(" ");
            bad = bad.replacen(line, &rebuilt, 1);
            line_no = i + 1;
            break;
        }
    }
    assert!(line_no > 0, "found an edge line");
    let err = read_brep_bytes(bad.as_bytes()).expect_err("bad vertex index");
    assert!(err.to_string().contains("vertex"), "{err}");

    // Wire referencing a missing edge (corrupt the first edge index).
    let mut bad = text.clone();
    for line in text.lines() {
        if line.starts_with("w ") {
            let mut parts: Vec<&str> = line.split_whitespace().collect();
            parts[2] = "99";
            let rebuilt = parts.join(" ");
            bad = bad.replacen(line, &rebuilt, 1);
            break;
        }
    }
    let err = read_brep_bytes(bad.as_bytes()).expect_err("bad wire edge");
    assert!(err.to_string().contains("edge"), "{err}");

    // Knots out of order in a bspline edge: build one directly.
    // e bspline <deg> <nknots> <npts> <rational> <knots> <coords> <t0> <t1> <s> <e>
    let bad_knots = "FCBREP 2\nvertices 1\nv 0 0 0\nedges 1\ne bspline 1 5 3 0 0 1 0.5 0 0 0 1 0 2 0 0 0 2 0 0.0 1.0 0 0\nfaces 0\nend\n";
    let err = read_brep_bytes(bad_knots.as_bytes()).expect_err("bad knots");
    assert!(err.to_string().contains("non-decreasing"), "{err}");

    // Edge parameter range must be strictly increasing.
    let bad_range =
        "FCBREP 2\nvertices 1\nv 0 0 0\nedges 1\ne line 0 0 0 1 0 0 1.0 1.0 0 0\nfaces 0\nend\n";
    let err = read_brep_bytes(bad_range.as_bytes()).expect_err("empty range");
    assert!(err.to_string().contains("t0 < t1"), "{err}");

    // Content after end.
    let mut trailing = text.clone();
    trailing.push_str("v 0 0 0\n");
    let err = read_brep_bytes(trailing.as_bytes()).expect_err("trailing");
    assert!(err.to_string().contains("end"), "{err}");
}

#[test]
fn validate_catches_structural_breaks() {
    // An edge whose vertex does not match its curve endpoint.
    let broken = ExactBRep {
        vertices: vec![
            fc_part::topology::TopoVertex {
                point: v3(0., 0., 0.),
            },
            fc_part::topology::TopoVertex {
                point: v3(9., 9., 9.),
            }, // wrong
        ],
        edges: vec![fc_part::topology::TopoEdge {
            curve: Curve::Line {
                origin: v3(0., 0., 0.),
                direction: v3(1., 0., 0.),
            },
            range: (0.0, 1.0),
            start: 0,
            end: 1,
        }],
        faces: vec![],
    };
    let err = broken.validate().expect_err("endpoint mismatch");
    assert!(
        err.to_string().contains("vertex"),
        "error names the vertex: {err}"
    );

    // A wire that is not vertex-continuous.
    let broken = ExactBRep {
        vertices: vec![
            fc_part::topology::TopoVertex {
                point: v3(0., 0., 0.),
            },
            fc_part::topology::TopoVertex {
                point: v3(1., 0., 0.),
            },
            fc_part::topology::TopoVertex {
                point: v3(1., 1., 0.),
            },
            fc_part::topology::TopoVertex {
                point: v3(0., 1., 0.),
            },
        ],
        edges: vec![
            fc_part::topology::TopoEdge {
                curve: Curve::Line {
                    origin: v3(0., 0., 0.),
                    direction: v3(1., 0., 0.),
                },
                range: (0.0, 1.0),
                start: 0,
                end: 1,
            },
            fc_part::topology::TopoEdge {
                // runs 2 -> 3, but the previous edge-use ended at 1
                curve: Curve::Line {
                    origin: v3(1., 1., 0.),
                    direction: v3(-1., 0., 0.),
                },
                range: (0.0, 1.0),
                start: 2,
                end: 3,
            },
        ],
        faces: vec![fc_part::topology::TopoFace {
            surface: Surface::Plane {
                origin: v3(0., 0., 0.),
                normal: v3(0., 0., 1.),
                x_dir: v3(1., 0., 0.),
            },
            uv_bounds: [0.0, 1.0, 0.0, 1.0],
            wires: vec![fc_part::topology::TopoWire {
                edges: vec![
                    fc_part::topology::WireEdge {
                        edge: 0,
                        forward: true,
                    },
                    fc_part::topology::WireEdge {
                        edge: 1,
                        forward: false,
                    },
                ],
            }],
        }],
    };
    let err = broken.validate().expect_err("discontinuous wire");
    assert!(
        err.to_string().contains("wire"),
        "error names the wire: {err}"
    );
}
