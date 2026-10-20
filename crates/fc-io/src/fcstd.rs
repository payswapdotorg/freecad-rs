//! `.FCStd` reader/writer. See the crate docs for the format.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use fc_app::{Document, DocumentObject, ObjectStatus, Property};
use fc_base::{Placement, Rotation, Vector3};
use fc_part::brep::{self, BrpFormat};
use fc_part::features::{
    BooleanFeature, BooleanKind, BoxFeature, ConeFeature, CylinderFeature, SphereFeature,
    TorusFeature, TransformFeature,
};
use fc_part::{TopoShape, DEFAULT_DEFLECTION};
use fc_sketcher::SketchObject;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Debug)]
pub enum FcstdError {
    Io(std::io::Error),
    Zip(String),
    Xml(String),
    Unsupported(String),
}

impl std::fmt::Display for FcstdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FcstdError::Io(e) => write!(f, "i/o error: {e}"),
            FcstdError::Zip(e) => write!(f, "zip container error: {e}"),
            FcstdError::Xml(e) => write!(f, "Document.xml error: {e}"),
            FcstdError::Unsupported(e) => write!(f, "unsupported: {e}"),
        }
    }
}

impl std::error::Error for FcstdError {}

impl From<std::io::Error> for FcstdError {
    fn from(e: std::io::Error) -> Self {
        FcstdError::Io(e)
    }
}

/// What happened while loading (never fatal by itself; warnings are
/// reported, objects are still created when possible).
#[derive(Debug, Default, Clone)]
pub struct LoadReport {
    pub warnings: Vec<String>,
    /// Objects successfully constructed.
    pub objects: usize,
    /// Objects skipped entirely (unknown type, unreadable).
    pub skipped: usize,
}

// ---------------------------------------------------------------------------
// Saving
// ---------------------------------------------------------------------------

/// Save `doc` as `.FCStd`. Writes `Document.xml` first, then one FCBREP
/// side file per part feature (`Shape001.brp`, `Shape002.brp`, ...).
/// Deflate-compressed entries, like FreeCAD.
pub fn save_document(doc: &Document, path: &Path) -> Result<(), FcstdError> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut side_files: Vec<(String, Vec<u8>)> = Vec::new();
    let xml = build_document_xml(doc, &mut side_files)?;

    zip.start_file("Document.xml", options).map_err(zip_err)?;
    zip.write_all(xml.as_bytes())?;
    for (name, bytes) in &side_files {
        zip.start_file(name.as_str(), options).map_err(zip_err)?;
        zip.write_all(bytes)?;
    }
    zip.finish().map_err(zip_err)?;
    Ok(())
}

fn zip_err(e: zip::result::ZipError) -> FcstdError {
    FcstdError::Zip(e.to_string())
}

