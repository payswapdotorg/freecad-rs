//! Integration tests for the PartDesign wave: `Sketch` + Pad/Pocket/
//! Revolution documents across the `.FCStd` boundary.
//!
//! Covers the acceptance points the fc-part unit tests cannot reach:
//!
//! * **Save/reload** — a built Sketch+Pad (plus Pocket and Revolution)
//!   document round-trips with sketch links, dependency edges and
//!   computed meshes intact; a recompute on the reloaded document
//!   reproduces the persisted volume exactly (FCBREP stores full f64
//!   vertices, so the equality is bit-level in practice — asserted to
//!   1e-9).
//! * **Parametric propagation** — replacing the sketch's content (the
//!   sanctioned in-load mutation window the CLI's `sketch-import` uses)
//!   with a wider rectangle touches the sketch **and its transitive
//!   dependents**; the recompute re-solves the sketch and re-executes
//!   the Pad, and the Pad's volume scales with the new width — the
//!   sketch → feature dependency edge proven end-to-end.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use fc_app::{Document, ObjectStatus, Property};
use fc_base::{Placement, Rotation, Vector3};
use fc_io::{load_document, load_document_with_hook, save_document};
use fc_part::features::BoxFeature;
use fc_part::mesh_of;
use fc_part::partdesign::{PadFeature, PocketFeature, RevolutionFeature};
use fc_sketcher::constraint::Constraint;
use fc_sketcher::geometry::{Geometry, Point};
use fc_sketcher::{PointPos, Sketch, SketchObject};

fn temp_path(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "fcrs-partdesign-{}-{}.fcstd",
        tag,
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ))
}

/// A constrained `w x h` rectangle with its lower-left corner at
/// `(x0, y0)` (the fc-part partdesign test fixture, mirrored here —
/// fc-part's dev-dependency closure has no fc-io, so the persistence
/// tests rebuild it).
fn rectangle_at(x0: f64, y0: f64, w: f64, h: f64) -> Sketch {
    let mut sk = Sketch::new();
    let top = sk.add_geometry(Geometry::LineSegment {
        start: Point::new(x0, y0),
        end: Point::new(x0 + w, y0),
    });
    let right = sk.add_geometry(Geometry::LineSegment {
        start: Point::new(x0 + w, y0),
        end: Point::new(x0 + w, y0 + h),
    });
    let bottom = sk.add_geometry(Geometry::LineSegment {
        start: Point::new(x0 + w, y0 + h),
        end: Point::new(x0, y0 + h),
    });
    let left = sk.add_geometry(Geometry::LineSegment {
        start: Point::new(x0, y0 + h),
        end: Point::new(x0, y0),
    });
    for (a, pa, b, pb) in [
        (top, PointPos::End, right, PointPos::Start),
        (right, PointPos::End, bottom, PointPos::Start),
        (bottom, PointPos::End, left, PointPos::Start),
        (left, PointPos::End, top, PointPos::Start),
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
        pos1: PointPos::Start,
        g2: top,
        pos2: PointPos::End,
        value: w,
    })
    .unwrap();
    sk.add_constraint(Constraint::DistanceY {
        g1: right,
        pos1: PointPos::Start,
        g2: right,
        pos2: PointPos::End,
        value: h,
    })
    .unwrap();
    sk
}

/// A half-disc of radius `r` (arc on +x, diameter on the y axis) — the
/// sphere test profile.
fn half_disc_sketch(r: f64) -> Sketch {
    let mut sk = Sketch::new();
    sk.add_geometry(Geometry::ArcOfCircle {
        center: Point::new(0.0, 0.0),
        radius: r,
        start_angle: -std::f64::consts::FRAC_PI_2,
        end_angle: std::f64::consts::FRAC_PI_2,
    });
    sk.add_geometry(Geometry::LineSegment {
        start: Point::new(0.0, r),
        end: Point::new(0.0, -r),
    });
    sk
}

