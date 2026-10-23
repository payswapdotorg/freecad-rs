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
    let _original_summary = summary_pairs(&doc);
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

    // Object order, names and types survive. Parametric features load
    // Touched (the writer persists no Shape payloads for them — real
    // FreeCAD restores only OCCT BRep — so the first recompute rebuilds
    // their geometry, ours or FreeCAD's own engine's).
    let expected_pairs: Vec<(String, String, ObjectStatus)> = vec![
        ("Base".into(), "Part::Box".into(), ObjectStatus::Touched),
        (
            "Lift".into(),
            "Part::Transform".into(),
            ObjectStatus::Touched,
        ),
    ];
    assert_eq!(summary_pairs(&loaded), expected_pairs);
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
        &ObjectStatus::Touched
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

    // Geometry is rebuilt by recompute, not restored from the file: the
    // writer persists parameters only for parametric features (a real
    // FreeCAD restores solely OCCT BRep and would reject our bytes), so
    // the loaded pair is Touched and one recompute rebuilds exactly.
    assert_eq!(shape_of(loaded.get_by_name("Base").unwrap()), None);
    let mut mutable = loaded;
    let executed = mutable.recompute_all().expect("reload recompute");
    assert_eq!(executed.len(), 2, "both Touched features recompute");
    assert_eq!(
        shape_of(mutable.get_by_name("Base").unwrap()),
        Some(TopoShape::Box {
            length: 3.5,
            width: 2.25,
            height: 7.125
        })
    );
    assert_eq!(
        shape_of(mutable.get_by_name("Lift").unwrap()),
        Some(original_lift_shape.clone()),
        "transform mesh rebuilt bit-for-bit (deterministic kernel)"
    );
    assert!(
        matches!(original_lift_shape, TopoShape::Mesh(_)),
        "the lift shape is a mesh"
    );

    // A recomputed document is recomputed idempotently: nothing to do.
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
    // Unknown types are no longer dropped: the object loads as an opaque
    // ForeignObject (raw data preserved verbatim) and is counted there.
    assert_eq!(report.skipped, 0, "nothing is dropped outright anymore");
    assert_eq!(
        report.foreign, 1,
        "the unknown type is preserved as foreign"
    );
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
            .any(|w| w.contains("OCCT BRep shape not modeled")),
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

    // The foreign object is present under its real name and type (raw
    // data preserved); the surviving ones are geometry-less but Touched,
    // so a recompute regenerates them from their (default) parameters.
    assert_eq!(doc.object_count(), 3);
    assert!(doc.get_by_name("Mystery").is_some());
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
// objects, skipped, foreign, skipped_properties, quantity_properties) is
// part of this file's compiled contract.
#[test]
fn load_report_has_the_documented_fields() {
    let report = LoadReport::default();
    assert_eq!(report.objects, 0);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.foreign, 0);
    assert!(report.warnings.is_empty());
    assert!(report.skipped_properties.is_empty());
    assert!(report.quantity_properties.is_empty());
    let report = LoadReport {
        warnings: vec!["w".to_string()],
        objects: 3,
        skipped: 1,
        foreign: 2,
        skipped_properties: vec!["Obj.Prop".to_string()],
        quantity_properties: vec!["Obj.Angle (unit deg)".to_string()],
    };
    assert_eq!(report.objects, 3);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.foreign, 2);
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

// ---------------------------------------------------------------------------
// FreeCAD 1.0 native sketch properties: `Geometry`
// (`Part::PropertyGeometryList`) + `Constraints`
// (`Sketcher::PropertyConstraintList`) — the exact forms real FreeCAD
// writes, verified against the FreeCAD 1.0 source
// (`PropertyGeometryList::Save`, `Constraint::Save`). The ordinals,
// element names (`<Constrain/>` — FreeCAD's own spelling), and the
// GeoId/PointPos conventions come from there, not from our dialect.
// ---------------------------------------------------------------------------

