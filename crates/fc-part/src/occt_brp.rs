//! Reading real Open CASCADE BRep payloads (`DBRep_DrawableShape`,
//! the "CASCADE Topology V3" dialect FreeCAD writes as `.brp` into
//! its `.FCStd` archives) into the exact topology model — the K4
//! "NURBS operands load" clause, and ADR-0001 trigger 3 ("native
//! `.brp`/OCCT file fidelity") answered without binding OCCT: the
//! format is parsed, not linked.
//!
//! # The format (as OCCT 7.8 writes it)
//!
//! ```text
//! DBRep_DrawableShape
//! <blank>
//! CASCADE Topology V3, (c) Open Cascade
//! Locations <n>        ; "1 <3x4 matrix>" datums, "2 (d e)* 0" chains
//! Curve2ds <n>         ; 2D trims: line(1) circle(2) ellipse(3) bspline(7)
//! Curves <n>           ; 3D: line(1) circle(2) ellipse(3) bspline(7)
//! Polygon3D <n>        ; refused unless 0 (no tessellation payloads)
//! PolygonOnTriangulations <n>
//! Surfaces <n>         ; plane(1) cyl(2) cone(3) sph(4) tor(5) bspline(9)
//! Triangulations <n>
//! TShapes <n>          ; Ve/Ed/Wi/Fa/Sh/So records, children-first
//! <root ref>           ; "+<idx> <loc>" — sign × shape × location
//! ```
//!
//! Reference arithmetic (the part every hand-rolled reader gets
//! wrong): TShapes are numbered 1..n in **file order** (a reference
//! `-34` means record 34), the sign is the child's orientation in its
//! parent (`+` forward, `-` reversed), and an edge's vertex children
//! carry **start `+`, end `-`**. Location chains list their
//! `(datum, exponent)` pairs first-applied first, and locations
//! compose down the tree (root ∘ reference ∘ record).
//!
//! # The reconciliation tier (the honesty contract)
//!
//! OCCT's own format is *not* bit-exact: vertex coordinates are
//! written at 15 significant digits while curves carry 17, and OCCT
//! reconciles the difference through per-vertex tolerances. This
//! model's contract is stricter — [`crate::topology::ExactBRep::validate`]
//! demands each edge's curve reproduce its vertices **bit-exactly**.
//! The reader therefore never trusts a vertex point to also be a
//! curve evaluation; it *derives* the model from the curves:
//!
//! * Every edge endpoint **resolves** its file vertex to the edge's
//!   own curve evaluation at that endpoint's parameter (recovering,
//!   for a line at `t = 0`, the full 17-digit origin OCCT truncated).
//!   Edges are processed non-degenerate first, in file order, so the
//!   deterministic first evaluation defines the model vertex.
//! * A later edge meeting an already-resolved vertex **pins** to it:
//!   lines become degree-1 B-splines through the resolved points (de
//!   Boor returns the endpoints exactly — the K3c-3 seam pattern),
//!   arcs become quadrant-split rational quadratic B-splines with the
//!   end poles replaced (the K3d NURBS arc), a closed circle becomes
//!   the classical 9-pole NURBS circle with `P0 = P8 =` the resolved
//!   vertex, and B-spline curves with full-domain ranges have their
//!   end poles replaced.
//! * Degenerate edges (sphere poles) are zero-radius circles centered
//!   on the resolved vertex — exactly the K1 sphere wire.
//!
//! Every pin moves geometry by at most the file's own vertex
//! tolerance (measured, reported in [`OcctReadReport`]); anything
//! beyond it is a refusal naming the entity, never a silent snap.
//!
//! What is refused by name: non-zero polygon/triangulation sections,
//! periodic B-splines, offset/revolution/extrusion/trimmed/bezier
//! surfaces, natural-restriction (untrimmed periodic) faces, reversed
//! roots and solids with inner (void) shells, edges whose endpoints
//! sit beyond the file's own tolerance, and roots that are not a
//! solid or a shell.

use std::collections::HashMap;

use fc_base::Vector3;

use crate::topology::{
    validate_curve, validate_surface, Curve, ExactBRep, Surface, TopoEdge, TopoFace, TopoVertex,
    TopoWire, WireEdge,
};

// ---------------------------------------------------------------------------
// lexer
// ---------------------------------------------------------------------------

