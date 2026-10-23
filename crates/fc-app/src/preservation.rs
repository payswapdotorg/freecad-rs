//! Round-trip preservation: raw data the document model does not capture
//! but the `.FCStd` writer re-emits verbatim.
//!
//! FreeCAD's file format names far more than any re-implementation models:
//! GUI state (`GuiDocument.xml`, thumbnails), OCCT BRep shape payloads,
//! property types outside the modeled vocabulary, document metadata (Uid,
//! Company, License, ...), sketcher constructs without a solver counterpart
//! (construction geometry, SnellsLaw constraints, ...). The honest policy
//! for those is **preservation**: the loader stores the original bytes
//! untouched, the writer re-emits them byte-identically, and the load report
//! says so in so many words ("preserved verbatim for round-trip"). Live
//! state always wins: a freshly computed shape replaces its preserved OCCT
//! payload, an edited sketch drops its preserved raw elements — the
//! preservation bag is a shadow of the file, never of the model.
//!
//! FreeCAD-parity note: this mirrors what FreeCAD itself must do for
//! unknown-but-tolerated data (its reader restores properties through
//! registered C++ types and skips the rest — with the difference that a
//! real FreeCAD has a type for nearly everything; this kernel does not,
//! so the bytes are kept instead).

use std::collections::BTreeMap;

/// One shape payload this kernel cannot parse but keeps verbatim: the
/// raw `<Property name="Shape" ...>…</Property>` element (which carries
/// the entry reference — and any extra attributes/siblings like
/// FreeCAD's `ElementMap`) plus the entry's original bytes.
#[derive(Clone)]
pub struct PreservedShape {
    /// The property the payload arrived under (e.g. `Shape`,
    /// `InternalShape`) — the raw property element is re-emitted under
    /// that name.
    pub property: String,
    /// The raw `<Property …>…</Property>` source, verbatim.
    pub raw_property: String,
    /// The original zip entry name the raw property references (empty for
    /// inline payloads — FreeCAD's legacy `brep="1"` form re-emits inline).
    pub entry: String,
    /// The raw payload bytes.
    pub data: Vec<u8>,
    /// Storage form of the original entry: `true` = stored (uncompressed),
    /// `false` = deflated (FreeCAD's `Z_DEFAULT_COMPRESSION` form).
    pub stored: bool,
}

impl std::fmt::Debug for PreservedShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreservedShape")
            .field("property", &self.property)
            .field("entry", &self.entry)
            .field("bytes", &self.data.len())
            .field("stored", &self.stored)
            .finish()
    }
}

/// Per-object preservation bag (loader-set, writer-consumed). Lives inside
/// [`crate::PropertyContainer`] so every object kind can carry it without
/// a trait change per type.
#[derive(Default, Clone)]
pub struct ObjectPreservation {
    /// Attributes of the `<Object>` element beyond `name` (e.g. FreeCAD's
    /// `Extensions="True"`), in file order.
    pub object_attrs: Vec<(String, String)>,
    /// The raw `<Extensions …>…</Extensions>` element when the file
    /// carried one (FreeCAD's `Part::AttachExtension` blocks).
    pub extensions: Option<String>,
    /// Unmodeled `<Property>` elements: property name → raw source, in
    /// file order. Re-emitted verbatim after the modeled properties.
    /// The property types this kernel models are live state and never
    /// appear here.
    pub properties: Vec<(String, String)>,
    /// FreeCAD's transient `<_Property …/>` markers (value-less; the
    /// `TransientCount` attribute on the `<Properties>` element counts
    /// them): raw sources, in file order.
    pub transient_properties: Vec<(String, String)>,
    /// Attributes of the object's `<Properties>` element beyond
    /// `Count`/`TransientCount`, in file order.
    pub properties_attrs: Vec<(String, String)>,
    /// The file's `status` attribute per modeled property (FreeCAD's
    /// persisted property-state bitmask: `1` Touched, `4` ReadOnly,
    /// `8` Hidden, `8388608`… LockDynamic etc. — the editor preferences
    /// a user set are stored here). The model does not compute these
    /// bits; they re-emit verbatim on the canonically re-serialized
    /// properties (unmodeled ones carry theirs inside the raw element
    /// already).
    pub property_status: Vec<(String, String)>,
    /// Shape payloads kept verbatim (OCCT BRep, unknown formats).
    pub payloads: Vec<PreservedShape>,
}

