//! FCBREP: our own stable text serialization for `TopoShape`, stored inside
//! `.FCStd` archives next to `Document.xml` (the role OCCT `.brp` files
//! play in FreeCAD).
//!
//! Grammar (version 1, line-oriented, UTF-8):
//!
//! ```text
//! FCBREP 1
//! kind Box|Sphere|Cylinder|Cone|Torus|Mesh
//! param <name> <float>          ; zero or more, analytic kinds
//! mesh <vertex-count> <triangle-count>   ; only for kind Mesh
//! v <x> <y> <z>
//! t <i> <j> <k>
//! end
//! ```
//!
//! Parameter names per kind: `Box`: length, width, height · `Sphere`:
//! radius · `Cylinder`: radius, height · `Cone`: radius1, radius2, height
//! · `Torus`: radius1, radius2. Floats are written with Rust's `{}`,
//! which emits the shortest round-trip form, and read back with full
//! `f64` precision. The parser is line-based and strict: every deviation
//! (wrong token count, unknown kind, unknown/duplicate/missing
//! parameter, out-of-range index, non-finite number, content after
//! `end`) is a [`BrpError::Malformed`] carrying the 1-based line number.
//!
//! Real FreeCAD `.brp` payloads are OCCT BRep ("DBRep_DrawableShape" /
//! "CASCADE Topology" headers). `probe_format` recognizes them so the
//! loader can report them honestly as unsupported instead of misparsing.

use std::collections::BTreeMap;

use crate::shape::{Mesh, TopoShape};
use fc_base::Vector3;

/// Serialization outcome of a byte payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrpFormat {
    /// Ours.
    FcBrep,
    /// OpenCascade BRep — readable only by OCCT; reported as unsupported.
    OcctBrep,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrpError {
    /// Structural problem with a payload we believed was ours.
    Malformed {
        line: usize,
        reason: String,
    },
    /// OCCT payloads cannot be loaded by this kernel (by design).
    UnsupportedOcct,
    UnknownFormat,
}

impl std::fmt::Display for BrpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrpError::Malformed { line, reason } => {
                write!(f, "malformed FCBREP at line {line}: {reason}")
            }
            BrpError::UnsupportedOcct => {
                write!(f, "OCCT BRep payload: not supported by this kernel")
            }
            BrpError::UnknownFormat => write!(f, "unknown shape payload format"),
        }
    }
}

impl std::error::Error for BrpError {}

/// Sniff a payload (typically the first bytes of a zip entry).
pub fn probe_format(data: &[u8]) -> BrpFormat {
    let head = String::from_utf8_lossy(&data[..data.len().min(128)]);
    if head.starts_with("FCBREP") {
        BrpFormat::FcBrep
    } else if head.contains("DBRep_DrawableShape") || head.contains("CASCADE Topology") {
        BrpFormat::OcctBrep
    } else {
        BrpFormat::Unknown
    }
}

/// Serialize `shape` in FCBREP v1. Mesh variants are written with their
/// full geometry; analytic variants as parameters.
pub fn write_shape<W: std::io::Write>(w: &mut W, shape: &TopoShape) -> std::io::Result<()> {
    writeln!(w, "FCBREP 1")?;
    match shape {
        TopoShape::Box {
            length,
            width,
            height,
        } => {
            writeln!(w, "kind Box")?;
            writeln!(w, "param length {length}")?;
            writeln!(w, "param width {width}")?;
            writeln!(w, "param height {height}")?;
        }
        TopoShape::Sphere { radius } => {
            writeln!(w, "kind Sphere")?;
            writeln!(w, "param radius {radius}")?;
        }
        TopoShape::Cylinder { radius, height } => {
            writeln!(w, "kind Cylinder")?;
            writeln!(w, "param radius {radius}")?;
            writeln!(w, "param height {height}")?;
        }
        TopoShape::Cone {
            radius1,
            radius2,
            height,
        } => {
            writeln!(w, "kind Cone")?;
            writeln!(w, "param radius1 {radius1}")?;
            writeln!(w, "param radius2 {radius2}")?;
            writeln!(w, "param height {height}")?;
        }
        TopoShape::Torus { radius1, radius2 } => {
            writeln!(w, "kind Torus")?;
            writeln!(w, "param radius1 {radius1}")?;
            writeln!(w, "param radius2 {radius2}")?;
        }
        TopoShape::Mesh(mesh) => {
            writeln!(w, "kind Mesh")?;
            writeln!(w, "mesh {} {}", mesh.vertices.len(), mesh.triangles.len())?;
            for v in &mesh.vertices {
                writeln!(w, "v {} {} {}", v.x, v.y, v.z)?;
            }
            for t in &mesh.triangles {
                writeln!(w, "t {} {} {}", t[0], t[1], t[2])?;
            }
        }
    }
    writeln!(w, "end")
}

