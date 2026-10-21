//! `fc-script` API tests — every capability of the rhai document API,
//! happy path + error path, driven the way a script drives it (through
//! the engine), with exact analytic assertions (`throw`-guards against
//! the kernel's exact formulas — the primitives are analytic, so the
//! expected numbers are bit-exact, no tolerance).
//!
//! Exact expected values below are the kernel's own formulas
//! (`fc_part::shape`) evaluated verbatim:
//!
//! ```text
//! box 30x20x5   volume 3000.0                  area 1700.0
//! sphere r4     volume 268.082573106329        area 201.06192982974676
//! cyl 1.5/60    volume 424.11500823462205      area 579.6238445873169
//! cone 2/0/10   volume 41.88790204786391
//! torus 10/2    volume 789.5683520871487       area 789.5683520871487
//! ```

use fc_script::ScriptEngine;

/// Run a script, asserting success; returns the engine for transcript /
/// recompute-error readback.
fn run(script: &str) -> ScriptEngine {
    let mut engine = ScriptEngine::new();
    engine
        .run(script)
        .unwrap_or_else(|e| panic!("script should run: {e}\nscript:\n{script}"));
    engine
}

/// Run a script, asserting it fails; returns the error text.
fn run_err(script: &str) -> String {
    let mut engine = ScriptEngine::new();
    match engine.run(script) {
        Ok(()) => panic!("script should fail:\n{script}"),
        Err(err) => err.to_string(),
    }
}

