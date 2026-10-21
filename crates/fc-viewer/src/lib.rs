//! `fc-viewer` — the M1 interactive-viewer core.
//!
//! Two halves, per `docs/PLAN-APPLICATION.md` (M1):
//!
//! - **Camera math** ([`camera`]) — orbit (target fixed), zoom
//!   (exponential in wheel ticks), pan (parallel to the screen plane)
//!   and the four standard views (`iso`/`top`/`front`/`right`) matching
//!   `export svg`'s cameras — pure functions, unit-tested headlessly.
//! - **Offscreen renderer** ([`renderer`]) — wgpu, surface-less: the
//!   scene assembled on the CPU ([`scene`]: the exports' tessellation
//!   rules, placements, error flags), flat-shaded, read back, written
//!   as PNG. Picking ([`pick`]) is a CPU raycast through the pixel.
//!
//! Interactive orbit (winit event loop) lives behind the non-default
//! `interactive` cargo feature (`examples/viewer.rs`). GPU tests are
//! gated behind `FC_VIEWER_GPU=1`; CI runs them on a software adapter
//! (lavapipe).

pub mod camera;
pub mod pick;
pub mod renderer;
pub mod scene;

use std::path::Path;

/// Render `file` offscreen and write a PNG to `output`.
///
/// `view` is one of `iso` / `top` / `front` / `right` — the same cameras
/// as `export svg`. With `bless`, additionally (re)write the crate's
/// committed golden image for the view (`tests/goldens/<stem>-<view>.png`):
/// the regeneration path, so goldens are never hand-edited. Returns the
/// rendered triangle count.
pub fn render_png(
    file: &str,
    output: &str,
    view: &str,
    width: u32,
    height: u32,
    bless: bool,
) -> Result<usize, String> {
    let scene = scene::build_scene(file)?;
    let view = camera::View::parse(view)?;
    let cam = camera::Camera::framing(scene.center, scene.radius, view);

    let frame = renderer::render_offscreen(&scene, &cam, width, height)?;
    write_png(output, &frame)?;

    if bless {
        let golden = golden_path(file, view)?;
        if let Some(parent) = golden.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create golden dir '{}': {e}", parent.display()))?;
        }
        write_png(&golden.to_string_lossy(), &frame)?;
        eprintln!(
            "bless: golden image written to {} — commit it (regenerated, never hand-edited)",
            golden.display()
        );
    }

    Ok(scene.triangle_count())
}

/// Query the object rendered at pixel (`x`, `y`) in the `view` frame.
///
/// `Ok(None)` when the pixel misses every object. Error objects are
/// pickable like any other.
pub fn pick_object(
    file: &str,
    view: &str,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
) -> Result<Option<String>, String> {
    let scene = scene::build_scene(file)?;
    let view = camera::View::parse(view)?;
    let cam = camera::Camera::framing(scene.center, scene.radius, view);
    Ok(pick::pick_at(&scene, &cam, width, height, x, y))
}

/// Encode a rendered frame as an RGBA8 PNG.
fn write_png(output: &str, frame: &renderer::GpuFrame) -> Result<(), String> {
    let file =
        std::fs::File::create(output).map_err(|e| format!("cannot create '{output}': {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("PNG header for '{output}' failed: {e}"))?;
    writer
        .write_image_data(&frame.rgba)
        .map_err(|e| format!("PNG body for '{output}' failed: {e}"))?;
    writer
        .finish()
        .map_err(|e| format!("PNG trailer for '{output}' failed: {e}"))?;
    Ok(())
}

/// The committed golden image path for `file` at `view`:
/// `<crate>/tests/goldens/<document-stem>-<view>.png`.
fn golden_path(file: &str, view: camera::View) -> Result<std::path::PathBuf, String> {
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("cannot derive a golden stem from '{file}'"))?;
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(format!("{stem}-{}.png", view.label())))
}
