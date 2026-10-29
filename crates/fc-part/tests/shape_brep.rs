//! Task 6 — `TopoShape::Brep`: the exact model becomes a first-class
//! shape variant.
//!
//! K4-1 reads real OCCT BRep payloads into `ExactBRep`; K3/K4-2 measure
//! and tessellate it exactly. This file pins the wiring the loader
//! needs before it can honestly attach a `.FCStd` shape payload: a
//! `TopoShape::Brep` carries the exact B-rep, answers the polymorphic
//! surface (`kind_name`/`validate`/`bbox`/`volume`/`surface_area`/
//! `tessellate`) with the exact answers, and — where the exact tiers
//! refuse by name (B-spline faces) — refuses through an honest
//! `Result` channel instead of silently returning a wrong number.
//!
//! The fixtures are the K4-1 corpus (`tests/fixtures/brp/`, authored by
//! real Open CASCADE; `expected.json` carries OCCT's own measures — see
//! `tests/occt_brp.rs` for the reader-side acceptance).

use std::collections::HashMap;
use std::fs;

use fc_part::brep::write_shape;
use fc_part::{read_occt_brp, ExactBRep, Surface, TopoShape};

fn fixture(name: &str) -> String {
    fs::read_to_string(format!(
        "{}/tests/fixtures/brp/{name}.brp",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("missing fixture {name}.brp: {e}"))
}

/// OCCT's own measure for a fixture (the corpus' `expected.json`).
fn expected_measure(name: &str) -> (f64, f64) {
    let text = fs::read_to_string(format!(
        "{}/tests/fixtures/brp/expected.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("missing expected.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("expected.json parses");
    let case = &value["cases"][name];
    (
        case["volume"].as_f64().expect("volume"),
        case["area"].as_f64().expect("area"),
    )
}

/// A closed 2-manifold: every directed edge carried exactly once — the
/// no-crack, no-T-junction, no-fold contract of a solid boundary. (A
/// local re-imation of `tests/occt_brp.rs`'s predicate; that file's
/// shared helpers are not modified from here.)
fn assert_closed_manifold(mesh: &fc_part::Mesh, ctx: &str) {
    assert!(!mesh.triangles.is_empty(), "{ctx}: no triangles");
    let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        assert!(
            t[0] != t[1] && t[1] != t[2] && t[0] != t[2],
            "{ctx}: triangle {i} collapses onto one point"
        );
        for (u, v) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *directed.entry((u, v)).or_default() += 1;
        }
    }
    for (&(u, v), &n) in &directed {
        assert_eq!(n, 1, "{ctx}: edge {u}→{v} traversed {n} times");
        assert_eq!(
            directed.get(&(v, u)).copied().unwrap_or(0),
            1,
            "{ctx}: edge {u}→{v} has no opposite traversal — a crack or a fold"
        );
    }
}

// ---------------------------------------------------------------------------
// the exact answers — cylinder.brp (r = 2, h = 10)
// ---------------------------------------------------------------------------

#[test]
fn cylinder_brep_shape_measures_and_tessellates_exactly() {
    let read =
        read_occt_brp(&fixture("cylinder")).unwrap_or_else(|e| panic!("cylinder reads: {e}"));
    let shape = TopoShape::Brep(read.brep);

    assert_eq!(shape.kind_name(), "Brep");
    // The reader validates on read (K4-1's honesty contract); the
    // polymorphic validate re-runs the same structural checks.
    assert!(shape.validate().is_ok(), "the read B-rep validates");

    // The exact measure, both through the Result channel and the
    // infallible one (a measuring B-rep agrees with itself).
    let (want_volume, want_area) = expected_measure("cylinder");
    let v = shape
        .try_volume()
        .unwrap_or_else(|e| panic!("volume refused: {e}"));
    let a = shape
        .try_surface_area()
        .unwrap_or_else(|e| panic!("area refused: {e}"));
    assert!(
        (v - want_volume).abs() / want_volume <= 1e-9,
        "volume {v:.12} vs OCCT {want_volume:.12}"
    );
    assert!(
        (a - want_area).abs() / want_area <= 1e-9,
        "area {a:.12} vs OCCT {want_area:.12}"
    );
    assert_eq!(shape.volume(), v, "infallible volume matches try_volume");
    assert_eq!(shape.surface_area(), a, "infallible area matches try_area");

    // The tessellation path: deflection-true and watertight.
    let mesh = shape
        .try_tessellate(0.05)
        .unwrap_or_else(|e| panic!("tessellation refused: {e}"));
    assert_closed_manifold(&mesh, "cylinder.brp at d=0.05");
    // The measured inscribed deficit at d=0.05, r=2 (the same 3.0e-2
    // bound tests/occt_brp.rs pins for this fixture).
    assert!(
        (mesh.volume() - want_volume).abs() / want_volume <= 3.0e-2,
        "mesh volume {} vs exact {want_volume}",
        mesh.volume()
    );
    // The infallible tessellate returns the same mesh.
    assert_eq!(shape.tessellate(0.05), mesh);

    // The bounding box: the deflection-grown tessellation box — an
    // OUTER bound (every mesh vertex lies on the surface, every surface
    // point lies within the deflection of the mesh), so it contains the
    // true box and exceeds it by at most the deflection slack.
    let bbox = shape.bbox();
    let true_min = fc_base::Vector3::new(-2.0, -2.0, 0.0);
    let true_max = fc_base::Vector3::new(2.0, 2.0, 10.0);
    let slack = fc_part::DEFAULT_DEFLECTION + 1e-9;
    for (got, want) in [
        (bbox.min.x, true_min.x),
        (bbox.min.y, true_min.y),
        (bbox.min.z, true_min.z),
        (bbox.max.x, true_max.x),
        (bbox.max.y, true_max.y),
        (bbox.max.z, true_max.z),
    ] {
        assert!(
            got <= want + slack && got >= want - slack,
            "bbox axis {got} vs the true box {want} (slack {slack})"
        );
    }
    assert!(bbox.contains_point(&true_min) && bbox.contains_point(&true_max));
}

// ---------------------------------------------------------------------------
// the honest refusals — bspline_face.brp
// ---------------------------------------------------------------------------

#[test]
fn bspline_brep_refuses_by_name_through_every_channel() {
    let read = read_occt_brp(&fixture("bspline_face"))
        .unwrap_or_else(|e| panic!("bspline_face reads: {e}"));
    let brep = read.brep;
    let shape = TopoShape::Brep(brep.clone());

    // The variant is a real shape — named, validated (the reader's own
    // contract) — it is the MEASURE and the TESSELLATION that refuse.
    assert_eq!(shape.kind_name(), "Brep");
    assert!(shape.validate().is_ok());

    // The Result channels name the refusing geometry.
    let err = shape.try_volume().expect_err("a Bspline face refuses");
    assert!(
        err.contains("Bspline"),
        "the volume refusal names the face: {err}"
    );
    let err = shape
        .try_surface_area()
        .expect_err("a Bspline face refuses");
    assert!(
        err.contains("Bspline"),
        "the area refusal names the face: {err}"
    );
    let err = shape
        .try_tessellate(0.05)
        .expect_err("a Bspline face refuses");
    assert!(
        err.contains("Bspline"),
        "the tessellation refusal names the face: {err}"
    );

    // The infallible channels never invent a number: NaN is the honest
    // "no number" (it propagates through sums — a poisoned total can
    // never read as a real one), and the empty mesh is visibly nothing.
    assert!(shape.volume().is_nan(), "volume is NaN, not a lie");
    assert!(shape.surface_area().is_nan(), "area is NaN, not a lie");
    let mesh = shape.tessellate(0.05);
    assert!(
        mesh.vertices.is_empty() && mesh.triangles.is_empty(),
        "the tessellation refusal yields the empty mesh"
    );

    // The fallback bounding box still bounds what is exactly known:
    // every vertex of the model and every control point of the
    // B-spline faces (the convex-hull property makes the pole net an
    // outer bound of the patch).
    let bbox = shape.bbox();
    for v in &brep.vertices {
        assert!(bbox.contains_point(&v.point), "vertex {v:?} is in the box");
    }
    for face in &brep.faces {
        if let Surface::Bspline { points, .. } = &face.surface {
            for row in points {
                for p in row {
                    assert!(bbox.contains_point(p), "pole {p:?} is in the box");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the topology identity and the serializer's honest refusal
// ---------------------------------------------------------------------------

#[test]
fn brep_shape_is_its_own_exact_topology() {
    let read = read_occt_brp(&fixture("box")).unwrap();
    let shape = TopoShape::Brep(read.brep.clone());
    // from_shape is the identity on the Brep variant — the exact model
    // IS the shape, no rebuild needed.
    assert_eq!(ExactBRep::from_shape(&shape), Ok(read.brep));
}

#[test]
fn write_shape_refuses_the_brep_variant_by_name() {
    // The FCBREP v1 writer covers the parametric primitives and meshes;
    // a Brep is the exact model's territory (FCBREP v2+, `write_brep`).
    // The .FCStd writer never routes a Brep here — it keeps the file's
    // own OCCT payload instead — but the API refuses loudly rather
    // than emitting a payload its own reader would reject.
    let read = read_occt_brp(&fixture("box")).unwrap();
    let mut out = Vec::new();
    let err = write_shape(&mut out, &TopoShape::Brep(read.brep))
        .expect_err("the v1 writer refuses the Brep variant");
    assert!(
        err.to_string().contains("Brep") && err.to_string().contains("write_brep"),
        "the refusal names the variant and the right writer: {err}"
    );
    assert!(out.is_empty(), "nothing partial was written");
}
