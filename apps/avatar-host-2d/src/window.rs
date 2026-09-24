//! P1 native host. Compositing and coarse input originate in the validated P0 probe.
use anyhow::{Context, Result, bail};
use mocari::{
    assets::{RuntimeModel, load_model_runtime},
    core::Matrix44,
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

pub fn run(entry: &Path) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    // A separate lease survives a stalled native event loop or blocked loader.
    let epoch = Instant::now();
    let last = Arc::new(AtomicU64::new(0));
    let lease = last.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(250));
            if epoch.elapsed().as_millis() as u64 > lease.load(Ordering::Acquire) + 8000 {
                eprintln!("avatar-host-2d: parent lease expired");
                std::process::exit(1);
            }
        }
    });
    let pack = if entry.file_name().is_some_and(|n| n == "manifest.json") {
        Some(avatar_pack::open(entry)?)
    } else {
        None
    };
    let entry = if let Some(pack) = &pack {
        pack.entry.clone()
    } else {
        crate::audit::preflight(entry)?.0
    };
    let mut builder = EventLoop::<Control>::with_user_event();
    #[cfg(target_os = "macos")]
    builder
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_activate_ignoring_other_apps(false);
    let event_loop = builder.build()?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let result = pet_ipc::server::serve_with(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
            |command| {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                proxy
                    .send_event(Control::Command(command, tx))
                    .map_err(|_| anyhow::anyhow!("render loop closed"))?;
                let value = rx
                    .recv_timeout(Duration::from_secs(4))
                    .context("render loop timeout")?
                    .map_err(anyhow::Error::msg)?;
                last.store(epoch.elapsed().as_millis() as u64, Ordering::Release);
                Ok(value)
            },
        );
        let _ = proxy.send_event(Control::Stop(result.map_err(|e| format!("{e:#}"))));
    });
    let mut app = AvatarHost {
        entry,
        pack,
        state: None,
        error: None,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    Ok(())
}

#[derive(Debug)]
enum Control {
    Command(
        pet_ipc::server::Command,
        std::sync::mpsc::SyncSender<Result<serde_json::Value, String>>,
    ),
    Stop(Result<(), String>),
}

