//! Always-on (CPU) integration tests: scene assembly, picking, honest
//! error paths. The GPU render tests live in `gpu_render.rs` behind
//! `FC_VIEWER_GPU=1`.

mod common;

use fc_viewer::camera::{Camera, View};
use fc_viewer::scene::build_scene;
use fc_viewer::{pick_object, render_png};

fn demo() -> String {
    common::demo_path().to_string_lossy().into_owned()
}

/// World-space centroid of an object's triangle soup (the sphere's
/// centroid is its center — good enough as a pick target).
fn centroid(object: &fc_viewer::scene::SceneObject) -> [f64; 3] {
    let mut sum = [0.0f64; 3];
    for p in &object.positions {
        sum[0] += p[0];
        sum[1] += p[1];
        sum[2] += p[2];
    }
    let n = object.positions.len().max(1) as f64;
    [sum[0] / n, sum[1] / n, sum[2] / n]
}

/// Every object is pickable somewhere in the frame (nearest-hit
/// semantics: overlaps go to the front object, so scan a grid).
fn picked_names(
    scene: &fc_viewer::scene::Scene,
    camera: &Camera,
    width: u32,
    height: u32,
) -> Vec<String> {
    let mut names = Vec::new();
    for y in (0..height as usize).step_by((height / 20) as usize) {
        for x in (0..width as usize).step_by((width / 32) as usize) {
            if let Some(name) =
                fc_viewer::pick::pick_at(scene, camera, width, height, x as u32, y as u32)
            {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

#[test]
fn demo_scene_loads_all_four_objects() {
    let scene = build_scene(&demo()).expect("demo loads");
    assert_eq!(scene.object_names(), vec!["Base", "Ball", "Stem", "Lift"]);
    assert!(scene.objects.iter().all(|o| !o.flagged), "all healthy");
    assert!(scene.recompute_error.is_none());
    assert!(scene.triangle_count() > 0);
}

#[test]
fn demo_scene_triangle_count_is_pinned() {
    let scene = build_scene(&demo()).expect("demo loads");
    // Pinned from the committed demo document at deflection 0.2 mm —
    // the same tessellation every export uses. Any change here is a
    // rendering-surface regression (or a deliberate tessellation change
    // that must re-bless the golden).
    assert_eq!(scene.triangle_count(), 18156, "rendered surface changed");
}

#[test]
fn every_demo_object_is_pickable_somewhere() {
    let scene = build_scene(&demo()).expect("demo loads");
    let cam = Camera::framing(scene.center, scene.radius, View::Iso);
    let mut names = picked_names(&scene, &cam, 1280, 800);
    names.sort();
    assert_eq!(
        names,
        vec!["Ball", "Base", "Lift", "Stem"],
        "every object must be reachable by a click somewhere"
    );
}

#[test]
fn ball_centroid_pixel_picks_the_nearest_object() {
    // The Ball's projected centroid ray may hit a nearer occluder (the
    // demo's Lift transform overlaps it in the iso view) — nearest-hit
    // is the correct semantic. The pixel must pick *some* demo object.
    let scene = build_scene(&demo()).expect("demo loads");
    let cam = Camera::framing(scene.center, scene.radius, View::Iso);
    let ball = scene
        .objects
        .iter()
        .find(|o| o.name == "Ball")
        .expect("Ball in the demo");
    let pixel = cam.screen_pixel(centroid(ball), 1280.0, 800.0);
    let hit = pick_object(&demo(), "iso", 1280, 800, pixel[0] as u32, pixel[1] as u32)
        .expect("pick runs")
        .expect("the Ball's projected center hits something");
    assert!(["Ball", "Base", "Stem", "Lift"].contains(&hit.as_str()));
}

#[test]
fn pick_misses_the_empty_margin() {
    // Framing guarantees MARGIN (8%) of empty space on every side, so
    // the very corner of the frame is background.
    let miss = pick_object(&demo(), "iso", 1280, 800, 2, 2).expect("pick runs");
    assert_eq!(miss, None, "top-left corner must be empty");
}

#[test]
fn missing_file_and_bad_view_error_honestly() {
    assert!(render_png("no-such.fcstd", "/tmp/x.png", "iso", 8, 8, false).is_err());
    assert!(pick_object("no-such.fcstd", "iso", 8, 8, 0, 0).is_err());
    assert!(render_png(&demo(), "/tmp/x.png", "diagonal", 8, 8, false).is_err());
    assert!(pick_object(&demo(), "diagonal", 8, 8, 0, 0).is_err());
}

#[test]
fn error_doc_scene_flags_the_broken_object_with_geometry() {
    let (path, broken) = common::error_doc();
    let scene = build_scene(path.to_string_lossy().as_ref()).expect("error doc loads");
    // A loaded document's objects are not Touched, so the load-time
    // recompute is a no-op — the failure state arrives via the
    // *persisted* Error status (FreeCAD persists object errors in the
    // file; loading shows them). The flag, not a fresh recompute
    // failure, is what the viewer must render honestly.
    assert!(
        scene.recompute_error.is_none(),
        "load-time recompute must be a no-op (nothing Touched)"
    );
    let flagged: Vec<&str> = scene
        .objects
        .iter()
        .filter(|o| o.flagged)
        .map(|o| o.name.as_str())
        .collect();
    assert_eq!(
        flagged,
        vec![broken],
        "exactly the broken object is flagged"
    );
    let flagged_object = scene
        .objects
        .iter()
        .find(|o| o.flagged)
        .expect("flagged object present");
    assert!(
        flagged_object.triangle_count() > 0,
        "the flagged object keeps its last-good geometry"
    );
    // ...and its rendered color is the error tint, not its base color.
    assert_ne!(flagged_object.color, flagged_object.base_color);

    // The flagged object is pickable (M1: error objects render and pick
    // like any other — no crash on half-computed documents). Scan: the
    // Lift's own region must report Lift despite the Error status.
    let scene2 = build_scene(path.to_string_lossy().as_ref()).unwrap();
    let cam = Camera::framing(scene2.center, scene2.radius, View::Iso);
    let names = picked_names(&scene2, &cam, 1280, 800);
    assert!(
        names.iter().any(|n| n == broken),
        "the error-flagged object must be pickable (found: {names:?})"
    );
}
