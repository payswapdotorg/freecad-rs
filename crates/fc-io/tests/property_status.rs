//! Property `status` bits round-trip — FreeCAD's persisted per-property
//! state (the `status` attribute on `<Property>` elements).
//!
//! FreeCAD stores more than values in a `.FCStd`: every property element
//! can carry a `status` bitmask — `1` Touched, `4` ReadOnly, `8` Hidden,
//! and the higher editor/engine bits (`8388608` LockDynamic,
//! `134217728` …). The bits a user sets in the editor (read-only,
//! hidden) are *persisted preferences*: dropping them on a round trip
//! would silently flip the user's document back to default visibility
//! and editability.
//!
//! The fixture (`freecad_property_status.fcstd`) was authored by the
//! real `freecadcmd` 1.0 (r39109): a `Part::Box` whose `Length` is
//! marked read-only (`setEditorMode("Length", 1)` → `status="4"`) and
//! whose `Width` is hidden (`setEditorMode("Width", 2)` →
//! `status="8"`), with FreeCAD's own session bits on the other
//! properties (object `Label` `134217728`, `Placement` `8388608`,
//! `Visibility` `648`, document `Label` `1`, `CreationDate`
//! `16777217`, …).
//!
//! The contract under test: the model re-serializes modeled properties
//! canonically (values it owns), but the file's `status` attribute —
//! state the model does not compute — re-emits verbatim, exactly like
//! every other preserved byte. Unmodeled properties carry theirs inside
//! their raw elements, untouched by construction.

use std::io::Read;
use std::path::PathBuf;

use fc_io::{load_document, save_document};

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    p
}

