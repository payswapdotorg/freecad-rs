//! `fc-viewer` — the M1 interactive-viewer core.
//!
//! Two halves, per `docs/PLAN-APPLICATION.md` (M1):
//!
//! - **Camera math** — orbit (target fixed), zoom (exponential in wheel
//!   ticks), pan (parallel to the screen plane) and the four standard
//!   views (`iso`/`top`/`front`/`right`) matching `export svg`'s cameras —
//!   pure functions, unit-tested headlessly.
//! - **Offscreen renderer** — wgpu, surface-less: load the document via
//!   `fc-io`, tessellate with the CLI exports' deflection rules,
//!   flat-shade, read the frame back, write a PNG. Error-flagged objects
//!   render visibly flagged; half-computed documents never crash the
//!   render.
//!
//! Interactive orbit (winit event loop) lives behind the non-default
//! `interactive` cargo feature. GPU work is gated in tests behind
//! `FC_VIEWER_GPU=1` (CI runs it on a software adapter).
//!
//! Scaffold state: this stub pins the CLI-facing surface; the WOP work
//! order implements it.

/// Render `file` offscreen and write a PNG to `output`.
///
/// `view` is one of `iso` / `top` / `front` / `right` — the same cameras
/// as `export svg`. With `bless`, additionally (re)write the crate's
/// committed golden image for the view: the regeneration path, so
/// goldens are never hand-edited. Returns the rendered triangle count.
pub fn render_png(
    file: &str,
    output: &str,
    view: &str,
    width: u32,
    height: u32,
    bless: bool,
) -> Result<usize, String> {
    let _ = (output, bless);
    Err(format!(
        "fc-viewer: not yet implemented (render {} view={} {}x{}) — WOP in flight",
        file, view, width, height
    ))
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
    Err(format!(
        "fc-viewer: not yet implemented (pick {} at {},{} in view={} {}x{}) — WOP in flight",
        file, x, y, view, width, height
    ))
}