impl std::fmt::Debug for ObjectPreservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectPreservation")
            .field("object_attrs", &self.object_attrs)
            .field("extensions", &self.extensions.is_some())
            .field("property_status", &self.property_status)
            .field(
                "properties",
                &self
                    .properties
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>(),
            )
            .field(
                "payloads",
                &self
                    .payloads
                    .iter()
                    .map(|p| p.property.as_str())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// A zip entry the loader does not consume (no modeled data references
/// it): GUI state (`GuiDocument.xml`), thumbnails, anything else a real
/// FreeCAD wrote. Re-emitted byte-identically with the same storage form.
#[derive(Clone)]
pub struct PreservedZipEntry {
    /// Entry path inside the archive (e.g. `Thumbnails/Thumbnail.png`).
    pub name: String,
    /// The raw entry bytes.
    pub data: Vec<u8>,
    /// Storage form of the original entry: `true` = stored,
    /// `false` = deflated.
    pub stored: bool,
}

impl std::fmt::Debug for PreservedZipEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreservedZipEntry")
            .field("name", &self.name)
            .field("bytes", &self.data.len())
            .field("stored", &self.stored)
            .finish()
    }
}

/// Document-level preservation bag (loader-set, writer-consumed):
/// everything attached to `<Document>` itself rather than to an object.
#[derive(Default, Clone)]
pub struct DocumentPreservation {
    /// Root-element attributes beyond the regenerated trio
    /// (`SchemaVersion`/`ProgramVersion`/`FileVersion`), e.g. FreeCAD
    /// 1.0's `StringHasher="1"` — in file order.
    pub root_attributes: Vec<(String, String)>,
    /// Raw non-property children of `<Document>` this kernel does not
    /// model (e.g. `<StringHasher …/>`), verbatim.
    pub raw_elements: Vec<String>,
    /// Attributes of the document `<Properties>` element beyond `Count`
    /// (e.g. FreeCAD's `TransientCount="3"`).
    pub properties_attrs: Vec<(String, String)>,
    /// Unmodeled document `<Property>` elements: name → raw source, in
    /// file order. The three the model carries
    /// (`Label`/`Creator`/`CreationDate`) are live state, never here.
    pub properties: Vec<(String, String)>,
    /// The file's `status` attribute on the modeled document properties
    /// (`Label`/`Creator`/`CreationDate` — FreeCAD's persisted
    /// property-state bitmask; unmodeled ones carry theirs inside the
    /// raw element already), re-emitted verbatim.
    pub property_status: Vec<(String, String)>,
    /// FreeCAD's transient document `<_Property …/>` markers
    /// (FileName/Tip/TransientDir — value-less, counted by the
    /// section's `TransientCount` attribute): raw sources, in file
    /// order.
    pub transient_properties: Vec<(String, String)>,
    /// Attributes of the `<Objects>` element beyond `Count` (e.g.
    /// FreeCAD 1.0's `Dependencies="1"`), in file order.
    pub objects_attrs: Vec<(String, String)>,
    /// Attributes of the `<ObjectData>` element beyond `Count`, in file
    /// order.
    pub objectdata_attrs: Vec<(String, String)>,
    /// Per-object `<ObjectDeps …/>` elements, keyed by the object name
    /// they precede (FreeCAD's partial-restore dependency hints).
    pub object_deps: BTreeMap<String, String>,
    /// Unconsumed zip entries, preserved for verbatim re-emission.
    pub zip_entries: Vec<PreservedZipEntry>,
}

impl std::fmt::Debug for DocumentPreservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentPreservation")
            .field("root_attributes", &self.root_attributes)
            .field(
                "raw_elements",
                &self
                    .raw_elements
                    .iter()
                    .map(|e| e.len())
                    .collect::<Vec<_>>(),
            )
            .field("properties_attrs", &self.properties_attrs)
            .field("property_status", &self.property_status)
            .field(
                "properties",
                &self
                    .properties
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("object_deps", &self.object_deps.keys().collect::<Vec<_>>())
            .field(
                "zip_entries",
                &self
                    .zip_entries
                    .iter()
                    .map(|e| e.name.as_str())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Is this load-report warning a *preservation* note (data kept verbatim
/// for round-trip) rather than a loss? The load report keeps one channel
/// for all honest messages; this predicate — matching the documented
/// phrase every preservation warning carries — lets callers separate
/// "kept, not modeled" from "kept nothing" when they assert on losses.
pub fn is_preservation_warning(warning: &str) -> bool {
    warning.contains("preserved verbatim")
}
