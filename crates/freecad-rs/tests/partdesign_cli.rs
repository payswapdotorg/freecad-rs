//! CLI end-to-end tests for the PartDesign verbs (`pad` / `pocket` /
//! `revolution`): real `.FCStd` files, real `freecad-rs` invocations (the
//! sketch_cli.rs pattern). Covers the golden path (sketch → pad →
//! parametric cascade → save/reload), the sketch-content propagation
//! (`sketch-import` a wider rectangle → the pad's volume scales), the
//! pocket and revolution geometry, and the honest failure paths (open
//! profile → exit 1, errored object on record).

use std::io::Write;
use std::path::{Path, PathBuf};
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

/// Unique scratch dir for one test invocation.
fn scratch(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "freecad-rs-partdesign-{tag}-{}-{}",
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

/// A half-disc of radius 5 (arc on +x, diameter on the y axis) — the
/// sphere revolution profile. Unconstrained (solves trivially).
fn half_disc_fragment(path: &Path) -> String {
    let text = "<Sketch>\n\
         \x20 <Geometry type=\"3\" centerx=\"0\" centery=\"0\" radius=\"5.0\" \
         startangle=\"-1.5707963267948966\" endangle=\"1.5707963267948966\"/>\n\
         \x20 <Geometry type=\"1\" startx=\"0\" starty=\"5\" endx=\"0\" endy=\"-5\"/>\n\
         </Sketch>\n";
    write_fragment(path, text)
}

/// The volume printed on a `pad`/`pocket`/`revolution` status line
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

/// The volume `stats` reports for `object` (`"  Pad (Mesh): volume
/// 2000.000 mm³, area ..."`).
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

#[test]
fn cli_partdesign_golden_path_from_sketch_to_parametric_pad() {
    let dir = scratch("golden");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file, "--label", "PartDesign"]);
    run(&["add", file, "sketch"]);
    let rect20 = rectangle_fragment(&dir.join("rect20.xml"), 0.0, 0.0, 20.0, 10.0);
    run(&["sketch-import", file, "Sketch", &rect20]);

    // Pad the rectangle 10 deep: a 20 x 10 x 10 box, watertight, exact.
    let out = run(&["pad", file, "--sketch", "Sketch", "--length", "10"]);
    assert!(out.contains("recomputed Pad (PartDesign::Pad)"), "{out}");
    assert!(out.contains("added Pad (PartDesign::Pad) -> "), "{out}");
    assert!(
        out.contains("Pad: Valid, volume 2000.000 mm³, 12 triangles"),
        "{out}"
    );

    // The dependency edge is in the graph.
    let dot = dir.join("graph.dot");
    let dot = dot.to_str().unwrap();
    let out = run(&["graph", file, dot]);
    assert!(out.contains("1 edges"), "{out}");
    let dot_text = std::fs::read_to_string(dot).unwrap();
    assert!(dot_text.contains("\"Pad\" -> \"Sketch\""), "{dot_text}");

    // The parametric trigger: set the Pad's Length and the volume
    // follows (25 deep now).
    let out = run(&["set", file, "Pad", "Length", "25"]);
    assert!(out.contains("touched Pad"), "{out}");
    let out = run(&["stats", file]);
    assert!(
        (volume_of_stats(&out, "Pad") - 5000.0).abs() < 1e-6,
        "{out}"
    );

    // Save/reload: the link, the status and the geometry survive.
    let out = run(&["get", file, "Pad", "Profile"]);
    assert!(out.contains("Sketch"), "{out}");
    let out = run(&["list", file]);
    assert!(out.contains("PartDesign::Pad"), "{out}");
    assert!(out.contains("Valid"), "{out}");

    // The pad step is journaled and undoable like every `add`/`set`.
    let out = run(&["undo", file]);
    assert!(out.contains("undid \"set Pad.Length\""), "{out}");
    let out = run(&["stats", file]);
    assert!(
        (volume_of_stats(&out, "Pad") - 2000.0).abs() < 1e-6,
        "{out}"
    );
    let out = run(&["redo", file]);
    assert!(out.contains("redid \"set Pad.Length\""), "{out}");
    let out = run(&["undo", file]); // the set again
    assert!(out.contains("undid \"set Pad.Length\""), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_sketch_content_change_propagates_into_the_pad() {
    let dir = scratch("propagate");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    run(&["add", file, "sketch"]);
    let rect20 = rectangle_fragment(&dir.join("rect20.xml"), 0.0, 0.0, 20.0, 10.0);
    run(&["sketch-import", file, "Sketch", &rect20]);
    run(&["pad", file, "--sketch", "Sketch", "--length", "10"]);

    // Edit the sketch: same rectangle, width 30. The import touches the
    // sketch AND its dependent, the recompute re-solves + re-executes,
    // and the pad's volume scales with the new width.
    let rect30 = rectangle_fragment(&dir.join("rect30.xml"), 0.0, 0.0, 30.0, 10.0);
    let out = run(&["sketch-import", file, "Sketch", &rect30]);
    assert!(
        out.contains("touched Sketch") && out.contains("touched Pad"),
        "the cascade must touch both: {out}"
    );
    assert!(out.contains("recomputed Pad (PartDesign::Pad)"), "{out}");
    let out = run(&["stats", file]);
    assert!(
        (volume_of_stats(&out, "Pad") - 3000.0).abs() < 1e-6,
        "30 x 10 x 10 after the edit: {out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_pocket_cuts_a_notch_and_inverted_misses() {
    let dir = scratch("pocket");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    run(&["add", file, "--name", "Body", "box"]);
    // Real-assembly geometry: the corner-origin box (FreeCAD 1.0
    // convention) is centered under the sketch.
    run(&[
        "set",
        file,
        "Body",
        "Placement",
        "pos=(-5,-5,0);axis=(0,0,1);angle=0",
    ]);
    run(&["add", file, "--name", "Hole", "sketch"]);
    let square = rectangle_fragment(&dir.join("square.xml"), -2.0, -2.0, 4.0, 4.0);
    run(&["sketch-import", file, "Hole", &square]);

    // 10 x 10 x 10 box (now centered in XY) minus a 4 x 4 x 4 notch.
    let out = run(&[
        "pocket", file, "--sketch", "Hole", "--base", "Body", "--length", "4",
    ]);
    assert!(
        out.contains("added Pocket (PartDesign::Pocket) -> "),
        "{out}"
    );
    assert!(out.contains("Pocket: Valid, volume 936.000 mm³"), "{out}");

    // The inverted tool extrudes below z = 0 where the box is not.
    let out = run(&[
        "pocket",
        file,
        "--sketch",
        "Hole",
        "--base",
        "Body",
        "--length",
        "4",
        "--inverted",
        "--name",
        "Miss",
    ]);
    assert!(out.contains("Miss: Valid, volume 1000.000 mm³"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_revolution_sweeps_a_half_disc_into_a_sphere() {
    let dir = scratch("revolution");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    run(&["add", file, "--name", "Lathe", "sketch"]);
    let half_disc = half_disc_fragment(&dir.join("half-disc.xml"));
    run(&["sketch-import", file, "Lathe", &half_disc]);

    // A full turn about the z axis (the lathe frame): a sphere of
    // radius 5 within the tessellation tolerance.
    let out = run(&["revolution", file, "--sketch", "Lathe", "--angle", "360"]);
    assert!(
        out.contains("added Revolution (PartDesign::Revolution) -> "),
        "{out}"
    );
    let sphere = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
    let volume = volume_of_status_line(&out);
    assert!(
        (volume - sphere).abs() / sphere < 0.01,
        "sphere volume {volume} vs {sphere}"
    );

    // A half turn sweeps exactly half the volume.
    let out = run(&[
        "revolution",
        file,
        "--sketch",
        "Lathe",
        "--angle",
        "180",
        "--name",
        "HalfRev",
    ]);
    let half = 2.0 / 3.0 * std::f64::consts::PI * 125.0;
    let volume = volume_of_status_line(&out);
    assert!(
        (volume - half).abs() / half < 0.01,
        "half-sphere volume {volume} vs {half}"
    );

    // The sketch's vertical axis sweeps the same full-turn sphere.
    let out = run(&[
        "revolution",
        file,
        "--sketch",
        "Lathe",
        "--axis",
        "y",
        "--name",
        "RevY",
    ]);
    let volume = volume_of_status_line(&out);
    assert!(
        (volume - sphere).abs() / sphere < 0.01,
        "y-axis sphere volume {volume} vs {sphere}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_pad_on_an_open_profile_fails_honestly() {
    let dir = scratch("open-wire");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    run(&["add", file, "sketch"]);
    let open = write_fragment(
        &dir.join("open.xml"),
        "<Sketch>\n  <Geometry type=\"1\" startx=\"0\" starty=\"0\" endx=\"20\" endy=\"0\"/>\n</Sketch>\n",
    );
    run(&["sketch-import", file, "Sketch", &open]);

    // The dangling line solves as a sketch but extracts to an open
    // wire: the pad errors, the command exits 1 — and the document is
    // saved with the failing object on record, FreeCAD-style.
    let output = Command::new(cli())
        .args(["pad", file, "--sketch", "Sketch", "--length", "5"])
        .output()
        .expect("run freecad-rs");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stdout.contains("added Pad (PartDesign::Pad) -> "),
        "the object is on record: {stdout}"
    );
    assert!(
        stdout.contains("Pad: Error:") && stdout.contains("no computed geometry"),
        "honest status line: {stdout}"
    );
    assert!(
        stderr.contains("recompute of 'Pad' failed"),
        "exit error names the feature: {stderr}"
    );
    assert!(stderr.contains("open wire"), "{stderr}");

    // The saved document carries the errored pad.
    let out = run(&["list", file]);
    assert!(out.contains("Pad"), "{out}");
    assert!(out.contains("Error"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_partdesign_verbs_validate_their_targets() {
    let dir = scratch("validation");
    let file = dir.join("demo.fcstd");
    let file = file.to_str().unwrap();

    run(&["new", file]);
    run(&["add", file, "box"]);
    run(&["add", file, "sketch"]);

    // The profile must be a sketch object.
    let stderr = run_expect_failure(&["pad", file, "--sketch", "Box", "--length", "5"]);
    assert!(stderr.contains("not a Sketcher::SketchObject"), "{stderr}");

    // The pocket's base must exist.
    let stderr = run_expect_failure(&[
        "pocket", file, "--sketch", "Sketch", "--base", "Ghost", "--length", "2",
    ]);
    assert!(
        stderr.contains("--base object 'Ghost' not found"),
        "{stderr}"
    );

    // Unknown sketch names are refused before anything is written.
    let stderr = run_expect_failure(&["revolution", file, "--sketch", "Ghost", "--angle", "90"]);
    assert!(stderr.contains("object 'Ghost' not found"), "{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}
