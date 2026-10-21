//! Interactive viewer — the M1 window mode, behind the non-default
//! `interactive` cargo feature.
//!
//! ```sh
//! cargo run -p fc-viewer --features interactive --example viewer -- [FILE]
//! ```
//!
//! Left-drag orbits (target fixed), the wheel zooms (exponential in
//! ticks), right-drag (or middle-drag) pans parallel to the screen
//! plane; `1`–`4` snap to iso/top/front/right, `Esc` quits. Rendering
//! reuses the offscreen pipeline against a surface view.
//!
//! This example is **build-tested only** — CI compiles it (with the
//! feature on) but never runs it: the runner has no display. The camera
//! gestures it exercises are the same pure functions the unit tests
//! verify.

use fc_viewer::camera::{Camera, View};
use fc_viewer::renderer::{create_context, render_to_view, GpuContext};
use fc_viewer::scene::{build_scene, Scene};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

struct WindowState {
    window: Window,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,
    size: (u32, u32),
}

impl WindowState {
    fn new(window: Window, ctx: &GpuContext) -> Self {
        let size = window.inner_size();
        let surface = ctx
            .instance
            .create_surface(window.clone())
            .expect("create surface");
        let caps = surface.get_capabilities(&ctx.adapter);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps
                .formats
                .iter()
                .copied()
                .find(|f| f.is_srgb())
                .unwrap_or(caps.formats[0]),
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&ctx.device, &config);
        let depth = create_depth(&ctx.device, config.width, config.height);
        WindowState {
            window,
            surface,
            config,
            depth,
            size: (config.width, config.height),
        }
    }

    fn resize(&mut self, ctx: &GpuContext) {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        if (w, h) != self.size {
            self.size = (w, h);
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&ctx.device, &self.config);
            self.depth = create_depth(&ctx.device, w, h);
        }
    }
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("viewer depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

struct Viewer {
    ctx: Option<GpuContext>,
    scene: Option<Scene>,
    camera: Camera,
    state: Option<WindowState>,
    drag: Option<(bool, (f64, f64))>, // (pan?, last position)
}

impl Viewer {
    fn new() -> Self {
        // Framing is replaced once the scene loads; this keeps the
        // struct total before `resumed`.
        Self {
            ctx: None,
            scene: None,
            camera: Camera::framing([0.0; 3], 1.0, View::Iso),
            state: None,
            drag: None,
        }
    }

    fn redraw(&mut self) {
        let Some(state) = &self.state else { return };
        let Some(ctx) = &self.ctx else { return };
        let Some(scene) = &self.scene else { return };
        let frame = match state.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(_) => return, // lost the frame; the next redraw retries
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let _ = render_to_view(
            scene,
            &self.camera,
            &view,
            &state.depth,
            ctx,
            state.size.0,
            state.size.1,
        );
        let _ = frame.present();
        state.window.request_redraw();
    }
}

impl ApplicationHandler for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.ctx.is_none() {
            let file = std::env::args()
                .nth(1)
                .unwrap_or_else(|| "examples/demo.fcstd".to_string());
            let scene = build_scene(&file).expect("load document");
            self.camera = Camera::framing(scene.center, scene.radius, View::Iso);
            self.scene = Some(scene);
            self.ctx = Some(create_context().expect("GPU context"));
        }
        let window = event_loop
            .create_window(
                winit::window::Window::default_attributes()
                    .with_title("fc-viewer (M1 interactive)"),
            )
            .expect("create window");
        let state = WindowState::new(window, self.ctx.as_ref().unwrap());
        self.state = Some(state);
        self.state.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) => {
                if let (Some(state), Some(ctx)) = (&mut self.state, &self.ctx) {
                    state.resize(ctx);
                    state.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((pan, last)) = self.drag.take() {
                    let (dx, dy) = (position.x - last.0, position.y - last.1);
                    let viewport_h = self.state.as_ref().map(|s| s.size.1).unwrap_or(800) as f64;
                    if pan {
                        self.camera.pan(dx, dy, viewport_h);
                    } else {
                        self.camera.orbit(dx, dy);
                    }
                    self.drag = Some((pan, (position.x, position.y)));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, b) => {
                    let pan = matches!(b, MouseButton::Right | MouseButton::Middle);
                    if let Some(pos) = self.state.as_ref().and_then(|s| s.window.cursor_position())
                    {
                        self.drag = Some((pan, (pos.x, pos.y)));
                    }
                }
                (ElementState::Released, _) => self.drag = None,
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let ticks = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => lines as f64,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f64 / 40.0,
                };
                self.camera.zoom(ticks);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                        PhysicalKey::Code(KeyCode::Digit1) => {
                            self.camera.set_view(View::Iso);
                        }
                        PhysicalKey::Code(KeyCode::Digit2) => {
                            self.camera.set_view(View::Top);
                        }
                        PhysicalKey::Code(KeyCode::Digit3) => {
                            self.camera.set_view(View::Front);
                        }
                        PhysicalKey::Code(KeyCode::Digit4) => {
                            self.camera.set_view(View::Right);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut viewer = Viewer::new();
    event_loop.run_app(&mut viewer).expect("viewer run");
}
