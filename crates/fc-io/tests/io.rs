//! Integration tests for `fc-io`: `.FCStd` save/load round trips,
//! the repo's own demo file, and loader behavior on imperfect archives.

use std::io::Write;
use std::path::{Path, PathBuf};

use fc_app::{Document, ObjectStatus, Property};
use fc_base::{Placement, Rotation, Vector3};
use fc_io::{load_document, save_document, FcstdError, LoadReport};
use fc_part::features::{BoxFeature, TransformFeature};
use fc_part::{shape_of, TopoShape};

const DEG: f64 = std::f64::consts::PI / 180.0;

/// A unique-enough temp path per test tag (the pid keeps parallel runs of
/// this binary from colliding).
fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("fcrs-woc-io-{}-{tag}.fcstd", std::process::id()))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn assert_placement_close(got: &Placement, want: &Placement, ctx: &str) {
    let dp = (
        (got.position.x - want.position.x).abs(),
        (got.position.y - want.position.y).abs(),
        (got.position.z - want.position.z).abs(),
    );
    assert!(
        dp.0 <= 1e-12 && dp.1 <= 1e-12 && dp.2 <= 1e-12,
        "{ctx}: position {got:?} != {want:?}"
    );
    for (a, b) in [
        (got.rotation.x, want.rotation.x),
        (got.rotation.y, want.rotation.y),
        (got.rotation.z, want.rotation.z),
        (got.rotation.w, want.rotation.w),
    ] {
        assert!(
            (a - b).abs() <= 1e-12,
            "{ctx}: quaternion component {a} != {b}"
        );
    }
}

/// Build the canonical round-trip document: a box with a placed
/// placement and extra properties of every type, and a transform
/// depending on it.
fn sample_document() -> Document {
    let mut doc = Document::new("RoundTrip");
    doc.set_creator("woc-test");

    let mut base = BoxFeature::new("Base");
    base.container.set("Length", Property::Float(3.5));
    base.container.set("Width", Property::Float(2.25));
    base.container.set("Height", Property::Float(7.125));
    base.container.set(
        "Placement",
        Property::Placement(Placement::new(
            Vector3::new(1.5, -2.25, 3.125),
            Rotation::around_z(30.0 * DEG),
        )),
    );
    // Every remaining property type rides along on the box.
    base.container.set("Count", Property::Integer(42));
    base.container.set("Visible", Property::Bool(true));
    base.container.set(
        "Note",
        Property::String("round \"trip\"\nline 2 <&;>".into()),
    );
    base.container
        .set("Dir", Property::Vector(Vector3::new(0.5, -1.0, 2.0)));
    doc.add(Box::new(base));

    let mut lift = TransformFeature::new("Lift");
    lift.container
        .set("Source", Property::Link("Base".to_string()));
    lift.container.set(
        "Placement",
        Property::Placement(Placement::new(
            Vector3::new(0.0, 0.0, 4.0),
            Rotation::around_x(90.0 * DEG),
        )),
    );
    lift.container.set("Scale", Property::Float(1.5));
    doc.add(Box::new(lift));

    doc
}

fn summary_pairs(doc: &Document) -> Vec<(String, String, ObjectStatus)> {
    doc.summary()
        .into_iter()
        .map(|s| (s.name, s.type_name, s.status))
        .collect()
}

