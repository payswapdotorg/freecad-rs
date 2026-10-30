//! Task 6 acceptance — a real OCCT BRep payload becomes live geometry.
//!
//! The `.FCStd` loader used to refuse every `BrpFormat::OcctBrep`
//! payload with "OCCT BRep shape not modeled" — honest, but obsolete:
//! K4-1 reads the dialect into the exact model, K3/K4-2 measure and
//! tessellate it exactly. This file pins the wiring the loader now
//! owes its users:
//!
//! * a **readable** payload loads as `TopoShape::Brep` with OCCT's own
//!   exact volume/area (the K4-1 corpus fixtures, authored by real
//!   Open CASCADE via `BRepTools::Write`), the object NOT stuck
//!   `Touched` (the geometry restored — the file's `Valid` verdict
//!   stands, exactly what a real FreeCAD leaves after restore), and
//!   the payload riding back out of a re-save **byte-identically**
//!   (this kernel parses OCCT but writes no OCCT dialect — the file's
//!   own bytes stay the only truthful serialization of that geometry);
//! * a **rejected** payload (truncated; or non-UTF-8 bytes, which no
//!   CASCADE payload ever is) keeps the honest skip — warn with the
//!   reader's reason, parameters kept, object Touched so recompute
//!   regenerates what it can, bytes preserved verbatim;
//! * a `TopoShape::Brep` with **no preserved payload** (built in
//!   memory — the loader always pairs the two) makes `save_document`
//!   refuse loudly rather than silently dropping geometry no parameter
//!   set can rebuild.
//!
//! The BRep bytes come from the K4-1 corpus
//! (`fc-part/tests/fixtures/brp/`, `expected.json` carrying OCCT's own
//! measures — the same single corpus the reader-side acceptance in
//! `fc-part/tests/occt_brp.rs` pins; referencing it from here keeps one
//! source of truth for the fixture, two tiers of acceptance).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fc_app::{Document, ObjectStatus, Property};
use fc_io::{load_document, save_document};
use fc_part::features::MeshFeature;
use fc_part::{mesh_of, read_occt_brp, shape_of, TopoShape};

/// A K4-1 corpus fixture's bytes — real Open CASCADE output.
fn brp_fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fc-part/tests/fixtures/brp")
        .join(format!("{name}.brp"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name}.brp readable: {e}"))
}

/// A unique temp path (the corpus convention: pid + thread id — the
/// tests run on parallel threads and must not share a file).
fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "fcrs-occtshape-{tag}-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ))
}

/// Craft an in-memory `.FCStd`: `Document.xml` plus side files, the
/// same harness form `io.rs` uses for its imperfect-archive tests.
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
    let mut zip = zip::ZipArchive::new(file).expect("file is a zip");
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry readable");
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("entry readable");
        out.insert(entry.name().to_string(), data);
    }
    out
}

/// The document XML of a zip, as text.
fn zip_xml(path: &Path) -> String {
    String::from_utf8(zip_entries(path)["Document.xml"].clone()).expect("utf-8")
}

/// A two-object document: `Imported` (a `Part::Feature` — the
/// imported-data case, the type real FreeCAD files hang OCCT payloads
/// on) and `ParamBox` (a `Part::Box` — the parametric case: FreeCAD
/// persists every feature's computed shape, and a healthy feature's
/// parameters stay the rebuild truth). Each carries one shape payload
/// side file.
fn two_object_xml(first_entry: &str, second_entry: &str) -> String {
    format!(
        "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"2\" Dependencies=\"1\">
  <Object type=\"Part::Feature\" name=\"Imported\" id=\"1\"/>
  <Object type=\"Part::Box\" name=\"ParamBox\" id=\"2\"/>
 </Objects>
 <ObjectData Count=\"2\">
  <Object name=\"Imported\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"{first_entry}\"/>
    </Property>
   </Properties>
  </Object>
  <Object name=\"ParamBox\">
   <Properties Count=\"4\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"{second_entry}\"/>
    </Property>
    <Property name=\"Length\" type=\"App::PropertyLength\"><Float value=\"3\"/></Property>
    <Property name=\"Width\" type=\"App::PropertyLength\"><Float value=\"2\"/></Property>
    <Property name=\"Height\" type=\"App::PropertyLength\"><Float value=\"1.5\"/></Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
"
    )
}

