//! Integration tests for `fc-app` transactions: the FreeCAD
//! `App::Transaction` undo/redo architecture — change records
//! (additions, removals, property and label changes), AutoTransaction
//! semantics (bracketed steps and auto-committed temp steps), the
//! undo/redo stacks with their 100-entry cap, and the journal-restore
//! APIs the CLI's `undo`/`redo`/`history` commands replay through.
//!
//! Test doubles are defined here (fc-app deliberately ships no concrete
//! object types — features live in fc-part, which depends on this
//! crate). The last two tests drive the real `freecad-rs` binary
//! end-to-end on real `.FCStd` files.

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fc_app::{
    impl_document_object, Document, JournalChange, ObjectStatus, Property, PropertyContainer,
    AUTO_TRANSACTION_LABEL, UNDO_STACK_CAP,
};

// ---------------------------------------------------------------------------
// Test objects and helpers
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
        let mut object = Self::new(name, log);
        object
            .container
            .set("Source", Property::Link(target.to_string()));
        object
    }

    /// A recorder with a `Value` float property.
    fn valued(name: &str, value: f64, log: &Arc<Mutex<Vec<String>>>) -> Self {
        let mut object = Self::new(name, log);
        object.container.set("Value", Property::Float(value));
        object
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

/// Fresh shared execution log.
fn log() -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(Vec::new()))
}

/// Snapshot of a log's contents.
fn logged(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().expect("log mutex").clone()
}

/// A recorder's property, cloned out of the document.
fn value_of(doc: &Document, name: &str, key: &str) -> Property {
    doc.get_by_name(name)
        .unwrap_or_else(|| panic!("object '{name}' missing"))
        .properties()
        .get(key)
        .unwrap_or_else(|| panic!("property '{key}' of '{name}' missing"))
        .clone()
}

/// Object names in creation order.
fn names(doc: &Document) -> Vec<String> {
    doc.summary().into_iter().map(|s| s.name).collect()
}

/// Assert an object's status.
fn assert_status(doc: &Document, name: &str, expected: ObjectStatus) {
    let status = doc
        .get_by_name(name)
        .unwrap_or_else(|| panic!("object '{name}' missing"))
        .status()
        .clone();
    assert_eq!(status, expected, "status of '{name}'");
}

// ---------------------------------------------------------------------------
// Documented constants
// ---------------------------------------------------------------------------

#[test]
fn documented_constants() {
    assert_eq!(UNDO_STACK_CAP, 100);
    assert_eq!(AUTO_TRANSACTION_LABEL, "temp");
}

// ---------------------------------------------------------------------------
// Add / undo / redo (mandatory: add -> undo -> redo)
// ---------------------------------------------------------------------------

#[test]
fn add_undo_redo_round_trip_restores_object_and_properties() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 5.0, &log)));
    doc.set_property("A", "Value", Property::Float(7.0))
        .unwrap();
    assert_eq!(doc.undo_count(), 2); // the add + the set

    // Undo the set, then the add.
    doc.undo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(5.0));
    doc.undo().unwrap().unwrap();
    assert!(doc.get_by_name("A").is_none());
    assert_eq!(doc.object_count(), 0);

    // Redo the add: the parked object returns with its properties.
    let info = doc.redo().unwrap().unwrap();
    assert_eq!(info.changes, 1);
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(5.0));

    // Redo the set.
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(7.0));
}

// ---------------------------------------------------------------------------
// Bracketed transactions (AutoTransaction)
// ---------------------------------------------------------------------------

#[test]
fn bracketed_transaction_groups_changes_into_one_undo() {
    let log = log();
    let mut doc = Document::new("test");
    doc.open_transaction(Some("create and tune"));
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.set_property("A", "Value", Property::Float(2.0))
        .unwrap();
    doc.set_property("A", "Bonus", Property::Bool(true))
        .unwrap();
    doc.close_transaction();

    // One undoable step holding three change records.
    assert_eq!(doc.undo_count(), 1);
    let entries = doc.undo_history();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].label.as_deref(), Some("create and tune"));
    assert_eq!(entries[0].changes, 3);

    // Undo: the whole step rolls back (object gone).
    doc.undo().unwrap().unwrap();
    assert_eq!(doc.object_count(), 0);

    // Redo: the object returns with both properties set.
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(2.0));
    assert_eq!(value_of(&doc, "A", "Bonus"), Property::Bool(true));
}

