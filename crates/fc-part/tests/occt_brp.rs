//! K4 — reading real OCCT BRep payloads into the exact model.
//!
//! The corpus under `fixtures/brp/` is authored by real Open CASCADE
//! (`ocp` 0.1.9 bundles OCCT 7.8.x, the family FreeCAD 1.0 ships) via
//! `BRepTools.Write_s`; `expected.json` carries OCCT's own volumes
//! and areas. The acceptance discipline: every shape must **load**,
//! **validate** (the K1 bit-exactness contract — this is the whole
//! point of the reconciliation tier), carry **Euler characteristic
//! 2** (a closed solid), measure against OCCT's own numbers, and
//! **round-trip** through our FCBREP serialization.

use std::fs;

use fc_part::brep::{read_brep_bytes, write_brep};
use fc_part::measure::{brep_area, brep_volume};
use fc_part::{read_occt_brp, Surface};

fn fixture(name: &str) -> String {
    fs::read_to_string(format!(
        "{}/tests/fixtures/brp/{name}.brp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("missing fixture {name}.brp: {e}"))
}

struct Expected {
    volume: f64,
    area: f64,
}

fn expected(name: &str) -> Expected {
    let text = fs::read_to_string(format!(
        "{}/tests/fixtures/brp/expected.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("missing expected.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("expected.json parses");
    let case = &value["cases"][name];
    Expected {
        volume: case["volume"].as_f64().expect("volume"),
        area: case["area"].as_f64().expect("area"),
    }
}

/// Load, validate, Euler-check, measure against OCCT, round-trip.
fn acceptance(name: &str, vol_tol: f64, area_tol: f64) -> fc_part::OcctRead {
    acceptance_chi(name, vol_tol, area_tol, 2)
}

/// [`acceptance`] with an explicit Euler characteristic (the torus is
/// genus 1: `V − E + F = 0`).
fn acceptance_chi(name: &str, vol_tol: f64, area_tol: f64, chi_want: i64) -> fc_part::OcctRead {
    let data = fixture(name);
    let read = read_occt_brp(&data).unwrap_or_else(|e| panic!("{name}: read failed: {e}"));
    let brep = &read.brep;
    let chi = brep.euler_characteristic();
    assert_eq!(
        chi, chi_want,
        "{name}: Euler characteristic {chi}, expected {chi_want}"
    );
    let (v, a) = (
        brep_volume(brep).unwrap_or_else(|e| panic!("{name}: volume refused: {e}")),
        brep_area(brep).unwrap_or_else(|e| panic!("{name}: area refused: {e}")),
    );
    let exp = expected(name);
    let dv = (v - exp.volume).abs() / exp.volume.abs().max(1.0);
    let da = (a - exp.area).abs() / exp.area.abs().max(1.0);
    assert!(
        dv <= vol_tol,
        "{name}: volume {v:.12} vs OCCT {:.12} (rel {dv:.3e} > {vol_tol})",
        exp.volume
    );
    assert!(
        da <= area_tol,
        "{name}: area {a:.12} vs OCCT {:.12} (rel {da:.3e} > {area_tol})",
        exp.area
    );
    // FCBREP round-trip: the read model survives our own
    // serialization byte-faithfully (the K1 payload discipline).
    let mut out = Vec::new();
    write_brep(&mut out, brep).unwrap_or_else(|e| panic!("{name}: write failed: {e}"));
    let back = read_brep_bytes(&out).unwrap_or_else(|e| panic!("{name}: re-read failed: {e}"));
    assert_eq!(&back, brep, "{name}: FCBREP round-trip changed the model");
    read
}

// ---------------------------------------------------------------------------
// the analytic primitives — exact or machine-close
// ---------------------------------------------------------------------------

#[test]
fn box_loads_validates_and_measures_exactly() {
    let read = acceptance("box", 1e-12, 1e-12);
    let brep = &read.brep;
    // 8 corners, 12 edges, 6 plane faces.
    assert_eq!(brep.vertices.len(), 8);
    assert_eq!(brep.edges.len(), 12);
    assert_eq!(brep.faces.len(), 6);
    assert!(brep
        .faces
        .iter()
        .all(|f| matches!(f.surface, Surface::Plane { .. })));
    // A box is all lines and planes: the pinned seams reproduce the
    // exact geometry, so the measure is exact — and with no pins at
    // all if every line resolves its own corners (OCCT writes line
    // origins at the vertices).
    let exp = expected("box");
    assert_eq!(brep_volume(brep).unwrap(), exp.volume);
    assert_eq!(brep_area(brep).unwrap(), exp.area);
    assert!(read.report.max_pin_deviation <= 1e-12);
}

#[test]
fn cylinder_loads_and_measures() {
    let read = acceptance("cylinder", 1e-9, 1e-9);
    let brep = &read.brep;
    // The K1 cylinder: 2 seam vertices, 3 edges (two closed circles +
    // the seam), 3 faces.
    assert_eq!(brep.vertices.len(), 2);
    assert_eq!(brep.edges.len(), 3);
    assert_eq!(brep.faces.len(), 3);
    let kinds: Vec<&str> = brep.faces.iter().map(|f| f.surface.kind_name()).collect();
    assert!(kinds.contains(&"Cylinder"));
    assert_eq!(kinds.iter().filter(|k| **k == "Plane").count(), 2);
}

#[test]
fn cone_loads_and_measures() {
    let read = acceptance("cone", 1e-9, 1e-9);
    let brep = &read.brep;
    assert_eq!(brep.vertices.len(), 2);
    assert_eq!(brep.faces.len(), 3);
    assert!(brep
        .faces
        .iter()
        .any(|f| matches!(f.surface, Surface::Cone { .. })));
}

#[test]
fn sphere_loads_and_measures() {
    let read = acceptance("sphere", 1e-9, 1e-9);
    let brep = &read.brep;
    // The K1 sphere wire: 2 pole vertices, the seam circle plus two
    // degenerate pole circles, one spherical face.
    assert_eq!(brep.vertices.len(), 2);
    assert_eq!(brep.edges.len(), 3);
    assert_eq!(brep.faces.len(), 1);
    assert_eq!(read.report.degenerate_edges, 2);
    assert!(matches!(brep.faces[0].surface, Surface::Sphere { .. }));
}

#[test]
fn torus_loads_and_measures() {
    // A torus is genus 1: the Euler characteristic is 0, not 2.
    let read = acceptance_chi("torus", 1e-9, 1e-9, 0);
    let brep = &read.brep;
    // The K1 torus: 1 vertex (the seam crossing), 2 closed seam
    // circles, 1 face.
    assert_eq!(brep.vertices.len(), 1);
    assert_eq!(brep.edges.len(), 2);
    assert_eq!(brep.faces.len(), 1);
    assert!(matches!(brep.faces[0].surface, Surface::Torus { .. }));
}

// ---------------------------------------------------------------------------
// locations
// ---------------------------------------------------------------------------

#[test]
fn located_cylinder_composes_the_transform() {
    let read = acceptance("cylinder_located", 1e-9, 1e-9);
    // The world vertices must match OCCT's own (the located seam
    // points), to the 15-digit vertex-truncation scale.
    let text = fs::read_to_string(format!(
        "{}/tests/fixtures/brp/expected.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let world = value["cases"]["cylinder_located"]["world_vertices"]
        .as_array()
        .expect("world vertices");
    let brep = &read.brep;
    assert_eq!(brep.vertices.len(), world.len());
    for v in &brep.vertices {
        let p = (v.point.x, v.point.y, v.point.z);
        let ok = world.iter().any(|w| {
            let x = w[0].as_f64().unwrap();
            let y = w[1].as_f64().unwrap();
            let z = w[2].as_f64().unwrap();
            (x - p.0).abs() < 1e-9 && (y - p.1).abs() < 1e-9 && (z - p.2).abs() < 1e-9
        });
        assert!(
            ok,
            "vertex {p:?} has no OCCT world match in {world:?} — the location composition is \
             wrong"
        );
    }
}

// ---------------------------------------------------------------------------
// composed geometry — the differential cases
// ---------------------------------------------------------------------------

#[test]
fn oblique_cut_loads_with_its_ellipse() {
    let read = acceptance("cyl_oblique_cut", 1e-6, 1e-6);
    // The elliptical rim must survive as a real Ellipse curve.
    assert!(read
        .brep
        .edges
        .iter()
        .any(|e| matches!(e.curve, fc_part::Curve::Ellipse { .. })));
}

#[test]
fn fillet_box_loads_validates_and_round_trips() {
    // The straight-edge fillet: quarter-cylinder patches, quarter-arc
    // rims, and — where two fillets meet at a corner — an elliptical
    // corner curve. Everything loads, validates and round-trips; the
    // mixed arc/elliptical-arc strip trim is the next measure slice
    // (the refusal is honest and names the geometry).
    let data = fixture("fillet_box");
    let read = read_occt_brp(&data).unwrap_or_else(|e| panic!("read failed: {e}"));
    let brep = &read.brep;
    assert_eq!(brep.euler_characteristic(), 2);
    assert!(brep
        .faces
        .iter()
        .any(|f| matches!(f.surface, Surface::Cylinder { .. })));
    let mut out = Vec::new();
    write_brep(&mut out, brep).unwrap();
    let back = read_brep_bytes(&out).unwrap();
    assert_eq!(&back, brep);
    let err = brep_volume(brep).unwrap_err();
    assert!(
        err.contains("unrecognized rim") || err.contains("not a trim this reader knows"),
        "the honest refusal names the trim: {err}"
    );
}

#[test]
fn torus_fillet_loads_and_measures() {
    let read = acceptance("cyl_top_fillet", 1e-6, 1e-6);
    assert!(read
        .brep
        .faces
        .iter()
        .any(|f| matches!(f.surface, Surface::Torus { .. })));
}

// ---------------------------------------------------------------------------
// the NURBS operand — load, validate, round-trip; measure refuses honestly
// ---------------------------------------------------------------------------

#[test]
fn nurbs_face_loads_validates_and_round_trips() {
    let data = fixture("bspline_face");
    let read = read_occt_brp(&data).unwrap_or_else(|e| panic!("read failed: {e}"));
    let brep = &read.brep;
    assert_eq!(brep.faces.len(), 1);
    assert!(matches!(brep.faces[0].surface, Surface::Bspline { .. }));
    // A degree-(2,2) patch over [0,4]², clamped.
    if let Surface::Bspline {
        u_degree,
        v_degree,
        points,
        ..
    } = &brep.faces[0].surface
    {
        assert_eq!(*u_degree, 2);
        assert_eq!(*v_degree, 2);
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].len(), 3);
    }
    // The four B-spline boundary edges: full-domain ranges, the
    // corner poles pinned to the shared vertices.
    assert!(brep.edges.len() >= 4);
    assert!(brep
        .edges
        .iter()
        .all(|e| matches!(e.curve, fc_part::Curve::Bspline { .. })));
    // Measure refuses honestly: a lone patch is not a closed shell
    // (and even closed, the trimmed B-spline face measure is the
    // next K4 slice).
    let err = brep_volume(brep).unwrap_err();
    assert!(
        err.contains("not a closed shell"),
        "the lone-face refusal must say so: {err}"
    );
    // FCBREP round-trip.
    let mut out = Vec::new();
    write_brep(&mut out, brep).unwrap();
    let back = read_brep_bytes(&out).unwrap();
    assert_eq!(&back, brep);
}

// ---------------------------------------------------------------------------
// honest refusals
// ---------------------------------------------------------------------------

#[test]
fn unmodeled_geometry_refuses_by_name() {
    // A surface of revolution (kind 7).
    let revolution = fixture("box").replace("Surfaces 6", "Surfaces 7");
    // (Not a real revolution record; use a purpose-built payload.)
    let _ = revolution;

    let payload = "\
DBRep_DrawableShape

CASCADE Topology V3, (c) Open Cascade
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
7 0 0 0 0 0 1 1
1 0 0 0 0 1
Triangulations 0
TShapes 1
Fa
0  1e-07 1 0

0111000
*
+1 0
";
    let err = read_occt_brp(payload).unwrap_err();
    assert!(
        err.contains("revolution"),
        "the refusal must name the surface: {err}"
    );
}

#[test]
fn triangulated_payloads_refuse_by_name() {
    let mut payload = fixture("box");
    payload = payload.replace("Triangulations 0", "Triangulations 2");
    let err = read_occt_brp(&payload).unwrap_err();
    assert!(
        err.contains("Triangulations"),
        "the refusal must name the section: {err}"
    );
}

#[test]
fn garbage_refuses_with_a_position() {
    let err = read_occt_brp("hello world").unwrap_err();
    assert!(
        err.contains("not an OCCT BRep payload"),
        "the refusal must say what it is: {err}"
    );
}