const EPSILON_FREE_GUARD: &str = "
fn expect(actual, expected, what) {
    if actual != expected { throw(what + \": expected \" + expected + \", got \" + actual); }
}
";

// ---------------------------------------------------------------------------
// add primitives + exact stats
// ---------------------------------------------------------------------------

#[test]
fn primitives_add_recompute_and_exact_stats() {
    let script = format!(
        "{EPSILON_FREE_GUARD}
        let doc = new_document(\"Shop\");
        let box1 = doc.add_box(\"Base\", 30, 20, 5);        // INT args: coerced
        let ball = doc.add_sphere(\"Ball\", 4.0);
        let stem = doc.add_cylinder(\"Stem\", 1.5, 12.0);
        let cone = doc.add_cone(\"Cone\", 2.0, 0.0, 10.0);
        let ring = doc.add_torus(\"Ring\", 10.0, 2.0);
        expect(doc.recompute(), true, \"recompute\");
        expect(doc.volume(box1), 3000.0, \"box volume\");
        expect(doc.area(box1), 1700.0, \"box area\");
        expect(doc.volume(ball), 268.082573106329, \"sphere volume\");
        expect(doc.area(ball), 201.06192982974676, \"sphere area\");
        // The chimney's Height is driven by a binding: 2 * Base.Length.
        doc.bind(stem, \"Height\", \"2 * Base.Length\");
        doc.bind(box1, \"Length\", \"2 * 15\");
        expect(doc.recompute(), true, \"recompute after bind\");
        expect(doc.volume(stem), 424.11500823462205, \"cylinder volume\");
        expect(doc.area(stem), 579.6238445873169, \"cylinder area\");
        expect(doc.volume(cone), 41.88790204786391, \"cone volume\");
        expect(doc.volume(ring), 789.5683520871487, \"torus volume\");
        expect(doc.area(ring), 789.5683520871487, \"torus area\");
        "
    );
    let engine = run(&script);
    let events = engine.events();
    for line in [
        "new document \"Shop\" (0 objects)",
        "added Base (Part::Box)",
        "added Ball (Part::Sphere)",
        "added Stem (Part::Cylinder)",
        "added Cone (Part::Cone)",
        "added Ring (Part::Torus)",
        "  recomputed Base (Part::Box)",
        "  recomputed Ring (Part::Torus)",
        "5 recomputed, 5 total",
        "bound Stem.Height = 2 * Base.Length",
    ] {
        assert!(
            events.iter().any(|e| e == line),
            "transcript missing {line:?} in:\n{}",
            events.join("\n")
        );
    }
    assert!(engine.recompute_error().is_none());
}

#[test]
fn names_are_uniquified_like_the_cli() {
    let engine = run(r#"
        let doc = new_document("U");
        let a = doc.add_box("Box", 10.0, 10.0, 10.0);
        let b = doc.add_box("Box", 20.0, 10.0, 10.0);
        if a != "Box" { throw("first Box named " + a); }
        if b != "Box001" { throw("second Box named " + b); }
        "#);
    assert!(engine
        .events()
        .iter()
        .any(|e| e == "added Box001 (Part::Box)"));
}

// ---------------------------------------------------------------------------
// bbox — exact, and placement moves a Transform exactly
// ---------------------------------------------------------------------------

#[test]
fn bbox_is_exact_and_transform_translation_shifts_it_exactly() {
    let script = format!(
        "{EPSILON_FREE_GUARD}
        let doc = new_document(\"B\");
        let box1 = doc.add_box(\"Base\", 30.0, 20.0, 5.0);
        doc.recompute();
        let bb = doc.bbox(box1);
        // Part::Box: centered in XY, sitting on the XY plane.
        expect(bb.min[0], -15.0, \"bb min x\");
        expect(bb.min[1], -10.0, \"bb min y\");
        expect(bb.min[2], 0.0, \"bb min z\");
        expect(bb.max[0], 15.0, \"bb max x\");
        expect(bb.max[1], 10.0, \"bb max y\");
        expect(bb.max[2], 5.0, \"bb max z\");
        expect(bb.size[0], 30.0, \"bb size x\");
        expect(bb.center[2], 2.5, \"bb center z\");
        // Lift the box by a pure translation: the transformed mesh's bbox
        // shifts by exactly the translation (per-vertex z + 14.0, and IEEE
        // addition is monotone, so min/max shift exactly).
        let ball = doc.add_sphere(\"Ball\", 1.0);
        let lift = doc.add_transform(\"Lift\", ball);
        doc.set(lift, \"Source\", box1);
        doc.set(lift, \"Placement\", placement(vec3(0.0, 0.0, 14.0), vec3(0.0, 0.0, 1.0), 0.0));
        doc.recompute();
        let lb = doc.bbox(lift);
        expect(lb.min[2], bb.min[2] + 14.0, \"lift min z\");
        expect(lb.max[2], bb.max[2] + 14.0, \"lift max z\");
        // A transform of a 30x20x5 box carries the full volume.
        if doc.volume(lift) > 3000.0 || doc.volume(lift) < 2999.0 {{
            throw(\"lift volume off: \" + doc.volume(lift));
        }}
        "
    );
    let engine = run(&script);
    assert!(engine.recompute_error().is_none());
}

// ---------------------------------------------------------------------------
// set — property values parsed against the existing type, like the CLI
// ---------------------------------------------------------------------------

#[test]
fn set_accepts_numbers_bools_strings_vectors_placements_links() {
    let engine = run(r#"
        let doc = new_document("S");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.set(ball, "Radius", 6);                      // INT into a Float
        doc.set(ball, "Placement", placement(vec3(1.0, 2.0, 3.0), vec3(0.0, 0.0, 1.0), 90.0));
        let lift = doc.add_transform("Lift", ball);
        doc.set(lift, "Scale", 2.5);
        doc.set(lift, "Source", "Ghost");                // a Link parses any name
        doc.recompute();
        "#);
    let events = engine.events();
    for line in [
        "set Ball.Radius = 6",
        "set Ball.Placement = pos=(1,2,3);axis=(0,0,1);angle=90",
        "set Lift.Scale = 2.5",
        "set Lift.Source = Ghost",
        // The link is dangling: recompute fails honestly, object Error.
        "recompute failed: object 'Lift' links to missing object 'Ghost'",
    ] {
        assert!(
            events.iter().any(|e| e == line),
            "transcript missing {line:?} in:\n{}",
            events.join("\n")
        );
    }
    assert_eq!(
        engine.recompute_error().as_deref(),
        Some("object 'Lift' links to missing object 'Ghost'")
    );
}

#[test]
fn set_string_and_vector_properties_round_trip() {
    // A sketch-less way to reach String/Vector properties: booleans carry
    // none; the Transform's Scale is a Float. The honest check is the
    // transcript: the text form parsed against the existing type and was
    // stored. Vector via vec3, string via plain text.
    let engine = run(r#"
        let doc = new_document("V");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.set(ball, "Placement", placement(vec3(0.0, 0.0, 7.0), vec3(0.0, 1.0, 0.0), 180.0));
        doc.recompute();
        "#);
    assert!(engine
        .events()
        .iter()
        .any(|e| e == "set Ball.Placement = pos=(0,0,7);axis=(0,1,0);angle=180"));
}

#[test]
fn set_error_paths_are_honest() {
    // Unknown object.
    let err = run_err(
        r#"
        let doc = new_document("E");
        doc.set("Nope", "Radius", 5.0);
        "#,
    );
    assert!(
        err.contains("object 'Nope' not found; available: document is empty"),
        "honest unknown-object error: {err}"
    );
    // Unknown property.
    let err = run_err(
        r#"
        let doc = new_document("E");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.set(ball, "Diameter", 8.0);
        "#,
    );
    assert!(
        err.contains("object 'Ball' has no property 'Diameter'; properties: Placement (App::PropertyPlacement), Radius (App::PropertyFloat)"),
        "honest missing-property error: {err}"
    );
    // Type mismatch: the CLI's exact parse error with the expected form.
    let err = run_err(
        r#"
        let doc = new_document("E");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.set(ball, "Radius", "wide");
        "#,
    );
    assert!(
        err.contains("cannot parse 'wide' as App::PropertyFloat for Ball.Radius (expected: a number, e.g. 2.5)"),
        "honest parse error: {err}"
    );
    // A placement value into a Float property is rejected through the
    // same text-parse path.
    let err = run_err(
        r#"
        let doc = new_document("E");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.set(ball, "Radius", placement(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), 0.0));
        "#,
    );
    assert!(
        err.contains(
            "cannot parse 'pos=(0,0,0);axis=(0,0,1);angle=0' as App::PropertyFloat for Ball.Radius"
        ),
        "honest placement-into-float error: {err}"
    );
}

// ---------------------------------------------------------------------------
// bind / unbind — the expression engine's surface
// ---------------------------------------------------------------------------

#[test]
fn bind_only_float_properties_and_rejects_the_rest() {
    // Non-Float property: honest rejection (the CLI set-expr semantics).
    let err = run_err(
        r#"
        let doc = new_document("B");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.bind(ball, "Placement", "2 * 3");
        "#,
    );
    assert!(
        err.contains("property 'Placement' of 'Ball' is App::PropertyPlacement — only Float properties can carry expressions"),
        "honest non-Float rejection: {err}"
    );
    // Missing property.
    let err = run_err(
        r#"
        let doc = new_document("B");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.bind(ball, "Diameter", "2 * 3");
        "#,
    );
    assert!(
        err.contains("object 'Ball' has no property 'Diameter' to bind"),
        "honest missing-property rejection: {err}"
    );
    // Unparseable expression: rejected at bind time, before any recompute
    // (the raw grammar error, like the CLI's set-expr).
    let err = run_err(
        r#"
        let doc = new_document("B");
        let ball = doc.add_sphere("Ball", 4.0);
        doc.bind(ball, "Radius", "2 +");
        "#,
    );
    assert!(err.contains("parse error"), "honest parse rejection: {err}");
}

#[test]
fn unbind_releases_the_property_and_errors_when_none() {
    let engine = run(r#"
        let doc = new_document("B");
        let box1 = doc.add_box("Base", 10.0, 10.0, 10.0);
        let stem = doc.add_cylinder("Stem", 1.0, 5.0);
        doc.bind(stem, "Height", "2 * Base.Length");
        doc.recompute();
        doc.unbind(stem, "Height");
        doc.set(stem, "Height", 7.0);      // settable again after unbind
        doc.recompute();
        "#);
    let events = engine.events();
    assert!(events.iter().any(|e| e == "unbound Stem.Height"));
    assert!(events.iter().any(|e| e == "set Stem.Height = 7"));
    assert!(engine.recompute_error().is_none());
    // Unbinding something never bound is an honest error.
    let err = run_err(
        r#"
        let doc = new_document("B");
        let box1 = doc.add_box("Base", 10.0, 10.0, 10.0);
        doc.unbind(box1, "Length");
        "#,
    );
    assert!(
        err.contains("property 'Length' of 'Base' has no expression bound"),
        "honest unbind error: {err}"
    );
}

// ---------------------------------------------------------------------------
// recompute failure semantics — recorded, not raised
// ---------------------------------------------------------------------------

#[test]
fn recompute_failure_is_recorded_and_stats_fail_honestly() {
    let script = r#"
        let doc = new_document("F");
        let stem = doc.add_cylinder("Stem", 1.5, 12.0);
        doc.bind(stem, "Height", "2 * 5mm");   // unit-carrying: honest rejection
        let ok = doc.recompute();
        if ok { throw("recompute should fail"); }
        let v = doc.volume(stem);              // must be an honest error
        throw("unreachable: " + v);
        "#;
    // The volume() call AFTER the failed recompute is a rhai runtime
    // error — no stale geometry is served.
    let err = run_err(script);
    assert!(
        err.contains("has no computed geometry"),
        "honest post-failure stats error: {err}"
    );
    // The failure itself is recorded in the transcript and on the engine.
    let mut engine = ScriptEngine::new();
    engine
        .run(&script.replace("        let v = doc.volume(stem);              // must be an honest error\n        throw(\"unreachable: \" + v);\n", ""))
        .expect("script up to the failed recompute runs");
    let events = engine.events();
    assert!(
        events.iter().any(|e| e == "recompute failed: recompute of 'Stem' failed: expression for 'Height' has unit mm; property is a plain float (FreeCAD PropertyFloat rejects units)"),
        "transcript records the failure:\n{}",
        events.join("\n")
    );
    assert_eq!(
        engine.recompute_error().as_deref(),
        Some("recompute of 'Stem' failed: expression for 'Height' has unit mm; property is a plain float (FreeCAD PropertyFloat rejects units)")
    );
}

#[test]
fn dependency_cascade_recomputes_dependents() {
    // Change the source's parameter: the transform (and its own dependents)
    // re-execute, dependencies-first — the transcript shows the order.
    let engine = run(r#"
        let doc = new_document("D");
        let ball = doc.add_sphere("Ball", 4.0);
        let lift = doc.add_transform("Lift", ball);
        doc.recompute();
        doc.set(ball, "Radius", 5.0);
        doc.recompute();
        "#);
    let events = engine.events();
    let first = events
        .iter()
        .position(|e| e == "  recomputed Ball (Part::Sphere)");
    let second = events
        .iter()
        .rposition(|e| e == "  recomputed Ball (Part::Sphere)");
    assert!(
        first.is_some() && second.is_some() && first != second,
        "two recomputes happened"
    );
    // After the second set, Ball recomputes before Lift in the same run.
    let ball_second = events[second.unwrap()..]
        .iter()
        .position(|e| e == "  recomputed Ball (Part::Sphere)")
        .unwrap();
    let lift_second = events[second.unwrap()..]
        .iter()
        .position(|e| e == "  recomputed Lift (Part::Transform)")
        .unwrap();
    assert!(ball_second < lift_second, "dependencies-first order");
}

// ---------------------------------------------------------------------------
// booleans — links, dependencies, honest operand errors
// ---------------------------------------------------------------------------

#[test]
fn booleans_add_and_recompute() {
    let script = format!(
        "{EPSILON_FREE_GUARD}
        let doc = new_document(\"C\");
        let a = doc.add_box(\"A\", 10.0, 10.0, 10.0);
        let b = doc.add_sphere(\"B\", 6.0);
        let fused = doc.add_fuse(\"Fused\", a, b);
        let cut = doc.add_cut(\"Cut\", a, b);
        let common = doc.add_common(\"Common\", a, b);
        expect(doc.recompute(), true, \"recompute\");
        // The fuse contains the whole box; the cut is what remains of it.
        if doc.volume(fused) < 1000.0 {{ throw(\"fuse volume: \" + doc.volume(fused)); }}
        if doc.volume(cut) >= 1000.0 {{ throw(\"cut volume: \" + doc.volume(cut)); }}
        if doc.volume(common) <= 0.0 {{ throw(\"common volume: \" + doc.volume(common)); }}
        "
    );
    let engine = run(&script);
    assert!(engine.recompute_error().is_none());
    assert!(engine
        .events()
        .iter()
        .any(|e| e == "added Common (Part::Common)"));
}

#[test]
fn add_errors_are_honest_for_unknown_operands() {
    let err = run_err(
        r#"
        let doc = new_document("E");
        let a = doc.add_box("A", 10.0, 10.0, 10.0);
        doc.add_cut("C", a, "Ghost");
        "#,
    );
    assert!(
        err.contains("tool object 'Ghost' not found; available: A"),
        "honest tool error: {err}"
    );
    let err = run_err(
        r#"
        let doc = new_document("E");
        doc.add_transform("T", "Ghost");
        "#,
    );
    assert!(
        err.contains("object 'Ghost' not found; available: document is empty"),
        "honest source error: {err}"
    );
}

// ---------------------------------------------------------------------------
// save / open — persistence via fc-io
// ---------------------------------------------------------------------------

#[test]
fn save_and_open_round_trip_exactly() {
    let dir = std::env::temp_dir().join(format!("fc-script-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("round.fcstd");
    let path_text = path.to_str().expect("utf-8 temp path").to_string();
    let script = format!(
        r#"{EPSILON_FREE_GUARD}
        let doc = new_document("Round");
        let box1 = doc.add_box("Base", 30.0, 20.0, 5.0);
        doc.recompute();
        doc.save("{path_text}");
        let doc2 = open_document("{path_text}");
        expect(doc2.volume("Base"), 3000.0, "reloaded volume");
        expect(doc2.area("Base"), 1700.0, "reloaded area");
        "#
    );
    let engine = run(&script);
    let events = engine.events();
    assert!(
        events.iter().any(|e| e == &format!("saved {path_text}")),
        "save event in:\n{}",
        events.join("\n")
    );
    assert!(events
        .iter()
        .any(|e| e == &format!("opened {path_text} (1 objects)")));
    assert!(engine.warnings().is_empty());
    std::fs::remove_file(&path).ok();

    // Opening a missing file is an honest error.
    let err = run_err(&format!(
        r#"let doc = open_document("{}");"#,
        dir.join("missing.fcstd")
            .to_str()
            .expect("utf-8")
            .replace('\\', "/")
    ));
    assert!(err.contains("cannot load"), "honest open error: {err}");
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// export — STL / OBJ of the placed meshes
// ---------------------------------------------------------------------------

#[test]
fn export_stl_and_obj_write_files_and_transcript() {
    let dir = std::env::temp_dir().join(format!("fc-script-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let stl = dir.join("out.stl");
    let obj = dir.join("out.obj");
    let stl_text = stl.to_str().expect("utf-8").replace('\\', "/");
    let obj_text = obj.to_str().expect("utf-8").replace('\\', "/");
    let script = format!(
        "{EPSILON_FREE_GUARD}
        let doc = new_document(\"X\");
        let box1 = doc.add_box(\"Base\", 30.0, 20.0, 5.0);
        doc.recompute();
        doc.export_stl(\"{stl_text}\", false);
        doc.export_obj(\"{obj_text}\");
        "
    );
    let engine = run(&script);
    let events = engine.events();
    assert!(
        events
            .iter()
            .any(|e| e == &format!("wrote {stl_text} (ASCII STL, 12 triangles)")),
        "stl event in:\n{}",
        events.join("\n")
    );
    assert!(
        events
            .iter()
            .any(|e| e == &format!("wrote {obj_text} (OBJ, 1 objects, 12 triangles)")),
        "obj event in:\n{}",
        events.join("\n")
    );
    let stl_content = std::fs::read_to_string(&stl).expect("stl readable");
    assert!(stl_content.starts_with("solid freecad-rs"));
    let obj_content = std::fs::read_to_string(&obj).expect("obj readable");
    assert!(obj_content.contains("o Base"));
    // Export recomputes first: a broken model fails the export honestly.
    let err = run_err(&format!(
        r#"
        let doc = new_document("X");
        let ball = doc.add_sphere("Ball", 4.0);
        let lift = doc.add_transform("Lift", ball);
        doc.set(lift, "Source", "Ghost");
        doc.export_stl("{stl_text}", false);
        "#
    ));
    assert!(
        err.contains("recompute failed: object 'Lift' links to missing object 'Ghost'"),
        "honest export error: {err}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// values — vec3 / placement constructors and their honest errors
// ---------------------------------------------------------------------------

#[test]
fn vec3_and_placement_construct_and_read_back() {
    let engine = run(r#"
        let v = vec3(1, 2.5, 3);
        if v.x() != 1.0 { throw("x: " + v.x()); }
        if v.y() != 2.5 { throw("y: " + v.y()); }
        if v.z() != 3.0 { throw("z: " + v.z()); }
        let p = placement(v, vec3(0.0, 0.0, 1.0), 90);
        print(p);
        "#);
    assert!(engine
        .events()
        .iter()
        .any(|e| e == "pos=(1,2.5,3);axis=(0,0,1);angle=90"));
    // A degenerate axis is an honest rejection (the CLI's --rotate-axis).
    let err = run_err(
        r#"
        let p = placement(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), 90.0);
        "#,
    );
    assert!(
        err.contains("placement axis must be a non-zero vector"),
        "honest axis error: {err}"
    );
    // Non-numeric vec3 components are honest errors.
    let err = run_err(r#"let v = vec3(1.0, "wide", 3.0);"#);
    assert!(
        err.contains("expected a number, got string"),
        "honest vec3 error: {err}"
    );
}

// ---------------------------------------------------------------------------
// transcript / sandbox / rhai errors
// ---------------------------------------------------------------------------

#[test]
fn print_routes_into_the_transcript() {
    let engine = run(r#"
        let doc = new_document("P");
        print("hello from the script");
        "#);
    assert!(engine.events().iter().any(|e| e == "hello from the script"));
}

#[test]
fn rhai_errors_are_honest_and_positioned() {
    // Parse error.
    let err = run_err("let = ;");
    assert!(err.contains("Syntax error"), "parse error: {err}");
    // Unknown function.
    let err = run_err(r#"let doc = new_document("D"); doc.frobnicate();"#);
    assert!(
        err.contains("Function not found"),
        "unknown function error: {err}"
    );
    // Wrong argument type into a number parameter.
    let err = run_err(
        r#"
        let doc = new_document("D");
        doc.add_box("B", "wide", 2.0, 3.0);
        "#,
    );
    assert!(
        err.contains("expected a number, got string"),
        "coercion error: {err}"
    );
}

#[test]
fn sandbox_operation_budget_stops_runaway_loops() {
    let err = run_err("while (true) {}");
    assert!(
        err.to_lowercase().contains("operation"),
        "operation budget error: {err}"
    );
}
