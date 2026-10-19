//! Shaded orthographic SVG rendering of document meshes.
//!
//! Simple, honest software rendering: project each triangle with a fixed
//! view matrix, painter-sort by depth, flat-shade with a headlight. Good
//! enough to eyeboll parametric changes from the command line.

use fc_base::{Matrix4, Rotation, Vector3};
use fc_part::Mesh;

/// Named projections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    /// Classic isometric-ish: yaw -30 deg, pitch -20 deg.
    Iso,
    Top,
    Front,
    Right,
}

impl View {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "iso" => Ok(View::Iso),
            "top" => Ok(View::Top),
            "front" => Ok(View::Front),
            "right" => Ok(View::Right),
            other => Err(format!("unknown view '{other}' (iso|top|front|right)")),
        }
    }

    /// View matrix (world -> camera).
    pub fn matrix(&self) -> Matrix4 {
        todo!()
    }
}

/// Render `meshes` (each with its display name) to `out`.
///
/// Layout: 960x720 viewBox, 16:12, white background, gray-shaded filled
/// triangles with hairline strokes, one `<g>` per object with a `<title>`
/// carrying its name (hover text), plus a caption line with the object
/// count.
pub fn render_svg<W: std::io::Write>(
    meshes: &[(String, Mesh)],
    view: View,
    out: &mut W,
) -> std::io::Result<()> {
    todo!()
}