/// Parse an FCBREP v1 payload (bytes). OCCT/unknown payloads return the
/// corresponding error; use [`probe_format`] first when you want to
/// distinguish them.
pub fn read_shape_bytes(data: &[u8]) -> Result<TopoShape, BrpError> {
    match probe_format(data) {
        BrpFormat::FcBrep => parse_fcbrp(data),
        BrpFormat::OcctBrep => Err(BrpError::UnsupportedOcct),
        BrpFormat::Unknown => Err(BrpError::UnknownFormat),
    }
}

fn malformed(line: usize, reason: impl Into<String>) -> BrpError {
    BrpError::Malformed {
        line,
        reason: reason.into(),
    }
}

/// Strict FCBREP v1 parser (payload already probed as ours).
fn parse_fcbrp(data: &[u8]) -> Result<TopoShape, BrpError> {
    let text = std::str::from_utf8(data).map_err(|_| malformed(1, "payload is not valid UTF-8"))?;
    let mut reader = LineReader::new(text);

    // Line 1: "FCBREP 1".
    let (line, header) = reader
        .next_line()
        .ok_or_else(|| malformed(1, "empty payload"))?;
    match header.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["FCBREP", "1"] => {}
        ["FCBREP", version] => {
            return Err(malformed(
                line,
                format!("unsupported FCBREP version '{version}'"),
            ))
        }
        _ => return Err(malformed(line, "expected 'FCBREP <version>'")),
    }

    // Line 2: "kind <name>".
    let (line, kind_line) = reader.next_line().ok_or_else(|| {
        malformed(
            reader.last_line(),
            "unexpected end of payload, expected 'kind <name>'",
        )
    })?;
    let kind = match kind_line.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["kind", name] => *name,
        _ => return Err(malformed(line, "expected 'kind <name>'")),
    };

    let shape = match kind {
        "Mesh" => parse_mesh_body(&mut reader)?,
        "Box" | "Sphere" | "Cylinder" | "Cone" | "Torus" => parse_analytic_body(&mut reader, kind)?,
        other => return Err(malformed(line, format!("unknown kind '{other}'"))),
    };

    // Nothing may follow "end".
    if let Some((line, extra)) = reader.next_line() {
        let shown: String = extra.chars().take(30).collect();
        return Err(malformed(
            line,
            format!("unexpected content after 'end': '{shown}'"),
        ));
    }
    Ok(shape)
}

/// `param` lines up to `end`, then the fully-populated analytic shape.
/// `kind` is one of the analytic kind names (pre-validated).
fn parse_analytic_body(reader: &mut LineReader<'_>, kind: &str) -> Result<TopoShape, BrpError> {
    const BOX_PARAMS: &[&str] = &["length", "width", "height"];
    const SPHERE_PARAMS: &[&str] = &["radius"];
    const CYLINDER_PARAMS: &[&str] = &["radius", "height"];
    const CONE_PARAMS: &[&str] = &["radius1", "radius2", "height"];
    const TORUS_PARAMS: &[&str] = &["radius1", "radius2"];

    let allowed: &[&str] = match kind {
        "Box" => BOX_PARAMS,
        "Sphere" => SPHERE_PARAMS,
        "Cylinder" => CYLINDER_PARAMS,
        "Cone" => CONE_PARAMS,
        _ => TORUS_PARAMS,
    };

    let mut params: BTreeMap<&str, f64> = BTreeMap::new();
    let end_line;
    loop {
        let (line, body) = reader.next_line().ok_or_else(|| {
            malformed(
                reader.last_line(),
                "unexpected end of payload, expected 'end'",
            )
        })?;
        match body.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["end"] => {
                end_line = line;
                break;
            }
            ["param", name, value] => {
                if !allowed.contains(name) {
                    return Err(malformed(
                        line,
                        format!("unexpected parameter '{name}' for kind {kind}"),
                    ));
                }
                let value: f64 = value
                    .parse()
                    .map_err(|_| malformed(line, format!("invalid float '{value}'")))?;
                if !value.is_finite() {
                    return Err(malformed(line, format!("parameter '{name}' is not finite")));
                }
                if params.insert(*name, value).is_some() {
                    return Err(malformed(line, format!("duplicate parameter '{name}'")));
                }
            }
            _ => {
                return Err(malformed(line, "expected 'param <name> <float>' or 'end'"));
            }
        }
    }
    for &name in allowed {
        if !params.contains_key(name) {
            return Err(malformed(
                end_line,
                format!("missing parameter '{name}' for kind {kind}"),
            ));
        }
    }
    let shape = match kind {
        "Box" => TopoShape::Box {
            length: params["length"],
            width: params["width"],
            height: params["height"],
        },
        "Sphere" => TopoShape::Sphere {
            radius: params["radius"],
        },
        "Cylinder" => TopoShape::Cylinder {
            radius: params["radius"],
            height: params["height"],
        },
        "Cone" => TopoShape::Cone {
            radius1: params["radius1"],
            radius2: params["radius2"],
            height: params["height"],
        },
        _ => TopoShape::Torus {
            radius1: params["radius1"],
            radius2: params["radius2"],
        },
    };
    Ok(shape)
}