#[test]
fn save_load_round_trip_preserves_everything() {
    let path = temp_path("roundtrip");
    cleanup(&path);

    let mut doc = sample_document();
    doc.recompute_all().expect("sample recomputes");
    // Snapshot the computed geometry before saving.
    let original_lift_shape =
        shape_of(doc.get_by_name("Lift").expect("lift")).expect("lift computed a shape");

    let original_created = doc.created().to_string();
    let original_summary = summary_pairs(&doc);
    save_document(&doc, &path).expect("save");
    let (loaded, report) = load_document(&path).expect("load");
    cleanup(&path);

    // LoadReport: everything loaded, nothing skipped, nothing to warn.
    assert_eq!(report.objects, 2);
    assert_eq!(report.skipped, 0);
    assert!(
        report.warnings.is_empty(),
        "unexpected warnings: {:?}",
        report.warnings
    );

    // Document metadata survives.
    assert_eq!(loaded.label(), "RoundTrip");
    assert_eq!(loaded.creator(), "woc-test");
    assert_eq!(loaded.created(), original_created);
    assert_eq!(loaded.object_count(), 2);

    // Object order, names, types and statuses survive.
    assert_eq!(summary_pairs(&loaded), original_summary);
    let rows = loaded.summary();
    let names: Vec<&str> = rows.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Base", "Lift"]);
    assert_eq!(loaded.get_by_name("Base").unwrap().type_name(), "Part::Box");
    assert_eq!(
        loaded.get_by_name("Lift").unwrap().type_name(),
        "Part::Transform"
    );
    assert_eq!(
        loaded.get_by_name("Base").unwrap().status(),
        &ObjectStatus::Valid
    );

    // Scalar and aggregate properties are preserved exactly.
    let props = loaded.get_by_name("Base").unwrap().properties();
    assert_eq!(props.get("Length"), Some(&Property::Float(3.5)));
    assert_eq!(props.get("Width"), Some(&Property::Float(2.25)));
    assert_eq!(props.get("Height"), Some(&Property::Float(7.125)));
    assert_eq!(props.get("Count"), Some(&Property::Integer(42)));
    assert_eq!(props.get("Visible"), Some(&Property::Bool(true)));
    assert_eq!(
        props.get("Note"),
        Some(&Property::String("round \"trip\"\nline 2 <&;>".into()))
    );
    assert_eq!(
        props.get("Dir"),
        Some(&Property::Vector(Vector3::new(0.5, -1.0, 2.0)))
    );
    let want_placement = Placement::new(
        Vector3::new(1.5, -2.25, 3.125),
        Rotation::around_z(30.0 * DEG),
    );
    match props.get("Placement") {
        Some(Property::Placement(p)) => {
            assert_placement_close(p, &want_placement, "Base.Placement")
        }
        other => panic!("Base.Placement did not survive: {other:?}"),
    }

    // Links are preserved.
    let lift_props = loaded.get_by_name("Lift").unwrap().properties();
    assert_eq!(
        lift_props.get("Source"),
        Some(&Property::Link("Base".into()))
    );
    assert_eq!(lift_props.get("Scale"), Some(&Property::Float(1.5)));
    assert_eq!(loaded.links_of("Lift"), vec!["Base".to_string()]);
    assert!(loaded.links_of("Base").is_empty());

    // Shapes survive: the analytic box exactly, and the transform's
    // computed mesh exactly (FCBREP round-trips f64 exactly).
    assert_eq!(
        shape_of(loaded.get_by_name("Base").unwrap()),
        Some(TopoShape::Box {
            length: 3.5,
            width: 2.25,
            height: 7.125
        })
    );
    assert_eq!(
        shape_of(loaded.get_by_name("Lift").unwrap()),
        Some(original_lift_shape.clone()),
        "transform mesh preserved bit-for-bit"
    );
    assert!(
        matches!(original_lift_shape, TopoShape::Mesh(_)),
        "the lift shape is a mesh"
    );

    // A loaded-and-Valid document is recomputed idempotently: nothing
    // to do.
    let mut mutable = loaded;
    let executed = mutable.recompute_all().expect("idempotent recompute");
    assert_eq!(executed, Vec::<fc_app::ObjectId>::new());
}

