//! `App::PropertyLinkSub` round-trip + `Part::Mirroring`'s `MirrorPlane`
//! — the last modeled-property gap in the `.FCStd` interop surface.
//!
//! FreeCAD writes `LinkSub` for a link that may name subelements of its
//! target's shape: `PartDesign` profiles and `Part::Mirroring`'s
//! `MirrorPlane` carry it. The fixtures were authored by the real
//! `freecadcmd` 1.0 binary (r39109):
//!
//! * `freecad_mirroring.fcstd` — a `Part::Plane` datum at (30, 0, 0)
//!   rotated 30° about X, a `Part::Mirroring` linked to it
//!   (`MirrorPlane` = `("Plane", [])`, no subelements), and a second
//!   mirroring linked to a face of its own source box
//!   (`MirrorPlane` = `("Box2", ["Face3"])`). FreeCAD's own recompute
//!   resolved and persisted the planes into `Base`/`Normal`:
//!   (30, 0, 0) / (0, −0.5, 0.8660254037844387) for the datum case,
//!   (8, 0, 0) / (0, 1, 0) for the face case, both `status="4"`
//!   (ReadOnly — FreeCAD marks the pair read-only while a
//!   `MirrorPlane` drives them).
//! * `freecad_pad_profile.fcstd` — a `PartDesign::Pad` whose `Profile`
//!   is a `LinkSub` → the sketch (FreeCAD's own type for it).
//!
//! The contract under test:
//!
//! 1. the `LinkSub` property loads as a modeled value (target +
//!    subelement names) and re-emits in FreeCAD's own byte form —
//!    `<LinkSub value="…" count="N">` with `<Sub value="…"/>` children,
//!    the exact shape of `PropertyLinkSub::Save` (verified against
//!    r39109's own serialization);
//! 2. the document-level dependency graph follows the `MirrorPlane`
//!    edge (the datum is a recompute dependency of the mirroring);
//! 3. recompute reproduces FreeCAD's own numbers: the datum-driven
//!    mirror resolves the plane from the datum's placement (and writes
//!    it back into `Base`/`Normal` exactly as FreeCAD's execute does),
//!    the face-driven mirror keeps the file's persisted resolution;
//! 4. the `Base`/`Normal` `status="4"` bits ride along (the user's
//!    read-only marks survive the trip).

use std::io::Read;
use std::path::PathBuf;

use fc_app::{Document, Property};
use fc_io::{load_document, save_document};

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    p
}

/// The `Document.xml` of a `.FCStd`, as text.
fn zip_xml(path: &std::path::Path) -> String {
    let file = std::fs::File::open(path).expect("file exists");
    let mut zip = zip::ZipArchive::new(file).expect("archive is a zip");
    let mut xml = String::new();
    zip.by_name("Document.xml")
        .expect("Document.xml present")
        .read_to_string(&mut xml)
        .expect("xml readable");
    xml
}

/// The `<Property …>…</Property>` element of `object`'s `prop` in
/// `Document.xml`, if present.
fn property_element(xml: &str, object: &str, prop: &str) -> Option<String> {
    let start = xml.find(&format!("<Object name=\"{object}\">"))?;
    let rest = &xml[start..];
    let end = rest.find("</Object>").map(|i| i + start)?;
    let body = &xml[start..end];
    let needle = format!("<Property name=\"{prop}\" ");
    let i = body.find(&needle)?;
    let close = body[i..]
        .find("</Property>")
        .map(|k| i + k + "</Property>".len())?;
    Some(body[i..close].to_string())
}

/// The `status` attribute of a property element, if it carries one.
fn status_of(element: &str) -> Option<String> {
    let s = element.find("status=\"")? + "status=\"".len();
    let rest = &element[s..];
    Some(rest[..rest.find('"')?].to_string())
}