// ---------------------------------------------------------------------------
// the readable payload — live exact geometry
// ---------------------------------------------------------------------------

#[test]
fn readable_occt_brep_payload_loads_as_exact_shape_and_round_trips() {
    let path = temp_path("readable");
    let cylinder = brp_fixture("cylinder");
    let box_payload = brp_fixture("box");
    craft_fcstd(
        &path,
        &two_object_xml("Shape001.brp", "Shape002.brp"),
        &[("Shape001.brp", &cylinder), ("Shape002.brp", &box_payload)],
    );

    let (mut doc, report) = load_document(&path).expect("the crafted document loads");

    // A payload the reader takes is not a warning — the honest report
    // is silence: geometry restored, nothing skipped, nothing lost.
    assert!(
        report.warnings.is_empty(),
        "a readable payload warns about nothing: {:?}",
        report.warnings
    );
    assert_eq!(report.objects, 2);

    // --- The imported shape: the exact model, OCCT's own numbers. ---
    let imported = doc.get_by_name("Imported").expect("the import loads");
    assert_eq!(imported.type_name(), "Part::Feature");
    // The geometry restored, so the object is NOT stuck Touched — the
    // file said Valid (no Touched attribute) and restoring the shape
    // is exactly what keeps that verdict true. A real FreeCAD leaves
    // the same state after its own restore.
    assert_eq!(
        imported.status(),
        &ObjectStatus::Valid,
        "restored geometry keeps the file's Valid verdict"
    );
    let shape = shape_of(imported).expect("the payload became live geometry");
    assert_eq!(shape.kind_name(), "Brep");
    // OCCT's own measures for this very fixture (expected.json).
    let want_volume = 125.6637061435917; // r=2, h=10
    let want_area = 150.79644737231004;
    let volume = shape
        .try_volume()
        .unwrap_or_else(|e| panic!("the cylinder measures: {e}"));
    let area = shape
        .try_surface_area()
        .unwrap_or_else(|e| panic!("the cylinder's area: {e}"));
    assert!(
        (volume - want_volume).abs() / want_volume <= 1e-9,
        "exact volume {volume:.12} vs OCCT {want_volume:.12}"
    );
    assert!(
        (area - want_area).abs() / want_area <= 1e-9,
        "exact area {area:.12} vs OCCT {want_area:.12}"
    );
    // The infallible channel agrees (a measuring B-rep has one number).
    assert_eq!(shape.volume(), volume);
    assert_eq!(shape.surface_area(), area);

    // --- The parametric feature: same wiring, its own payload. ---
    let param = doc.get_by_name("ParamBox").expect("the box loads");
    assert_eq!(param.status(), &ObjectStatus::Valid);
    let param_shape = shape_of(param).expect("the box payload loaded too");
    assert_eq!(param_shape.kind_name(), "Brep");
    let param_volume = param_shape
        .try_volume()
        .unwrap_or_else(|e| panic!("the box payload measures: {e}"));
    assert!(
        (param_volume - 3000.0).abs() / 3000.0 <= 1e-9,
        "box.brp is FreeCAD's own 30x10x10 box: {param_volume}"
    );

    // --- Recompute semantics: nothing is Touched, nothing rebuilds. ---
    // The restored shapes stand; a recompute is a no-op (Valid objects
    // are skipped, exactly FreeCAD's rule) — the payload's geometry
    // survives instead of being swapped for a parameter rebuild.
    let recomputed = doc.recompute_all().expect("recompute runs");
    assert!(
        recomputed.is_empty(),
        "no object is stuck Touched: {recomputed:?}"
    );
    assert_eq!(
        shape_of(doc.get_by_name("Imported").unwrap())
            .expect("the import keeps its shape")
            .kind_name(),
        "Brep"
    );
    assert_eq!(
        shape_of(doc.get_by_name("ParamBox").unwrap())
            .expect("the box keeps its shape")
            .try_volume()
            .expect("still measures"),
        param_volume,
        "the restored box shape survives a recompute"
    );

    // --- The tessellation renders: deflection-true and closed. ---
    let mesh =
        mesh_of(doc.get_by_name("Imported").unwrap(), 0.05).expect("the exact model tessellates");
    assert!(!mesh.triangles.is_empty(), "the mesh is not empty");
    assert!(
        (mesh.volume() - want_volume).abs() / want_volume <= 3.0e-2,
        "mesh volume {} within the measured d=0.05 bound of {want_volume}",
        mesh.volume()
    );

    // --- The re-save: the payloads ride out byte-identically. ---
    // This kernel parses OCCT but writes none — the file's own bytes
    // are the only truthful serialization, so the writer keeps them
    // under their original entry names (and the raw <Part> property
    // references them, not a fresh FCBREP entry).
    let temp = temp_path("readable-resave");
    save_document(&doc, &temp).expect("re-save works");
    let entries = zip_entries(&temp);
    assert_eq!(
        entries.get("Shape001.brp"),
        Some(&cylinder),
        "the cylinder payload rides out byte-identically"
    );
    assert_eq!(
        entries.get("Shape002.brp"),
        Some(&box_payload),
        "the box payload rides out byte-identically"
    );
    let xml = zip_xml(&temp);
    assert!(
        xml.contains("<Part file=\"Shape001.brp\"/>"),
        "the raw shape property still references the original entry"
    );
    assert!(
        !xml.contains("FCBREP"),
        "no fresh FCBREP side file evicts a preserved OCCT payload"
    );

    // --- Reload: the re-saved file restores the same geometry. -------
    let (doc2, report2) = load_document(&temp).expect("the re-saved file loads");
    assert!(
        report2.warnings.is_empty(),
        "the fixed point stays warning-free: {:?}",
        report2.warnings
    );
    let volume2 = shape_of(doc2.get_by_name("Imported").unwrap())
        .expect("the payload is live geometry again")
        .try_volume()
        .expect("it measures again");
    assert_eq!(volume2, volume, "the round-trip is volume-exact");

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}

