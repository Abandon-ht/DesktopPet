//! P0 surface probe with standalone and supervised IPC modes.
use anyhow::{Context, Result, bail, ensure};
use mocari::{
    assets::{RuntimeModel, load_model_runtime},
    core::Matrix44,
    expression::{ExpressionPlayer, load_expression},
    render::wgpu::*,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(target_os = "macos")]
use winit::platform::macos::{
    ActivationPolicy, EventLoopBuilderExtMacOS, WindowAttributesExtMacOS,
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId, WindowLevel},
};

pub fn run(entry: &Path, seconds: u64) -> Result<()> {
    run_mode(entry, seconds, false)
}

pub fn run_host(entry: &Path) -> Result<()> {
    run_mode(entry, 3600, true)
}

#[derive(Debug)]
enum Control {
    Command(std::sync::mpsc::SyncSender<()>),
    Stop(std::result::Result<(), String>),
}

fn run_mode(entry: &Path, seconds: u64, ipc: bool) -> Result<()> {
    ensure!(
        (1..=3600).contains(&seconds),
        "duration must be 1–3600 seconds"
    );
    let (entry, _, expressions) = crate::audit::preflight(entry)?;
    let mut builder = EventLoop::<Control>::with_user_event();
    #[cfg(target_os = "macos")]
    builder
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_activate_ignoring_other_apps(false);
    let event_loop = builder.build()?;
    if ipc {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            let result = p0_ipc_probe::server::serve(
                &mut std::io::stdin().lock(),
                &mut std::io::stdout().lock(),
                |_| {
                    let (tx, rx) = std::sync::mpsc::sync_channel(1);
                    proxy
                        .send_event(Control::Command(tx))
                        .map_err(|_| anyhow::anyhow!("render loop closed"))?;
                    rx.recv_timeout(Duration::from_secs(10))
                        .context("render loop response timeout")?;
                    Ok(())
                },
            );
            // shutdown reply is flushed before asking the main thread to exit.
            let _ = proxy.send_event(Control::Stop(result.map_err(|e| format!("{e:#}"))));
        });
    }
    let mut app = Probe {
        entry,
        expressions,
        state: None,
        duration: if ipc {
            None
        } else {
            Some(Duration::from_secs(seconds))
        },
        error: None,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    Ok(())
}

struct Probe {
    entry: PathBuf,
    expressions: Vec<PathBuf>,
    duration: Option<Duration>,
    state: Option<State>,
    error: Option<anyhow::Error>,
}

