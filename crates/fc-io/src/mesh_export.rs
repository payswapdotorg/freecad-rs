//! Mesh export to Wavefront OBJ and STL (ASCII + binary).
//!
//! Clean-room reimplementation of the mesh-export side of FreeCAD's Mesh
//! module (architecture reference only: `src/Mod/Mesh/App/Mesh.cpp`,
//! whose export path fans a mesh out to per-format writers). No FreeCAD
//! code is used; the byte-level behavior below is the contract, pinned
//! by tests.
//!
//! Layering: mesh geometry (`fc_part::Mesh`) lives in `fc-part`, but this
//! module deliberately does not import it. Writers take plain slices —
//! a vertex array plus 0-based triangle index triples — so `fc-io` gains
//! no new dependencies (only `fc-base::Vector3`, already required) and any
//! mesh source can feed them. A `fc_part::Mesh` slots straight in:
//! `write_obj(&mesh.vertices, &mesh.triangles, out)`.
//!
//! # Format contracts
//!
//! **OBJ** (Wavefront):
//!
//! ```text
//! # freecad-rs export
//! # vertices: 4
//! # triangles: 4
//! v 0 0 0
//! v 1 0 0
//! v 0 1 0
//! v 0 0 1
//! f 1 3 2
//! f 1 2 4
//! f 1 4 3
//! f 2 3 4
//! ```
//!
//! Comment header with counts, one `v x y z` per vertex, one `f i j k`
//! per triangle with **1-based** indices, single-space separated. Vertices
//! are written once and shared by reference (no duplication). Floats use
//! Rust's shortest round-trip `Display` form (`1`, `-0.5`,
//! `0.5773502691896258`): parsing a coordinate back with `str::parse`
//! yields the exact original `f64`, and the same holds for ASCII STL
//! below. Coordinates are written without exponent notation (Rust's `{}`
//! never emits one), which keeps files parser-friendly.
//!
//! **STL ASCII** — classic 3D Systems layout (accepted by every STL
//! parser; the grammar is whitespace-insensitive):
//!
//! ```text
//! solid name
//!   facet normal 0 0 -1
//!     outer loop
//!       vertex 0 0 0
//!       vertex 0 1 0
//!       vertex 1 0 0
//!     endloop
//!   endfacet
//! endsolid name
//! ```
//!
//! `solid`/`endsolid` at column 0, `facet`/`endfacet` indented 2 spaces,
//! `outer loop`/`endloop` 4, `vertex` 6. The normal is the unit normal of
//! the counter-clockwise winding, `(v1-v0)x(v2-v0)` normalized; a
//! zero-area (degenerate) triangle is still emitted, with normal
//! `0 0 0`.
//!
//! **STL binary** — an 80-byte header (the `name`, zero-padded, never
//! prefixed with `solid`, and truncated at a UTF-8 character boundary if
//! longer than 80 bytes), then a little-endian `u32` triangle count, then
//! one 50-byte record per triangle: 3x `f32` LE normal, 9x `f32` LE
//! vertex components, `u16` LE attribute byte count (always 0). The exact
//! total size is [`stl_binary_size`]: `84 + 50*n`. Vertex coordinates are
//! downcast from `f64` to `f32` (the format's precision).
//!
//! All writers validate triangle indices up front and return an
//! [`std::io::Error`] of kind [`std::io::ErrorKind::InvalidInput`] for
//! out-of-range indices, and (binary only) for triangle counts exceeding
//! `u32::MAX` — library code never panics on bad input.

use std::io::{self, Write};

use fc_base::Vector3;

/// Exact byte size of a binary STL with `triangle_count` triangles:
/// `84 + 50 * n` (80-byte header, 4-byte count, 50 bytes per triangle).
///
/// Saturates rather than overflowing in the pathological regime where
/// `50 * n` would exceed `usize`.
pub fn stl_binary_size(triangle_count: usize) -> usize {
    50_usize.saturating_mul(triangle_count).saturating_add(84)
}