#[test]
fn demo_fcstd_loads_four_objects() {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/demo.fcstd"
    ));
    let (doc, report) = load_document(&path).expect("demo.fcstd loads");

    assert_eq!(report.objects, 4);
    assert_eq!(report.skipped, 0);
    assert!(
        report.warnings.is_empty(),
        "demo warnings: {:?}",
        report.warnings
    );

    assert_eq!(doc.object_count(), 4);
    assert_eq!(doc.label(), "Demo model");
    assert_eq!(doc.creator(), "freecad-rs");

    let rows = summary_pairs(&doc);
    assert_eq!(
        rows.iter()
            .map(|(name, _, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Base", "Ball", "Stem", "Lift"],
        "object order preserved"
    );
    assert_eq!(
        rows.iter()
            .map(|(_, ty, _)| ty.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Part::Box",
            "Part::Sphere",
            "Part::Cylinder",
            "Part::Transform"
        ]
    );

    // Geometry came back with the parameters.
    assert_eq!(
        shape_of(doc.get_by_name("Base").unwrap()),
        Some(TopoShape::Box {
            length: 30.0,
            width: 20.0,
            height: 5.0
        })
    );
    assert_eq!(
        shape_of(doc.get_by_name("Ball").unwrap()),
        Some(TopoShape::Sphere { radius: 6.0 })
    );
    assert_eq!(
        shape_of(doc.get_by_name("Stem").unwrap()),
        Some(TopoShape::Cylinder {
            radius: 1.5,
            height: 12.0
        })
    );
    let lift_mesh = shape_of(doc.get_by_name("Lift").unwrap()).expect("lift geometry present");
    match lift_mesh {
        TopoShape::Mesh(mesh) => {
            assert!(mesh.triangle_count() > 0, "lift mesh has triangles");
            assert!(mesh.volume() > 0.0, "lift mesh volume positive");
            assert_eq!(
                mesh.triangle_count(),
                9024,
                "known coarse-sphere tessellation"
            );
        }
        other => panic!("lift should carry a mesh, got {other:?}"),
    }

    // The dependency link (Lift -> Ball) and placements survived.
    assert_eq!(doc.links_of("Lift"), vec!["Ball".to_string()]);
    assert_eq!(doc.links_of("Ball"), Vec::<String>::new());
    match doc
        .get_by_name("Ball")
        .unwrap()
        .properties()
        .get("Placement")
    {
        Some(Property::Placement(p)) => {
            assert_placement_close(
                p,
                &Placement::new(Vector3::new(-6.0, 0.0, 11.0), Rotation::IDENTITY),
                "Ball.Placement",
            );
        }
        other => panic!("Ball.Placement missing: {other:?}"),
    }
}

/// Write a hand-crafted `.FCStd` archive (zip with Document.xml first).
fn craft_fcstd(path: &Path, xml: &str, side_files: &[(&str, &str)]) {
    let file = std::fs::File::create(path).expect("create crafted file");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("Document.xml", options)
        .expect("start Document.xml");
    zip.write_all(xml.as_bytes()).expect("write xml");
    for (name, body) in side_files {
        zip.start_file(*name, options).expect("start side file");
        zip.write_all(body.as_bytes()).expect("write side file");
    }
    zip.finish().expect("finish zip");
}

#[test]
fn load_report_is_sensible_for_imperfect_archives() {
    let path = temp_path("imperfect");
    cleanup(&path);

    // One object of unknown type, one whose shape payload is OCCT BRep,
    // and one whose shape side file is missing from the archive.
    let xml = "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"3\" Dependencies=\"1\">
  <Object type=\"App::Thingy\" name=\"Mystery\" id=\"1\"/>
  <Object type=\"Part::Box\" name=\"OcctBox\" id=\"2\"/>
  <Object type=\"Part::Box\" name=\"LostBox\" id=\"3\"/>
 </Objects>
 <ObjectData Count=\"3\">
  <Object name=\"OcctBox\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"Shape001.brp\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"LostBox\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"Missing.brp\"/>
    </Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
";
    craft_fcstd(
        &path,
        xml,
        &[("Shape001.brp", "DBRep_DrawableShape\nCASCADE Topology\n")],
    );

    let (mut doc, report) = load_document(&path).expect("imperfect archive still loads");
    cleanup(&path);

    assert_eq!(report.objects, 2, "the two Part::Box objects load");
    assert_eq!(report.skipped, 1, "the unknown type is skipped");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("unknown type 'App::Thingy'")),
        "warnings: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("OCCT BRep shape not supported")),
        "warnings: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("'Missing.brp' missing from archive")),
        "warnings: {:?}",
        report.warnings
    );

    // The skipped object is really absent; the surviving ones are
    // geometry-less but Touched, so a recompute regenerates them from
    // their (default) parameters.
    assert_eq!(doc.object_count(), 2);
    assert!(doc.get_by_name("Mystery").is_none());
    for name in ["OcctBox", "LostBox"] {
        let obj = doc.get_by_name(name).expect("box exists");
        assert!(
            shape_of(obj).is_none(),
            "{name}: geometry was skipped, parameters kept"
        );
        assert_eq!(obj.status(), &ObjectStatus::Touched, "{name} stays touched");
    }
    doc.recompute_all().expect("regenerate from parameters");
    for name in ["OcctBox", "LostBox"] {
        assert_eq!(
            shape_of(doc.get_by_name(name).unwrap()),
            Some(TopoShape::Box {
                length: 10.0,
                width: 10.0,
                height: 10.0
            }),
            "{name} regenerated with FreeCAD defaults"
        );
    }
}

/// `load_document` error, or panic (Document is not Debug, so plain
/// `expect_err` does not compile).
fn expect_load_err(path: &Path, what: &str) -> FcstdError {
    match load_document(path) {
        Ok(_) => panic!("{what}: expected an error, got a document"),
        Err(e) => e,
    }
}

