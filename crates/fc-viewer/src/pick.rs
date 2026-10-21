//! CPU picking — an orthographic ray through the pixel, cast against
//! the scene's triangles (Möller–Trumbore, nearest hit).
//!
//! The camera is orthographic (see [`crate::camera`]), so a pick ray
//! is: **origin** = the eye translated within the screen plane to the
//! pixel's NDC position, **direction** = the camera's view direction
//! (`-back`) — the same ray for every pixel, offset per pixel. This is
//! the exact inverse of
//! [`Camera::screen_pixel`](crate::camera::Camera::screen_pixel)'s
//! top-left-origin mapping.

use crate::camera::Camera;
use crate::scene::Scene;

/// The object rendered at pixel (`x`, `y`), if any.
///
/// The ray is cast through the pixel's center, in the camera's frame
/// (the same framing `render` uses). Error-flagged objects are pickable
/// like any other.
pub fn pick_at(
    scene: &Scene,
    camera: &Camera,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
) -> Option<String> {
    if width == 0 || height == 0 {
        return None;
    }
    let aspect = width as f64 / height as f64;
    let extent = camera.extent();
    // The pixel's camera-space offset from the eye (y up, top-left
    // pixel grid — the exact inverse of `Camera::screen_pixel`):
    // camera x of the pixel = ndc_x · extent_x, y = ndc_y · extent.
    let cx = (2.0 * (x as f64 + 0.5) / width as f64 - 1.0) * extent * aspect;
    let cy = (1.0 - 2.0 * (y as f64 + 0.5) / height as f64) * extent;

    // Orthographic: the ray is the camera's view direction (-back),
    // offset to the pixel's screen-plane position.
    let (right, up, back) = (camera.right(), camera.up(), camera.back());
    let dir = [-back[0], -back[1], -back[2]];
    let eye = camera.eye();
    let origin = [
        eye[0] + right[0] * cx + up[0] * cy,
        eye[1] + right[1] * cx + up[1] * cy,
        eye[2] + right[2] * cx + up[2] * cy,
    ];

    let mut best: Option<(f64, &str)> = None;
    for object in &scene.objects {
        let tris = object.positions.chunks_exact(3);
        for tri in tris {
            if let Some(t) = hit_triangle(&origin, &dir, &tri[0], &tri[1], &tri[2]) {
                if best.is_none() || t < best.unwrap().0 {
                    best = Some((t, object.name.as_str()));
                }
            }
        }
    }
    best.map(|(_, name)| name.to_string())
}

/// Möller–Trumbore triangle intersection: the ray parameter `t` when the
/// ray hits, `None` when it misses (or hits the back face — solids are
/// closed, so the front face always wins the nearest-hit race).
fn hit_triangle(
    origin: &[f64; 3],
    dir: &[f64; 3],
    a: &[f64; 3],
    b: &[f64; 3],
    c: &[f64; 3],
) -> Option<f64> {
    const EPS: f64 = 1e-12;
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let h = [
        dir[1] * e2[2] - dir[2] * e2[1],
        dir[2] * e2[0] - dir[0] * e2[2],
        dir[0] * e2[1] - dir[1] * e2[0],
    ];
    let det = e1[0] * h[0] + e1[1] * h[1] + e1[2] * h[2];
    if det.abs() < EPS {
        return None; // parallel or degenerate
    }
    let inv = 1.0 / det;
    let s = [origin[0] - a[0], origin[1] - a[1], origin[2] - a[2]];
    let u = inv * (s[0] * h[0] + s[1] * h[1] + s[2] * h[2]);
    if u < 0.0 || u > 1.0 {
        return None;
    }
    let q = [
        s[1] * e1[2] - s[2] * e1[1],
        s[2] * e1[0] - s[0] * e1[2],
        s[0] * e1[1] - s[1] * e1[0],
    ];
    let v = inv * (dir[0] * q[0] + dir[1] * q[1] + dir[2] * q[2]);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = inv * (e2[0] * q[0] + e2[1] * q[1] + e2[2] * q[2]);
    (t > EPS).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::View;
    use crate::scene::SceneObject;

    /// A camera looking down -Z from +Z (the `top` view is a pure frame
    /// for this), a single triangle in the z=0 plane, and exact pixel
    /// assertions — the orthographic pick contract, no files involved.
    #[test]
    fn ortho_pick_hits_exact_pixels() {
        // Camera framed on the unit box around the origin, top view:
        // right=+x, up=+y, back=+z (looking down -z).
        let cam = Camera::framing([0.0, 0.0, 0.0], 1.0, View::Top);
        // Triangle covering x,y in [-0.5, 0.5] (the full frame at the
        // center pixel).
        let object = SceneObject {
            name: "Tri".into(),
            color: [1.0, 1.0, 1.0, 1.0],
            base_color: [1.0, 1.0, 1.0, 1.0],
            flagged: false,
            positions: vec![[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
        };
        let scene = Scene {
            objects: vec![object],
            center: [0.0, 0.0, 0.0],
            radius: 1.0,
            recompute_error: None,
        };
        // Inside the triangle (center pixel): hit.
        assert_eq!(
            pick_at(&scene, &cam, 100, 100, 50, 50),
            Some("Tri".to_string()),
            "center pixel must hit the triangle"
        );
        // Outside (top-left corner, empty margin): miss.
        assert_eq!(pick_at(&scene, &cam, 100, 100, 2, 2), None);
        // Below the hypotenuse (bottom-center): miss.
        assert_eq!(pick_at(&scene, &cam, 100, 100, 50, 95), None);
    }

    /// Two stacked squares: the front one wins (nearest-hit).
    #[test]
    fn nearest_hit_wins() {
        let cam = Camera::framing([0.0, 0.0, 0.0], 1.0, View::Top);
        let square = |name: &str, z: f64| SceneObject {
            name: name.into(),
            color: [1.0, 1.0, 1.0, 1.0],
            base_color: [1.0, 1.0, 1.0, 1.0],
            flagged: false,
            positions: vec![
                [-0.5, -0.5, z],
                [0.5, -0.5, z],
                [0.5, 0.5, z],
                [-0.5, -0.5, z],
                [0.5, 0.5, z],
                [-0.5, 0.5, z],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 6],
        };
        // back is toward the camera in the top view: z=+0.5 is in front.
        let scene = Scene {
            objects: vec![square("Back", -0.5), square("Front", 0.5)],
            center: [0.0, 0.0, 0.0],
            radius: 1.0,
            recompute_error: None,
        };
        assert_eq!(
            pick_at(&scene, &cam, 100, 100, 50, 50),
            Some("Front".to_string())
        );
    }
}
