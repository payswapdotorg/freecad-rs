//! ADR 0004 slice 1 acceptance — the exact boolean dispatch through the
//! real `Part::Cut` / `Part::Fuse` / `Part::Common` document features.
//!
//! The decisive chain the whole slice exists for: a real
//! FreeCAD-authored document carrying a supported OCCT solid opens,
//! the exact geometry is live on its feature, a boolean on it executes
//! through the exact kernel (the result carrying `TopoShape::Brep`,
//! measuring the kernel's own exact numbers), the document saves, and
//! the reopen + recompute re-derives the identical exact result — the
//! operand's OCCT bytes riding out verbatim the whole way.
//!
//! The refusals are as load-bearing as the successes (ADR 0004 rule
//! 3): a multi-surface B-rep operand, a B-spline B-rep operand, an
//! unpaired box × cylinder combination, a faceted operand beside an
//! exact one, and the sphere-Base cuts (cylinder or box Tool) each
//! refuse with the operands and classes named — never a silent
//! tessellation of the imported geometry.
//!
//! K4-2d/3 adds the box × sphere family: the imported OCCT box (the
//! 30×20×5 corpus payload) meets a placed sphere primitive at the
//! boundary-skeleton stations, and the exact bite taxonomy runs
//! through the real features — the same decisive chain, the
//! hand-derived closed forms (== real OCCT's own `BRepAlgoAPI`
//! numbers, `expected_box_sphere.json`) as the reference.
//!
//! The fixtures are the K4-1 corpus (`fc-part/tests/fixtures/brp/`,
//! authored by real Open CASCADE via `BRepTools::Write`): the sphere
//! (r=5 at the origin), the cylinder (r=2, h=10, +Z from the
//! origin), and the box (30×20×5 at the origin). Differential
//! references come from the exact kernel APIs themselves — called
//! here with the same placed parameters the dispatch derives, pinned
//! against closed forms by `fc-part/tests/brep_boolean.rs` and
//! `fc-part/tests/box_sphere_boolean.rs`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use fc_app::{Document, Property, RecomputeError};
use fc_base::Vector3;
use fc_io::{load_document, save_document};
use fc_part::brep_boolean::{
    box_sphere_boolean, cylinder_cylinder_boolean, sphere_sphere_boolean, BooleanOp,
    BooleanOutcomeKind,
};
use fc_part::features::{BooleanFeature, BooleanKind, CylinderFeature, MeshFeature, SphereFeature};
use fc_part::measure::brep_volume;
use fc_part::{mesh_of, shape_of, TopoShape};

fn temp_path(tag: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "fcrs-exactbool-{}-{}.fcstd",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

/// A K4-1 corpus fixture's bytes — real Open CASCADE output.
fn brp_fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fc-part/tests/fixtures/brp")
        .join(format!("{name}.brp"));
    std::fs::read(path).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// Craft a minimal `.FCStd`: the XML plus side entries, at `path`.
fn craft_fcstd(path: &Path, xml: &str, side_files: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).expect("create crafted file");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("Document.xml", options)
        .expect("start Document.xml");
    zip.write_all(xml.as_bytes()).expect("write xml");
    for (name, body) in side_files {
        zip.start_file(*name, options).expect("start side file");
        zip.write_all(body).expect("write side file");
    }
    zip.finish().expect("finish zip");
}

/// Every entry of a zip, name → bytes.
fn zip_entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let file = std::fs::File::open(path).expect("file exists");
    let mut zip = zip::ZipArchive::new(file).expect("is a zip");
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry readable");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).expect("entry bytes");
        out.insert(entry.name().to_string(), bytes);
    }
    out
}

/// The document XML: an imported `Part::Feature` carrying one OCCT
/// payload, a `Part::Sphere` primitive (`radius`, placement `px` along
/// X), and a boolean linking Base → Import, Tool → Ball. The minimal
/// carrier for the dispatch chains.
fn import_sphere_ball_xml(kind: &str, radius: f64, px: f64) -> String {
    format!(
        "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"3\" Dependencies=\"1\">
  <Object type=\"Part::Feature\" name=\"Import\" id=\"1\"/>
  <Object type=\"Part::Sphere\" name=\"Ball\" id=\"2\"/>
  <Object type=\"{kind}\" name=\"Op\" id=\"3\"/>
 </Objects>
 <ObjectData Count=\"3\">
  <Object name=\"Import\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"ImportShape.brp\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Ball\">
   <Properties Count=\"2\">
    <Property name=\"Radius\" type=\"App::PropertyFloat\">
     <Float value=\"{radius}\"/>
    </Property>
    <Property name=\"Placement\" type=\"App::PropertyPlacement\">
     <PropertyPlacement Px=\"{px}\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Op\">
   <Properties Count=\"2\">
    <Property name=\"Base\" type=\"App::PropertyLink\">
     <Link value=\"Import\"/>
    </Property>
    <Property name=\"Tool\" type=\"App::PropertyLink\">
     <Link value=\"Ball\"/>
    </Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
"
    )
}

