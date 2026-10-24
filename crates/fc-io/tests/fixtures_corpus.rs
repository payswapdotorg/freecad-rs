//! WOM native-file interop — the fixtures corpus acceptance tests.
//!
//! The corpus lives in `tests/fixtures/`: `.FCStd` zips (Document.xml
//! first, side files after — the same layout `save_document` writes) with
//! their XML committed as sibling `.xml` sources for review and
//! `.expected.txt` dumps recording name/type/status plus selected
//! properties per object. A sync test pins the committed zips to the
//! committed XML. Provenance is mixed and deliberate: `partdesign_body`,
//! `group_links` and the two sketch fixtures were regenerated through
//! real FreeCAD 1.0 (`scripts/parity/regen_fixtures.py`) after the
//! parity gate caught their hand-authored forms as un-restoreable in
//! the real program; the rest are hand-authored in the documented
//! canonical form.
//!
//! The four acceptance gates of the work order are covered here:
//!
//! 1. **Round-trip** — every fixture loads (no panics, no silent drops),
//!    re-saves, and the re-saved file loads back with identical `list`
//!    output (names/types/statuses; our writer emits our canonical form,
//!    so XML byte-parity is explicitly not the bar).
//! 2. **Per-object expectations** — each fixture is asserted against its
//!    `.expected.txt` dump.
//! 3. **`.brp` handling** — the `partdesign_body` Pad carries an OCCT
//!    BRep payload: the documented warning must appear, the readable
//!    parameters must load, the object must carry no geometry, and the
//!    load must not panic.
//! 4. **Unknown-type honesty** — `unknown_object.fcstd` has an object
//!    type we do not model: it must produce a per-object skipped record
//!    and must not appear as a silent success.

use std::io::Read;
use std::path::PathBuf;

use fc_app::{Document, Property};
use fc_io::{load_document, save_document, LoadReport};
use fc_part::partdesign::PadFeature;
use fc_part::shape_of;

