//! Viewer camera — pure `f64` math, no GPU types.
//!
//! The camera is a **dolly-framed orthographic turntable**: an
//! orbit/pan/zoom camera whose framing (the visible half-height,
//! [`Camera::extent`]) is tied to the eye-to-target distance by a
//! 45°-equivalent field of view, `extent = distance · tan(FOV/2)`.
//! Consequences (all deliberate):
//!
//! * **zoom** multiplies the distance exponentially in wheel ticks; the
//!   extent follows, so the image scales with it even under an
//!   orthographic projection.
//! * **pan** moves the target along the screen-plane basis by a
//!   world-units-per-pixel proportional to the extent, hence to the
//!   distance.
//! * **orbit** rotates the camera basis around the (fixed) target.
//!
//! Conventions (inherited from `freecad-rs`'s `svg.rs` renderer, so the
//! two cameras stay comparable): camera space has x right, y up, z
//! *toward the viewer*; the world→camera matrix follows fc-base's
//! row-vector convention (`p' = p * M`, rows = images of the world axes).
//! The camera basis (right/up/back, world-space) is the columns of that
//! matrix; the eye sits at `target + distance · back`.
//!
//! The four standard views reproduce `export svg`'s cameras exactly —
//! same matrices, entry for entry. The parity test in `tests/camera.rs`
//! parses `../freecad-rs/src/svg.rs` itself and asserts equality, so the
//! two cameras cannot fork quietly.
//!
//! Sign conventions for the interactive gestures (drag directions,
//! wheel direction) are UX choices made to feel like FreeCAD's turntable
//! and are documented per-method; they are not unit-testable
//! headlessly.

use fc_base::{Matrix4, Rotation, Vector3};

/// Equivalent vertical field of view: the orthographic half-height at the
/// target plane equals `distance · tan(FOV/2)`.
pub const FOV_EQUIV_DEG: f64 = 45.0;

/// Framing margin: the scene's bounding sphere keeps this fraction of the
/// frame free on every side (svg.rs fits with margins; this is the
/// viewer's analogous, orientation-independent rule).
pub const MARGIN: f64 = 0.08;

/// One wheel tick multiplies the distance by this factor (zoom-in).
pub const ZOOM_BASE: f64 = 1.1;

/// Orbit sensitivity, degrees of camera rotation per pixel of drag.
pub const ORBIT_DEG_PER_PIXEL: f64 = 0.5;

/// `tan(FOV/2)`: half-height per unit distance.
fn half_fov_tan() -> f64 {
    (FOV_EQUIV_DEG * 0.5).to_radians().tan()
}

/// The four standard views — the same cameras as `export svg`
/// (`freecad-rs`'s `crate::svg::View`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Classic isometric-ish: yaw -30 deg, pitch -20 deg.
    Iso,
    /// Looking down `-Z` (screen x = world x, screen y = world y).
    Top,
    /// Looking along `+Y` (camera on `-Y`).
    Front,
    /// Looking along `-X` (camera on `+X`).
    Right,
}

