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
use crate::topology::{
    validate_curve, validate_surface, Curve, ExactBRep, Surface, TopoEdge, TopoFace, TopoVertex,
    TopoWire, WireEdge,
};
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
        ["FCBREP", "2"] => {
            return Err(malformed(
                line,
                "FCBREP v2 payloads are structural BReps — read them with read_brep_bytes",
            ))
        }
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

// ===========================================================================
// FCBREP v2 — the structural BRep payload (K1)
// ===========================================================================
//
// Grammar (version 2, line-oriented, UTF-8, strict):
//
// ```text
// FCBREP 2
// vertices <n>
// v <x> <y> <z>                            ; x n
// edges <n>
// e <curve> <t0> <t1> <start-vertex> <end-vertex>   ; x n
// faces <n>
// f <surface> <umin> <umax> <vmin> <vmax> <wire-count>  ; x n
// w <edge-count> (<edge-index> <fwd>) ...   ; x wire-count
// end
// ```
//
// Curves (one line each, tokens after `e`):
//
// ```text
// line <origin:3> <direction:3>
// circle <center:3> <axis:3> <ref_dir:3> <radius>
// ellipse <center:3> <axis:3> <major_dir:3> <major> <minor>
// bspline <degree> <nknots> <npts> <rational:0|1>
//         <knots ×nknots> <coords ×(3·npts)> [<weights ×npts>]
// ```
//
// Surfaces (tokens after `f`):
//
// ```text
// plane <origin:3> <normal:3> <x_dir:3>
// cylinder <origin:3> <axis:3> <ref_dir:3> <radius>
// cone <origin:3> <axis:3> <ref_dir:3> <semi_angle> <ref_radius>
// sphere <center:3> <axis:3> <ref_dir:3> <radius>
// torus <center:3> <axis:3> <ref_dir:3> <major> <minor>
// bspline <u_degree> <v_degree> <nuknots> <nvknots> <npu> <npv> <rational:0|1>
//         <uknots> <vknots> <coords ×(3·npu·npv)> [<weights ×(npu·npv)>]
// ```
//
// Floats print with Rust's `{}` (shortest round-trip form), so a
// write→read→write cycle is byte-identical. The parser is strict and
// line-numbered like v1: every deviation is a `Malformed` error.

/// Serialize `brep` as FCBREP v2 (the structural BRep payload).
pub fn write_brep<W: std::io::Write>(w: &mut W, brep: &ExactBRep) -> std::io::Result<()> {
    writeln!(w, "FCBREP 2")?;
    writeln!(w, "vertices {}", brep.vertices.len())?;
    for v in &brep.vertices {
        let p = v.point;
        writeln!(w, "v {} {} {}", p.x, p.y, p.z)?;
    }
    writeln!(w, "edges {}", brep.edges.len())?;
    for e in &brep.edges {
        write!(w, "e ")?;
        write_curve(w, &e.curve)?;
        writeln!(w, " {} {} {} {}", e.range.0, e.range.1, e.start, e.end)?;
    }
    writeln!(w, "faces {}", brep.faces.len())?;
    for f in &brep.faces {
        write!(w, "f ")?;
        write_surface(w, &f.surface)?;
        let [umin, umax, vmin, vmax] = f.uv_bounds;
        writeln!(w, " {} {} {} {} {}", umin, umax, vmin, vmax, f.wires.len())?;
        for wire in &f.wires {
            write!(w, "w {}", wire.edges.len())?;
            for we in &wire.edges {
                write!(w, " {} {}", we.edge, if we.forward { 1 } else { 0 })?;
            }
            writeln!(w)?;
        }
    }
    writeln!(w, "end")
}