/// One FreeCAD `<Constrain …/>` element with every attribute FreeCAD's
/// own writer emits (defaults for the presentation flags).
fn constrain(
    ty: i64,
    first: i64,
    first_pos: i64,
    second: i64,
    second_pos: i64,
    value: f64,
) -> String {
    constrain3(ty, first, first_pos, second, second_pos, -2000, 0, value)
}

/// The same, with an explicit third element reference (Symmetric).
#[allow(clippy::too_many_arguments)]
fn constrain3(
    ty: i64,
    first: i64,
    first_pos: i64,
    second: i64,
    second_pos: i64,
    third: i64,
    third_pos: i64,
    value: f64,
) -> String {
    format!(
        "<Constrain Name=\"\" MetaData=\"\" Type=\"{ty}\" Orientation=\"0\" Value=\"{value}\" \
         LabelDistance=\"0\" LabelPosition=\"0\" IsDriving=\"1\" IsInVirtualSpace=\"0\" \
         IsVisible=\"1\" IsActive=\"1\" First=\"{first}\" FirstPos=\"{first_pos}\" \
         Second=\"{second}\" SecondPos=\"{second_pos}\" Third=\"{third}\" ThirdPos=\"{third_pos}\"/>"
    )
}

/// A FreeCAD-form `Document.xml` with one sketch: a 40×30 quad (geos
/// 0-3) + a circle (geo 4) + a point (geo 5), plus `constraints`
/// appended verbatim into the `<ConstraintList>` payload.
fn freecad_sketch_document(constraints: &str, count: usize) -> String {
    let mut geometry = String::new();
    for (a, b) in [
        ((0, 0), (40, 0)),
        ((40, 0), (40, 30)),
        ((40, 30), (0, 30)),
        ((0, 30), (0, 0)),
    ] {
        geometry.push_str(&format!(
            "<Geometry type=\"Part::GeomLineSegment\" migrated=\"1\">\
             <GeoExtensions count=\"0\"></GeoExtensions>\
             <LineSegment StartX=\"{}\" StartY=\"{}\" StartZ=\"0\" \
             EndX=\"{}\" EndY=\"{}\" EndZ=\"0\"/></Geometry>",
            a.0, a.1, b.0, b.1
        ));
    }
    geometry.push_str(
        "<Geometry type=\"Part::GeomCircle\" migrated=\"1\">\
         <GeoExtensions count=\"0\"></GeoExtensions>\
         <Circle CenterX=\"60\" CenterY=\"10\" CenterZ=\"0\" NormalX=\"0\" NormalY=\"0\" \
         NormalZ=\"1\" AngleXU=\"0\" Radius=\"9\"/></Geometry>",
    );
    geometry.push_str(
        "<Geometry type=\"Part::GeomPoint\" migrated=\"1\">\
         <GeoExtensions count=\"0\"></GeoExtensions>\
         <GeomPoint X=\"7\" Y=\"8\" Z=\"0\"/></Geometry>",
    );
    format!(
        "<Document SchemaVersion=\"4\" ProgramVersion=\"FreeCAD 1.0\" FileVersion=\"1\">\
         <Properties Count=\"1\">\
         <Property name=\"Label\" type=\"App::PropertyString\"><String value=\"doc\"/></Property>\
         </Properties>\
         <Objects Count=\"1\" Dependencies=\"1\">\
         <Object type=\"Sketcher::SketchObject\" name=\"Sketch\" id=\"1\"/>\
         </Objects>\
         <ObjectData Count=\"1\">\
         <Object name=\"Sketch\">\
         <Properties Count=\"4\">\
         <Property name=\"Label\" type=\"App::PropertyString\"><String value=\"Sketch\"/></Property>\
         <Property name=\"Placement\" type=\"App::PropertyPlacement\">\
         <PropertyPlacement Px=\"0\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>\
         </Property>\
         <Property name=\"Geometry\" type=\"Part::PropertyGeometryList\">\
         <GeometryList count=\"6\">{geometry}</GeometryList>\
         </Property>\
         <Property name=\"Constraints\" type=\"Sketcher::PropertyConstraintList\">\
         <ConstraintList count=\"{count}\">{constraints}</ConstraintList>\
         </Property>\
         </Properties>\
         </Object>\
         </ObjectData>\
         </Document>"
    )
}

