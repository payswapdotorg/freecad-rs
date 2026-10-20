//! Full-stack integration tests for the expression engine and the
//! boolean features: document-level binding, recompute ordering,
//! cycle detection, persistence round-trips.

use fc_app::{Document, Property, RecomputeError};
use fc_io::{load_document, save_document};
use fc_part::features::{BooleanFeature, BoxFeature, CylinderFeature, SphereFeature};
use fc_part::{BooleanKind, TopoShape};

fn temp_path(tag: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("fcrs-it-{}-{}.fcstd", tag, std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

/// A box + sphere + cylinder playground document.
/// Float property of an object, via the trait surface.
fn float_of(doc: &Document, object: &str, property: &str) -> Option<f64> {
    doc.get_by_name(object)?
        .properties()
        .get(property)
        .and_then(|p| match p {
            Property::Float(v) => Some(*v),
            _ => None,
        })
}

fn playground() -> Document {
    let mut doc = Document::new("it");
    let mut base = BoxFeature::new("Base");
    base.container.set("Length", Property::Float(30.0));
    base.container.set("Width", Property::Float(20.0));
    base.container.set("Height", Property::Float(5.0));
    doc.add(Box::new(base));

    let mut ball = SphereFeature::new("Ball");
    ball.container.set("Radius", Property::Float(4.0));
    doc.add(Box::new(ball));

    let mut stem = CylinderFeature::new("Stem");
    stem.container.set("Radius", Property::Float(1.5));
    stem.container.set("Height", Property::Float(12.0));
    doc.add(Box::new(stem));
    doc.recompute_all().expect("playground recomputes");
    doc
}

#[test]
fn expression_binds_and_recomputes() {
    let mut doc = playground();
    doc.bind_expression("Stem", "Height", "2 * Base.Length / 3")
        .expect("binding is valid");
    doc.recompute_all().expect("recompute");
    assert_eq!(float_of(&doc, "Stem", "Height"), Some(20.0), "2 * 30 / 3");
}

#[test]
fn expression_cascades_when_source_changes() {
    let mut doc = playground();
    doc.bind_expression("Stem", "Height", "2 * Base.Length / 3")
        .expect("binding is valid");
    doc.recompute_all().expect("recompute");
    doc.set_property("Base", "Length", Property::Float(45.0))
        .expect("set source");
    doc.recompute_all().expect("recompute");
    assert_eq!(
        float_of(&doc, "Stem", "Height"),
        Some(30.0),
        "the expression re-evaluates when Base.Length changes"
    );
}

#[test]
fn expression_self_reference_and_functions() {
    let mut doc = playground();
    doc.bind_expression("Ball", "Radius", "sqrt(Stem.Height) + cos(60)")
        .expect("binding is valid");
    doc.recompute_all().expect("recompute");
    // sqrt(12) + cos(60 deg) = 3.4641... + 0.5
    let expected = 12.0_f64.sqrt() + 0.5;
    let got = float_of(&doc, "Ball", "Radius").unwrap();
    assert!(
        (got - expected).abs() < 1e-12,
        "got {got}, expected {expected}"
    );
}

#[test]
fn bound_property_is_read_only() {
    let mut doc = playground();
    doc.bind_expression("Stem", "Height", "Base.Length")
        .expect("binding is valid");
    let err = doc
        .set_property("Stem", "Height", Property::Float(1.0))
        .expect_err("bound properties reject set_property");
    assert!(
        err.contains("bound to an expression"),
        "error mentions the binding: {err}"
    );
    doc.clear_expression("Stem", "Height").expect("clear");
    doc.set_property("Stem", "Height", Property::Float(1.0))
        .expect("settable after clear");
}

#[test]
fn expression_cycle_is_rejected() {
    let mut doc = playground();
    doc.bind_expression("Stem", "Height", "Base.Length")
        .expect("Stem <- Base is fine");
    doc.bind_expression("Base", "Length", "Stem.Radius * 2")
        .expect("Base <- Stem is fine on its own");
    let err = doc.recompute_all().expect_err("Stem <-> Base is a cycle");
    match err {
        RecomputeError::Cyclic { cycle } => {
            assert_eq!(cycle, vec!["Base", "Stem", "Base"]);
        }
        other => panic!("expected Cyclic, got {other:?}"),
    }
}

#[test]
fn expressions_survive_a_save_load_round_trip() {
    let path = temp_path("expr");
    let mut doc = playground();
    doc.bind_expression("Stem", "Height", "2 * Base.Length / 3")
        .expect("binding is valid");
    doc.set_creator("it");
    doc.recompute_all().expect("recompute");
    save_document(&doc, &path).expect("save");

    let (mut reloaded, report) = load_document(&path).expect("load");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        reloaded
            .get_by_name("Stem")
            .unwrap()
            .expressions()
            .get("Height")
            .map(String::as_str),
        Some("2 * Base.Length / 3"),
        "the binding text persists"
    );
    // And it still drives the property after another change.
    reloaded
        .set_property("Base", "Length", Property::Float(60.0))
        .expect("set source");
    reloaded.recompute_all().expect("recompute");
    assert_eq!(
        float_of(&reloaded, "Stem", "Height"),
        Some(40.0),
        "2 * 60 / 3 after reload"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn boolean_cut_in_a_document() {
    let mut doc = playground();
    let mut cut = BooleanFeature::new("Notch", BooleanKind::Cut);
    cut.container.set("Base", Property::Link("Base".into()));
    cut.container.set("Tool", Property::Link("Stem".into()));
    doc.add(Box::new(cut));
    doc.recompute_all().expect("recompute");

    let notch = doc.get_by_name("Notch").unwrap();
    let (volume, tris) = match fc_part::shape_of(notch) {
        Some(TopoShape::Mesh(mesh)) => (mesh.volume(), mesh.triangle_count()),
        other => panic!("cut result should be a mesh, got {other:?}"),
    };
    // Base 30x20x5 minus the cylinder r=1.5 through its height (5).
    let expected = 30.0 * 20.0 * 5.0 - std::f64::consts::PI * 1.5 * 1.5 * 5.0;
    assert!(
        (volume - expected).abs() / expected < 0.01,
        "volume {volume} vs expected {expected}"
    );
    assert!(tris > 0);
}

#[test]
fn boolean_recomputes_when_operands_change() {
    let mut doc = playground();
    let mut cut = BooleanFeature::new("Notch", BooleanKind::Cut);
    cut.container.set("Base", Property::Link("Base".into()));
    cut.container.set("Tool", Property::Link("Stem".into()));
    doc.add(Box::new(cut));
    doc.recompute_all().expect("recompute");

    // Grow the tool radius: the notch must grow with it.
    doc.set_property("Stem", "Radius", Property::Float(3.0))
        .expect("set");
    doc.recompute_all().expect("recompute");
    let volume = match fc_part::shape_of(doc.get_by_name("Notch").unwrap()) {
        Some(TopoShape::Mesh(mesh)) => mesh.volume(),
        other => panic!("expected mesh, got {other:?}"),
    };
    let expected = 30.0 * 20.0 * 5.0 - std::f64::consts::PI * 3.0 * 3.0 * 5.0;
    assert!(
        (volume - expected).abs() / expected < 0.01,
        "volume {volume} vs expected {expected}"
    );
}

#[test]
fn booleans_survive_a_save_load_round_trip() {
    let path = temp_path("bool");
    let mut doc = playground();
    let mut fuse = BooleanFeature::new("Fused", BooleanKind::Fuse);
    fuse.container.set("Base", Property::Link("Ball".into()));
    fuse.container.set("Tool", Property::Link("Stem".into()));
    doc.add(Box::new(fuse));
    doc.recompute_all().expect("recompute");
    let before = match fc_part::shape_of(doc.get_by_name("Fused").unwrap()) {
        Some(TopoShape::Mesh(mesh)) => mesh,
        other => panic!("expected mesh, got {other:?}"),
    };
    doc.set_creator("it");
    save_document(&doc, &path).expect("save");

    let (reloaded, report) = load_document(&path).expect("load");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        reloaded.get_by_name("Fused").unwrap().type_name(),
        "Part::Fuse",
        "type preserved through the round trip"
    );
    let after = match fc_part::shape_of(reloaded.get_by_name("Fused").unwrap()) {
        Some(TopoShape::Mesh(mesh)) => mesh,
        other => panic!("expected mesh, got {other:?}"),
    };
    assert_eq!(
        before.triangle_count(),
        after.triangle_count(),
        "mesh preserved through the round trip"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn boolean_with_missing_link_reports_error() {
    let mut doc = playground();
    let mut cut = BooleanFeature::new("Notch", BooleanKind::Cut);
    cut.container.set("Base", Property::Link("Base".into()));
    // An unset Tool link is an empty name: the dependency walk rejects
    // it before the feature even executes.
    cut.container.set("Tool", Property::Link(String::new()));
    doc.add(Box::new(cut));
    let err = doc.recompute_all().expect_err("Tool is unset");
    match err {
        RecomputeError::MissingLink { object, target } => {
            assert_eq!(object, "Notch");
            assert_eq!(target, "");
        }
        other => panic!("expected MissingLink, got {other:?}"),
    }
}
