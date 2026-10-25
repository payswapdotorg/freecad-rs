//! CLI end-to-end acceptance test for the **mechanical design workflow**
//! milestone (recommendation #5/#6 of the external review): feature
//! patterns — `PartDesign::LinearPattern` + `PartDesign::PolarPattern` —
//! composed into a real mechanical part, the bracket plate.
//!
//! The acceptance criterion is the tutorial part a mechanical designer
//! actually builds: a 120×80×10 plate, four Ø6 corner holes produced by
//! **nested** linear patterns (a 2×2 grid from one pocket — FreeCAD's
//! multiplicative `Transformed` composition), and a Ø80 bolt circle of
//! six Ø4 holes via a polar pattern. Every intermediate volume is
//! asserted against tool volumes measured through the engine itself
//! (the same tessellated profile, padded in a parallel document), so
//! the arithmetic is exact, not `πr²h` approximations.
//!
//! Written BEFORE the implementation (the review's
//! acceptance-criteria-first process): the verbs `pattern-linear` /
//! `pattern-polar` must make this file pass.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Path of the freshly built `freecad-rs` binary (workspace target dir).
fn cli() -> &'static str {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug/freecad-rs");
    assert!(
        std::path::Path::new(path).exists(),
        "freecad-rs binary not found at {path} — run `cargo build --workspace` first"
    );
    path
}

/// Unique scratch dir for one test invocation.
fn scratch(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "freecad-rs-mechanical-{tag}-{}-{}",
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

/// Write `text` to `path` (a scratch fragment FILE to import from).
fn write_fragment(path: &Path, text: &str) -> String {
    let file = std::fs::File::create(path).expect("create fragment file");
    let mut file = std::io::BufWriter::new(file);
    file.write_all(text.as_bytes()).expect("write fragment");
    path.to_str().unwrap().to_string()
}

/// A fully-constrained `w x h` rectangle fragment at `(x0, y0)`:
/// coincident corners, H/V on all sides, DistanceX width, DistanceY
/// height (10 constraints — DOF 2, the translation invariance).
fn rectangle_fragment(path: &Path, x0: f64, y0: f64, w: f64, h: f64) -> String {
    let (x1, y1) = (x0 + w, y0 + h);
    let text = format!(
        "<Sketch>\n  \
         <Geometry type=\"1\" startx=\"{x0}\" starty=\"{y0}\" endx=\"{x1}\" endy=\"{y0}\"/>\n  \
         <Geometry type=\"1\" startx=\"{x1}\" starty=\"{y0}\" endx=\"{x1}\" endy=\"{y1}\"/>\n  \
         <Geometry type=\"1\" startx=\"{x1}\" starty=\"{y1}\" endx=\"{x0}\" endy=\"{y1}\"/>\n  \
         <Geometry type=\"1\" startx=\"{x0}\" starty=\"{y1}\" endx=\"{x0}\" endy=\"{y0}\"/>\n  \
         <Constraint type=\"0\" first=\"0\" firstpos=\"2\" second=\"1\" secondpos=\"1\"/>\n  \
         <Constraint type=\"0\" first=\"1\" firstpos=\"2\" second=\"2\" secondpos=\"1\"/>\n  \
         <Constraint type=\"0\" first=\"2\" firstpos=\"2\" second=\"3\" secondpos=\"1\"/>\n  \
         <Constraint type=\"0\" first=\"3\" firstpos=\"2\" second=\"0\" secondpos=\"1\"/>\n  \
         <Constraint type=\"1\" first=\"0\"/>\n  \
         <Constraint type=\"2\" first=\"1\"/>\n  \
         <Constraint type=\"1\" first=\"2\"/>\n  \
         <Constraint type=\"2\" first=\"3\"/>\n  \
         <Constraint type=\"6\" first=\"0\" firstpos=\"1\" second=\"0\" secondpos=\"2\" value=\"{w}\"/>\n  \
         <Constraint type=\"7\" first=\"1\" firstpos=\"1\" second=\"1\" secondpos=\"2\" value=\"{h}\"/>\n\
         </Sketch>\n"
    );
    write_fragment(path, &text)
}

/// An unconstrained circle of radius `r` at `(cx, cy)` — the pocket tool
/// profiles for the corner holes and the bolt circle.
fn circle_fragment(path: &Path, cx: f64, cy: f64, r: f64) -> String {
    let text = format!(
        "<Sketch>\n  \
         <Geometry type=\"2\" centerx=\"{cx}\" centery=\"{cy}\" radius=\"{r}\"/>\n\
         </Sketch>\n"
    );
    write_fragment(path, &text)
}

/// The volume printed on a feature status line
/// (`"  Pad: Valid, volume 2000.000 mm³, 12 triangles"`).
fn volume_of_status_line(stdout: &str) -> f64 {
    let line = stdout
        .lines()
        .find(|l| l.contains("volume "))
        .unwrap_or_else(|| panic!("no volume line in: {stdout}"));
    let after = line
        .split("volume ")
        .nth(1)
        .and_then(|rest| rest.split(" mm³").next())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .unwrap_or_else(|| panic!("cannot parse volume from: {line}"));
    after
}

/// The volume `stats` reports for `object`.
fn volume_of_stats(stats_output: &str, object: &str) -> f64 {
    let line = stats_output
        .lines()
        .find(|l| l.starts_with(&format!("  {object} ")) && l.contains("volume "))
        .unwrap_or_else(|| panic!("no stats line for {object} in: {stats_output}"));
    let after = line
        .split("volume ")
        .nth(1)
        .and_then(|rest| rest.split(" mm³").next())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .unwrap_or_else(|| panic!("cannot parse volume from: {line}"));
    after
}

/// Assert `got == want` within the tight tolerance the deterministic
/// tessellation allows (identical inputs produce identical meshes, so
/// this is near-exact; 1e-6 relative guards float printing).
fn assert_volume(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() < 1e-6 * want.abs().max(1.0),
        "{what}: got {got}, want {want}"
    );
}