/// Render `Document.xml` (FreeCAD's exact skeleton, one-space indentation)
/// and collect the FCBREP side files to store next to it.
fn build_document_xml(
    doc: &Document,
    side_files: &mut Vec<(String, Vec<u8>)>,
) -> Result<String, FcstdError> {
    let mut xml = String::new();
    xml.push_str(&format!(
        "<Document SchemaVersion=\"4\" ProgramVersion=\"freecad-rs {}\" FileVersion=\"1\">\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Document-level properties (Label / Creator / CreationDate).
    xml.push_str(" <Properties Count=\"3\">\n");
    push_property(
        &mut xml,
        2,
        "Label",
        &Property::String(doc.label().to_string()),
    );
    push_property(
        &mut xml,
        2,
        "Creator",
        &Property::String(doc.creator().to_string()),
    );
    push_property(
        &mut xml,
        2,
        "CreationDate",
        &Property::String(doc.created().to_string()),
    );
    xml.push_str(" </Properties>\n");

    let objects = doc.summary();

    // Object table.
    xml.push_str(&format!(
        " <Objects Count=\"{}\" Dependencies=\"1\">\n",
        objects.len()
    ));
    for o in &objects {
        xml.push_str(&format!(
            "  <Object type=\"{}\" name=\"{}\" id=\"{}\"",
            escape_attr(&o.type_name),
            escape_attr(&o.name),
            o.id.0
        ));
        match o.status {
            ObjectStatus::Touched => xml.push_str(" status=\"Touched\""),
            ObjectStatus::Valid => xml.push_str(" status=\"Valid\""),
            ObjectStatus::Error(ref reason) => {
                xml.push_str(&format!(
                    " status=\"Error\" Error=\"{}\"",
                    escape_attr(reason)
                ));
            }
        }
        xml.push_str("/>\n");
    }
    xml.push_str(" </Objects>\n");

    // Per-object property data.
    xml.push_str(&format!(" <ObjectData Count=\"{}\">\n", objects.len()));
    for o in &objects {
        let obj = doc.get(o.id).ok_or_else(|| {
            FcstdError::Xml(format!(
                "internal: object '{}' missing from document",
                o.name
            ))
        })?;
        let props = obj.properties();
        let shape = computed_shape(obj);
        let count = props.len() + usize::from(shape.is_some());

        xml.push_str(&format!("  <Object name=\"{}\">\n", escape_attr(&o.name)));
        xml.push_str(&format!("   <Properties Count=\"{}\">\n", count));
        for (key, value) in props {
            push_property(&mut xml, 4, key, value);
        }
        // Computed geometry goes last, as one referenced FCBREP side file.
        if let Some(shape) = shape {
            let file = format!("Shape{:03}.brp", side_files.len() + 1);
            xml.push_str("    <Property name=\"Shape\" type=\"Part::PropertyPartShape\">\n");
            xml.push_str(&format!("     <Part file=\"{}\"/>\n", escape_attr(&file)));
            xml.push_str("    </Property>\n");
            let mut bytes = Vec::new();
            brep::write_shape(&mut bytes, &shape)?;
            side_files.push((file, bytes));
        }
        xml.push_str("   </Properties>\n");
        // Sketch content (`Sketcher::SketchObject`): geometry and
        // constraints are object data, not properties — FreeCAD keeps them
        // on the object itself, and so do we, as one `<Sketch>` fragment
        // (the exact element `Sketch::to_xml()` produces) emitted as a
        // child of `<Object>`, next to `<Properties>`. The fragment's own
        // two-space child indent is preserved relative to the 3-space
        // `<Object>` child base, so it nests consistently with the
        // surrounding one-space-per-level writer style (see
        // `push_indented_fragment`).
        if let Some(fragment) = sketch_fragment(obj) {
            push_indented_fragment(&mut xml, &fragment, 3);
        }
        // Expression bindings (FreeCAD persists them as children of the
        // <Object> element, after the properties).
        for (path, expression) in obj.expressions() {
            xml.push_str(&format!(
                "   <Expression path=\"{}\" expression=\"{}\"/>\n",
                escape_attr(path),
                escape_attr(expression)
            ));
        }
        xml.push_str("  </Object>\n");
    }
    xml.push_str(" </ObjectData>\n");
    xml.push_str("</Document>\n");
    Ok(xml)
}

/// Append one `<Property>` (with its value element) at `indent` spaces.
fn push_property(xml: &mut String, indent: usize, name: &str, value: &Property) {
    let ind = " ".repeat(indent);
    let (ty, body) = property_xml(value);
    xml.push_str(&format!(
        "{ind}<Property name=\"{}\" type=\"{ty}\">\n",
        escape_attr(name)
    ));
    xml.push_str(&format!("{ind} {body}\n"));
    xml.push_str(&format!("{ind}</Property>\n"));
}

/// The FreeCAD value element for a property, per the crate's element table.
fn property_xml(value: &Property) -> (&'static str, String) {
    match value {
        Property::Bool(v) => (
            "App::PropertyBool",
            format!("<Bool value=\"{v}\"/>"),
        ),
        // Rust's `{}` for f64 prints the shortest round-trip form, which is
        // what FreeCAD's full-precision writer aims for too.
        Property::Float(v) => (
            "App::PropertyFloat",
            format!("<Float value=\"{v}\"/>"),
        ),
        Property::Integer(v) => (
            "App::PropertyInteger",
            format!("<Integer value=\"{v}\"/>"),
        ),
        Property::String(v) => (
            "App::PropertyString",
            format!("<String value=\"{}\"/>", escape_attr(v)),
        ),
        Property::Vector(v) => (
            "App::PropertyVector",
            format!(
                "<PropertyVector valueX=\"{}\" valueY=\"{}\" valueZ=\"{}\"/>",
                v.x, v.y, v.z
            ),
        ),
        // A/Ox/Oy/Oz are optional in FreeCAD's format; we omit them.
        Property::Placement(p) => (
            "App::PropertyPlacement",
            format!(
                "<PropertyPlacement Px=\"{}\" Py=\"{}\" Pz=\"{}\" Q0=\"{}\" Q1=\"{}\" Q2=\"{}\" Q3=\"{}\"/>",
                p.position.x,
                p.position.y,
                p.position.z,
                p.rotation.x,
                p.rotation.y,
                p.rotation.z,
                p.rotation.w
            ),
        ),
        Property::Link(v) => (
            "App::PropertyLink",
            format!("<Link value=\"{}\"/>", escape_attr(v)),
        ),
        Property::LinkList(v) => (
            "App::PropertyLinkList",
            format!("<LinkList value=\"{}\"/>", escape_attr(&v.join(","))),
        ),
    }
}

/// The `<Sketch>` fragment of a sketch object, if it is one (`None` for
/// every other object type). Save-side counterpart of the load-side
/// `store_sketch_fragment`.
fn sketch_fragment(obj: &dyn DocumentObject) -> Option<String> {
    obj.as_any()
        .downcast_ref::<SketchObject>()
        .map(|sketch_object| sketch_object.sketch.to_xml())
}

/// Append a `<Sketch>` fragment at `base_indent` spaces. The fragment's
/// internal indentation is uniform (0 for the `<Sketch>`/`</Sketch>`
/// lines, 2 for each child), so re-emitting every non-blank line at
/// `base_indent + <its own leading spaces>` preserves the fragment's
/// relative shape exactly — the sketch parser is whitespace-tolerant and
/// the result nests cleanly inside `<Object>`.
fn push_indented_fragment(xml: &mut String, fragment: &str, base_indent: usize) {
    for line in fragment.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let relative = line.len() - trimmed.len();
        xml.push_str(&" ".repeat(base_indent + relative));
        xml.push_str(trimmed);
        xml.push('\n');
    }
}

/// Computed geometry of a part feature, if any: primitives carry their
/// analytic `TopoShape`, transform features their result mesh. Objects
/// without geometry (or transforms without a result) return `None`.
fn computed_shape(obj: &dyn DocumentObject) -> Option<TopoShape> {
    let any = obj.as_any();
    if let Some(f) = any.downcast_ref::<BoxFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<SphereFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<CylinderFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<ConeFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<TorusFeature>() {
        f.shape.clone()
    } else if let Some(f) = any.downcast_ref::<TransformFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else if let Some(f) = any.downcast_ref::<BooleanFeature>() {
        f.result.clone().map(TopoShape::Mesh)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load a `.FCStd`. Accepts our own files and real FreeCAD files (see
/// crate docs for fidelity limits). Objects are added in file order (the
/// file's ids are cosmetic — links resolve by name) and each gets its
/// persisted properties, status and geometry where possible.
pub fn load_document(path: &Path) -> Result<(Document, LoadReport), FcstdError> {
    load_document_with_hook(path, None)
}

/// Per-object mutation hook for [`load_document_with_hook`]: receives the
/// object's file name and the fully-built object (properties, status and
/// sketch content already restored) just before it is added to the
/// document.
pub type ObjectHook<'a> = dyn FnMut(&str, &mut dyn DocumentObject) + 'a;

/// Load a `.FCStd`, letting `mutate` visit every fully-built object with
/// mutable access right before it enters the document.
///
/// This is the load-time escape hatch for content operations the document
/// API cannot express once an object is inside a `Document` (which
/// deliberately exposes no mutable object access): e.g. the CLI's
/// `sketch-import` swaps a `Sketcher::SketchObject`'s content via
/// `replace_sketch` (which touches the object) and marks the objects that
/// link to it as `Touched` so they re-execute on the next recompute. The
/// hook runs while objects are still outside the document, so touch
/// cascades and journal records are the caller's responsibility.
pub fn load_document_with_hook(
    path: &Path,
    mutate: Option<&mut ObjectHook<'_>>,
) -> Result<(Document, LoadReport), FcstdError> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_err)?;

    let xml_bytes = match zip.by_name("Document.xml") {
        Ok(mut entry) => {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            bytes
        }
        Err(zip::result::ZipError::FileNotFound) => {
            return Err(FcstdError::Xml(
                "no Document.xml entry in archive (not a .FCStd file?)".to_string(),
            ));
        }
        Err(e) => return Err(zip_err(e)),
    };

    let mut report = LoadReport::default();
    let mut doc = Document::new("untitled");
    parse_document(&xml_bytes, &mut zip, &mut doc, &mut report, mutate)?;
    Ok((doc, report))
}

/// One `<Part>` payload reference found while parsing.
struct ShapeRef {
    /// Side-file name inside the zip archive.
    file: Option<String>,
    /// Inline payload bytes (FreeCAD's `brep="1"` mode).
    inline: Vec<u8>,
}

/// A parsed property value element.
enum PropValue {
    Value(Property),
    Shape(ShapeRef),
}

/// The pull-parser state we need between helper calls.
type XmlReader<'x> = Reader<&'x [u8]>;

/// Read the next event as owned data (so the shared buffer can be reused).
fn next_event(reader: &mut XmlReader<'_>, buf: &mut Vec<u8>) -> Result<Event<'static>, FcstdError> {
    reader
        .read_event_into(buf)
        .map(|ev| ev.into_owned())
        .map_err(|e| FcstdError::Xml(format!("XML parse error: {e}")))
}

/// All attributes of an element, unescaped, as an owned map.
fn attrs_of(e: &BytesStart<'_>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for a in e.attributes().flatten() {
        if let Ok(v) = a.unescape_value() {
            map.insert(
                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                v.into_owned(),
            );
        }
    }
    map
}

fn attr_f64(map: &BTreeMap<String, String>, key: &str) -> Option<f64> {
    map.get(key)?.trim().parse::<f64>().ok()
}

/// FreeCAD's bool spellings (`true`/`false`, `1`/`0`).
fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "1" | "True" | "TRUE" => Some(true),
        "false" | "0" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

/// `Touched="1"` / `Invalid="1"` (FreeCAD) and `status="Touched|Valid|Error"`
/// (ours) both map onto [`ObjectStatus`].
fn parse_status(map: &BTreeMap<String, String>) -> ObjectStatus {
    if let Some(s) = map.get("status") {
        return match s.as_str() {
            "Valid" => ObjectStatus::Valid,
            "Error" => ObjectStatus::Error(
                map.get("Error")
                    .cloned()
                    .unwrap_or_else(|| "error (from file)".to_string()),
            ),
            // Unknown status words degrade to "needs recompute", the safe default.
            _ => ObjectStatus::Touched,
        };
    }
    if parse_bool(map.get("Invalid").map_or("", String::as_str)).unwrap_or(false) {
        return ObjectStatus::Error("invalid (from file)".to_string());
    }
    if parse_bool(map.get("Touched").map_or("", String::as_str)).unwrap_or(false) {
        ObjectStatus::Touched
    } else {
        // No status information at all: FreeCAD only omits Touched/Invalid
        // for objects that are up to date.
        ObjectStatus::Valid
    }
}

/// Consume the rest of a just-opened element (depth-counted).
fn skip_element(reader: &mut XmlReader<'_>, buf: &mut Vec<u8>) -> Result<(), FcstdError> {
    let mut depth = 1usize;
    loop {
        match next_event(reader, buf)? {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unexpected end of Document.xml".to_string(),
                ));
            }
            _ => {}
        }
    }
}