/// Every fixture of the corpus, by stem (`.fcstd` + `.xml` +
/// `.expected.txt` all committed).
const FIXTURES: [&str; 5] = [
    "partdesign_body",
    "app_part_group",
    "property_zoo",
    "group_links",
    "unknown_object",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_path(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

fn read_fixture_text(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|e| panic!("fixture '{name}' readable: {e}"))
}

fn load_fixture(stem: &str) -> (Document, LoadReport) {
    load_document(&fixture_path(&format!("{stem}.fcstd")))
        .unwrap_or_else(|e| panic!("fixture '{stem}' loads: {e}"))
}

/// The `(name, type, status)` rows behind the CLI `list` output — the
/// round-trip parity bar (see `summaries_rows`).
fn summaries_rows(doc: &Document) -> Vec<(String, String, String)> {
    doc.summary()
        .into_iter()
        .map(|s| (s.name, s.type_name, s.status.to_string()))
        .collect()
}

/// The expectations dump: `type name status` per object (file order),
/// then the selected properties as `  key = to_text` lines.
fn dump(doc: &Document, select: &[(&str, &[&str])]) -> String {
    let mut out = String::new();
    for (name, keys) in select {
        let obj = doc
            .get_by_name(name)
            .unwrap_or_else(|| panic!("expectations: object '{name}' missing from document"));
        out.push_str(&format!(
            "{} {} {}\n",
            obj.type_name(),
            obj.name(),
            obj.status()
        ));
        for key in *keys {
            let value = obj
                .properties()
                .get(*key)
                .unwrap_or_else(|| panic!("expectations: '{name}' missing property '{key}'"))
                .to_text();
            out.push_str(&format!("  {key} = {value}\n"));
        }
    }
    out
}

/// Gate 2: the loaded document matches the committed `.expected.txt`.
fn assert_expectations(doc: &Document, stem: &str, select: &[(&str, &[&str])]) {
    let expected = read_fixture_text(&format!("{stem}.expected.txt"));
    let actual = dump(doc, select);
    assert_eq!(actual, expected, "{stem}: expectations mismatch");
}

/// Gate 1: re-save `doc`, reload it, and require identical `list` output
/// plus a second load report whose every warning is a *preservation*
/// note (data the model does not carry, kept verbatim — the re-saved
/// file re-presents it, and the honest reload says so; the canonical
/// forms — quantities/colors/lists/matrices — the reader takes
/// silently, as on the first load).
fn assert_round_trip_list_parity(stem: &str, doc: &Document) {
    // Unique per stem AND per thread: the expectations tests and the
    // every-fixture round-trip run in parallel test threads within one
    // process, so the pid alone does not disambiguate — a shared path
    // had two threads writing/reading one file concurrently, and the
    // reader legitimately saw a half-written zip ("incomplete deflate
    // stream"; first seen as a CI-only flake).
    let temp = std::env::temp_dir().join(format!(
        "fcrs-wom-roundtrip-{stem}-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(doc, &temp).unwrap_or_else(|e| panic!("{stem}: re-save: {e}"));
    let (reloaded, report) =
        load_document(&temp).unwrap_or_else(|e| panic!("{stem}: reload of re-saved file: {e}"));
    let _ = std::fs::remove_file(&temp);

    assert_eq!(
        summaries_rows(&reloaded),
        summaries_rows(doc),
        "{stem}: list parity across re-save"
    );
    assert_eq!(report.skipped, 0, "{stem}: nothing skipped on reload");
    assert!(
        report
            .warnings
            .iter()
            .all(|w| fc_app::is_preservation_warning(w)),
        "{stem}: reload warnings are all preservation notes (nothing lost): {:?}",
        report.warnings
    );
    assert!(
        report.skipped_properties.iter().all(|_| report
            .warnings
            .iter()
            .any(|w| fc_app::is_preservation_warning(w))),
        "{stem}: per-property skips on reload are all preservation-backed: {:?}",
        report.skipped_properties
    );
}

// ---------------------------------------------------------------------------
// Corpus integrity
// ---------------------------------------------------------------------------

/// The committed zips carry exactly the committed XML sources (the
/// reviewable `.xml` files are the source of truth for the corpus).
#[test]
fn fixture_zips_carry_the_committed_xml_sources() {
    for stem in FIXTURES {
        let path = fixture_path(&format!("{stem}.fcstd"));
        let file = std::fs::File::open(&path).expect("fixture zip opens");
        let mut zip = zip::ZipArchive::new(file).expect("fixture zip is a zip");
        let mut xml = String::new();
        zip.by_name("Document.xml")
            .expect("Document.xml entry")
            .read_to_string(&mut xml)
            .expect("Document.xml is utf-8");
        assert_eq!(
            xml,
            read_fixture_text(&format!("{stem}.xml")),
            "{stem}: Document.xml drifted from the committed .xml source"
        );
    }
}

// ---------------------------------------------------------------------------
// Gate 2 + 3: partdesign_body — Body/Origin(all six features)/Sketch/Pad,
// real FreeCAD 1.0 forms. Regenerated through freecadcmd (r39109) after
// the parity gate caught the hand-authored `Profile` persisted as
// `App::PropertyLink` — FreeCAD 1.0's Pad uses `App::PropertyLinkSub`,
// and the fake form broke restore ("No object linked"). Provenance:
// scripts/parity/regen_fixtures.py.
// ---------------------------------------------------------------------------

#[test]
fn partdesign_body_loads_per_expectations() {
    let (doc, report) = load_fixture("partdesign_body");

    // Gate 2: names, types, statuses, selected properties.
    assert_expectations(
        &doc,
        "partdesign_body",
        &[
            ("Body", &["Group", "Label", "Tip"]),
            ("Origin", &["Label", "OriginFeatures"]),
            ("X_Axis", &["Label", "Role"]),
            ("Y_Axis", &["Label", "Role"]),
            ("XY_Plane", &["Label", "Role"]),
            ("YZ_Plane", &["Label", "Role"]),
            ("Sketch", &["Label", "FullyConstrained"]),
            (
                "Pad",
                &[
                    "Direction",
                    "Label",
                    "Length",
                    "Midplane",
                    "Profile",
                    "Reversed",
                ],
            ),
        ],
    );

    // The whole PartDesign structure real FreeCAD builds: Body, Origin
    // with all six axis/plane features, Sketch, Pad. Nothing skipped,
    // nothing foreign — and every load warning is a preservation note
    // (the modern property set FreeCAD 1.0 writes rides verbatim).
    assert_eq!(report.objects, 10);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.foreign, 0);
    assert!(
        report
            .warnings
            .iter()
            .all(|w| fc_app::is_preservation_warning(w)),
        "{:?}",
        report.warnings
    );

    // The Pad's Profile is the LinkSub form, exactly as FreeCAD 1.0
    // wrote it (`<LinkSub value="Sketch" count="0">`).
    let pad = doc.get_by_name("Pad").unwrap();
    assert_eq!(
        pad.properties().get("Profile"),
        Some(&Property::LinkSub("Sketch".to_string(), Vec::new()))
    );
    let pad_feature = pad
        .as_any()
        .downcast_ref::<PadFeature>()
        .expect("Pad is a PadFeature");

    // Gate 3: the OCCT .brp payloads (the Pad's own shapes, the Body
    // and Sketch shapes) are skipped with THE documented warning, while
    // the readable parameters load.
    assert!(
        report.warnings.iter().any(|w| w.contains("Pad")
            && w.contains("OCCT BRep shape not modeled")
            && w.contains("geometry skipped (parameters kept)")
            && w.contains("payload preserved verbatim for round-trip")),
        "documented .brp warning (skipped AND preserved): {:?}",
        report.warnings
    );
    // (b) readable parameters loaded — proven by the expectations dump
    // above; (c) no geometry on the object:
    assert!(shape_of(pad).is_none(), "Pad carries no restored geometry");
    assert!(pad_feature.result.is_none(), "no computed result restored");
    // ...and the object is Touched, so a recompute would regenerate what
    // it can from the loaded parameters:
    assert_eq!(
        pad.status().to_string(),
        "Touched",
        "geometry-skipped feature stays Touched"
    );
    // (d) no panic — reaching this line is the proof.

    // The quantity-typed parameters were mapped to floats and recorded.
    assert_eq!(
        report.quantity_properties,
        vec![
            "Pad.Length".to_string(),
            "Pad.Length2".to_string(),
            "Pad.Offset".to_string(),
            "Pad.TaperAngle".to_string(),
            "Pad.TaperAngle2".to_string(),
        ],
        "quantity mapping recorded per property"
    );

    // Gate 1: re-save and reload with identical list output.
    assert_round_trip_list_parity("partdesign_body", &doc);
}

// ---------------------------------------------------------------------------
// Gate 2: app_part_group — App::Part container + grouped boxes + link
// ---------------------------------------------------------------------------

#[test]
fn app_part_group_loads_per_expectations() {
    let (doc, report) = load_fixture("app_part_group");

    assert_expectations(
        &doc,
        "app_part_group",
        &[
            ("Part", &["Group", "Label", "Placement"]),
            ("BoxA", &["Height", "Label", "Length", "Width"]),
            ("BoxB", &["Height", "Label", "Length", "Width"]),
            ("LinkB", &["Label", "LinkedObject", "LinkTransform"]),
        ],
    );

    assert_eq!(report.objects, 4);
    assert_eq!(report.skipped, 0);
    assert!(
        report.warnings.is_empty(),
        "clean fixture loads warning-free: {:?}",
        report.warnings
    );
    assert!(report.quantity_properties.is_empty());
    assert!(report.skipped_properties.is_empty());

    // The App::Link's targets are readable as ordinary properties.
    let link = doc.get_by_name("LinkB").unwrap();
    assert_eq!(
        link.properties().get("LinkedObject"),
        Some(&Property::Link("BoxB".to_string()))
    );
    assert_eq!(
        link.properties().get("LinkTransform"),
        Some(&Property::Bool(true))
    );
    // ...and the document graph sees the dependency edge.
    assert_eq!(doc.links_of("LinkB"), vec!["BoxB".to_string()]);

    assert_round_trip_list_parity("app_part_group", &doc);
}

// ---------------------------------------------------------------------------
// Gate 2: property_zoo — every property type of the work order, section 1
// ---------------------------------------------------------------------------

#[test]
fn property_zoo_maps_every_documented_property_type() {
    let (doc, report) = load_fixture("property_zoo");

    assert_expectations(
        &doc,
        "property_zoo",
        &[(
            "Zoo",
            &[
                "AColor",
                "AConstraint",
                "ADistance",
                "AFloatList",
                "ALength",
                "AMatrix",
                "AnAngle",
                "AnIntList",
                "AStringList",
                "AStringListEntries",
                "AVector",
                "AVectorElem",
                "Label",
            ],
        )],
    );

    assert_eq!(report.objects, 1);
    assert_eq!(report.skipped, 0);

    // Direct value asserts (the expectations dump proves the same things,
    // but spelling the mappings out here is the readable contract).
    let props = doc.get_by_name("Zoo").unwrap().properties();
    use fc_base::Vector3;
    assert_eq!(
        props.get("AVector"),
        Some(&Property::Vector(Vector3::new(1.0, -2.0, 0.5))),
        "PropertyVector, attribute form"
    );
    assert_eq!(
        props.get("AVectorElem"),
        Some(&Property::Vector(Vector3::new(3.0, 4.0, 5.0))),
        "PropertyVector, element form"
    );
    // Enumerations are preserved raw, not modeled: their FreeCAD form
    // (type attribute + custom value lists) is their truth, and a modeled
    // copy would re-serialize with a drifting type attribute. The value
    // is still in the preserved raw element (round-trips verbatim).
    assert!(
        !props.contains_key("AnEnum"),
        "enumeration is preserved raw, not modeled"
    );
    assert_eq!(props.get("AnAngle"), Some(&Property::Float(45.0)));
    assert_eq!(props.get("ALength"), Some(&Property::Float(12.5)));
    assert_eq!(props.get("ADistance"), Some(&Property::Float(3.25)));
    assert_eq!(props.get("AConstraint"), Some(&Property::Integer(7)));
    assert_eq!(
        props.get("AColor"),
        Some(&Property::String("#FF0000".into())),
        "packed uint 16711680 → documented \"#RRGGBB\" form"
    );
    assert_eq!(
        props.get("AStringList"),
        Some(&Property::String("alpha beta gamma".into())),
        "space/newline-separated payload, canonical single-space form"
    );
    assert_eq!(
        props.get("AStringListEntries"),
        Some(&Property::String("one two three".into())),
        "child-entry payload, canonical single-space form"
    );
    assert_eq!(
        props.get("AFloatList"),
        Some(&Property::String("0.5 1.5 2.5".into()))
    );
    assert_eq!(
        props.get("AnIntList"),
        Some(&Property::String("1 2 3".into()))
    );
    assert_eq!(
        props.get("AMatrix"),
        Some(&Property::String("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1".into())),
        "16 floats, row-major, single-space joined"
    );

    // Quantity typings recorded (file order — the zoo XML lists its
    // properties alphabetically); the unit survives in the record where
    // the file carried one.
    assert_eq!(
        report.quantity_properties,
        vec![
            "Zoo.ADistance (unit mm)".to_string(),
            "Zoo.ALength".to_string(),
            "Zoo.AnAngle (unit deg)".to_string(),
        ]
    );

    // The unknown property type and the enumeration are per-property
    // records, never silent drops and never load failures (the raw
    // elements ride along for verbatim round-trip).
    assert_eq!(
        report.skipped_properties,
        vec!["Zoo.AMystery".to_string(), "Zoo.AnEnum".to_string()]
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("Zoo")
            && w.contains("AMystery")
            && w.contains("unsupported value element <Weird>")),
        "unknown property warning: {:?}",
        report.warnings
    );

    assert_round_trip_list_parity("property_zoo", &doc);
}

// ---------------------------------------------------------------------------
// Gate 2: group_links — real FreeCAD 1.0 group nesting + a link to a
// grouped object. Regenerated through freecadcmd (r39109) after the
// parity gate caught the hand-authored corpus using the invented
// `App::DocumentGroup` type (which real FreeCAD drops on restore) and
// a string-list `Group`. Provenance: scripts/parity/regen_fixtures.py.
// ---------------------------------------------------------------------------

#[test]
fn group_links_load_per_expectations() {
    let (doc, report) = load_fixture("group_links");

    assert_expectations(
        &doc,
        "group_links",
        &[
            ("BoxA", &["Label", "Length"]),
            ("LeafBox", &["Label", "Length"]),
            ("LeafSphere", &["Label", "Radius"]),
            ("Outer", &["Group", "Label"]),
            ("Nested", &["Group", "Label"]),
            ("LinkLeaf", &["Label", "LinkedObject", "LinkTransform"]),
        ],
    );

    // The whole regenerated corpus file: six objects, nothing skipped,
    // nothing foreign — and every warning a preservation note (the
    // boxes' .brp shape payloads + FreeCAD 1.0's modern property set
    // ride in the preservation bag).
    assert_eq!(report.objects, 6);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.foreign, 0);
    assert!(report
        .warnings
        .iter()
        .all(|w| fc_app::is_preservation_warning(w)));

    // FreeCAD 1.0's real group type — the hand-authored predecessor's
    // fake `App::DocumentGroup` is gone.
    assert_eq!(
        doc.get_by_name("Outer").unwrap().type_name(),
        "App::DocumentObjectGroup"
    );
    assert_eq!(
        doc.get_by_name("Nested").unwrap().type_name(),
        "App::DocumentObjectGroup"
    );

    // The link's target is an `App::PropertyXLink` in the file
    // (`<XLink file="" stamp="" name="LeafBox"/>`): an in-document
    // extended link, mapped as the LinkSub it is semantically.
    assert_eq!(
        doc.get_by_name("LinkLeaf")
            .unwrap()
            .properties()
            .get("LinkedObject"),
        Some(&Property::LinkSub("LeafBox".to_string(), Vec::new()))
    );

    // The link points at an object that only exists inside a group's
    // (link-list) Group property — it still resolves by name.
    assert_eq!(doc.links_of("LinkLeaf"), vec!["LeafBox".to_string()]);
    assert_eq!(
        doc.links_of("Nested"),
        vec!["LeafBox".to_string(), "LeafSphere".to_string()]
    );

    // The writer re-emits FreeCAD's own XLink element for the pair
    // (the `<LinkSub>` form silently drops the value on FreeCAD's
    // restore — see `XLINK_PROPERTIES` in fcstd.rs; observed against
    // the r39109 binary). Pin the element in our re-save so the plain
    // hermetic suite holds the form the parity gate proved.
    let temp = std::env::temp_dir().join(format!(
        "fcrs-group-links-xlink-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&doc, &temp).unwrap();
    let file = std::fs::File::open(&temp).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut xml = String::new();
    zip.by_name("Document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let _ = std::fs::remove_file(&temp);
    assert!(
        xml.contains("<XLink file=\"\" stamp=\"\" name=\"LeafBox\"/>"),
        "the XLink element form is preserved in our re-save"
    );

    assert_round_trip_list_parity("group_links", &doc);
}

// ---------------------------------------------------------------------------
// Gate 4: unknown_object — unknown types are reported, never silent
// ---------------------------------------------------------------------------

#[test]
fn unknown_object_type_is_reported_not_silent() {
    let (doc, report) = load_fixture("unknown_object");

    // The unknown type is a per-object record: an opaque ForeignObject,
    // counted as foreign (nothing is dropped outright anymore).
    assert_eq!(report.skipped, 0, "nothing is dropped outright");
    assert_eq!(
        report.foreign, 1,
        "the unknown object is counted as foreign"
    );
    assert_eq!(report.objects, 1, "the known object still loads");
    assert!(
        report.warnings.iter().any(|w| w.contains("Ghost")
            && w.contains("unknown type 'Mesh::Feature'")
            && w.contains("preserved verbatim")),
        "per-object warning: {:?}",
        report.warnings
    );
    // ...and it is NOT a silent success: the object is really there,
    // under its file name and type, carrying its raw data.
    let ghost = doc.get_by_name("Ghost").expect("the foreign object loads");
    assert_eq!(ghost.type_name(), "Mesh::Feature", "the real type is kept");
    assert_eq!(doc.object_count(), 2);

    // The rest of the document is intact and dumped per expectations.
    assert_expectations(&doc, "unknown_object", &[("Known", &["Label", "Length"])]);

    assert_round_trip_list_parity("unknown_object", &doc);
}

// ---------------------------------------------------------------------------
// Gate 1, corpus-wide: every fixture round-trips to identical list output
// ---------------------------------------------------------------------------

/// Drives the full loop over all fixtures in one test so the acceptance
/// gate reads as one sentence: load → (expectations are the other tests'
/// business) → save → load → identical list rows.
#[test]
fn every_fixture_round_trips_to_identical_list_output() {
    for stem in FIXTURES {
        let (doc, report) = load_fixture(stem);
        // No silent drops on the FIRST load either: everything the file
        // names is loaded (modeled or foreign), or per-object skipped
        // with a record.
        let accounted = report.objects + report.skipped + report.foreign;
        assert!(
            accounted >= doc.object_count(),
            "{stem}: every file entry is loaded or recorded"
        );
        assert_round_trip_list_parity(stem, &doc);
    }
}

// ---------------------------------------------------------------------------
// Real-binary provenance fixtures: sketch constraints, both directions.
//
// `freecad_sketch_constraints.fcstd` was re-written by **real FreeCAD
// 1.0** (AppImage r39109, `freecadcmd` + `scripts/parity/regen_fixtures.py`)
// after the parity gate caught the hand-designed set as solver-conflicting
// (Perpendicular+Angle+Horizontal+Vertical overlap; Radius 9 + Diameter 18
// on one circle). The regenerated file carries two DOF-consistent sketches —
// FreeCAD itself reports both solved and fully constrained — with every one
// of the thirteen mappable kinds. Only the two absolute-position pins
// (Sketch) and the origin pin (Sketch001) reference the root point / X axis;
// their kinds are duplicated by non-root constraints, so the reader's
// preserve-verbatim handling of root-referencing constraints costs no
// coverage. Verified with the real binary both ways: FreeCAD opens
// `fcrs_constraints_roundtrip.fcstd` (freecad-rs's own output) and restores
// all thirteen constraints exactly; freecad-rs loads the FreeCAD-authored
// file to the same geometry + constraints.
// ---------------------------------------------------------------------------

/// The fixture authored by real FreeCAD 1.0 loads with every mappable
/// constraint kind intact — geometry, ordinals, point-slots and values,
/// exactly as the real program wrote them — with the root-referencing
/// position pins honestly preserved instead of silently dropped.
#[test]
fn freecad_authored_sketch_loads_all_thirteen_constraint_kinds() {
    let (doc, report) = load_fixture("freecad_sketch_constraints");

    // Two objects: the two sketches (FreeCAD's own console-authored
    // corpus file carries document properties we do not model —
    // Uid/Meta/Material — and each sketch's OCCT shape payloads; all
    // honestly warned, none fatal).
    assert_eq!(report.objects, 2);
    assert_eq!(report.skipped, 0);
    let object = doc.get_by_name("Sketch").expect("the first sketch loads");
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .expect("it is a Sketcher::SketchObject");
    let object2 = doc
        .get_by_name("Sketch001")
        .expect("the second sketch loads");
    let sketch_object2 = object2
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .expect("it is a Sketcher::SketchObject");

    // Geometry: the solved trapezoid (bottom 50, top 32, the right side
    // at the authored 1.9942294… rad angle — the height closes at
    // 39.93816941878718, the solver's own arithmetic) plus two
    // circles placed from the bottom-left vertex and a point on the
    // top edge.
    let sketch = &sketch_object.sketch;
    assert_eq!(sketch.geometry().len(), 7);
    let top_y = 39.93816941878718;
    assert!(matches!(
        sketch.geometry()[0],
        fc_sketcher::Geometry::LineSegment { start, end, .. }
            if (start.x + 25.0).abs() < 1e-9 && start.y.abs() < 1e-9
                && (end.x - 25.0).abs() < 1e-9 && end.y.abs() < 1e-9
    ));
    assert!(matches!(
        sketch.geometry()[1],
        fc_sketcher::Geometry::LineSegment { start, end, .. }
            if (start.x - 25.0).abs() < 1e-9 && start.y.abs() < 1e-9
                && (end.x - 7.0).abs() < 1e-9 && (end.y - top_y).abs() < 1e-9
    ));
    assert!(matches!(
        sketch.geometry()[4],
        fc_sketcher::Geometry::Circle { radius, .. } if (radius - 6.0).abs() < 1e-9
    ));
    assert!(matches!(
        sketch.geometry()[5],
        fc_sketcher::Geometry::Circle { radius, .. } if (radius - 4.0).abs() < 1e-9
    ));
    assert!(matches!(
        sketch.geometry()[6],
        fc_sketcher::Geometry::Point { x: -11.0, y } if (y - top_y).abs() < 1e-9
    ));

    // Eighteen of the twenty constraints load (the two root-point
    // position pins ride verbatim), with the authored targets and
    // values — eleven kinds in the first sketch alone.
    let authored_angle = 1.9942294539833942;
    let cs = sketch.constraints();
    assert_eq!(cs.len(), 18);
    use fc_sketcher::Constraint;
    assert!(
        matches!(cs[0], Constraint::Coincident { g1, pos1: fc_sketcher::PointPos::End, g2, pos2: fc_sketcher::PointPos::Start } if g1.0 == 0 && g2.0 == 1)
    );
    assert!(
        matches!(cs[1], Constraint::Coincident { g1, pos1: fc_sketcher::PointPos::End, g2, pos2: fc_sketcher::PointPos::Start } if g1.0 == 1 && g2.0 == 2)
    );
    assert!(
        matches!(cs[2], Constraint::Coincident { g1, pos1: fc_sketcher::PointPos::End, g2, pos2: fc_sketcher::PointPos::Start } if g1.0 == 2 && g2.0 == 3)
    );
    assert!(
        matches!(cs[3], Constraint::Coincident { g1, pos1: fc_sketcher::PointPos::End, g2, pos2: fc_sketcher::PointPos::Start } if g1.0 == 3 && g2.0 == 0)
    );
    assert!(matches!(cs[4], Constraint::Horizontal { g } if g.0 == 0));
    assert!(matches!(cs[5], Constraint::Parallel { g1, g2 } if g1.0 == 2 && g2.0 == 0));
    assert!(matches!(cs[6], Constraint::Perpendicular { g1, g2 } if g1.0 == 3 && g2.0 == 2));
    assert!(
        matches!(cs[7], Constraint::Angle { value, .. } if (value - authored_angle).abs() < 1e-15)
    );
    assert!(matches!(cs[8], Constraint::Distance { value, .. } if value == 50.0));
    assert!(matches!(cs[9], Constraint::DistanceX { value, .. } if value == -32.0));
    assert!(matches!(cs[10], Constraint::Radius { value, .. } if value == 6.0));
    assert!(matches!(cs[11], Constraint::DistanceX { value, .. } if value == 25.0));
    assert!(matches!(cs[12], Constraint::DistanceY { value, .. } if value == 20.0));
    assert!(matches!(cs[13], Constraint::Diameter { value, .. } if value == 8.0));
    assert!(matches!(cs[14], Constraint::DistanceX { value, .. } if value == 25.0));
    assert!(matches!(cs[15], Constraint::DistanceY { value, .. } if value == 32.0));
    assert!(
        matches!(cs[16], Constraint::PointOnObject { g1, pos1: fc_sketcher::PointPos::Start, g2 } if g1.0 == 6 && g2.0 == 2)
    );
    assert!(matches!(cs[17], Constraint::DistanceX { value, .. } if value == -18.0));

    // The second sketch adds the last two kinds (Vertical, Equal) on an
    // equal H/V pair from a shared start point.
    let sketch2 = &sketch_object2.sketch;
    assert_eq!(sketch2.geometry().len(), 2);
    let cs2 = sketch2.constraints();
    assert_eq!(cs2.len(), 5);
    assert!(
        matches!(cs2[0], Constraint::Coincident { g1, pos1: fc_sketcher::PointPos::Start, g2, pos2: fc_sketcher::PointPos::Start } if g1.0 == 1 && g2.0 == 0)
    );
    assert!(matches!(cs2[1], Constraint::Horizontal { g } if g.0 == 0));
    assert!(matches!(cs2[2], Constraint::Vertical { g } if g.0 == 1));
    assert!(matches!(cs2[3], Constraint::Equal { g1, g2 } if g1.0 == 0 && g2.0 == 1));
    assert!(matches!(cs2[4], Constraint::Distance { value, .. } if value == 10.0));

    // The three root-referencing constraints (Sketch's two position
    // pins + Sketch001's origin pin) are preserved verbatim — the
    // honest record, never a silent drop.
    let warnings = report.warnings.join("\n");
    assert!(
        warnings
            .matches("constraint #10 (13): references geometry")
            .count()
            == 1
            && warnings
                .matches("constraint #11 (7): references geometry")
                .count()
                == 1
            && warnings
                .matches("constraint #0 (1): references geometry")
                .count()
                == 1,
        "the three root-point pins are honestly preserved: {warnings}"
    );

    // The OCCT .brp payloads FreeCAD wrote (each sketch's Shape and
    // InternalShape) are the documented skip: honest warnings,
    // parameters kept, the objects left Touched for recompute.
    assert!(
        warnings.matches("unknown shape payload format").count() == 2,
        "both InternalShape payloads are honestly warned: {warnings}"
    );
    assert!(
        matches!(object.status(), fc_app::ObjectStatus::Touched)
            && matches!(object2.status(), fc_app::ObjectStatus::Touched),
        "both sketches reload Touched (their shapes are OCCT-only): {:?} / {:?}",
        object.status(),
        object2.status()
    );
}

/// freecad-rs's own file (the same thirteen constraints, our writer's
/// FreeCAD-native output) round-trips through our reader with the
/// identical content — and real FreeCAD 1.0 restores it exactly
/// (verified with the r39109 binary: ConstraintCount 13, every First/
/// FirstPos/Second/SecondPos/Value matching, recompute clean).
#[test]
fn our_constraint_output_round_trips_through_both_engines() {
    let (doc, report) = load_fixture("fcrs_constraints_roundtrip");
    assert_eq!(report.objects, 1);
    let object = doc.get_by_name("Sketch").expect("the sketch loads");
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .expect("it is a Sketcher::SketchObject");
    let sketch = &sketch_object.sketch;
    assert_eq!(sketch.geometry().len(), 6);
    assert_eq!(sketch.constraints().len(), 13);

    // Re-save keeps the content (our canonical writer).
    let temp = std::env::temp_dir().join(format!(
        "fcrs-constraints-rt-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&doc, &temp).unwrap();
    let (doc2, report2) = load_document(&temp).unwrap();
    let _ = std::fs::remove_file(&temp);
    assert!(report2.warnings.is_empty(), "{:?}", report2.warnings);
    let object2 = doc2.get_by_name("Sketch").unwrap();
    let sketch2 = &object2
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap()
        .sketch;
    assert_eq!(
        sketch2.to_xml(),
        sketch.to_xml(),
        "constraint + geometry content survives our save/load cycle byte-identically"
    );
}

// ---------------------------------------------------------------------------
// Real-binary provenance fixture: Tangent / Symmetric / Block — the three
// FreeCAD-native kinds that closed the last constraint parity gap.
//
// `freecad_tangent_symmetric_block.fcstd` was written by **real FreeCAD
// 1.0** (AppImage r39109, `freecadcmd`): twelve geometries and six
// constraints — a line tangent to a circle, two externally tangent
// circles, two collinear lines (FreeCAD's line-line tangency), points
// symmetric about a line, points symmetric about a point, and a blocked
// point. Every constraint was authored pre-satisfied, so the saved
// geometry is deterministic and both engines report it solved with
// zero movement. Verified with the real binary both ways: FreeCAD opens
// freecad-rs's re-save of this file and restores all six constraints
// exactly (including the about-point Symmetric's ThirdPos=3), recompute
// clean; freecad-rs loads the FreeCAD-authored file to the same
// geometry + constraints with DOF 23 — the same arithmetic FreeCAD
// reports (33 params - 2 blocked - 8 equations).
// ---------------------------------------------------------------------------

/// The fixture authored by real FreeCAD 1.0 loads with every Tangent /
/// Symmetric / Block constraint intact, and its DOF matches FreeCAD's
/// own arithmetic.
#[test]
fn freecad_authored_tangent_symmetric_block_loads_every_constraint() {
    let (doc, report) = load_fixture("freecad_tangent_symmetric_block");

    // One object: the sketch (the console-authored file carries
    // document properties we do not model and two OCCT .brp shape
    // payloads; all honestly warned, none fatal).
    assert_eq!(report.objects, 1);
    assert_eq!(report.skipped, 0);
    let object = doc.get_by_name("Sketch").expect("the sketch loads");
    let sketch_object = object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .expect("it is a Sketcher::SketchObject");
    let sketch = &sketch_object.sketch;

    // Twelve geometries, exactly as authored: bottom line + tangent
    // circle, the tangent circle pair, the collinear pair, the two
    // symmetric point pairs + symmetry point, the blocked point.
    assert_eq!(sketch.geometry().len(), 12);
    use fc_sketcher::Constraint;
    let cs = sketch.constraints();
    assert_eq!(cs.len(), 6);
    // Tangent: line 0 - circle 1 (both pos 0 = the simple edge form).
    assert!(matches!(cs[0], Constraint::Tangent { g1, g2 } if g1.0 == 0 && g2.0 == 1));
    // Tangent: circles 2 and 3, externally tangent.
    assert!(matches!(cs[1], Constraint::Tangent { g1, g2 } if g1.0 == 2 && g2.0 == 3));
    // Tangent: lines 4 and 5, FreeCAD's collinear decomposition.
    assert!(matches!(cs[2], Constraint::Tangent { g1, g2 } if g1.0 == 4 && g2.0 == 5));
    // Symmetric: points 6 and 7 about line 0.
    assert!(matches!(
        cs[3],
        Constraint::Symmetric {
            pos3: None,
            g3,
            ..
        } if g3.0 == 0
    ));
    // Symmetric: points 8 and 9 about point 10 (FreeCAD writes the
    // standalone point's slot as mid — ThirdPos=3).
    assert!(matches!(
        cs[4],
        Constraint::Symmetric {
            pos3: Some(fc_sketcher::PointPos::Center),
            g3,
            ..
        } if g3.0 == 10
    ));
    // Block: the point at index 11.
    assert!(matches!(cs[5], Constraint::Block { g } if g.0 == 11));

    // Every constraint was authored pre-satisfied: the sketch solves
    // without moving anything, and the DOF matches FreeCAD's own
    // arithmetic (33 params - 2 blocked - 8 equations = 23).
    assert_eq!(sketch.dof(), 23);
    let mut probe = sketch_object.sketch.clone();
    assert!(matches!(
        probe.solve(),
        Ok(fc_sketcher::SolveResult::Solved { iterations: 0, .. })
    ));
    // And the original geometry is bit-identical to what FreeCAD saved.
    let fc_sketcher::Geometry::Point { x, y } = sketch.geometry()[11] else {
        panic!("geometry 11 is the blocked point");
    };
    assert_eq!((x, y), (50.0, 50.0));

    // The OCCT .brp payloads are the documented skip (this file's
    // Sketch.Shape landed in the newer payload wrapper, its
    // InternalShape in the older raw stream — both honestly warned).
    let warnings = report.warnings.join("\n");
    let payload_warns = warnings.matches("unknown shape payload format").count()
        + warnings.matches("OCCT BRep shape not modeled").count();
    assert!(
        payload_warns >= 2,
        "both BRep payloads are honestly warned: {warnings}"
    );
}

/// freecad-rs's re-save of the FreeCAD-authored file round-trips through
/// our own reader byte-identically — and real FreeCAD 1.0 restores it
/// exactly (verified with the r39109 binary: ConstraintCount 6, every
/// First/FirstPos/Second/SecondPos/Third/ThirdPos matching — including
/// the about-point Symmetric's ThirdPos=3 — recompute clean, solve 0).
#[test]
fn our_tangent_symmetric_block_output_round_trips() {
    let (doc, report) = load_fixture("freecad_tangent_symmetric_block");
    assert_eq!(report.objects, 1);
    let object = doc.get_by_name("Sketch").expect("the sketch loads");
    let before = &object
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap()
        .sketch
        .to_xml();

    let temp = std::env::temp_dir().join(format!(
        "fcrs-tsb-rt-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&doc, &temp).unwrap();
    let (doc2, report2) = load_document(&temp).unwrap();
    let _ = std::fs::remove_file(&temp);
    // Our canonical writer emits only forms our reader takes cleanly —
    // the honest exceptions are preservation notes (raw data the model
    // does not carry, re-presented on every load).
    let warnings: Vec<String> = report2
        .warnings
        .iter()
        .filter(|w| !fc_app::is_preservation_warning(w))
        .cloned()
        .collect();
    assert!(warnings.is_empty(), "{:?}", warnings);
    let after = &doc2
        .get_by_name("Sketch")
        .unwrap()
        .as_any()
        .downcast_ref::<fc_sketcher::SketchObject>()
        .unwrap()
        .sketch
        .to_xml();
    assert_eq!(
        after, before,
        "tangent/symmetric/block content survives our save/load cycle byte-identically"
    );
}