impl Probe {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<Control> for Probe {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Control) {
        match event {
            Control::Command(reply) => {
                if self.state.is_some() {
                    let _ = reply.send(());
                }
            }
            Control::Stop(result) => {
                if let Err(error) = result {
                    self.error = Some(anyhow::anyhow!(error));
                }
                event_loop.exit();
            }
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("DesktopPet P0 — auto-exit probe")
            .with_inner_size(LogicalSize::new(500., 600.))
            .with_decorations(false)
            .with_transparent(true)
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnTop);
        #[cfg(target_os = "macos")]
        let attributes = attributes
            .with_has_shadow(false)
            .with_accepts_first_mouse(true);
        let result = (|| {
            let window = Arc::new(event_loop.create_window(attributes)?);
            pollster::block_on(State::new(window, &self.entry, &self.expressions))
        })();
        match result {
            Ok(state) => {
                self.state = Some(state);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &mut self.state else {
            return;
        };
        let now = Instant::now();
        if let Err(error) = state.sample_input() {
            self.fail(event_loop, error);
            return;
        }
        if self
            .duration
            .is_some_and(|duration| now.duration_since(state.started) >= duration)
        {
            state.report();
            event_loop.exit();
        } else {
            if now >= state.next_frame {
                state.window.request_redraw();
                state.next_frame = now + Duration::from_secs_f64(1. / 30.);
            }
            let deadline = if state.dynamic_input {
                state.next_frame.min(now + Duration::from_millis(16))
            } else {
                state.next_frame
            };
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        let result = match event {
            WindowEvent::CloseRequested => {
                state.report();
                event_loop.exit();
                Ok(())
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    state.config.width = size.width;
                    state.config.height = size.height;
                    state.surface.configure(&state.device, &state.config);
                    state.composite = crate::composite::Composite::new(
                        &state.device,
                        &state.config,
                        state.backend,
                    );
                    let matrix = fit_matrix(state.model.runtime(), size.width, size.height);
                    state.transform.update_matrix(&state.queue, &matrix);
                }
                Ok(())
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                state.input.press();
                p0_ipc_probe::event_log!("{}", json!({"event": "drag_requested"}));
                state.window.drag_window().map_err(Into::into)
            }
            WindowEvent::Focused(focused) => {
                p0_ipc_probe::event_log!("{}", json!({"event": "focus", "focused": focused}));
                Ok(())
            }
            WindowEvent::RedrawRequested => state.render(),
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }
}

struct State {
    snap_enabled: bool,
    anchor_ratio: f64,
    backend: wgpu::Backend,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: WgpuLive2dRenderer,
    model: RuntimeModel,
    buffers: WgpuMeshBuffers,
    textures: Vec<WgpuTexture>,
    clipping: WgpuClippingResources,
    mask: WgpuMaskRenderTarget,
    transform: WgpuTransform,
    composite: crate::composite::Composite,
    expressions: Vec<PathBuf>,
    phase: usize,
    started: Instant,
    next_frame: Instant,
    timings: Vec<f64>,
    captured: std::collections::HashSet<usize>,
    input: crate::input::Input,
    dynamic_input: bool,
}

impl State {
    async fn new(window: Arc<Window>, entry: &Path, expressions: &[PathBuf]) -> Result<Self> {
        let started = Instant::now();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, size.width, size.height)
            .context("surface unsupported")?;
        config.format = preferred_surface_format(&caps.formats).context("no surface format")?;
        config.alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
        ]
        .into_iter()
        .find(|mode| caps.alpha_modes.contains(mode))
        .context("surface does not support transparent compositing")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);
        let backend = adapter.get_info().backend;
        let composite = crate::composite::Composite::new(&device, &config, backend);
        p0_ipc_probe::event_log!(
            "{}",
            json!({"event":"alpha_output", "backend":format!("{backend:?}"), "straight_alpha":crate::composite::straight_output(backend, config.alpha_mode)})
        );
        let renderer = WgpuLive2dRenderer::new(&device, config.format);
        let model = load_model_runtime(entry)?;
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for vertex in model
            .runtime()
            .meshes()
            .iter()
            .flat_map(|mesh| mesh.vertices())
        {
            let [x, y] = vertex.position();
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
        }
        let aspect = size.width as f32 / size.height.max(1) as f32;
        let sy = (1.85 / ((bounds[2] - bounds[0]).max(0.001) * aspect))
            .min(1.85 / (bounds[3] - bounds[1]).max(0.001));
        // Neutral lower mesh bound, frozen so expression changes do not move
        // the window. This is a P0 anchor, not a semantic foot annotation.
        let anchor_ratio = (0.5 + (bounds[3] - bounds[1]) * sy * 0.25) as f64;
        let textures = model
            .textures()
            .iter()
            .map(|t| {
                renderer.create_rgba8_texture(&device, &queue, t.width(), t.height(), t.rgba())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let buffers = WgpuMeshBuffers::from_drawables(&device, model.runtime().meshes())
            .context("mesh buffer creation failed")?;
        let mut plan = WgpuClippingPlan::from_mesh_buffers(&buffers);
        plan.prepare_single_texture_masks(&buffers)
            .context("clipping layout failed")?;
        let clipping = renderer.create_clipping_resources(&device, &plan)?;
        let mask = renderer.create_mask_render_target(&device, 512)?;
        let transform = renderer.create_transform(
            &device,
            &fit_matrix(model.runtime(), size.width, size.height),
        );
        let monitors: Vec<_> = window.available_monitors().map(|m| json!({"name": m.name(), "position": [m.position().x, m.position().y], "size": [m.size().width, m.size().height], "scale": m.scale_factor()})).collect();
        // A fixed pass-through mode isolates compositing from the later hit-region gate.
        // Set P0_PASSTHROUGH=1 for an entire-window cross-app click experiment.
        let dynamic_input = std::env::var("P0_INPUT").as_deref() == Ok("dynamic");
        let passthrough = dynamic_input || std::env::var("P0_PASSTHROUGH").as_deref() == Ok("1");
        window.set_cursor_hittest(!passthrough)?;
        p0_ipc_probe::event_log!(
            "{}",
            json!({"event": "ready", "adapter": format!("{:?}", adapter.get_info()), "alpha_modes": format!("{:?}", caps.alpha_modes), "selected_alpha": format!("{:?}", config.alpha_mode), "format": format!("{:?}", config.format), "initialization_ms": started.elapsed().as_secs_f64() * 1000., "monitors": monitors, "passthrough": passthrough, "ax_trusted": crate::platform::ax_trusted(), "clipping_contexts": plan.contexts().len()})
        );
        Ok(Self {
            snap_enabled: std::env::var("P0_SNAP").as_deref() == Ok("1"),
            anchor_ratio,
            backend,
            window,
            surface,
            config,
            device,
            queue,
            renderer,
            model,
            buffers,
            textures,
            clipping,
            mask,
            transform,
            composite,
            expressions: expressions.to_vec(),
            phase: usize::MAX,
            started: Instant::now(),
            next_frame: Instant::now(),
            timings: Vec::new(),
            captured: std::collections::HashSet::new(),
            input: crate::input::Input::default(),
            dynamic_input,
        })
    }

    fn sample_input(&mut self) -> Result<()> {
        if self.dynamic_input {
            let was_dragging = self.input.dragging;
            let (x, y, down) = crate::platform::pointer(&self.window)
                .context("global pointer sampling unavailable")?;
            let size = self
                .window
                .inner_size()
                .to_logical::<f64>(self.window.scale_factor());
            let previous = self.input.receiving;
            let receiving = self.input.sample(x, y, size.width, size.height, down);
            if previous != receiving {
                self.window.set_cursor_hittest(receiving)?;
                p0_ipc_probe::event_log!(
                    "{}",
                    json!({"event":"hit_region", "receiving":receiving,"point":[x,y],"left_down":down})
                );
            }
            if self.snap_enabled && was_dragging && !self.input.dragging {
                crate::platform::snap_floor(&self.window, self.anchor_ratio)?;
            }
        }
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        let start = Instant::now();
        // A fresh neutral pose before every case prevents expressions accumulating.
        // 0 neutral; 1–3 eyes/mouth; remaining phases enumerate discovered expressions.
        let phase = (self.started.elapsed().as_secs() / 3) as usize % (4 + self.expressions.len());
        if phase != self.phase {
            let runtime = self.model.runtime_mut();
            runtime.reset_parameters();
            let label = match phase {
                0 => "neutral".to_owned(),
                1 => {
                    runtime.set_parameter("ParamEyeLOpen", 0.);
                    "left-eye-closed".to_owned()
                }
                2 => {
                    runtime.set_parameter("ParamEyeROpen", 0.);
                    "right-eye-closed".to_owned()
                }
                3 => {
                    runtime.set_parameter("ParamMouthOpenY", 1.);
                    "mouth-open".to_owned()
                }
                _ => {
                    let path = &self.expressions[phase - 4];
                    let mut player = ExpressionPlayer::new(load_expression(path)?);
                    player.tick(player.expression().resolved_fade_in_time() + 1.);
                    player.apply(runtime);
                    path.file_name().unwrap().to_string_lossy().into_owned()
                }
            };
            runtime.update_meshes().context("mesh update failed")?;
            self.buffers
                .update_drawables(&self.queue, runtime.meshes())?;
            let mut plan = WgpuClippingPlan::from_mesh_buffers(&self.buffers);
            plan.prepare_single_texture_masks(&self.buffers)?;
            if !self
                .renderer
                .update_clipping_resources(&self.queue, &mut self.clipping, &plan)?
            {
                self.clipping = self
                    .renderer
                    .create_clipping_resources(&self.device, &plan)?;
            }
            self.phase = phase;
            p0_ipc_probe::event_log!(
                "{}",
                json!({"event": "case", "phase": phase, "label": label})
            );
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("surface validation failure"),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.mask.view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer.draw_masks_with_textures(
                &mut pass,
                &self.buffers,
                &self.clipping,
                &self.textures,
            )?;
        }
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
            self.renderer.draw_with_textures_clipping_and_transform(
                &mut pass,
                &self.buffers,
                &self.textures,
                &self.clipping,
                &self.mask,
                &self.transform,
            )?;
        }
        self.composite.draw(&mut encoder, &view);
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(frame);
        self.timings.push(start.elapsed().as_secs_f64() * 1000.);
        if !self.captured.contains(&phase) {
            if let Some(directory) = std::env::var_os("P0_CAPTURE_DIR") {
                let directory = PathBuf::from(directory);
                std::fs::create_dir_all(&directory)?;
                crate::capture::save(
                    &self.device,
                    &self.queue,
                    &self.composite,
                    &self.config,
                    &directory.join(format!("case-{phase:02}.png")),
                )?;
            }
            self.captured.insert(phase);
        }
        Ok(())
    }

