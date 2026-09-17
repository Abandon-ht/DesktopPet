//! Original two-window native alpha calibration. No model files required.
#[path = "../composite.rs"]
mod composite;
#[path = "../calibration_fixture.rs"]
mod fixture;

use anyhow::{Context, Result, bail, ensure};
use mocari::{
    moc3::{Moc3DrawableMesh, Moc3DrawableVertex},
    render::wgpu::{WgpuLive2dRenderer, WgpuMeshBuffers, WgpuTexture},
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(target_os = "macos")]
use winit::platform::macos::{WindowAttributesExtMacOS, WindowExtMacOS};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("alpha-calibration: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let seconds = std::env::args()
        .nth(1)
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(300);
    ensure!(
        (1..=3600).contains(&seconds),
        "duration must be 1–3600 seconds"
    );
    let event_loop = EventLoop::new()?;
    let mut app = Calibration {
        state: None,
        error: None,
        duration: Duration::from_secs(seconds),
    };
    event_loop.run_app(&mut app)?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok(())
}

struct Calibration {
    state: Option<Pair>,
    error: Option<anyhow::Error>,
    duration: Duration,
}
struct Pair {
    background: Surface,
    overlay: Surface,
    straight: bool,
    shadow: bool,
    start: Instant,
}

impl Calibration {
    fn fail(&mut self, event_loop: &ActiveEventLoop, e: anyhow::Error) {
        self.error = Some(e);
        event_loop.exit();
    }
}

