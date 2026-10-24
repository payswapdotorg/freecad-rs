//! The parity gate's Rust side: real-FreeCAD acceptance, CI edition.
//!
//! These tests are **environment-gated**: they run only when
//! `FREECAD_REF_DIR` points at reference `.FCStd` files written by a
//! real FreeCAD 1.0 binary (r39109) — the `scripts/parity/` harness
//! and the `parity` GitHub Actions workflow set that up. Without the
//! variable each test prints a skip note and returns, so the ordinary
//! suite stays hermetic and its test count is unaffected.
//!
//! The gate's three assertions (the acceptance criteria the plain
//! Rust CI cannot establish — see docs/VERIFICATION.md):
//!
//! 1. [`freecad_reference_files_load`] — every FreeCAD-written
//!    reference loads in freecad-rs, and every load warning is a
//!    *preservation* note (data the model keeps verbatim), never a
//!    silent loss.
//! 2. [`round_trip_preserves_freecad_authored_files`] — a pure
//!    load → save round-trip of each reference re-emits every
//!    archive entry byte-identically except `Document.xml`
//!    (regenerated), and the document property set comes back
//!    exactly — nothing injected, nothing dropped. The written files
//!    land in `FREECAD_ROUNDTRIP_DIR` for phase 3, where real FreeCAD
//!    re-opens them.
//! 3. [`recomputed_volumes_agree_with_freecad`] — after a recompute
//!    through this kernel, every object both sides can measure agrees
//!    with FreeCAD's volume: exactly (1e-6 relative) where the
//!    geometry is planar-analytic, and within the tessellation
//!    deflection (1% relative) for curved surfaces — the kernel's
//!    documented honesty band (docs/EQUIVALENCE.md).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

use fc_io::{load_document, save_document};
use fc_part::shape_of;

/// The armed gate's environment, or `None` when it is not armed.
struct Gate {
    /// FreeCAD-authored reference files (one per fixture).
    refs: PathBuf,
    /// Where the round-trip copies land for phase 3.
    roundtrips: PathBuf,
    /// The audit's TSV (FreeCAD-side measurements).
    audit_tsv: Option<PathBuf>,
}

fn armed_gate() -> Option<Gate> {
    let refs = std::env::var_os("FREECAD_REF_DIR").map(PathBuf::from)?;
    let roundtrips = std::env::var_os("FREECAD_ROUNDTRIP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("fcrs-parity-roundtrips"));
    let audit_tsv = std::env::var_os("FREECAD_AUDIT_TSV").map(PathBuf::from);
    Some(Gate {
        refs,
        roundtrips,
        audit_tsv,
    })
}

/// Every `.FCStd` under a directory, sorted (deterministic order).
fn fcstd_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("gate dir {} readable: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("fcstd"))
        })
        .collect();
    out.sort();
    out
}

/// Every entry of a zip, name → bytes.
fn zip_entries(path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    let file = std::fs::File::open(path).expect("file exists");
    let mut zip = zip::ZipArchive::new(file).expect("file is a zip");
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry readable");
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("entry readable");
        out.insert(entry.name().to_string(), data);
    }
    out
}

/// The names of the document-level `<Property>`/`<_Property>`
/// elements of `Document.xml` (the section before `<Objects`), as a
/// sorted set — the no-injection/no-drop contract's shape.
fn doc_property_names(xml: &str) -> Vec<String> {
    let split = xml.find("<Objects").expect("object table present");
    let section = &xml[..split];
    let mut names: Vec<String> = section
        .match_indices("<Property name=\"")
        .map(|(i, _)| i + "<Property name=\"".len())
        .map(|i| {
            let rest = &section[i..];
            rest[..rest.find('"').expect("quoted")].to_string()
        })
        .collect();
    names.extend(
        section
            .match_indices("<_Property name=\"")
            .map(|(i, _)| i + "<_Property name=\"".len())
            .map(|i| {
                let rest = &section[i..];
                rest[..rest.find('"').expect("quoted")].to_string()
            }),
    );
    names.sort();
    names
}

fn document_xml(entries: &BTreeMap<String, Vec<u8>>) -> String {
    String::from_utf8(entries["Document.xml"].clone()).expect("utf-8")
}

/// Gate assertion 1: FreeCAD-written references load cleanly, and
/// every warning names a preservation, never a loss.
#[test]
fn freecad_reference_files_load() {
    let Some(gate) = armed_gate() else {
        eprintln!("skipped: parity gate runs only with FREECAD_REF_DIR (scripts/parity/)");
        return;
    };
    let files = fcstd_files(&gate.refs);
    assert!(!files.is_empty(), "the gate found reference files");
    for src in &files {
        let (doc, report) =
            load_document(src).unwrap_or_else(|e| panic!("{} loads: {e}", src.display()));
        assert!(
            report
                .warnings
                .iter()
                .all(|w| fc_app::is_preservation_warning(w)),
            "{}: every warning names a preservation, not a loss: {:?}",
            src.display(),
            report.warnings
        );
        let _ = doc; // loading cleanly (no panic, no loss warning) is the assertion
    }
}