/// The Pad/Pocket/Revolution document: a 20x10 rectangle sketch padded
/// to 10, a 4x4 centered pocket sketch cut 4 deep out of a 10x10x10
/// box, and a half-disc revolved into a sphere.
fn partdesign_document() -> Document {
    let mut doc = Document::new("partdesign");

    doc.add(Box::new(SketchObject::with_sketch(
        "RectSketch",
        rectangle_at(0.0, 0.0, 20.0, 10.0),
    )));
    doc.add(Box::new(PadFeature::new("Pad")));
    doc.set_property("Pad", "Profile", Property::Link("RectSketch".into()))
        .unwrap();
    doc.set_property("Pad", "Length", Property::Float(10.0))
        .unwrap();

    doc.add(Box::new(BoxFeature::new("Body")));
    // Real-assembly geometry: the 10×10×10 base box sits centered
    // under the pocket sketch — `Part::Box` occupies its corner-origin
    // `[0, 10]³` now (FreeCAD 1.0 convention, verified against the
    // AppImage), so the placement moves it to `[-5, 5] × [-5, 5]`.
    doc.set_property(
        "Body",
        "Placement",
        Property::Placement(Placement::new(
            Vector3::new(-5.0, -5.0, 0.0),
            Rotation::IDENTITY,
        )),
    )
    .unwrap();
    doc.add(Box::new(SketchObject::with_sketch(
        "PocketSketch",
        rectangle_at(-2.0, -2.0, 4.0, 4.0),
    )));
    doc.add(Box::new(PocketFeature::new("Pocket")));
    doc.set_property("Pocket", "Profile", Property::Link("PocketSketch".into()))
        .unwrap();
    doc.set_property("Pocket", "Base", Property::Link("Body".into()))
        .unwrap();
    doc.set_property("Pocket", "Length", Property::Float(4.0))
        .unwrap();

    doc.add(Box::new(SketchObject::with_sketch(
        "LatheSketch",
        half_disc_sketch(5.0),
    )));
    doc.add(Box::new(RevolutionFeature::new("Revolution")));
    doc.set_property(
        "Revolution",
        "Profile",
        Property::Link("LatheSketch".into()),
    )
    .unwrap();

    doc.recompute_all().expect("the fixture recomputes");
    doc
}

/// Volume of the named object's computed mesh (the feature results).
fn volume_of(doc: &Document, name: &str) -> f64 {
    mesh_of(doc.get_by_name(name).unwrap(), 0.2)
        .unwrap_or_else(|| panic!("{name} has no computed mesh"))
        .volume()
}

#[test]
fn partdesign_document_round_trips_with_links_and_geometry() {
    let path = temp_path("roundtrip");
    let doc = partdesign_document();
    let volumes_before = (
        volume_of(&doc, "Pad"),
        volume_of(&doc, "Pocket"),
        volume_of(&doc, "Revolution"),
    );
    // Sanity: the analytic expectations of the three features.
    assert!((volumes_before.0 - 2000.0).abs() < 1e-9);
    assert!((volumes_before.1 - 936.0).abs() < 1e-6);
    let sphere = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
    assert!((volumes_before.2 - sphere).abs() / sphere < 0.01);

    save_document(&doc, &path).expect("save");

    let (loaded, report) = load_document(&path).expect("reload");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.objects, 7);
    assert_eq!(report.skipped, 0);

    // Types, statuses and creation order survived.
    let rows: Vec<(String, String)> = loaded
        .summary()
        .into_iter()
        .map(|s| (s.name, s.type_name))
        .collect();
    let expected = [
        ("RectSketch", "Sketcher::SketchObject"),
        ("Pad", "PartDesign::Pad"),
        ("Body", "Part::Box"),
        ("PocketSketch", "Sketcher::SketchObject"),
        ("Pocket", "PartDesign::Pocket"),
        ("LatheSketch", "Sketcher::SketchObject"),
        ("Revolution", "PartDesign::Revolution"),
    ];
    let actual: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    assert_eq!(actual, expected);
    for (name, _) in &rows {
        let expected = if matches!(
            loaded.get_by_name(name).unwrap().type_name(),
            "PartDesign::Pad" | "PartDesign::Pocket" | "PartDesign::Revolution" | "Part::Box"
        ) {
            // Parametric features reload Touched: the writer persists
            // parameters only (real FreeCAD restores solely OCCT BRep),
            // so the first recompute rebuilds their geometry.
            ObjectStatus::Touched
        } else {
            ObjectStatus::Valid
        };
        assert_eq!(
            *loaded.get_by_name(name).unwrap().status(),
            expected,
            "{name} status after reload"
        );
    }

    // The sketch links round-trip (Profile and Base).
    let pad = loaded.get_by_name("Pad").unwrap();
    assert_eq!(
        pad.properties().get("Profile"),
        Some(&Property::Link("RectSketch".into()))
    );
    let pocket = loaded.get_by_name("Pocket").unwrap();
    assert_eq!(
        pocket.properties().get("Profile"),
        Some(&Property::Link("PocketSketch".into()))
    );
    assert_eq!(
        pocket.properties().get("Base"),
        Some(&Property::Link("Body".into()))
    );

    // ... and with them the dependency edges.
    assert_eq!(
        loaded.links_of("Pad"),
        vec!["RectSketch".to_string()],
        "the Pad's Profile link is its one dependency"
    );
    assert_eq!(loaded.links_of("Pocket").len(), 2);
    assert!(loaded
        .dependents_of("RectSketch")
        .contains(&"Pad".to_string()));
    assert!(loaded
        .dependents_of("PocketSketch")
        .contains(&"Pocket".to_string()));

    // Computed geometry rebuilds on the reload recompute (parameters,
    // links and placements are the persisted truth — like a real
    // FreeCAD, whose engine rebuilds what it cannot restore).
    let mut loaded = loaded;
    loaded.recompute_all().expect("reload recompute");
    let volumes_after = (
        volume_of(&loaded, "Pad"),
        volume_of(&loaded, "Pocket"),
        volume_of(&loaded, "Revolution"),
    );
    for (before, after, name) in [
        (volumes_before.0, volumes_after.0, "Pad"),
        (volumes_before.1, volumes_after.1, "Pocket"),
        (volumes_before.2, volumes_after.2, "Revolution"),
    ] {
        assert!(
            (before - after).abs() < 1e-9,
            "{name} volume changed across the round-trip: {before} vs {after}"
        );
    }

    // A recompute on the reloaded document (touch the Pad through the
    // property API; the persisted geometry is regenerated, not trusted):
    // identical volume. The reloaded sketch is Valid and stays skipped —
    // its persisted solved geometry IS the sketch's state.
    let mut reloaded = loaded;
    reloaded
        .set_property("Pad", "Length", Property::Float(10.0))
        .unwrap();
    let recomputed = reloaded.recompute_all().expect("recompute after reload");
    assert_eq!(recomputed.len(), 1, "only the Pad re-executes");
    assert!(
        (volume_of(&reloaded, "Pad") - volumes_before.0).abs() < 1e-9,
        "re-executed Pad must reproduce the persisted volume exactly"
    );
}