/// Parse the whole `Document.xml` into `doc`/`report`, reading shape
/// payloads from `zip`.
fn parse_document(
    xml: &[u8],
    zip: &mut zip::ZipArchive<std::fs::File>,
    doc: &mut Document,
    report: &mut LoadReport,
    mut mutate: Option<&mut ObjectHook<'_>>,
) -> Result<(), FcstdError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    // Locate the <Document> root (skipping declaration / DOCTYPE / comments).
    loop {
        match next_event(&mut reader, &mut buf)? {
            Event::Start(e) => {
                if e.name().as_ref() != b"Document" {
                    return Err(FcstdError::Xml(format!(
                        "expected <Document> root, found <{}>",
                        String::from_utf8_lossy(e.name().as_ref())
                    )));
                }
                break;
            }
            Event::Eof => {
                return Err(FcstdError::Xml("no <Document> root element".to_string()));
            }
            _ => continue,
        }
    }

    // Objects built from the <Objects> table, in FILE order. They are
    // completed (properties, status, geometry) before being added to the
    // document, which then assigns fresh ids in insertion order.
    //
    // ID handling: the file's `id` attributes are deliberately ignored.
    // Links in the format are BY NAME (see the `<Link value="...">`
    // element), so real FreeCAD files with id gaps (objects were deleted)
    // load fine — `doc.add` gives our ids 1..N in insertion order instead.
    // The only observable difference is the cosmetic `id` attribute
    // renumbered on the next save.
    let mut built: Vec<(String, Option<Box<dyn DocumentObject>>)> = Vec::new();

    loop {
        match next_event(&mut reader, &mut buf)? {
            Event::Start(e) => match e.name().as_ref() {
                b"Properties" => parse_document_properties(&mut reader, &mut buf, doc, report)?,
                b"Objects" => parse_objects_section(&mut reader, &mut buf, &mut built, report)?,
                b"ObjectData" => {
                    parse_object_data_section(&mut reader, &mut buf, xml, &mut built, zip, report)?
                }
                _ => skip_element(&mut reader, &mut buf)?,
            },
            // Self-closing sections: nothing to read inside them.
            Event::Empty(_) => {}
            Event::End(e) if e.name().as_ref() == b"Document" => break,
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Document> element".to_string(),
                ));
            }
            _ => {}
        }
    }

    // Add the finished objects in file order; the document owns the ids from
    // here on. `add` may suffix names for uniqueness (Box, Box001, ...) —
    // only possible with duplicate names in the file, which we report.
    for (file_name, object) in built {
        if let Some(mut object) = object {
            // The caller's hook sees the finished object (properties,
            // status, sketch content) before the document takes ownership.
            if let Some(mutate) = mutate.as_deref_mut() {
                mutate(&file_name, object.as_mut());
            }
            let id = doc.add(object);
            let final_name = doc.get(id).map(DocumentObject::name).unwrap_or_default();
            if final_name != file_name {
                report.warnings.push(format!(
                    "object '{file_name}' renamed to '{final_name}' (duplicate name in file)"
                ));
            }
        }
    }

    // Links are by name; verify they resolve now that everything is loaded.
    for summary in doc.summary() {
        if let Some(obj) = doc.get_by_name(&summary.name) {
            for (key, value) in obj.properties() {
                let targets: Vec<String> = match value {
                    Property::Link(t) => vec![t.clone()],
                    Property::LinkList(ts) => ts.clone(),
                    _ => continue,
                };
                for t in targets {
                    if doc.get_by_name(&t).is_none() {
                        report.warnings.push(format!(
                            "object '{}': property '{key}' links to '{t}', which was not loaded \
                             (unknown or skipped object)",
                            summary.name
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Document-level `<Properties>`: we persist Label/Creator/CreationDate;
/// other metadata (Uid, Company, ...) is not modeled and ignored.
fn parse_document_properties(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    doc: &mut Document,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == b"Property" => {
                let map = attrs_of(&e);
                let name = map.get("name").cloned().unwrap_or_default();
                let value = parse_property_value(reader, buf, "document", &name, &map, report)?;
                match (name.as_str(), value) {
                    ("Label", Some(PropValue::Value(Property::String(s)))) => doc.set_label(&s),
                    ("Creator", Some(PropValue::Value(Property::String(s)))) => doc.set_creator(&s),
                    ("CreationDate", Some(PropValue::Value(Property::String(s)))) => {
                        doc.set_created(s)
                    }
                    (n @ ("Label" | "Creator" | "CreationDate"), Some(_)) => {
                        report.warnings.push(format!(
                            "document property '{n}': not a string value — skipped"
                        ));
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.name().as_ref() == b"Properties" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Properties> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// The `<Objects>` table: type/name/id triplets, plus status dialects.
/// Builds the objects immediately (they get their data later).
fn parse_objects_section(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    built: &mut Vec<(String, Option<Box<dyn DocumentObject>>)>,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"Object" => {
                let map = attrs_of(&e);
                let type_id = map.get("type").cloned().unwrap_or_default();
                let name = map.get("name").cloned().unwrap_or_default();
                // `id` is read but intentionally unused (see parse_document).
                let _file_id = map.get("id");
                if type_id.is_empty() || name.is_empty() {
                    report.skipped += 1;
                    report.warnings.push(format!(
                        "<Object> entry without type or name (id {}) — skipped",
                        map.get("id").map_or("?", String::as_str)
                    ));
                    continue;
                }
                let status = parse_status(&map);
                let object = match build_object(&type_id, &name) {
                    Some(mut object) => {
                        object.set_status(status);
                        report.objects += 1;
                        Some(object)
                    }
                    None => {
                        report.skipped += 1;
                        report.warnings.push(format!(
                            "object '{name}': unknown type '{type_id}' — skipped"
                        ));
                        None
                    }
                };
                built.push((name, object));
            }
            Event::End(e) if e.name().as_ref() == b"Objects" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Objects> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// The `<ObjectData>` section: feeds each `<Object>`'s `<Properties>` into
/// the object built from the `<Objects>` table (matched by name).
fn parse_object_data_section(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    xml: &[u8],
    built: &mut Vec<(String, Option<Box<dyn DocumentObject>>)>,
    zip: &mut zip::ZipArchive<std::fs::File>,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == b"Object" => {
                let map = attrs_of(&e);
                let name = map.get("name").cloned().unwrap_or_default();
                if name.is_empty() {
                    report
                        .warnings
                        .push("<ObjectData> entry without a name — skipped".to_string());
                    skip_element(reader, buf)?;
                    continue;
                }
                match built.iter().position(|(n, _)| *n == name) {
                    Some(i) => {
                        let owner = built[i].0.clone();
                        if let Some(object) = built[i].1.as_mut() {
                            parse_object_properties(reader, buf, xml, object, &owner, zip, report)?;
                        }
                        // An object that failed to build was already reported
                        // (with its skip) in the <Objects> section.
                    }
                    None => {
                        report.warnings.push(format!(
                            "object '{name}': <ObjectData> entry has no matching <Objects> entry — ignored"
                        ));
                        skip_element(reader, buf)?;
                    }
                }
            }
            Event::Empty(_) => {}
            Event::End(e) if e.name().as_ref() == b"ObjectData" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <ObjectData> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// Consume one `<Object>`'s properties (called after its Start event;
/// returns at its End event). `xml` is the whole Document.xml source —
/// needed to extract `<Sketch>` fragments byte-exactly (they are object
/// data, not `<Property>` elements).
fn parse_object_properties(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    xml: &[u8],
    object: &mut Box<dyn DocumentObject>,
    owner: &str,
    zip: &mut zip::ZipArchive<std::fs::File>,
    report: &mut LoadReport,
) -> Result<(), FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == b"Property" => {
                let map = attrs_of(&e);
                let name = map.get("name").cloned().unwrap_or_default();
                let value = parse_property_value(reader, buf, owner, &name, &map, report)?;
                match (name.is_empty(), value) {
                    (false, Some(PropValue::Value(v))) => {
                        object.properties_mut().insert(name, v);
                    }
                    (false, Some(PropValue::Shape(shape_ref))) => {
                        match load_shape_payload(zip, &shape_ref, owner, report)? {
                            Some(shape) => attach_shape(object, shape, owner, report),
                            // Geometry not restorable: keep the parameters
                            // and stay Touched so recompute regenerates
                            // what it can.
                            None => object.set_status(ObjectStatus::Touched),
                        }
                    }
                    (true, _) => {
                        report
                            .warnings
                            .push(format!("{owner}: property without a name — skipped"));
                    }
                    (false, None) => {}
                }
            }
            Event::Empty(e) if e.name().as_ref() == b"Property" => {
                let name = attrs_of(&e).get("name").cloned().unwrap_or_default();
                report
                    .warnings
                    .push(format!("{owner}: property '{name}' has no value — skipped"));
            }
            Event::Empty(e) if e.name().as_ref() == b"Expression" => {
                let map = attrs_of(&e);
                let path = map.get("path").cloned().unwrap_or_default();
                let expression = map.get("expression").cloned().unwrap_or_default();
                if path.is_empty() || expression.is_empty() {
                    report.warnings.push(format!(
                        "{owner}: <Expression> without path or text — skipped"
                    ));
                } else if let Some(expressions) = object.expressions_mut() {
                    expressions.insert(path, expression);
                }
            }
            // The sketch content of a `Sketcher::SketchObject`: a
            // `<Sketch>` fragment that is object data, not a `<Property>`
            // (our writer emits it as a child of `<Object>`, next to
            // `<Properties>`; the sketch parser is whitespace-tolerant).
            Event::Start(e) if e.name().as_ref() == b"Sketch" => {
                let inner = raw_inner_xml(xml, reader, buf, b"Sketch")?;
                store_sketch_fragment(object, &format!("<Sketch>{inner}</Sketch>"), owner, report);
            }
            Event::Empty(e) if e.name().as_ref() == b"Sketch" => {
                // A self-closing `<Sketch/>` is an empty sketch.
                store_sketch_fragment(object, "<Sketch/>", owner, report);
            }
            Event::End(e) if e.name().as_ref() == b"Object" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml("unterminated <Object> element".to_string()));
            }
            // The enclosing <Properties> start/end and stray text.
            _ => continue,
        }
    }
}

/// Called right after a `<name>` Start event: consumes events up to the
/// matching `</name>` and returns the **raw source bytes** between the
/// two tags, so the fragment reaches the sketch parser exactly as it was
/// written (no event re-serialization, no attribute churn).
fn raw_inner_xml(
    xml: &[u8],
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    name: &[u8],
) -> Result<String, FcstdError> {
    let start = reader.buffer_position() as usize;
    let mut depth = 1usize;
    loop {
        match next_event(reader, buf)? {
            Event::Start(e) if e.name().as_ref() == name => depth += 1,
            Event::End(e) if e.name().as_ref() == name => {
                depth -= 1;
                if depth == 0 {
                    // buffer_position() is past the closing tag; the tag's
                    // own '<' is the LAST '<' in the range, so cutting there
                    // returns exactly the bytes between the two tags.
                    let end = reader.buffer_position() as usize;
                    let range = &xml[start..end];
                    let tag_start = range.iter().rposition(|&b| b == b'<').ok_or_else(|| {
                        FcstdError::Xml("malformed element: closing tag not found".to_string())
                    })?;
                    return Ok(String::from_utf8_lossy(&range[..tag_start]).into_owned());
                }
            }
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unexpected end of Document.xml".to_string(),
                ));
            }
            _ => {}
        }
    }
}

/// Parse a `<Sketch>` fragment and store it into `object` when that
/// object is a `Sketcher::SketchObject` (via `replace_sketch`, which
/// touches the object — loaded sketch content always re-solves on the
/// next recompute, exactly like every other loaded object re-executes).
/// Anything else is a recoverable defect reported through the warnings
/// channel: a malformed fragment, or a `<Sketch>` element on a non-sketch
/// object. The loader reports + continues, mirroring how it treats
/// unreadable shape payloads.
fn store_sketch_fragment(
    object: &mut Box<dyn DocumentObject>,
    fragment: &str,
    owner: &str,
    report: &mut LoadReport,
) {
    match fc_sketcher::xml::from_xml(fragment) {
        Ok(sketch) => {
            let any = object.as_any_mut();
            if let Some(sketch_object) = any.downcast_mut::<SketchObject>() {
                sketch_object.replace_sketch(sketch);
            } else {
                report.warnings.push(format!(
                    "{owner}: <Sketch> element on a non-sketch object — ignored"
                ));
            }
        }
        Err(err) => {
            report.warnings.push(format!(
                "{owner}: malformed <Sketch> fragment: {err} — sketch content skipped"
            ));
        }
    }
}

/// Parse one `<Property>`'s value element. Called after the Property Start
/// event; consumes through the Property End event and returns the value
/// (or `None` after recording a warning).
fn parse_property_value(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    owner: &str,
    prop: &str,
    prop_attrs: &BTreeMap<String, String>,
    report: &mut LoadReport,
) -> Result<Option<PropValue>, FcstdError> {
    let prop_type = prop_attrs.get("type").map_or("", String::as_str);
    loop {
        match next_event(reader, buf)? {
            // Self-closing value element, the normal case.
            Event::Empty(e) => {
                let value = parse_value_element(&e, None, owner, prop, prop_type, report)?;
                return finish_property(reader, buf, value);
            }
            // Value element with content (e.g. FreeCAD's inline
            // `<Part brep="1">CASCADE Topology ...` payloads).
            Event::Start(e) => {
                let element_name = e.name().as_ref().to_vec();
                let mut text = Vec::new();
                loop {
                    match next_event(reader, buf)? {
                        Event::Text(t) => text.extend_from_slice(&t.into_inner()),
                        Event::End(end) if end.name().as_ref() == element_name.as_slice() => break,
                        Event::Eof => {
                            return Err(FcstdError::Xml(format!(
                                "unterminated <{}> value element",
                                String::from_utf8_lossy(&element_name)
                            )));
                        }
                        _ => continue,
                    }
                }
                let value = parse_value_element(&e, Some(&text), owner, prop, prop_type, report)?;
                return finish_property(reader, buf, value);
            }
            Event::End(e) if e.name().as_ref() == b"Property" => {
                report
                    .warnings
                    .push(format!("{owner}: property '{prop}' has no value — skipped"));
                return Ok(None);
            }
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Property> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// After the value element, drain up to the closing `</Property>`.
fn finish_property(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
    value: Option<PropValue>,
) -> Result<Option<PropValue>, FcstdError> {
    loop {
        match next_event(reader, buf)? {
            Event::End(e) if e.name().as_ref() == b"Property" => return Ok(value),
            Event::Eof => {
                return Err(FcstdError::Xml(
                    "unterminated <Property> element".to_string(),
                ));
            }
            _ => continue,
        }
    }
}

/// Parse one value element (Bool/Integer/Float/String/PropertyVector/
/// PropertyPlacement/Link/LinkList/Part) by its element name, per the
/// crate's element table. `text` is the element's content when it was not
/// self-closing.
fn parse_value_element(
    e: &BytesStart<'_>,
    text: Option<&[u8]>,
    owner: &str,
    prop: &str,
    prop_type: &str,
    report: &mut LoadReport,
) -> Result<Option<PropValue>, FcstdError> {
    macro_rules! skip_warn {
        ($($arg:tt)*) => {{
            report.warnings.push(format!($($arg)*));
            return Ok(None);
        }};
    }

    let element = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let map = attrs_of(e);
    match element.as_str() {
        "Bool" => match parse_bool(map.get("value").map_or("", String::as_str)) {
            Some(v) => Ok(Some(PropValue::Value(Property::Bool(v)))),
            None => skip_warn!(
                "{owner}: property '{prop}': malformed Bool value '{}' — skipped",
                map.get("value").map_or("?", String::as_str)
            ),
        },
        "Integer" => match map.get("value").and_then(|v| v.trim().parse::<i64>().ok()) {
            Some(v) => Ok(Some(PropValue::Value(Property::Integer(v)))),
            None => skip_warn!(
                "{owner}: property '{prop}': malformed Integer value '{}' — skipped",
                map.get("value").map_or("?", String::as_str)
            ),
        },
        "Float" => match map.get("value").and_then(|v| v.trim().parse::<f64>().ok()) {
            Some(v) => Ok(Some(PropValue::Value(Property::Float(v)))),
            None => skip_warn!(
                "{owner}: property '{prop}': malformed Float value '{}' — skipped",
                map.get("value").map_or("?", String::as_str)
            ),
        },
        "String" => Ok(Some(PropValue::Value(Property::String(
            map.get("value").cloned().unwrap_or_default(),
        )))),
        "PropertyVector" => match (
            attr_f64(&map, "valueX"),
            attr_f64(&map, "valueY"),
            attr_f64(&map, "valueZ"),
        ) {
            (Some(x), Some(y), Some(z)) => {
                Ok(Some(PropValue::Value(Property::Vector(Vector3::new(x, y, z)))))
            }
            _ => skip_warn!(
                "{owner}: property '{prop}': malformed PropertyVector — skipped"
            ),
        },
        "PropertyPlacement" => match parse_placement(&map) {
            Some(p) => Ok(Some(PropValue::Value(Property::Placement(p)))),
            None => {
                report.warnings.push(format!(
                    "{owner}: property '{prop}': malformed PropertyPlacement — skipped"
                ));
                Ok(None)
            }
        },
        // FreeCAD writes single-element link lists as <Link .../>.
        "Link" => {
            let value = map.get("value").cloned().unwrap_or_default();
            let property = if prop_type == "App::PropertyLinkList" {
                Property::LinkList(vec![value])
            } else {
                Property::Link(value)
            };
            Ok(Some(PropValue::Value(property)))
        }
        "LinkList" => {
            let list = map
                .get("value")
                .map(|v| {
                    v.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(Some(PropValue::Value(Property::LinkList(list))))
        }
        "Part" => Ok(Some(PropValue::Shape(ShapeRef {
            file: map.get("file").filter(|f| !f.is_empty()).cloned(),
            inline: text.unwrap_or_default().to_vec(),
        }))),
        _ => skip_warn!(
            "{owner}: property '{prop}': unsupported value element <{element}> (type '{prop_type}') — skipped"
        ),
    }
}

/// `<PropertyPlacement>`: position + quaternion (Q0..Q3, scalar last).
/// FreeCAD also writes the equivalent A/Ox/Oy/Oz axis-angle form; use it
/// only when the quaternion is absent.
fn parse_placement(map: &BTreeMap<String, String>) -> Option<Placement> {
    let position = Vector3::new(
        attr_f64(map, "Px").unwrap_or(0.0),
        attr_f64(map, "Py").unwrap_or(0.0),
        attr_f64(map, "Pz").unwrap_or(0.0),
    );
    let rotation = if ["Q0", "Q1", "Q2", "Q3"]
        .iter()
        .any(|k| map.contains_key(*k))
    {
        normalized_rotation(Rotation {
            x: attr_f64(map, "Q0").unwrap_or(0.0),
            y: attr_f64(map, "Q1").unwrap_or(0.0),
            z: attr_f64(map, "Q2").unwrap_or(0.0),
            w: attr_f64(map, "Q3").unwrap_or(1.0),
        })
    } else if let (Some(a), Some(ox), Some(oy), Some(oz)) = (
        attr_f64(map, "A"),
        attr_f64(map, "Ox"),
        attr_f64(map, "Oy"),
        attr_f64(map, "Oz"),
    ) {
        // A is radians in FreeCAD's format.
        Rotation::from_axis_angle(Vector3::new(ox, oy, oz), a).ok()?
    } else {
        Rotation::IDENTITY
    };
    Some(Placement { position, rotation })
}

/// Re-normalize a quaternion read from a file (FreeCAD normalizes on
/// restore; near-unit inputs are unchanged).
fn normalized_rotation(r: Rotation) -> Rotation {
    let len = (r.x * r.x + r.y * r.y + r.z * r.z + r.w * r.w).sqrt();
    if len > 0.0 && (len - 1.0).abs() > 1e-12 {
        Rotation {
            x: r.x / len,
            y: r.y / len,
            z: r.z / len,
            w: r.w / len,
        }
    } else {
        r
    }
}

/// Resolve a `<Part>` reference into a shape: read the side file from the
/// zip (or the inline payload), probe its format and parse ours.
fn load_shape_payload(
    zip: &mut zip::ZipArchive<std::fs::File>,
    shape_ref: &ShapeRef,
    owner: &str,
    report: &mut LoadReport,
) -> Result<Option<TopoShape>, FcstdError> {
    let bytes: Vec<u8> = if let Some(file) = &shape_ref.file {
        match zip.by_name(file) {
            Ok(mut entry) => {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                bytes
            }
            Err(zip::result::ZipError::FileNotFound) => {
                report.warnings.push(format!(
                    "{owner}: shape file '{file}' missing from archive — geometry skipped (parameters kept)"
                ));
                return Ok(None);
            }
            Err(e) => return Err(zip_err(e)),
        }
    } else if !shape_ref.inline.is_empty() {
        shape_ref.inline.clone()
    } else {
        // An empty <Part/> is FreeCAD's marker for "no computed shape".
        // Returning None leaves the object Touched (uniform with the other
        // no-geometry paths), so recompute can regenerate it.
        return Ok(None);
    };

    match brep::probe_format(&bytes) {
        BrpFormat::FcBrep => match brep::read_shape_bytes(&bytes) {
            Ok(shape) => Ok(Some(shape)),
            Err(e) => {
                report.warnings.push(format!(
                    "{owner}: malformed FCBREP payload: {e} — geometry skipped (parameters kept)"
                ));
                Ok(None)
            }
        },
        BrpFormat::OcctBrep => {
            report.warnings.push(format!(
                "{owner}: OCCT BRep shape not supported — geometry skipped (parameters kept)"
            ));
            Ok(None)
        }
        BrpFormat::Unknown => {
            report.warnings.push(format!(
                "{owner}: unknown shape payload format — geometry skipped (parameters kept)"
            ));
            Ok(None)
        }
    }
}

/// Attach a parsed shape to the concrete feature owning it.
fn attach_shape(
    object: &mut Box<dyn DocumentObject>,
    shape: TopoShape,
    owner: &str,
    report: &mut LoadReport,
) {
    let any = object.as_any_mut();
    if let Some(f) = any.downcast_mut::<BoxFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<SphereFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<CylinderFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<ConeFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<TorusFeature>() {
        f.shape = Some(shape);
    } else if let Some(f) = any.downcast_mut::<TransformFeature>() {
        // Transform features persist their computed mesh.
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else if let Some(f) = any.downcast_mut::<BooleanFeature>() {
        // Boolean features persist their computed mesh.
        f.result = match shape {
            TopoShape::Mesh(mesh) => Some(mesh),
            other => Some(other.tessellate(DEFAULT_DEFLECTION)),
        };
    } else {
        report.warnings.push(format!(
            "{owner}: object type carries no shape field — geometry skipped"
        ));
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Escape a string for an XML attribute body.
pub(crate) fn escape_attr(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

/// Map a FreeCAD type id (e.g. `Part::Box`) to a fresh object instance.
/// Returns `None` for unknown types (the loader reports + skips).
pub(crate) fn build_object(type_id: &str, name: &str) -> Option<Box<dyn fc_app::DocumentObject>> {
    match type_id {
        "Part::Box" => Some(Box::new(BoxFeature::new(name))),
        "Part::Sphere" => Some(Box::new(SphereFeature::new(name))),
        "Part::Cylinder" => Some(Box::new(CylinderFeature::new(name))),
        "Part::Cone" => Some(Box::new(ConeFeature::new(name))),
        "Part::Torus" => Some(Box::new(TorusFeature::new(name))),
        "Part::Transform" => Some(Box::new(TransformFeature::new(name))),
        "Part::Cut" | "Part::Fuse" | "Part::Common" => BooleanKind::from_type_name(type_id)
            .map(|kind| Box::new(BooleanFeature::new(name, kind)) as _),
        "Sketcher::SketchObject" => Some(Box::new(SketchObject::new(name))),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests: Sketcher::SketchObject persistence
// ---------------------------------------------------------------------------

#[cfg(test)]
mod sketch_persistence_tests {
    use super::*;
    use fc_part::features::TransformFeature;
    use fc_sketcher::constraint::Constraint;
    use fc_sketcher::geometry::{Geometry, Point};
    use fc_sketcher::xml::from_xml;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_path(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "fcrs-woi-fcstd-{}-{}.fcstd",
            tag,
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
    }

    /// Two lines sharing a corner, the corner pinned horizontal: 8
    /// parameters, 2 (coincident) + 1 (horizontal) = 3 constraint
    /// dimensions -> DOF 5.
    fn corner_sketch() -> fc_sketcher::Sketch {
        let mut sk = fc_sketcher::Sketch::new();
        let a = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(40.5, 0.25),
        });
        let b = sk.add_geometry(Geometry::LineSegment {
            start: Point::new(40.5, 0.25),
            end: Point::new(40.25, 30.75),
        });
        sk.add_constraint(Constraint::Coincident {
            g1: a,
            pos1: fc_sketcher::PointPos::End,
            g2: b,
            pos2: fc_sketcher::PointPos::Start,
        })
        .unwrap();
        sk.add_constraint(Constraint::Horizontal { g: a }).unwrap();
        sk
    }

    /// A one-circle sketch (fresh content for import-style swaps).
    fn circle_sketch() -> fc_sketcher::Sketch {
        let mut sk = fc_sketcher::Sketch::new();
        sk.add_geometry(Geometry::Circle {
            center: Point::new(5.0, 6.0),
            radius: 2.5,
        });
        sk
    }

    fn sketch_of(doc: &Document, name: &str) -> fc_sketcher::Sketch {
        doc.get_by_name(name)
            .unwrap()
            .as_any()
            .downcast_ref::<SketchObject>()
            .unwrap()
            .sketch
            .clone()
    }

    fn read_document_xml(path: &Path) -> String {
        let file = std::fs::File::open(path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut entry = zip.by_name("Document.xml").unwrap();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        text
    }

    /// Write a `.FCStd` archive whose Document.xml is exactly `xml` (the
    /// craft-a-file style of the io integration tests).
    fn craft_fcstd(path: &Path, xml: &str) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("Document.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    /// Does `object` carry a Link/LinkList property pointing at `target`?
    fn links_to(object: &dyn DocumentObject, target: &str) -> bool {
        object.properties().values().any(|value| match value {
            Property::Link(name) => name == target,
            Property::LinkList(names) => names.iter().any(|name| name == target),
            _ => false,
        })
    }

    #[test]
    fn sketch_object_round_trips_byte_identically() {
        let path = temp_path("roundtrip");

        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        doc.recompute_all().unwrap();
        let original = sketch_of(&doc, "Sketch").to_xml();
        save_document(&doc, &path).unwrap();

        let (loaded, report) = load_document(&path).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.objects, 1);
        assert_eq!(report.skipped, 0);

        let object = loaded.get_by_name("Sketch").unwrap();
        assert_eq!(object.type_name(), "Sketcher::SketchObject");
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        // The strongest round-trip oracle: identical `to_xml()` output.
        assert_eq!(sketch_object.sketch.to_xml(), original);
        // Solve results are not persisted: loaded sketches are Touched and
        // re-solve on the next recompute.
        assert_eq!(sketch_object.solved, None);
        assert_eq!(*sketch_object.status(), ObjectStatus::Touched);
        assert_eq!(
            sketch_object.container.get_string("Label"),
            Some("Sketch".into())
        );
        assert_eq!(
            sketch_object.container.get_placement("Placement"),
            Some(Placement::IDENTITY)
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn empty_sketch_round_trips() {
        let path = temp_path("empty");
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::new("Sketch")));
        doc.recompute_all().unwrap();
        save_document(&doc, &path).unwrap();

        let (loaded, report) = load_document(&path).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let object = loaded.get_by_name("Sketch").unwrap();
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        assert_eq!(sketch_object.sketch.to_xml(), "<Sketch>\n</Sketch>\n");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn document_xml_embeds_the_sketch_fragment_as_object_data() {
        let path = temp_path("embed");
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        doc.recompute_all().unwrap();
        save_document(&doc, &path).unwrap();

        let xml = read_document_xml(&path);
        // One <Sketch> element, indented as a child of <Object> (3 spaces),
        // with its own children kept at the fragment's relative indent.
        assert!(xml.contains("   <Sketch>\n"), "{xml}");
        assert!(xml.contains("     <Geometry type=\"1\" "), "{xml}");
        assert!(xml.contains("     <Constraint type=\"0\" "), "{xml}");
        assert!(xml.contains("   </Sketch>\n"), "{xml}");
        // It sits after the object's <Properties> (it is object data, not
        // a property) and is not itself wrapped in a <Property> element.
        let object_section = xml.split("<Object name=\"Sketch\">").nth(1).unwrap();
        let properties_end = object_section.find("</Properties>").unwrap();
        let sketch_start = object_section.find("<Sketch>").unwrap();
        assert!(properties_end < sketch_start);
        assert!(!xml.contains("<Property name=\"Sketch\""));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sketch_and_box_survive_a_round_trip_together() {
        let path = temp_path("mixed");
        let mut doc = Document::new("test");
        doc.add(Box::new(BoxFeature::new("Base"))); // default 10x10x10
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        doc.recompute_all().unwrap();
        // The solve may have adjusted the geometry; compare against the
        // sketch exactly as it was saved.
        let original = sketch_of(&doc, "Sketch").to_xml();
        save_document(&doc, &path).unwrap();

        let (loaded, report) = load_document(&path).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.objects, 2);

        let object = loaded.get_by_name("Base").unwrap();
        assert_eq!(object.type_name(), "Part::Box");
        assert_eq!(
            object.properties().get("Length"),
            Some(&Property::Float(10.0))
        );
        assert_eq!(*object.status(), ObjectStatus::Valid);

        let object = loaded.get_by_name("Sketch").unwrap();
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        assert_eq!(sketch_object.sketch.to_xml(), original);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn malformed_sketch_fragment_warns_and_the_rest_loads() {
        let path = temp_path("malformed");
        craft_fcstd(
            &path,
            r##"<Document SchemaVersion="4" ProgramVersion="test" FileVersion="1">
 <Objects Count="2" Dependencies="1">
  <Object type="Part::Box" name="Box" id="1"/>
  <Object type="Sketcher::SketchObject" name="Sketch" id="2"/>
 </Objects>
 <ObjectData Count="2">
  <Object name="Box">
   <Properties Count="1">
    <Property name="Length" type="App::PropertyFloat">
     <Float value="3.5"/>
    </Property>
   </Properties>
  </Object>
  <Object name="Sketch">
   <Properties Count="1">
    <Property name="Label" type="App::PropertyString">
     <String value="Mysketch"/>
    </Property>
   </Properties>
   <Sketch>
    <Bogus/>
   </Sketch>
  </Object>
 </ObjectData>
</Document>
"##,
        );

        let (doc, report) = load_document(&path).unwrap();
        // The bad fragment is a warning, not an error: the loader reports
        // and continues.
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Sketch") && w.contains("malformed <Sketch> fragment")),
            "{:?}",
            report.warnings
        );
        assert_eq!(report.objects, 2);
        assert_eq!(report.skipped, 0);

        // The other object is fully intact.
        let object = doc.get_by_name("Box").unwrap();
        assert_eq!(
            object.properties().get("Length"),
            Some(&Property::Float(3.5))
        );

        // The sketch object exists with its label but no content.
        let object = doc.get_by_name("Sketch").unwrap();
        let sketch_object = object.as_any().downcast_ref::<SketchObject>().unwrap();
        assert_eq!(
            sketch_object.container.get_string("Label"),
            Some("Mysketch".into())
        );
        assert!(sketch_object.sketch.geometry().is_empty());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sketch_element_on_a_non_sketch_object_is_ignored_with_a_warning() {
        let path = temp_path("stray");
        craft_fcstd(
            &path,
            r##"<Document SchemaVersion="4" ProgramVersion="test" FileVersion="1">
 <Objects Count="1" Dependencies="1">
  <Object type="Part::Box" name="Box" id="1"/>
 </Objects>
 <ObjectData Count="1">
  <Object name="Box">
   <Properties Count="0">
   </Properties>
   <Sketch>
    <Geometry type="0" x="1" y="2"/>
   </Sketch>
  </Object>
 </ObjectData>
</Document>
"##,
        );

        let (doc, report) = load_document(&path).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Box") && w.contains("<Sketch> element on a non-sketch object")),
            "{:?}",
            report.warnings
        );
        assert_eq!(report.objects, 1);
        assert!(doc.get_by_name("Box").is_some());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_hook_swaps_sketch_content_and_touches_dependents() {
        let path = temp_path("hook");

        // Sketch + a dependent that links to it (Part::Transform with
        // Source=Sketch). The transform cannot execute (a sketch has no
        // computed mesh), so it sits in Error — which is fine here: the
        // hook must touch it so the next recompute re-evaluates it.
        let mut doc = Document::new("test");
        doc.add(Box::new(SketchObject::with_sketch(
            "Sketch",
            corner_sketch(),
        )));
        let mut transform = TransformFeature::new("Dep");
        transform
            .container
            .set("Source", Property::Link("Sketch".to_string()));
        doc.add(Box::new(transform));
        let _ = doc.recompute_all(); // transform fails on purpose

        save_document(&doc, &path).unwrap();
        let original_status = doc.get_by_name("Dep").unwrap().status().clone();
        assert!(matches!(original_status, ObjectStatus::Error(_)));

        // Import-style hook: swap the sketch, touch everything linking to
        // it (this is exactly what `sketch-import` does).
        let fresh = circle_sketch();
        let fresh_xml = fresh.to_xml();
        let mut hook = |name: &str, object: &mut dyn DocumentObject| {
            if name == "Sketch" {
                object
                    .as_any_mut()
                    .downcast_mut::<SketchObject>()
                    .expect("target must be a SketchObject")
                    .replace_sketch(fresh.clone());
            } else if links_to(object, "Sketch") {
                object.set_status(ObjectStatus::Touched);
            }
        };
        let (loaded, report) = load_document_with_hook(&path, Some(&mut hook)).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        // The content swap took, byte-identically.
        assert_eq!(sketch_of(&loaded, "Sketch").to_xml(), fresh_xml);
        let object = loaded.get_by_name("Sketch").unwrap();
        assert_eq!(*object.status(), ObjectStatus::Touched);
        // The dependent was touched (its persisted status was Error).
        let object = loaded.get_by_name("Dep").unwrap();
        assert_eq!(*object.status(), ObjectStatus::Touched);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parsed_fragments_round_trip_through_the_parser_too() {
        // Save-side fragment -> from_xml -> to_xml is the identity, the
        // property the whole persistence story leans on.
        let sketch = corner_sketch();
        let xml = sketch.to_xml();
        let parsed = from_xml(&xml).unwrap();
        assert_eq!(parsed.to_xml(), xml);
        assert_eq!(parsed.dof(), sketch.dof());
    }
}