#[test]
fn load_rejects_broken_files() {
    // A file that is not a zip at all.
    let garbage = temp_path("garbage");
    cleanup(&garbage);
    std::fs::write(&garbage, b"this is definitely not a zip archive").expect("write garbage");
    let err = expect_load_err(&garbage, "garbage must fail");
    assert!(
        matches!(err, FcstdError::Zip(_)),
        "expected a zip container error, got {err:?}"
    );
    cleanup(&garbage);

    // A zip whose first entry is not named Document.xml.
    let no_doc = temp_path("nodoc");
    cleanup(&no_doc);
    {
        let file = std::fs::File::create(&no_doc).expect("create crafted file");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("README.txt", options).expect("start entry");
        zip.write_all(b"hello").expect("write entry");
        zip.finish().expect("finish zip");
    }
    let err = expect_load_err(&no_doc, "missing Document.xml must fail");
    assert!(
        matches!(err, FcstdError::Xml(_)),
        "expected an xml error, got {err:?}"
    );
    assert!(
        err.to_string().contains("no Document.xml entry"),
        "error explains the problem: {err}"
    );
    cleanup(&no_doc);

    // A Document.xml whose content is not XML at all.
    let not_xml = temp_path("notxml");
    cleanup(&not_xml);
    craft_fcstd(&not_xml, "not xml at all", &[]);
    let err = expect_load_err(&not_xml, "garbage xml must fail");
    assert!(
        matches!(err, FcstdError::Xml(_)),
        "expected an xml error, got {err:?}"
    );
    assert!(
        err.to_string().contains("no <Document> root element"),
        "error explains the problem: {err}"
    );
    cleanup(&not_xml);

    // A missing file is an i/o error.
    let missing = temp_path("missing");
    cleanup(&missing);
    let err = expect_load_err(&missing, "missing file must fail");
    assert!(
        matches!(err, FcstdError::Io(_)),
        "expected io error, got {err:?}"
    );

    // Saving to an unwritable path is an i/o error too.
    let bad_dir = std::env::temp_dir().join("fcrs-woc-io-does-not-exist");
    let mut doc = Document::new("Unwritable");
    doc.recompute_all().expect("empty recompute");
    let err = save_document(&doc, &bad_dir.join("x.fcstd")).expect_err("bad save path");
    assert!(
        matches!(err, FcstdError::Io(_)),
        "expected io error, got {err:?}"
    );
}

#[test]
fn empty_document_round_trip() {
    let path = temp_path("empty");
    cleanup(&path);

    let mut doc = Document::new("Empty");
    doc.set_creator("nobody");
    doc.recompute_all().expect("nothing to do");
    save_document(&doc, &path).expect("save empty");
    let (loaded, report) = load_document(&path).expect("load empty");
    cleanup(&path);

    assert_eq!(report.objects, 0);
    assert_eq!(report.skipped, 0);
    assert!(report.warnings.is_empty());
    assert_eq!(loaded.object_count(), 0);
    assert_eq!(loaded.label(), "Empty");
    assert_eq!(loaded.creator(), "nobody");
    assert_eq!(loaded.created(), doc.created());
    let mut mutable = loaded;
    assert_eq!(
        mutable.recompute_all().expect("still nothing to do"),
        Vec::<fc_app::ObjectId>::new()
    );
}

// Reference the LoadReport type once more so its full shape (warnings,
// objects, skipped, skipped_properties, quantity_properties) is part of
// this file's compiled contract.
#[test]
fn load_report_has_the_documented_fields() {
    let report = LoadReport::default();
    assert_eq!(report.objects, 0);
    assert_eq!(report.skipped, 0);
    assert!(report.warnings.is_empty());
    assert!(report.skipped_properties.is_empty());
    assert!(report.quantity_properties.is_empty());
    let report = LoadReport {
        warnings: vec!["w".to_string()],
        objects: 3,
        skipped: 1,
        skipped_properties: vec!["Obj.Prop".to_string()],
        quantity_properties: vec!["Obj.Angle (unit deg)".to_string()],
    };
    assert_eq!(report.objects, 3);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.skipped_properties, vec!["Obj.Prop".to_string()]);
    assert_eq!(
        report.quantity_properties,
        vec!["Obj.Angle (unit deg)".to_string()]
    );
}

/// FC-6.1 follow-up regression: a solved-and-saved sketch reloads with
/// its persisted status (`Valid`), exactly like every other object kind
/// (FreeCAD stores solved geometry; the solve *info* is the only thing
/// not persisted — the loader restores content through the non-touching
/// `restore_sketch` path).
#[test]
fn sketch_reloads_with_persisted_status() {
    let path = temp_path("sketch-status");
    // A single line segment: no constraints, so the solve trivially
    // succeeds (0 residual) and the object goes Valid on recompute.
    let fragment = "<Sketch><Geometry type=\"1\" startx=\"0\" starty=\"0\" \
                    endx=\"40\" endy=\"0\"/></Sketch>";
    let sketch = fc_sketcher::xml::from_xml(fragment).unwrap();

    let mut doc = Document::new("t");
    doc.add(Box::new(fc_sketcher::SketchObject::with_sketch(
        "Sketch", sketch,
    )));
    doc.recompute_all().unwrap();
    save_document(&doc, &path).unwrap();

    let (doc, report) = load_document(&path).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let object = doc
        .get_by_name("Sketch")
        .expect("sketch survived the round-trip");
    assert_eq!(
        format!("{:?}", object.status()),
        format!("{:?}", ObjectStatus::Valid),
        "loaded sketch keeps its persisted status"
    );
    let sketch = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap();
    assert!(sketch.solved.is_none(), "solve info is not persisted");
    assert_eq!(sketch.sketch.geometry().len(), 1);
    cleanup(&path);
}