// ---------------------------------------------------------------------------
// the rejected payloads — the honest skip, now with the reader's reason
// ---------------------------------------------------------------------------

#[test]
fn corrupted_occt_brep_payload_warns_skips_and_preserves_verbatim() {
    let path = temp_path("corrupt");
    // The same truncated form io.rs's imperfect-archive test uses: an
    // OCCT header with nothing behind it — the reader's refusal, not
    // the format probe's, is what skips it now.
    let payload: &[u8] = b"DBRep_DrawableShape\nCASCADE Topology\n";
    craft_fcstd(
        &path,
        &two_object_xml("Shape001.brp", "Shape001.brp"),
        &[("Shape001.brp", payload)],
    );

    let (mut doc, report) = load_document(&path).expect("the crafted document loads");

    // The warning carries the reader's verdict — the skip is the
    // reader's decision now, and the message says whose.
    assert!(
        report.warnings.iter().any(|w| w.contains("Imported")
            && w.contains("OCCT BRep payload rejected by the reader")
            && w.contains("geometry skipped (parameters kept)")
            && w.contains("payload preserved verbatim for round-trip")),
        "the rejection is honestly reported: {:?}",
        report.warnings
    );
    // Both objects (the import AND the parametric box) saw the same
    // verdict — the payload is one entry, the skip is per-property.
    assert!(
        report
            .warnings
            .iter()
            .filter(|w| w.contains("OCCT BRep payload rejected by the reader"))
            .count()
            >= 2,
        "each owning property reports its own skip: {:?}",
        report.warnings
    );

    // Geometry skipped, parameters kept, objects Touched for recompute.
    let imported = doc.get_by_name("Imported").expect("the import loads");
    assert!(
        shape_of(imported).is_none(),
        "the corrupted payload became no geometry"
    );
    assert_eq!(
        imported.status(),
        &ObjectStatus::Touched,
        "the import is Touched until something recomputes it"
    );
    let param = doc.get_by_name("ParamBox").expect("the box loads");
    assert_eq!(param.status(), &ObjectStatus::Touched);
    assert_eq!(
        param.properties().get("Length"),
        Some(&Property::Float(3.0)),
        "the parameters survived the skip"
    );

    // A recompute regenerates what it can: the box rebuilds from its
    // parameters (the parametric truth), the import has none (its
    // geometry was the payload — still preserved).
    let recomputed = doc.recompute_all().expect("recompute runs");
    assert_eq!(recomputed.len(), 2, "both Touched objects recompute");
    assert_eq!(
        shape_of(doc.get_by_name("ParamBox").unwrap()),
        Some(TopoShape::Box {
            length: 3.0,
            width: 2.0,
            height: 1.5
        }),
        "the box rebuilt from parameters"
    );
    assert!(
        shape_of(doc.get_by_name("Imported").unwrap()).is_none(),
        "an import without parameters grows no geometry"
    );

    // The re-save: the payload is the ONLY copy of the data — it must
    // ride out byte-identically under its original entry name.
    let temp = temp_path("corrupt-resave");
    save_document(&doc, &temp).expect("re-save works");
    assert_eq!(
        zip_entries(&temp).get("Shape001.brp"),
        Some(&payload.to_vec()),
        "the corrupted payload is preserved byte-identically"
    );
    assert!(
        zip_xml(&temp).contains("<Part file=\"Shape001.brp\"/>"),
        "the raw shape property re-emits its original reference"
    );

    // And the reload of that re-save repeats the honest verdict — the
    // skip is a fixed point, not a one-shot.
    let (_doc2, report2) = load_document(&temp).expect("the re-saved file loads");
    assert!(
        report2
            .warnings
            .iter()
            .any(|w| w.contains("OCCT BRep payload rejected by the reader")),
        "the reload reports the same rejection: {:?}",
        report2.warnings
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}

/// Non-UTF-8 bytes are a reader rejection, not a crash: a CASCADE BRep
/// payload is ASCII text, and bytes that are not can never parse.
#[test]
fn non_utf8_occt_brep_payload_is_a_reader_rejection() {
    let path = temp_path("notutf8");
    // An OCCT-shaped header (the probe classifies it OcctBrep) with
    // bytes no UTF-8 decoder accepts.
    let payload: &[u8] = b"DBRep_DrawableShape\nCASCADE Topology\nLocations \xFF\xFE";
    craft_fcstd(
        &path,
        &two_object_xml("Shape001.brp", "Shape001.brp"),
        &[("Shape001.brp", payload)],
    );

    let (doc, report) = load_document(&path).expect("the crafted document loads");
    assert!(
        report.warnings.iter().any(|w| w.contains("Imported")
            && w.contains("OCCT BRep payload rejected by the reader")
            && w.contains("not valid UTF-8")),
        "the non-UTF-8 rejection is reported as the reader's: {:?}",
        report.warnings
    );
    let imported = doc.get_by_name("Imported").expect("the import loads");
    assert!(shape_of(imported).is_none());
    assert_eq!(imported.status(), &ObjectStatus::Touched);

    // Verbatim round-trip, as for every rejection.
    let temp = temp_path("notutf8-resave");
    save_document(&doc, &temp).expect("re-save works");
    assert_eq!(
        zip_entries(&temp).get("Shape001.brp"),
        Some(&payload.to_vec()),
        "the non-UTF-8 payload is preserved byte-identically"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}

// ---------------------------------------------------------------------------
// the in-memory B-rep — no payload to preserve, so the save refuses
// ---------------------------------------------------------------------------

/// A `TopoShape::Brep` the file never carried (the loader always pairs
/// a live B-rep with its payload's provenance) has no serialization:
/// FCBREP v1 refuses the variant by name, and this kernel writes no
/// OCCT dialect. Saving such a shape silently would DROP geometry no
/// parameter set can rebuild — the writer refuses loudly instead.
#[test]
fn provenance_free_brep_refuses_the_save_loudly() {
    let brep = read_occt_brp(std::str::from_utf8(&brp_fixture("cylinder")).expect("ascii"))
        .expect("the fixture reads")
        .brep;

    let mut doc = Document::new("memory");
    let mut object = MeshFeature::new("Imported");
    object
        .container
        .set("Label", Property::String("Imported".into()));
    object.shape = Some(TopoShape::Brep(brep));
    doc.add(Box::new(object));

    let temp = temp_path("nopayload");
    let err = save_document(&doc, &temp).expect_err("the writer refuses an unwritable shape");
    // The refusal names the object and the shape kind — the user can
    // act on it (preserve a payload, or drop the shape deliberately).
    assert!(
        err.to_string().contains("Imported") && err.to_string().contains("Brep"),
        "the refusal names the object and the kind: {err}"
    );
    // The refusal precedes the first archive entry: the dropped
    // ZipWriter closes an EMPTY zip (its 22-byte end-of-central-
    // directory record) — no document inside, no half document.
    assert!(
        !zip_entries(&temp).contains_key("Document.xml"),
        "no document was written into the refused save: {:?}",
        zip_entries(&temp).keys().collect::<Vec<_>>()
    );
    std::fs::remove_file(&temp).ok();
}

// ---------------------------------------------------------------------------
// the attach tiers — where a B-rep can live, and where it cannot
// ---------------------------------------------------------------------------

/// A one-object document whose `Shape` property references one side
/// payload — the minimal carrier for exercising each attach tier.
fn single_object_xml(type_name: &str, entry: &str) -> String {
    format!(
        "\
<Document SchemaVersion=\"4\" ProgramVersion=\"test\" FileVersion=\"1\">
 <Objects Count=\"1\" Dependencies=\"1\">
  <Object type=\"{type_name}\" name=\"Obj\" id=\"1\"/>
 </Objects>
 <ObjectData Count=\"1\">
  <Object name=\"Obj\">
   <Properties Count=\"1\">
    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">
     <Part file=\"{entry}\"/>
    </Property>
   </Properties>
  </Object>
 </ObjectData>
</Document>
"
    )
}

/// A closed 2-manifold: every directed edge carried exactly once (the
/// same predicate shape as `fc-part`'s B-rep acceptance — a local copy,
/// since the shared helpers live in that crate's test suite).
fn assert_closed_manifold(mesh: &fc_part::Mesh, ctx: &str) {
    use std::collections::HashMap;
    assert!(!mesh.triangles.is_empty(), "{ctx}: no triangles");
    let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
    for t in &mesh.triangles {
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

/// A boolean's payload IS its last-good computation — a readable one
/// lands as the exact model (ADR 0004 rule 4: the mesh bridge for
/// booleans is retired). The object keeps the file's `Valid` verdict
/// (the geometry restored, exactly what a real FreeCAD leaves after
/// its own restore), the exact channels measure OCCT's own numbers,
/// the display mesh is the deflection-true tessellation of the exact
/// model — and the exact bytes ride out verbatim (they are still the
/// only truthful serialization of that computation).
#[test]
fn boolean_payload_lands_as_the_exact_last_good_result() {
    let path = temp_path("exact-restore");
    let cylinder = brp_fixture("cylinder");
    craft_fcstd(
        &path,
        &single_object_xml("Part::Cut", "CutShape.brp"),
        &[("CutShape.brp", &cylinder)],
    );

    let (doc, report) = load_document(&path).expect("the crafted document loads");
    // The payload reads — the honest report is silence.
    assert!(
        report.warnings.is_empty(),
        "a restored payload warns about nothing: {:?}",
        report.warnings
    );
    let obj = doc.get_by_name("Obj").expect("the cut loads");
    assert_eq!(
        obj.status(),
        &ObjectStatus::Valid,
        "the restored geometry keeps the file's Valid verdict"
    );
    // THE exact model is live on the boolean (no bridge, no stand-in):
    // its channels carry OCCT's own measure — the r=2, h=10 cylinder.
    let shape = shape_of(obj).expect("the exact model is live geometry");
    assert_eq!(
        shape.kind_name(),
        "Brep",
        "the boolean carries the exact model, not a bridged mesh"
    );
    let want_volume = 125.6637061435917; // OCCT's own measure, r=2 h=10
    let got = shape.try_volume().expect("the cylinder measures exactly");
    assert!(
        (got - want_volume).abs() / want_volume <= 1e-12,
        "the exact channel carries OCCT's own volume: {got} vs {want_volume}"
    );
    assert!(
        (shape.volume() - want_volume).abs() / want_volume <= 1e-12,
        "the infallible channel agrees — every face reads (to rounding)"
    );
    // The display mesh derives from the exact model at the caller's
    // deflection: watertight, within the deflection discipline (the
    // reader-side acceptance pins a 2.712e-2 inscribed deficit at
    // d=0.05 — the same measurement style).
    let mesh = mesh_of(obj, 0.05).expect("the exact model tessellates");
    assert_closed_manifold(&mesh, "the tessellated cylinder");
    assert!(
        (mesh.volume() - want_volume).abs() / want_volume <= 3.0e-2,
        "display mesh volume {} within the d=0.05 deficit of {want_volume}",
        mesh.volume()
    );

    // The re-save: the exact payload rides out byte-identically (the
    // restore never evicts it), and the boolean re-saves `Valid` —
    // the payload still IS the live geometry (`result_from_payload`),
    // so the file's promise holds for any reader (ours restores the
    // exact model; a real FreeCAD its own OCCT original).
    let temp = temp_path("exact-restore-resave");
    save_document(&doc, &temp).expect("re-save works");
    assert_eq!(
        zip_entries(&temp).get("CutShape.brp"),
        Some(&cylinder),
        "the exact payload rides out byte-identically"
    );
    assert!(
        zip_xml(&temp).contains("status=\"Valid\""),
        "the exact restore keeps the Valid promise — the payload is the geometry"
    );
    // And the reload lands exact again — the fixed point holds.
    let (doc2, report2) = load_document(&temp).expect("the re-saved file loads");
    assert!(report2.warnings.is_empty(), "still nothing to warn");
    let shape2 = shape_of(doc2.get_by_name("Obj").unwrap()).expect("still live");
    assert_eq!(
        shape2.kind_name(),
        "Brep",
        "the reload restores the exact model"
    );
    let got2 = shape2.try_volume().expect("still exact");
    assert!(
        (got2 - want_volume).abs() / want_volume <= 1e-12,
        "the fixed point measures identically: {got2} vs {want_volume}"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}

/// A B-rep the exact tiers cannot tessellate (the B-spline face), bound
/// for a mesh-result feature: the payload READS but nothing can land —
/// the reader-rejection semantics with the tessellator's reason in the
/// warning, the object Touched so recompute regenerates from profile.
#[test]
fn refusing_brep_on_mesh_result_feature_warns_and_stays_touched() {
    let path = temp_path("padrefuse");
    let bspline = brp_fixture("bspline_face");
    craft_fcstd(
        &path,
        &single_object_xml("PartDesign::Pad", "PadShape.brp"),
        &[("PadShape.brp", &bspline)],
    );

    let (doc, report) = load_document(&path).expect("the crafted document loads");
    // The refusal is reported as the tessellator's, with the refusing
    // geometry named — never a silent empty mesh standing in for a pad.
    assert!(
        report.warnings.iter().any(|w| w.contains("Obj")
            && w.contains("tessellation refused")
            && w.contains("Bspline")
            && w.contains("geometry skipped (parameters kept)")
            && w.contains("payload preserved verbatim for round-trip")),
        "the tessellation refusal is honestly reported: {:?}",
        report.warnings
    );
    let obj = doc.get_by_name("Obj").expect("the pad loads");
    assert_eq!(
        obj.status(),
        &ObjectStatus::Touched,
        "the pad is Touched until a recompute regenerates it"
    );
    assert!(
        mesh_of(obj, 0.05).is_none(),
        "no display mesh exists for the refused bridge"
    );

    // The re-save: the payload is the only copy of the data.
    let temp = temp_path("padrefuse-resave");
    save_document(&doc, &temp).expect("re-save works");
    assert_eq!(
        zip_entries(&temp).get("PadShape.brp"),
        Some(&bspline),
        "the refusing payload is preserved byte-identically"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}

/// The same refusing B-rep on a shape-holding feature (`Part::Feature`
/// — the import) attaches EXACTLY: the topology is live and queryable,
/// the per-operation refusals ride their honest channels (NaN never
/// reads as a number, `try_volume` names the face), and the loader
/// warns that no display mesh will exist rather than letting "no
/// mesh" read as "no geometry".
#[test]
fn refusing_brep_on_an_import_loads_exact_geometry_with_named_refusals() {
    let path = temp_path("importrefuse");
    let bspline = brp_fixture("bspline_face");
    craft_fcstd(
        &path,
        &single_object_xml("Part::Feature", "ImportShape.brp"),
        &[("ImportShape.brp", &bspline)],
    );

    let (doc, report) = load_document(&path).expect("the crafted document loads");
    // The geometry loaded exactly — but the display refusal is said
    // out loud (an import has no parameters to rebuild from, so
    // Touched would be a lie; Valid + the warning is the truth).
    assert!(
        report.warnings.iter().any(|w| w.contains("Obj")
            && w.contains("will not tessellate")
            && w.contains("Bspline")
            && w.contains("geometry loaded exactly, no display mesh")
            && w.contains("payload preserved verbatim for round-trip")),
        "the display refusal is reported without denying the geometry: {:?}",
        report.warnings
    );
    let obj = doc.get_by_name("Obj").expect("the import loads");
    assert_eq!(obj.status(), &ObjectStatus::Valid, "the geometry restored");
    let shape = shape_of(obj).expect("the exact model is live geometry");
    assert_eq!(shape.kind_name(), "Brep");
    // The honest channels: the refusal names the geometry; the
    // infallible one never invents a number.
    let err = shape.try_volume().expect_err("a Bspline face refuses");
    assert!(err.contains("Bspline"), "the refusal names the face: {err}");
    assert!(shape.volume().is_nan(), "volume is NaN, not a lie");
    assert!(
        mesh_of(obj, 0.05).is_none(),
        "no display mesh (the refusal, not an empty one)"
    );

    // The re-save and reload: the fixed point — Valid, the same
    // warning, the same bytes.
    let temp = temp_path("importrefuse-resave");
    save_document(&doc, &temp).expect("re-save works");
    assert_eq!(
        zip_entries(&temp).get("ImportShape.brp"),
        Some(&bspline),
        "the refusing payload is preserved byte-identically"
    );
    assert!(
        zip_xml(&temp).contains("status=\"Valid\""),
        "an import's restored geometry keeps its Valid verdict"
    );
    let (_doc2, report2) = load_document(&temp).expect("the re-saved file loads");
    assert!(
        report2
            .warnings
            .iter()
            .any(|w| w.contains("will not tessellate")),
        "the reload repeats the display refusal: {:?}",
        report2.warnings
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&temp).ok();
}