#[test]
fn freecad_native_constraints_all_sixteen_kinds_load() {
    let path = temp_path("fc-constraints");
    cleanup(&path);
    let half_pi = std::f64::consts::FRAC_PI_2;
    // One `<Constrain>` per mappable kind (FreeCAD ordinals; point-slots
    // 1=start, 2=end, 3=mid-as-center):
    let constraints = [
        constrain(1, 0, 2, 1, 1, 0.0),         // Coincident (0-end ↔ 1-start)
        constrain(2, 0, 0, -2000, 0, 0.0),     // Horizontal on line 0
        constrain(3, 1, 0, -2000, 0, 0.0),     // Vertical on line 1
        constrain(4, 0, 0, 2, 0, 0.0),         // Parallel 0 ∥ 2
        constrain(10, 0, 0, 1, 0, 0.0),        // Perpendicular 0 ⊥ 1
        constrain(6, 0, 1, 0, 2, 40.0),        // Distance |line 0| = 40
        constrain(7, 2, 1, 3, 1, -40.0),       // DistanceX start(2)→start(3)
        constrain(8, 0, 1, 2, 1, 30.0),        // DistanceY start(0)→start(2)
        constrain(9, 0, 0, 1, 0, half_pi),     // Angle line0→line1 = 90°
        constrain(11, 4, 0, -2000, 0, 9.0),    // Radius of circle
        constrain(18, 4, 0, -2000, 0, 18.0),   // Diameter of circle
        constrain(12, 0, 0, 2, 0, 0.0),        // Equal length 0 == 2
        constrain(13, 2, 1, 0, 0, 0.0),        // PointOnObject start(2) on line 0
        constrain(5, 0, 0, 4, 0, 0.0),         // Tangent line 0 ↔ circle 4 (simple form)
        constrain3(14, 0, 1, 2, 1, 1, 0, 0.0), // Symmetric start(0) ↔ start(2) about line 1
        constrain3(14, 0, 2, 3, 2, 5, 3, 0.0), // Symmetric end(0) ↔ end(3) about point 5 (mid)
        constrain(17, 5, 0, -2000, 0, 0.0),    // Block the point geometry
    ]
    .join("");
    craft_fcstd(&path, &freecad_sketch_document(&constraints, 17), &[]);

    let (doc, report) = load_document(&path).unwrap();
    assert!(
        report.warnings.is_empty(),
        "every mappable kind loads without warnings: {:?}",
        report.warnings
    );
    let object = doc.get_by_name("Sketch").expect("sketch loads");
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap();
    let sketch = &sketch_object.sketch;
    // Geometry: 4 lines + circle + point, all XY.
    assert_eq!(sketch.geometry().len(), 6);
    assert!(matches!(
        sketch.geometry()[5],
        fc_sketcher::Geometry::Point { x: 7.0, y: 8.0 }
    ));
    assert!(matches!(
        sketch.geometry()[4],
        fc_sketcher::Geometry::Circle { radius, .. } if (radius - 9.0).abs() < 1e-12
    ));
    // All 16 kinds mapped, with the right values and targets.
    let cs = sketch.constraints();
    assert_eq!(cs.len(), 17);
    assert!(matches!(cs[0], fc_sketcher::Constraint::Coincident { .. }));
    assert!(matches!(cs[1], fc_sketcher::Constraint::Horizontal { g } if g.0 == 0));
    assert!(matches!(cs[2], fc_sketcher::Constraint::Vertical { g } if g.0 == 1));
    assert!(matches!(cs[3], fc_sketcher::Constraint::Parallel { .. }));
    assert!(matches!(
        cs[4],
        fc_sketcher::Constraint::Perpendicular { .. }
    ));
    assert!(matches!(cs[5], fc_sketcher::Constraint::Distance { value, .. } if value == 40.0));
    assert!(matches!(cs[6], fc_sketcher::Constraint::DistanceX { value, .. } if value == -40.0));
    assert!(matches!(cs[7], fc_sketcher::Constraint::DistanceY { value, .. } if value == 30.0));
    assert!(
        matches!(cs[8], fc_sketcher::Constraint::Angle { value, .. } if (value - half_pi).abs() < 1e-15)
    );
    assert!(matches!(cs[9], fc_sketcher::Constraint::Radius { value, .. } if value == 9.0));
    assert!(matches!(cs[10], fc_sketcher::Constraint::Diameter { value, .. } if value == 18.0));
    assert!(matches!(cs[11], fc_sketcher::Constraint::Equal { .. }));
    assert!(matches!(
        cs[12],
        fc_sketcher::Constraint::PointOnObject { .. }
    ));
    // The three FreeCAD-native kinds added for parity: Tangent (simple
    // form), Symmetric (about-line and about-point) and Block.
    assert!(matches!(
        cs[13],
        fc_sketcher::Constraint::Tangent { g1, g2 } if g1.0 == 0 && g2.0 == 4
    ));
    assert!(matches!(
        cs[14],
        fc_sketcher::Constraint::Symmetric { pos3: None, g3, .. } if g3.0 == 1
    ));
    assert!(matches!(
        cs[15],
        fc_sketcher::Constraint::Symmetric {
            pos3: Some(fc_sketcher::PointPos::Center),
            g3,
            ..
        } if g3.0 == 5
    ));
    assert!(matches!(cs[16], fc_sketcher::Constraint::Block { g } if g.0 == 5));
    cleanup(&path);
}

