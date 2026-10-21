//! `fc-step` — STEP (ISO 10303-21, AP203) import/export.
//!
//! Per `docs/adr/0002-step-milestone.md`:
//!
//! - **Export** — the document's solids as *faceted* B-rep: legal STEP,
//!   honest about being a tessellation. The faceted nature is carried in
//!   the file's metadata (product-name marker), asserted by tests — not
//!   merely documented.
//! - **Import** — a STEP body becomes a `Part::Feature` mesh
//!   tessellated under an explicit deflection (the kernel's sagitta
//!   discipline); fidelity is asserted against deflection-derived
//!   tolerances, never hand-waved.
//!
//! Pure Rust; the workspace's zero-C/C++ property is intact. The primary
//! path is `ruststep`; the sanctioned fallback (ADR-0002 §5) is a
//! clean-room minimal AP203 subset for the fixture class.
//!
//! Scaffold state: this stub pins the CLI-facing surface; the WOQ work
//! order implements it.

/// Export the document's solid objects to `output` as faceted AP203 STEP.
///
/// The file's product name carries the faceted marker — the tessellated
/// nature is stated in the file itself, and a test asserts it.
pub fn export_step(file: &str, output: &str) -> Result<(), String> {
    Err(format!(
        "fc-step: not yet implemented (export {} to {}) — WOQ in flight",
        file, output
    ))
}

/// Import `input` (.step/.stp) into the document `file` as a
/// `Part::Feature` mesh, tessellated under `deflection` (mm).
///
/// Creates the document when missing; auto-uniquifies `name`. Returns
/// the created object's name.
pub fn import_step(
    file: &str,
    input: &str,
    name: Option<&str>,
    deflection: f64,
) -> Result<String, String> {
    Err(format!(
        "fc-step: not yet implemented (import {} into {} as {:?} @ {} mm) — WOQ in flight",
        input, file, name, deflection
    ))
}