impl ApplicationHandler for Calibration {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let result = (|| -> Result<Pair> {
            let background_attr = Window::default_attributes()
                .with_title("DesktopPet Alpha Calibration")
                .with_inner_size(LogicalSize::new(fixture::WIDTH, fixture::HEIGHT))
                .with_resizable(false)
                .with_visible(false);
            let overlay_attr = Window::default_attributes()
                .with_title("DesktopPet Alpha Overlay")
                .with_inner_size(LogicalSize::new(fixture::OVERLAY[0], fixture::OVERLAY[1]))
                .with_resizable(false)
                .with_decorations(false)
                .with_transparent(true)
                .with_active(false)
                .with_visible(false);
            #[cfg(target_os = "macos")]
            let (background_attr, overlay_attr) = (
                background_attr.with_has_shadow(false),
                overlay_attr.with_has_shadow(false),
            );
            let background = Arc::new(event_loop.create_window(background_attr)?);
            let overlay = Arc::new(event_loop.create_window(overlay_attr)?);
            overlay.set_cursor_hittest(false)?;
            let (backdrop, foreground) = fixture::images();
            let output = std::env::var_os("P0_CALIBRATION_OUTPUT");
            if let Some(output) = output {
                let output = std::path::PathBuf::from(output);
                std::fs::create_dir_all(&output)?;
                backdrop.save(output.join("fixture-background.png"))?;
                foreground.save(output.join("fixture-overlay.png"))?;
            }
            let background_gpu =
                pollster::block_on(Surface::new(background.clone(), &backdrop, false))?;
            let overlay_gpu = pollster::block_on(Surface::new(overlay.clone(), &foreground, true))?;
            // Main window is a normal app window so the test can be controlled with
            // public accessibility tools. Only the child simulates the pet overlay.
            background.set_visible(true);
            align(&background, &overlay)?;
            attach_child(&background, &overlay)?;
            overlay.set_visible(true);
            let straight = overlay_gpu.straight;
            let pair = Pair {
                background: background_gpu,
                overlay: overlay_gpu,
                straight,
                shadow: false,
                start: Instant::now(),
            };
            pair.title();
            pair.redraw();
            Ok(pair)
        })();
        match result {
            Ok(pair) => self.state = Some(pair),
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(pair) = &self.state {
            let deadline = pair.start + self.duration;
            if Instant::now() >= deadline {
                event_loop.exit();
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(pair) = &mut self.state else {
            return;
        };
        let result = (|| -> Result<()> {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Moved(_) if id == pair.background.window.id() => {
                    align(&pair.background.window, &pair.overlay.window)?;
                }
                WindowEvent::RedrawRequested => {
                    if id == pair.background.window.id() {
                        pair.background.draw()?;
                    } else {
                        pair.overlay.draw()?;
                    }
                }
                WindowEvent::Resized(_) => {
                    if id == pair.background.window.id() {
                        pair.background.resize()?;
                    } else {
                        pair.overlay.resize()?;
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed =>
                {
                    match event.logical_key {
                        Key::Named(NamedKey::Escape) => event_loop.exit(),
                        Key::Character(key) if key == "1" || key == "2" => {
                            pair.straight = key == "1";
                            pair.overlay.straight = pair.straight;
                            pair.overlay.composite = composite::Composite::with_straight_alpha(
                                &pair.overlay.device,
                                &pair.overlay.config,
                                pair.straight,
                            );
                            pair.title();
                            pair.redraw();
                        }
                        Key::Character(key) if key.eq_ignore_ascii_case("s") => {
                            pair.shadow = !pair.shadow;
                            #[cfg(target_os = "macos")]
                            pair.overlay.window.set_has_shadow(pair.shadow);
                            pair.title();
                            pair.redraw();
                        }
                        _ => (),
                    }
                }
                _ => (),
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.fail(event_loop, e);
        }
    }
}

impl Pair {
    fn title(&self) {
        let mode = if self.straight {
            "1 STRAIGHT (unpremultiply)"
        } else {
            "2 PREMULTIPLIED (preserve)"
        };
        self.background.window.set_title(&format!(
            "DesktopPet Alpha | {mode} | shadow={} | 1/2 switch, S shadow, Esc quit",
            self.shadow
        ));
        eprintln!(
            "{}",
            json!({"event":"calibration_case","straight_alpha":self.straight,"shadow":self.shadow,"overlay_origin":format!("{:?}",self.overlay.window.outer_position()),"overlay_size":format!("{:?}",self.overlay.window.inner_size()),"background_origin":format!("{:?}",self.background.window.inner_position())})
        );
    }
    fn redraw(&self) {
        self.background.window.request_redraw();
        self.overlay.window.request_redraw();
    }
}

fn align(background: &Window, overlay: &Window) -> Result<()> {
    let origin = background.inner_position()?;
    let scale = background.scale_factor();
    overlay.set_outer_position(PhysicalPosition::new(
        origin.x + (fixture::OFFSET[0] as f64 * scale) as i32,
        origin.y + (fixture::OFFSET[1] as f64 * scale) as i32,
    ));
    Ok(())
}

#[cfg(target_os = "macos")]
fn attach_child(background: &Window, overlay: &Window) -> Result<()> {
    use objc2_app_kit::{NSView, NSWindowOrderingMode};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let parent = background.window_handle()?.as_raw();
    let child = overlay.window_handle()?.as_raw();
    if let (RawWindowHandle::AppKit(parent), RawWindowHandle::AppKit(child)) = (parent, child) {
        // winit owns both NSViews for the live windows; called only on the event
        // loop's main thread. Retained NSWindows are not kept past their owners.
        unsafe {
            let parent_view = &*parent.ns_view.as_ptr().cast::<NSView>();
            let child_view = &*child.ns_view.as_ptr().cast::<NSView>();
            let parent_window = parent_view.window().context("no parent NSWindow")?;
            let child_window = child_view.window().context("no overlay NSWindow")?;
            parent_window
                .addChildWindow_ordered(&child_window, NSWindowOrderingMode::NSWindowAbove);
        }
    } else {
        bail!("expected AppKit windows");
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn attach_child(_: &Window, _: &Window) -> Result<()> {
    bail!("native calibration currently requires macOS")
}

struct Surface {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: WgpuLive2dRenderer,
    buffers: WgpuMeshBuffers,
    texture: WgpuTexture,
    composite: composite::Composite,
    straight: bool,
}
impl Surface {
    async fn new(window: Arc<Window>, image: &image::RgbaImage, transparent: bool) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter.request_device(&Default::default()).await?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, size.width, size.height)
            .context("surface unsupported")?;
        ensure!(
            caps.formats.contains(&wgpu::TextureFormat::Bgra8Unorm),
            "calibration requires Bgra8Unorm"
        );
        config.format = wgpu::TextureFormat::Bgra8Unorm;
        config.alpha_mode = if transparent {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            wgpu::CompositeAlphaMode::Opaque
        };
        ensure!(
            caps.alpha_modes.contains(&config.alpha_mode),
            "required alpha mode unavailable"
        );
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        let renderer = WgpuLive2dRenderer::new(&device, config.format);
        let texture = renderer.create_rgba8_texture(
            &device,
            &queue,
            image.width(),
            image.height(),
            image.as_raw(),
        )?;
        let mesh = Moc3DrawableMesh::from_parts(
            0,
            0,
            1.,
            0.,
            vec![
                Moc3DrawableVertex::new([-1., 1.], [0., 0.]),
                Moc3DrawableVertex::new([1., 1.], [1., 0.]),
                Moc3DrawableVertex::new([1., -1.], [1., 1.]),
                Moc3DrawableVertex::new([-1., -1.], [0., 1.]),
            ],
            vec![0, 1, 2, 0, 2, 3],
            vec![],
        );
        let buffers = WgpuMeshBuffers::from_drawables(&device, &[mesh]).context("mesh buffer")?;
        let composite = composite::Composite::new(&device, &config, adapter.get_info().backend);
        eprintln!(
            "{}",
            json!({"event":"surface","transparent":transparent,"adapter":format!("{:?}",adapter.get_info()),"alpha":format!("{:?}",config.alpha_mode),"size":[size.width,size.height],"scale":window.scale_factor()})
        );
        let straight = composite::straight_output(adapter.get_info().backend, config.alpha_mode);
        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            buffers,
            texture,
            composite,
            straight,
        })
    }
    fn resize(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.composite =
            composite::Composite::with_straight_alpha(&self.device, &self.config, self.straight);
        self.window.request_redraw();
        Ok(())
    }
    fn draw(&self) -> Result<()> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("surface validation failure"),
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.composite.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer.draw_with_textures(
                &mut pass,
                &self.buffers,
                std::slice::from_ref(&self.texture),
            )?;
        }
        self.composite.draw(
            &mut encoder,
            &frame.texture.create_view(&Default::default()),
        );
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(frame);
        Ok(())
    }
}
