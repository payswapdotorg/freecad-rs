//! Shared test helpers (both integration test binaries `mod common`).

use std::path::PathBuf;

/// The repo's committed demo document (4 objects: Base/Ball/Stem/Lift).
pub fn demo_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo.fcstd")
}

/// Build a document that contains a **failed** object **with geometry**:
/// a valid `Part::Transform` (computed), whose `Source` is then broken
/// into a non-Link — the next recompute fails inside execute and leaves
/// the object in `Error` with its last-good mesh (the repo's established
/// semantics). Saved to a unique temp path; returns the path and the
/// broken object's name.
pub fn error_doc() -> (PathBuf, &'static str) {
    use fc_app::{Document, Property};
    use fc_part::features::{BoxFeature, TransformFeature};

    let mut doc = Document::new("error-demo");
    doc.add(Box::new(BoxFeature::new("Base")));
    let mut lift = TransformFeature::new("Lift");
    lift.container.set("Source", Property::Link("Base".into()));
    // Offset the copy so it is visibly beside (and pickable apart from)
    // its source — a coincident copy would be occluded by the nearest-hit
    // rule, which is correct but untestable.
    lift.container.set(
        "Placement",
        Property::Placement(fc_base::Placement {
            position: fc_base::Vector3::new(15.0, 0.0, 0.0),
            rotation: fc_base::Rotation::IDENTITY,
        }),
    );
    doc.add(Box::new(lift));
    doc.recompute_all().expect("healthy document recomputes");
    assert!(
        fc_part::mesh_of(doc.get_by_name("Lift").unwrap(), 0.2).is_some(),
        "the transform must have computed geometry before we break it"
    );

    // Break the link *after* the mesh exists: execute fails, the object
    // is marked Error, the last-good geometry stays.
    doc.set_property("Lift", "Source", Property::Float(0.0))
        .expect("set broken source");
    doc.recompute_all().expect_err("broken source must fail");
    assert!(matches!(
        doc.get_by_name("Lift").unwrap().status(),
        fc_app::ObjectStatus::Error(_)
    ));

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("fc-viewer-error-{}.fcstd", nanos));
    fc_io::save_document(&doc, &path).expect("save error fixture");
    (path, "Lift")
}