fn write_curve<W: std::io::Write>(w: &mut W, c: &Curve) -> std::io::Result<()> {
    match c {
        Curve::Line { origin, direction } => {
            write!(
                w,
                "line {} {} {} {} {} {}",
                origin.x, origin.y, origin.z, direction.x, direction.y, direction.z
            )
        }
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => write!(
            w,
            "circle {} {} {} {} {} {} {} {} {} {}",
            center.x,
            center.y,
            center.z,
            axis.x,
            axis.y,
            axis.z,
            ref_dir.x,
            ref_dir.y,
            ref_dir.z,
            radius
        ),
        Curve::Ellipse {
            center,
            axis,
            major_dir,
            major_radius,
            minor_radius,
        } => write!(
            w,
            "ellipse {} {} {} {} {} {} {} {} {} {} {}",
            center.x,
            center.y,
            center.z,
            axis.x,
            axis.y,
            axis.z,
            major_dir.x,
            major_dir.y,
            major_dir.z,
            major_radius,
            minor_radius
        ),
        Curve::Bspline {
            degree,
            knots,
            points,
            weights,
        } => {
            write!(
                w,
                "bspline {} {} {} {}",
                degree,
                knots.len(),
                points.len(),
                if weights.is_some() { 1 } else { 0 }
            )?;
            for k in knots {
                write!(w, " {}", k)?;
            }
            for p in points {
                write!(w, " {} {} {}", p.x, p.y, p.z)?;
            }
            if let Some(ws) = weights {
                for x in ws {
                    write!(w, " {}", x)?;
                }
            }
            Ok(())
        }
    }
}

fn write_surface<W: std::io::Write>(w: &mut W, s: &Surface) -> std::io::Result<()> {
    let v3w = |w: &mut W, v: &Vector3| write!(w, "{} {} {}", v.x, v.y, v.z);
    match s {
        Surface::Plane {
            origin,
            normal,
            x_dir,
        } => {
            write!(w, "plane ")?;
            v3w(w, origin)?;
            write!(w, " ")?;
            v3w(w, normal)?;
            write!(w, " ")?;
            v3w(w, x_dir)
        }
        Surface::Cylinder {
            origin,
            axis,
            ref_dir,
            radius,
        } => {
            write!(w, "cylinder ")?;
            v3w(w, origin)?;
            write!(w, " ")?;
            v3w(w, axis)?;
            write!(w, " ")?;
            v3w(w, ref_dir)?;
            write!(w, " {}", radius)
        }
        Surface::Cone {
            origin,
            axis,
            ref_dir,
            semi_angle,
            ref_radius,
        } => {
            write!(w, "cone ")?;
            v3w(w, origin)?;
            write!(w, " ")?;
            v3w(w, axis)?;
            write!(w, " ")?;
            v3w(w, ref_dir)?;
            write!(w, " {} {}", semi_angle, ref_radius)
        }
        Surface::Sphere {
            center,
            axis,
            ref_dir,
            radius,
        } => {
            write!(w, "sphere ")?;
            v3w(w, center)?;
            write!(w, " ")?;
            v3w(w, axis)?;
            write!(w, " ")?;
            v3w(w, ref_dir)?;
            write!(w, " {}", radius)
        }
        Surface::Torus {
            center,
            axis,
            ref_dir,
            major_radius,
            minor_radius,
        } => {
            write!(w, "torus ")?;
            v3w(w, center)?;
            write!(w, " ")?;
            v3w(w, axis)?;
            write!(w, " ")?;
            v3w(w, ref_dir)?;
            write!(w, " {} {}", major_radius, minor_radius)
        }
        Surface::Bspline {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            points,
            weights,
        } => {
            write!(
                w,
                "bspline {} {} {} {} {} {} {}",
                u_degree,
                v_degree,
                u_knots.len(),
                v_knots.len(),
                points.len(),
                points[0].len(),
                if weights.is_some() { 1 } else { 0 }
            )?;
            for k in u_knots {
                write!(w, " {}", k)?;
            }
            for k in v_knots {
                write!(w, " {}", k)?;
            }
            for row in points {
                for p in row {
                    write!(w, " {} {} {}", p.x, p.y, p.z)?;
                }
            }
            if let Some(ws) = weights {
                for row in ws {
                    for x in row {
                        write!(w, " {}", x)?;
                    }
                }
            }
            Ok(())
        }
    }
}