/// Pad the same circle profile in a parallel document and read the
/// volume — the engine's own measurement of the tessellated tool's
/// volume (identical geometry + deflection ⇒ identical mesh).
fn tool_volume(dir: &Path, tag: &str, fragment: &str) -> f64 {
    let file = dir.join(format!("tool-{tag}.fcstd"));
    let file = file.to_str().unwrap().to_string();
    run(&["new", &file]);
    run(&["add", &file, "sketch"]);
    run(&["sketch-import", &file, "Sketch", fragment]);
    let out = run(&["pad", &file, "--sketch", "Sketch", "--length", "10"]);
    volume_of_status_line(&out)
}

/// The bracket plate, end to end: plate → pocket → nested linear
/// patterns (2×2 corner holes) → pocket → polar pattern (bolt circle).
/// Every volume is the exact arithmetic of measured tool volumes.
#[test]
fn cli_bracket_plate_full_mechanical_workflow() {
    let dir = scratch("bracket");
    let file = dir.join("bracket.fcstd");
    let file = file.to_str().unwrap().to_string();

    // Tool volumes, measured through the engine itself.
    let corner = circle_fragment(&dir.join("corner.xml"), -50.0, 30.0, 3.0);
    let bolt = circle_fragment(&dir.join("bolt.xml"), 40.0, 0.0, 2.0);
    let v_corner = tool_volume(&dir, "corner", &corner);
    let v_bolt = tool_volume(&dir, "bolt", &bolt);

    // The plate: a centered 120 x 80 rectangle, padded 10 deep. (The
    // polar pattern revolves about the sketch origin, so the plate is
    // centered there — exactly how a designer builds a round-flange
    // part.)
    run(&["new", &file, "--label", "Bracket"]);
    run(&["add", &file, "sketch"]);
    let plate = rectangle_fragment(&dir.join("plate.xml"), -60.0, -40.0, 120.0, 80.0);
    run(&["sketch-import", &file, "Sketch", &plate]);
    let out = run(&[
        "pad", &file, "--sketch", "Sketch", "--length", "10", "--name", "Plate",
    ]);
    assert_volume(volume_of_status_line(&out), 96000.0, "plate volume");

    // One corner hole: circle r=3 at (-50, 30), pocketed through.
    run(&["add", &file, "sketch"]);
    run(&["sketch-import", &file, "Sketch001", &corner]);
    let out = run(&[
        "pocket",
        &file,
        "--sketch",
        "Sketch001",
        "--base",
        "Plate",
        "--length",
        "10",
        "--name",
        "Hole",
    ]);
    assert_volume(volume_of_status_line(&out), 96000.0 - v_corner, "one hole");

    // Pattern the pocket down the Y edge: occurrences 2, span 60,
    // reversed (the hole sits at +30; the copy lands at −30).
    let out = run(&[
        "pattern-linear",
        &file,
        "--originals",
        "Hole",
        "--base",
        "Plate",
        "--direction",
        "y",
        "--length",
        "60",
        "--occurrences",
        "2",
        "--reversed",
        "--name",
        "HolesY",
    ]);
    assert!(
        out.contains("added HolesY (PartDesign::LinearPattern)"),
        "{out}"
    );
    assert_volume(
        volume_of_status_line(&out),
        96000.0 - 2.0 * v_corner,
        "two holes (linear)",
    );

    // Nested pattern: pattern THE PATTERN across X — FreeCAD's
    // multiplicative Transformed composition. Originals = the Y
    // pattern, base = the plate (the state before the patterned
    // feature was applied): copies of both Y holes across X, span 100,
    // occurrences 2 ⇒ the full 2x2 corner grid.
    let out = run(&[
        "pattern-linear",
        &file,
        "--originals",
        "HolesY",
        "--base",
        "Plate",
        "--direction",
        "x",
        "--length",
        "100",
        "--occurrences",
        "2",
        "--name",
        "HolesGrid",
    ]);
    assert!(
        out.contains("added HolesGrid (PartDesign::LinearPattern)"),
        "{out}"
    );
    assert_volume(
        volume_of_status_line(&out),
        96000.0 - 4.0 * v_corner,
        "2x2 grid (nested linear)",
    );

    // The bolt circle: one Ø4 hole at (40, 0), pocketed from the grid.
    run(&["add", &file, "sketch"]);
    run(&["sketch-import", &file, "Sketch002", &bolt]);
    let out = run(&[
        "pocket",
        &file,
        "--sketch",
        "Sketch002",
        "--base",
        "HolesGrid",
        "--length",
        "10",
        "--name",
        "BoltHole",
    ]);
    assert_volume(
        volume_of_status_line(&out),
        96000.0 - 4.0 * v_corner - v_bolt,
        "grid + one bolt hole",
    );

    // Pattern it around Z: 360°, 6 occurrences — six holes on the Ø80
    // circle (FreeCAD's 360° rule spaces them 360/N so the last copy
    // does not stack on the first).
    let out = run(&[
        "pattern-polar",
        &file,
        "--originals",
        "BoltHole",
        "--base",
        "HolesGrid",
        "--axis",
        "z",
        "--angle",
        "360",
        "--occurrences",
        "6",
        "--name",
        "BoltCircle",
    ]);
    assert!(
        out.contains("added BoltCircle (PartDesign::PolarPattern)"),
        "{out}"
    );
    assert_volume(
        volume_of_status_line(&out),
        96000.0 - 4.0 * v_corner - 6.0 * v_bolt,
        "bracket plate (final)",
    );

    // The whole part is watertight and on record after save/reload.
    let out = run(&["list", &file]);
    assert!(out.contains("PartDesign::LinearPattern"), "{out}");
    assert!(out.contains("PartDesign::PolarPattern"), "{out}");
    let out = run(&["stats", &file]);
    assert_volume(
        volume_of_stats(&out, "BoltCircle"),
        96000.0 - 4.0 * v_corner - 6.0 * v_bolt,
        "reloaded bracket plate",
    );

    // The dependency graph carries the pattern edges: the nested grid
    // depends on the Y pattern, the polar on its pocket.
    let dot = dir.join("graph.dot");
    let dot = dot.to_str().unwrap().to_string();
    run(&["graph", &file, &dot]);
    let dot_text = std::fs::read_to_string(&dot).unwrap();
    assert!(
        dot_text.contains("\"HolesGrid\" -> \"HolesY\""),
        "{dot_text}"
    );
    assert!(dot_text.contains("\"HolesY\" -> \"Hole\""), "{dot_text}");
    assert!(
        dot_text.contains("\"BoltCircle\" -> \"BoltHole\""),
        "{dot_text}"
    );

    // The parametric cascade: deepen nothing — shrink the corner hole
    // depth to 6 (a milled pocket, not a through hole) and every
    // dependent pattern recomputes: the corner holes' cut volume
    // scales to 6/10 of the tool.
    let out = run(&["set", &file, "Hole", "Length", "6"]);
    assert!(out.contains("touched Hole"), "{out}");
    let out = run(&["stats", &file]);
    assert_volume(
        volume_of_stats(&out, "HolesGrid"),
        96000.0 - 4.0 * v_corner * 0.6,
        "grid after parametric edit",
    );
    // The bolt circle is untouched (it depends on BoltHole, not Hole).
    assert_volume(
        volume_of_stats(&out, "BoltCircle"),
        96000.0 - 4.0 * v_corner * 0.6 - 6.0 * v_bolt,
        "final part after parametric edit",
    );

    // Undo restores the through hole — the pattern edit is journaled
    // like every set.
    let out = run(&["undo", &file]);
    assert!(out.contains("undid \"set Hole.Length\""), "{out}");
    let out = run(&["stats", &file]);
    assert_volume(
        volume_of_stats(&out, "BoltCircle"),
        96000.0 - 4.0 * v_corner - 6.0 * v_bolt,
        "final part after undo",
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Honest failures: the pattern verbs validate their inputs and exit 1
/// with the feature named in the error (FreeCAD's "Occurrences must be
/// greater than 1", zero-length spans, unknown originals/bases).
#[test]
fn cli_pattern_validation_failures() {
    let dir = scratch("reject");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap().to_string();

    run(&["new", &file]);
    run(&["add", &file, "sketch"]);
    let rect = rectangle_fragment(&dir.join("rect.xml"), 0.0, 0.0, 20.0, 10.0);
    run(&["sketch-import", &file, "Sketch", &rect]);
    run(&["pad", &file, "--sketch", "Sketch", "--length", "10"]);
    run(&["add", &file, "sketch"]);
    let hole = circle_fragment(&dir.join("hole.xml"), 10.0, 5.0, 2.0);
    run(&["sketch-import", &file, "Sketch001", &hole]);
    run(&[
        "pocket",
        &file,
        "--sketch",
        "Sketch001",
        "--base",
        "Pad",
        "--length",
        "10",
        "--name",
        "Hole",
    ]);

    // Occurrences < 2 is not a pattern.
    let err = run_expect_failure(&[
        "pattern-linear",
        &file,
        "--originals",
        "Hole",
        "--base",
        "Pad",
        "--direction",
        "x",
        "--length",
        "10",
        "--occurrences",
        "1",
    ]);
    assert!(err.contains("Occurrences"), "{err}");

    // A zero-length span stacks every copy — FreeCAD rejects it.
    let err = run_expect_failure(&[
        "pattern-linear",
        &file,
        "--originals",
        "Hole",
        "--base",
        "Pad",
        "--direction",
        "x",
        "--length",
        "0",
        "--occurrences",
        "2",
    ]);
    assert!(err.contains("Length"), "{err}");

    // A zero angle is no revolution at all.
    let err = run_expect_failure(&[
        "pattern-polar",
        &file,
        "--originals",
        "Hole",
        "--base",
        "Pad",
        "--axis",
        "z",
        "--angle",
        "0",
        "--occurrences",
        "3",
    ]);
    assert!(err.contains("Angle"), "{err}");

    // Unknown originals / base are named honestly.
    let err = run_expect_failure(&[
        "pattern-linear",
        &file,
        "--originals",
        "Nope",
        "--base",
        "Pad",
        "--direction",
        "x",
        "--length",
        "10",
        "--occurrences",
        "2",
    ]);
    assert!(err.contains("Nope"), "{err}");
    let err = run_expect_failure(&[
        "pattern-polar",
        &file,
        "--originals",
        "Hole",
        "--base",
        "Nope",
        "--axis",
        "z",
        "--angle",
        "360",
        "--occurrences",
        "3",
    ]);
    assert!(err.contains("Nope"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}
