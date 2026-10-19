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
//! Real FreeCAD `.brp` payloads are OCCT BRep ("DBRep_DrawableShape" /
//! "CASCADE Topology" headers). `probe_format` recognizes them so the
//! loader can report them honestly as unsupported instead of misparsing.

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
    Malformed { line: usize, reason: String },
    /// OCCT payloads cannot be loaded by this kernel (by design).
    UnsupportedOcct,
    UnknownFormat,
}

impl std::fmt::Display for BrpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrpError::Malformed { line, reason } => write!(f, "malformed FCBREP at line {line}: {reason}"),
            BrpError::UnsupportedOcct => write!(f, "OCCT BRep payload: not supported by this kernel"),
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
    todo!()
}

/// Parse an FCBREP v1 payload (bytes). OCCT/unknown payloads return the
/// corresponding error; use [`probe_format`] first when you want to
/// distinguish them.
pub fn read_shape_bytes(data: &[u8]) -> Result<TopoShape, BrpError> {
    todo!()
}

/// Shared helper: parse one `v x y z` line. (Keep private helpers near
/// their callers.)
fn parse_vertex(parts: std::str::SplitWhitespace<'_>, line: usize) -> Result<Vector3, BrpError> {
    todo!()
}