/// Parse an FCBREP v2 structural BRep payload (bytes). OCCT/unknown
/// payloads and v1 primitive-shape payloads return the corresponding
/// errors.
pub fn read_brep_bytes(data: &[u8]) -> Result<ExactBRep, BrpError> {
    match probe_format(data) {
        BrpFormat::FcBrep => parse_fcbrp_v2(data),
        BrpFormat::OcctBrep => Err(BrpError::UnsupportedOcct),
        BrpFormat::Unknown => Err(BrpError::UnknownFormat),
    }
}

fn parse_fcbrp_v2(data: &[u8]) -> Result<ExactBRep, BrpError> {
    let text = std::str::from_utf8(data).map_err(|_| malformed(1, "payload is not valid UTF-8"))?;
    let mut reader = LineReader::new(text);

    // Line 1: "FCBREP 2".
    let (line, header) = reader
        .next_line()
        .ok_or_else(|| malformed(1, "empty payload"))?;
    match header.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["FCBREP", "2"] => {}
        ["FCBREP", "1"] => {
            return Err(malformed(
                line,
                "FCBREP v1 payloads are primitive shapes — read them with read_shape_bytes",
            ))
        }
        ["FCBREP", version] => {
            return Err(malformed(
                line,
                format!("unsupported FCBREP version '{version}'"),
            ))
        }
        _ => return Err(malformed(line, "expected 'FCBREP <version>'")),
    }

    // "vertices <n>" then n vertex lines.
    let nv = read_count(&mut reader, "vertices")?;
    let mut vertices = Vec::new();
    for _ in 0..nv {
        let (line, body) = reader.next_line().ok_or_else(|| {
            malformed(
                reader.last_line(),
                "unexpected end of payload inside the vertex list",
            )
        })?;
        let toks = body.split_whitespace().collect::<Vec<_>>();
        if toks.len() != 4 || toks[0] != "v" {
            return Err(malformed(line, "expected 'v <x> <y> <z>'"));
        }
        vertices.push(TopoVertex {
            point: parse_vec3(&toks[1..4], line)?,
        });
    }

    // "edges <n>" then n edge lines.
    let ne = read_count(&mut reader, "edges")?;
    let mut edges = Vec::new();
    let mut edge_lines = Vec::new();
    for _ in 0..ne {
        let (line, body) = reader.next_line().ok_or_else(|| {
            malformed(
                reader.last_line(),
                "unexpected end of payload inside the edge list",
            )
        })?;
        edge_lines.push(line);
        let curve_toks = body.split_whitespace().collect::<Vec<_>>();
        let (curve, tail) = parse_curve_tokens(&curve_toks, line)?;
        if tail.len() != 4 {
            return Err(malformed(
                line,
                format!(
                    "edge expects <t0> <t1> <start> <end> after the curve, found {} more tokens",
                    tail.len()
                ),
            ));
        }
        let t0 = parse_f64(tail[0], line)?;
        let t1 = parse_f64(tail[1], line)?;
        if t0 >= t1 {
            return Err(malformed(
                line,
                format!("edge parameter range must satisfy t0 < t1 (got {t0}..{t1})"),
            ));
        }
        let start = parse_index(tail[2], line, nv, "vertex")?;
        let end = parse_index(tail[3], line, nv, "vertex")?;
        edges.push(TopoEdge {
            curve,
            range: (t0, t1),
            start,
            end,
        });
    }

    // "faces <n>" then per face: one face line + its wire lines.
    let nf = read_count(&mut reader, "faces")?;
    let mut faces = Vec::new();
    let mut face_lines = Vec::new();
    for _ in 0..nf {
        let (line, body) = reader.next_line().ok_or_else(|| {
            malformed(
                reader.last_line(),
                "unexpected end of payload inside the face list",
            )
        })?;
        face_lines.push(line);
        let toks = body.split_whitespace().collect::<Vec<_>>();
        let (surface, tail) = parse_surface_tokens(&toks, line)?;
        if tail.len() != 5 {
            return Err(malformed(
                line,
                format!(
                    "face expects <umin> <umax> <vmin> <vmax> <wire-count> after the surface, found {} more tokens",
                    tail.len()
                ),
            ));
        }
        let mut uv = [0.0f64; 4];
        for (slot, tok) in uv.iter_mut().zip(&tail[0..4]) {
            *slot = parse_f64(tok, line)?;
        }
        if uv[0] >= uv[1] || uv[2] >= uv[3] {
            return Err(malformed(
                line,
                format!(
                    "face UV bounds must satisfy umin < umax and vmin < vmax (got u {}..{}, v {}..{})",
                    uv[0], uv[1], uv[2], uv[3]
                ),
            ));
        }
        let nwires: usize = tail[4]
            .parse()
            .map_err(|_| malformed(line, format!("invalid wire count '{}'", tail[4])))?;
        let mut wires = Vec::with_capacity(nwires);
        for _ in 0..nwires {
            let (wline, wbody) = reader.next_line().ok_or_else(|| {
                malformed(
                    reader.last_line(),
                    "unexpected end of payload inside a face's wire list",
                )
            })?;
            let wtoks = wbody.split_whitespace().collect::<Vec<_>>();
            if wtoks.len() < 2 || wtoks[0] != "w" {
                return Err(malformed(wline, "expected 'w <edge-count> …'"));
            }
            let k: usize = wtoks[1]
                .parse()
                .map_err(|_| malformed(wline, format!("invalid wire edge count '{}'", wtoks[1])))?;
            if wtoks.len() != 2 + 2 * k {
                return Err(malformed(
                    wline,
                    format!(
                        "wire declares {k} edge uses but the line carries {} tokens",
                        (wtoks.len() - 2) / 2
                    ),
                ));
            }
            let mut uses = Vec::with_capacity(k);
            for i in 0..k {
                let ei = parse_index(wtoks[2 + 2 * i], wline, ne, "edge")?;
                let fwd = match wtoks[3 + 2 * i] {
                    "1" => true,
                    "0" => false,
                    other => {
                        return Err(malformed(
                            wline,
                            format!("edge orientation must be 0 or 1, found '{other}'"),
                        ))
                    }
                };
                uses.push(WireEdge {
                    edge: ei,
                    forward: fwd,
                });
            }
            wires.push(TopoWire { edges: uses });
        }
        faces.push(TopoFace {
            surface,
            uv_bounds: uv,
            wires,
        });
    }

    // "end" and nothing after.
    let (line, tail) = reader.next_line().ok_or_else(|| {
        malformed(
            reader.last_line(),
            "unexpected end of payload, expected 'end'",
        )
    })?;
    if tail.trim() != "end" {
        return Err(malformed(line, "expected 'end'"));
    }
    if let Some((line, extra)) = reader.next_line() {
        let shown: String = extra.chars().take(30).collect();
        return Err(malformed(
            line,
            format!("unexpected content after 'end': '{shown}'"),
        ));
    }

    let brep = ExactBRep {
        vertices,
        edges,
        faces,
    };
    // The parser is format-strict; the topology-level rules (knot
    // monotonicity, radii, weights) come from the shared validators,
    // attributed to the line each entity came from.
    for (i, e) in brep.edges.iter().enumerate() {
        validate_curve(&e.curve)
            .map_err(|msg| malformed(edge_lines[i], format!("edge {i}: {msg}")))?;
    }
    for (i, f) in brep.faces.iter().enumerate() {
        validate_surface(&f.surface)
            .map_err(|msg| malformed(face_lines[i], format!("face {i}: {msg}")))?;
    }
    Ok(brep)
}