/// The re-saved `Document.xml`, split at the `<Objects` boundary:
/// (document section, object section).
fn roundtrip_sections() -> (String, String) {
    let src = fixture_path("freecad_property_status.fcstd");
    let (doc, report) = load_document(&src).expect("fixture loads");
    assert!(
        report
            .warnings
            .iter()
            .all(|w| fc_app::is_preservation_warning(w)),
        "every warning names a preservation, not a loss: {:?}",
        report.warnings
    );
    assert_eq!(doc.object_count(), 1, "the Box loads");
    let base = doc.get_by_name("Base").expect("Base present");
    assert_eq!(base.type_name(), "Part::Box");
    // The modeled values took: 10/10/10 (the defaults the fixture set).
    let props = base.properties();
    assert_eq!(props.get("Length"), Some(&fc_app::Property::Float(10.0)));
    assert_eq!(props.get("Width"), Some(&fc_app::Property::Float(10.0)));
    assert_eq!(props.get("Height"), Some(&fc_app::Property::Float(10.0)));

    let temp = std::env::temp_dir().join(format!(
        "fcrs-propstatus-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&doc, &temp).expect("re-save works");

    let xml = zip_xml(&temp);
    std::fs::remove_file(&temp).ok();
    let split = xml.find("<Objects").expect("object table present");
    (xml[..split].to_string(), xml[split..].to_string())
}

fn zip_xml(path: &std::path::Path) -> String {
    let file = std::fs::File::open(path).expect("re-saved file exists");
    let mut zip = zip::ZipArchive::new(file).expect("re-saved file is a zip");
    let mut xml = String::new();
    zip.by_name("Document.xml")
        .expect("Document.xml present")
        .read_to_string(&mut xml)
        .expect("xml readable");
    xml
}

/// The status attribute of `<Property name="…">` inside `section`, if the
/// element carries one.
fn status_of(section: &str, prop: &str) -> Option<String> {
    let needle = format!("<Property name=\"{prop}\" ");
    let i = section.find(&needle)? + needle.len();
    let rest = &section[i..];
    let attr_end = rest.find('>').expect("tag ends");
    let attrs = &rest[..attr_end];
    let s = attrs.find("status=\"")?;
    let v = &attrs[s + "status=\"".len()..];
    Some(v[..v.find('"').expect("quoted")].to_string())
}

#[test]
fn property_status_bits_round_trip() {
    let (doc_section, obj_section) = roundtrip_sections();

    // --- The user's editor preferences, byte-identical: ----------------
    // `setEditorMode("Length", 1)` — read-only.
    assert_eq!(status_of(&obj_section, "Length").as_deref(), Some("4"));
    // `setEditorMode("Width", 2)` — hidden.
    assert_eq!(status_of(&obj_section, "Width").as_deref(), Some("8"));
    // The untouched dimension carries no status attribute at all.
    assert_eq!(status_of(&obj_section, "Height"), None);

    // --- FreeCAD's own session bits on the modeled properties: --------
    assert_eq!(
        status_of(&obj_section, "Label").as_deref(),
        Some("134217728")
    );
    assert_eq!(
        status_of(&obj_section, "Placement").as_deref(),
        Some("8388608")
    );
    assert_eq!(
        status_of(&obj_section, "Visibility").as_deref(),
        Some("648")
    );

    // --- Document-level modeled properties: ---------------------------
    assert_eq!(status_of(&doc_section, "Label").as_deref(), Some("1"));
    assert_eq!(
        status_of(&doc_section, "CreationDate").as_deref(),
        Some("16777217")
    );
    // FreeCAD 1.0 writes no doc-level `Creator` — and the round-trip
    // injects none (the re-save omits the property entirely).
    assert!(!doc_section.contains("<Property name=\"Creator\""));
    assert_eq!(status_of(&doc_section, "Creator"), None);

    // --- Unmodeled properties keep theirs inside the raw element: -----
    // (`License` is unmodeled → raw-preserved with its status="1";
    // `ExpressionEngine` → raw with status="67108864".)
    assert_eq!(status_of(&doc_section, "License").as_deref(), Some("1"));
    assert_eq!(
        status_of(&obj_section, "ExpressionEngine").as_deref(),
        Some("67108864")
    );

    // --- Modeled-but-unusual types keep theirs through the canonical
    // path too (String/Bool/Float properties outside the model's core
    // vocabulary still parse as values):
    assert_eq!(
        status_of(&obj_section, "Label2").as_deref(),
        Some("67108992")
    );
    assert_eq!(
        status_of(&obj_section, "AttacherType").as_deref(),
        Some("83886080")
    );
    assert_eq!(status_of(&obj_section, "MapReversed").as_deref(), Some("8"));

    // --- And the values survived alongside the bits: ------------------
    assert!(obj_section
        .contains("<Property name=\"Length\" type=\"App::PropertyLength\" status=\"4\">"));
    assert!(obj_section.contains("<Float value=\"10\"/>"));
}

#[test]
fn reloaded_roundtrip_keeps_values() {
    // A re-save of the fixture reloads cleanly: same object, same
    // dimensions — the preserved status bits ride along without
    // disturbing the modeled data.
    let src = fixture_path("freecad_property_status.fcstd");
    let (doc, _) = load_document(&src).expect("fixture loads");
    let temp = std::env::temp_dir().join(format!(
        "fcrs-propstatus-reload-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&doc, &temp).expect("re-save works");
    let (again, report) = load_document(&temp).expect("re-saved file re-loads");
    std::fs::remove_file(&temp).ok();
    assert_eq!(again.object_count(), 1);
    assert_eq!(report.objects, 1);
    let base = again.get_by_name("Base").expect("Base present");
    let props = base.properties();
    assert_eq!(props.get("Length"), Some(&fc_app::Property::Float(10.0)));
    assert_eq!(props.get("Width"), Some(&fc_app::Property::Float(10.0)));
    assert_eq!(props.get("Height"), Some(&fc_app::Property::Float(10.0)));

    // And the second generation keeps the bits too.
    let temp2 = std::env::temp_dir().join(format!(
        "fcrs-propstatus-gen2-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&again, &temp2).expect("generation 2 saves");
    let xml2 = zip_xml(&temp2);
    std::fs::remove_file(&temp2).ok();
    let split = xml2.find("<Objects").expect("object table");
    assert_eq!(
        status_of(&xml2[split..], "Length").as_deref(),
        Some("4"),
        "read-only bit survives two generations"
    );
    assert_eq!(
        status_of(&xml2[split..], "Width").as_deref(),
        Some("8"),
        "hidden bit survives two generations"
    );
}