    fn report(&mut self) {
        self.timings.sort_by(f64::total_cmp);
        let n = self.timings.len();
        p0_ipc_probe::event_log!(
            "{}",
            json!({"event": "summary", "elapsed_s": self.started.elapsed().as_secs_f64(), "presented_frames": n, "cpu_submit_ms_p95": if n > 0 { Some(self.timings[((n as f64 * 0.95).ceil() as usize - 1).min(n - 1)]) } else { None }, "metric_scope": "CPU render/submit/present call duration, NOT GPU completion or frame interval", "visual_verdict": "manual review required"})
        );
    }
}

fn fit_matrix(runtime: &mocari::ModelRuntime, width: u32, height: u32) -> Matrix44 {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for v in runtime.meshes().iter().flat_map(|m| m.vertices()) {
        let [x, y] = v.position();
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x);
        bounds[3] = bounds[3].max(y);
    }
    let aspect = width as f32 / height.max(1) as f32;
    let sy = (1.85 / ((bounds[2] - bounds[0]).max(0.001) * aspect))
        .min(1.85 / (bounds[3] - bounds[1]).max(0.001));
    let sx = sy / aspect;
    let mut matrix = Matrix44::identity();
    matrix.scale(sx, sy);
    matrix.translate(
        -(bounds[0] + bounds[2]) * 0.5 * sx,
        -(bounds[1] + bounds[3]) * 0.5 * sy,
    );
    matrix
}