#[test]
fn unbracketed_sets_undo_individually_in_reverse_order() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));

    // Three unbracketed sets: three auto-committed temp steps.
    for value in [2.0, 3.0, 4.0] {
        doc.set_property("A", "Value", Property::Float(value))
            .unwrap();
    }
    assert_eq!(doc.undo_count(), 4); // add + three sets
    let entries = doc.undo_history();
    for entry in &entries[1..] {
        assert_eq!(entry.label.as_deref(), Some(AUTO_TRANSACTION_LABEL));
    }

    // Each undo restores the previous value, newest change first.
    doc.undo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(3.0));
    doc.undo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(2.0));
    doc.undo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(1.0));

    // The add itself is still undoable; then the stack is empty.
    doc.undo().unwrap().unwrap();
    assert_eq!(doc.object_count(), 0);
    assert_eq!(doc.undo().unwrap(), None, "nothing left to undo");

    // Redo walks it all forward again: add, then the sets in order.
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(1.0));
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(2.0));
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(3.0));
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(4.0));
}

// ---------------------------------------------------------------------------
// Remove / undo (mandatory: remove -> undo -> properties intact)
// ---------------------------------------------------------------------------

#[test]
fn remove_undo_redo_restores_properties_and_creation_order() {
    let log = log();
    let mut doc = Document::new("test");
    for name in ["A", "B", "C"] {
        doc.add(Box::new(Recorder::valued(name, 1.0, &log)));
    }
    doc.set_property("B", "Value", Property::Float(9.0))
        .unwrap();
    doc.remove("B").unwrap();
    assert_eq!(doc.object_count(), 2);
    assert_eq!(names(&doc), ["A", "C"]);

    // Undo the removal: the parked object returns with its properties,
    // at its original creation-order position.
    doc.undo().unwrap().unwrap();
    assert_eq!(doc.object_count(), 3);
    assert_eq!(names(&doc), ["A", "B", "C"]);
    assert_eq!(value_of(&doc, "B", "Value"), Property::Float(9.0));

    // Redo the removal.
    doc.redo().unwrap().unwrap();
    assert_eq!(doc.object_count(), 2);
    assert_eq!(names(&doc), ["A", "C"]);
}

// ---------------------------------------------------------------------------
// Redo invalidation (mandatory: new step clears the redo stack)
// ---------------------------------------------------------------------------

#[test]
fn new_commit_clears_redo_stack() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.set_property("A", "Value", Property::Float(2.0))
        .unwrap();
    doc.set_property("A", "Value", Property::Float(3.0))
        .unwrap();
    doc.undo().unwrap().unwrap();
    assert_eq!(doc.redo_count(), 1);

    // A new committed step invalidates the undone future.
    doc.set_property("A", "Value", Property::Float(4.0))
        .unwrap();
    assert_eq!(doc.redo_count(), 0);
    assert_eq!(doc.undo_count(), 3); // add, set=2, set=4
    assert_eq!(doc.redo().unwrap(), None, "the future is gone");
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(4.0));
}

// ---------------------------------------------------------------------------
// Labels and history (mandatory: explicit label appears in history)
// ---------------------------------------------------------------------------

#[test]
fn explicit_labels_appear_in_history_and_undo_info() {
    let log = log();
    let mut doc = Document::new("test");
    doc.open_transaction(Some("create tools"));
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.set_property("A", "Value", Property::Float(2.0))
        .unwrap();
    doc.close_transaction();

    let entries = doc.undo_history();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].label.as_deref(), Some("create tools"));

    let info = doc.undo().unwrap().unwrap();
    assert_eq!(info.label.as_deref(), Some("create tools"));
}

#[test]
fn label_changes_undo_and_redo() {
    let mut doc = Document::new("original");
    assert_eq!(doc.undo_count(), 0, "the initial label is not a step");
    doc.set_label("renamed");
    assert_eq!(doc.label(), "renamed");
    assert_eq!(doc.undo_count(), 1);

    doc.undo().unwrap().unwrap();
    assert_eq!(doc.label(), "original");
    doc.redo().unwrap().unwrap();
    assert_eq!(doc.label(), "renamed");
}