/// `vertices <n>` / `edges <n>` / `faces <n>` count lines.
fn read_count(reader: &mut LineReader<'_>, what: &str) -> Result<usize, BrpError> {
    let (line, body) = reader.next_line().ok_or_else(|| {
        malformed(
            reader.last_line(),
            format!("unexpected end of payload, expected '{what} <n>'"),
        )
    })?;
    let toks = body.split_whitespace().collect::<Vec<_>>();
    if toks.len() != 2 || toks[0] != what {
        return Err(malformed(line, format!("expected '{what} <n>'")));
    }
    toks[1]
        .parse()
        .map_err(|_| malformed(line, format!("invalid {what} count '{}'", toks[1])))
}

fn parse_vec3(toks: &[&str], line: usize) -> Result<Vector3, BrpError> {
    if toks.len() != 3 {
        return Err(malformed(
            line,
            format!("expected 3 coordinates, found {}", toks.len()),
        ));
    }
    Ok(Vector3::new(
        parse_f64(toks[0], line)?,
        parse_f64(toks[1], line)?,
        parse_f64(toks[2], line)?,
    ))
}

fn parse_f64(tok: &str, line: usize) -> Result<f64, BrpError> {
    let v: f64 = tok
        .parse()
        .map_err(|_| malformed(line, format!("invalid float '{tok}'")))?;
    if !v.is_finite() {
        return Err(malformed(line, format!("value '{tok}' is not finite")));
    }
    Ok(v)
}