/// `mesh` header line, then `v`/`t` lines, then `end`.
fn parse_mesh_body(reader: &mut LineReader<'_>) -> Result<TopoShape, BrpError> {
    let (line, header) = reader.next_line().ok_or_else(|| {
        malformed(
            reader.last_line(),
            "unexpected end of payload, expected 'mesh <vcount> <tcount>'",
        )
    })?;
    let (vcount, tcount) = match header.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["mesh", v, t] => (
            v.parse::<usize>()
                .map_err(|_| malformed(line, format!("invalid vertex count '{v}'")))?,
            t.parse::<usize>()
                .map_err(|_| malformed(line, format!("invalid triangle count '{t}'")))?,
        ),
        _ => {
            return Err(malformed(
                line,
                "expected 'mesh <vertex-count> <triangle-count>'",
            ))
        }
    };

    // No with_capacity(vcount): the header is untrusted, and the strict
    // per-line loop below fails long before any oversized allocation.
    let mut vertices = Vec::new();
    for read in 0..vcount {
        let (line, body) = reader.next_line().ok_or_else(|| {
            malformed(
                reader.last_line(),
                "unexpected end of payload inside the vertex list",
            )
        })?;
        let mut parts = body.split_whitespace();
        match parts.next() {
            Some("v") => vertices.push(parse_vertex(parts, line)?),
            _ => {
                return Err(malformed(
                    line,
                    format!("expected 'v <x> <y> <z>' ({read}/{vcount} vertices read)"),
                ))
            }
        }
    }

    let mut triangles = Vec::new();
    for read in 0..tcount {
        let (line, body) = reader.next_line().ok_or_else(|| {
            malformed(
                reader.last_line(),
                "unexpected end of payload inside the triangle list",
            )
        })?;
        let mut parts = body.split_whitespace();
        match parts.next() {
            Some("t") => triangles.push(parse_triangle(parts, line, vcount)?),
            _ => {
                return Err(malformed(
                    line,
                    format!("expected 't <i> <j> <k>' ({read}/{tcount} triangles read)"),
                ))
            }
        }
    }

    let (line, tail) = reader.next_line().ok_or_else(|| {
        malformed(
            reader.last_line(),
            "unexpected end of payload, expected 'end'",
        )
    })?;
    match tail.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["end"] => {}
        _ => return Err(malformed(line, "expected 'end'")),
    }
    Ok(TopoShape::Mesh(Mesh::new(vertices, triangles)))
}

/// Shared helper: parse one `v x y z` line. (Keep private helpers near
/// their callers.) Receives the tokens *after* the `v` tag.
fn parse_vertex(parts: std::str::SplitWhitespace<'_>, line: usize) -> Result<Vector3, BrpError> {
    let tokens: Vec<&str> = parts.collect();
    if tokens.len() != 3 {
        return Err(malformed(
            line,
            format!("vertex expects 3 coordinates, got {}", tokens.len()),
        ));
    }
    let mut out = [0.0f64; 3];
    for (slot, token) in out.iter_mut().zip(tokens) {
        let value: f64 = token
            .parse()
            .map_err(|_| malformed(line, format!("invalid coordinate '{token}'")))?;
        if !value.is_finite() {
            return Err(malformed(
                line,
                format!("coordinate '{token}' is not finite"),
            ));
        }
        *slot = value;
    }
    Ok(Vector3::new(out[0], out[1], out[2]))
}

/// `t i j k` line (tokens after the `t` tag); indices must reference
/// existing vertices.
fn parse_triangle(
    parts: std::str::SplitWhitespace<'_>,
    line: usize,
    vertex_count: usize,
) -> Result<[u32; 3], BrpError> {
    let tokens: Vec<&str> = parts.collect();
    if tokens.len() != 3 {
        return Err(malformed(
            line,
            format!("triangle expects 3 indices, got {}", tokens.len()),
        ));
    }
    let mut out = [0u32; 3];
    for (slot, token) in out.iter_mut().zip(tokens) {
        let index: u32 = token
            .parse()
            .map_err(|_| malformed(line, format!("invalid vertex index '{token}'")))?;
        if index as usize >= vertex_count {
            return Err(malformed(
                line,
                format!("vertex index {index} out of range (mesh has {vertex_count} vertices)"),
            ));
        }
        *slot = index;
    }
    Ok(out)
}

/// 1-based line cursor over the payload; remembers where it stopped so
/// truncation errors can point at a real line.
struct LineReader<'a> {
    lines: std::str::Lines<'a>,
    next_no: usize,
}

impl<'a> LineReader<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            lines: text.lines(),
            next_no: 1,
        }
    }

    fn next_line(&mut self) -> Option<(usize, &'a str)> {
        let line = self.lines.next()?;
        let no = self.next_no;
        self.next_no += 1;
        Some((no, line))
    }

    /// Number of the last line actually seen (1 when nothing was read).
    fn last_line(&self) -> usize {
        self.next_no.saturating_sub(1).max(1)
    }
}