// ---------------------------------------------------------------------------
// Stack cap (mandatory: cap at 100, oldest dropped)
// ---------------------------------------------------------------------------

#[test]
fn undo_stack_caps_at_100_oldest_dropped() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 0.0, &log)));
    for step in 1..=105i64 {
        doc.set_property("A", "Value", Property::Float(step as f64))
            .unwrap();
    }
    assert_eq!(doc.undo_count(), 100);
    assert_eq!(doc.redo_count(), 0);

    // The 5 oldest steps were dropped: undoing everything lands on the
    // value set by the 5th step, not the original 0.
    for _ in 0..100 {
        doc.undo().unwrap().unwrap();
    }
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(5.0));
    assert_eq!(doc.undo().unwrap(), None, "the oldest steps are gone");
    assert_eq!(doc.redo_count(), 100);

    // The capped stack still redoes fully.
    for _ in 0..100 {
        doc.redo().unwrap().unwrap();
    }
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(105.0));
}

// ---------------------------------------------------------------------------
// Recompute integration (mandatory: undo touches dependents -> recompute)
// ---------------------------------------------------------------------------

#[test]
fn undo_marks_object_and_dependents_touched_so_recompute_reruns() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("Base", 1.0, &log)));
    doc.add(Box::new(Recorder::linked("Lift", "Base", &log)));
    doc.recompute_all().unwrap();
    assert_eq!(logged(&log), ["Base", "Lift"]);

    // A set cascades: Base + Lift recompute.
    doc.set_property("Base", "Value", Property::Float(2.0))
        .unwrap();
    doc.recompute_all().unwrap();
    assert_eq!(logged(&log).len(), 4);

    // Undo restores the value and marks Base + Lift Touched again...
    doc.undo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "Base", "Value"), Property::Float(1.0));
    assert_status(&doc, "Base", ObjectStatus::Touched);
    assert_status(&doc, "Lift", ObjectStatus::Touched);

    // ...so the next recompute re-evaluates both, dependencies first.
    doc.recompute_all().unwrap();
    assert_eq!(
        logged(&log),
        ["Base", "Lift", "Base", "Lift", "Base", "Lift"]
    );
}

// ---------------------------------------------------------------------------
// Edge semantics
// ---------------------------------------------------------------------------

#[test]
fn empty_transaction_is_discarded_on_close() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.open_transaction(Some("no-op step"));
    doc.close_transaction();
    assert_eq!(doc.undo_count(), 1, "only the add; the empty step is gone");
}

#[test]
fn undo_while_transaction_is_open_is_an_error() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.open_transaction(Some("mid-edit"));
    doc.set_property("A", "Value", Property::Float(2.0))
        .unwrap();
    assert!(doc.undo().is_err(), "cannot undo mid-transaction");
    assert!(doc.redo().is_err(), "cannot redo mid-transaction");
    doc.close_transaction();
    assert!(doc.undo().unwrap().is_some(), "works once closed");
}

#[test]
fn undo_redo_on_empty_stacks_return_none() {
    let mut doc = Document::new("test");
    assert_eq!(doc.undo().unwrap(), None);
    assert_eq!(doc.redo().unwrap(), None);
}

#[test]
fn repeated_sets_collapse_into_one_change_record() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.open_transaction(Some("tweak"));
    doc.set_property("A", "Value", Property::Float(2.0))
        .unwrap();
    doc.set_property("A", "Value", Property::Float(3.0))
        .unwrap();
    doc.close_transaction();

    // Two sets of the same property collapsed to one record.
    assert_eq!(doc.undo_count(), 2); // add + tweak
    let entries = doc.undo_history();
    assert_eq!(entries[1].changes, 1);

    // One undo returns to the original value (the intermediate 2.0 is
    // unobservable).
    doc.undo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(1.0));
}

#[test]
fn set_property_on_missing_key_undoes_to_absent() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.set_property("A", "Fresh", Property::Integer(3))
        .unwrap();
    assert_eq!(value_of(&doc, "A", "Fresh"), Property::Integer(3));

    // The before-snapshot was "absent": undo removes the property again.
    doc.undo().unwrap().unwrap();
    assert!(doc
        .get_by_name("A")
        .unwrap()
        .properties()
        .get("Fresh")
        .is_none());
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Fresh"), Property::Integer(3));
}