fn parse_index(tok: &str, line: usize, count: usize, what: &str) -> Result<usize, BrpError> {
    let v: usize = tok
        .parse()
        .map_err(|_| malformed(line, format!("invalid {what} index '{tok}'")))?;
    if v >= count {
        return Err(malformed(
            line,
            format!("{what} {v} out of range (payload declares {count})"),
        ));
    }
    Ok(v)
}

/// Parse one edge line's curve + return the trailing tokens.
fn parse_curve_tokens<'a>(
    toks: &[&'a str],
    line: usize,
) -> Result<(Curve, Vec<&'a str>), BrpError> {
    if toks.is_empty() || toks[0] != "e" {
        return Err(malformed(line, "expected 'e <curve> …'"));
    }
    match toks.get(1) {
        Some(&"line") => {
            if toks.len() != 12 {
                return Err(malformed(
                    line,
                    "line edge needs 6 curve params + 4 tail tokens",
                ));
            }
            Ok((
                Curve::Line {
                    origin: parse_vec3(&toks[2..5], line)?,
                    direction: parse_vec3(&toks[5..8], line)?,
                },
                toks[8..].to_vec(),
            ))
        }
        Some(&"circle") => {
            if toks.len() != 16 {
                return Err(malformed(
                    line,
                    "circle edge needs 10 curve params + 4 tail tokens",
                ));
            }
            Ok((
                Curve::Circle {
                    center: parse_vec3(&toks[2..5], line)?,
                    axis: parse_vec3(&toks[5..8], line)?,
                    ref_dir: parse_vec3(&toks[8..11], line)?,
                    radius: parse_f64(toks[11], line)?,
                },
                toks[12..].to_vec(),
            ))
        }
        Some(&"ellipse") => {
            if toks.len() != 17 {
                return Err(malformed(
                    line,
                    "ellipse edge needs 11 curve params + 4 tail tokens",
                ));
            }
            Ok((
                Curve::Ellipse {
                    center: parse_vec3(&toks[2..5], line)?,
                    axis: parse_vec3(&toks[5..8], line)?,
                    major_dir: parse_vec3(&toks[8..11], line)?,
                    major_radius: parse_f64(toks[11], line)?,
                    minor_radius: parse_f64(toks[12], line)?,
                },
                toks[13..].to_vec(),
            ))
        }
        Some(&"bspline") => {
            if toks.len() < 6 {
                return Err(malformed(line, "bspline edge header too short"));
            }
            let degree: usize = toks[2]
                .parse()
                .map_err(|_| malformed(line, format!("invalid degree '{}'", toks[2])))?;
            let nknots: usize = toks[3]
                .parse()
                .map_err(|_| malformed(line, format!("invalid knot count '{}'", toks[3])))?;
            let npts: usize = toks[4].parse().map_err(|_| {
                malformed(line, format!("invalid control point count '{}'", toks[4]))
            })?;
            let rational = match toks[5] {
                "0" => false,
                "1" => true,
                other => {
                    return Err(malformed(
                        line,
                        format!("rational flag must be 0 or 1, found '{other}'"),
                    ))
                }
            };
            let expected = 6 + nknots + 3 * npts + if rational { npts } else { 0 } + 4;
            if toks.len() != expected {
                return Err(malformed(
                    line,
                    format!("bspline edge declares {nknots} knots + {npts} points but the line has {} tokens (expected {expected})", toks.len()),
                ));
            }
            let mut knots = Vec::with_capacity(nknots);
            for i in 0..nknots {
                knots.push(parse_f64(toks[6 + i], line)?);
            }
            let mut pts = Vec::with_capacity(npts);
            for i in 0..npts {
                let base = 6 + nknots + 3 * i;
                pts.push(parse_vec3(&toks[base..base + 3], line)?);
            }
            let weights = if rational {
                let base = 6 + nknots + 3 * npts;
                let mut ws = Vec::with_capacity(npts);
                for i in 0..npts {
                    ws.push(parse_f64(toks[base + i], line)?);
                }
                Some(ws)
            } else {
                None
            };
            let tail_base = 6 + nknots + 3 * npts + if rational { npts } else { 0 };
            Ok((
                Curve::Bspline {
                    degree,
                    knots,
                    points: pts,
                    weights,
                },
                toks[tail_base..].to_vec(),
            ))
        }
        _ => Err(malformed(
            line,
            "expected 'e line|circle|ellipse|bspline …'",
        )),
    }
}