#[test]
fn freecad_native_constraints_skip_unmappable_with_honest_warnings() {
    let path = temp_path("fc-constraints-skip");
    cleanup(&path);
    let constraints = [
        constrain(5, 0, 1, 4, 0, 0.0),  // Tangent via a point slot — point form
        constrain(14, 0, 1, 1, 1, 0.0), // Symmetric without a Third reference
        constrain3(14, 0, 1, 2, 1, 4, 0, 0.0), // Symmetric about a circle as its "line"
        constrain3(14, 0, 1, 2, 1, 4, 1, 0.0), // Symmetric about a slot a circle does not carry
        constrain(15, 0, 0, -2000, 0, 0.0), // InternalAlignment — no counterpart
        constrain(16, 0, 0, -2000, 0, 0.0), // SnellsLaw — no counterpart
        constrain(1, -1, 1, 0, 2, 0.0), // Coincident on the root point
        constrain(2, 0, 1, 1, 1, 0.0),  // two-point Horizontal form
        constrain(11, 4, 0, -2000, 0, 9.0), // Radius — mappable, must load
    ]
    .join("");
    craft_fcstd(&path, &freecad_sketch_document(&constraints, 9), &[]);

    let (doc, report) = load_document(&path).unwrap();
    let warnings = report.warnings.join("\n");
    // Every unmappable case is named honestly.
    assert!(warnings.contains("Tangent"), "{warnings}");
    assert!(warnings.contains("Symmetric"), "{warnings}");
    assert!(warnings.contains("InternalAlignment"), "{warnings}");
    assert!(warnings.contains("SnellsLaw"), "{warnings}");
    assert!(warnings.contains("not a line"), "{warnings}");
    assert!(warnings.contains("does not carry"), "{warnings}");
    assert!(
        warnings.contains("axis, root point, external"),
        "{warnings}"
    );
    assert!(
        warnings.contains("form this kernel does not model"),
        "{warnings}"
    );
    // And the mappable one still loads.
    let object = doc.get_by_name("Sketch").expect("sketch loads");
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap();
    let cs = sketch_object.sketch.constraints();
    assert_eq!(cs.len(), 1);
    assert!(matches!(cs[0], fc_sketcher::Constraint::Radius { value, .. } if value == 9.0));
    cleanup(&path);
}

