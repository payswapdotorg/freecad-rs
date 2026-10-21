//! GPU-gated render tests — run only when `FC_VIEWER_GPU=1` (the CI
//! viewer job sets it with a software adapter, lavapipe). Locally they
//! skip with a clear message: the sandbox/CI split is honest.

mod common;

use std::path::PathBuf;

use fc_viewer::render_png;
use fc_viewer::scene::build_scene;

fn gpu() -> bool {
    std::env::var("FC_VIEWER_GPU").ok().as_deref() == Some("1")
}

fn demo() -> String {
    common::demo_path().to_string_lossy().into_owned()
}

/// Decode a PNG into (width, height, rgba).
fn decode_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(path).expect("png opens"),
    ));
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("output size known")];
    let info = reader.next_frame(&mut buf).expect("png pixels");
    (info.width, info.height, buf)
}

#[test]
fn golden_iso_frame_matches() {
    if !gpu() {
        eprintln!("skipped: FC_VIEWER_GPU not set (CI runs this on lavapipe)");
        return;
    }
    let out = std::env::temp_dir().join("fc-viewer-golden-test.png");
    let triangles =
        render_png(&demo(), out.to_str().unwrap(), "iso", 1280, 800, false).expect("demo renders");

    // Acceptance 1: the rendered triangle count is the scene's count.
    let scene = build_scene(&demo()).expect("demo loads");
    assert_eq!(triangles, scene.triangle_count());

    // Acceptance 2: golden-image comparison, per-channel tolerance.
    // Missing golden is a FAILURE (strict, no silent skip) — the
    // sanctioned regeneration path is the CI viewer job's bless
    // artifact, committed by the TL. Goldens are never hand-edited.
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/demo-iso.png");
    assert!(
        golden.exists(),
        "golden image missing: {} — regenerate via the CI viewer job's bless artifact \
         and commit it (see docs/cli.md, `render --bless`)",
        golden.display()
    );

    let (w, h, rendered) = decode_png(&out);
    let (gw, gh, gold) = decode_png(&golden);
    assert_eq!((w, h), (gw, gh), "golden size drift");

    const CHANNEL_TOLERANCE: i32 = 8;
    let mut off: u64 = 0;
    for (a, b) in rendered.chunks_exact(4).zip(gold.chunks_exact(4)) {
        let delta = (a[0] as i32 - b[0] as i32)
            .abs()
            .max((a[1] as i32 - b[1] as i32).abs());
        let delta = delta.max((a[2] as i32 - b[2] as i32).abs());
        if delta > CHANNEL_TOLERANCE {
            off += 1;
        }
    }
    let pixels = u64::from(w * h);
    let fraction = off as f64 / pixels as f64;
    assert!(
        fraction <= 0.005,
        "golden mismatch: {off}/{pixels} pixels ({fraction:.4}) exceed ±{CHANNEL_TOLERANCE}/channel"
    );
}

#[test]
fn error_doc_renders_without_crashing_and_shows_the_flag() {
    if !gpu() {
        eprintln!("skipped: FC_VIEWER_GPU not set (CI runs this on lavapipe)");
        return;
    }
    // Acceptance 4: a document containing a failed recompute opens and
    // renders; the failing object is visibly flagged.
    let (path, _) = common::error_doc();
    let out = std::env::temp_dir().join("fc-viewer-error-test.png");
    let triangles = render_png(
        path.to_str().unwrap(),
        out.to_str().unwrap(),
        "iso",
        320,
        240,
        false,
    )
    .expect("error doc renders");
    assert!(triangles > 0, "geometry rendered despite the failure");

    let (_w, _h, rgba) = decode_png(&out);
    // The error tint is a saturated red: after the headlight shading
    // (>= 0.35 floor) its red channel stays far above its green/blue.
    let red: usize = rgba
        .chunks_exact(4)
        .filter(|p| p[0] > 100 && p[0] as i32 - p[1] as i32 > 50 && p[0] as i32 - p[2] as i32 > 50)
        .count();
    assert!(
        red > 200,
        "the flagged object must be visibly red-tinted (found {red} red pixels in 320x240)"
    );
}