/// Write `vertices` + `triangles` as a Wavefront OBJ file.
///
/// See the [module docs](self) for the exact byte layout: comment header,
/// `v` lines, then 1-based `f` lines. Vertices are shared (written once),
/// so a vertex used by many triangles costs one `v` record total.
pub fn write_obj(
    vertices: &[Vector3],
    triangles: &[[u32; 3]],
    out: &mut impl Write,
) -> io::Result<()> {
    validate_indices(vertices.len(), triangles)?;
    let nv = vertices.len();
    let nt = triangles.len();
    writeln!(out, "# freecad-rs export")?;
    writeln!(out, "# vertices: {nv}")?;
    writeln!(out, "# triangles: {nt}")?;
    for v in vertices {
        let Vector3 { x, y, z } = *v;
        writeln!(out, "v {x} {y} {z}")?;
    }
    for t in triangles {
        // 1-based indices; computed in u64 so the +1 cannot overflow.
        writeln!(
            out,
            "f {} {} {}",
            t[0] as u64 + 1,
            t[1] as u64 + 1,
            t[2] as u64 + 1
        )?;
    }
    Ok(())
}

/// Write `vertices` + `triangles` as an ASCII STL file named `name`.
///
/// See the [module docs](self) for the exact indentation and normal
/// convention (CCW unit normal; degenerate triangles get normal `0 0 0`).
pub fn write_stl_ascii(
    name: &str,
    vertices: &[Vector3],
    triangles: &[[u32; 3]],
    out: &mut impl Write,
) -> io::Result<()> {
    validate_indices(vertices.len(), triangles)?;
    writeln!(out, "solid {name}")?;
    for t in triangles {
        let (a, b, c) = triangle_verts(vertices, t);
        let n = facet_normal(a, b, c);
        let Vector3 { x, y, z } = n;
        writeln!(out, "  facet normal {x} {y} {z}")?;
        writeln!(out, "    outer loop")?;
        let Vector3 { x, y, z } = a;
        writeln!(out, "      vertex {x} {y} {z}")?;
        let Vector3 { x, y, z } = b;
        writeln!(out, "      vertex {x} {y} {z}")?;
        let Vector3 { x, y, z } = c;
        writeln!(out, "      vertex {x} {y} {z}")?;
        writeln!(out, "    endloop")?;
        writeln!(out, "  endfacet")?;
    }
    writeln!(out, "endsolid {name}")?;
    Ok(())
}

/// Write `vertices` + `triangles` as a binary STL file named `name`.
///
/// Layout: 80-byte zero-padded header (no `solid` prefix; long names are
/// UTF-8-boundary-truncated), LE `u32` count, then 50-byte records
/// (`f32` LE normal, 9x `f32` LE coordinates, zero attribute count).
/// The result is exactly [`stl_binary_size(triangles.len())`] bytes.
pub fn write_stl_binary(
    name: &str,
    vertices: &[Vector3],
    triangles: &[[u32; 3]],
    out: &mut impl Write,
) -> io::Result<()> {
    validate_indices(vertices.len(), triangles)?;
    let count = u32::try_from(triangles.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "binary STL supports at most u32::MAX triangles",
        )
    })?;

    let mut header = [0u8; 80];
    let bytes = name.as_bytes();
    let take = header_name_len(bytes);
    header[..take].copy_from_slice(&bytes[..take]);
    out.write_all(&header)?;
    out.write_all(&count.to_le_bytes())?;

    let mut rec = [0u8; 50];
    for t in triangles {
        let (a, b, c) = triangle_verts(vertices, t);
        let n = facet_normal(a, b, c);
        rec[0..4].copy_from_slice(&(n.x as f32).to_le_bytes());
        rec[4..8].copy_from_slice(&(n.y as f32).to_le_bytes());
        rec[8..12].copy_from_slice(&(n.z as f32).to_le_bytes());
        rec[12..16].copy_from_slice(&(a.x as f32).to_le_bytes());
        rec[16..20].copy_from_slice(&(a.y as f32).to_le_bytes());
        rec[20..24].copy_from_slice(&(a.z as f32).to_le_bytes());
        rec[24..28].copy_from_slice(&(b.x as f32).to_le_bytes());
        rec[28..32].copy_from_slice(&(b.y as f32).to_le_bytes());
        rec[32..36].copy_from_slice(&(b.z as f32).to_le_bytes());
        rec[36..40].copy_from_slice(&(c.x as f32).to_le_bytes());
        rec[40..44].copy_from_slice(&(c.y as f32).to_le_bytes());
        rec[44..48].copy_from_slice(&(c.z as f32).to_le_bytes());
        // rec[48..50] stays 0: attribute byte count.
        out.write_all(&rec)?;
    }
    Ok(())
}

