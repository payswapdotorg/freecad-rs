//! Integration tests for `fc-app`: the property system, the document
//! object graph, and the dependency-driven recompute engine.
//!
//! Test doubles are defined here (fc-app deliberately ships no concrete
//! object types — features live in fc-part, which depends on this crate).

use std::sync::{Arc, Mutex};

use fc_app::{
    impl_document_object, Document, DocumentObject, ObjectId, ObjectStatus, Property,
    PropertyContainer, RecomputeError,
};
use fc_base::{Placement, Rotation, Vector3};

const DEG: f64 = std::f64::consts::PI / 180.0;

// ---------------------------------------------------------------------------
// Test objects
// ---------------------------------------------------------------------------

/// A document object that records its name into a shared log whenever it
/// executes successfully.
struct Recorder {
    container: PropertyContainer,
    log: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn new(name: &str, log: &Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            container: PropertyContainer::new(name),
            log: Arc::clone(log),
        }
    }

    /// A recorder that links to `target` through its `Source` property.
    fn linked(name: &str, target: &str, log: &Arc<Mutex<Vec<String>>>) -> Self {
        let mut obj = Self::new(name, log);
        obj.container
            .set("Source", Property::Link(target.to_string()));
        obj
    }
}

impl Recorder {
    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        self.log
            .lock()
            .expect("log mutex")
            .push(self.container.name().to_string());
        Ok(())
    }
}

impl_document_object!(Recorder, "Test::Recorder");

/// A document object whose execution always fails with a fixed reason.
struct Failing {
    container: PropertyContainer,
}

impl Failing {
    fn execute_impl(&mut self, _doc: &Document) -> Result<(), String> {
        Err("kaboom".to_string())
    }
}

impl_document_object!(Failing, "Test::Failing");

/// Fresh shared execution log.
fn log() -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(Vec::new()))
}

/// Snapshot of a log's contents.
fn logged(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().expect("log mutex").clone()
}

