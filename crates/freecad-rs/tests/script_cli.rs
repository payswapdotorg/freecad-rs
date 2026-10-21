//! CLI end-to-end tests for the `script` verb: real `.rhai` files, real
//! `freecad-rs` invocations (the partdesign_cli.rs / sketch_cli.rs
//! pattern). Covers the milestone's acceptance gates:
//!
//! 1. **Script-driven build** — the committed example script
//!    (`crates/fc-script/examples/build-house.rhai`) runs against the CLI,
//!    saves a document, and the resulting stats are the exact analytic
//!    numbers (checked through the `stats`/`get` verbs, no tolerance).
//! 2. **Failure semantics** — a script whose recompute fails surfaces the
//!    Error honestly (the CLI's recompute-failure message style) and the
//!    process exits non-zero; rhai parse/runtime errors do the same.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Path of the freshly built `freecad-rs` binary (workspace target dir).
/// `cargo test --workspace` builds it alongside the test binaries.
fn cli() -> &'static str {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug/freecad-rs");
    assert!(
        Path::new(path).exists(),
        "freecad-rs binary not found at {path} — run `cargo build --workspace` first"
    );
    path
}

/// Unique scratch dir for one test invocation.
fn scratch(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "freecad-rs-script-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run the CLI (optionally with a working directory), asserting success,
/// and return its stdout.
fn run_in(dir: Option<&Path>, args: &[&str]) -> String {
    let output = command(dir, args);
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
fn run_expect_failure_in(dir: Option<&Path>, args: &[&str]) -> String {
    let output = command(dir, args);
    assert_eq!(
        output.status.code(),
        Some(1),
        "freecad-rs {args:?} should exit 1: stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn command(dir: Option<&Path>, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(cli());
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd.args(args).output().expect("run freecad-rs")
}

/// Write `text` to `path` and return the path as a string.
fn write_script(path: &Path, text: &str) -> String {
    std::fs::write(path, text).expect("write script");
    path.to_str().expect("utf-8 temp path").to_string()
}

// ---------------------------------------------------------------------------
// Acceptance 1: the committed example script builds a document, and its
// stats are exact.
// ---------------------------------------------------------------------------

#[test]
fn example_script_builds_document_with_exact_stats() {
    let dir = scratch("example");
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/fc-script/examples/build-house.rhai"
    );
    let stdout = run_in(Some(&dir), &["script", script]);

    // The honest transcript: what was added, bound, set, recomputed, saved.
    for line in [
        "new document \"House\" (0 objects)",
        "added Base (Part::Box)",
        "added Chimney (Part::Cylinder)",
        "bound Chimney.Height = 2 * Base.Length",
        "added Ball (Part::Sphere)",
        "added Lift (Part::Transform)",
        "  recomputed Base (Part::Box)",
        "  recomputed Chimney (Part::Cylinder)",
        "  recomputed Ball (Part::Sphere)",
        "  recomputed Lift (Part::Transform)",
        "4 recomputed, 4 total",
        "saved house.FCStd",
        "wrote house.stl (ASCII STL, ",
    ] {
        assert!(
            stdout.contains(line),
            "transcript missing {line:?} in:\n{stdout}"
        );
    }

    // The script's own read-back of the exact analytic stats. The box is
    // integer-exact; the chimney's volume is the kernel's formula
    // PI * r * r * Height with Height bound to 2 * Base.Length = 60.
    let chimney_volume = std::f64::consts::PI * 1.5 * 1.5 * 60.0;
    for line in [
        "Base volume: 3000.0",
        "Base area:   1700.0",
        &format!("Chimney volume (Height = 2 * Base.Length): {chimney_volume}"),
    ] {
        assert!(
            stdout.contains(line),
            "stats missing {line:?} in:\n{stdout}"
        );
    }

    // The saved document exists in the script's working directory and
    // reads back through the regular verbs with the exact numbers.
    let fcstd = dir.join("house.FCStd");
    assert!(fcstd.exists(), "the script must save house.FCStd");
    let file = fcstd.to_str().expect("utf-8 scratch path");
    let stats = run_in(None, &["stats", file]);
    assert!(
        stats.contains("Base (Box): volume 3000.000 mm³, area 1700.000 mm²"),
        "exact stats on the reloaded document:\n{stats}"
    );
    assert!(
        stats.contains("Chimney (Cylinder): volume 424.115 mm³"),
        "bound chimney volume on the reloaded document:\n{stats}"
    );
    let height = run_in(None, &["get", file, "Chimney", "Height"]);
    assert!(
        height.starts_with("60\n") && height.contains("(bound to expression: 2 * Base.Length)"),
        "bound height reads back as 60: {height}"
    );
    let list = run_in(None, &["list", file]);
    assert!(
        list.contains("Base    Part::Box       Valid"),
        "list:\n{list}"
    );
    assert!(
        list.contains("Lift    Part::Transform Valid"),
        "list:\n{list}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Acceptance 2: failure semantics — a failed recompute surfaces the Error
// honestly and exits non-zero.
// ---------------------------------------------------------------------------

#[test]
fn script_with_failed_recompute_exits_nonzero_with_the_error() {
    let dir = scratch("recompute-failure");
    // The honest trigger from the repo's existing failure-mode tests
    // (fc-app recompute tests): a binding whose result carries a unit is
    // rejected by PropertyFloat at recompute time.
    let script = write_script(
        &dir.join("broken.rhai"),
        r#"
let doc = new_document("Broken");
let stem = doc.add_cylinder("Stem", 1.5, 12.0);
doc.bind(stem, "Height", "2 * 5mm");
doc.recompute();
"#,
    );
    let stderr = run_expect_failure_in(Some(&dir), &["script", &script]);
    assert!(
        stderr.contains(
            "error: recompute of 'Stem' failed: expression for 'Height' has unit mm; \
             property is a plain float (FreeCAD PropertyFloat rejects units)"
        ),
        "CLI-style recompute failure on stderr: {stderr}"
    );

    // The transcript is still on record (stdout), ending with the failure.
    let stdout = {
        let output = Command::new(cli())
            .current_dir(&dir)
            .args(["script", &script])
            .output()
            .expect("run freecad-rs");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    for line in [
        "new document \"Broken\" (0 objects)",
        "added Stem (Part::Cylinder)",
        "bound Stem.Height = 2 * 5mm",
        "recompute failed: recompute of 'Stem' failed: expression for 'Height' has unit mm; property is a plain float (FreeCAD PropertyFloat rejects units)",
    ] {
        assert!(stdout.contains(line), "transcript missing {line:?} in:\n{stdout}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn script_with_missing_link_exits_nonzero() {
    let dir = scratch("missing-link");
    // A link to an object that does not exist: the other honest failure
    // mode (MissingLink, from fc-app's recompute).
    let script = write_script(
        &dir.join("ghost.rhai"),
        r#"
let doc = new_document("Ghost");
let ball = doc.add_sphere("Ball", 4.0);
let lift = doc.add_transform("Lift", ball);
doc.set(lift, "Source", "Ghost");
doc.recompute();
"#,
    );
    let stderr = run_expect_failure_in(Some(&dir), &["script", &script]);
    assert!(
        stderr.contains("error: object 'Lift' links to missing object 'Ghost'"),
        "MissingLink surfaced honestly: {stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rhai_errors_exit_nonzero_with_clear_messages() {
    let dir = scratch("rhai-errors");
    // A rhai parse error.
    let script = write_script(&dir.join("syntax.rhai"), "let = ;\n");
    let stderr = run_expect_failure_in(Some(&dir), &["script", &script]);
    assert!(
        stderr.contains("error:") && stderr.contains("Syntax error"),
        "clear parse error: {stderr}"
    );
    // A rhai runtime error (unknown function).
    let script = write_script(&dir.join("runtime.rhai"), "frobnicate();\n");
    let stderr = run_expect_failure_in(Some(&dir), &["script", &script]);
    assert!(
        stderr.contains("error:") && stderr.contains("Function not found"),
        "clear runtime error: {stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_script_file_is_an_honest_error() {
    let dir = scratch("missing-file");
    let stderr = run_expect_failure_in(None, &["script", "no-such-script.rhai"]);
    assert!(
        stderr.contains("error: cannot read 'no-such-script.rhai'"),
        "honest read error: {stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