struct AvatarHost {
    entry: PathBuf,
    pack: Option<avatar_pack::Pack>,
    state: Option<State>,
    error: Option<anyhow::Error>,
}
impl AvatarHost {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}
impl ApplicationHandler<Control> for AvatarHost {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Control) {
        use pet_ipc::server::Command;
        use pet_protocol::DesktopCommand;
        match event {
            Control::Command(command, reply) => {
                let result = (|| -> Result<serde_json::Value> {
                    let state = self.state.as_mut().context("renderer not ready")?;
                    match command {
                        Command::Hello => {
                            return Ok(
                                json!({"avatar":state.pack.as_ref().map(|p|p.manifest.capabilities()).unwrap_or_default(),"ax_trusted":crate::platform::ax_trusted()}),
                            );
                        }
                        Command::Ping => {}
                        Command::Poll => {
                            return Ok(json!({"events":std::mem::take(&mut state.events)}));
                        }
                        Command::Avatar(command) => state.avatar(command)?,
                        Command::Desktop(DesktopCommand::SetScale(scale)) => {
                            state.set_scale(scale)?
                        }
                        Command::Shutdown => state.set_visible(false)?,
                        Command::Desktop(DesktopCommand::SetVisible(visible)) => {
                            state.set_visible(visible)?
                        }
                        Command::Desktop(DesktopCommand::Detach) => state.detach_external(),
                        Command::Desktop(DesktopCommand::SetExternalSnapEnabled(enabled)) => {
                            state.set_external_enabled(enabled)?
                        }
                        Command::Desktop(_) => bail!("capability unavailable in P1-02"),
                    }
                    Ok(json!({}))
                })();
                let _ = reply.send(result.map_err(|e| format!("{e:#}")));
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
            .with_title("DesktopPet")
            .with_inner_size(LogicalSize::new(500., 600.))
            .with_decorations(false)
            .with_transparent(true)
            .with_active(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop);
        #[cfg(target_os = "macos")]
        let attributes = attributes
            .with_has_shadow(false)
            .with_accepts_first_mouse(true);
        let result = (|| {
            let window = Arc::new(event_loop.create_window(attributes)?);
            pollster::block_on(State::new(window, &self.entry, self.pack.clone()))
        })();
        match result {
            Ok(state) => self.state = Some(state),
            Err(error) => self.fail(event_loop, error),
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &mut self.state else {
            return;
        };
        if Instant::now() >= state.next_desktop_check {
            state.maintain_desktop();
            state.next_desktop_check = Instant::now() + Duration::from_secs(1);
        }
        if !state.visible {
            event_loop.set_control_flow(ControlFlow::WaitUntil(state.next_desktop_check));
            return;
        }
        let now = Instant::now();
        if now >= state.next_external_check {
            state.maintain_external();
            state.next_external_check = now + Duration::from_millis(250);
        }
        if let Err(error) = state.sample_input() {
            self.fail(event_loop, error);
            return;
        }
        if now >= state.next_frame {
            state.window.request_redraw();
            state.next_frame += Duration::from_secs_f64(1. / 30.);
            if state.next_frame <= now {
                state.next_frame = now + Duration::from_secs_f64(1. / 30.);
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            state.next_frame.min(state.next_external_check).min(
                now + Duration::from_millis(if state.pointer_near || state.input.dragging {
                    16
                } else {
                    33
                }),
            ),
        ));
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        let result = match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                Ok(())
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                state.config.width = size.width;
                state.config.height = size.height;
                state.surface.configure(&state.device, &state.config);
                state.composite =
                    crate::composite::Composite::new(&state.device, &state.config, state.backend);
                state.transform.update_matrix(
                    &state.queue,
                    &fit_bounds(state.neutral_bounds, size.width, size.height),
                );
                if state.restored && !state.input.dragging {
                    state.restore_placement();
                }
                Ok(())
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                state.next_desktop_check = Instant::now();
                Ok(())
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if state.visible => {
                state.begin_press();
                state.input.press();
                // Failed OS drag initiation must not crash the whole character.
                if let Err(error) = state.window.drag_window() {
                    eprintln!("drag: {error}");
                }
                Ok(())
            }
            WindowEvent::RedrawRequested if state.visible => state.render(),
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }
}

struct State {
    ax_probe: crate::ax::Probe,
    external_snap: crate::external_snap::ExternalSnap,
    external_enabled: bool,
    next_external_check: Instant,
    metrics: crate::metrics::Metrics,
    animation: crate::animation::Animation,
    gaze_target: [f32; 2],
    layout_path: Option<PathBuf>,
    placement: Option<crate::placement::Saved>,
    screens: Vec<crate::placement::Screen>,
    next_desktop_check: Instant,
    restored: bool,
    pointer_near: bool,
    pack: Option<avatar_pack::Pack>,
    feedbacks: Vec<(
        pet_protocol::Feedback,
        mocari::expression::ExpressionPlayer,
        u32,
    )>,
    active: Option<(mocari::expression::ExpressionPlayer, u32)>,
    last_animation: Instant,
    pose_dirty: bool,
    events: Vec<pet_protocol::AvatarEvent>,
    pressed: Option<(pet_protocol::HitRegion, [f64; 2], Instant)>,
    neutral_bounds: [f32; 4],
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
    next_frame: Instant,
    input: crate::input::Input,
    dynamic_input: bool,
    visible: bool,
}

impl State {
    async fn new(
        window: Arc<Window>,
        entry: &Path,
        pack: Option<avatar_pack::Pack>,
    ) -> Result<Self> {
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
        pet_ipc::event_log!(
            "{}",
            json!({"event":"alpha_output", "backend":format!("{backend:?}"), "straight_alpha":crate::composite::straight_output(backend, config.alpha_mode)})
        );
        let renderer = WgpuLive2dRenderer::new(&device, config.format);
        let mut model = if let Some(pack) = &pack {
            pack.load_model()?
        } else {
            load_model_runtime(entry)?
        };
        let mut feedbacks = Vec::new();
        if let Some(pack) = &pack {
            for feedback in [
                pet_protocol::Feedback::HeadPat,
                pet_protocol::Feedback::BodyTap,
            ] {
                if let Some(action) = pack.manifest.action(feedback) {
                    feedbacks.push((
                        feedback,
                        mocari::expression::ExpressionPlayer::new(
                            mocari::expression::load_expression(avatar_pack::checked_path(
                                &pack.root,
                                &action.expression,
                            )?)?,
                        ),
                        action.duration_ms,
                    ));
                }
            }
        }
        model.runtime_mut().reset_parameters();
        model
            .runtime_mut()
            .update_meshes()
            .context("neutral mesh update failed")?;
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
        let anchor_ratio = pack
            .as_ref()
            .map(|p| p.manifest.interaction.anchor[1])
            .unwrap_or((0.5 + (bounds[3] - bounds[1]) * sy * 0.25) as f64);
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
        let dynamic_input = cfg!(target_os = "macos");
        let passthrough = true;
        window.set_cursor_hittest(!passthrough)?;
        pet_ipc::event_log!(
            "{}",
            json!({"event": "ready", "adapter": format!("{:?}", adapter.get_info()), "alpha_modes": format!("{:?}", caps.alpha_modes), "selected_alpha": format!("{:?}", config.alpha_mode), "format": format!("{:?}", config.format), "initialization_ms": started.elapsed().as_secs_f64() * 1000., "monitors": monitors, "passthrough": passthrough, "ax_trusted": crate::platform::ax_trusted(), "clipping_contexts": plan.contexts().len()})
        );
        let layout_path = std::env::var_os("DESKTOPPET_LAYOUT").map(PathBuf::from);
        let placement = layout_path
            .as_ref()
            .and_then(|p| avatar_pack::read_json::<crate::placement::Saved>(p).ok())
            .filter(|s| s.valid());
        let screens = crate::platform::desktop(&window)
            .map(|(_, screens, _)| screens)
            .unwrap_or_default();
        Ok(Self {
            ax_probe: crate::ax::Probe::new(),
            external_snap: Default::default(),
            external_enabled: false,
            next_external_check: Instant::now(),
            metrics: Default::default(),
            animation: Default::default(),
            gaze_target: [0.0; 2],
            layout_path,
            placement,
            screens,
            next_desktop_check: Instant::now(),
            restored: false,
            pointer_near: false,
            pack,
            feedbacks,
            active: None,
            last_animation: Instant::now(),
            pose_dirty: false,
            events: Vec::new(),
            pressed: None,
            neutral_bounds: bounds,
            snap_enabled: cfg!(target_os = "macos"),
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
            next_frame: Instant::now(),
            input: crate::input::Input::default(),
            dynamic_input,
            visible: false,
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
            self.gaze_target = [
                ((x / size.width - 0.5) * 2.0).clamp(-1.0, 1.0) as f32,
                ((0.3 - y / size.height) * 2.0).clamp(-1.0, 1.0) as f32,
            ];
            self.pointer_near =
                x >= -100.0 && y >= -100.0 && x <= size.width + 100.0 && y <= size.height + 100.0;
            let previous = self.input.receiving;
            let receiving = if let Some(pack) = &self.pack {
                self.input.sample_region(
                    pack.manifest.interaction.near(
                        [x / size.width, y / size.height],
                        [size.width, size.height],
                        if previous { 6.0 } else { 0.0 },
                    ),
                    down,
                )
            } else {
                self.input.sample(x, y, size.width, size.height, down)
            };
            if was_dragging
                && !self.input.dragging
                && let Some((region, origin, started)) = self.pressed.take()
            {
                let (current, _, _) = crate::platform::desktop(&self.window)?;
                let unmoved =
                    (current.x - origin[0]).abs() <= 4.0 && (current.y - origin[1]).abs() <= 4.0;
                let still_hit = self.pack.as_ref().and_then(|p| {
                    p.manifest
                        .interaction
                        .hit([x / size.width, y / size.height])
                }) == Some(region);
                if unmoved
                    && still_hit
                    && started.elapsed() < Duration::from_secs(1)
                    && self.events.len() < 32
                {
                    self.events.push(pet_protocol::AvatarEvent::Hit(region));
                }
            }
            if previous != receiving {
                self.window.set_cursor_hittest(receiving)?;
                pet_ipc::event_log!(
                    "{}",
                    json!({"event":"hit_region", "receiving":receiving,"point":[x,y],"left_down":down})
                );
            }
            if self.snap_enabled && was_dragging && !self.input.dragging {
                let mut attached = false;
                if self.external_enabled
                    && let Some(target) = self.ax_probe.latest()
                    && let Ok((window, _, _)) = crate::platform::desktop(&self.window)
                    && let Some(destination) = self.external_snap.release(
                        window,
                        self.anchor_ratio,
                        crate::platform::ax_trusted() == Some(true),
                        std::process::id() as i32,
                        &[target],
                    )
                {
                    match crate::platform::move_to(&self.window, destination) {
                        Ok(()) => {
                            attached = true;
                            pet_ipc::event_log!(
                                "{}",
                                json!({"event":"external_attached","pid":target.pid,"window":target.window})
                            );
                        }
                        Err(error) => {
                            self.external_snap.detach();
                            eprintln!("external snap move: {error:#}");
                        }
                    }
                }
                if !attached
                    && let Err(error) = crate::platform::snap_floor(&self.window, self.anchor_ratio)
                {
                    eprintln!("screen snap unavailable: {error:#}");
                }
                self.remember_placement();
            }
        }
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        let now = Instant::now();
        let delta = now
            .duration_since(self.last_animation)
            .as_secs_f32()
            .min(0.1);
        self.last_animation = now;
        let procedural = self.pack.as_ref().is_some_and(|p| {
            [
                "blink_left",
                "blink_right",
                "gaze_x",
                "gaze_y",
                "head_x",
                "head_y",
            ]
            .iter()
            .any(|key| p.manifest.parameter_map.contains_key(*key))
        });
        let pose = self
            .animation
            .tick(delta, self.gaze_target, self.input.dragging);
        if self.active.is_some() || self.pose_dirty || procedural {
            let runtime = self.model.runtime_mut();
            runtime.reset_parameters();
            if let Some(pack) = &self.pack {
                for (key, value, amplitude) in [
                    ("gaze_x", pose.gaze[0], 0.8),
                    ("gaze_y", pose.gaze[1], 0.8),
                    ("head_x", pose.gaze[0], 0.25),
                    ("head_y", pose.gaze[1], 0.25),
                ] {
                    if let Some(id) = pack.manifest.parameter_map.get(key)
                        && let Some(info) = runtime.parameter_info(id)
                    {
                        let center = info.default();
                        let value = center
                            + value
                                * amplitude
                                * if value >= 0.0 {
                                    info.maximum() - center
                                } else {
                                    center - info.minimum()
                                };
                        runtime.set_parameter(id, value);
                    }
                }
            }
            if let Some((player, duration)) = &mut self.active {
                player.tick(delta);
                if player.time() * 1000.0 >= *duration as f32 && !player.is_fading_out() {
                    player.start_fade_out();
                }
                if player.is_finished() {
                    self.active = None;
                } else {
                    player.apply(runtime);
                }
            }
            if let Some(pack) = &self.pack {
                for key in ["blink_left", "blink_right"] {
                    if let Some(id) = pack.manifest.parameter_map.get(key) {
                        let owned = self.active.as_ref().is_some_and(|(p, _)| {
                            p.expression()
                                .parameters()
                                .iter()
                                .any(|parameter| parameter.id() == id)
                        });
                        if !owned && let Some(info) = runtime.parameter_info(id) {
                            let value =
                                info.minimum() + (info.default() - info.minimum()) * pose.eye_open;
                            runtime.set_parameter(id, value);
                        }
                    }
                }
            }
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
            self.pose_dirty = false;
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
        self.metrics.presented(now);
        Ok(())
    }
    fn begin_press(&mut self) {
        self.detach_external();
        self.active = None;
        self.pose_dirty = true;
        self.events.clear();
        if let (Some(pack), Some((x, y, _)), Ok(origin)) = (
            &self.pack,
            crate::platform::pointer(&self.window),
            crate::platform::desktop(&self.window),
        ) {
            let size = self
                .window
                .inner_size()
                .to_logical::<f64>(self.window.scale_factor());
            self.pressed = pack
                .manifest
                .interaction
                .hit([x / size.width, y / size.height])
                .map(|r| (r, [origin.0.x, origin.0.y], Instant::now()));
        }
    }
    fn avatar(&mut self, command: pet_protocol::AvatarCommand) -> Result<()> {
        match command {
            pet_protocol::AvatarCommand::CancelFeedback => {
                self.active = None;
                self.pose_dirty = true;
            }
            pet_protocol::AvatarCommand::PlayFeedback(feedback) => {
                if !self.visible || self.input.dragging {
                    return Ok(());
                }
                let (_, player, duration) = self
                    .feedbacks
                    .iter()
                    .find(|(f, _, _)| *f == feedback)
                    .context("feedback unavailable")?;
                let mut player = player.clone();
                player.restart();
                self.active = Some((player, *duration));
                self.last_animation = Instant::now();
                pet_ipc::event_log!(
                    "{}",
                    json!({"event":"feedback_started","feedback":feedback})
                );
            }
        }
        Ok(())
    }
    fn set_scale(&mut self, scale: u16) -> Result<()> {
        anyhow::ensure!((50..=150).contains(&scale), "scale must be 50–150 percent");
        self.pressed = None;
        self.events.clear();
        self.input = Default::default();
        self.window.set_cursor_hittest(false)?;
        let _ = self.window.request_inner_size(LogicalSize::new(
            5.0 * f64::from(scale),
            6.0 * f64::from(scale),
        ));
        Ok(())
    }
    fn restore_placement(&mut self) {
        if let Some(saved) = &self.placement
            && let Ok((window, screens, _)) = crate::platform::desktop(&self.window)
            && let Some(target) =
                saved.restore(&screens, [window.width, window.height], self.anchor_ratio)
        {
            match crate::platform::move_to(&self.window, target) {
                Err(error) => eprintln!("position restore: {error:#}"),
                Ok(()) => {
                    if let Ok((actual, _, monitor)) = crate::platform::desktop(&self.window) {
                        pet_ipc::event_log!(
                            "{}",
                            json!({"event":"position_restored","monitor":monitor,"requested":[target.x,target.y],"actual":[actual.x,actual.y]})
                        );
                    }
                }
            }
        }
    }
    fn remember_placement(&mut self) {
        if let Ok((window, screens, id)) = crate::platform::desktop(&self.window)
            && let Some(screen) = screens.iter().find(|s| s.id == id)
        {
            let saved = crate::placement::Saved::capture(window, screen, self.anchor_ratio);
            if self.placement.as_ref() == Some(&saved) {
                return;
            }
            self.placement = Some(saved.clone());
            if let Some(path) = &self.layout_path {
                let result = (|| -> Result<()> {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
                    std::fs::write(&temp, serde_json::to_vec_pretty(&saved)?)?;
                    std::fs::rename(temp, path)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    eprintln!("position save: {error:#}");
                }
            }
        }
    }
    fn maintain_desktop(&mut self) {
        if self.input.dragging {
            return;
        }
        if let Ok((window, screens, id)) = crate::platform::desktop(&self.window)
            && screens != self.screens
        {
            self.screens = screens;
            if self.placement.is_none()
                && let Some(screen) = self.screens.iter().find(|s| s.id == id)
            {
                self.placement = Some(crate::placement::Saved::capture(
                    window,
                    screen,
                    self.anchor_ratio,
                ));
            }
            self.restore_placement();
            if self.restored {
                self.remember_placement();
            }
        }
    }
    fn detach_external(&mut self) {
        if self.external_snap.detach() {
            pet_ipc::event_log!("{}", json!({"event":"external_detached"}));
        }
    }
    fn fall_to_screen(&mut self) {
        if let Ok((window, screens, id)) = crate::platform::desktop(&self.window)
            && let Some(screen) = screens
                .iter()
                .find(|screen| screen.id == id)
                .or(screens.first())
        {
            let work = screen.work;
            let target = crate::snap::Rect {
                x: window
                    .x
                    .clamp(work.x, work.x + (work.width - window.width).max(0.0)),
                y: work.y + work.height - window.height * self.anchor_ratio,
                ..window
            };
            if let Err(error) = crate::platform::move_to(&self.window, target) {
                eprintln!("screen fallback: {error:#}");
            }
        }
        self.remember_placement();
    }
    fn set_external_enabled(&mut self, enabled: bool) -> Result<()> {
        if enabled {
            anyhow::ensure!(
                crate::platform::ax_trusted() == Some(true),
                "Accessibility permission is required"
            );
        } else {
            let was_attached = self.external_snap.attached();
            self.detach_external();
            if was_attached && self.visible {
                self.fall_to_screen();
            }
        }
        self.external_enabled = enabled;
        self.ax_probe.set_enabled(enabled && self.visible);
        Ok(())
    }
    fn maintain_external(&mut self) {
        if !self.external_snap.attached() || self.input.dragging || !self.visible {
            return;
        }
        let trusted = crate::platform::ax_trusted() == Some(true);
        let current = crate::platform::desktop(&self.window)
            .ok()
            .map(|result| result.0);
        let destination = current.and_then(|window| {
            self.external_snap
                .follow(window, self.anchor_ratio, trusted, self.ax_probe.latest())
        });
        match destination {
            Some(destination) => {
                if current.is_some_and(|current| {
                    (current.x - destination.x).abs() < 0.5
                        && (current.y - destination.y).abs() < 0.5
                }) {
                    return;
                }
                if let Err(error) = crate::platform::move_to(&self.window, destination) {
                    eprintln!("external follow: {error:#}");
                    self.detach_external();
                }
            }
            None => {
                self.detach_external();
                self.fall_to_screen();
            }
        }
    }
    fn set_visible(&mut self, visible: bool) -> Result<()> {
        if !visible {
            let was_attached = self.external_snap.attached();
            self.detach_external();
            if was_attached {
                self.remember_placement();
            }
        }
        self.ax_probe.set_enabled(visible && self.external_enabled);
        self.pressed = None;
        self.events.clear();
        self.active = None;
        self.pose_dirty = true;
        self.window.set_cursor_hittest(false)?;
        self.input = crate::input::Input::default();
        if !self.restored {
            self.restore_placement();
            self.restored = true;
        }
        self.last_animation = Instant::now();
        self.metrics.pause();
        self.visible = visible;
        self.window.set_visible(visible);
        self.next_frame = Instant::now();
        pet_ipc::event_log!(
            "{}",
            json!({"event":"visibility_applied","visible":visible})
        );
        Ok(())
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let report = self.metrics.report();
        pet_ipc::event_log!(
            "{}",
            serde_json::json!({"event":"render_metrics","metrics":report})
        );
        if report["presented_frames"].as_u64().unwrap_or(0) > 0
            && let Some(path) = &self.layout_path
        {
            let path = path.with_file_name("render-metrics.json");
            if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
                let _ = std::fs::write(path, bytes);
            }
        }
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
    fit_bounds(bounds, width, height)
}
fn fit_bounds(bounds: [f32; 4], width: u32, height: u32) -> Matrix44 {
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