/// Parse one face line's surface + return the trailing tokens.
fn parse_surface_tokens<'a>(
    toks: &[&'a str],
    line: usize,
) -> Result<(Surface, Vec<&'a str>), BrpError> {
    if toks.is_empty() || toks[0] != "f" {
        return Err(malformed(line, "expected 'f <surface> …'"));
    }
    match toks.get(1) {
        Some(&"plane") => {
            if toks.len() != 16 {
                return Err(malformed(
                    line,
                    "plane face needs 9 surface params + 5 tail tokens",
                ));
            }
            Ok((
                Surface::Plane {
                    origin: parse_vec3(&toks[2..5], line)?,
                    normal: parse_vec3(&toks[5..8], line)?,
                    x_dir: parse_vec3(&toks[8..11], line)?,
                },
                toks[11..].to_vec(),
            ))
        }
        Some(&"cylinder") => {
            if toks.len() != 17 {
                return Err(malformed(
                    line,
                    "cylinder face needs 10 surface params + 5 tail tokens",
                ));
            }
            Ok((
                Surface::Cylinder {
                    origin: parse_vec3(&toks[2..5], line)?,
                    axis: parse_vec3(&toks[5..8], line)?,
                    ref_dir: parse_vec3(&toks[8..11], line)?,
                    radius: parse_f64(toks[11], line)?,
                },
                toks[12..].to_vec(),
            ))
        }
        Some(&"cone") => {
            if toks.len() != 18 {
                return Err(malformed(
                    line,
                    "cone face needs 11 surface params + 5 tail tokens",
                ));
            }
            Ok((
                Surface::Cone {
                    origin: parse_vec3(&toks[2..5], line)?,
                    axis: parse_vec3(&toks[5..8], line)?,
                    ref_dir: parse_vec3(&toks[8..11], line)?,
                    semi_angle: parse_f64(toks[11], line)?,
                    ref_radius: parse_f64(toks[12], line)?,
                },
                toks[13..].to_vec(),
            ))
        }
        Some(&"sphere") => {
            if toks.len() != 17 {
                return Err(malformed(
                    line,
                    "sphere face needs 10 surface params + 5 tail tokens",
                ));
            }
            Ok((
                Surface::Sphere {
                    center: parse_vec3(&toks[2..5], line)?,
                    axis: parse_vec3(&toks[5..8], line)?,
                    ref_dir: parse_vec3(&toks[8..11], line)?,
                    radius: parse_f64(toks[11], line)?,
                },
                toks[12..].to_vec(),
            ))
        }
        Some(&"torus") => {
            if toks.len() != 18 {
                return Err(malformed(
                    line,
                    "torus face needs 11 surface params + 5 tail tokens",
                ));
            }
            Ok((
                Surface::Torus {
                    center: parse_vec3(&toks[2..5], line)?,
                    axis: parse_vec3(&toks[5..8], line)?,
                    ref_dir: parse_vec3(&toks[8..11], line)?,
                    major_radius: parse_f64(toks[11], line)?,
                    minor_radius: parse_f64(toks[12], line)?,
                },
                toks[13..].to_vec(),
            ))
        }
        Some(&"bspline") => {
            if toks.len() < 9 {
                return Err(malformed(line, "bspline face header too short"));
            }
            let u_degree: usize = toks[2]
                .parse()
                .map_err(|_| malformed(line, format!("invalid u degree '{}'", toks[2])))?;
            let v_degree: usize = toks[3]
                .parse()
                .map_err(|_| malformed(line, format!("invalid v degree '{}'", toks[3])))?;
            let nuk: usize = toks[4]
                .parse()
                .map_err(|_| malformed(line, format!("invalid u knot count '{}'", toks[4])))?;
            let nvk: usize = toks[5]
                .parse()
                .map_err(|_| malformed(line, format!("invalid v knot count '{}'", toks[5])))?;
            let npu: usize = toks[6]
                .parse()
                .map_err(|_| malformed(line, format!("invalid u control count '{}'", toks[6])))?;
            let npv: usize = toks[7]
                .parse()
                .map_err(|_| malformed(line, format!("invalid v control count '{}'", toks[7])))?;
            let rational = match toks[8] {
                "0" => false,
                "1" => true,
                other => {
                    return Err(malformed(
                        line,
                        format!("rational flag must be 0 or 1, found '{other}'"),
                    ))
                }
            };
            let expected = 9 + nuk + nvk + 3 * npu * npv + if rational { npu * npv } else { 0 } + 5;
            if toks.len() != expected {
                return Err(malformed(
                    line,
                    format!("bspline face token count {} does not match its declared sizes (expected {expected})", toks.len()),
                ));
            }
            let mut pos = 9;
            let mut u_knots = Vec::with_capacity(nuk);
            for _ in 0..nuk {
                u_knots.push(parse_f64(toks[pos], line)?);
                pos += 1;
            }
            let mut v_knots = Vec::with_capacity(nvk);
            for _ in 0..nvk {
                v_knots.push(parse_f64(toks[pos], line)?);
                pos += 1;
            }
            let mut points = Vec::with_capacity(npu);
            for _ in 0..npu {
                let mut row = Vec::with_capacity(npv);
                for _ in 0..npv {
                    row.push(parse_vec3(&toks[pos..pos + 3], line)?);
                    pos += 3;
                }
                points.push(row);
            }
            let weights = if rational {
                let mut ws = Vec::with_capacity(npu);
                for _ in 0..npu {
                    let mut row = Vec::with_capacity(npv);
                    for _ in 0..npv {
                        row.push(parse_f64(toks[pos], line)?);
                        pos += 1;
                    }
                    ws.push(row);
                }
                Some(ws)
            } else {
                None
            };
            Ok((
                Surface::Bspline {
                    u_degree,
                    v_degree,
                    u_knots,
                    v_knots,
                    points,
                    weights,
                },
                toks[pos..].to_vec(),
            ))
        }
        _ => Err(malformed(
            line,
            "expected 'f plane|cylinder|cone|sphere|torus|bspline …'",
        )),
    }
}
