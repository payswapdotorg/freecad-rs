//! Sketch round-trip preservation: the raw FreeCAD elements this
//! kernel's solver vocabulary cannot express, kept for verbatim
//! re-emission.
//!
//! FreeCAD's sketcher carries constructs with no counterpart here:
//! construction geometry and unmodeled curve kinds (B-splines,
//! ellipses, ...) in the `Geometry` list, constraint kinds like
//! `SnellsLaw`/`InternalAlignment`/`Weight` (and forms of mappable
//! kinds the solver does not carry, e.g. the point-form Tangent), plus
//! per-geometry `<GeoExtensions>` metadata. The loader translates what
//! it models — compacting the geometry list and remapping every
//! constraint GeoId — and records here everything it did **not** model,
//! so the writer can invert the remap and re-emit the raw elements at
//! their original slots. In-memory behavior is unchanged: the preserved
//! constructs stay out of the sketch (and out of the solve); only the
//! file sees them again.
//!
//! Contract: the preservation data describes the sketch *exactly as the
//! file wrote it*. [`crate::SketchObject::replace_sketch`] drops it
//! (new content has no file shadow); the loader's restore path keeps
//! it. Mutating the sketch's fields directly keeps stale preservation
//! for the untouched slots — new geometry/constraints append after the
//! original ones (see [`SketchPreservation::original_geoid`]).

use std::collections::BTreeMap;

/// The file shadow of a sketch: everything the loader skipped for
/// modeling, in the file's own terms.
#[derive(Debug, Clone, Default)]
pub struct SketchPreservation {
    /// FreeCAD-file GeoId → this kernel's GeoId, one entry per geometry
    /// element in the file's `GeometryList` (`None` = skipped:
    /// construction, off-plane, or an unmodeled curve kind). This is the
    /// table the reader compacted the geometry list with; the writer
    /// inverts it (see [`SketchPreservation::original_geoid`]).
    pub geo_remap: Vec<Option<u32>>,
    /// The file's total geometry count (`GeometryList count="…"`).
    pub geometry_count: usize,
    /// Raw `<Geometry …>…</Geometry>` blocks of skipped geometry, keyed
    /// by their original GeoId — re-emitted verbatim at that slot.
    pub skipped_geometry: BTreeMap<u32, String>,
    /// The raw `<GeoExtensions …>…</GeoExtensions>` element of *modeled*
    /// geometry, keyed by original GeoId (FreeCAD writes one per
    /// geometry; this kernel's writer emits an empty one when absent).
    pub geo_extensions: BTreeMap<u32, String>,
    /// The `id` attribute FreeCAD wrote on each `<Geometry>` element
    /// (GeoId + 1 — the persistent element id of the StringHasher era),
    /// keyed by original GeoId; re-emitted when present.
    pub geo_ids: BTreeMap<u32, String>,
    /// The file's total constraint count (`ConstraintList count="…"`).
    pub constraint_count: usize,
    /// Raw `<Constrain …/>` elements of unmappable constraints, keyed by
    /// their original list position — re-emitted verbatim at that
    /// position (they stay out of the solve).
    pub raw_constraints: BTreeMap<usize, String>,
    /// Raw trailing content of the `Constraints` property payload after
    /// `</ConstraintList>` (e.g. a `<ConstraintExtensions>` block), if
    /// the file carried any.
    pub constraint_extensions: Option<String>,
}

impl SketchPreservation {
    /// The original file GeoId this kernel's GeoId `ours` stands for:
    /// the inverse of [`SketchPreservation::geo_remap`]. `None` for
    /// geometry added after load — such geometry has no file slot, so
    /// the writer appends it after the original ones (slot
    /// `geometry_count + (ours - mapped)`; the same arithmetic this
    /// method performs for its callers).
    pub fn original_geoid(&self, ours: u32) -> Option<u32> {
        for (orig, mapped) in self.geo_remap.iter().enumerate() {
            if mapped == &Some(ours) {
                return Some(orig as u32);
            }
        }
        None
    }

    /// The GeoId to write into a saved constraint reference for this
    /// kernel's GeoId `ours`: the original when the geometry came from
    /// the file, else a fresh slot past the preserved ones.
    pub fn saved_geoid(&self, ours: u32) -> u32 {
        match self.original_geoid(ours) {
            Some(orig) => orig,
            None => {
                let mapped = self
                    .geo_remap
                    .iter()
                    .filter(|m| m.is_some())
                    .count() as u32;
                self.geometry_count as u32 + (ours - mapped)
            }
        }
    }
}