/// A two-import document: the OCCT cylinder payload as Base, a
/// `Part::Cylinder` primitive (r=1, h=6, placed at (1.5, 0, 2)) as
/// Tool — parallel secant axes, the class `cylinder_cylinder_boolean`
/// answers.
fn import_cylinder_stem_xml(kind: &str) -> String {
    format!(
        "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"3\" Dependencies=\"1\">
  <Object type=\"Part::Feature\" name=\"Import\" id=\"1\"/>
  <Object type=\"Part::Cylinder\" name=\"Stem\" id=\"2\"/>
  <Object type=\"{kind}\" name=\"Op\" id=\"3\"/>
 </Objects>
 <ObjectData Count=\"3\">
  <Object name=\"Import\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"ImportShape.brp\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Stem\">
   <Properties Count=\"3\">
    <Property name=\"Radius\" type=\"App::PropertyFloat\">
     <Float value=\"1\"/>
    </Property>
    <Property name=\"Height\" type=\"App::PropertyFloat\">
     <Float value=\"10\"/>
    </Property>
    <Property name=\"Placement\" type=\"App::PropertyPlacement\">
     <PropertyPlacement Px=\"1.5\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Op\">
   <Properties Count=\"2\">
    <Property name=\"Base\" type=\"App::PropertyLink\">
     <Link value=\"Import\"/>
    </Property>
    <Property name=\"Tool\" type=\"App::PropertyLink\">
     <Link value=\"Stem\"/>
    </Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
"
    )
}

