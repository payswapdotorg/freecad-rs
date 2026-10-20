//! CLI end-to-end tests for the `Sketcher::SketchObject` work order:
//! real `.FCStd` files, real `freecad-rs` invocations (the WOE e2e
//! pattern). Covers the golden path (`add sketch` → `sketch-import` →
//! `recompute` → `sketch-info` → `sketch-export` round-trip), the
//! non-sketch-object error, and the undo/redo journal interplay.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// The repo's example sketch fragment (a 40x30 quad plus a circle).
fn quad_fragment() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/sketch-quad.xml"
    );
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read example fragment {path}: {e}"))
}

/// Unique scratch dir for one test invocation.
fn scratch(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "freecad-rs-woi-{tag}-{}-{}",
        std::process::id(),
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

/// Run the CLI, asserting a clean exit-code-1 failure; returns stderr.
fn run_expect_failure(args: &[&str]) -> String {
    let output = Command::new(cli())
        .args(args)
        .output()
        .expect("run freecad-rs");
    assert_eq!(
        output.status.code(),
        Some(1),
        "freecad-rs {args:?} should exit 1: stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Run the CLI; returns (code, stdout, stderr) verbatim.
fn run_raw(args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(cli())
        .args(args)
        .output()
        .expect("run freecad-rs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The example quad as a scratch fragment FILE (the CLI verbs take paths).
fn quad_path(dir: &Path) -> String {
    write_fragment(&dir.join("quad.xml"), &quad_fragment())
}

/// Write `text` to `path` (a scratch file to import from).
fn write_fragment(path: &std::path::Path, text: &str) -> String {
    let file = std::fs::File::create(path).expect("create fragment file");
    let mut file = std::io::BufWriter::new(file);
    file.write_all(text.as_bytes()).expect("write fragment");
    path.to_str().unwrap().to_string()
}

#[test]
fn cli_sketch_golden_path_from_new_to_export() {
    let dir = scratch("golden");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    // new → add sketch (auto-named "Sketch").
    let out = run(&["new", file, "--label", "Sketches"]);
    assert!(out.contains("created"), "{out}");
    let out = run(&["add", file, "sketch"]);
    assert!(
        out.contains("added Sketch (Sketcher::SketchObject)"),
        "{out}"
    );
    // The empty sketch solves trivially during the add recompute.
    assert!(out.contains("recomputed Sketch"), "{out}");

    // sketch-import the repo's example quad (8 constraints).
    let quad = quad_path(&dir);
    let out = run(&["sketch-import", file, "Sketch", &quad]);
    assert!(out.contains("touched Sketch"), "{out}");
    assert!(out.contains("Solved, 8 constraints, dof 7"), "{out}");

    // The explicit recompute step of the golden path re-solves the loaded
    // sketch (solve results are not persisted; the loader touches it).
    let out = run(&["recompute", file]);
    assert!(
        out.contains("recomputed Sketch (Sketcher::SketchObject)"),
        "{out}"
    );
    assert!(out.contains("1 recomputed, 1 total"), "{out}");

    // sketch-info shows the solved state + DOF.
    let out = run(&["sketch-info", file, "Sketch"]);
    assert!(out.contains("5 geometries, 8 constraints, DOF 7"), "{out}");
    assert!(out.contains("4 line(s)"), "{out}");
    assert!(out.contains("1 circle(s)"), "{out}");
    assert!(out.contains("solve: Solved in "), "{out}");

    // sketch-export → re-import the exported fragment → export again:
    // byte-identical (the fixed point of import+solve is stable).
    let export1 = write_fragment(&dir.join("e1.xml"), "");
    let out = run(&["sketch-export", file, "Sketch", &export1]);
    assert!(out.contains("wrote"), "{out}");
    let first = std::fs::read_to_string(&export1).unwrap();
    assert!(first.starts_with("<Sketch>"), "{first}");

    let out = run(&["sketch-import", file, "Sketch", &export1]);
    assert!(out.contains("Solved, 8 constraints, dof 7"), "{out}");
    let export2 = write_fragment(&dir.join("e2.xml"), "");
    run(&["sketch-export", file, "Sketch", &export2]);
    let second = std::fs::read_to_string(&export2).unwrap();
    assert_eq!(
        first, second,
        "export of a re-imported sketch must be identical"
    );

    // And the re-imported state survives a file round-trip untouched.
    let out = run(&["list", file]);
    assert!(out.contains("Sketcher::SketchObject"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_sketch_import_rejects_non_sketch_objects() {
    let dir = scratch("non-sketch");
    let file = dir.join("doc.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    run(&["add", file, "box"]);

    // Not a sketch object: clear error, exit code 1, file untouched.
    let quad = quad_path(&dir);
    let stderr = run_expect_failure(&["sketch-import", file, "Box", &quad]);
    assert!(stderr.contains("not a Sketcher::SketchObject"), "{stderr}");
    assert!(stderr.contains("Part::Box"), "{stderr}");

    // The other sketch verbs refuse it just as clearly.
    let stderr = run_expect_failure(&["sketch-export", file, "Box", "/tmp/never.xml"]);
    assert!(stderr.contains("not a Sketcher::SketchObject"), "{stderr}");
    let stderr = run_expect_failure(&["sketch-info", file, "Box"]);
    assert!(stderr.contains("not a Sketcher::SketchObject"), "{stderr}");

    // Unknown object: clear error too.
    let stderr = run_expect_failure(&["sketch-import", file, "Ghost", &quad]);
    assert!(stderr.contains("object 'Ghost' not found"), "{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_sketch_import_rejects_malformed_fragments_without_touching_the_file() {
    let dir = scratch("malformed");
    let file = dir.join("doc.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    run(&["add", file, "sketch"]);

    let bad = write_fragment(&dir.join("bad.xml"), "<Sketch><Bogus/></Sketch>");
    let stderr = run_expect_failure(&["sketch-import", file, "Sketch", &bad]);
    assert!(stderr.contains("parse error"), "{stderr}");

    // A nonexistent fragment file fails just as cleanly.
    let stderr = run_expect_failure(&["sketch-import", file, "Sketch", "/nonexistent/frag.xml"]);
    assert!(stderr.contains("cannot read"), "{stderr}");

    // The document itself is still healthy and its sketch still empty.
    let out = run(&["sketch-info", file, "Sketch"]);
    assert!(out.contains("0 geometries, 0 constraints"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_add_sketch_undo_removes_it_and_redo_rebuilds_it() {
    let dir = scratch("undo");
    let file = dir.join("doc.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    let out = run(&["add", file, "sketch"]);
    assert!(
        out.contains("added Sketch (Sketcher::SketchObject)"),
        "{out}"
    );

    // Undo the add in a NEW process: the sketch leaves the document.
    let out = run(&["undo", file]);
    assert!(out.contains("undid \"add Sketch\""), "{out}");
    let out = run(&["info", file]);
    assert!(out.contains("objects: 0"), "{out}");

    // Redo: the object is rebuilt from the session journal — including
    // its sketch content (the journal records the `<Sketch>` fragment of
    // added sketch objects).
    let out = run(&["redo", file]);
    assert!(out.contains("redid \"add Sketch\""), "{out}");
    let out = run(&["info", file]);
    assert!(out.contains("objects: 1"), "{out}");
    let out = run(&["sketch-info", file, "Sketch"]);
    assert!(out.contains("Sketcher::SketchObject"), "{out}");
    assert!(out.contains("0 geometries, 0 constraints"), "{out}");
    assert!(out.contains("solve: Solved in"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_sketch_import_touches_and_recomputes_dependents() {
    let dir = scratch("cascade");
    let file = dir.join("dependent.fcstd");
    let file_str = file.to_str().unwrap().to_string();

    // Sketch + a Part::Transform linking to it, built through the library
    // (no CLI verb can attach a transform to a sketch: its recompute
    // honestly fails — a sketch has no computed mesh for v1). The import
    // must still touch the dependent and re-run it.
    {
        let mut doc = fc_app::Document::new("t");
        doc.add(Box::new(fc_sketcher::SketchObject::new("Sketch")));
        let mut transform = fc_part::features::TransformFeature::new("Dep");
        transform
            .container
            .set("Source", fc_app::Property::Link("Sketch".to_string()));
        doc.add(Box::new(transform));
        // The transform's own execute would fail (no computed geometry
        // behind a sketch) — save without recompute, the import does it.
        fc_io::save_document(&doc, &file).expect("save fixture");
    }

    let quad = quad_path(&dir);
    let (code, stdout, stderr) = run_raw(&["sketch-import", &file_str, "Sketch", &quad]);
    assert!(
        stdout.contains("touched Sketch") && stdout.contains("touched Dep"),
        "cascade must touch both: {stdout}"
    );
    assert!(stdout.contains("Solved, 8 constraints, dof 7"), "{stdout}");
    // The dependent re-ran and honestly failed (v1: no sketch-to-mesh
    // bridge), so the command exits 1 after still saving the document —
    // exactly the `set` command's recompute-failure semantics.
    assert_eq!(code, Some(1), "stdout: {stdout} stderr: {stderr}");
    assert!(stderr.contains("recompute of 'Dep' failed"), "{stderr}");

    // The file was saved despite the failed recompute: the sketch content
    // is in, the dependent is still there (Error), FreeCAD-style.
    let out = run(&["list", &file_str]);
    assert!(out.contains("Sketcher::SketchObject"), "{out}");
    assert!(out.contains("Part::Transform"), "{out}");
    let out = run(&["sketch-info", &file_str, "Sketch"]);
    assert!(out.contains("5 geometries, 8 constraints, DOF 7"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}