/// Verify that every triangle index addresses an existing vertex.
fn validate_indices(vertex_count: usize, triangles: &[[u32; 3]]) -> io::Result<()> {
    for (n, t) in triangles.iter().enumerate() {
        for &i in t {
            if i as usize >= vertex_count {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "triangle {n} references vertex {i} \
                         but only {vertex_count} vertices exist"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// The three vertex positions of `t` (indices already validated).
fn triangle_verts(vertices: &[Vector3], t: &[u32; 3]) -> (Vector3, Vector3, Vector3) {
    (
        vertices[t[0] as usize],
        vertices[t[1] as usize],
        vertices[t[2] as usize],
    )
}

/// Unit normal of the counter-clockwise triangle `(a, b, c)`:
/// `(b - a) x (c - a)`, normalized. Zero-area triangles yield
/// `Vector3::ZERO`.
fn facet_normal(a: Vector3, b: Vector3, c: Vector3) -> Vector3 {
    (b - a)
        .cross(&(c - a))
        .normalized()
        .unwrap_or(Vector3::ZERO)
}

/// Bytes of `name` to copy into the 80-byte binary STL header: at most
/// 80, and never splitting a UTF-8 character.
fn header_name_len(bytes: &[u8]) -> usize {
    if bytes.len() <= 80 {
        bytes.len()
    } else {
        let mut end = 80;
        // Back up over continuation bytes (0b10xxxxxx) to a char start.
        while end > 0 && bytes[end] & 0xC0 == 0x80 {
            end -= 1;
        }
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit tetrahedron (a cube corner cut by x+y+z=1), all faces wound
    /// CCW viewed from outside. Face normals: -Z, -Y, -X, +(1,1,1)/sqrt(3).
    fn tetra() -> (Vec<Vector3>, Vec<[u32; 3]>) {
        let verts = vec![Vector3::ZERO, Vector3::X, Vector3::Y, Vector3::Z];
        let tris = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        (verts, tris)
    }

    fn le_f32(buf: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(buf[at..at + 4].try_into().unwrap())
    }

    fn le_u16(buf: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(buf[at..at + 2].try_into().unwrap())
    }

    // -----------------------------------------------------------------
    // OBJ
    // -----------------------------------------------------------------

    #[test]
    fn obj_single_triangle_exact() {
        let verts = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ];
        let tris = vec![[0u32, 1, 2]];
        let mut buf = Vec::new();
        write_obj(&verts, &tris, &mut buf).unwrap();
        let expected = "\
# freecad-rs export
# vertices: 3
# triangles: 1
v 0 0 0
v 1 0 0
v 0 1 0
f 1 2 3
";
        assert_eq!(String::from_utf8(buf).unwrap(), expected);
    }

    #[test]
    fn obj_tetrahedron_exact_and_vertex_sharing() {
        let (verts, tris) = tetra();
        let mut buf = Vec::new();
        write_obj(&verts, &tris, &mut buf).unwrap();
        let expected = "\
# freecad-rs export
# vertices: 4
# triangles: 4
v 0 0 0
v 1 0 0
v 0 1 0
v 0 0 1
f 1 3 2
f 1 2 4
f 1 4 3
f 2 3 4
";
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text, expected);

        // 4 faces over 4 shared vertices: exactly 4 `v` lines (no
        // duplication), exactly 4 `f` lines, indices within 1..=4.
        let lines: Vec<&str> = text.lines().collect();
        let v_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("v ")).collect();
        let f_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("f ")).collect();
        assert_eq!(v_lines.len(), 4);
        assert_eq!(f_lines.len(), 4);
        for l in &f_lines {
            let idx: Vec<u32> = l[2..]
                .split_whitespace()
                .map(|s| s.parse().unwrap())
                .collect();
            assert_eq!(idx.len(), 3);
            assert!(idx.iter().all(|&i| (1..=4).contains(&i)));
        }
    }

    #[test]
    fn obj_f64_round_trip() {
        // Awkward f64s: shortest-round-trip decimal must parse back
        // bit-for-bit identical.
        let verts = vec![
            Vector3::new(0.1, 0.2, 0.30000000000000004),
            Vector3::new(-42.75, 1e-15, 2.5e20),
            Vector3::new(123456.7890123456, -0.0, 1.0 / 3.0),
            Vector3::new(5e-324, 1e300, f64::MIN_POSITIVE),
        ];
        let tris = vec![[0u32, 1, 2], [0, 2, 3], [3, 1, 2]];
        let mut buf = Vec::new();
        write_obj(&verts, &tris, &mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();

        let mut parsed: Vec<[f64; 3]> = Vec::new();
        let mut faces: Vec<[u32; 3]> = Vec::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts[0] {
                "v" => parsed.push([
                    parts[1].parse().unwrap(),
                    parts[2].parse().unwrap(),
                    parts[3].parse().unwrap(),
                ]),
                "f" => faces.push([
                    parts[1].parse().unwrap(),
                    parts[2].parse().unwrap(),
                    parts[3].parse().unwrap(),
                ]),
                "#" => {}
                other => panic!("unexpected OBJ line kind: {other}"),
            }
        }
        assert_eq!(parsed.len(), verts.len());
        assert_eq!(faces.len(), tris.len());
        for (p, v) in parsed.iter().zip(&verts) {
            assert_eq!(p[0], v.x, "x coordinate must round-trip exactly");
            assert_eq!(p[1], v.y, "y coordinate must round-trip exactly");
            assert_eq!(p[2], v.z, "z coordinate must round-trip exactly");
        }
        for (f, t) in faces.iter().zip(&tris) {
            assert_eq!(f[0], t[0] + 1);
            assert_eq!(f[1], t[1] + 1);
            assert_eq!(f[2], t[2] + 1);
        }
    }

    // -----------------------------------------------------------------
    // STL ASCII
    // -----------------------------------------------------------------

    #[test]
    fn stl_ascii_tetrahedron_exact() {
        let (verts, tris) = tetra();
        let mut buf = Vec::new();
        write_stl_ascii("tetra", &verts, &tris, &mut buf).unwrap();
        // 1/sqrt(3) = 0.5773502691896258 (shortest round-trip f64).
        let expected = "\
solid tetra
  facet normal 0 0 -1
    outer loop
      vertex 0 0 0
      vertex 0 1 0
      vertex 1 0 0
    endloop
  endfacet
  facet normal 0 -1 0
    outer loop
      vertex 0 0 0
      vertex 1 0 0
      vertex 0 0 1
    endloop
  endfacet
  facet normal -1 0 0
    outer loop
      vertex 0 0 0
      vertex 0 0 1
      vertex 0 1 0
    endloop
  endfacet
  facet normal 0.5773502691896258 0.5773502691896258 0.5773502691896258
    outer loop
      vertex 1 0 0
      vertex 0 1 0
      vertex 0 0 1
    endloop
  endfacet
endsolid tetra
";
        assert_eq!(String::from_utf8(buf).unwrap(), expected);
    }

    #[test]
    fn stl_ascii_degenerate_triangle_emits_zero_normal() {
        // Collinear points: zero area, normal 0 0 0, triangle still written.
        let verts = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
        ];
        let tris = vec![[0u32, 1, 2]];
        let mut buf = Vec::new();
        write_stl_ascii("deg", &verts, &tris, &mut buf).unwrap();
        let expected = "\
solid deg
  facet normal 0 0 0
    outer loop
      vertex 0 0 0
      vertex 1 0 0
      vertex 2 0 0
    endloop
  endfacet
endsolid deg
";
        assert_eq!(String::from_utf8(buf).unwrap(), expected);
    }

    // -----------------------------------------------------------------
    // STL binary
    // -----------------------------------------------------------------

    #[test]
    fn stl_binary_tetrahedron_decodes_back() {
        let (verts, tris) = tetra();
        let mut buf = Vec::new();
        write_stl_binary("tetra", &verts, &tris, &mut buf).unwrap();

        // Byte-exact size.
        assert_eq!(buf.len(), stl_binary_size(4));
        assert_eq!(stl_binary_size(4), 284);

        // Header: "tetra", zero-padded to 80 bytes, no "solid" prefix.
        assert_eq!(&buf[0..5], b"tetra");
        assert!(buf[5..80].iter().all(|&b| b == 0));

        // Count field (LE u32 at offset 80).
        let count = u32::from_le_bytes(buf[80..84].try_into().unwrap());
        assert_eq!(count, 4);

        // Full record decode: normals unit-length or zero; vertices match
        // the f64 originals within the f32 downcast tolerance (1e-3).
        let expected_normals = [
            [0.0f64, 0.0, -1.0],
            [0.0, -1.0, 0.0],
            [-1.0, 0.0, 0.0],
            [
                1.0 / 3.0f64.sqrt(),
                1.0 / 3.0f64.sqrt(),
                1.0 / 3.0f64.sqrt(),
            ],
        ];
        let expected_tris = [
            [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        ];
        for i in 0..count as usize {
            let base = 84 + 50 * i;
            let rec = &buf[base..base + 50];
            let n = [le_f32(rec, 0), le_f32(rec, 4), le_f32(rec, 8)];
            let len =
                (n[0] as f64 * n[0] as f64 + n[1] as f64 * n[1] as f64 + n[2] as f64 * n[2] as f64)
                    .sqrt();
            assert!(
                (len - 1.0).abs() < 1e-3 || len == 0.0,
                "normal must be unit-length or zero, got length {len}"
            );
            for k in 0..3 {
                assert!(
                    (n[k] as f64 - expected_normals[i][k]).abs() < 1e-3,
                    "triangle {i} normal component {k}"
                );
            }
            for vi in 0..3 {
                for k in 0..3 {
                    let got = le_f32(rec, 12 + 12 * vi + 4 * k) as f64;
                    assert!(
                        (got - expected_tris[i][vi][k]).abs() < 1e-3,
                        "triangle {i} vertex {vi} component {k}: {got} vs {}",
                        expected_tris[i][vi][k]
                    );
                }
            }
            assert_eq!(le_u16(rec, 48), 0, "attribute byte count must be 0");
        }
    }

    #[test]
    fn stl_binary_size_helper() {
        assert_eq!(stl_binary_size(0), 84);
        assert_eq!(stl_binary_size(1), 134);
        assert_eq!(stl_binary_size(4), 284);
        assert_eq!(stl_binary_size(968), 48484);
        assert_eq!(stl_binary_size(1000), 50084);
    }

    #[test]
    fn stl_binary_long_name_is_truncated_at_80_bytes() {
        let name = "x".repeat(100);
        let mut buf = Vec::new();
        write_stl_binary(&name, &[], &[], &mut buf).unwrap();
        assert_eq!(buf.len(), 84);
        assert_eq!(&buf[0..80], "x".repeat(80).as_bytes());
        assert_eq!(u32::from_le_bytes(buf[80..84].try_into().unwrap()), 0);
    }

    #[test]
    fn stl_binary_multibyte_name_truncates_on_char_boundary() {
        // "a" + 80 two-byte 'é' chars = 161 bytes: byte 80 is the second
        // byte of the 40th 'é', so truncation must back up to byte 79 and
        // keep 40 whole characters ('a' + 39 'é'), zero-padded after.
        let name = format!("a{}", "\u{e9}".repeat(80));
        let mut buf = Vec::new();
        write_stl_binary(&name, &[], &[], &mut buf).unwrap();
        assert_eq!(buf.len(), 84);
        let header = &buf[0..80];
        assert_eq!(&header[0..1], b"a");
        // No UTF-8 character was split, and the tail is zero-padded.
        assert!(std::str::from_utf8(header).is_ok());
        let decoded = std::str::from_utf8(header).unwrap();
        assert_eq!(decoded.chars().filter(|&c| c == '\u{e9}').count(), 39);
        assert_eq!(decoded.chars().filter(|&c| c == 'a').count(), 1);
        assert_eq!(decoded.chars().filter(|&c| c == '\0').count(), 1);
        assert_eq!(&header[79..80], &[0]);
    }

    // -----------------------------------------------------------------
    // Empty meshes
    // -----------------------------------------------------------------

    #[test]
    fn empty_meshes_are_valid() {
        // OBJ: header comments only.
        let mut buf = Vec::new();
        write_obj(&[], &[], &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "# freecad-rs export\n# vertices: 0\n# triangles: 0\n"
        );

        // ASCII STL: bare solid/endsolid pair.
        let mut buf = Vec::new();
        write_stl_ascii("empty", &[], &[], &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "solid empty\nendsolid empty\n"
        );

        // Binary STL: exactly 84 bytes, byte-exact, count 0.
        let mut buf = Vec::new();
        write_stl_binary("empty", &[], &[], &mut buf).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(b"empty");
        expected.resize(80, 0);
        expected.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(buf.len(), 84);
        assert_eq!(buf, expected);

        // Binary STL with an empty name: fully zero header.
        let mut buf = Vec::new();
        write_stl_binary("", &[], &[], &mut buf).unwrap();
        assert_eq!(buf, vec![0u8; 84]);
    }

    // -----------------------------------------------------------------
    // ~1k-triangle generated mesh
    // -----------------------------------------------------------------

    #[test]
    fn grid_mesh_around_1k_triangles() {
        // 23x23 vertex grid (529 shared vertices), 968 triangles.
        let side = 22usize;
        let mut verts = Vec::new();
        for i in 0..=side {
            for j in 0..=side {
                let (x, y) = (i as f64, j as f64);
                verts.push(Vector3::new(x, y, (0.3 * x).sin() + (0.2 * y).cos()));
            }
        }
        let idx = |i: usize, j: usize| (i * (side + 1) + j) as u32;
        let mut tris = Vec::new();
        for i in 0..side {
            for j in 0..side {
                let (a, b, c, d) = (idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1));
                tris.push([a, b, c]);
                tris.push([a, c, d]);
            }
        }
        assert_eq!(verts.len(), 529);
        assert_eq!(tris.len(), 968);

        // OBJ: all three writers succeed, line counts consistent.
        let mut obj = Vec::new();
        write_obj(&verts, &tris, &mut obj).unwrap();
        let text = String::from_utf8(obj).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("v ")).count(), 529);
        assert_eq!(text.lines().filter(|l| l.starts_with("f ")).count(), 968);

        // ASCII STL: 968 facets, proper solid/endsolid bookends.
        let mut ascii = Vec::new();
        write_stl_ascii("grid", &verts, &tris, &mut ascii).unwrap();
        let text = String::from_utf8(ascii).unwrap();
        assert!(text.starts_with("solid grid\n"));
        assert!(text.ends_with("endsolid grid\n"));
        assert_eq!(text.matches("endfacet").count(), 968);
        assert_eq!(text.matches("vertex ").count(), 968 * 3);

        // Binary STL: exact size, count, and all records well-formed.
        let mut bin = Vec::new();
        write_stl_binary("grid", &verts, &tris, &mut bin).unwrap();
        assert_eq!(bin.len(), stl_binary_size(968));
        assert_eq!(bin.len(), 48484);
        assert_eq!(u32::from_le_bytes(bin[80..84].try_into().unwrap()), 968);
        for i in 0..968usize {
            let base = 84 + 50 * i;
            let rec = &bin[base..base + 50];
            let n = [le_f32(rec, 0), le_f32(rec, 4), le_f32(rec, 8)];
            let len =
                (n[0] as f64 * n[0] as f64 + n[1] as f64 * n[1] as f64 + n[2] as f64 * n[2] as f64)
                    .sqrt();
            assert!(
                (len - 1.0).abs() < 1e-3 || len == 0.0,
                "record {i}: normal length {len}"
            );
            assert_eq!(le_u16(rec, 48), 0, "record {i}: attribute count");
        }
    }

    // -----------------------------------------------------------------
    // Rejected inputs (no panics)
    // -----------------------------------------------------------------

    #[test]
    fn out_of_range_indices_are_errors_not_panics() {
        let verts = vec![Vector3::ZERO, Vector3::X, Vector3::Y];
        let bad = vec![[0u32, 1, 7], [0, 3, 2]];
        let mut buf = Vec::new();
        assert!(write_obj(&verts, &bad, &mut buf).is_err());
        let mut buf = Vec::new();
        assert!(write_stl_ascii("bad", &verts, &bad, &mut buf).is_err());
        let mut buf = Vec::new();
        assert!(write_stl_binary("bad", &verts, &bad, &mut buf).is_err());
    }
}
