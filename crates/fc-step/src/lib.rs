//! `fc-step` — STEP (ISO 10303-21, AP203/AP214) import/export.
//!
//! Per `docs/adr/0002-step-milestone.md`:
//!
//! - **Parser** — [`ruststep`] 0.4 (pure Rust; nom/serde/thiserror, no
//!   C/C++): the Part 21 grammar. This crate owns the geometry
//!   semantics over its generic AST.
//! - **Export** — the document's solids as *faceted* B-rep: legal
//!   STEP, honestly labeled as a tessellation. The product name
//!   carries the faceted marker ([`FACETED_MARKER`]), asserted by
//!   tests — not merely documented.
//! - **Import** — a STEP body becomes a `Part::Feature` mesh
//!   tessellated under an explicit deflection (the kernel's sagitta
//!   discipline); fidelity is asserted against deflection-derived
//!   tolerances, never hand-waved. The supported subset is enforced
//!   honestly (see [`import`]): planar + cylindrical faces, LINE +
//!   CIRCLE edges, POLY_LOOP facets; anything else is an error, not
//!   a silent drop.

pub mod export;
pub mod import;
pub mod parse;

use std::path::{Path, PathBuf};

/// The faceted marker carried in every exported product name.
pub use export::FACETED_MARKER;

use fc_app::{Document, Property};
use fc_io::{load_document, save_document};
use fc_part::{mesh_of, MeshFeature, TopoShape};

/// Tessellate exported/imported geometry at the CLI exports' rule.
const EXPORT_DEFLECTION: f64 = 0.2;

/// Export the document's solid objects to `output` as faceted AP203
/// STEP.
///
/// Mirrors the other exporters' rules: load, recompute (refusing to
/// write stale geometry), take every mesh-bearing object's tessellation
/// at the CLI's deflection. The file's product names carry the faceted
/// marker — the tessellated nature is stated in the file itself, and a
/// test asserts it.
pub fn export_step(file: &str, output: &str) -> Result<(), String> {
    let (mut doc, report) =
        load_document(Path::new(file)).map_err(|e| format!("cannot load '{file}': {e}"))?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    doc.clear_history();
    doc.recompute_all()
        .map_err(|e| format!("recompute failed; not exporting stale geometry: {e}"))?;

    let mut objects: Vec<(String, fc_part::Mesh)> = Vec::new();
    for summary in doc.summary() {
        let obj = doc.get(summary.id).expect("summary id valid");
        if let Some(mesh) = mesh_of(obj, EXPORT_DEFLECTION) {
            objects.push((summary.name.clone(), mesh));
        }
    }
    if objects.is_empty() {
        return Err(format!(
            "no mesh-bearing objects in '{file}' — nothing to export"
        ));
    }
    let text = export::write_step(&objects);
    std::fs::write(output, text).map_err(|e| format!("cannot write '{output}': {e}"))?;
    Ok(())
}

/// Import `input` (.step/.stp) into the document `file` as
/// `Part::Feature` mesh objects, tessellated under `deflection` (mm).
///
/// Creates the document when missing; auto-uniquifies `name`. Returns
/// the created object's name. Each `MANIFOLD_SOLID_BREP` in the file
/// becomes one object (`name`, `name_2`, …).
pub fn import_step(
    file: &str,
    input: &str,
    name: Option<&str>,
    deflection: f64,
) -> Result<String, String> {
    let text = std::fs::read_to_string(input).map_err(|e| format!("cannot read '{input}': {e}"))?;
    let set = parse::EntitySet::parse(&text)?;
    let solids = import::import_solids(&set, deflection)?;
    // Honest unit context: the file must carry millimetre length units.
    if !text.contains(".MILLI.") && !text.contains("MILLI") {
        return Err(
            "STEP file does not declare millimetre length units — the importer is \
             millimetre-only (refusing to guess a scale)"
                .to_string(),
        );
    }

    let path = PathBuf::from(file);
    let (mut doc, report) = if path.exists() {
        load_document(&path).map_err(|e| format!("cannot load '{file}': {e}"))?
    } else {
        (Document::new("untitled"), Default::default())
    };
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    doc.clear_history();

    let base = name.unwrap_or("Imported");
    let mut created = String::new();
    for (i, (solid_name, mesh)) in solids.into_iter().enumerate() {
        let object_name = if i == 0 {
            uniquify(&doc, base)
        } else {
            uniquify(&doc, &format!("{base}_{}", i + 1))
        };
        let mut feature = MeshFeature::new(&object_name);
        feature
            .container
            .set("Label", Property::String(solid_name.clone()));
        feature.shape = Some(TopoShape::Mesh(mesh));
        doc.add(Box::new(feature));
        doc.recompute_all()
            .map_err(|e| format!("recompute after import failed: {e}"))?;
        if created.is_empty() {
            created.clone_from(&object_name);
        }
    }
    save_document(&doc, &path).map_err(|e| format!("cannot save '{file}': {e}"))?;
    Ok(created)
}

fn uniquify(doc: &Document, base: &str) -> String {
    if doc.get_id(base).is_none() {
        return base.to_string();
    }
    for i in 2.. {
        let candidate = format!("{base}_{i}");
        if doc.get_id(&candidate).is_none() {
            return candidate;
        }
    }
    unreachable!()
}