// ---------------------------------------------------------------------------
// Journal restore (the CLI's rehydration path)
// ---------------------------------------------------------------------------

#[test]
fn restore_undo_transaction_rebuilds_stack_without_applying() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.clear_history(); // like the CLI right after loading a file
    assert_eq!(doc.undo_count(), 0);

    doc.restore_undo_transaction(
        Some("add A"),
        vec![JournalChange::Added {
            object: "A".to_string(),
        }],
    )
    .unwrap();
    assert_eq!(doc.undo_count(), 1);
    assert!(
        doc.get_by_name("A").is_some(),
        "restoring must not apply anything"
    );

    // The real machinery now applies it backward.
    let info = doc.undo().unwrap().unwrap();
    assert_eq!(info.label.as_deref(), Some("add A"));
    assert!(doc.get_by_name("A").is_none());
    doc.redo().unwrap().unwrap();
    assert!(doc.get_by_name("A").is_some());
}

#[test]
fn restore_redo_transactions_requeue_for_chronological_redo() {
    let log = log();
    let mut doc = Document::new("test");
    doc.add(Box::new(Recorder::valued("A", 1.0, &log)));
    doc.set_property("A", "Value", Property::Float(2.0))
        .unwrap();
    doc.clear_history();

    // Undo region: add + set (the document reflects both).
    doc.restore_undo_transaction(
        Some("add A"),
        vec![JournalChange::Added {
            object: "A".to_string(),
        }],
    )
    .unwrap();
    doc.restore_undo_transaction(
        Some("set A.Value = 2"),
        vec![JournalChange::Property {
            object: "A".to_string(),
            key: "Value".to_string(),
            before: Some(Property::Float(1.0)),
            after: Some(Property::Float(2.0)),
        }],
    )
    .unwrap();
    // Redo region, in commit order: two undone sets of the same value.
    doc.restore_redo_transaction(
        Some("set A.Value = 3"),
        vec![JournalChange::Property {
            object: "A".to_string(),
            key: "Value".to_string(),
            before: Some(Property::Float(2.0)),
            after: Some(Property::Float(3.0)),
        }],
    )
    .unwrap();
    doc.restore_redo_transaction(
        Some("set A.Value = 4"),
        vec![JournalChange::Property {
            object: "A".to_string(),
            key: "Value".to_string(),
            before: Some(Property::Float(3.0)),
            after: Some(Property::Float(4.0)),
        }],
    )
    .unwrap();

    assert_eq!(doc.undo_count(), 2);
    assert_eq!(doc.redo_count(), 2);
    assert_eq!(
        doc.redo_history()
            .iter()
            .map(|e| e.label.clone())
            .collect::<Vec<_>>(),
        [
            Some("set A.Value = 3".to_string()),
            Some("set A.Value = 4".to_string()),
        ],
        "redo replays chronologically"
    );

    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(3.0));
    doc.redo().unwrap().unwrap();
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(4.0));

    // And the whole chain unwinds backwards, add last.
    doc.undo().unwrap().unwrap();
    doc.undo().unwrap().unwrap();
    doc.undo().unwrap().unwrap();
    doc.undo().unwrap().unwrap();
    assert_eq!(doc.object_count(), 0);
}

#[test]
fn restore_added_absent_parks_rebuilt_object_for_redo() {
    // The CLI scenario after `add A; set A.Value; undo 2` + restart: the
    // document is empty, the addition lives only in the rebuilt record.
    let log = log();
    let mut doc = Document::new("test");
    let mut rebuilt = Recorder::valued("A", 7.0, &log);
    rebuilt.container.set_name("A");
    doc.restore_redo_transaction(
        Some("add A"),
        vec![JournalChange::AddedAbsent {
            object: Box::new(rebuilt),
        }],
    )
    .unwrap();
    // A property change of the still-absent object resolves through the
    // parked-object registry.
    doc.restore_redo_transaction(
        Some("set A.Value = 9"),
        vec![JournalChange::Property {
            object: "A".to_string(),
            key: "Value".to_string(),
            before: Some(Property::Float(7.0)),
            after: Some(Property::Float(9.0)),
        }],
    )
    .unwrap();
    assert_eq!(doc.object_count(), 0);

    doc.redo().unwrap().unwrap(); // the addition: parked object returns
    assert!(doc.get_by_name("A").is_some());
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(7.0));
    doc.redo().unwrap().unwrap(); // the set
    assert_eq!(value_of(&doc, "A", "Value"), Property::Float(9.0));
}

