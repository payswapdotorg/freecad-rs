//! STEP export — a deterministic faceted-B-rep AP203 writer.
//!
//! Each mesh-bearing object becomes a `MANIFOLD_SOLID_BREP` of
//! `CLOSED_SHELL` of triangle `ADVANCED_FACE`s (a `POLY_LOOP` per
//! triangle on its own `PLANE`): legal STEP that honestly carries a
//! *tessellation*, not exact B-rep. The product name carries the
//! faceted marker (`freecad-rs faceted export`), asserted by tests —
//! the nature is stated in the file itself, per ADR-0002 §3.

use std::fmt::Write as _;

use fc_base::Vector3;
use fc_part::Mesh;

/// The faceted marker carried in every exported product name.
pub const FACETED_MARKER: &str = "freecad-rs faceted export";

struct Writer {
    out: String,
    next_id: u64,
}

impl Writer {
    fn new() -> Self {
        Writer {
            out: String::new(),
            next_id: 1,
        }
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn raw(&mut self, id: u64, body: &str) {
        let _ = writeln!(self.out, "#{id}={body};");
    }

    fn point(&mut self, p: &Vector3) -> u64 {
        let id = self.id();
        self.raw(
            id,
            &format!(
                "CARTESIAN_POINT('',({:.10E},{:.10E},{:.10E}))",
                p.x, p.y, p.z
            ),
        );
        id
    }

    fn direction(&mut self, d: &Vector3) -> u64 {
        let id = self.id();
        let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
        self.raw(
            id,
            &format!(
                "DIRECTION('',({:.10E},{:.10E},{:.10E}))",
                d.x / len,
                d.y / len,
                d.z / len
            ),
        );
        id
    }

    fn axis2(&mut self, origin: &Vector3, axis: &Vector3, ref_dir: &Vector3) -> u64 {
        let p = self.point(origin);
        let a = self.direction(axis);
        let r = self.direction(ref_dir);
        let id = self.id();
        self.raw(id, &format!("AXIS2_PLACEMENT_3D('',#{p},#{a},#{r})"));
        id
    }
}

/// Escape a STEP string literal (the '' doubling rule).
fn esc(s: &str) -> String {
    s.replace('\'', "''")
}

/// Export one named solid as a complete Part 21 document (header +
/// one data section). Multiple objects are appended as additional
/// products into the same data section.
pub fn write_step(objects: &[(String, Mesh)]) -> String {
    let mut w = Writer::new();

    // --- Header -------------------------------------------------------
    let _ = writeln!(
        w.out,
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(\
('freecad-rs faceted STEP export — tessellated B-rep, not exact geometry'),'2;1');"
    );
    let _ = writeln!(
        w.out,
        "FILE_NAME('freecad-rs export.step','{}',('freecad-rs'),('payswapdotorg'),'freecad-rs','','');",
        now_iso()
    );
    let _ = writeln!(
        w.out,
        "FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;"
    );

    // --- Shared context entities ---------------------------------------
    let app_context = w.id();
    w.raw(
        app_context,
        "APPLICATION_CONTEXT('core data for automotive mechanical design processes')",
    );
    let protocol = w.id();
    w.raw(
        protocol,
        &format!(
            "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2010,#{app_context})"
        ),
    );
    let product_def_context = w.id();
    w.raw(
        product_def_context,
        &format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app_context},'design')"),
    );
    let product_context = w.id();
    w.raw(
        product_context,
        &format!("PRODUCT_CONTEXT('',#{app_context},'mechanical')"),
    );
    let length_unit = w.id();
    w.raw(
        length_unit,
        "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
    );
    let angle_unit = w.id();
    w.raw(
        angle_unit,
        "(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))",
    );
    let solid_angle_unit = w.id();
    w.raw(
        solid_angle_unit,
        "(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())",
    );
    let uncertainty = w.id();
    w.raw(
        uncertainty,
        &format!(
            "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{length_unit},'distance_accuracy_value','confusion accuracy')"
        ),
    );
    let geom_context = w.id();
    w.raw(
        geom_context,
        &format!(
            "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{uncertainty}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length_unit},#{angle_unit},#{solid_angle_unit}))REPRESENTATION_CONTEXT('Context #1','3D Context'))"
        ),
    );

    for (name, mesh) in objects {
        export_solid(
            &mut w,
            name,
            mesh,
            app_context,
            product_def_context,
            product_context,
            geom_context,
        );
    }

    let _ = writeln!(w.out, "ENDSEC;\nEND-ISO-10303-21;\n");
    w.out
}

