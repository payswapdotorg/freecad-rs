//! Shaded orthographic SVG rendering of document meshes.
//!
//! Simple, honest software rendering: project each triangle with a fixed
//! view matrix, painter-sort by depth, flat-shade with a headlight. Good
//! enough to eyeball parametric changes from the command line.
//!
//! Conventions:
//!
//! * fc-base matrices follow the **row-vector** convention (`p' = p * M`),
//!   so the rows of a view matrix are the images of the world axes.
//! * The camera sits on the `+z` side of camera space looking along `-z`:
//!   **larger camera `z` means closer to the viewer** — the depth used for
//!   painter sorting.
//! * Screen coordinates grow right and **up**; the y axis is flipped when
//!   the SVG is written (SVG y grows down), so models appear right-side-up.

use std::io::Write;

use fc_base::{Matrix4, Rotation};
use fc_part::Mesh;

/// Canvas size and margin, in SVG user units.
const WIDTH: f64 = 960.0;
const HEIGHT: f64 = 720.0;
const MARGIN: f64 = 40.0;

/// One shaded, projected, drawable triangle.
struct Tri {
    /// Mean camera-space depth of the three vertices (larger = closer).
    depth: f64,
    /// SVG pixel coordinates (y already flipped to SVG's down-growing axis).
    pts: [[f64; 2]; 3],
    /// Flat shade in `[0.30, 0.92]`.
    shade: f64,
}

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

    /// Canonical lowercase name (used in captions and CLI output).
    pub fn label(&self) -> &'static str {
        match self {
            View::Iso => "iso",
            View::Top => "top",
            View::Front => "front",
            View::Right => "right",
        }
    }

    /// View matrix (world -> camera), in fc-base's row-vector convention
    /// (`p' = p * M`, so **rows are the images of the world axes**). The
    /// camera looks along `-z` of camera space; screen x grows right,
    /// screen y grows up (flipped on SVG output).
    ///
    /// * `Iso` — the model is yawed -30 deg about world Z, then pitched
    ///   -20 deg about the camera X axis. The camera ends up above the
    ///   model, slightly to the front-right (`+X` projects down-right,
    ///   `+Z` up, top + front + right faces visible).
    /// * `Top` — looking down `-Z`: screen x = world x, screen y = world
    ///   y, depth = world z. The identity.
    /// * `Front` — looking along `+Y` (camera on `-Y`): screen x = world
    ///   x, screen y = world z, depth = -world y.
    /// * `Right` — looking along `-X` (camera on `+X`): screen x = world
    ///   y (facing the model, world `+Y` appears to the right), screen
    ///   y = world z, depth = world x.
    pub fn matrix(&self) -> Matrix4 {
        match self {
            View::Iso => {
                let yaw = Rotation::around_z((-30.0_f64).to_radians()).to_matrix();
                let pitch = Rotation::around_x((-20.0_f64).to_radians()).to_matrix();
                // `A.multiply(&B)` applies A first (row-vector convention):
                // yaw the model, then pitch it.
                yaw.multiply(&pitch)
            }
            View::Top => Matrix4::IDENTITY,
            View::Front => Matrix4 {
                m: [
                    [1.0, 0.0, 0.0, 0.0],  // world +X -> screen right
                    [0.0, 0.0, -1.0, 0.0], // world +Y -> away from camera
                    [0.0, 1.0, 0.0, 0.0],  // world +Z -> screen up
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
            View::Right => Matrix4 {
                m: [
                    [0.0, 0.0, 1.0, 0.0], // world +X -> toward camera
                    [1.0, 0.0, 0.0, 0.0], // world +Y -> screen right
                    [0.0, 1.0, 0.0, 0.0], // world +Z -> screen up
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
        }
    }
}

/// Render `meshes` (each with its display name) to `out`.
///
/// Layout: 960x720 viewBox, 16:12, white background, gray-shaded filled
/// triangles with hairline strokes, one `<g>` per object with a `<title>`
/// carrying its name (hover text), plus a caption line with the object
/// count.
///
/// Occlusion is a two-level painter's algorithm: objects are drawn
/// far-to-near by mean triangle depth, and each object's triangles are
/// sorted far-to-near within it. Exact for the non-interpenetrating
/// solids this renderer is meant for, and it keeps the one-`<g>`-per
/// -object grouping that makes hover titles work.
pub fn render_svg<W: Write>(
    meshes: &[(String, Mesh)],
    view: View,
    out: &mut W,
) -> std::io::Result<()> {
    let view_matrix = view.matrix();
    let light = normalized([0.35, 0.35, 0.87]);
    // Blinn half-vector for a modest specular highlight (view = +z).
    let halfway = normalized([light[0], light[1], light[2] + 1.0]);

    // Pass 1: world -> camera space, and the extent of the projection.
    let mut lo = [f64::INFINITY, f64::INFINITY];
    let mut hi = [f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut cams: Vec<Vec<[f64; 3]>> = Vec::with_capacity(meshes.len());
    for (_, mesh) in meshes {
        let mut cam = Vec::with_capacity(mesh.vertices.len());
        for v in &mesh.vertices {
            let p = transform(&view_matrix, [v.x, v.y, v.z]);
            lo[0] = lo[0].min(p[0]);
            hi[0] = hi[0].max(p[0]);
            lo[1] = lo[1].min(p[1]);
            hi[1] = hi[1].max(p[1]);
            cam.push(p);
        }
        cams.push(cam);
    }
    if lo[0] > hi[0] {
        // No vertices at all; keep a degenerate but finite frame.
        lo = [0.0, 0.0];
        hi = [0.0, 0.0];
    }

    // Fit into the canvas with a margin, preserving aspect. Screen y
    // grows up in camera space and down in SVG, hence the flip.
    let scale = fit_scale([hi[0] - lo[0], hi[1] - lo[1]]);
    let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
    let project = |p: [f64; 3]| -> [f64; 2] {
        [
            WIDTH * 0.5 + (p[0] - center[0]) * scale,
            HEIGHT * 0.5 - (p[1] - center[1]) * scale,
        ]
    };

    // Pass 2: shaded triangles.
    let mut groups: Vec<(f64, &str, Vec<Tri>)> = Vec::new();
    for ((name, mesh), cam) in meshes.iter().zip(cams.iter()) {
        let mut tris: Vec<Tri> = Vec::new();
        for t in &mesh.triangles {
            let (ia, ib, ic) = (t[0] as usize, t[1] as usize, t[2] as usize);
            if ia >= cam.len() || ib >= cam.len() || ic >= cam.len() {
                continue; // malformed index; skip defensively
            }
            let (a, b, c) = (cam[ia], cam[ib], cam[ic]);
            // View matrices are pure rotations, so the camera-space
            // normal of the transformed vertices *is* the transformed
            // world normal.
            let n = cross(sub(b, a), sub(c, a));
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len <= 1e-12 {
                continue; // degenerate sliver
            }
            let lambert = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len;
            let spec = {
                let h = (n[0] * halfway[0] + n[1] * halfway[1] + n[2] * halfway[2]) / len;
                h.max(0.0).powi(48) * 0.22
            };
            tris.push(Tri {
                depth: (a[2] + b[2] + c[2]) / 3.0,
                pts: [project(a), project(b), project(c)],
                shade: (0.30 + 0.62 * lambert.max(0.0) + spec).min(1.0),
            });
        }
        if tris.is_empty() {
            continue;
        }
        let mean_depth = tris.iter().map(|t| t.depth).sum::<f64>() / tris.len() as f64;
        tris.sort_by(|x, y| x.depth.total_cmp(&y.depth)); // far first
        groups.push((mean_depth, name.as_str(), tris));
    }
    groups.sort_by(|x, y| x.0.total_cmp(&y.0)); // far objects first

    writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="720" viewBox="0 0 960 720">"#
    )?;
    writeln!(
        out,
        r##"  <rect x="0" y="0" width="960" height="720" fill="#ffffff"/>"##
    )?;
    for (_, name, tris) in groups {
        writeln!(out, "  <g>")?;
        writeln!(out, "    <title>{}</title>", xml_escape(name))?;
        for t in tris {
            let fill = gray(t.shade);
            // Same-color hairline stroke: SVG rasterizers antialias shared
            // polygon edges, which leaves visible 1px seams between
            // adjacent tessellation triangles. Painting each triangle with
            // its own color as stroke covers the seam exactly — the
            // standard SVG-mesh trick for smooth-looking surfaces.
            writeln!(
                out,
                "    <polygon points=\"{:.2},{:.2} {:.2},{:.2} {:.2},{:.2}\" fill=\"{fill}\" stroke=\"{fill}\" stroke-width=\"0.4\" stroke-linejoin=\"round\"/>",
                t.pts[0][0],
                t.pts[0][1],
                t.pts[1][0],
                t.pts[1][1],
                t.pts[2][0],
                t.pts[2][1],
            )?;
        }
        writeln!(out, "  </g>")?;
    }
    writeln!(
        out,
        "  <text x=\"40\" y=\"700\" font-family=\"monospace\" font-size=\"15\" fill=\"#333333\">freecad-rs — {} objects · view {}</text>",
        meshes.len(),
        view.label(),
    )?;
    writeln!(out, "</svg>")?;
    Ok(())
}

/// Uniform scale fitting a `screen-space extent` into the canvas minus
/// its margins; `1.0` for a degenerate (flat or empty) projection.
fn fit_scale(extent: [f64; 2]) -> f64 {
    const EPS: f64 = 1e-12;
    let avail = [WIDTH - 2.0 * MARGIN, HEIGHT - 2.0 * MARGIN];
    let sx = if extent[0] > EPS {
        avail[0] / extent[0]
    } else {
        f64::INFINITY
    };
    let sy = if extent[1] > EPS {
        avail[1] / extent[1]
    } else {
        f64::INFINITY
    };
    let s = sx.min(sy);
    if s.is_finite() {
        s
    } else {
        1.0
    }
}

/// Row-vector point transform `p * m` (fc-base's convention), spelled out
/// on plain arrays: the renderer stays self-contained against the public
/// `Matrix4::m` field and trivially auditable.
fn transform(m: &Matrix4, p: [f64; 3]) -> [f64; 3] {
    let r = &m.m;
    [
        p[0] * r[0][0] + p[1] * r[1][0] + p[2] * r[2][0] + r[3][0],
        p[0] * r[0][1] + p[1] * r[1][1] + p[2] * r[2][1] + r[3][1],
        p[0] * r[0][2] + p[1] * r[1][2] + p[2] * r[2][2] + r[3][2],
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(v: [f64; 3]) -> [f64; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

/// Map a shade in `[0.30, 0.92]` onto a grayscale color between `#202020`
/// and `#e8e8e8`.
fn gray(shade: f64) -> String {
    // Float-precision rgb() — CSS color values accept decimals. Integer
    // 8-bit quantization here produced visible contour banding between
    // adjacent flat-shaded triangles (read as a wireframe mesh).
    let v = (32.0 + 200.0 * shade).clamp(0.0, 255.0);
    format!("rgb({v:.2},{v:.2},{v:.2})")
}

/// Escape text for XML content (`&`, `<`, `>`).
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
