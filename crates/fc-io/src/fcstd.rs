//! `.FCStd` reader/writer. See the crate docs for the format.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use fc_app::{Document, DocumentObject, ObjectStatus, Property};
use fc_base::{Placement, Rotation, Vector3};
use fc_part::brep::{self, BrpFormat};
use fc_part::features::{
    BoxFeature, ConeFeature, CylinderFeature, SphereFeature, TorusFeature, TransformFeature,
};
use fc_part::{TopoShape, DEFAULT_DEFLECTION};
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
    parse_document(&xml_bytes, &mut zip, &mut doc, &mut report)?;
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
                    parse_object_data_section(&mut reader, &mut buf, &mut built, zip, report)?
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
        if let Some(object) = object {
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
                            parse_object_properties(reader, buf, object, &owner, zip, report)?;
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
/// returns at its End event).
fn parse_object_properties(
    reader: &mut XmlReader<'_>,
    buf: &mut Vec<u8>,
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
            Event::End(e) if e.name().as_ref() == b"Object" => return Ok(()),
            Event::Eof => {
                return Err(FcstdError::Xml("unterminated <Object> element".to_string()));
            }
            // The enclosing <Properties> start/end and stray text.
            _ => continue,
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
        _ => None,
    }
}