fn export_solid(
    w: &mut Writer,
    name: &str,
    mesh: &Mesh,
    app_context: u64,
    product_def_context: u64,
    product_context: u64,
    geom_context: u64,
) {
    // --- Faces (triangles) ---------------------------------------------
    let mut face_ids = Vec::with_capacity(mesh.triangles.len());
    for t in &mesh.triangles {
        let (ia, ib, ic) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let a = mesh.vertices[ia];
        let b = mesh.vertices[ib];
        let c = mesh.vertices[ic];
        // CCW winding viewed from outside → normal by the right-hand rule.
        let n = Vector3::new(
            (b.y - a.y) * (c.z - a.z) - (b.z - a.z) * (c.y - a.y),
            (b.z - a.z) * (c.x - a.x) - (b.x - a.x) * (c.z - a.z),
            (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x),
        );
        let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
        if len < 1e-20 {
            continue; // degenerate triangle: not exported
        }
        let n = Vector3::new(n.x / len, n.y / len, n.z / len);
        // A stable x-basis in the face's plane.
        let ref_dir = if n.x.abs() < 0.9 {
            Vector3::X
        } else {
            Vector3::Y
        };
        let dot = ref_dir.x * n.x + ref_dir.y * n.y + ref_dir.z * n.z;
        let u = Vector3::new(
            ref_dir.x - dot * n.x,
            ref_dir.y - dot * n.y,
            ref_dir.z - dot * n.z,
        );
        let u_len = (u.x * u.x + u.y * u.y + u.z * u.z).sqrt();
        let u = Vector3::new(u.x / u_len, u.y / u_len, u.z / u_len);

        let pa = w.point(&a);
        let pb = w.point(&b);
        let pc = w.point(&c);
        let loop_id = w.id();
        w.raw(loop_id, &format!("POLY_LOOP('',(#{pa},#{pb},#{pc}))"));
        let bound = w.id();
        w.raw(bound, &format!("FACE_OUTER_BOUND('',#{loop_id},.T.)"));
        let axis = w.axis2(&a, &n, &u);
        let plane = w.id();
        w.raw(plane, &format!("PLANE('',#{axis})"));
        let face = w.id();
        w.raw(face, &format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)"));
        face_ids.push(face);
    }

    // --- Shell + solid ---------------------------------------------------
    let faces_list = face_ids
        .iter()
        .map(|f| format!("#{f}"))
        .collect::<Vec<_>>()
        .join(",");
    let shell = w.id();
    w.raw(shell, &format!("CLOSED_SHELL('',({faces_list}))"));
    let brep = w.id();
    w.raw(brep, &format!("MANIFOLD_SOLID_BREP('{name}',#{shell})"));

    // --- Product structure ----------------------------------------------
    let product = w.id();
    w.raw(
        product,
        &format!(
            "PRODUCT('{}', '{}', '', (#{product_context}))",
            esc(&format!("{FACETED_MARKER}: {name}")),
            esc(name)
        ),
    );
    let formation = w.id();
    w.raw(
        formation,
        &format!(
            "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('', '', #{product}, .NOT_KNOWN.)"
        ),
    );
    let product_definition = w.id();
    w.raw(
        product_definition,
        &format!("PRODUCT_DEFINITION('design', '', #{formation}, #{product_def_context})"),
    );
    let pd_shape = w.id();
    w.raw(
        pd_shape,
        &format!("PRODUCT_DEFINITION_SHAPE('', '', #{product_definition})"),
    );
    let origin = w.id();
    w.raw(origin, "CARTESIAN_POINT('',(0.,0.,0.))");
    let d_z = w.id();
    w.raw(d_z, "DIRECTION('',(0.,0.,1.))");
    let d_x = w.id();
    w.raw(d_x, "DIRECTION('',(1.,0.,0.))");
    let axis = w.id();
    w.raw(
        axis,
        &format!("AXIS2_PLACEMENT_3D('',#{origin},#{d_z},#{d_x})"),
    );
    let shape_rep = w.id();
    w.raw(
        shape_rep,
        &format!("SHAPE_REPRESENTATION('',(#{axis},#{brep}),#{geom_context})"),
    );
    let sdr = w.id();
    w.raw(
        sdr,
        &format!("SHAPE_DEFINITION_REPRESENTATION(#{pd_shape},#{shape_rep})"),
    );
    let _ = app_context;
}

fn now_iso() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let days = secs / 86400;
            let rem = secs % 86400;
            // Civil-from-days (Howard Hinnant's algorithm, condensed).
            let z = days as i64 + 719468;
            let era = z.div_euclid(146097);
            let doe = z.rem_euclid(146097);
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let day = doy - (153 * mp + 2) / 5 + 1;
            let month = if mp < 10 { mp + 3 } else { mp - 9 };
            let year = if month <= 2 { y + 1 } else { y };
            format!(
                "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
                rem / 3600,
                (rem % 3600) / 60,
                rem % 60
            )
        })
        .unwrap_or_else(|_| "1970-01-01T00:00:00".to_string())
}