#[test]
fn freecad_native_constraints_reference_constraints_and_driven_flags_skip() {
    let path = temp_path("fc-constraints-driven");
    cleanup(&path);
    // IsDriving=0: a reference (measurement), not an equation.
    let constraints =
        "<Constrain Name=\"\" MetaData=\"\" Type=\"6\" Orientation=\"0\" Value=\"40\" \
         LabelDistance=\"0\" LabelPosition=\"0\" IsDriving=\"0\" IsInVirtualSpace=\"0\" \
         IsVisible=\"1\" IsActive=\"1\" First=\"0\" FirstPos=\"1\" \
         Second=\"0\" SecondPos=\"2\" Third=\"-2000\" ThirdPos=\"0\"/>";
    craft_fcstd(&path, &freecad_sketch_document(constraints, 1), &[]);

    let (doc, report) = load_document(&path).unwrap();
    let warnings = report.warnings.join("\n");
    assert!(warnings.contains("reference (IsDriving=0)"), "{warnings}");
    let object = doc.get_by_name("Sketch").expect("sketch loads");
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap();
    assert!(sketch_object.sketch.constraints().is_empty());
    cleanup(&path);
}

#[test]
fn freecad_native_constraints_remap_through_skipped_geometry() {
    let path = temp_path("fc-constraints-remap");
    cleanup(&path);
    // A construction line between two real geometries: FreeCAD keeps it
    // in the geometry list (GeoId 1) — a constraint on the circle
    // (FreeCAD GeoId 2) must land on OUR GeoId 1 after the skip.
    // Real line (FreeCAD geo 0) → construction line (geo 1, skipped) →
    // circle (geo 2): the constraint on FreeCAD's geo 2 must land on
    // OUR geo 1 after the skip.
    let geometry = "<Geometry type=\"Part::GeomLineSegment\" migrated=\"1\">\
        <GeoExtensions count=\"0\"></GeoExtensions>\
        <LineSegment StartX=\"0\" StartY=\"0\" StartZ=\"0\" EndX=\"40\" EndY=\"0\" EndZ=\"0\"/>\
        </Geometry>\
        <Geometry type=\"Part::GeomLineSegment\" migrated=\"1\">\
        <GeoExtensions count=\"0\"></GeoExtensions>\
        <LineSegment StartX=\"0\" StartY=\"0\" StartZ=\"0\" EndX=\"40\" EndY=\"0\" EndZ=\"0\"/>\
        <Construction value=\"1\"/>\
        </Geometry>\
        <Geometry type=\"Part::GeomCircle\" migrated=\"1\">\
        <GeoExtensions count=\"0\"></GeoExtensions>\
        <Circle CenterX=\"60\" CenterY=\"10\" CenterZ=\"0\" NormalX=\"0\" NormalY=\"0\" \
        NormalZ=\"1\" AngleXU=\"0\" Radius=\"9\"/></Geometry>";
    let document = format!(
        "<Document SchemaVersion=\"4\" ProgramVersion=\"FreeCAD 1.0\" FileVersion=\"1\">\
         <Objects Count=\"1\" Dependencies=\"1\">\
         <Object type=\"Sketcher::SketchObject\" name=\"Sketch\" id=\"1\"/>\
         </Objects>\
         <ObjectData Count=\"1\">\
         <Object name=\"Sketch\">\
         <Properties Count=\"4\">\
         <Property name=\"Label\" type=\"App::PropertyString\"><String value=\"Sketch\"/></Property>\
         <Property name=\"Placement\" type=\"App::PropertyPlacement\">\
         <PropertyPlacement Px=\"0\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>\
         </Property>\
         <Property name=\"Geometry\" type=\"Part::PropertyGeometryList\">\
         <GeometryList count=\"3\">{geometry}</GeometryList>\
         </Property>\
         <Property name=\"Constraints\" type=\"Sketcher::PropertyConstraintList\">\
         <ConstraintList count=\"1\">{}</ConstraintList>\
         </Property>\
         </Properties>\
         </Object>\
         </ObjectData>\
         </Document>",
        constrain(11, 2, 0, -2000, 0, 9.0) // Radius on FreeCAD's geo 2
    );
    craft_fcstd(&path, &document, &[]);

    let (doc, report) = load_document(&path).unwrap();
    let warnings = report.warnings.join("\n");
    assert!(warnings.contains("construction"), "{warnings}");
    let object = doc.get_by_name("Sketch").expect("sketch loads");
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap();
    let sketch = &sketch_object.sketch;
    assert_eq!(
        sketch.geometry().len(),
        2,
        "the construction line was skipped"
    );
    let cs = sketch.constraints();
    assert_eq!(cs.len(), 1);
    // FreeCAD GeoId 2 → our GeoId 1 (the circle), not 2 (out of range).
    assert!(
        matches!(cs[0], fc_sketcher::Constraint::Radius { g, value } if g.0 == 1 && value == 9.0),
        "the constraint followed the remap: {:?}",
        cs[0]
    );
    cleanup(&path);
}