/// Names of the given object ids, in order.
fn names_of(doc: &Document, ids: &[ObjectId]) -> Vec<String> {
    ids.iter()
        .map(|id| doc.get(*id).expect("object exists").name().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Property text round-trips
// ---------------------------------------------------------------------------

#[test]
fn property_text_round_trip_scalars_and_aggregates() {
    let samples: Vec<(Property, &str)> = vec![
        (Property::Bool(true), "true"),
        (Property::Bool(false), "false"),
        (Property::Float(2.5), "2.5"),
        (Property::Float(-0.0), "-0"),
        (Property::Float(0.1 + 0.2), "0.30000000000000004"),
        (Property::Float(1234567.89012345), "1234567.89012345"),
        (Property::Integer(7), "7"),
        (Property::Integer(-42), "-42"),
        (Property::Integer(i64::MIN), "-9223372036854775808"),
        (Property::String("hello world".into()), "hello world"),
        // Strings are raw: commas, semicolons and equals signs survive.
        (Property::String("a,b;c=d".into()), "a,b;c=d"),
        (
            Property::Vector(Vector3::new(1.5, -2.0, 0.25)),
            "1.5,-2,0.25",
        ),
        (
            Property::Vector(Vector3::new(0.1 + 0.2, 1e-13, -3.0)),
            "0.30000000000000004,0.0000000000001,-3",
        ),
        (Property::Link("Base".into()), "Base"),
        (Property::LinkList(Vec::new()), ""),
        (
            Property::LinkList(vec!["Base".into(), "Ball".into()]),
            "Base,Ball",
        ),
    ];
    for (value, text) in &samples {
        let ty = value.type_name();
        // Contract half 1: to_text emits the documented form...
        assert_eq!(&value.to_text(), text, "{ty}: to_text");
        // ...which parse re-parses exactly...
        let parsed = Property::parse(ty, text).unwrap_or_else(|| panic!("{ty}: parse of '{text}'"));
        assert_eq!(&parsed, value, "{ty}: parse(to_text) == value");
        // ...and contract half 2: to_text(parse(t)) == t.
        assert_eq!(&parsed.to_text(), text, "{ty}: to_text(parse(t)) == t");
    }

    // Every variant reports its FreeCAD-compatible type name.
    let types = [
        (Property::Bool(false), "App::PropertyBool"),
        (Property::Float(0.0), "App::PropertyFloat"),
        (Property::Integer(0), "App::PropertyInteger"),
        (Property::String(String::new()), "App::PropertyString"),
        (Property::Vector(Vector3::ZERO), "App::PropertyVector"),
        (
            Property::Placement(Placement::IDENTITY),
            "App::PropertyPlacement",
        ),
        (Property::Link(String::new()), "App::PropertyLink"),
        (Property::LinkList(Vec::new()), "App::PropertyLinkList"),
    ];
    for (value, ty) in types {
        assert_eq!(value.type_name(), ty);
    }
}

#[test]
fn property_placement_text_round_trip() {
    let samples = vec![
        Placement::IDENTITY,
        Placement::new(Vector3::ZERO, Rotation::around_z(90.0 * DEG)),
        Placement::new(
            Vector3::new(1.5, -2.25, 3.125),
            Rotation::around_x(45.0 * DEG),
        ),
        Placement::new(
            Vector3::new(12.0, 0.0, -4.0),
            Rotation::from_axis_angle(Vector3::new(1.0, 1.0, 1.0), 30.0 * DEG)
                .expect("non-zero axis"),
        ),
        Placement::new(Vector3::ZERO, Rotation::around_y(270.0 * DEG)),
        Placement::new(
            Vector3::new(-1.0, 2.0, 3.0),
            Rotation::around_x(180.0 * DEG),
        ),
        Placement::new(
            Vector3::new(0.1, 0.2, 0.3),
            Rotation::from_axis_angle(Vector3::new(0.0, -2.0, 3.0), 123.4567 * DEG)
                .expect("non-zero axis"),
        ),
        // Near-full-turn rotation: canonicalized by the emitter, still
        // text-stable.
        Placement::new(Vector3::ZERO, Rotation::around_z(359.9 * DEG)),
    ];
    for p in &samples {
        let text = Property::Placement(*p).to_text();
        let parsed = Property::parse("App::PropertyPlacement", &text)
            .unwrap_or_else(|| panic!("parse of '{text}'"));
        // Text stability of the emitted form...
        assert_eq!(
            parsed.to_text(),
            text,
            "to_text(parse(t)) == t for '{text}'"
        );
        // ...and value fidelity: position exactly, quaternion to 1e-9
        // (the axis/angle text grid quantizes the rotation slightly).
        let Property::Placement(q) = parsed else {
            panic!("parsed a placement");
        };
        assert_eq!(q.position, p.position, "position exact for '{text}'");
        for (a, b) in [
            (q.rotation.x, p.rotation.x),
            (q.rotation.y, p.rotation.y),
            (q.rotation.z, p.rotation.z),
            (q.rotation.w, p.rotation.w),
        ] {
            assert!(
                (a - b).abs() <= 1e-9,
                "quaternion drifted beyond 1e-9 for '{text}': {a} vs {b}"
            );
        }
    }

    // The identity and the 360-degree rotation share the canonical text.
    let full_turn = Rotation::from_axis_angle(Vector3::Z, 360.0 * DEG).expect("non-zero axis");
    assert_eq!(
        Property::Placement(Placement::new(Vector3::ZERO, full_turn)).to_text(),
        "pos=(0,0,0);axis=(0,0,1);angle=0"
    );
}

#[test]
fn property_parse_tolerates_whitespace_and_reordering() {
    // Whitespace around scalars and list entries is tolerated.
    assert_eq!(
        Property::parse("App::PropertyFloat", "  2.5  "),
        Some(Property::Float(2.5))
    );
    assert_eq!(
        Property::parse("App::PropertyInteger", " 7"),
        Some(Property::Integer(7))
    );
    assert_eq!(
        Property::parse("App::PropertyVector", "1, -2 , 3"),
        Some(Property::Vector(Vector3::new(1.0, -2.0, 3.0)))
    );
    assert_eq!(
        Property::parse("App::PropertyLinkList", " Base , Ball "),
        Some(Property::LinkList(vec!["Base".into(), "Ball".into()]))
    );
    assert_eq!(
        Property::parse("App::PropertyBool", "TRUE"),
        Some(Property::Bool(true))
    );
    // Placement segments may appear in any order.
    assert_eq!(
        Property::parse(
            "App::PropertyPlacement",
            "angle=90 ; axis=(0,0,1) ; pos=(1,2,3)"
        ),
        Property::parse(
            "App::PropertyPlacement",
            "pos=(1,2,3);axis=(0,0,1);angle=90"
        )
    );

    // An empty LinkList parses to the empty list.
    assert_eq!(
        Property::parse("App::PropertyLinkList", "  "),
        Some(Property::LinkList(Vec::new()))
    );
}

#[test]
fn property_parse_rejects_malformed_text() {
    assert_eq!(Property::parse("App::PropertyBool", "yes"), None);
    assert_eq!(Property::parse("App::PropertyFloat", "abc"), None);
    assert_eq!(Property::parse("App::PropertyInteger", "7.5"), None);
    assert_eq!(Property::parse("App::PropertyInteger", ""), None);
    assert_eq!(Property::parse("App::PropertyVector", "1,2"), None);
    assert_eq!(Property::parse("App::PropertyVector", "1,2,3,4"), None);
    assert_eq!(Property::parse("App::PropertyVector", "1,2,x"), None);
    assert_eq!(
        Property::parse("App::PropertyLinkList", "a,,b"),
        None,
        "empty entry inside a link list"
    );
    // Placement: missing / duplicate / unknown segments.
    assert_eq!(
        Property::parse("App::PropertyPlacement", "pos=(1,2,3);axis=(0,0,1)"),
        None,
        "missing angle"
    );
    assert_eq!(
        Property::parse(
            "App::PropertyPlacement",
            "pos=(1,2,3);pos=(4,5,6);axis=(0,0,1);angle=90"
        ),
        None,
        "duplicate pos"
    );
    assert_eq!(
        Property::parse(
            "App::PropertyPlacement",
            "pos=(1,2,3);axis=(0,0,1);angle=90;scale=2"
        ),
        None,
        "unknown key"
    );
    assert_eq!(
        Property::parse("App::PropertyPlacement", "pos=(1,2);axis=(0,0,1);angle=90"),
        None,
        "two-component position"
    );
    assert_eq!(
        Property::parse(
            "App::PropertyPlacement",
            "pos=(1,2,3);axis=(0,0,0);angle=90"
        ),
        None,
        "zero axis is degenerate"
    );
    // Unknown type names never parse.
    assert_eq!(Property::parse("App::PropertyNonsense", "1"), None);
}

/// The documented round-trip contract (`to_text(parse(t)) == t` for every
/// text this method emits) does not hold for degenerate link names.
///
/// The round-trip contract held for every representable name; degenerate
/// names were the gap. Fixed: `parse` keeps a single trailing empty list
/// entry, so `to_text` of `LinkList(["a", ""])` re-parses exactly.
/// (`LinkList([""])` still maps to the empty list — the one documented
/// ambiguity. `Link` names are verbatim, so padded names round-trip too.
/// unreachable through `Document::add`.)
#[test]
fn property_text_round_trip_degenerate_link_names() {
    let bad = Property::LinkList(vec!["a".into(), String::new()]);
    let text = bad.to_text();
    assert_eq!(
        Property::parse(bad.type_name(), &text).as_ref(),
        Some(&bad),
        "emitted '{text}' must re-parse to the original value"
    );

    let padded = Property::Link("Base ".into());
    let text = padded.to_text();
    assert_eq!(
        Property::parse(padded.type_name(), &text).as_ref(),
        Some(&padded),
        "emitted '{text}' must re-parse to the original value"
    );
}

// ---------------------------------------------------------------------------
// PropertyContainer
// ---------------------------------------------------------------------------

#[test]
fn property_container_set_get_and_typed_accessors() {
    let mut c = PropertyContainer::new("Thing");
    assert_eq!(c.name(), "Thing");
    c.set_name("Other");
    assert_eq!(c.name(), "Other");

    // Fresh containers start Touched (FreeCAD semantics).
    assert_eq!(c.status(), &ObjectStatus::Touched);

    c.set("F", Property::Float(1.5));
    c.set("S", Property::String("hi".into()));
    c.set("V", Property::Vector(Vector3::new(1.0, 2.0, 3.0)));
    c.set("P", Property::Placement(Placement::IDENTITY));
    c.set("L", Property::Link("Base".into()));
    c.set("LL", Property::LinkList(vec!["Base".into(), "Ball".into()]));

    assert_eq!(c.get("F"), Some(&Property::Float(1.5)));
    assert_eq!(c.get_float("F"), Some(1.5));
    assert_eq!(c.get_string("S"), Some("hi".into()));
    assert_eq!(c.get_vector("V"), Some(Vector3::new(1.0, 2.0, 3.0)));
    assert_eq!(c.get_placement("P"), Some(Placement::IDENTITY));
    assert_eq!(c.get_links("L"), Some(vec!["Base".to_string()]));
    assert_eq!(
        c.get_links("LL"),
        Some(vec!["Base".to_string(), "Ball".to_string()])
    );

    // Typed accessors return None on a missing key...
    assert_eq!(c.get_float("nope"), None);
    // ...and on a variant mismatch.
    assert_eq!(c.get_float("S"), None, "Float accessor on a String");
    assert_eq!(c.get_string("F"), None, "String accessor on a Float");
    assert_eq!(c.get_vector("F"), None, "Vector accessor on a Float");
    assert_eq!(c.get_placement("V"), None, "Placement accessor on a Vector");
    assert_eq!(c.get_links("V"), None, "Links accessor on a Vector");
    // get_links accepts both Link and LinkList variants.
    assert_eq!(c.get_links("L"), Some(vec!["Base".to_string()]));

    // touch() flips a Valid status back to Touched.
    c.set_status(ObjectStatus::Valid);
    c.touch();
    assert_eq!(c.status(), &ObjectStatus::Touched);
}

// ---------------------------------------------------------------------------
// Document: names, properties, links
// ---------------------------------------------------------------------------

#[test]
fn document_assigns_freecad_style_unique_names() {
    let log = log();
    let mut doc = Document::new("Names");

    // FreeCAD-style suffixing: Box, Box001, Box002.
    let a = doc.add(Box::new(Recorder::new("Box", &log)));
    let b = doc.add(Box::new(Recorder::new("Box", &log)));
    let c = doc.add(Box::new(Recorder::new("Box", &log)));
    assert_eq!(doc.get(a).unwrap().name(), "Box");
    assert_eq!(doc.get(b).unwrap().name(), "Box001");
    assert_eq!(doc.get(c).unwrap().name(), "Box002");
    // Ids are assigned 1, 2, 3, ... in creation order.
    assert_eq!((a, b, c), (ObjectId(1), ObjectId(2), ObjectId(3)));

    // The uniquifier predicts the next free name.
    assert_eq!(doc.unique_name("Box"), "Box003");
    assert_eq!(doc.unique_name("Sphere"), "Sphere");

    // Names are sanitized: keep [A-Za-z0-9_], collapse other runs to '_'.
    let weird = doc.add(Box::new(Recorder::new("My Box!", &log)));
    assert_eq!(doc.get(weird).unwrap().name(), "My_Box_");
    let symbols = doc.add(Box::new(Recorder::new("###", &log)));
    assert_eq!(doc.get(symbols).unwrap().name(), "_");
    let empty = doc.add(Box::new(Recorder::new("", &log)));
    assert_eq!(doc.get(empty).unwrap().name(), "Object");

    assert_eq!(doc.object_count(), 6);

    // Name lookup agrees with id lookup.
    assert_eq!(doc.get_id("Box001"), Some(b));
    assert_eq!(doc.get_id("Nonexistent"), None);
    assert_eq!(
        doc.get_by_name("Box002").map(DocumentObject::name),
        Some("Box002")
    );
    assert!(doc.get_by_name("nonexistent-in-any-case").is_none());
}

#[test]
fn document_set_and_get_property() {
    let log = log();
    let mut doc = Document::new("Props");
    let id = doc.add(Box::new(Recorder::new("Thing", &log)));

    doc.set_property("Thing", "Length", Property::Float(42.5))
        .expect("set on existing object");
    doc.set_property("Thing", "Count", Property::Integer(3))
        .expect("set on existing object");
    doc.set_property("Thing", "Note", Property::String("rounded".into()))
        .expect("set on existing object");

    let obj = doc.get(id).expect("object exists");
    let props = obj.properties();
    assert_eq!(props.get("Length"), Some(&Property::Float(42.5)));
    assert_eq!(props.get("Count"), Some(&Property::Integer(3)));
    assert_eq!(props.get("Note"), Some(&Property::String("rounded".into())));
    assert_eq!(props.len(), 3, "exactly the three properties set");

    // Unknown objects are an error, not a panic.
    assert_eq!(
        doc.set_property("Ghost", "Length", Property::Float(1.0)),
        Err("object 'Ghost' not found".to_string())
    );

    // Setting a property touches the object.
    assert_eq!(doc.get(id).unwrap().status(), &ObjectStatus::Touched);
}

#[test]
fn document_links_of_and_dependents_of() {
    let log = log();
    let mut doc = Document::new("Graph");
    doc.add(Box::new(Recorder::new("A", &log)));
    doc.add(Box::new(Recorder::linked("B", "A", &log)));
    doc.add(Box::new(Recorder::linked("C", "B", &log)));
    doc.add(Box::new(Recorder::new("D", &log)));
    doc.add(Box::new(Recorder::linked("X", "A", &log)));

    // links_of: the targets of one object's Link/LinkList properties.
    assert_eq!(doc.links_of("A"), Vec::<String>::new());
    assert_eq!(doc.links_of("B"), vec!["A".to_string()]);
    assert_eq!(doc.links_of("C"), vec!["B".to_string()]);
    assert_eq!(doc.links_of("Missing"), Vec::<String>::new());

    // dependents_of: breadth-first, creation-ordered within each level.
    assert_eq!(
        doc.dependents_of("A"),
        vec!["B".to_string(), "X".to_string(), "C".to_string()]
    );
    assert_eq!(doc.dependents_of("B"), vec!["C".to_string()]);
    assert_eq!(doc.dependents_of("C"), Vec::<String>::new());
    assert_eq!(doc.dependents_of("Ghost"), Vec::<String>::new());
}

#[test]
fn document_remove_respects_parametric_integrity() {
    let log = log();
    let mut doc = Document::new("Remove");
    doc.add(Box::new(Recorder::new("A", &log)));
    doc.add(Box::new(Recorder::linked("B", "A", &log)));

    // A linked-to object cannot be removed; the error names the dependent.
    let err = doc.remove("A").expect_err("still linked");
    assert!(
        err.contains("cannot remove 'A'"),
        "error names the object: {err}"
    );
    assert!(err.contains("B"), "error names the dependent: {err}");
    assert_eq!(doc.object_count(), 2);

    // An unlinked object removes cleanly.
    doc.remove("B").expect("unlinked object removes");
    assert_eq!(doc.object_count(), 1);
    assert!(doc.get_by_name("B").is_none());
    doc.remove("A").expect("dependent gone, source frees up");
    assert_eq!(doc.object_count(), 0);
    assert_eq!(
        doc.remove("A"),
        Err("object 'A' not found".to_string()),
        "removing twice fails cleanly"
    );
}

// ---------------------------------------------------------------------------
// Recompute engine
// ---------------------------------------------------------------------------

#[test]
fn recompute_runs_dependencies_first() {
    // Build the chain in *reverse* dependency order (C, then B, then A):
    // the engine must still execute A before B before C.
    let log = log();
    let mut doc = Document::new("Chain");
    let c = doc.add(Box::new(Recorder::linked("C", "B", &log)));
    let b = doc.add(Box::new(Recorder::linked("B", "A", &log)));
    let a = doc.add(Box::new(Recorder::new("A", &log)));

    let executed = doc.recompute_all().expect("acyclic chain recomputes");
    assert_eq!(executed, vec![a, b, c], "execution order is A, B, C");
    assert_eq!(logged(&log), vec!["A", "B", "C"]);

    // Everything is Valid after a successful pass.
    for name in ["A", "B", "C"] {
        assert_eq!(
            doc.get_by_name(name).unwrap().status(),
            &ObjectStatus::Valid,
            "{name} valid after recompute"
        );
    }

    // A second pass with nothing touched is a no-op.
    let again = doc.recompute_all().expect("nothing touched");
    assert_eq!(again, Vec::<ObjectId>::new(), "no object is recomputed");
    assert_eq!(logged(&log), vec!["A", "B", "C"], "no extra executions");
}

#[test]
fn touch_propagation_recomputes_only_dependents() {
    let log = log();
    let mut doc = Document::new("Cascade");
    doc.add(Box::new(Recorder::new("A", &log)));
    doc.add(Box::new(Recorder::linked("B", "A", &log)));
    doc.add(Box::new(Recorder::linked("C", "B", &log)));
    let d = doc.add(Box::new(Recorder::new("D", &log)));

    doc.recompute_all().expect("initial recompute");
    assert_eq!(logged(&log), vec!["A", "B", "C", "D"]);

    // Changing A's property touches A and its transitive dependents,
    // but leaves the unrelated D untouched.
    doc.set_property("A", "Pad", Property::Float(1.0))
        .expect("set property");
    assert_eq!(
        doc.get_by_name("A").unwrap().status(),
        &ObjectStatus::Touched
    );
    assert_eq!(
        doc.get_by_name("B").unwrap().status(),
        &ObjectStatus::Touched
    );
    assert_eq!(
        doc.get_by_name("C").unwrap().status(),
        &ObjectStatus::Touched
    );
    assert_eq!(doc.get_by_name("D").unwrap().status(), &ObjectStatus::Valid);

    // The next recompute executes exactly the touched chain,
    // dependencies first, and skips D.
    let executed = doc.recompute_all().expect("cascade recompute");
    assert_eq!(names_of(&doc, &executed), vec!["A", "B", "C"]);
    assert_eq!(
        logged(&log),
        vec!["A", "B", "C", "D", "A", "B", "C"],
        "D was not executed again"
    );
    assert_eq!(doc.get(d).unwrap().status(), &ObjectStatus::Valid);
}

#[test]
fn recompute_detects_two_object_cycle() {
    let log = log();
    let mut doc = Document::new("Cycle");
    doc.add(Box::new(Recorder::linked("X", "Y", &log)));
    doc.add(Box::new(Recorder::linked("Y", "X", &log)));

    let err = doc.recompute_all().expect_err("cycle must fail");
    match &err {
        RecomputeError::Cyclic { cycle } => {
            // Names the cycle in order, first element repeated at the end.
            assert_eq!(
                cycle,
                &vec!["X".to_string(), "Y".to_string(), "X".to_string()]
            );
        }
        other => panic!("expected Cyclic, got {other:?}"),
    }
    // The Display form spells the cycle out.
    assert_eq!(err.to_string(), "cyclic dependency: X -> Y -> X");
    // Nothing was executed while detecting the cycle.
    assert_eq!(logged(&log), Vec::<String>::new());
}

#[test]
fn recompute_reports_missing_link() {
    let log = log();
    let mut doc = Document::new("Dangling");
    doc.add(Box::new(Recorder::linked("Z", "Ghost", &log)));

    let err = doc.recompute_all().expect_err("dangling link must fail");
    assert_eq!(
        err,
        RecomputeError::MissingLink {
            object: "Z".to_string(),
            target: "Ghost".to_string(),
        }
    );
    assert_eq!(
        err.to_string(),
        "object 'Z' links to missing object 'Ghost'"
    );
    assert_eq!(logged(&log), Vec::<String>::new());
}

#[test]
fn recompute_execute_failure_marks_object_error() {
    let log = log();
    let mut doc = Document::new("Failure");
    let ok = doc.add(Box::new(Recorder::new("Src", &log)));
    let bad = doc.add(Box::new(Failing {
        container: PropertyContainer::new("Boom"),
    }));

    // The healthy object executes first (creation + dependency order);
    // the failing object aborts the run.
    let err = doc.recompute_all().expect_err("failing execute must abort");
    assert_eq!(
        err,
        RecomputeError::ExecuteFailed {
            object: "Boom".to_string(),
            reason: "kaboom".to_string(),
        }
    );
    assert_eq!(logged(&log), vec!["Src".to_string()]);

    // The failing object carries the reason; the healthy one stays Valid.
    assert_eq!(
        doc.get(bad).unwrap().status(),
        &ObjectStatus::Error("kaboom".to_string())
    );
    assert_eq!(doc.get(ok).unwrap().status(), &ObjectStatus::Valid);
    assert_eq!(doc.get(bad).unwrap().status().to_string(), "Error: kaboom");
}

#[test]
fn document_metadata_accessors() {
    let mut doc = Document::new("My Label");
    assert_eq!(doc.label(), "My Label");
    doc.set_label("Renamed");
    assert_eq!(doc.label(), "Renamed");
    assert_eq!(doc.creator(), "");
    doc.set_creator("woc-test");
    assert_eq!(doc.creator(), "woc-test");

    // `created` is stamped at construction as an RFC 3339 UTC second-
    // resolution stamp.
    let created = doc.created().to_string();
    assert!(created.ends_with('Z'), "UTC marker: {created}");
    assert!(created.contains('T'), "date/time separator: {created}");
    assert_eq!(created.len(), 20, "YYYY-MM-DDTHH:MM:SSZ");

    // Summaries list objects in creation order with their types.
    let log = log();
    doc.add(Box::new(Recorder::new("R", &log)));
    let summary = doc.summary();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].name, "R");
    assert_eq!(summary[0].type_name, "Test::Recorder");
    assert_eq!(summary[0].status, ObjectStatus::Touched);
}