impl View {
    /// Parse a CLI view name (`iso|top|front|right`), like `svg::View`.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "iso" => Ok(View::Iso),
            "top" => Ok(View::Top),
            "front" => Ok(View::Front),
            "right" => Ok(View::Right),
            other => Err(format!("unknown view '{other}' (iso|top|front|right)")),
        }
    }

    /// Canonical lowercase name.
    pub fn label(&self) -> &'static str {
        match self {
            View::Iso => "iso",
            View::Top => "top",
            View::Front => "front",
            View::Right => "right",
        }
    }

    /// World→camera rotation, identical entry-for-entry to `svg.rs`'s
    /// `View::matrix()` (rows = images of the world axes; the parity test
    /// parses that source file and asserts equality).
    pub fn svg_view_matrix(&self) -> Matrix4 {
        match self {
            View::Iso => {
                let yaw = Rotation::around_z((-30.0_f64).to_radians()).to_matrix();
                let pitch = Rotation::around_x((-20.0_f64).to_radians()).to_matrix();
                // Row-vector convention: `A.multiply(&B)` applies A first.
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

/// The orbit/pan/zoom camera. All math is `f64`; the GPU pass narrows to
/// `f32` at upload time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Orbit/pan center (world space). Orbit keeps it fixed; pan
    /// translates it parallel to the screen plane.
    target: [f64; 3],
    /// Eye-to-target distance. Zoom multiplies it exponentially in wheel
    /// ticks; the orthographic extent follows it.
    distance: f64,
    /// Camera basis in world space: screen-right, screen-up, and
    /// toward-the-viewer. Orthonormal; `right × up = back`.
    right: [f64; 3],
    up: [f64; 3],
    back: [f64; 3],
    /// Scene bounding radius the camera was framed for — sizes the depth
    /// range and the zoom clamp.
    fit_radius: f64,
}

impl Camera {
    /// A camera framed on a scene bounding sphere: the standard view's
    /// orientation, target at the sphere center, distance chosen so the
    /// sphere (with [`MARGIN`] free on every side) fits the frame.
    ///
    /// A degenerate `radius` (≤ 0, empty scene) is framed as a unit
    /// sphere — renders stay finite even with no geometry.
    pub fn framing(center: [f64; 3], radius: f64, view: View) -> Camera {
        let radius = if radius > 1e-12 { radius } else { 1.0 };
        let basis = Self::basis_of(view.svg_view_matrix());
        Camera {
            target: center,
            distance: (radius / (1.0 - 2.0 * MARGIN)) / half_fov_tan(),
            right: basis.0,
            up: basis.1,
            back: basis.2,
            fit_radius: radius,
        }
    }

    /// Camera basis (columns) of a world→camera rotation matrix.
    fn basis_of(m: Matrix4) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let r = &m.m;
        (
            [r[0][0], r[1][0], r[2][0]],
            [r[0][1], r[1][1], r[2][1]],
            [r[0][2], r[1][2], r[2][2]],
        )
    }

    /// Snap to a standard view, keeping the current target, distance and
    /// framing.
    pub fn set_view(&mut self, view: View) {
        let (right, up, back) = Self::basis_of(view.svg_view_matrix());
        self.right = right;
        self.up = up;
        self.back = back;
    }

    /// Orbit around the target (which stays fixed): `dx`/`dy` are drag
    /// deltas in pixels. Dragging right yaws the model's front to the
    /// right; dragging down pitches the camera to look from further
    /// below. The basis is re-orthonormalized to absorb rounding drift.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let yaw = Rotation::from_axis_angle(
            Vector3::new(0.0, 0.0, 1.0),
            (-dx * ORBIT_DEG_PER_PIXEL).to_radians(),
        )
        .unwrap_or_else(|_| Rotation::around_x(0.0));
        let right_axis = Vector3::new(self.right[0], self.right[1], self.right[2]);
        let pitch = Rotation::from_axis_angle(right_axis, (-dy * ORBIT_DEG_PER_PIXEL).to_radians())
            .unwrap_or_else(|_| Rotation::around_x(0.0));
        let rotate = |v: [f64; 3]| -> [f64; 3] {
            let step = yaw.rotate_vector(&Vector3::new(v[0], v[1], v[2]));
            let out = pitch.rotate_vector(&step);
            [out.x, out.y, out.z]
        };
        self.right = rotate(self.right);
        self.up = rotate(self.up);
        self.back = rotate(self.back);
        self.orthonormalize();
    }

    /// Zoom by wheel ticks — the exponential law
    /// `distance ← distance · ZOOM_BASE^(−ticks)`. Positive ticks zoom in
    /// (wheel up). Clamped so the camera cannot collapse onto the target
    /// or run away to infinity; within the clamp the law is exact.
    pub fn zoom(&mut self, ticks: f64) {
        let factor = ZOOM_BASE.powf(-ticks);
        self.distance = (self.distance * factor).clamp(self.min_distance(), self.max_distance());
    }

    /// Pan: translate the target parallel to the screen plane. `dx`/`dy`
    /// are drag deltas in pixels (y down); the drag follows the cursor
    /// (drag right moves the scene right). World-units-per-pixel is
    /// `2 · extent / viewport_height` — proportional to the distance.
    pub fn pan(&mut self, dx: f64, dy: f64, viewport_height: f64) {
        if viewport_height <= 0.0 {
            return;
        }
        let world_per_pixel = 2.0 * self.extent() / viewport_height;
        for i in 0..3 {
            self.target[i] += (-self.right[i] * dx + self.up[i] * dy) * world_per_pixel;
        }
    }

    /// Re-orthonormalize the basis (Gram-Schmidt, `back` primary).
    fn orthonormalize(&mut self) {
        self.back = normalized(self.back);
        // right ← right − (right·back)·back, normalized.
        let projection = dot(self.right, self.back);
        for i in 0..3 {
            self.right[i] -= projection * self.back[i];
        }
        self.right = normalized(self.right);
        self.up = cross(self.back, self.right);
    }

    pub fn target(&self) -> [f64; 3] {
        self.target
    }

    pub fn distance(&self) -> f64 {
        self.distance
    }

    /// Orthographic half-height of the visible frame at the target plane
    /// (the full height is `2 · extent`).
    pub fn extent(&self) -> f64 {
        self.distance * half_fov_tan()
    }

    /// Screen-right basis vector (world space).
    pub fn right(&self) -> [f64; 3] {
        self.right
    }

    /// Screen-up basis vector (world space).
    pub fn up(&self) -> [f64; 3] {
        self.up
    }

    /// Toward-the-viewer basis vector (world space); the eye sits at
    /// `target + distance · back`.
    pub fn back(&self) -> [f64; 3] {
        self.back
    }

    /// Where the camera looks (unit vector, world space): `-back`.
    pub fn view_direction(&self) -> [f64; 3] {
        [-self.back[0], -self.back[1], -self.back[2]]
    }

    /// The eye position (world space).
    pub fn eye(&self) -> [f64; 3] {
        [
            self.target[0] + self.distance * self.back[0],
            self.target[1] + self.distance * self.back[1],
            self.target[2] + self.distance * self.back[2],
        ]
    }

    /// Lower zoom clamp (the target must stay reachable).
    pub fn min_distance(&self) -> f64 {
        self.fit_radius * 1e-4
    }

    /// Upper zoom clamp.
    pub fn max_distance(&self) -> f64 {
        self.fit_radius * 1e6
    }

    /// World→camera rotation, svg.rs's row-vector convention: rows are
    /// the images of the world axes. Bitwise identical to
    /// `View::svg_view_matrix` for a freshly set standard view.
    pub fn world_to_camera(&self) -> Matrix4 {
        Matrix4 {
            m: [
                [self.right[0], self.up[0], self.back[0], 0.0],
                [self.right[1], self.up[1], self.back[1], 0.0],
                [self.right[2], self.up[2], self.back[2], 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// Camera-space coordinates of a world point: `(x right, y up, z
    /// toward the viewer)` relative to the eye.
    pub fn camera_point(&self, p: [f64; 3]) -> [f64; 3] {
        let eye = self.eye();
        let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
        [dot(d, self.right), dot(d, self.up), dot(d, self.back)]
    }

    /// The pixel a world point projects to (top-left origin, y down —
    /// image coordinates, matching the readback/PNG orientation).
    /// Consistent with [`Camera::view_projection`].
    pub fn screen_pixel(&self, p: [f64; 3], width: f64, height: f64) -> [f64; 2] {
        let c = self.camera_point(p);
        let aspect = width / height;
        let extent = self.extent();
        [
            (c[0] / (extent * aspect) + 1.0) * 0.5 * width,
            (1.0 - c[1] / extent) * 0.5 * height,
        ]
    }

    /// The world-space headlight `svg.rs` shades with — its camera-space
    /// direction `(0.35, 0.35, 0.87)`, normalized, rotated into the world.
    pub fn headlight_world(&self) -> [f64; 3] {
        let light = normalized([0.35, 0.35, 0.87]);
        [
            light[0] * self.right[0] + light[1] * self.up[0] + light[2] * self.back[0],
            light[0] * self.right[1] + light[1] * self.up[1] + light[2] * self.back[1],
            light[0] * self.right[2] + light[1] * self.up[2] + light[2] * self.back[2],
        ]
    }

    /// The full world→clip matrix for the GPU pass (column-vector
    /// convention `p_clip = A · p_world`; upload column-major as `f32`).
    ///
    /// NDC follows WebGPU/wgpu (the D3D/Metal family, not Vulkan): x
    /// right, y **up** (+1 = the top row of the render target), z in
    /// `[0, 1]` growing away from the viewer — camera-up maps directly
    /// to the top of the readback image, matching `screen_pixel`'s
    /// top-left-origin pixel grid. Orthographic, so the depth range is
    /// chosen for ordering only: everything up to two fit radii *behind*
    /// the eye stays inside (matters when zoomed into the model) down to
    /// the far side of the scene.
    pub fn view_projection(&self, aspect: f64) -> [[f64; 4]; 4] {
        let extent = self.extent();
        let extent_x = extent * aspect;
        let near_cz = 2.0 * self.fit_radius; // behind the eye
        let far_cz = -(self.distance + 2.0 * self.fit_radius);
        let depth_span = near_cz - far_cz;

        // View (column convention): camera coords of (p − eye).
        let eye = self.eye();
        let basis = [self.right, self.up, self.back];
        let mut view = [[0.0; 4]; 4];
        for (i, b) in basis.iter().enumerate() {
            view[i][..3].copy_from_slice(b);
            view[i][3] = -dot(eye, *b);
        }
        view[3] = [0.0, 0.0, 0.0, 1.0];

        // Projection (column convention): orthographic, WebGPU NDC
        // (y up, z [0,1] away from the viewer).
        let mut proj = [[0.0; 4]; 4];
        proj[0][0] = 1.0 / extent_x;
        proj[1][1] = 1.0 / extent;
        proj[2][2] = -1.0 / depth_span;
        proj[2][3] = near_cz / depth_span;
        proj[3][3] = 1.0;

        // A = proj · view
        let mut a = [[0.0; 4]; 4];
        for (row, out_row) in a.iter_mut().enumerate() {
            for (col, out) in out_row.iter_mut().enumerate() {
                *out = (0..4).map(|k| proj[row][k] * view[k][col]).sum();
            }
        }
        a
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(v: [f64; 3]) -> [f64; 3] {
    let len = dot(v, v).sqrt();
    if len > 0.0 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn framing_fits_sphere_with_margin() {
        let cam = Camera::framing([1.0, 2.0, 3.0], 10.0, View::Iso);
        assert_eq!(cam.target(), [1.0, 2.0, 3.0]);
        // extent = radius / (1 - 2·margin)
        assert!(approx(cam.extent(), 10.0 / (1.0 - 2.0 * MARGIN), 1e-12));
        // distance = extent / tan(fov/2)
        assert!(approx(cam.distance, cam.extent() / half_fov_tan(), 1e-12));
        // degenerate radius frames a unit sphere, never NaN
        let empty = Camera::framing([0.0; 3], 0.0, View::Top);
        assert!(empty.extent().is_finite() && empty.extent() > 0.0);
    }

    #[test]
    fn world_to_camera_of_standard_views_is_svg_matrix() {
        for view in [View::Iso, View::Top, View::Front, View::Right] {
            let cam = Camera::framing([0.0; 3], 5.0, view);
            assert_eq!(
                cam.world_to_camera().m,
                view.svg_view_matrix().m,
                "{view:?} view matrix forked from svg.rs"
            );
        }
    }

    #[test]
    fn semantic_view_directions_and_ups() {
        // The directions/up vectors svg.rs documents in its comments.
        let cases: [(View, [f64; 3], [f64; 3]); 4] = [
            (View::Top, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            (View::Front, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            (View::Right, [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            // Iso: from the -30° yaw / -20° pitch construction — the
            // camera ends up above, slightly front (-Y) and right (+X).
            (
                View::Iso,
                [
                    -(30.0_f64).to_radians().sin() * (20.0_f64).to_radians().sin(),
                    (30.0_f64).to_radians().cos() * (20.0_f64).to_radians().sin(),
                    -(20.0_f64).to_radians().cos(),
                ],
                [
                    -(30.0_f64).to_radians().sin() * (20.0_f64).to_radians().cos(),
                    (30.0_f64).to_radians().cos() * (20.0_f64).to_radians().cos(),
                    (20.0_f64).to_radians().sin(),
                ],
            ),
        ];
        for (view, forward, up) in cases {
            let cam = Camera::framing([0.0; 3], 5.0, view);
            for i in 0..3 {
                assert!(
                    approx(cam.view_direction()[i], forward[i], 1e-12),
                    "{view:?} view_direction[{i}] = {} (want {forward:?})",
                    cam.view_direction()[i]
                );
                assert!(
                    approx(cam.up()[i], up[i], 1e-12),
                    "{view:?} up[{i}] = {} (want {up:?})",
                    cam.up()[i]
                );
            }
        }
    }

    #[test]
    fn orbit_keeps_target_fixed_and_basis_orthonormal() {
        let mut cam = Camera::framing([3.0, -2.0, 7.0], 4.0, View::Iso);
        let target = cam.target();
        for k in 0..1000 {
            cam.orbit(7.3 * (k as f64 % 5.0) - 10.0, 3.1 * (k as f64 % 7.0) - 8.0);
            assert_eq!(cam.target(), target, "orbit moved the target");
            let (r, u, b) = (cam.right(), cam.up(), cam.back());
            assert!(approx(dot(r, r), 1.0, 1e-9), "right not unit");
            assert!(approx(dot(u, u), 1.0, 1e-9), "up not unit");
            assert!(approx(dot(b, b), 1.0, 1e-9), "back not unit");
            assert!(approx(dot(r, u), 0.0, 1e-9), "right·up ≠ 0");
            assert!(approx(dot(r, b), 0.0, 1e-9), "right·back ≠ 0");
            assert!(approx(dot(u, b), 0.0, 1e-9), "up·back ≠ 0");
            // Handedness: r × u = b
            let cross_ru = cross(r, u);
            for i in 0..3 {
                assert!(approx(cross_ru[i], b[i], 1e-9), "basis flipped");
            }
        }
    }

    #[test]
    fn orbit_from_top_yaw_keeps_back_axis() {
        // Yaw is about world +Z; from the Top view (back = +Z) yaw must
        // spin the screen axes in place and leave the view direction
        // untouched.
        let mut cam = Camera::framing([0.0; 3], 5.0, View::Top);
        let before = cam.back();
        cam.orbit(120.0, 0.0);
        for (after, was) in cam.back().iter().zip(before.iter()) {
            assert!(approx(*after, *was, 1e-12));
        }
        // 120 px at 0.5°/px = 60°: right must rotate by -60° about Z.
        let right = cam.right();
        let expected = [
            (60.0_f64).to_radians().cos(),
            -(60.0_f64).to_radians().sin(),
            0.0,
        ];
        for i in 0..3 {
            assert!(approx(right[i], expected[i], 1e-12));
        }
    }

    #[test]
    fn zoom_is_exponential_in_ticks() {
        let mut cam = Camera::framing([0.0; 3], 10.0, View::Iso);
        let d0 = cam.distance();
        cam.zoom(1.0);
        assert!(approx(cam.distance(), d0 / ZOOM_BASE, 1e-9));
        cam.zoom(1.0);
        cam.zoom(1.0);
        assert!(approx(cam.distance(), d0 / ZOOM_BASE.powi(3), 1e-9));
        // Fractional ticks follow the same law.
        let d3 = cam.distance();
        cam.zoom(0.5);
        assert!(approx(cam.distance(), d3 / ZOOM_BASE.powf(0.5), 1e-9));
        // Negative ticks zoom out; +n then −n returns (within the clamp).
        cam.zoom(-2.5);
        assert!(approx(cam.distance(), d3 / ZOOM_BASE.powf(-2.0), 1e-9));
        // Extent tracks the distance (the visible exponential).
        assert!(approx(cam.extent(), cam.distance() * half_fov_tan(), 1e-12));
    }

    #[test]
    fn zoom_clamps_instead_of_degenerating() {
        let mut cam = Camera::framing([0.0; 3], 10.0, View::Top);
        for _ in 0..200 {
            cam.zoom(100.0);
        }
        assert!(cam.distance() >= cam.min_distance());
        assert!(cam.distance().is_finite() && cam.distance() > 0.0);
        for _ in 0..200 {
            cam.zoom(-100.0);
        }
        assert!(cam.distance() <= cam.max_distance());
        assert!(cam.distance().is_finite());
    }

    #[test]
    fn pan_translates_target_parallel_to_screen() {
        let mut cam = Camera::framing([1.0, 1.0, 1.0], 5.0, View::Iso);
        let back = cam.back();
        cam.pan(80.0, -40.0, 600.0);
        let delta = [
            cam.target()[0] - 1.0,
            cam.target()[1] - 1.0,
            cam.target()[2] - 1.0,
        ];
        // Parallel to the screen plane: no component along the view axis.
        assert!(approx(dot(delta, back), 0.0, 1e-12));
        // Right/down-drag directions: pan(+x, 0) moves the target along
        // −right; pan(0, +y) along +up.
        let mut cam2 = Camera::framing([0.0; 3], 5.0, View::Front);
        let right = cam2.right();
        cam2.pan(10.0, 0.0, 100.0);
        let wpp = 2.0 * cam2.extent() / 100.0;
        assert!(approx(cam2.target()[0], -right[0] * 10.0 * wpp, 1e-12));
        assert!(approx(cam2.target()[1], -right[1] * 10.0 * wpp, 1e-12));
        assert!(approx(cam2.target()[2], -right[2] * 10.0 * wpp, 1e-12));
    }

    #[test]
    fn pan_scales_with_distance() {
        // Doubling the distance doubles the world-units-per-pixel.
        let mut near = Camera::framing([0.0; 3], 5.0, View::Iso);
        let mut far = near;
        far.zoom(-1.0); // distance × ZOOM_BASE
        near.pan(50.0, 0.0, 500.0);
        far.pan(50.0, 0.0, 500.0);
        let ratio = (far.target()[0] - near.target()[0]) / near.target()[0];
        assert!(
            approx(ratio.abs(), ZOOM_BASE - 1.0, 1e-9),
            "pan is not proportional to the distance"
        );
    }

    #[test]
    fn camera_point_and_eye_agree_with_basis() {
        let cam = Camera::framing([2.0, -3.0, 4.0], 6.0, View::Iso);
        // The target sits at camera (-0, -0, -distance): dead center,
        // one distance away from the eye.
        let c = cam.camera_point(cam.target());
        assert!(approx(c[0], 0.0, 1e-9));
        assert!(approx(c[1], 0.0, 1e-9));
        assert!(approx(c[2], -cam.distance(), 1e-9));
        // A point one `back`-length *deeper* than the target (away from
        // the eye) is one unit further.
        let further = [
            cam.target()[0] - 1.0 * cam.back()[0],
            cam.target()[1] - 1.0 * cam.back()[1],
            cam.target()[2] - 1.0 * cam.back()[2],
        ];
        assert!(approx(
            cam.camera_point(further)[2],
            -cam.distance() - 1.0,
            1e-9
        ));
    }

    #[test]
    fn screen_pixel_centers_the_target_and_matches_projection() {
        let cam = Camera::framing([1.0, 2.0, 3.0], 7.0, View::Iso);
        let (w, h) = (640.0, 400.0);
        let px = cam.screen_pixel(cam.target(), w, h);
        assert!(approx(px[0], w / 2.0, 1e-9));
        assert!(approx(px[1], h / 2.0, 1e-9));
        // A point at +extent in camera-up lands on the top edge.
        let top = [
            cam.target()[0] + cam.extent() * cam.up()[0],
            cam.target()[1] + cam.extent() * cam.up()[1],
            cam.target()[2] + cam.extent() * cam.up()[2],
        ];
        let px = cam.screen_pixel(top, w, h);
        assert!(approx(px[1], 0.0, 1e-9), "top edge: {:?}", px);
        // view_projection agrees: NDC = 2·px/size − 1 (y up, top-left
        // pixel grid) — evaluated on the *world* point.
        let a = cam.view_projection(w / h);
        let world = top;
        let clip = [
            a[0][0] * world[0] + a[0][1] * world[1] + a[0][2] * world[2] + a[0][3],
            a[1][0] * world[0] + a[1][1] * world[1] + a[1][2] * world[2] + a[1][3],
        ];
        assert!(approx(clip[0], 2.0 * px[0] / w - 1.0, 1e-9));
        assert!(approx(clip[1], 1.0 - 2.0 * px[1] / h, 1e-9));
    }

    #[test]
    fn headlight_matches_svg_light_in_camera_space() {
        // The headlight is fixed in camera space: shading is
        // view-independent in the camera's own frame.
        let expected = normalized([0.35, 0.35, 0.87]);
        for view in [View::Iso, View::Top, View::Front, View::Right] {
            let cam = Camera::framing([0.0; 3], 5.0, view);
            let light = cam.headlight_world();
            let m = cam.world_to_camera().m;
            let camera_light = [
                light[0] * m[0][0] + light[1] * m[1][0] + light[2] * m[2][0],
                light[0] * m[0][1] + light[1] * m[1][1] + light[2] * m[2][1],
                light[0] * m[0][2] + light[1] * m[1][2] + light[2] * m[2][2],
            ];
            for i in 0..3 {
                assert!(
                    approx(camera_light[i], expected[i], 1e-12),
                    "{view:?} headlight drifted from svg.rs's light"
                );
            }
        }
    }
}