#[test]
fn our_writer_emits_freecad_native_constraint_ordinals() {
    // The writer side of the interop: what `save_document` writes must
    // be the FreeCAD form — `<Constrain …/>` elements with FreeCAD's
    // `ConstraintType` ordinals — so real FreeCAD restores them.
    let path = temp_path("writer-constrain");
    cleanup(&path);
    let fragment = "<Sketch>\
        <Geometry type=\"1\" startx=\"0\" starty=\"0\" endx=\"40\" endy=\"0\"/>\
        <Geometry type=\"1\" startx=\"40\" starty=\"0\" endx=\"40\" endy=\"30\"/>\
        <Geometry type=\"2\" centerx=\"60\" centery=\"10\" radius=\"9\"/>\
        <Constraint type=\"0\" first=\"0\" firstpos=\"2\" second=\"1\" secondpos=\"1\"/>\
        <Constraint type=\"1\" first=\"0\"/>\
        <Constraint type=\"8\" first=\"2\" value=\"9.0\"/>\
        <Constraint type=\"11\" first=\"0\" second=\"1\" value=\"1.5707963267948966\"/>\
        </Sketch>";
    let sketch = fc_sketcher::xml::from_xml(fragment).unwrap();
    let mut doc = Document::new("t");
    doc.add(Box::new(fc_sketcher::SketchObject::with_sketch(
        "Sketch", sketch,
    )));
    save_document(&doc, &path).unwrap();

    let file = std::fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("Document.xml").unwrap(), &mut xml).unwrap();
    // Coincident (ours 0) → FreeCAD 1, with the point slots.
    assert!(
        xml.contains("<Constrain Name=\"\" MetaData=\"\" Type=\"1\" ")
            && xml.contains("First=\"0\" FirstPos=\"2\" Second=\"1\" SecondPos=\"1\""),
        "{xml}"
    );
    // Horizontal (ours 1) → FreeCAD 2, edge form (pos 0, unused -2000).
    assert!(
        xml.contains("Type=\"2\"") && xml.contains("First=\"0\" FirstPos=\"0\""),
        "{xml}"
    );
    // Radius (ours 8) → FreeCAD 11.
    assert!(
        xml.contains("Type=\"11\"") && xml.contains("Value=\"9.0\""),
        "{xml}"
    );
    // Angle (ours 11) → FreeCAD 9, radians.
    assert!(xml.contains("Type=\"9\""), "{xml}");
    // The full attribute set FreeCAD's Restore requires/prefers.
    assert!(
        xml.contains("IsDriving=\"1\"") && xml.contains("Third=\"-2000\" ThirdPos=\"0\""),
        "{xml}"
    );
    // And the whole thing round-trips back through our reader.
    let (doc, report) = load_document(&path).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let object = doc.get_by_name("Sketch").unwrap();
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap();
    assert_eq!(sketch_object.sketch.constraints().len(), 4);
    assert_eq!(sketch_object.sketch.geometry().len(), 3);
    cleanup(&path);
}
