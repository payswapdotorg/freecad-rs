//! Test fixtures: the exact-B-rep STEP generator (a box with a
//! cylindrical through hole) and a ring sketch for the Pad round-trip.
//!
//! The entity graph is hand-authored per AP203's topology subset: 6
//! planar `ADVANCED_FACE`s (top and bottom carry the hole circle as a
//! second `FACE_OUTER_BOUND`) + 1 cylindrical `ADVANCED_FACE` (the hole
//! wall), assembled into a `CLOSED_SHELL` and one
//! `MANIFOLD_SOLID_BREP`. All coordinates derive from the analytic
//! parameters, so the fixture's exact volume is `L·W·H − π·r²·h` by
//! construction.

use std::fmt::Write as _;

struct W {
    out: String,
    next: u64,
}

impl W {
    fn new() -> Self {
        W {
            out: String::new(),
            next: 1,
        }
    }
    fn raw(&mut self, body: String) -> u64 {
        let id = self.next;
        self.next += 1;
        let _ = writeln!(self.out, "#{id}={body};");
        id
    }
    fn point(&mut self, x: f64, y: f64, z: f64) -> u64 {
        self.raw(format!("CARTESIAN_POINT('',({x:.10},{y:.10},{z:.10}))"))
    }
    fn direction(&mut self, x: f64, y: f64, z: f64) -> u64 {
        self.raw(format!("DIRECTION('',({x:.10},{y:.10},{z:.10}))"))
    }
    fn vertex(&mut self, p: u64) -> u64 {
        self.raw(format!("VERTEX_POINT('',#{p})"))
    }
    fn axis2(&mut self, o: (f64, f64, f64), z: (f64, f64, f64), x: (f64, f64, f64)) -> u64 {
        let p = self.point(o.0, o.1, o.2);
        let dz = self.direction(z.0, z.1, z.2);
        let dx = self.direction(x.0, x.1, x.2);
        self.raw(format!("AXIS2_PLACEMENT_3D('',#{p},#{dz},#{dx})"))
    }
    /// A LINE edge from `a` to `b` (vertex ids) with known coordinates.
    fn line_edge(&mut self, a: u64, b: u64, sa: (f64, f64, f64), sb: (f64, f64, f64)) -> u64 {
        let (dx, dy, dz) = (sb.0 - sa.0, sb.1 - sa.1, sb.2 - sa.2);
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        let p = self.point(sa.0, sa.1, sa.2);
        let d = self.direction(dx / len, dy / len, dz / len);
        let vec = self.raw(format!("VECTOR('',#{d},{len:.10})"));
        let line = self.raw(format!("LINE('',#{p},#{vec})"));
        self.raw(format!("EDGE_CURVE('',#{a},#{b},#{line},.T.)"))
    }
    fn oriented(&mut self, e: u64, f: bool) -> u64 {
        let flag = if f { ".T." } else { ".F." };
        self.raw(format!("ORIENTED_EDGE('',*,*,#{e},{flag})"))
    }
    fn edge_loop(&mut self, es: &[u64]) -> u64 {
        let list = es
            .iter()
            .map(|e| format!("#{e}"))
            .collect::<Vec<_>>()
            .join(",");
        self.raw(format!("EDGE_LOOP('',({list}))"))
    }
    fn bound(&mut self, lp: u64, sense: bool) -> u64 {
        let flag = if sense { ".T." } else { ".F." };
        self.raw(format!("FACE_OUTER_BOUND('',#{lp},{flag})"))
    }
    /// A rectangle of LINE edges over corner coords, CCW as given.
    fn rect_loop(&mut self, vids: [u64; 4], coords: [(f64, f64, f64); 4]) -> u64 {
        let pairs = [
            (vids[0], vids[1], coords[0], coords[1]),
            (vids[1], vids[2], coords[1], coords[2]),
            (vids[2], vids[3], coords[2], coords[3]),
            (vids[3], vids[0], coords[3], coords[0]),
        ];
        let mut edges = Vec::new();
        for (a, b, sa, sb) in pairs {
            let e = self.line_edge(a, b, sa, sb);
            edges.push(self.oriented(e, true));
        }
        self.edge_loop(&edges)
    }
    /// A full circle as one loop of two half-arc EDGE_CURVEs over the
    /// ±x vertices (no geometry ambiguity: the CIRCLE entity carries
    /// the supporting circle).
    fn circle_loop(&mut self, v_plus_x: u64, v_minus_x: u64, circle: u64) -> u64 {
        let arc1 = self.raw(format!(
            "EDGE_CURVE('',#{v_plus_x},#{v_minus_x},#{circle},.T.)"
        ));
        let arc2 = self.raw(format!(
            "EDGE_CURVE('',#{v_minus_x},#{v_plus_x},#{circle},.T.)"
        ));
        let o1 = self.oriented(arc1, true);
        let o2 = self.oriented(arc2, true);
        self.edge_loop(&[o1, o2])
    }
}