/// The kernel's own volume for an outcome — the differential reference.
fn outcome_volume(out: fc_part::brep_boolean::BooleanOutcome) -> f64 {
    match out.kind {
        BooleanOutcomeKind::Solid(brep) => brep_volume(&brep).expect("the kernel result measures"),
        BooleanOutcomeKind::Empty => 0.0,
        BooleanOutcomeKind::Refused { why } => panic!("the kernel refused: {why}"),
    }
}

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// THE decisive chain (the handoff milestone in miniature): an imported
/// real OCCT sphere, an exact `Part::Cut` by a placed primitive, the
/// exact result measuring the kernel's own numbers, and the full
/// save → reopen → recompute cycle re-deriving the identical exact
/// geometry with the operand's bytes untouched.
#[test]
fn cut_of_imported_sphere_by_primitive_runs_exact_through_the_round_trip() {
    let path = temp_path("sphere-cut");
    let sphere = brp_fixture("sphere");
    craft_fcstd(
        &path,
        &import_sphere_ball_xml("Part::Cut", 2.5, 4.0),
        &[("ImportShape.brp", &sphere)],
    );

    let (mut doc, report) = load_document(&path).expect("the crafted document loads");
    assert!(
        report.warnings.is_empty(),
        "a readable payload and a clean primitive warn about nothing: {:?}",
        report.warnings
    );
    doc.recompute_all().expect("recompute executes the cut");

    // The operand: the imported exact model, live.
    let import_shape = shape_of(doc.get_by_name("Import").unwrap()).expect("the import is live");
    assert_eq!(import_shape.kind_name(), "Brep");
    // The result: EXACT — TopoShape::Brep, the kernel's own numbers.
    let op = doc.get_by_name("Op").unwrap();
    let result = shape_of(op).expect("the cut computed");
    assert_eq!(
        result.kind_name(),
        "Brep",
        "the cut of an exact operand carries the exact model (ADR 0004 rule 3)"
    );
    let got = result.try_volume().expect("the crescent measures exactly");
    // The differential reference: the kernel API called with the same
    // placed parameters the dispatch derived (r=5 at the origin;
    // the primitive r=2.5 placed at (4, 0, 0)).
    let reference = outcome_volume(
        sphere_sphere_boolean(
            BooleanOp::Cut,
            v3(0.0, 0.0, 0.0),
            5.0,
            v3(4.0, 0.0, 0.0),
            2.5,
        )
        .expect("the reference configuration is supported"),
    );
    assert!(
        (got - reference).abs() <= 1e-9 * reference,
        "the feature's exact volume {got} is the kernel's own {reference}"
    );
    // The display channel refuses honestly: the crescent's outer face
    // is a sphere-minus-cap complement trim, whose domain
    // classification is the K4-2d slice — `mesh_of` returns no mesh
    // (never a plausible-looking wrong one), the named refusal riding
    // `try_tessellate`. The two-channel honesty contract, live.
    assert!(
        mesh_of(op, 0.05).is_none(),
        "no display mesh exists for the complement-trimmed crescent"
    );
    let tess_err = result
        .try_tessellate(0.05)
        .expect_err("the trim class refuses");
    assert!(
        tess_err.contains("trim-domain classification"),
        "the display refusal names the slice: {tess_err}"
    );

    // The save: the operand's OCCT bytes are its only truthful
    // serialization — they ride out verbatim; the derived exact result
    // is NOT serialized (ADR 0004 rule 4: the feature's links are the
    // serialization; a real FreeCAD recomputes from them).
    let temp = temp_path("sphere-cut-roundtrip");
    save_document(&doc, &temp).expect("the exact-result document saves");
    assert_eq!(
        zip_entries(&temp).get("ImportShape.brp"),
        Some(&sphere),
        "the operand's OCCT payload rides out byte-identically"
    );

    // The reopen: the operand restores exactly; the cut re-saved
    // touched (no payload — its geometry is a recompute away); the
    // recompute re-derives the IDENTICAL exact result.
    let (mut reloaded, report2) = load_document(&temp).expect("the saved file loads");
    assert!(
        report2.warnings.is_empty(),
        "the round trip is clean: {:?}",
        report2.warnings
    );
    let reloaded_import = shape_of(reloaded.get_by_name("Import").unwrap()).unwrap();
    assert_eq!(
        reloaded_import.kind_name(),
        "Brep",
        "the operand restores exactly"
    );
    reloaded
        .recompute_all()
        .expect("the reload recompute re-derives");
    let reloaded_result =
        shape_of(reloaded.get_by_name("Op").unwrap()).expect("the recompute restored the cut");
    assert_eq!(
        reloaded_result.kind_name(),
        "Brep",
        "the recomputed result is the exact model again"
    );
    let got2 = reloaded_result
        .try_volume()
        .expect("still measures exactly");
    assert_eq!(
        got2, got,
        "the re-derived exact volume is bit-identical to the first computation"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}

/// The cylinder family through a feature: the imported OCCT cylinder,
/// a parallel secant primitive, `Part::Fuse` through
/// `cylinder_cylinder_boolean` — the kernel's own volume.
#[test]
fn fuse_of_imported_cylinder_with_primitive_runs_exact() {
    let path = temp_path("cyl-fuse");
    let cylinder = brp_fixture("cylinder");
    craft_fcstd(
        &path,
        &import_cylinder_stem_xml("Part::Fuse"),
        &[("ImportShape.brp", &cylinder)],
    );

    let (mut doc, report) = load_document(&path).expect("the crafted document loads");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    doc.recompute_all().expect("recompute executes the fuse");

    let result = shape_of(doc.get_by_name("Op").unwrap()).expect("the fuse computed");
    assert_eq!(
        result.kind_name(),
        "Brep",
        "the fuse of an exact operand carries the exact model"
    );
    let got = result.try_volume().expect("the fuse measures exactly");
    // The differential reference: the payload cylinder (r=2, h=10, +Z
    // from the origin) and the primitive (r=1, h=10 at (1.5, 0, 0) —
    // aligned caps, parallel secant axes).
    let reference = outcome_volume(
        cylinder_cylinder_boolean(
            BooleanOp::Fuse,
            v3(0.0, 0.0, 0.0),
            v3(0.0, 0.0, 1.0),
            2.0,
            10.0,
            v3(1.5, 0.0, 0.0),
            v3(0.0, 0.0, 1.0),
            1.0,
            10.0,
        )
        .expect("the reference configuration is supported"),
    );
    assert!(
        (got - reference).abs() <= 1e-9 * reference,
        "the feature's exact volume {got} is the kernel's own {reference}"
    );

    std::fs::remove_file(&path).ok();
}

/// An exact empty answer: `Part::Common` of disjoint exact operands is
/// the empty set — carried as an empty mesh result (emptiness has no
/// geometry identity to lose), the recompute succeeding.
#[test]
fn common_of_disjoint_exact_operands_is_exact_emptiness() {
    let path = temp_path("disjoint-common");
    let sphere = brp_fixture("sphere");
    // The primitive placed far away: d = 20 against r = 5 + 2.5.
    craft_fcstd(
        &path,
        &import_sphere_ball_xml("Part::Common", 2.5, 20.0),
        &[("ImportShape.brp", &sphere)],
    );

    let (mut doc, report) = load_document(&path).expect("the crafted document loads");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    doc.recompute_all()
        .expect("an exact empty answer is a success");

    let result = shape_of(doc.get_by_name("Op").unwrap()).expect("the common computed");
    match result {
        TopoShape::Mesh(mesh) => {
            assert!(
                mesh.triangles.is_empty() && mesh.vertices.is_empty(),
                "the empty set is an empty mesh"
            );
            assert_eq!(mesh.volume(), 0.0, "empty is empty");
        }
        other => panic!("the exact empty answer is an empty mesh, got {other:?}"),
    }

    std::fs::remove_file(&path).ok();
}

/// The named refusals — as load-bearing as the successes. Each refuses
/// through the feature's execution error, naming the operands, their
/// classes, and the way to the faceted workflow explicitly.
#[test]
fn unsupported_exact_combinations_refuse_by_name() {
    // A multi-surface B-rep operand (the filleted box: planes AND
    // cylinders): no exact pairing — the class named.
    let path = temp_path("multisurface-refuses");
    let fillet_box = brp_fixture("fillet_box");
    craft_fcstd(
        &path,
        &import_sphere_ball_xml("Part::Cut", 2.5, 4.0),
        &[("ImportShape.brp", &fillet_box)],
    );
    let (mut doc, _) = load_document(&path).expect("loads");
    let err = doc
        .recompute_all()
        .expect_err("the multi-surface class refuses");
    match err {
        RecomputeError::ExecuteFailed { object, reason } => {
            assert_eq!(object, "Op");
            assert!(
                reason.contains("multi-surface"),
                "the refusal names the class: {reason}"
            );
            assert!(
                reason.contains("Import"),
                "the refusal names the operand: {reason}"
            );
            assert!(
                reason.contains("tessellate first or use mesh operands"),
                "the refusal says the faceted route explicitly: {reason}"
            );
        }
        other => panic!("expected ExecuteFailed, got {other:?}"),
    }
    // The last-good contract: no result was computed, none is claimed.
    assert!(
        shape_of(doc.get_by_name("Op").unwrap()).is_none(),
        "a refused boolean claims no geometry"
    );
    std::fs::remove_file(&path).ok();

    // An unpaired combination: the imported box-class B-rep (which
    // now pairs with a sphere — K4-2d/3) against a cylinder
    // primitive — outside the pairing table, by name.
    let path = temp_path("box-cylinder-refuses");
    let boxbrp = brp_fixture("box");
    craft_fcstd(
        &path,
        &import_cylinder_stem_xml("Part::Cut"),
        &[("ImportShape.brp", &boxbrp)],
    );
    let (mut doc, _) = load_document(&path).expect("loads");
    let err = doc.recompute_all().expect_err("box × cylinder refuses");
    match err {
        RecomputeError::ExecuteFailed { object, reason } => {
            assert_eq!(object, "Op");
            assert!(
                reason.contains("outside the pairing table"),
                "the refusal names the boundary: {reason}"
            );
            assert!(
                reason.contains("box×sphere"),
                "the refusal says what DOES pair: {reason}"
            );
        }
        other => panic!("expected ExecuteFailed, got {other:?}"),
    }
    std::fs::remove_file(&path).ok();

    // B-spline-faced geometry: the same honest refusal, the face named.
    let path = temp_path("bspline-refuses");
    let bspline = brp_fixture("bspline_face");
    craft_fcstd(
        &path,
        &import_sphere_ball_xml("Part::Cut", 2.5, 4.0),
        &[("ImportShape.brp", &bspline)],
    );
    let (mut doc, _) = load_document(&path).expect("loads");
    let err = doc.recompute_all().expect_err("the B-spline class refuses");
    match err {
        RecomputeError::ExecuteFailed { object, reason } => {
            assert_eq!(object, "Op");
            assert!(
                reason.contains("B-spline"),
                "the refusal names the geometry: {reason}"
            );
        }
        other => panic!("expected ExecuteFailed, got {other:?}"),
    }
    std::fs::remove_file(&path).ok();

    // Sphere as Base, cylinder as Tool, Cut: the operand order has no
    // kernel path — the refusal says the swap, never a silent one.
    let path = temp_path("swap-refuses");
    let sphere = brp_fixture("sphere");
    craft_fcstd(
        &path,
        &import_cylinder_stem_xml("Part::Cut"),
        &[("ImportShape.brp", &sphere)],
    );
    let (mut doc, _) = load_document(&path).expect("loads");
    let err = doc.recompute_all().expect_err("the swap case refuses");
    match err {
        RecomputeError::ExecuteFailed { object, reason } => {
            assert_eq!(object, "Op");
            assert!(
                reason.contains("swap Base and Tool"),
                "the refusal names the workaround: {reason}"
            );
        }
        other => panic!("expected ExecuteFailed, got {other:?}"),
    }
    std::fs::remove_file(&path).ok();
}

/// A faceted operand beside an exact one: the exact intent dominates
/// (ADR 0004 rule 3) — the combination refuses with the faceted operand
/// named, the imported geometry never silently tessellated.
#[test]
fn brep_times_faceted_operand_refuses_by_name() {
    let path = temp_path("mesh-tool");
    let sphere = brp_fixture("sphere");
    // The tool: a `Part::Feature` carrying this kernel's own FCBREP v1
    // mesh payload (a tessellated box) — faceted geometry, honestly.
    let mesh = TopoShape::Box {
        length: 4.0,
        width: 4.0,
        height: 4.0,
    }
    .tessellate(0.5);
    let mut mesh_payload = Vec::new();
    fc_part::brep::write_shape(&mut mesh_payload, &TopoShape::Mesh(mesh)).expect("v1 mesh payload");
    let xml = "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"3\" Dependencies=\"1\">
  <Object type=\"Part::Feature\" name=\"Import\" id=\"1\"/>
  <Object type=\"Part::Feature\" name=\"Meshy\" id=\"2\"/>
  <Object type=\"Part::Cut\" name=\"Op\" id=\"3\"/>
 </Objects>
 <ObjectData Count=\"3\">
  <Object name=\"Import\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"ImportShape.brp\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Meshy\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"MeshyShape.brp\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Op\">
   <Properties Count=\"2\">
    <Property name=\"Base\" type=\"App::PropertyLink\">
     <Link value=\"Import\"/>
    </Property>
    <Property name=\"Tool\" type=\"App::PropertyLink\">
     <Link value=\"Meshy\"/>
    </Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
";
    craft_fcstd(
        &path,
        xml,
        &[
            ("ImportShape.brp", &sphere[..]),
            ("MeshyShape.brp", &mesh_payload[..]),
        ],
    );

    let (mut doc, _) = load_document(&path).expect("loads");
    let err = doc.recompute_all().expect_err("the faceted tool refuses");
    match err {
        RecomputeError::ExecuteFailed { object, reason } => {
            assert_eq!(object, "Op");
            assert!(
                reason.contains("Meshy") && reason.contains("faceted"),
                "the refusal names the faceted operand: {reason}"
            );
            assert!(
                reason.contains("tessellate first or use mesh operands"),
                "the refusal says the faceted route explicitly: {reason}"
            );
        }
        other => panic!("expected ExecuteFailed, got {other:?}"),
    }

    std::fs::remove_file(&path).ok();
}

/// The no-Brep branch is untouched: primitive × primitive keeps today's
/// faceted path — the mesh boolean, the result carrying
/// `TopoShape::Mesh` (the representation records the approximation).
#[test]
fn primitive_pairs_keep_the_faceted_path() {
    let mut doc = Document::new("it");
    let mut ball = SphereFeature::new("Ball");
    ball.container.set("Radius", Property::Float(2.0));
    doc.add(Box::new(ball));
    let mut stem = CylinderFeature::new("Stem");
    stem.container.set("Radius", Property::Float(1.0));
    stem.container.set("Height", Property::Float(3.0));
    doc.add(Box::new(stem));
    let mut cut = BooleanFeature::new("Notch", BooleanKind::Cut);
    cut.container.set("Base", Property::Link("Ball".into()));
    cut.container.set("Tool", Property::Link("Stem".into()));
    doc.add(Box::new(cut));
    doc.recompute_all().expect("the faceted path executes");

    let result = shape_of(doc.get_by_name("Notch").unwrap()).expect("computed");
    assert_eq!(
        result.kind_name(),
        "Mesh",
        "primitive pairs stay faceted until a later slice opts them in (ADR 0004 slice boundary)"
    );
    let mesh = mesh_of(doc.get_by_name("Notch").unwrap(), 0.05).expect("a display mesh");
    assert!(
        !mesh.triangles.is_empty(),
        "the mesh boolean produced geometry"
    );
}

/// A derived exact result whose operands later break: the last-good
/// exact geometry survives on the feature (an execution error never
/// evicts it), and the errored feature's save is the loud refusal —
/// that exact geometry has no parameter set that can rebuild it.
#[test]
fn errored_exact_boolean_keeps_last_good_and_refuses_its_save() {
    let mut doc = Document::new("it");
    // An in-memory import carrying the sphere payload's exact model:
    // the operand geometry this scenario's save refusal protects.
    let sphere_bytes = brp_fixture("sphere");
    let sphere_text = String::from_utf8(sphere_bytes.clone()).expect("the fixture is UTF-8 text");
    let read = fc_part::read_occt_brp(&sphere_text).expect("the fixture reads");
    let mut import = MeshFeature::new("Import");
    import.shape = Some(TopoShape::Brep(read.brep));
    doc.add(Box::new(import));
    let mut ball = SphereFeature::new("Ball");
    ball.container.set("Radius", Property::Float(2.5));
    // Secant, not concentric: the primitive's placement offsets its
    // center to (4, 0, 0).
    ball.container.set(
        "Placement",
        Property::Placement(fc_base::Placement::new(
            v3(4.0, 0.0, 0.0),
            fc_base::Rotation::IDENTITY,
        )),
    );
    doc.add(Box::new(ball));
    let mut cut = BooleanFeature::new("Cut", BooleanKind::Cut);
    cut.container.set("Base", Property::Link("Import".into()));
    cut.container.set("Tool", Property::Link("Ball".into()));
    doc.add(Box::new(cut));
    doc.recompute_all().expect("the exact cut executes");
    let good = shape_of(doc.get_by_name("Cut").unwrap()).unwrap();
    assert_eq!(good.kind_name(), "Brep");

    // Break the tool link: the recompute errors, the last-good exact
    // result survives on the feature (exactly what a real FreeCAD
    // shows for an errored feature — the old shape, flagged).
    doc.set_property("Cut", "Tool", Property::Link(String::new()))
        .expect("unset");
    let err = doc.recompute_all().expect_err("the broken link errors");
    match err {
        RecomputeError::ExecuteFailed { object, reason } => {
            assert_eq!(object, "Cut");
            assert!(reason.contains("Tool must link"), "{reason}");
        }
        other => panic!("expected ExecuteFailed, got {other:?}"),
    }
    let kept = shape_of(doc.get_by_name("Cut").unwrap()).expect("the last-good result survives");
    assert_eq!(
        kept.kind_name(),
        "Brep",
        "the exact last-good geometry is kept"
    );

    // The save: the IMPORT has no preserved payload (it was built in
    // memory), so the save refuses loudly — provenance-free exact
    // geometry is not silently dropped (the standing contract).
    let temp = temp_path("lastgood");
    let err = save_document(&doc, &temp).expect_err("the save refuses");
    assert!(
        err.to_string().contains("Import") && err.to_string().contains("Brep"),
        "the refusal names the object and the kind: {err}"
    );

    std::fs::remove_file(&temp).ok();
}

// ---------------------------------------------------------------------------
// K4-2d/3 — the box × sphere family through the real features
// ---------------------------------------------------------------------------

/// THE box×sphere decisive chain: the imported real OCCT box (the
/// 30×20×5 corpus payload), an exact `Part::Cut` by a placed sphere
/// primitive at the corner station, the exact result measuring the
/// closed form (== real OCCT's own `BRepAlgoAPI` number), and the
/// full save → reopen → recompute cycle re-deriving the identical
/// exact geometry with the operand's bytes untouched.
#[test]
fn cut_of_imported_box_by_primitive_runs_exact_through_the_round_trip() {
    let path = temp_path("box-cut");
    let boxbrp = brp_fixture("box");
    // The sphere primitive r=3 at the identity placement: its center
    // is the box's corner (0, 0, 0) — the corner station, the octant
    // bite, r below every extent (the binding one is the height 5).
    craft_fcstd(
        &path,
        &import_sphere_ball_xml("Part::Cut", 3.0, 0.0),
        &[("ImportShape.brp", &boxbrp)],
    );

    let (mut doc, report) = load_document(&path).expect("the crafted document loads");
    assert!(
        report.warnings.is_empty(),
        "a readable payload and a clean primitive warn about nothing: {:?}",
        report.warnings
    );
    doc.recompute_all().expect("recompute executes the cut");

    // The operand: the imported exact box, live.
    let import_shape = shape_of(doc.get_by_name("Import").unwrap()).expect("the import is live");
    assert_eq!(import_shape.kind_name(), "Brep");
    // The result: EXACT — TopoShape::Brep, the closed form's numbers.
    let op = doc.get_by_name("Op").unwrap();
    let result = shape_of(op).expect("the cut computed");
    assert_eq!(
        result.kind_name(),
        "Brep",
        "the cut of an exact box operand carries the exact model (ADR 0004 rule 3)"
    );
    let got = result
        .try_volume()
        .expect("the bitten box measures exactly");
    // The reference: the closed form 3000 − (1/8)(4/3)πr³ — the same
    // number real OCCT's own BRepAlgoAPI cut answers (the corpus'
    // expected_box_sphere.json, authored by OCCT itself).
    let want = 3000.0 - 4.5 * std::f64::consts::PI;
    assert!(
        (got - want).abs() <= 1e-9 * want,
        "the feature's exact volume {got} is the closed form {want}"
    );
    // The differential anchor: the kernel API with the same placed
    // parameters the dispatch derived.
    let reference = outcome_volume(
        box_sphere_boolean(
            BooleanOp::Cut,
            v3(0.0, 0.0, 0.0),
            v3(1.0, 0.0, 0.0),
            v3(0.0, 1.0, 0.0),
            v3(0.0, 0.0, 1.0),
            30.0,
            20.0,
            5.0,
            v3(0.0, 0.0, 0.0),
            3.0,
        )
        .expect("the corner station is supported"),
    );
    assert!(
        (got - reference).abs() <= 1e-9 * reference,
        "the feature's exact volume {got} is the kernel's own {reference}"
    );
    // The display channel refuses honestly: the bite patch is a
    // spherical polygon (an arc-bounded sphere face), whose domain
    // classification is the K4-2d slice — `mesh_of` returns no mesh,
    // the named refusal riding `try_tessellate`.
    assert!(
        mesh_of(op, 0.05).is_none(),
        "no display mesh exists for the polygon-patched bite"
    );
    let tess_err = result
        .try_tessellate(0.05)
        .expect_err("the trim class refuses");
    assert!(
        tess_err.contains("spherical polygon") || tess_err.contains("K4-2d"),
        "the display refusal names the slice: {tess_err}"
    );

    // The save: the operand's OCCT bytes ride out verbatim; the
    // derived exact result is recomputed, not serialized (rule 4).
    let temp = temp_path("box-cut-roundtrip");
    save_document(&doc, &temp).expect("the exact-result document saves");
    assert_eq!(
        zip_entries(&temp).get("ImportShape.brp"),
        Some(&boxbrp),
        "the operand's OCCT payload rides out byte-identically"
    );

    // The reopen + recompute: the identical exact result.
    let (mut reloaded, report2) = load_document(&temp).expect("the saved file loads");
    assert!(
        report2.warnings.is_empty(),
        "the round trip is clean: {:?}",
        report2.warnings
    );
    reloaded
        .recompute_all()
        .expect("the reload recompute re-derives");
    let reloaded_result =
        shape_of(reloaded.get_by_name("Op").unwrap()).expect("the recompute restored the cut");
    assert_eq!(
        reloaded_result.kind_name(),
        "Brep",
        "the recomputed result is the exact model again"
    );
    let got2 = reloaded_result
        .try_volume()
        .expect("still measures exactly");
    assert_eq!(
        got2, got,
        "the re-derived exact volume is bit-identical to the first computation"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}

/// The fuse and the common of the same imported box — the bulge and
/// the octant ball, the closed forms, through the real features.
#[test]
fn fuse_and_common_of_imported_box_run_exact() {
    for (kind, want) in [
        ("Part::Fuse", 3000.0 + 31.5 * std::f64::consts::PI),
        ("Part::Common", 4.5 * std::f64::consts::PI),
    ] {
        let path = temp_path("box-bite");
        let boxbrp = brp_fixture("box");
        craft_fcstd(
            &path,
            &import_sphere_ball_xml(kind, 3.0, 0.0),
            &[("ImportShape.brp", &boxbrp)],
        );
        let (mut doc, _) = load_document(&path).expect("loads");
        doc.recompute_all().expect("recompute executes");
        let result = shape_of(doc.get_by_name("Op").unwrap()).expect("computed");
        assert_eq!(
            result.kind_name(),
            "Brep",
            "{kind}: the exact model carries the result"
        );
        let got = result.try_volume().expect("measures exactly");
        assert!(
            (got - want).abs() <= 1e-9 * want,
            "{kind}: the exact volume {got} is the closed form {want}"
        );
        std::fs::remove_file(&path).ok();
    }
}

/// The other stations through the feature: the edge bite (the sphere
/// primitive placed on a box edge) and the face bite (placed inside a
/// face) — the quarter and half closed forms.
#[test]
fn edge_and_face_bites_of_imported_box_run_exact() {
    // The edge station: the primitive placed at (12, 0, 0) — on the
    // bottom-front edge. The cut: V = 3000 − (1/4)(4/3)πr³.
    let path = temp_path("box-edge-cut");
    let boxbrp = brp_fixture("box");
    craft_fcstd(
        &path,
        &import_sphere_ball_xml("Part::Cut", 3.0, 12.0),
        &[("ImportShape.brp", &boxbrp)],
    );
    let (mut doc, _) = load_document(&path).expect("loads");
    doc.recompute_all().expect("recompute executes");
    let result = shape_of(doc.get_by_name("Op").unwrap()).expect("computed");
    assert_eq!(result.kind_name(), "Brep", "the edge bite is exact");
    let got = result.try_volume().expect("measures exactly");
    let want = 3000.0 - 9.0 * std::f64::consts::PI;
    assert!(
        (got - want).abs() <= 1e-9 * want,
        "the edge-bite volume {got} is the closed form {want}"
    );
    std::fs::remove_file(&path).ok();

    // The face station: the primitive placed at (15, 10, 0) — the
    // interior of the bottom face. The cut: V = 3000 − (1/2)(4/3)πr³.
    // (A full in-face placement needs Py too — a dedicated XML.)
    let xml = "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"3\" Dependencies=\"1\">
  <Object type=\"Part::Feature\" name=\"Import\" id=\"1\"/>
  <Object type=\"Part::Sphere\" name=\"Ball\" id=\"2\"/>
  <Object type=\"Part::Cut\" name=\"Op\" id=\"3\"/>
 </Objects>
 <ObjectData Count=\"3\">
  <Object name=\"Import\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"ImportShape.brp\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Ball\">
   <Properties Count=\"2\">
    <Property name=\"Radius\" type=\"App::PropertyFloat\">
     <Float value=\"2\"/>
    </Property>
    <Property name=\"Placement\" type=\"App::PropertyPlacement\">
     <PropertyPlacement Px=\"15\" Py=\"10\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Op\">
   <Properties Count=\"2\">
    <Property name=\"Base\" type=\"App::PropertyLink\">
     <Link value=\"Import\"/>
    </Property>
    <Property name=\"Tool\" type=\"App::PropertyLink\">
     <Link value=\"Ball\"/>
    </Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
";
    let path = temp_path("box-face-cut");
    craft_fcstd(&path, xml, &[("ImportShape.brp", &boxbrp)]);
    let (mut doc, _) = load_document(&path).expect("loads");
    doc.recompute_all().expect("recompute executes");
    let result = shape_of(doc.get_by_name("Op").unwrap()).expect("computed");
    assert_eq!(result.kind_name(), "Brep", "the face bite is exact");
    let got = result.try_volume().expect("measures exactly");
    let want = 3000.0 - 16.0 * std::f64::consts::PI / 3.0;
    assert!(
        (got - want).abs() <= 1e-9 * want,
        "the face-bite volume {got} is the closed form {want}"
    );
    std::fs::remove_file(&path).ok();
}

/// The sphere-Base × box-Tool cut: no kernel path in that order — the
/// refusal says the swap, never a silent one (the mirror of the
/// cylinder case).
#[test]
fn sphere_base_box_tool_cut_refuses_with_the_swap() {
    let path = temp_path("sphere-box-swap");
    let sphere = brp_fixture("sphere");
    let xml = "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"3\" Dependencies=\"1\">
  <Object type=\"Part::Feature\" name=\"Import\" id=\"1\"/>
  <Object type=\"Part::Box\" name=\"Block\" id=\"2\"/>
  <Object type=\"Part::Cut\" name=\"Op\" id=\"3\"/>
 </Objects>
 <ObjectData Count=\"3\">
  <Object name=\"Import\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"ImportShape.brp\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Block\">
   <Properties Count=\"4\">
    <Property name=\"Length\" type=\"App::PropertyFloat\">
     <Float value=\"30\"/>
    </Property>
    <Property name=\"Width\" type=\"App::PropertyFloat\">
     <Float value=\"20\"/>
    </Property>
    <Property name=\"Height\" type=\"App::PropertyFloat\">
     <Float value=\"5\"/>
    </Property>
    <Property name=\"Placement\" type=\"App::PropertyPlacement\">
     <PropertyPlacement Px=\"0\" Py=\"0\" Pz=\"0\" Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"1\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"Op\">
   <Properties Count=\"2\">
    <Property name=\"Base\" type=\"App::PropertyLink\">
     <Link value=\"Import\"/>
    </Property>
    <Property name=\"Tool\" type=\"App::PropertyLink\">
     <Link value=\"Block\"/>
    </Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
";
    craft_fcstd(&path, xml, &[("ImportShape.brp", &sphere)]);
    let (mut doc, _) = load_document(&path).expect("loads");
    let err = doc
        .recompute_all()
        .expect_err("the sphere-Base box-Tool cut refuses");
    match err {
        RecomputeError::ExecuteFailed { object, reason } => {
            assert_eq!(object, "Op");
            assert!(
                reason.contains("swap Base and Tool"),
                "the refusal names the workaround: {reason}"
            );
        }
        other => panic!("expected ExecuteFailed, got {other:?}"),
    }
    std::fs::remove_file(&path).ok();
}