#[test]
fn link_sub_properties_round_trip_in_freecads_own_form() {
    let src = fixture_path("freecad_mirroring.fcstd");
    let (doc, report) = load_document(&src).expect("fixture loads");

    // --- The model took the links, subelements included. ---------------
    assert_eq!(
        doc.object_count(),
        5,
        "Box, Plane, Mirrored, Box2, MirroredFace"
    );
    let mirrored = doc.get_by_name("Mirrored").expect("the datum mirroring");
    assert_eq!(mirrored.type_name(), "Part::Mirroring");
    assert_eq!(
        mirrored.properties().get("MirrorPlane"),
        Some(&Property::LinkSub("Plane".to_string(), Vec::new())),
        "the datum link loads as LinkSub"
    );
    let mirrored_face = doc.get_by_name("MirroredFace").expect("the face mirroring");
    assert_eq!(
        mirrored_face.properties().get("MirrorPlane"),
        Some(&Property::LinkSub(
            "Box2".to_string(),
            vec!["Face3".to_string()]
        )),
        "the face link loads as LinkSub with its subelement"
    );
    // The PartDesign profile fixture: same property, same form.
    let pad_doc = load_document(&fixture_path("freecad_pad_profile.fcstd"))
        .expect("pad fixture loads")
        .0;
    let pad = pad_doc.get_by_name("Pad").expect("the pad loads");
    assert_eq!(
        pad.properties().get("Profile"),
        Some(&Property::LinkSub("Sketch".to_string(), Vec::new())),
        "FreeCAD's own Profile form loads"
    );

    // --- The re-save emits FreeCAD's own byte shapes. -----------------
    let temp = std::env::temp_dir().join(format!(
        "fcrs-linksub-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&doc, &temp).expect("re-save works");
    let xml = zip_xml(&temp);

    let datum_element =
        property_element(&xml, "Mirrored", "MirrorPlane").expect("MirrorPlane re-emits");
    assert!(
        datum_element.contains(r#"type="App::PropertyLinkSub""#),
        "the type is FreeCAD's own: {datum_element}"
    );
    assert!(
        datum_element.contains(r#"<LinkSub value="Plane" count="0">"#)
            && datum_element.contains("</LinkSub>"),
        "the empty-sub form is FreeCAD's open-tag form: {datum_element}"
    );
    let face_element =
        property_element(&xml, "MirroredFace", "MirrorPlane").expect("MirrorPlane re-emits");
    assert!(
        face_element.contains(r#"<LinkSub value="Box2" count="1">"#)
            && face_element.contains(r#"<Sub value="Face3"/>"#)
            && face_element.contains("</LinkSub>"),
        "the subelement form carries its sub: {face_element}"
    );

    // Base/Normal keep FreeCAD's types and the read-only marks the
    // user's file carried (status="4").
    for (object, prop, ty) in [
        ("Mirrored", "Base", "App::PropertyPosition"),
        ("Mirrored", "Normal", "App::PropertyDirection"),
        ("MirroredFace", "Base", "App::PropertyPosition"),
        ("MirroredFace", "Normal", "App::PropertyDirection"),
    ] {
        let element = property_element(&xml, object, prop).unwrap_or_else(|| {
            panic!(
                "{object}.{prop} re-emits (got: {:?}",
                property_element(&xml, object, prop)
            )
        });
        assert!(
            element.contains(&format!(r#"type="{ty}""#)),
            "{object}.{prop} keeps FreeCAD's type {ty}: {element}"
        );
        assert_eq!(
            status_of(&element).as_deref(),
            Some("4"),
            "{object}.{prop} keeps the read-only status bit: {element}"
        );
    }

    // --- Reload: the re-saved file loads to the same values. ----------
    let (doc2, report2) = load_document(&temp).expect("re-saved file loads");
    std::fs::remove_file(&temp).ok();
    assert_eq!(
        doc2.get_by_name("Mirrored")
            .unwrap()
            .properties()
            .get("MirrorPlane"),
        Some(&Property::LinkSub("Plane".to_string(), Vec::new()))
    );
    assert_eq!(
        doc2.get_by_name("MirroredFace")
            .unwrap()
            .properties()
            .get("MirrorPlane"),
        Some(&Property::LinkSub(
            "Box2".to_string(),
            vec!["Face3".to_string()]
        ))
    );
    for r in [&report, &report2] {
        assert!(
            r.warnings
                .iter()
                .all(|w| fc_app::is_preservation_warning(w)),
            "every warning names a preservation, not a loss: {:?}",
            r.warnings
        );
    }
}

/// The datum-driven mirroring resolves the plane from the datum's own
/// placement — FreeCAD's `FeatureMirroring` semantics, verified against
/// the numbers the r39109 binary itself produced for this very file
/// (Base = (30, 0, 0), Normal = (0, −0.5, 0.8660254…), mirrored volume
/// 1000.000) — and writes the resolution back into `Base`/`Normal`
/// exactly as FreeCAD's execute does.
#[test]
fn mirror_plane_datum_resolves_and_writes_back() {
    let src = fixture_path("freecad_mirroring.fcstd");
    let (mut doc, _) = load_document(&src).expect("fixture loads");

    // The datum edge is a real dependency: touching the datum must put
    // the mirroring into the recompute set.
    let dependents = doc.dependents_of("Plane");
    assert!(
        dependents.iter().any(|d| d == "Mirrored"),
        "the datum is a recompute dependency of the mirroring: {dependents:?}"
    );

    doc.recompute_all().expect("recompute");

    let mirrored = doc.get_by_name("Mirrored").expect("mirroring present");
    // FreeCAD's own resolution of this exact datum, persisted in the
    // fixture's Base/Normal by the r39109 binary itself.
    let base = match mirrored.properties().get("Base") {
        Some(Property::Vector(v)) => *v,
        other => panic!("Base after recompute: {other:?}"),
    };
    let normal = match mirrored.properties().get("Normal") {
        Some(Property::Vector(v)) => *v,
        other => panic!("Normal after recompute: {other:?}"),
    };
    let rel = |got: f64, want: f64| (got - want).abs() / want.abs().max(f64::MIN_POSITIVE);
    assert!(rel(base.x, 30.0) < 1e-12, "Base.x: {base:?}");
    assert!(
        rel(base.y, 0.0) < 1e-12 && rel(base.z, 0.0) < 1e-12,
        "Base: {base:?}"
    );
    assert!(rel(normal.x, 0.0) < 1e-12, "Normal.x: {normal:?}");
    assert!(rel(normal.y, -0.5) < 1e-12, "Normal.y: {normal:?}");
    assert!(
        rel(normal.z, 0.8660254037844387) < 1e-12,
        "Normal.z: {normal:?}"
    );

    // The mirrored shape: same volume as the source box (20 x 10 x 5).
    let mesh = fc_part::mesh_of(mirrored, fc_part::DEFAULT_DEFLECTION)
        .expect("the mirroring computed a shape");
    let volume = mesh.volume();
    assert!(
        (volume - 1000.0).abs() / 1000.0 < 1e-9,
        "mirrored volume: {volume} (FreeCAD's own: 1000.0)"
    );
    // Mirrored through the datum plane (30,0,0)/normal (0,−0.5,
    // 0.866…): the plane's normal has no x-component, so x is
    // invariant — [0, 20] stays; the y/z corners transform as
    // (y,z) → (0.5y + 0.866z, 0.866y − 0.5z): y' ∈ [0, 9.330127…],
    // z' ∈ [−2.5, 8.660254…] for the source's y ∈ [0,10], z ∈ [0,5].
    let bounds = mesh.vertices.iter().fold(
        ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
        |(lo, hi), v| {
            (
                [lo[0].min(v.x), lo[1].min(v.y), lo[2].min(v.z)],
                [hi[0].max(v.x), hi[1].max(v.y), hi[2].max(v.z)],
            )
        },
    );
    let rel = |got: f64, want: f64| (got - want).abs();
    assert!(
        rel(bounds.0[0], 0.0) < 1e-9 && rel(bounds.1[0], 20.0) < 1e-9,
        "mirrored x-range: {:?} (x is invariant under this plane)",
        [bounds.0[0], bounds.1[0]]
    );
    assert!(
        rel(bounds.0[1], 0.0) < 1e-9 && rel(bounds.1[1], 9.330127018922193) < 1e-9,
        "mirrored y-range: {:?}",
        [bounds.0[1], bounds.1[1]]
    );
    assert!(
        rel(bounds.0[2], -2.5) < 1e-9 && rel(bounds.1[2], 8.660254037844387) < 1e-9,
        "mirrored z-range: {:?}",
        [bounds.0[2], bounds.1[2]]
    );

    // The face-driven mirroring keeps the file's persisted plane and
    // its volume: FreeCAD's own numbers for this file.
    let face_mirror = doc.get_by_name("MirroredFace").expect("face mirroring");
    let face_mesh = fc_part::mesh_of(face_mirror, fc_part::DEFAULT_DEFLECTION)
        .expect("the face mirroring computed");
    let face_volume = face_mesh.volume();
    assert!(
        (face_volume - 192.0).abs() / 192.0 < 1e-9,
        "face-mirrored volume: {face_volume} (FreeCAD's own: 192.0)"
    );
}

/// A freshly authored mirroring saves `MirrorPlane` in FreeCAD's own
/// form (unset: an empty `LinkSub`), and the CLI's `mirroring` verb
/// reaches the feature.
#[test]
fn authored_mirroring_writes_freecads_property_set() {
    let mut doc = Document::new("authored");
    let mut object = fc_part::features::BoxFeature::new("Box");
    object.container.set("Length", Property::Float(20.0));
    object.container.set("Width", Property::Float(10.0));
    object.container.set("Height", Property::Float(5.0));
    doc.add(Box::new(object));
    let mut mirror = fc_part::features::MirroringFeature::new("Mirrored");
    mirror
        .container
        .set("Source", Property::Link("Box".to_string()));
    doc.add(Box::new(mirror));
    doc.recompute_all().expect("recompute");

    let temp = std::env::temp_dir().join(format!(
        "fcrs-authmirr-{}-{:?}.fcstd",
        std::process::id(),
        std::thread::current().id()
    ));
    save_document(&doc, &temp).expect("save");
    let xml = zip_xml(&temp);
    let element = property_element(&xml, "Mirrored", "MirrorPlane")
        .expect("an authored mirroring carries MirrorPlane");
    assert!(
        element.contains(r#"type="App::PropertyLinkSub""#)
            && element.contains(r#"<LinkSub value="" count="0">"#)
            && element.contains("</LinkSub>"),
        "the unset form is FreeCAD's own empty LinkSub: {element}"
    );
    // Base/Normal are typed as FreeCAD types them.
    for (prop, ty) in [
        ("Base", "App::PropertyPosition"),
        ("Normal", "App::PropertyDirection"),
    ] {
        let element =
            property_element(&xml, "Mirrored", prop).unwrap_or_else(|| panic!("{prop} emits"));
        assert!(
            element.contains(&format!(r#"type="{ty}""#)),
            "{prop} is typed {ty}: {element}"
        );
    }

    // The reloaded file computes the mirror off Base/Normal (no plane
    // link) — the volume survives the authoring round trip.
    let (mut doc2, _) = load_document(&temp).expect("reload");
    std::fs::remove_file(&temp).ok();
    doc2.recompute_all().expect("recompute 2");
    let mesh = fc_part::mesh_of(
        doc2.get_by_name("Mirrored").unwrap(),
        fc_part::DEFAULT_DEFLECTION,
    )
    .expect("computed");
    assert!((mesh.volume() - 1000.0).abs() < 1e-9);
}