#[test]
fn replacing_sketch_content_propagates_to_the_pad() {
    let path = temp_path("propagate");
    let doc = partdesign_document();
    save_document(&doc, &path).expect("save");
    assert!(
        (volume_of(&doc, "Pad") - 2000.0).abs() < 1e-9,
        "20 x 10 x 10 before the edit"
    );

    // The sanctioned content-mutation window: swap the sketch during a
    // reload (exactly what the CLI's `sketch-import` does) with the same
    // rectangle at width 30 — only the DistanceX constraint changes.
    let dependents: std::collections::BTreeSet<String> =
        doc.dependents_of("RectSketch").into_iter().collect();
    assert!(
        dependents.contains("Pad"),
        "the pad must be a dependent of the sketch it profiles"
    );
    let replaced = std::cell::Cell::new(false);
    let wider = rectangle_at(0.0, 0.0, 30.0, 10.0);
    let mut hook = |name: &str, object: &mut dyn fc_app::DocumentObject| {
        if name == "RectSketch" {
            if let Some(sketch) = object.as_any_mut().downcast_mut::<SketchObject>() {
                sketch.replace_sketch(wider.clone());
                replaced.set(true);
            }
        } else if dependents.contains(name) {
            object.set_status(ObjectStatus::Touched);
        }
    };
    let (mut reloaded, report) =
        load_document_with_hook(&path, Some(&mut hook)).expect("reload with the swap");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(replaced.get(), "the hook found the sketch");

    // The recompute re-solves the sketch and re-executes the pad (the
    // volume scales with the new width, 30 x 10 x 10): the sketch ->
    // feature dependency edge works end-to-end. The other parametric
    // features reload Touched (no shape payloads — FreeCAD restores
    // only OCCT BRep), so they rebuild in the same pass.
    let recomputed = reloaded.recompute_all().expect("propagated recompute");
    let names: Vec<&str> = recomputed
        .iter()
        .filter_map(|id| reloaded.get(*id).map(|o| o.name()))
        .collect();
    assert_eq!(
        names,
        vec!["RectSketch", "Pad", "Body", "Pocket", "Revolution"],
        "the cascade ran exactly these"
    );
    assert!(
        (volume_of(&reloaded, "Pad") - 3000.0).abs() < 1e-6,
        "30 x 10 x 10 after the edit, got {}",
        volume_of(&reloaded, "Pad")
    );
    assert_eq!(
        *reloaded.get_by_name("RectSketch").unwrap().status(),
        ObjectStatus::Valid
    );
    assert_eq!(
        *reloaded.get_by_name("Pad").unwrap().status(),
        ObjectStatus::Valid
    );
}