/// Box 30×20×10 mm; hole radius 4 mm at (15, 10), through +Z.
pub fn box_with_hole_step() -> String {
    let (l, w, h) = (30.0_f64, 20.0, 10.0);
    let (cx, cy, r) = (15.0_f64, 10.0, 4.0);
    let mut g = W::new();

    // Header.
    g.out.push_str("ISO-10303-21;\nHEADER;\n");
    g.out.push_str(
        "FILE_DESCRIPTION(('freecad-rs test fixture: box with cylindrical through hole'),'2;1');\n",
    );
    g.out.push_str("FILE_NAME('box-with-hole.step','2026-10-21T00:00:00',('freecad-rs'),('payswapdotorg'),'fixture-generator','','');\n");
    g.out.push_str(
        "FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n",
    );

    // Shared context.
    let app_context =
        g.raw("APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into());
    g.raw(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2010,#{app_context})"));
    let product_def_context = g.raw(format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{app_context},'design')"
    ));
    let product_context = g.raw(format!("PRODUCT_CONTEXT('',#{app_context},'mechanical')"));
    let length_unit = g.raw("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let angle_unit = g.raw("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let solid_angle_unit = g.raw("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let uncertainty = g.raw(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{length_unit},'distance_accuracy_value','confusion accuracy')"
    ));
    let geom_context = g.raw(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length_unit},#{angle_unit},#{solid_angle_unit}))REPRESENTATION_CONTEXT('Context #1','3D Context'))"
    ));

    // Box corner vertices.
    let corner = [
        (0.0, 0.0, 0.0),
        (l, 0.0, 0.0),
        (l, w, 0.0),
        (0.0, w, 0.0),
        (0.0, 0.0, h),
        (l, 0.0, h),
        (l, w, h),
        (0.0, w, h),
    ];
    let mut vp = Vec::with_capacity(corner.len());
    for c in corner {
        let p = g.point(c.0, c.1, c.2);
        vp.push(g.vertex(p));
    }

    // Hole circle quarter vertices at z=0 and z=h (indices: +x, +y, −x, −y).
    let quarter = [(cx + r, cy), (cx, cy + r), (cx - r, cy), (cx, cy - r)];
    let mut qv = [[0u64; 4]; 2];
    for (zi, z) in [0.0, h].iter().enumerate() {
        for (qi, (x, y)) in quarter.iter().enumerate() {
            let p = g.point(*x, *y, *z);
            qv[zi][qi] = g.vertex(p);
        }
    }
    let circle_bottom = {
        let pl = g.axis2((cx, cy, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0));
        g.raw(format!("CIRCLE('',#{pl},{r:.10})"))
    };
    let circle_top = {
        let pl = g.axis2((cx, cy, h), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0));
        g.raw(format!("CIRCLE('',#{pl},{r:.10})"))
    };

    let mut face_ids = Vec::new();

    // Bottom (z=0, outer CCW seen from below) + hole inner bound.
    {
        let outer = g.rect_loop(
            [vp[3], vp[2], vp[1], vp[0]],
            [corner[3], corner[2], corner[1], corner[0]],
        );
        let outer_bound = g.bound(outer, true);
        let hole = g.circle_loop(qv[0][0], qv[0][2], circle_bottom);
        let inner_bound = g.bound(hole, false);
        let pl = g.axis2((0.0, 0.0, 0.0), (0.0, 0.0, -1.0), (1.0, 0.0, 0.0));
        let plane = g.raw(format!("PLANE('',#{pl})"));
        face_ids.push(g.raw(format!(
            "ADVANCED_FACE('',(#{outer_bound},#{inner_bound}),#{plane},.T.)"
        )));
    }
    // Top (z=h) + hole inner bound.
    {
        let outer = g.rect_loop(
            [vp[0], vp[1], vp[2], vp[3]],
            [corner[0], corner[1], corner[2], corner[3]],
        );
        let outer_bound = g.bound(outer, true);
        let hole = g.circle_loop(qv[1][0], qv[1][2], circle_top);
        let inner_bound = g.bound(hole, false);
        let pl = g.axis2((0.0, 0.0, h), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0));
        let plane = g.raw(format!("PLANE('',#{pl})"));
        face_ids.push(g.raw(format!(
            "ADVANCED_FACE('',(#{outer_bound},#{inner_bound}),#{plane},.T.)"
        )));
    }
    // Four sides (CCW seen from outside).
    {
        // (vertex ids, corner coordinates) per side.
        type Side = ([u64; 4], [(f64, f64, f64); 4]);
        let sides: [Side; 4] = [
            (
                [vp[0], vp[1], vp[5], vp[4]],
                [corner[0], corner[1], corner[5], corner[4]],
            ),
            (
                [vp[1], vp[2], vp[6], vp[5]],
                [corner[1], corner[2], corner[6], corner[5]],
            ),
            (
                [vp[2], vp[3], vp[7], vp[6]],
                [corner[2], corner[3], corner[7], corner[6]],
            ),
            (
                [vp[3], vp[0], vp[4], vp[7]],
                [corner[3], corner[0], corner[4], corner[7]],
            ),
        ];
        for (vids, coords) in sides {
            let outer = g.rect_loop(vids, coords);
            let b = g.bound(outer, true);
            // Face plane: normal = u × +Z where u = the side's direction
            // (normalized by the width, which every side runs along).
            let (a, c) = (coords[0], coords[1]);
            let (dx, dy, dz) = (c.0 - a.0, c.1 - a.1, c.2 - a.2);
            let (ux, uy, uz) = (dx / w, dy / w, dz / w);
            let n = (uy * h - 0.0, 0.0 - ux * h, 0.0); // u × (0,0,h)
            let pl = g.axis2(a, n, (ux, uy, uz));
            let plane = g.raw(format!("PLANE('',#{pl})"));
            face_ids.push(g.raw(format!("ADVANCED_FACE('',(#{b}),#{plane},.T.)")));
        }
    }
    // The cylindrical hole wall: one loop = seam up, top half-arc,
    // seam down, bottom half-arc.
    {
        let seam_p0 = g.point(cx + r, cy, 0.0);
        let seam_d = g.direction(0.0, 0.0, 1.0);
        let seam_vec = g.raw(format!("VECTOR('',#{seam_d},{h:.10})"));
        let seam_line = g.raw(format!("LINE('',#{seam_p0},#{seam_vec})"));
        let seam_up = g.raw(format!(
            "EDGE_CURVE('',#{},#{},#{seam_line},.T.)",
            qv[0][0], qv[1][0]
        ));
        let seam_down = g.raw(format!(
            "EDGE_CURVE('',#{},#{},#{seam_line},.T.)",
            qv[1][2], qv[0][2]
        ));
        let top_arc = g.raw(format!(
            "EDGE_CURVE('',#{},#{},#{circle_top},.T.)",
            qv[1][0], qv[1][2]
        ));
        let bottom_arc = g.raw(format!(
            "EDGE_CURVE('',#{},#{},#{circle_bottom},.T.)",
            qv[0][2], qv[0][0]
        ));
        let oe = [
            g.oriented(seam_up, true),
            g.oriented(top_arc, true),
            g.oriented(seam_down, true),
            g.oriented(bottom_arc, true),
        ];
        let lp = g.edge_loop(&oe);
        let b = g.bound(lp, false);
        let pl = g.axis2((cx, cy, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0));
        let surface = g.raw(format!("CYLINDRICAL_SURFACE('',#{pl},{r:.10})"));
        face_ids.push(g.raw(format!("ADVANCED_FACE('',(#{b}),#{surface},.T.)")));
    }

    // Shell, solid, product.
    let faces_list = face_ids
        .iter()
        .map(|f| format!("#{f}"))
        .collect::<Vec<_>>()
        .join(",");
    let shell = g.raw(format!("CLOSED_SHELL('',({faces_list}))"));
    let brep = g.raw(format!("MANIFOLD_SOLID_BREP('Body',#{shell})"));
    let product = g.raw(format!(
        "PRODUCT('box-with-hole','box-with-hole','',#{product_context})"
    ));
    let formation = g.raw(format!(
        "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#{product},.NOT_KNOWN.)"
    ));
    let product_definition = g.raw(format!(
        "PRODUCT_DEFINITION('design','',#{formation},#{product_def_context})"
    ));
    let pd_shape = g.raw(format!(
        "PRODUCT_DEFINITION_SHAPE('','',#{product_definition})"
    ));
    let origin = g.point(0.0, 0.0, 0.0);
    let dz = g.direction(0.0, 0.0, 1.0);
    let dx = g.direction(1.0, 0.0, 0.0);
    let axis = g.raw(format!("AXIS2_PLACEMENT_3D('',#{origin},#{dz},#{dx})"));
    let shape_rep = g.raw(format!(
        "SHAPE_REPRESENTATION('',(#{axis},#{brep}),#{geom_context})"
    ));
    g.raw(format!(
        "SHAPE_DEFINITION_REPRESENTATION(#{pd_shape},#{shape_rep})"
    ));

    g.out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    g.out
}

/// A ring (washer) sketch XML — two concentric circles — in the repo's
/// `Sketcher` fragment format (`sketch-import`'s input).
pub fn ring_sketch_xml() -> String {
    "<Sketch>\n  \
<Geometry type=\"2\" centerx=\"0.0\" centery=\"0.0\" radius=\"10.0\"/>\n  \
<Geometry type=\"2\" centerx=\"0.0\" centery=\"0.0\" radius=\"4.0\"/>\n</Sketch>\n"
        .to_string()
}