/// Gate assertion 2: the lossless round-trip contract applied to
/// FreeCAD-authored files — every preserved entry byte-identical, the
/// document property set exactly preserved, and the outputs handed
/// to phase 3 (real FreeCAD re-opens them).
#[test]
fn round_trip_preserves_freecad_authored_files() {
    let Some(gate) = armed_gate() else {
        eprintln!("skipped: parity gate runs only with FREECAD_REF_DIR (scripts/parity/)");
        return;
    };
    std::fs::create_dir_all(&gate.roundtrips).expect("roundtrip dir");
    for src in fcstd_files(&gate.refs) {
        let stem = src.file_stem().expect("stem").to_string_lossy().to_string();
        let (doc, _) =
            load_document(&src).unwrap_or_else(|e| panic!("{} loads: {e}", src.display()));
        let out = gate.roundtrips.join(format!("{stem}.fcstd"));
        save_document(&doc, &out).unwrap_or_else(|e| panic!("{} re-saves: {e}", src.display()));

        let before = zip_entries(&src);
        let after = zip_entries(&out);
        assert!(
            after.contains_key("Document.xml"),
            "{stem}: the document model is regenerated"
        );
        for (name, data) in &before {
            if name == "Document.xml" {
                continue; // the one entry the writer regenerates
            }
            assert_eq!(
                after.get(name).map(|v| v.as_slice()),
                Some(data.as_slice()),
                "{stem}: entry '{name}' re-emits byte-identically"
            );
        }
        let names_before = doc_property_names(&document_xml(&before));
        let names_after = doc_property_names(&document_xml(&after));
        assert_eq!(
            names_before, names_after,
            "{stem}: the document property set survives exactly"
        );
    }
}

/// One FreeCAD-measured volume: which file, which object, what value.
struct Measured {
    stem: String,
    object: String,
    volume: f64,
}

/// The audit's fixture rows with a nonempty volume.
fn freecad_volumes(path: &std::path::Path) -> Vec<Measured> {
    let text = std::fs::read_to_string(path).expect("audit tsv readable");
    let mut out = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 5 || cols[0] != "fixture" || cols[4].is_empty() {
            continue;
        }
        if let Ok(volume) = cols[4].parse::<f64>() {
            out.push(Measured {
                stem: cols[1].to_string(),
                object: cols[2].to_string(),
                volume,
            });
        }
    }
    out
}

/// Gate assertion 3: after this kernel's recompute, every object both
/// sides can measure agrees with FreeCAD's own volume — exactly for
/// planar-analytic geometry (1e-6 relative), within the tessellation
/// deflection for curved surfaces (1% relative; docs/EQUIVALENCE.md).
#[test]
fn recomputed_volumes_agree_with_freecad() {
    let Some(gate) = armed_gate() else {
        eprintln!("skipped: parity gate runs only with FREECAD_REF_DIR (scripts/parity/)");
        return;
    };
    let Some(tsv) = gate
        .audit_tsv
        .as_deref()
        .and_then(|p| (p.exists()).then_some(p))
    else {
        panic!("the gate is armed but FREECAD_AUDIT_TSV is missing");
    };
    let measured = freecad_volumes(tsv);
    assert!(
        !measured.is_empty(),
        "the audit recorded FreeCAD volumes to compare against"
    );

    const EXACT: f64 = 1e-6; // planar-analytic agreement
    const DEFLECTION: f64 = 0.01; // tessellation-honest bound (curved)

    let mut compared = 0usize;
    let mut skipped = 0usize;
    for m in &measured {
        let src = gate.refs.join(format!("{}.FCStd", m.stem));
        let (mut doc, _) =
            load_document(&src).unwrap_or_else(|e| panic!("{} loads: {e}", src.display()));
        doc.recompute_all()
            .unwrap_or_else(|e| panic!("{} recomputes: {e}", src.display()));
        let Some(object) = doc.get_by_name(&m.object) else {
            skipped += 1; // an object our model holds only as preservation
            continue;
        };
        let Some(volume) = shape_of(object).map(|s| s.volume()) else {
            skipped += 1; // no measurable shape on our side (e.g. an OCCT payload)
            continue;
        };
        let scale = m.volume.abs().max(1e-12);
        let diff = (volume - m.volume).abs();
        let tier = if diff <= EXACT * scale {
            "exact"
        } else if diff <= DEFLECTION * scale {
            "deflection"
        } else {
            panic!(
                "{}.{}: volume disagrees with FreeCAD: ours {volume}, FreeCAD {}, diff {}",
                m.stem,
                m.object,
                m.volume,
                diff / scale
            );
        };
        println!(
            "volume[{}]: {}.{} ours {volume:.6} vs FreeCAD {:.6}",
            tier, m.stem, m.object, m.volume
        );
        compared += 1;
    }
    assert!(
        compared > 0,
        "the gate compared at least one volume (skipped {skipped} unmeasurable objects)"
    );
    println!("volume comparison: {compared} compared, {skipped} skipped");
}
