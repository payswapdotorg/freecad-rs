//! CLI end-to-end tests for `Part::Mirroring` and the `LinkSub` property
//! family: the `mirroring` verb (plane given directly), the `set`/`get`
//! round trip of a `LinkSub` value (`name` / `name[sub]` — the text
//! form `App::PropertyLinkSub` properties travel through the CLI in),
//! and the honest failure paths (a bare non-datum `MirrorPlane` link
//! errors like FreeCAD's "No subelement specified").

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn cli() -> &'static str {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug/freecad-rs");
    assert!(
        std::path::Path::new(path).exists(),
        "freecad-rs binary not found at {path} — run `cargo build --workspace` first"
    );
    path
}

fn scratch(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "freecad-rs-mirroring-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

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

/// The golden path: a box, a mirroring through a directly given plane,
/// the volume preserved, the saved file carrying FreeCAD's own property
/// types (`MirrorPlane` as an empty `LinkSub`, `Base`/`Normal` as
/// `PropertyPosition`/`PropertyDirection`).
#[test]
fn mirroring_verb_reflects_through_the_given_plane() {
    let dir = scratch("golden");
    let file = dir.join("demo.FCStd");

    run(&["new", file.to_str().unwrap()]);
    run(&[
        "add",
        file.to_str().unwrap(),
        "--name",
        "Box",
        "box",
        "--length",
        "20",
        "--width",
        "10",
        "--height",
        "5",
    ]);
    let out = run(&[
        "add",
        file.to_str().unwrap(),
        "--name",
        "Mirrored",
        "mirroring",
        "--source",
        "Box",
        "--base",
        "30,0,0",
        "--normal",
        "1,0,0",
    ]);
    assert!(
        out.contains("recomputed Mirrored (Part::Mirroring)"),
        "the mirroring recomputes: {out}"
    );
    let stats = run(&["stats", file.to_str().unwrap()]);
    assert!(
        stats.contains("1000.000 mm³"),
        "the mirrored volume equals the source's: {stats}"
    );

    // The saved form: FreeCAD's property types on the mirroring.
    let xml = {
        let zip = std::fs::File::open(&file).unwrap();
        let mut zip = zip::ZipArchive::new(zip).unwrap();
        use std::io::Read;
        let mut xml = String::new();
        zip.by_name("Document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        xml
    };
    assert!(
        xml.contains(r#"<Property name="MirrorPlane" type="App::PropertyLinkSub">"#),
        "MirrorPlane saves as FreeCAD's own LinkSub form"
    );
    assert!(
        xml.contains(r#"<LinkSub value="" count="0">"#),
        "the unset form is the empty LinkSub"
    );
    let mirrored_block = xml
        .split("<Object name=\"Mirrored\">")
        .nth(1)
        .and_then(|rest| rest.split("</Object>").next())
        .expect("the Mirrored object block");
    assert!(
        mirrored_block.contains(r#"type="App::PropertyPosition""#),
        "Base is a PropertyPosition: {mirrored_block}"
    );
    assert!(
        mirrored_block.contains(r#"type="App::PropertyDirection""#),
        "Normal is a PropertyDirection: {mirrored_block}"
    );
}

/// `set`/`get` a `LinkSub` property: the text form (`name`,
/// `name[sub]`) round-trips through the CLI exactly.
#[test]
fn link_sub_values_round_trip_through_set_and_get() {
    let dir = scratch("setget");
    let file = dir.join("demo.FCStd");

    run(&["new", file.to_str().unwrap()]);
    run(&["add", file.to_str().unwrap(), "--name", "Box", "box"]);
    run(&[
        "add",
        file.to_str().unwrap(),
        "--name",
        "Mirrored",
        "mirroring",
        "--source",
        "Box",
    ]);

    // A subelement (face) link: parseable, printable, persisted — and it
    // recomputes: the face case rides on the file's persisted
    // Base/Normal, no datum required.
    run(&[
        "set",
        file.to_str().unwrap(),
        "Mirrored",
        "MirrorPlane",
        "Box[Face3]",
    ]);
    let got = run(&["get", file.to_str().unwrap(), "Mirrored", "MirrorPlane"]);
    assert!(
        got.trim() == "Box[Face3]",
        "the LinkSub text form round-trips: {got:?}"
    );
    // And the list shows the mirroring valid, not in error.
    let list = run(&["list", file.to_str().unwrap()]);
    assert!(
        list.contains("Mirrored") && !list.contains("Error"),
        "the face-linked mirroring recomputes cleanly: {list}"
    );
}

/// A bare `MirrorPlane` link to a non-datum is FreeCAD's own failure
/// ("No subelement specified" there): the add — which recomputes on
/// save — reports the object in error and the CLI exits 1.
#[test]
fn bare_non_datum_mirror_plane_is_an_honest_error() {
    let dir = scratch("error");
    let file = dir.join("demo.FCStd");

    run(&["new", file.to_str().unwrap()]);
    run(&["add", file.to_str().unwrap(), "--name", "Box", "box"]);
    let stderr = run_expect_failure(&[
        "add",
        file.to_str().unwrap(),
        "--name",
        "Mirrored",
        "mirroring",
        "--source",
        "Box",
        "--mirror-plane",
        "Box",
    ]);
    assert!(
        stderr.contains("not a datum plane") || stderr.contains("datum planes"),
        "the error names the modeling gap: {stderr}"
    );
}