#[test]
fn restore_rejects_changes_for_unknown_objects() {
    let mut doc = Document::new("test");
    let err = doc
        .restore_undo_transaction(
            Some("ghost step"),
            vec![JournalChange::Added {
                object: "Ghost".to_string(),
            }],
        )
        .unwrap_err();
    assert!(err.contains("Ghost"), "error should name the object: {err}");
}

// ---------------------------------------------------------------------------
// CLI end-to-end on real .FCStd files (mandatory)
// ---------------------------------------------------------------------------

/// Path of the freshly built `freecad-rs` binary (workspace target dir).
/// `cargo test --workspace` builds it alongside the test binaries.
fn cli() -> &'static str {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug/freecad-rs");
    assert!(
        std::path::Path::new(path).exists(),
        "freecad-rs binary not found at {path} — run `cargo build --workspace` first"
    );
    path
}

/// Unique scratch dir for one test invocation.
fn scratch(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "freecad-rs-woe-{tag}-{}",
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run the CLI, asserting success, and return its stdout.
fn run(args: &[&str]) -> String {
    let output = Command::new(cli())
        .args(args)
        .output()
        .expect("run freecad-rs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "freecad-rs {args:?} failed ({}): stdout: {stdout} stderr: {}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

#[test]
fn cli_undo_redo_history_on_real_fcstd() {
    let dir = scratch("cli-round-trip");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    let out = run(&["new", file, "--label", "Demo"]);
    assert!(out.contains("created"), "{out}");

    let out = run(&["add", file, "box", "--length", "10"]);
    assert!(out.contains("added Box"), "{out}");

    let out = run(&["set", file, "Box", "Height", "20"]);
    assert!(out.contains("set Box.Height = 20"), "{out}");

    // Undo the property change in a NEW process: back to the default.
    let out = run(&["undo", file]);
    assert!(out.contains("undid \"set Box.Height\""), "{out}");
    let out = run(&["get", file, "Box", "Height"]);
    assert_eq!(out.trim(), "10", "height back to the default after undo");

    // History: the add is still on the undo stack, the set can be redone.
    let out = run(&["history", file]);
    assert!(out.contains("undo stack (1)"), "{out}");
    assert!(out.contains("add Box"), "{out}");
    assert!(out.contains("redo available: 1"), "{out}");

    // Redo it back, then commit a fresh step: the redo stack clears.
    let out = run(&["redo", file]);
    assert!(out.contains("redid \"set Box.Height\""), "{out}");
    let out = run(&["get", file, "Box", "Height"]);
    assert_eq!(out.trim(), "20", "height redone");

    let out = run(&["set", file, "Box", "Width", "5"]);
    assert!(out.contains("set Box.Width = 5"), "{out}");
    let out = run(&["history", file]);
    assert!(out.contains("redo available: 0"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_undo_add_removes_object_and_redo_rebuilds_it() {
    let dir = scratch("cli-add-undo");
    let file = dir.join("doc.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    let out = run(&["add", file, "box"]);
    assert!(out.contains("added Box"), "{out}");

    // Undo the add in a new process: the object leaves the document on
    // disk.
    let out = run(&["undo", file]);
    assert!(out.contains("undid \"add Box\""), "{out}");
    let out = run(&["info", file]);
    assert!(out.contains("objects: 0"), "{out}");

    // Redo: the object is rebuilt from the session journal.
    let out = run(&["redo", file]);
    assert!(out.contains("redid \"add Box\""), "{out}");
    let out = run(&["info", file]);
    assert!(out.contains("objects: 1"), "{out}");

    let out = run(&["history", file]);
    assert!(out.contains("undo stack (1)"), "{out}");
    assert!(out.contains("add Box"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}