/// Whitespace-token stream. The format is not line-oriented (pole
/// blocks break lines at arbitrary counts), so everything is a token.
struct Tokens<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Tokens<'a> {
    fn new(src: &'a str) -> Self {
        Tokens { src, pos: 0 }
    }

    fn next(&mut self) -> Result<&'a str, String> {
        let b = self.src.as_bytes();
        while self.pos < b.len() && b[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        if self.pos >= b.len() {
            return Err("unexpected end of payload".to_string());
        }
        let start = self.pos;
        while self.pos < b.len() && !b[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        Ok(&self.src[start..self.pos])
    }

    fn f64(&mut self) -> Result<f64, String> {
        let t = self.next()?;
        t.parse::<f64>()
            .map_err(|_| format!("expected a number, got '{t}'"))
    }

    fn i64(&mut self) -> Result<i64, String> {
        let t = self.next()?;
        t.parse::<i64>()
            .map_err(|_| format!("expected an integer, got '{t}'"))
    }

    fn usize(&mut self) -> Result<usize, String> {
        let v = self.i64()?;
        if v < 0 {
            return Err(format!("expected a non-negative index, got {v}"));
        }
        Ok(v as usize)
    }

    fn expect(&mut self, word: &str) -> Result<(), String> {
        let t = self.next()?;
        if t != word {
            return Err(format!("expected '{word}', got '{t}'"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// locations
// ---------------------------------------------------------------------------

/// A rigid transform `x ↦ R·x + t` (an OCCT `gp_Trsf`'s rotational
/// part plus translation). Identity is detected exactly so that
/// unlocated geometry passes through **bit-unchanged**.
#[derive(Clone, Copy, Debug)]
struct Trsf {
    r: [[f64; 3]; 3],
    t: [f64; 3],
    identity: bool,
}

impl Trsf {
    fn identity() -> Self {
        Trsf {
            r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            t: [0.0, 0.0, 0.0],
            identity: true,
        }
    }

    /// From twelve numbers: three rows of `[r11 r12 r13 | t]`.
    fn from_rows(rows: &[[f64; 4]; 3]) -> Self {
        let identity = (0..3).all(|i| {
            (0..3).all(|j| rows[i][j] == if i == j { 1.0 } else { 0.0 }) && rows[i][3] == 0.0
        });
        Trsf {
            r: [
                [rows[0][0], rows[0][1], rows[0][2]],
                [rows[1][0], rows[1][1], rows[1][2]],
                [rows[2][0], rows[2][1], rows[2][2]],
            ],
            t: [rows[0][3], rows[1][3], rows[2][3]],
            identity,
        }
    }

    /// `self` applied after `other` (`x ↦ self(other(x))`).
    fn compose(&self, other: &Trsf) -> Trsf {
        if self.identity {
            return *other;
        }
        if other.identity {
            return *self;
        }
        let mut out = Trsf {
            r: [[0.0f64; 3]; 3],
            t: [0.0f64; 3],
            identity: false,
        };
        for (i, ri) in out.r.iter_mut().enumerate() {
            for (j, rij) in ri.iter_mut().enumerate() {
                *rij = self.r[i][0] * other.r[0][j]
                    + self.r[i][1] * other.r[1][j]
                    + self.r[i][2] * other.r[2][j];
            }
        }
        for (i, ti) in out.t.iter_mut().enumerate() {
            *ti = self.r[i][0] * other.t[0]
                + self.r[i][1] * other.t[1]
                + self.r[i][2] * other.t[2]
                + self.t[i];
        }
        out
    }

    fn point(&self, p: Vector3) -> Vector3 {
        if self.identity {
            return p;
        }
        Vector3::new(
            self.r[0][0] * p.x + self.r[0][1] * p.y + self.r[0][2] * p.z + self.t[0],
            self.r[1][0] * p.x + self.r[1][1] * p.y + self.r[1][2] * p.z + self.t[1],
            self.r[2][0] * p.x + self.r[2][1] * p.y + self.r[2][2] * p.z + self.t[2],
        )
    }

    fn direction(&self, d: Vector3) -> Vector3 {
        if self.identity {
            return d;
        }
        Vector3::new(
            self.r[0][0] * d.x + self.r[0][1] * d.y + self.r[0][2] * d.z,
            self.r[1][0] * d.x + self.r[1][1] * d.y + self.r[1][2] * d.z,
            self.r[2][0] * d.x + self.r[2][1] * d.y + self.r[2][2] * d.z,
        )
    }
}

/// A hashable transform identity (bit-level: two transforms are the
/// same key when every component is bit-equal).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TrsfKey([u64; 12]);

impl TrsfKey {
    fn of(t: &Trsf) -> Self {
        let mut k = [0u64; 12];
        let mut i = 0;
        for row in &t.r {
            for v in row {
                k[i] = v.to_bits();
                i += 1;
            }
        }
        for v in &t.t {
            k[i] = v.to_bits();
            i += 1;
        }
        TrsfKey(k)
    }
}

/// One location-table entry: an inline datum or a chain.
enum LocRec {
    Datum(Trsf),
    Chain(Vec<(usize, usize)>),
}

// ---------------------------------------------------------------------------
// 2D trims (the UV bounds source)
// ---------------------------------------------------------------------------

/// A parameter-space curve. Parameterizations follow OCCT's
/// `Geom2d`: lines `o + d·t`, conics `c + r·(cos t, sin t)` in the
/// stored axes, B-splines by de Boor (poles only feed the hull).
enum Curve2d {
    Line {
        o: [f64; 2],
        d: [f64; 2],
    },
    Conic {
        c: [f64; 2],
        r1: f64,
        r2: f64,
    },
    /// Poles only (the convex hull bounds the curve).
    BsplinePoles {
        poles: Vec<[f64; 2]>,
    },
}

impl Curve2d {
    fn point(&self, t: f64) -> [f64; 2] {
        match self {
            Curve2d::Line { o, d } => [o[0] + d[0] * t, o[1] + d[1] * t],
            Curve2d::Conic { c, r1, r2 } => {
                let (s, co) = t.sin_cos();
                [c[0] + r1 * co, c[1] + r2 * s]
            }
            Curve2d::BsplinePoles { .. } => [0.0, 0.0],
        }
    }

    /// Accumulate the 2D bounding box of the trim over `[a, b]`.
    fn accumulate_bounds(&self, a: f64, b: f64, bounds: &mut [f64; 4]) {
        if !a.is_finite() || !b.is_finite() || a == b {
            return;
        }
        let mut consider = |p: [f64; 2]| {
            bounds[0] = bounds[0].min(p[0]);
            bounds[1] = bounds[1].max(p[0]);
            bounds[2] = bounds[2].min(p[1]);
            bounds[3] = bounds[3].max(p[1]);
        };
        consider(self.point(a));
        consider(self.point(b));
        if let Curve2d::Conic { .. } = self {
            // Extremes sit at quadrant parameters within [a, b].
            let mut k = (a / std::f64::consts::FRAC_PI_2).floor() as i64;
            loop {
                let t = k as f64 * std::f64::consts::FRAC_PI_2;
                if t > b {
                    break;
                }
                if t >= a {
                    consider(self.point(t));
                }
                k += 1;
            }
        }
        if let Curve2d::BsplinePoles { poles } = self {
            for p in poles {
                consider(*p);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TShapes
// ---------------------------------------------------------------------------

/// A child reference: sign, record index (file order, 1-based), and
/// a location index (0 = identity).
struct SRef {
    reversed: bool,
    record: usize,
    loc: usize,
}

enum TShape {
    Vertex {
        tol: f64,
        point: Vector3,
    },
    Edge {
        degenerated: bool,
        /// `(curve record, location, first, last)` when a 3D curve exists.
        curve3d: Option<(usize, usize, f64, f64)>,
        /// `(pcurve record, surface record, location, first, last)`;
        /// on closed surfaces the seam comes as two records.
        pcurves: Vec<(usize, usize, usize, f64, f64)>,
        /// The vertex children: start carries `+`, end `-`.
        vertices: Vec<SRef>,
    },
    Face {
        natural_restriction: bool,
        surface: usize,
        loc: usize,
        children: Vec<SRef>,
    },
    Container {
        kind: &'static str,
        children: Vec<SRef>,
    },
}

// ---------------------------------------------------------------------------
// public result
// ---------------------------------------------------------------------------

/// The honest reading report: what was kept, what was pinned, and
/// how far any pin moved geometry (always against the file's own
/// vertex tolerances).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OcctReadReport {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub pinned_lines: usize,
    pub pinned_arcs: usize,
    pub repinned_circles: usize,
    pub pinned_splines: usize,
    pub degenerate_edges: usize,
    /// The largest endpoint displacement of any pin, against the file
    /// curve's own evaluation.
    pub max_pin_deviation: f64,
}

/// A successfully read payload: the exact B-rep plus the report.
#[derive(Debug, Clone)]
pub struct OcctRead {
    pub brep: ExactBRep,
    pub report: OcctReadReport,
}

/// Parse an OCCT BRep payload (the `DBRep_DrawableShape` /
/// `CASCADE Topology V3` dialect) into the exact topology model.
pub fn read_occt_brp(data: &str) -> Result<OcctRead, String> {
    let (reader, root) = Reader::parse(data)?;
    reader.build(root)
}

// ---------------------------------------------------------------------------
// the reader
// ---------------------------------------------------------------------------

struct Reader {
    locations: Vec<LocRec>,
    curve2ds: Vec<Curve2d>,
    curves: Vec<Curve>,
    /// Per curve record: an *indirect* frame (the stored YAxis
    /// opposes axis×XAxis) — OCCT parametrizes such curves with the
    /// mirrored second basis, i.e. `t → −t` against our right-handed
    /// reading.
    curve_indirect: Vec<bool>,
    surfaces: Vec<Surface>,
    /// Per surface record: an indirect Ax3 — the file's natural normal
    /// is the negation of our right-handed model's.
    surface_indirect: Vec<bool>,
    tshapes: Vec<TShape>,
}

impl Reader {
    fn parse(data: &str) -> Result<(Reader, SRef), String> {
        // The header is probed textually (the version line's wording
        // varies across V1/V3 and writers), then skipped up to the
        // "Locations" section keyword.
        if !data.contains("CASCADE Topology") {
            return Err("not an OCCT BRep payload (no CASCADE Topology header)".to_string());
        }
        let mut tk = Tokens::new(data);
        let mut saw_cascade = false;
        loop {
            let t = tk.next()?;
            if t == "Locations" {
                if !saw_cascade {
                    return Err("a Locations section without the CASCADE header".to_string());
                }
                break;
            }
            if t.starts_with("CASCADE") {
                saw_cascade = true;
            }
        }

        let locations = parse_locations(&mut tk)?;
        let curve2ds = parse_curve2ds(&mut tk)?;
        let (curves, curve_indirect) = parse_curves(&mut tk)?;
        tk.expect("Polygon3D")?;
        if tk.i64()? != 0 {
            return Err(
                "the payload carries Polygon3D tessellation — not modeled (export without \
                 triangulation)"
                    .to_string(),
            );
        }
        tk.expect("PolygonOnTriangulations")?;
        if tk.i64()? != 0 {
            return Err("the payload carries PolygonOnTriangulations — not modeled".to_string());
        }
        let (surfaces, surface_indirect) = parse_surfaces(&mut tk)?;
        tk.expect("Triangulations")?;
        if tk.i64()? != 0 {
            return Err("the payload carries Triangulations — not modeled".to_string());
        }
        let (tshapes, root) = parse_tshapes(&mut tk)?;
        if tshapes.is_empty() {
            return Err("TShapes is empty".to_string());
        }
        Ok((
            Reader {
                locations,
                curve2ds,
                curves,
                curve_indirect,
                surfaces,
                surface_indirect,
                tshapes,
            },
            root,
        ))
    }

    /// Resolve a location index to its total transform (identity for
    /// index 0). Chain pairs are first-applied first.
    fn location(&self, idx: usize) -> Result<Trsf, String> {
        if idx == 0 {
            return Ok(Trsf::identity());
        }
        if idx > self.locations.len() {
            return Err(format!(
                "reference to location {idx} of {}",
                self.locations.len()
            ));
        }
        match &self.locations[idx - 1] {
            LocRec::Datum(t) => Ok(*t),
            LocRec::Chain(pairs) => {
                // Pairs are first-applied first: after the first
                // datum, each subsequent one pre-composes on the
                // left (total = datum ∘ … ∘ first).
                let mut total = Trsf::identity();
                for &(d, exp) in pairs {
                    if d == 0 || d > self.locations.len() {
                        return Err(format!(
                            "location chain references datum {d} of {}",
                            self.locations.len()
                        ));
                    }
                    let datum = match &self.locations[d - 1] {
                        LocRec::Datum(t) => *t,
                        LocRec::Chain(_) => {
                            return Err(format!(
                                "location {idx}: a chain referencing a chain — not a datum"
                            ))
                        }
                    };
                    for _ in 0..exp {
                        total = datum.compose(&total);
                    }
                }
                Ok(total)
            }
        }
    }

    fn tshape(&self, record: usize, what: &str) -> Result<&TShape, String> {
        self.tshapes.get(record - 1).ok_or_else(|| {
            format!(
                "reference to TShape {record} ({what}) of {}",
                self.tshapes.len()
            )
        })
    }

    fn curve(&self, idx: usize, what: &str) -> Result<&Curve, String> {
        self.curves
            .get(idx - 1)
            .ok_or_else(|| format!("reference to curve {idx} ({what}) of {}", self.curves.len()))
    }

    fn surface(&self, idx: usize, what: &str) -> Result<&Surface, String> {
        self.surfaces.get(idx - 1).ok_or_else(|| {
            format!(
                "reference to surface {idx} ({what}) of {}",
                self.surfaces.len()
            )
        })
    }

    fn curve2d(&self, idx: usize, what: &str) -> Result<&Curve2d, String> {
        self.curve2ds.get(idx - 1).ok_or_else(|| {
            format!(
                "reference to pcurve {idx} ({what}) of {}",
                self.curve2ds.len()
            )
        })
    }

    // -- geometry placement ------------------------------------------------

    fn place_curve(&self, curve: &Curve, trsf: &Trsf) -> Curve {
        if trsf.identity {
            return curve.clone();
        }
        match curve {
            Curve::Line { origin, direction } => Curve::Line {
                origin: trsf.point(*origin),
                direction: trsf.direction(*direction),
            },
            Curve::Circle {
                center,
                axis,
                ref_dir,
                radius,
            } => Curve::Circle {
                center: trsf.point(*center),
                axis: trsf.direction(*axis),
                ref_dir: trsf.direction(*ref_dir),
                radius: *radius,
            },
            Curve::Ellipse {
                center,
                axis,
                major_dir,
                major_radius,
                minor_radius,
            } => Curve::Ellipse {
                center: trsf.point(*center),
                axis: trsf.direction(*axis),
                major_dir: trsf.direction(*major_dir),
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            },
            Curve::Bspline {
                degree,
                knots,
                points,
                weights,
            } => Curve::Bspline {
                degree: *degree,
                knots: knots.clone(),
                points: points.iter().map(|p| trsf.point(*p)).collect(),
                weights: weights.clone(),
            },
        }
    }

    fn place_surface(&self, surface: &Surface, trsf: &Trsf) -> Surface {
        if trsf.identity {
            return surface.clone();
        }
        match surface {
            Surface::Plane {
                origin,
                normal,
                x_dir,
            } => Surface::Plane {
                origin: trsf.point(*origin),
                normal: trsf.direction(*normal),
                x_dir: trsf.direction(*x_dir),
            },
            Surface::Cylinder {
                origin,
                axis,
                ref_dir,
                radius,
            } => Surface::Cylinder {
                origin: trsf.point(*origin),
                axis: trsf.direction(*axis),
                ref_dir: trsf.direction(*ref_dir),
                radius: *radius,
            },
            Surface::Cone {
                origin,
                axis,
                ref_dir,
                semi_angle,
                ref_radius,
            } => Surface::Cone {
                origin: trsf.point(*origin),
                axis: trsf.direction(*axis),
                ref_dir: trsf.direction(*ref_dir),
                semi_angle: *semi_angle,
                ref_radius: *ref_radius,
            },
            Surface::Sphere {
                center,
                axis,
                ref_dir,
                radius,
            } => Surface::Sphere {
                center: trsf.point(*center),
                axis: trsf.direction(*axis),
                ref_dir: trsf.direction(*ref_dir),
                radius: *radius,
            },
            Surface::Torus {
                center,
                axis,
                ref_dir,
                major_radius,
                minor_radius,
            } => Surface::Torus {
                center: trsf.point(*center),
                axis: trsf.direction(*axis),
                ref_dir: trsf.direction(*ref_dir),
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            },
            Surface::Bspline {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                points,
                weights,
            } => Surface::Bspline {
                u_degree: *u_degree,
                v_degree: *v_degree,
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
                points: points
                    .iter()
                    .map(|row| row.iter().map(|p| trsf.point(*p)).collect())
                    .collect(),
                weights: weights.clone(),
            },
        }
    }

    // -- the build ----------------------------------------------------------

    fn build(self, root: SRef) -> Result<OcctRead, String> {
        if root.reversed {
            return Err(
                "the root shape is REVERSED — complement solids are not modeled".to_string(),
            );
        }
        let mut faces: Vec<(SRef, Trsf)> = Vec::new();
        // `collect_faces` applies each node's own location, so the
        // walk starts from the identity (the root's location is
        // applied there — exactly once).
        self.collect_faces(&root, Trsf::identity(), &mut faces, "the root")?;
        let builder = Builder {
            reader: &self,
            vertices: Vec::new(),
            edges: Vec::new(),
            resolved: HashMap::new(),
            edge_of_key: HashMap::new(),
            report: OcctReadReport::default(),
        };
        builder.build_faces(&faces)
    }

    /// Recursively collect `(face reference, shell-frame transform)`
    /// pairs: compounds wrap solids, solids wrap one shell, shells
    /// hold the faces. A single face may stand alone as the root.
    fn collect_faces(
        &self,
        node: &SRef,
        trsf: Trsf,
        out: &mut Vec<(SRef, Trsf)>,
        what: &str,
    ) -> Result<(), String> {
        let here = trsf.compose(&self.location(node.loc)?);
        match self.tshape(node.record, what)? {
            TShape::Face { .. } => {
                out.push((clone_ref(node), here));
                Ok(())
            }
            TShape::Container { kind, children } => match *kind {
                "Sh" => {
                    for ch in children {
                        if ch.reversed {
                            // A REVERSED face in the shell is the
                            // ordinary orientation flag; handled by the
                            // builder. Reaching here means the SHELL
                            // itself was used reversed:
                        }
                        self.collect_faces(ch, here, out, "a shell's face")?;
                    }
                    Ok(())
                }
                "So" | "Co" | "CS" => {
                    let (child, what2) = match children.len() {
                        0 => return Err(format!("{what} has no children")),
                        1 => (&children[0], "the solid's shell"),
                        n => {
                            return Err(format!(
                                "{what} carries {n} children — compounds of several solids \
                                 and inner (void) shells are not modeled yet"
                            ))
                        }
                    };
                    if child.reversed && *kind == "So" {
                        return Err(
                            "a REVERSED shell in the solid — inner voids are not modeled"
                                .to_string(),
                        );
                    }
                    self.collect_faces(child, here, out, what2)
                }
                _ => Err(format!("{what} is a wire — not a face container")),
            },
            _ => Err(format!(
                "{what} is not a solid, shell or face — not modeled yet"
            )),
        }
    }
}

/// Clone a shared reference (they are read-only).
fn clone_ref(r: &SRef) -> SRef {
    SRef {
        reversed: r.reversed,
        record: r.record,
        loc: r.loc,
    }
}

// ---------------------------------------------------------------------------
// section parsers
// ---------------------------------------------------------------------------

fn parse_locations(tk: &mut Tokens<'_>) -> Result<Vec<LocRec>, String> {
    let n = tk.usize()?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let tag = tk.next()?;
        match tag {
            "1" => {
                let mut rows = [[0.0f64; 4]; 3];
                for row in &mut rows {
                    for v in row.iter_mut() {
                        *v = tk.f64()?;
                    }
                }
                out.push(LocRec::Datum(Trsf::from_rows(&rows)));
            }
            "2" => {
                let mut pairs = Vec::new();
                loop {
                    let d = tk.usize()?;
                    if d == 0 {
                        break;
                    }
                    let e = tk.usize()?;
                    if e == 0 {
                        return Err(format!(
                            "location {i}: a chain pair with exponent 0 — malformed"
                        ));
                    }
                    pairs.push((d, e));
                }
                if pairs.is_empty() {
                    return Err(format!("location {i}: an empty chain"));
                }
                out.push(LocRec::Chain(pairs));
            }
            other => {
                return Err(format!(
                    "location {i}: unknown record tag '{other}' (expected 1 or 2)"
                ))
            }
        }
    }
    Ok(out)
}

fn parse_curve2ds(tk: &mut Tokens<'_>) -> Result<Vec<Curve2d>, String> {
    tk.expect("Curve2ds")?;
    let n = tk.usize()?;
    let mut out = Vec::with_capacity(n);
    for i in 1..=n {
        let ty = tk.next()?;
        let c = read_curve2d_record(tk, ty, i, 0)?;
        out.push(c);
    }
    Ok(out)
}

/// One `Curve2ds` record (a trimmed 2D curve unwraps to its basis,
/// same parameter space).
fn read_curve2d_record(
    tk: &mut Tokens<'_>,
    ty: &str,
    i: usize,
    depth: usize,
) -> Result<Curve2d, String> {
    match ty {
        "1" => {
            let o = [tk.f64()?, tk.f64()?];
            let d = [tk.f64()?, tk.f64()?];
            Ok(Curve2d::Line { o, d })
        }
        "2" => {
            // "2 cx cy xdx xdy ydx ydy r" (7 numbers).
            let c = [tk.f64()?, tk.f64()?];
            for _ in 0..4 {
                tk.f64()?; // XAxis, YAxis directions
            }
            let r = tk.f64()?;
            Ok(Curve2d::Conic { c, r1: r, r2: r })
        }
        "3" => {
            // "3 cx cy xdx xdy ydx ydy r1 r2" (8 numbers).
            let c = [tk.f64()?, tk.f64()?];
            for _ in 0..4 {
                tk.f64()?;
            }
            let r1 = tk.f64()?;
            let r2 = tk.f64()?;
            Ok(Curve2d::Conic { c, r1, r2 })
        }
        "8" => {
            if depth >= 4 {
                return Err(format!(
                    "pcurve {i}: trimmed curves nested {depth} deep — not modeled"
                ));
            }
            tk.f64()?; // first
            tk.f64()?; // last
            let basis_ty = tk.next()?;
            read_curve2d_record(tk, basis_ty, i, depth + 1)
        }
        "7" => {
            let rational = tk.i64()?;
            let periodic = tk.i64()?;
            if periodic != 0 {
                return Err(format!(
                    "pcurve {i}: a periodic B-spline 2D curve — not modeled"
                ));
            }
            let degree = tk.usize()?;
            let npoles = tk.usize()?;
            let nknots = tk.usize()?;
            let mut poles = Vec::with_capacity(npoles);
            for _ in 0..npoles {
                let x = tk.f64()?;
                let y = tk.f64()?;
                if rational == 1 {
                    tk.f64()?;
                }
                poles.push([x, y]);
            }
            let mut flat = 0usize;
            for _ in 0..nknots {
                tk.f64()?;
                let m = tk.usize()?;
                flat += m;
            }
            if flat != npoles + degree + 1 {
                return Err(format!(
                    "pcurve {i}: B-spline bookkeeping ({} flat knots, {} poles, degree \
                     {degree})",
                    flat, npoles
                ));
            }
            Ok(Curve2d::BsplinePoles { poles })
        }
        other => Err(format!(
            "pcurve {i}: curve kind '{other}' — not modeled (only line, circle, ellipse, \
             B-spline)"
        )),
    }
}

fn parse_curves(tk: &mut Tokens<'_>) -> Result<(Vec<Curve>, Vec<bool>), String> {
    tk.expect("Curves")?;
    let n = tk.usize()?;
    let mut out = Vec::with_capacity(n);
    let mut indirect = Vec::with_capacity(n);
    for i in 1..=n {
        let ty = tk.next()?;
        let (curve, ind) = read_curve_record(tk, ty, i, 0)?;
        validate_curve(&curve).map_err(|e| format!("curve {i}: {e}"))?;
        out.push(curve);
        indirect.push(ind);
    }
    Ok((out, indirect))
}

/// Whether an (axis, xaxis, yaxis) triple is an *indirect* frame:
/// yaxis opposing axis×xaxis. OCCT parametrizes conics and the
/// analytic surfaces over the stored pair as-is, so an indirect
/// record's natural orientation is mirrored against the right-handed
/// reading this model normalizes to.
fn indirect_frame(axis: Vector3, x_axis: Vector3, y_axis: Vector3) -> bool {
    let y_right = axis.cross(&x_axis);
    y_right.dot(&y_axis) < 0.0
}

/// One `Curves` record. A trimmed curve (kind 8) is "8 first last"
/// followed by its basis curve — the same parameter space, so the
/// trim is redundant with the edge's own range and unwraps to the
/// basis.
fn read_curve_record(
    tk: &mut Tokens<'_>,
    ty: &str,
    i: usize,
    depth: usize,
) -> Result<(Curve, bool), String> {
    {
        let curve = match ty {
            "1" => {
                let origin = v3f(tk)?;
                let direction = v3f(tk)?;
                (Curve::Line { origin, direction }, false)
            }
            "2" => {
                let center = v3f(tk)?;
                let axis = v3f(tk)?;
                let ref_dir = v3f(tk)?;
                let y_dir = v3f(tk)?;
                let radius = tk.f64()?;
                (
                    Curve::Circle {
                        center,
                        axis,
                        ref_dir,
                        radius,
                    },
                    indirect_frame(axis, ref_dir, y_dir),
                )
            }
            "3" => {
                let center = v3f(tk)?;
                let axis = v3f(tk)?;
                let major_dir = v3f(tk)?;
                let y_dir = v3f(tk)?;
                let major_radius = tk.f64()?;
                let minor_radius = tk.f64()?;
                (
                    Curve::Ellipse {
                        center,
                        axis,
                        major_dir,
                        major_radius,
                        minor_radius,
                    },
                    indirect_frame(axis, major_dir, y_dir),
                )
            }
            "7" => {
                let rational = tk.i64()?;
                let periodic = tk.i64()?;
                if periodic != 0 {
                    return Err(format!(
                        "curve {i}: a periodic B-spline — not modeled (use the circle form)"
                    ));
                }
                let degree = tk.usize()?;
                let npoles = tk.usize()?;
                let nknots = tk.usize()?;
                let mut points = Vec::with_capacity(npoles);
                let mut weights = if rational == 1 {
                    Some(Vec::with_capacity(npoles))
                } else {
                    None
                };
                for _ in 0..npoles {
                    points.push(v3f(tk)?);
                    if let Some(w) = weights.as_mut() {
                        w.push(tk.f64()?);
                    }
                }
                let mut knots = Vec::new();
                for _ in 0..nknots {
                    let k = tk.f64()?;
                    let m = tk.usize()?;
                    for _ in 0..m {
                        knots.push(k);
                    }
                }
                (
                    Curve::Bspline {
                        degree,
                        knots,
                        points,
                        weights,
                    },
                    false,
                )
            }
            "8" => {
                if depth >= 4 {
                    return Err(format!(
                        "curve {i}: trimmed curves nested {depth} deep — not modeled"
                    ));
                }
                tk.f64()?; // the trim's first parameter
                tk.f64()?; // the trim's last parameter
                let basis_ty = tk.next()?;
                return read_curve_record(tk, basis_ty, i, depth + 1);
            }
            other => {
                return Err(format!(
                    "curve {i}: kind '{other}' — not modeled (only line, circle, ellipse, \
                     B-spline)"
                ))
            }
        };
        Ok(curve)
    }
}

fn parse_surfaces(tk: &mut Tokens<'_>) -> Result<(Vec<Surface>, Vec<bool>), String> {
    tk.expect("Surfaces")?;
    let n = tk.usize()?;
    let mut out = Vec::with_capacity(n);
    let mut indirect = Vec::with_capacity(n);
    for i in 1..=n {
        let ty = tk.next()?;
        let mut ind = false;
        let surface = match ty {
            "1" => {
                let origin = v3f(tk)?;
                let normal = v3f(tk)?;
                let x_dir = v3f(tk)?;
                let y_dir = v3f(tk)?;
                ind = indirect_frame(normal, x_dir, y_dir);
                Surface::Plane {
                    origin,
                    normal,
                    x_dir,
                }
            }
            "2" => {
                let origin = v3f(tk)?;
                let axis = v3f(tk)?;
                let ref_dir = v3f(tk)?;
                let y_dir = v3f(tk)?;
                ind = indirect_frame(axis, ref_dir, y_dir);
                let radius = tk.f64()?;
                Surface::Cylinder {
                    origin,
                    axis,
                    ref_dir,
                    radius,
                }
            }
            "3" => {
                let origin = v3f(tk)?;
                let axis = v3f(tk)?;
                let ref_dir = v3f(tk)?;
                let y_dir = v3f(tk)?;
                ind = indirect_frame(axis, ref_dir, y_dir);
                let ref_radius = tk.f64()?;
                let semi_angle = tk.f64()?;
                Surface::Cone {
                    origin,
                    axis,
                    ref_dir,
                    semi_angle,
                    ref_radius,
                }
            }
            "4" => {
                let center = v3f(tk)?;
                let axis = v3f(tk)?;
                let ref_dir = v3f(tk)?;
                let y_dir = v3f(tk)?;
                ind = indirect_frame(axis, ref_dir, y_dir);
                let radius = tk.f64()?;
                Surface::Sphere {
                    center,
                    axis,
                    ref_dir,
                    radius,
                }
            }
            "5" => {
                let center = v3f(tk)?;
                let axis = v3f(tk)?;
                let ref_dir = v3f(tk)?;
                let y_dir = v3f(tk)?;
                ind = indirect_frame(axis, ref_dir, y_dir);
                let major_radius = tk.f64()?;
                let minor_radius = tk.f64()?;
                Surface::Torus {
                    center,
                    axis,
                    ref_dir,
                    major_radius,
                    minor_radius,
                }
            }
            "9" => {
                let urational = tk.i64()?;
                let vrational = tk.i64()?;
                let uperiodic = tk.i64()?;
                let vperiodic = tk.i64()?;
                if uperiodic != 0 || vperiodic != 0 {
                    return Err(format!(
                        "surface {i}: a periodic B-spline surface — not modeled yet"
                    ));
                }
                let u_degree = tk.usize()?;
                let v_degree = tk.usize()?;
                let nupoles = tk.usize()?;
                let nvpoles = tk.usize()?;
                let nuknots = tk.usize()?;
                let nvknots = tk.usize()?;
                let rational = urational == 1 || vrational == 1;
                let mut points = vec![Vec::new(); nupoles];
                let mut weights = if rational {
                    Some(vec![Vec::new(); nupoles])
                } else {
                    None
                };
                for u in 0..nupoles {
                    for _ in 0..nvpoles {
                        points[u].push(v3f(tk)?);
                        if let Some(w) = weights.as_mut() {
                            w[u].push(tk.f64()?);
                        }
                    }
                }
                let mut expand = |nknots: usize| -> Result<Vec<f64>, String> {
                    let mut knots = Vec::new();
                    for _ in 0..nknots {
                        let k = tk.f64()?;
                        let m = tk.usize()?;
                        for _ in 0..m {
                            knots.push(k);
                        }
                    }
                    Ok(knots)
                };
                let u_knots = expand(nuknots)?;
                let v_knots = expand(nvknots)?;
                Surface::Bspline {
                    u_degree,
                    v_degree,
                    u_knots,
                    v_knots,
                    points,
                    weights,
                }
            }
            other => {
                let kind = match other {
                    "6" => "a linear-extrusion surface",
                    "7" => "a surface of revolution",
                    "8" => "a Bezier surface",
                    "10" => "a rectangular-trimmed surface",
                    "11" => "an offset surface",
                    _ => "an unrecognized surface",
                };
                return Err(format!("surface {i}: {kind} — not modeled"));
            }
        };
        validate_surface(&surface).map_err(|e| format!("surface {i}: {e}"))?;
        out.push(surface);
        indirect.push(ind);
    }
    Ok((out, indirect))
}

fn parse_tshapes(tk: &mut Tokens<'_>) -> Result<(Vec<TShape>, SRef), String> {
    tk.expect("TShapes")?;
    let n = tk.usize()?;
    // Reference arithmetic: a reference number m points at file
    // record `n − m + 1` (children were added to the set first, the
    // root last; the writer walks the map in insertion order).
    let rec_of = |m: usize| -> Result<usize, String> {
        if m == 0 || m > n {
            return Err(format!("reference to TShape {m} of {n}"));
        }
        Ok(n - m + 1)
    };
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let ty = tk.next()?;
        let shape = match ty {
            "Ve" => {
                let tol = tk.f64()?;
                let point = v3f(tk)?;
                // Point representations until "<p1> 0".
                loop {
                    tk.f64()?; // p1
                    let val = tk.i64()?;
                    if val == 0 {
                        break;
                    }
                    match val {
                        1 => {
                            tk.usize()?;
                        }
                        2 => {
                            tk.usize()?;
                            tk.usize()?;
                        }
                        3 => {
                            tk.f64()?;
                            tk.usize()?;
                        }
                        other => {
                            return Err(format!("vertex representation kind {other} — not modeled"))
                        }
                    }
                    tk.usize()?; // the location
                }
                let _ = flags(tk)?;
                let _ = children(tk, &rec_of)?;
                TShape::Vertex { tol, point }
            }
            "Ed" => {
                tk.f64()?; // tolerance
                tk.i64()?; // same-parameter
                tk.i64()?; // same-range
                let degenerated = tk.i64()? == 1;
                let mut curve3d = None;
                let mut pcurves = Vec::new();
                loop {
                    let val = tk.i64()?;
                    match val {
                        0 => break,
                        1 => {
                            let c = tk.usize()?;
                            let l = tk.usize()?;
                            let first = tk.f64()?;
                            let last = tk.f64()?;
                            curve3d = Some((c, l, first, last));
                        }
                        2 | 3 => {
                            let closed = val == 3;
                            let pc = tk.usize()?;
                            if closed {
                                // The writer glues the second pcurve
                                // index and the regularity token into
                                // one word ("3CN") — split digits from
                                // letters the way OCCT's stream reader
                                // does.
                                let tok = tk.next()?;
                                let split =
                                    tok.find(|c: char| !c.is_ascii_digit()).unwrap_or(tok.len());
                                let (pc2, _reg) = tok.split_at(split);
                                if pc2.parse::<usize>().is_err() {
                                    return Err(format!(
                                        "a closed-surface pcurve pair '{tok}' without a \
                                         second pcurve index"
                                    ));
                                }
                            }
                            let s = tk.usize()?;
                            let l = tk.usize()?;
                            let first = tk.f64()?;
                            let last = tk.f64()?;
                            pcurves.push((pc, s, l, first, last));
                        }
                        4 => {
                            tk.next()?; // regularity
                            tk.usize()?;
                            tk.usize()?;
                            tk.usize()?;
                            tk.usize()?;
                        }
                        5 => {
                            tk.usize()?;
                            tk.usize()?;
                        }
                        6 | 7 => {
                            if val == 7 {
                                tk.usize()?;
                            }
                            tk.usize()?;
                            tk.usize()?;
                        }
                        other => {
                            return Err(format!("edge representation kind {other} — not modeled"))
                        }
                    }
                }
                let flags = flags(tk)?;
                let _ = flags;
                let vertices = children(tk, &rec_of)?;
                TShape::Edge {
                    degenerated,
                    curve3d,
                    pcurves,
                    vertices,
                }
            }
            "Wi" | "Fa" | "Sh" | "So" | "Co" | "CS" => {
                let (natural_restriction, surface, loc) = if ty == "Fa" {
                    let nr = tk.i64()? == 1;
                    tk.f64()?; // tolerance
                    let s = tk.usize()?;
                    let l = tk.usize()?;
                    (nr, s, l)
                } else {
                    (false, 0, 0)
                };
                let _ = flags(tk)?;
                let children = children(tk, &rec_of)?;
                if ty == "Fa" {
                    TShape::Face {
                        natural_restriction,
                        surface,
                        loc,
                        children,
                    }
                } else {
                    let kind: &'static str = match ty {
                        "Wi" => "Wi",
                        "Sh" => "Sh",
                        "So" => "So",
                        "Co" => "Co",
                        _ => "CS",
                    };
                    TShape::Container { kind, children }
                }
            }
            other => return Err(format!("TShape kind '{other}' — not modeled")),
        };
        out.push(shape);
    }
    // The root reference: "<sign><idx> <loc>".
    let tok = tk.next()?;
    let (reversed, m) = parse_ref_token(tok)?;
    let record = rec_of(m)?;
    let loc = tk.usize()?;
    Ok((
        out,
        SRef {
            reversed,
            record,
            loc,
        },
    ))
}

fn flags(tk: &mut Tokens<'_>) -> Result<String, String> {
    let f = tk.next()?;
    if f.len() == 7 && f.bytes().all(|b| b == b'0' || b == b'1') {
        Ok(f.to_string())
    } else {
        Err(format!("expected the 7-digit flags line, got '{f}'"))
    }
}

fn children(
    tk: &mut Tokens<'_>,
    rec_of: &dyn Fn(usize) -> Result<usize, String>,
) -> Result<Vec<SRef>, String> {
    let mut out = Vec::new();
    loop {
        let tok = tk.next()?;
        if tok == "*" {
            break;
        }
        let (reversed, m) = parse_ref_token(tok)?;
        let record = rec_of(m)?;
        let loc = tk.usize()?;
        out.push(SRef {
            reversed,
            record,
            loc,
        });
    }
    Ok(out)
}

fn parse_ref_token(tok: &str) -> Result<(bool, usize), String> {
    let mut chars = tok.chars();
    let sign = chars
        .next()
        .ok_or_else(|| format!("malformed reference '{tok}'"))?;
    let num: String = chars.collect();
    let idx: usize = num
        .parse()
        .map_err(|_| format!("malformed reference index in '{tok}'"))?;
    match sign {
        '+' => Ok((false, idx)),
        '-' => Ok((true, idx)),
        'i' | 'e' => Err(format!(
            "an INTERNAL/EXTERNAL shape use ('{tok}') — not modeled"
        )),
        _ => Err(format!("malformed reference sign in '{tok}'")),
    }
}

fn v3f(tk: &mut Tokens<'_>) -> Result<Vector3, String> {
    Ok(Vector3::new(tk.f64()?, tk.f64()?, tk.f64()?))
}

// ---------------------------------------------------------------------------
// the builder (reconciliation + assembly)
// ---------------------------------------------------------------------------

/// One world face: the placed surface, its record, the orientation
/// sign, and the wires as (edge record, edge transform, direction).
struct FaceWorld {
    surface: Surface,
    surface_record: usize,
    reversed: bool,
    wires: Vec<Vec<(usize, Trsf, bool)>>,
}

/// An edge endpoint: the file-vertex key, its world point and
/// tolerance (for the pin guard and the degenerate fallback).
#[derive(Clone)]
struct Endpoint {
    key: (usize, TrsfKey),
    file_point: Vector3,
    tol: f64,
}

struct Builder<'a> {
    reader: &'a Reader,
    vertices: Vec<TopoVertex>,
    edges: Vec<TopoEdge>,
    /// File vertex key → model vertex index.
    resolved: HashMap<(usize, TrsfKey), usize>,
    /// File edge key → model edge index.
    edge_of_key: HashMap<(usize, TrsfKey), usize>,
    report: OcctReadReport,
}

impl<'a> Builder<'a> {
    fn build_faces(mut self, face_refs: &[(SRef, Trsf)]) -> Result<OcctRead, String> {
        let mut faces_world = Vec::new();
        for (fr, shell_trsf) in face_refs {
            let face_trsf = shell_trsf.compose(&self.reader.location(fr.loc)?);
            let TShape::Face {
                natural_restriction,
                surface,
                loc,
                children,
            } = self.reader.tshape(fr.record, "a face")?
            else {
                return Err(format!("shell child {} is not a face", fr.record));
            };
            let world_trsf = face_trsf.compose(&self.reader.location(*loc)?);
            let surface_world = self
                .reader
                .place_surface(self.reader.surface(*surface, "a face")?, &world_trsf);
            if *natural_restriction {
                // A natural-restriction face is the surface's whole
                // parameter domain: an unbounded region for the
                // analytic five, but exactly the finite patch for a
                // B-spline surface (the boundary wires still bound
                // it in the file).
                let bounded = matches!(surface_world, Surface::Bspline { .. });
                if !bounded {
                    return Err(format!(
                        "face {}: an untrimmed periodic (natural-restriction) face on {} — \
                         not modeled",
                        fr.record,
                        surface_world.kind_name()
                    ));
                }
            }
            let mut wires = Vec::new();
            for wr in children {
                let wire_trsf = world_trsf.compose(&self.reader.location(wr.loc)?);
                let TShape::Container {
                    kind: "Wi",
                    children: edge_refs,
                } = self.reader.tshape(wr.record, "a wire")?
                else {
                    return Err(format!("face child {} is not a wire", wr.record));
                };
                let mut uses = Vec::new();
                for er in edge_refs {
                    let edge_trsf = wire_trsf.compose(&self.reader.location(er.loc)?);
                    // Effective direction: a '-' edge ref in a
                    // forward wire traverses the stored start→end (the
                    // convention every BRepTools-written file in the
                    // corpus follows); a reversed wire use flips all
                    // of its edges.
                    let forward = wr.reversed != er.reversed;
                    uses.push((er.record, edge_trsf, forward));
                }
                wires.push(self.order_wire(fr.record, uses)?);
            }
            // An indirect Ax3 surface record negates the natural
            // normal our right-handed model computes, composing with
            // the face-use orientation.
            let indirect = self
                .reader
                .surface_indirect
                .get(*surface - 1)
                .copied()
                .unwrap_or(false);
            faces_world.push(FaceWorld {
                surface: surface_world,
                surface_record: *surface,
                reversed: fr.reversed != indirect,
                wires,
            });
        }
        if faces_world.is_empty() {
            return Err("the shell has no faces".to_string());
        }

        // The deterministic edge order: the face walk order, deduped;
        // non-degenerate edges resolve vertices first, degenerate
        // ones (sphere poles) take whatever is left.
        let mut order: Vec<(usize, Trsf)> = Vec::new();
        let mut seen: HashMap<(usize, TrsfKey), ()> = HashMap::new();
        for f in &faces_world {
            for w in &f.wires {
                for (record, trsf, _) in w {
                    let key = (*record, TrsfKey::of(trsf));
                    if seen.insert(key, ()).is_none() {
                        order.push((*record, *trsf));
                    }
                }
            }
        }
        // The resolution order decides whose evaluation defines a
        // shared vertex, and it prefers the curves whose verbatim form
        // measure keeps recognizing: closed conics first (their t = 0
        // evaluation is the canonical, bit-reproducible point — the
        // seam pattern), then open arcs and B-splines (both endpoints
        // self-resolve), then lines — a line between two resolved
        // points is exact, so lines always pin rather than define.
        let mut tiers = Vec::with_capacity(order.len());
        for (record, _trsf) in &order {
            let (deg, tier) = match self.reader.tshape(*record, "an edge")? {
                TShape::Edge {
                    degenerated,
                    curve3d,
                    vertices,
                    ..
                } => {
                    let closed = match (vertices.first(), vertices.get(1)) {
                        (Some(a), Some(b)) => {
                            a.record == b.record && a.loc == b.loc && a.reversed != b.reversed
                        }
                        _ => false,
                    };
                    let tier = if *degenerated {
                        3
                    } else if closed {
                        0
                    } else {
                        match curve3d
                            .as_ref()
                            .map(|&(c, _, _, _)| self.reader.curve(c, "an edge"))
                        {
                            Some(Ok(Curve::Line { .. })) => 2,
                            _ => 1,
                        }
                    };
                    (*degenerated, tier)
                }
                _ => return Err(format!("edge {record} is not an edge")),
            };
            tiers.push((deg, tier));
        }
        for tier in 0..3 {
            for (i, (record, trsf)) in order.iter().enumerate() {
                if !tiers[i].0 && tiers[i].1 == tier {
                    self.build_edge(*record, *trsf)?;
                }
            }
        }
        for (i, (record, trsf)) in order.iter().enumerate() {
            if tiers[i].0 {
                self.build_degenerate_edge(*record, *trsf)?;
            }
        }

        // Assemble the faces (wires reference model edges; UV bounds
        // come from the pcurves).
        let mut faces = Vec::new();
        for f in &faces_world {
            let mut wires = Vec::new();
            let mut bounds = [
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ];
            for w in &f.wires {
                let mut wire_edges = Vec::new();
                for (record, trsf, forward) in w {
                    let key = (*record, TrsfKey::of(trsf));
                    let ei = *self.edge_of_key.get(&key).ok_or_else(|| {
                        format!("edge {record} was not built (internal reader error)")
                    })?;
                    wire_edges.push(WireEdge {
                        edge: ei,
                        forward: *forward,
                    });
                    self.accumulate_uv(f, *record, &mut bounds)?;
                }
                wires.push(TopoWire { edges: wire_edges });
            }
            faces.push(TopoFace {
                surface: f.surface.clone(),
                uv_bounds: bounds,
                wires,
                reversed: f.reversed,
            });
        }

        let brep = ExactBRep {
            vertices: std::mem::take(&mut self.vertices),
            edges: std::mem::take(&mut self.edges),
            faces,
        };
        brep.validate()
            .map_err(|e| format!("the read B-rep does not validate: {e}"))?;
        let mut report = self.report.clone();
        report.faces = brep.faces.len();
        report.edges = brep.edges.len();
        report.vertices = brep.vertices.len();
        Ok(OcctRead { brep, report })
    }

    /// Fold one edge's pcurves (on this face's surface) into the UV
    /// bounds.
    fn accumulate_uv(
        &self,
        face: &FaceWorld,
        record: usize,
        bounds: &mut [f64; 4],
    ) -> Result<(), String> {
        let TShape::Edge { pcurves, .. } = self.reader.tshape(record, "an edge")? else {
            return Err(format!("edge {record} is not an edge"));
        };
        let mut found = false;
        for &(pc, s, _l, first, last) in pcurves {
            if s != face.surface_record {
                continue;
            }
            let c2 = self.reader.curve2d(pc, "a face trim")?;
            c2.accumulate_bounds(first, last, bounds);
            found = true;
        }
        if !found {
            return Err(format!(
                "edge {record}: no pcurve on surface {} — the UV region cannot be bounded",
                face.surface_record
            ));
        }
        Ok(())
    }

    /// Order a wire's edge uses into the traversal cycle. OCCT
    /// stores a wire's children as a *set* (the add order, often but
    /// not always the cycle), and the loop is the vertex-connectivity
    /// chain: each use's effective direction is already resolved;
    /// closed edges (start == end) pass through their single vertex.
    /// Ties (a vertex with several outgoing unconsumed uses — a
    /// non-manifold junction) resolve deterministically by stored
    /// order; [`ExactBRep::validate`] is the final arbiter of closure.
    fn order_wire(
        &self,
        face_record: usize,
        uses: Vec<(usize, Trsf, bool)>,
    ) -> Result<Vec<(usize, Trsf, bool)>, String> {
        if uses.is_empty() {
            return Err(format!("face {face_record}: an empty wire"));
        }
        // Per use: (start key, end key) of the EFFECTIVE traversal.
        let mut endpoints = Vec::with_capacity(uses.len());
        for &(record, trsf, forward) in &uses {
            let (start, end) = self.edge_endpoints(record, &trsf)?;
            let end = end.ok_or_else(|| format!("edge {record}: no end vertex"))?;
            let (s, e) = if forward {
                (start.key, end.key)
            } else {
                (end.key, start.key)
            };
            endpoints.push((s, e));
        }
        let mut remaining: Vec<usize> = (0..uses.len()).collect();
        let mut order = Vec::with_capacity(uses.len());
        let first = remaining.remove(0);
        order.push(first);
        let mut current = endpoints[first].1;
        let start_vertex = endpoints[first].0;
        while !remaining.is_empty() {
            let next = remaining
                .iter()
                .position(|&i| endpoints[i].0 == current)
                .ok_or_else(|| {
                    format!(
                        "face {face_record}: the wire breaks — no edge continues at vertex \
                         record {}",
                        current.0
                    )
                })?;
            let next = remaining.remove(next);
            order.push(next);
            current = endpoints[next].1;
        }
        if current != start_vertex {
            return Err(format!(
                "face {face_record}: the wire does not close (ends at vertex record {}, began \
                 at {})",
                current.0, start_vertex.0
            ));
        }
        Ok(order.into_iter().map(|i| uses[i]).collect())
    }

    /// The edge's endpoints from its vertex children (start `+`, end
    /// `-`; a closed edge repeats the same vertex both ways).
    fn edge_endpoints(
        &self,
        record: usize,
        trsf: &Trsf,
    ) -> Result<(Endpoint, Option<Endpoint>), String> {
        let TShape::Edge { vertices, .. } = self.reader.tshape(record, "an edge")? else {
            return Err(format!("edge {record} is not an edge"));
        };
        if vertices.len() != 2 {
            return Err(format!(
                "edge {record} has {} vertex children (expected 2)",
                vertices.len()
            ));
        }
        let mut start = None;
        let mut end = None;
        for ch in vertices {
            let v_trsf = trsf.compose(&self.reader.location(ch.loc)?);
            let (file_point, tol) = match self.reader.tshape(ch.record, "a vertex")? {
                TShape::Vertex { point, tol } => (v_trsf.point(*point), *tol),
                _ => return Err(format!("vertex {} is not a vertex", ch.record)),
            };
            let ep = Endpoint {
                key: (ch.record, TrsfKey::of(&v_trsf)),
                file_point,
                tol,
            };
            if ch.reversed {
                end = Some(ep);
            } else {
                start = Some(ep);
            }
        }
        Ok((
            start.ok_or_else(|| format!("edge {record}: no start vertex"))?,
            end,
        ))
    }

    /// Resolve a file vertex, defining it from `eval` if this is the
    /// first edge to touch it.
    fn resolve(&mut self, key: (usize, TrsfKey), eval: Vector3) -> usize {
        if let Some(&i) = self.resolved.get(&key) {
            return i;
        }
        self.vertices.push(TopoVertex { point: eval });
        let i = self.vertices.len() - 1;
        self.resolved.insert(key, i);
        i
    }

    fn build_edge(&mut self, record: usize, trsf: Trsf) -> Result<(), String> {
        let key = (record, TrsfKey::of(&trsf));
        if self.edge_of_key.contains_key(&key) {
            return Ok(());
        }
        let TShape::Edge { curve3d, .. } = self.reader.tshape(record, "an edge")? else {
            return Err(format!("edge {record} is not an edge"));
        };
        let (start, end) = self.edge_endpoints(record, &trsf)?;
        let end = end.ok_or_else(|| format!("edge {record}: no end vertex"))?;
        // A closed edge repeats the same vertex TShape both ways.
        let closed = start.key == end.key;
        let (crec, cloc, first, last) = curve3d
            .as_ref()
            .ok_or_else(|| format!("edge {record}: no 3D curve (a triangulated edge?)"))?;
        let curve_trsf = trsf.compose(&self.reader.location(*cloc)?);
        let world_curve = self
            .reader
            .place_curve(self.reader.curve(*crec, "an edge")?, &curve_trsf);
        if *first >= *last {
            return Err(format!(
                "edge {record}: parameter range {first}..{last} is empty"
            ));
        }

        // An indirect conic record parametrizes `t → −t` against our
        // right-handed reading: reparameterize (mirroring the range
        // and swapping the endpoint roles) so the model curve runs the
        // file's geometry with our convention.
        let indirect = self
            .reader
            .curve_indirect
            .get(*crec - 1)
            .copied()
            .unwrap_or(false);
        let (first, last, start, end, closed) = if indirect {
            let (s, e) = (end, start);
            (-*last, -*first, e, s, closed)
        } else {
            (*first, *last, start, end, closed)
        };

        let start_eval = world_curve.point(first);
        let end_eval = world_curve.point(last);
        let start_resolved = self.resolved.contains_key(&start.key);
        let model_start = self.resolve(start.key, start_eval);
        let (model_end, end_resolved) = if closed {
            (model_start, start_resolved)
        } else {
            let r = self.resolved.contains_key(&end.key);
            (self.resolve(end.key, end_eval), r)
        };

        let model_edge = if !start_resolved && !end_resolved {
            // This edge defines its own endpoints: keep the file curve
            // verbatim (the vertices are its evaluations, by
            // construction).
            TopoEdge {
                curve: world_curve,
                range: (first, last),
                start: model_start,
                end: model_end,
            }
        } else {
            self.pin_edge(
                record,
                &world_curve,
                (first, last),
                closed,
                model_start,
                model_end,
                &start,
                &end,
            )?
        };

        self.edges.push(model_edge);
        self.edge_of_key.insert(key, self.edges.len() - 1);
        Ok(())
    }

    /// Degenerate edges: zero-radius circles centered on the resolved
    /// vertex (the K1 sphere-pole representation). A lone degenerate
    /// edge whose vertex no real edge defined falls back to the
    /// file's own vertex point.
    fn build_degenerate_edge(&mut self, record: usize, trsf: Trsf) -> Result<(), String> {
        let key = (record, TrsfKey::of(&trsf));
        if self.edge_of_key.contains_key(&key) {
            return Ok(());
        }
        let (start, end) = self.edge_endpoints(record, &trsf)?;
        let _ = end;
        let p = match self.resolved.get(&start.key) {
            Some(&i) => self.vertices[i].point,
            None => start.file_point,
        };
        let curve = Curve::Circle {
            center: p,
            axis: Vector3::Z,
            ref_dir: Vector3::X,
            radius: 0.0,
        };
        let vi = self.resolve(start.key, p);
        self.edges.push(TopoEdge {
            curve,
            range: (0.0, std::f64::consts::TAU),
            start: vi,
            end: vi,
        });
        self.edge_of_key.insert(key, self.edges.len() - 1);
        self.report.degenerate_edges += 1;
        Ok(())
    }

    /// Pin an edge whose endpoint vertices were resolved by earlier
    /// edges: rebuild the curve to pass through them bit-exactly.
    #[allow(clippy::too_many_arguments)]
    fn pin_edge(
        &mut self,
        record: usize,
        curve: &Curve,
        range: (f64, f64),
        closed: bool,
        model_start: usize,
        model_end: usize,
        start: &Endpoint,
        end: &Endpoint,
    ) -> Result<TopoEdge, String> {
        let a = self.vertices[model_start].point;
        let b = self.vertices[model_end].point;
        // The deviation guard: the file's own endpoints vs the model
        // vertices, against the file's vertex tolerances.
        let p0 = curve.point(range.0);
        let p1 = curve.point(range.1);
        let d0 = (p0 - a).length();
        let d1 = if closed { 0.0 } else { (p1 - b).length() };
        let tol_cap = start.tol.max(end.tol);
        let dev = d0.max(d1);
        if dev > tol_cap {
            return Err(format!(
                "edge {record}: the endpoints are {dev:.3e} from the shared vertices, beyond \
                 the file's own vertex tolerance {tol_cap:.3e} — refusing rather than snapping"
            ));
        }
        if dev > self.report.max_pin_deviation {
            self.report.max_pin_deviation = dev;
        }
        let (curve, range) = match curve {
            Curve::Line { .. } => {
                // The seam pattern: de Boor returns the endpoints.
                self.report.pinned_lines += 1;
                (
                    Curve::Bspline {
                        degree: 1,
                        knots: vec![0.0, 0.0, 1.0, 1.0],
                        points: vec![a, b],
                        weights: None,
                    },
                    (0.0, 1.0),
                )
            }
            Curve::Circle { .. } | Curve::Ellipse { .. } => {
                if closed {
                    if matches!(curve, Curve::Ellipse { .. }) {
                        return Err(format!(
                            "edge {record}: a closed ellipse that must pin — the NURBS \
                             ellipse re-pin is not modeled yet"
                        ));
                    }
                    self.report.repinned_circles += 1;
                    (nurbs_circle_pinned(curve, a), (0.0, 1.0))
                } else {
                    self.report.pinned_arcs += 1;
                    let pinned = nurbs_arc_pinned(curve, range, a, b)
                        .ok_or_else(|| format!("edge {record}: the arc pin failed"))?;
                    (pinned, (0.0, 1.0))
                }
            }
            Curve::Bspline {
                degree,
                knots,
                points,
                weights,
            } => {
                let lo = knots[*degree];
                let hi = knots[points.len()];
                if range.0 != lo || range.1 != hi {
                    return Err(format!(
                        "edge {record}: a B-spline edge with an interior range {}..{} (the \
                         domain is {}..{}) that must pin — knot insertion is not modeled yet",
                        range.0, range.1, lo, hi
                    ));
                }
                self.report.pinned_splines += 1;
                let mut pts = points.clone();
                let n = pts.len();
                pts[0] = a;
                pts[n - 1] = b;
                (
                    Curve::Bspline {
                        degree: *degree,
                        knots: knots.clone(),
                        points: pts,
                        weights: weights.clone(),
                    },
                    (lo, hi),
                )
            }
        };
        Ok(TopoEdge {
            curve,
            range,
            start: model_start,
            end: model_end,
        })
    }
}

/// The classical 9-pole NURBS circle with `P0 = P8 = a` (the pinned
/// vertex), the rest of the control polygon from the file circle.
fn nurbs_circle_pinned(curve: &Curve, a: Vector3) -> Curve {
    let Curve::Circle {
        center,
        axis,
        ref_dir,
        radius,
    } = curve
    else {
        unreachable!("caller checked the curve kind")
    };
    let y = axis.cross(ref_dir);
    let c = *center;
    let x = *ref_dir;
    let r = *radius;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let pts = vec![
        a,
        c + (x + y) * r,
        c + y * r,
        c + (y - x) * r,
        c - x * r,
        c - (x + y) * r,
        c - y * r,
        c + (x - y) * r,
        a,
    ];
    Curve::Bspline {
        degree: 2,
        knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        points: pts,
        weights: Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
    }
}

/// A quadrant-split rational quadratic B-spline arc from `a` to `b`,
/// following the file conic's frame, sweep, and direction.
fn nurbs_arc_pinned(curve: &Curve, range: (f64, f64), a: Vector3, b: Vector3) -> Option<Curve> {
    // The conic frame: center, x̂, ŷ (unit), semi-axes (r1, r2).
    let (center, xh, yh, r1, r2) = match curve {
        Curve::Circle {
            center,
            axis,
            ref_dir,
            radius,
        } => (*center, *ref_dir, axis.cross(ref_dir), *radius, *radius),
        Curve::Ellipse {
            center,
            axis,
            major_dir,
            major_radius,
            minor_radius,
        } => (
            *center,
            *major_dir,
            axis.cross(major_dir),
            *major_radius,
            *minor_radius,
        ),
        _ => return None,
    };
    let (t0, t1) = range;
    if !t1.is_finite() || !t0.is_finite() || t1 <= t0 {
        return None;
    }
    // Quadrant breakpoints strictly inside (t0, t1), plus the ends.
    let frac_pi_2 = std::f64::consts::FRAC_PI_2;
    let mut breaks = vec![t0];
    let mut k = (t0 / frac_pi_2).ceil() as i64;
    loop {
        let t = k as f64 * frac_pi_2;
        if t >= t1 {
            break;
        }
        if t > t0 {
            breaks.push(t);
        }
        k += 1;
    }
    breaks.push(t1);
    let conic_pt = |t: f64| -> Vector3 {
        let (s, co) = t.sin_cos();
        center + xh * (r1 * co) + yh * (r2 * s)
    };
    // The tangent intersection between parameters u < v: the affine
    // image of the circle's — at the mid-angle, scaled by
    // 1/cos(half-sweep).
    let tangent_meet = |u: f64, v: f64| -> Vector3 {
        let mid = (u + v) / 2.0;
        let half = (v - u) / 2.0;
        let scale = half.cos();
        if scale.abs() < 1e-15 {
            return conic_pt(mid);
        }
        let inv = 1.0 / scale;
        let (sm, cm) = mid.sin_cos();
        center + xh * (r1 * cm * inv) + yh * (r2 * sm * inv)
    };
    // Poles: `[a, meet₁, q₁, meet₂, q₂, …, b]` — the interior
    // breakpoint poles are shared between adjacent spans (the previous
    // span's closing pole), so one opening pole plus two per span.
    let mut points = vec![a];
    let mut weights = vec![1.0];
    let n = breaks.len();
    for i in 1..n {
        let bl = breaks[i - 1];
        let br = breaks[i];
        points.push(tangent_meet(bl, br));
        weights.push(((br - bl) / 2.0).cos());
        let closing = if i == n - 1 { b } else { conic_pt(br) };
        points.push(closing);
        weights.push(1.0);
    }
    // Clamped knots with interior breakpoints doubled (degree 2):
    // [0×3, b₁, b₁, …, 1×3] — `points + degree + 1` of them.
    let mut knots = vec![0.0, 0.0, 0.0];
    for i in 1..n - 1 {
        let u = i as f64 / (n - 1) as f64;
        knots.push(u);
        knots.push(u);
    }
    knots.push(1.0);
    knots.push(1.0);
    knots.push(1.0);
    Some(Curve::Bspline {
        degree: 2,
        knots,
        points,
        weights: Some(weights),
    })
}
