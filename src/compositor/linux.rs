// Adapted from STAR/SCREEN, MIT, Copyright (c) 2026 bstar.
use super::common::*;
use smithay::wayland::seat::WaylandFocus;
// An offscreen Wayland desktop. The host desktop is never a recording source.
#[path = "handlers.rs"]
mod handlers;
#[path = "x11.rs"]
mod x11;

use crate::project::{Launch, Settings, WindowLayout};
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, KeyState},
    backend::{
        allocator::Fourcc,
        egl::{EGLContext, EGLDevice, EGLDisplay},
        renderer::{
            Bind, ExportMem, ImportDma, Offscreen, Renderer as _, TextureFilter, TextureMapping,
            damage::OutputDamageTracker,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::{GlesRenderbuffer, GlesRenderer},
        },
    },
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{
        Seat, SeatState,
        keyboard::{FilterResult, KeyboardTarget},
        pointer::{
            AxisFrame, ButtonEvent, CursorImageStatus, CursorImageSurfaceData, MotionEvent,
            RelativeMotionEvent,
        },
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        wayland_server::{Display, DisplayHandle, Resource, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Transform},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        dmabuf::{DmabufFeedbackBuilder, DmabufState},
        output::OutputManagerState,
        pointer_constraints::{PointerConstraintsState, with_pointer_constraint},
        relative_pointer::RelativePointerManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::{XdgShellState, decoration::XdgDecorationState},
        shm::ShmState,
        socket::ListeningSocketSource,
        xwayland_shell::XWaylandShellState,
    },
    xwayland::{X11Wm, XWayland, XWaylandEvent},
};
use std::{
    ffi::OsString,
    os::unix::fs::MetadataExt,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use starkit::native_surface::style::workspace::Wallpaper;
smithay::backend::renderer::element::render_elements! {
    DesktopElement<=GlesRenderer>;
    Window=smithay::desktop::space::SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Wallpaper=MemoryRenderBufferRenderElement<GlesRenderer>,
    Cursor=WaylandSurfaceRenderElement<GlesRenderer>,
}
fn wallpaper_buffer(wallpaper: Wallpaper, settings: &Settings) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        &wallpaper.bgra(settings.width, settings.height),
        Fourcc::Argb8888,
        (settings.width as i32, settings.height as i32),
        1,
        Transform::Normal,
        Some(vec![Rectangle::from_size(
            (settings.width as i32, settings.height as i32).into(),
        )]),
    )
}
/// Initial/client-requested geometry must fit the recording desktop. Manual
/// Alt-drags remain free to position a window beyond an edge during a demo.
fn fit_window_geometry(
    mut geometry: Rectangle<i32, Logical>,
    settings: &Settings,
) -> Rectangle<i32, Logical> {
    let width = settings.width as i32;
    let height = settings.height as i32;
    let scale = (f64::from(width) / f64::from(geometry.size.w.max(1)))
        .min(f64::from(height) / f64::from(geometry.size.h.max(1)))
        .min(1.);
    geometry.size.w = (f64::from(geometry.size.w.max(1)) * scale).floor().max(1.) as i32;
    geometry.size.h = (f64::from(geometry.size.h.max(1)) * scale).floor().max(1.) as i32;
    geometry.loc.x = geometry.loc.x.clamp(0, width - geometry.size.w);
    geometry.loc.y = geometry.loc.y.clamp(0, height - geometry.size.h);
    geometry
}
#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    #[ignore = "Requires a local hardware render device; no windows or apps are created"]
    fn wayland_client_discovers_hardware_gpu_without_a_window() {
        let desktop = Desktop::start(Settings {
            width: 320,
            height: 240,
            fps: 60,
        });
        let started = Instant::now();
        let socket = loop {
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "Headless compositor startup timed out"
            );
            match desktop.events.recv_timeout(Duration::from_secs(2)) {
                Ok(DesktopEvent::Ready(socket)) => break socket,
                Ok(DesktopEvent::Error(error)) => panic!("{error}"),
                _ => {}
            }
        };
        let result = Command::new("python3")
            .args(["-c", include_str!("../../scripts/profile-egl.py")])
            .env("WAYLAND_DISPLAY", socket)
            .output()
            .unwrap();
        let output = String::from_utf8_lossy(&result.stdout);
        eprintln!("{output}{}", String::from_utf8_lossy(&result.stderr));
        assert!(result.status.success(), "Headless EGL client failed");
        assert!(output.contains("GL_RENDERER="));
        assert!(
            !output.to_lowercase().contains("llvmpipe"),
            "Client is still software rendered"
        );
    }
    #[test]
    fn app_desktops_keep_sixty_hz_under_continuous_input() {
        let start = Instant::now();
        let mut clock = FrameClock::new(start);
        clock.burst = false;
        let mut frames = 0;
        for ms in 0..1000 {
            let now = start + Duration::from_millis(ms);
            clock.input(now, 60);
            clock.pending = true;
            if clock.ready(now, 60) {
                clock.rendered(now, 60);
                frames += 1;
            }
        }
        assert_eq!(frames, 60);
    }
    #[test]
    fn sixty_hz_does_not_accumulate_polling_jitter() {
        let start = Instant::now();
        let mut clock = FrameClock::new(start);
        let mut frames = 0;
        for ms in (0..=1000).step_by(2) {
            let now = start + Duration::from_millis(ms);
            if clock.ready(now, 60) {
                clock.rendered(now, 60);
                frames += 1;
            }
        }
        assert!(frames >= 60, "60 Hz drifted to {frames} frames");
    }
    #[test]
    fn input_bursts_are_bounded_and_do_not_accumulate_future_deadlines() {
        let start = Instant::now();
        let mut clock = FrameClock::new(start);
        clock.rendered(start, 60);
        clock.input(start + Duration::from_millis(1), 60);
        assert!(!clock.ready(start + Duration::from_millis(7), 60));
        assert!(clock.ready(start + Duration::from_millis(8), 60));
        clock.rendered(start + Duration::from_millis(8), 60);
        clock.pending = true;
        assert!(!clock.ready(start + Duration::from_millis(15), 60));
        assert!(clock.ready(start + Duration::from_millis(16), 60));
        clock.rendered(start + Duration::from_millis(16), 60);
        assert!(clock.next <= start + Duration::from_millis(33));
        let mut app = FrameClock::new(start);
        app.rendered(start, 30);
        app.input(start + Duration::from_millis(1), 30);
        app.pending = true;
        assert!(!app.ready(start + Duration::from_millis(16), 30));
        assert!(app.ready(start + Duration::from_millis(34), 30));
    }
    #[test]
    fn oversized_game_windows_and_stale_saved_layouts_fit_the_capture() {
        let settings = Settings::default();
        let exult = Rectangle::new((0, -180).into(), (1920, 1440).into());
        assert_eq!(
            fit_window_geometry(exult, &settings),
            Rectangle::new((0, 0).into(), (1440, 1080).into())
        );
        let terminal = Rectangle::new((40, 40).into(), (900, 650).into());
        assert_eq!(fit_window_geometry(terminal, &settings), terminal);
        let offscreen = Rectangle::new((5000, -100).into(), (320, 200).into());
        assert_eq!(
            fit_window_geometry(offscreen, &settings),
            Rectangle::new((1600, 0).into(), (320, 200).into())
        );
    }
}
pub(super) struct Drag {
    window: Window,
    pointer: Point<f64, Logical>,
    geometry: Rectangle<i32, Logical>,
    resize: u32,
}
// Keep quiet output bounded, but do not make an input response wait for both
// a client frame callback and another complete compositor refresh interval.
struct FrameClock {
    next: Instant,
    last: Instant,
    interactive_until: Instant,
    pending: bool,
    burst: bool,
}
impl FrameClock {
    fn new(now: Instant) -> Self {
        Self {
            next: now,
            last: now,
            interactive_until: now,
            pending: false,
            burst: true,
        }
    }
    fn input(&mut self, now: Instant, fps: u32) {
        if fps < 60 || !self.burst {
            return;
        }
        self.interactive_until = now + Duration::from_millis(150);
        self.next = self.next.min(now.max(self.last + Duration::from_millis(8)));
    }
    fn ready(&self, now: Instant, fps: u32) -> bool {
        now >= self.next
            || (fps >= 60
                && self.pending
                && now < self.interactive_until
                && now.saturating_duration_since(self.last) >= Duration::from_millis(8))
    }
    fn rendered(&mut self, now: Instant, fps: u32) {
        self.last = now;
        self.pending = false;
        let interval = Duration::from_secs_f64(1. / f64::from(fps));
        if now < self.next {
            self.next = now + interval;
        } else {
            self.next += interval;
            if now.saturating_duration_since(self.next) >= interval {
                self.next = now + interval;
            }
        }
    }
}
pub(super) struct State {
    clock: FrameClock,
    pub dh: DisplayHandle,
    pub space: Space<Window>,
    pub output: Output,
    pub settings: Settings,
    pub compositor: CompositorState,
    pub shell: XdgShellState,
    pub shm: ShmState,
    pub _output_manager: OutputManagerState,
    pub seat_state: SeatState<Self>,
    pub seat: Seat<Self>,
    pub data_device: DataDeviceState,
    pub popups: PopupManager,
    pub dmabuf: DmabufState,
    pub _decorations: XdgDecorationState,
    pub _relative: RelativePointerManagerState,
    pub _constraints: PointerConstraintsState,
    pub xwayland_shell: XWaylandShellState,
    pub xwm: Option<X11Wm>,
    pub display_number: Option<u32>,
    pub renderer: GlesRenderer,
    pub drag: Option<Drag>,
    window_drag: Option<u32>,
    buttons: std::collections::HashSet<u32>,
    pub started: Instant,
    pub socket: OsString,
    pub children: Vec<Child>,
    pub events: Sender<DesktopEvent>,
    pub audio_sink: Option<String>,
    pub relay_audio: bool,
    pub locked: bool,
    pub cursor_status: CursorImageStatus,
    pub keys: std::collections::HashSet<u32>,
    pub pending_launches: Vec<Launch>,
    pub layout: Vec<WindowLayout>,
    visible: Option<std::collections::HashSet<String>>,
}
#[derive(Default)]
pub(super) struct ClientState {
    compositor: CompositorClientState,
}
impl smithay::reexports::wayland_server::backend::ClientData for ClientState {
    fn initialized(&self, _: smithay::reexports::wayland_server::backend::ClientId) {}
    fn disconnected(
        &self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        _: smithay::reexports::wayland_server::backend::DisconnectReason,
    ) {
    }
}

fn renderer() -> Result<GlesRenderer> {
    let mut errors = vec![];
    for device in EGLDevice::enumerate()? {
        let software = device.is_software();
        // SAFETY: EGLDevice owns its handle and the context remains on this thread.
        let result =
            unsafe { EGLDisplay::new(device) }.and_then(|display| EGLContext::new(&display));
        match result {
            Ok(context) => match unsafe { GlesRenderer::new(context) } {
                Ok(mut renderer) => {
                    if std::env::var_os("STARMUX_RENDER_TIMING").is_some() {
                        let name = renderer.with_context(|gl| unsafe {
                            let name = gl.GetString(0x1F01); // GL_RENDERER
                            if name.is_null() {
                                String::from("unknown")
                            } else {
                                std::ffi::CStr::from_ptr(name.cast())
                                    .to_string_lossy()
                                    .into_owned()
                            }
                        })?;
                        eprintln!("Compositor renderer: {name}; software={}", software);
                    }
                    // Preserve 8-bit client pixels during the offscreen copy;
                    // default GL dithering can alter glyph coverage by a level.
                    renderer.with_context(|gl| unsafe { gl.Disable(0x0BD0) })?; // GL_DITHER
                    renderer.downscale_filter(TextureFilter::Nearest)?;
                    renderer.upscale_filter(TextureFilter::Nearest)?;
                    return Ok(renderer);
                }
                Err(e) => errors.push(e.to_string()),
            },
            Err(e) => errors.push(e.to_string()),
        }
    }
    anyhow::bail!("No usable offscreen OpenGL device: {}", errors.join("; "))
}

pub(super) fn run(
    settings: Settings,
    audio_sink: Option<String>,
    commands: Receiver<DesktopCommand>,
    events: Sender<DesktopEvent>,
    latest: Arc<Mutex<Option<Frame>>>,
) -> Result<()> {
    settings.validate()?;
    let mut display = Display::<State>::new()?;
    let dh = display.handle();
    let listener = ListeningSocketSource::new_auto()?;
    let socket = listener.socket_name().to_os_string();
    let renderer = renderer()?;
    let mut dmabuf = DmabufState::new();
    // Version 4 feedback tells Mesa which DRM render node backs this desktop.
    // Format-only v3 advertisement leaves Wayland EGL clients with fd=-1,
    // silently selecting llvmpipe even though our compositor uses the GPU.
    let feedback = (|| -> Result<_> {
        let device = EGLDevice::device_for_display(renderer.egl_context().display())?;
        let node = device.render_device_path()?;
        let main_device = std::fs::metadata(&node)?.rdev();
        let feedback =
            DmabufFeedbackBuilder::new(main_device, renderer.dmabuf_formats().iter().copied())
                .build()
                .context("Build render-device feedback")?;
        tracing::info!(render_node = %node.display(), "Advertising app GPU device");
        Ok(feedback)
    })();
    match feedback {
        Ok(feedback) => {
            dmabuf.create_global_with_default_feedback::<State>(&dh, &feedback);
        }
        Err(error) => {
            tracing::warn!(%error, "GPU device feedback unavailable; clients may use software rendering");
            dmabuf.create_global::<State>(&dh, renderer.dmabuf_formats().iter().copied());
        }
    }
    let output = Output::new(
        "STAR/MUX".into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "STAR/MUX".into(),
            model: "Demo desktop".into(),
        },
    );
    output.create_global::<State>(&dh);
    let mut seat_state = SeatState::new();
    let mut seat = seat_state.new_wl_seat(&dh, "starmux");
    seat.add_keyboard(Default::default(), 250, 30)?;
    seat.add_pointer();
    let mut state = State {
        clock: FrameClock::new(Instant::now()),
        compositor: CompositorState::new::<State>(&dh),
        shell: XdgShellState::new::<State>(&dh),
        shm: ShmState::new::<State>(&dh, vec![]),
        _output_manager: OutputManagerState::new_with_xdg_output::<State>(&dh),
        data_device: DataDeviceState::new::<State>(&dh),
        popups: PopupManager::default(),
        _decorations: XdgDecorationState::new::<State>(&dh),
        _relative: RelativePointerManagerState::new::<State>(&dh),
        _constraints: PointerConstraintsState::new::<State>(&dh),
        xwayland_shell: XWaylandShellState::new::<State>(&dh),
        dh,
        space: Space::default(),
        output,
        settings,
        seat_state,
        seat,
        dmabuf,
        renderer,
        cursor_status: CursorImageStatus::default_named(),
        drag: None,
        window_drag: None,
        buttons: Default::default(),
        xwm: None,
        display_number: None,
        started: Instant::now(),
        socket,
        children: vec![],
        events: events.clone(),
        audio_sink,
        relay_audio: false,
        locked: false,
        keys: Default::default(),
        pending_launches: vec![],
        layout: vec![],
        visible: None,
    };
    state.set_output();
    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    handle
        .insert_source(listener, |stream, _, state| {
            if let Err(e) = state
                .dh
                .insert_client(stream, Arc::new(ClientState::default()))
            {
                tracing::warn!("Wayland client: {e}");
            }
        })
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    match XWayland::spawn(
        &state.dh,
        None,
        std::iter::empty::<(String, String)>(),
        true,
        Stdio::null(),
        Stdio::null(),
        |_| {},
    ) {
        Ok((server, client)) => {
            let wm_handle = handle.clone();
            handle
                .insert_source(server, move |event, _, state| match event {
                    XWaylandEvent::Ready {
                        x11_socket,
                        display_number,
                    } => match X11Wm::start_wm(wm_handle.clone(), x11_socket, client.clone()) {
                        Ok(wm) => {
                            state.xwm = Some(wm);
                            state.display_number = Some(display_number);
                            for launch in std::mem::take(&mut state.pending_launches) {
                                state.launch(launch);
                            }
                        }
                        Err(e) => {
                            let _ = state
                                .events
                                .send(DesktopEvent::Error(format!("X11 support: {e}")));
                        }
                    },
                    XWaylandEvent::Error => {
                        let _ = state
                            .events
                            .send(DesktopEvent::Error("Xwayland did not start".into()));
                    }
                })
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        Err(e) => {
            let _ = events.send(DesktopEvent::Error(format!("X11 apps unavailable: {e}")));
        }
    }
    let _ = events.send(DesktopEvent::Ready(
        state.socket.to_string_lossy().into_owned(),
    ));
    let mut target: GlesRenderbuffer = state.renderer.create_buffer(
        Fourcc::Argb8888,
        (state.settings.width as i32, state.settings.height as i32).into(),
    )?;
    let mut damage = OutputDamageTracker::from_output(&state.output);
    let mut last_pixels: Option<Arc<Vec<u8>>> = None;
    let mut window_names = String::new();
    let mut last_selection = None;
    let mut saved_layout = Vec::new();
    let mut background = Wallpaper::from_palette(&Default::default());
    let mut wallpaper = wallpaper_buffer(background, &state.settings);
    let mut stopping = false;
    while !stopping {
        for command in commands.try_iter() {
            match command {
                DesktopCommand::Input(input) => {
                    if matches!(
                        &input,
                        Input::NamedKey { pressed: true, .. }
                            | Input::Key { pressed: true, .. }
                            | Input::Paste { .. }
                            | Input::Button { pressed: true, .. }
                            | Input::Scroll { .. }
                    ) {
                        state.clock.input(Instant::now(), state.settings.fps);
                    }
                    state.input(input);
                }
                DesktopCommand::InputBurst(enabled) => {
                    state.clock.burst = enabled;
                    if !enabled {
                        state.clock.interactive_until = Instant::now();
                    }
                }
                DesktopCommand::Launch(launch) => state.launch(launch),
                DesktopCommand::Visible(ids) => {
                    state.release();
                    state.visible = Some(ids.into_iter().collect());
                }
                DesktopCommand::FocusApp(id) => {
                    let window = state
                        .space
                        .elements()
                        .find(|w| state.info(w).app_id == id)
                        .cloned();
                    if let Some(window) = window {
                        state.focus(window);
                    }
                }
                DesktopCommand::FocusId(id)
                | DesktopCommand::MaximizeId(id)
                | DesktopCommand::CloseId(id) => {
                    let window = state
                        .space
                        .elements()
                        .find(|w| state.info(w).id == id)
                        .cloned();
                    if let Some(w) = window {
                        match command {
                            DesktopCommand::FocusId(_) => state.focus(w),
                            DesktopCommand::MaximizeId(_) => state.size_window(
                                &w,
                                Rectangle::from_size(
                                    (state.settings.width as i32, state.settings.height as i32)
                                        .into(),
                                ),
                            ),
                            _ => {
                                if let Some(t) = w.toplevel() {
                                    t.send_close()
                                } else if let Some(x) = w.x11_surface() {
                                    let _ = x.close();
                                }
                            }
                        }
                    }
                }
                DesktopCommand::RelayAudio(enabled) => state.relay_audio = enabled,
                DesktopCommand::Background(color) => {
                    if color != background {
                        background = color;
                        wallpaper = wallpaper_buffer(background, &state.settings);
                        damage = OutputDamageTracker::from_output(&state.output);
                        last_pixels = None;
                    }
                }
                DesktopCommand::Resize(settings) => {
                    settings.validate()?;
                    state.settings = settings;
                    state.set_output();
                    wallpaper = wallpaper_buffer(background, &state.settings);
                    target = state.renderer.create_buffer(
                        Fourcc::Argb8888,
                        (state.settings.width as i32, state.settings.height as i32).into(),
                    )?;
                    damage = OutputDamageTracker::from_output(&state.output);
                    last_pixels = None;
                }
                DesktopCommand::Focus(i) => {
                    let found_window = state.space.elements().nth(i).cloned();
                    if let Some(w) = found_window {
                        state.focus(w);
                    }
                }
                DesktopCommand::Maximize(i) => {
                    let found_window = state.space.elements().nth(i).cloned();
                    if let Some(w) = found_window {
                        state.size_window(
                            &w,
                            Rectangle::from_size(
                                (state.settings.width as i32, state.settings.height as i32).into(),
                            ),
                        );
                    }
                }
                DesktopCommand::Tile => {
                    let windows = state.space.elements().cloned().collect::<Vec<_>>();
                    let rects = super::tile_rects(windows.len(), &state.settings);
                    let layout = windows
                        .iter()
                        .zip(&rects)
                        .map(|(window, &[x, y, width, height])| WindowLayout {
                            app_id: state.info(window).app_id,
                            x,
                            y,
                            width,
                            height,
                        })
                        .collect();
                    for (window, [x, y, width, height]) in windows.iter().zip(rects) {
                        state.size_window(
                            window,
                            Rectangle::new((x, y).into(), (width, height).into()),
                        );
                    }
                    // Wayland configure acknowledgements arrive later; save requested geometry.
                    state.layout = layout;
                    let _ = events.send(DesktopEvent::Layout(state.layout.clone()));
                }
                DesktopCommand::Close(i) => {
                    if let Some(w) = state.space.elements().nth(i) {
                        if let Some(t) = w.toplevel() {
                            t.send_close();
                        } else if let Some(x) = w.x11_surface() {
                            let _ = x.close();
                        }
                    }
                }
                DesktopCommand::Restore(layout) => {
                    state.layout = layout;
                    for w in state.space.elements().cloned().collect::<Vec<_>>() {
                        state.restore_window(&w);
                    }
                }
                DesktopCommand::Layout => {
                    let _ = events.send(DesktopEvent::Layout(state.snapshot_layout()));
                }
                DesktopCommand::Shutdown => {
                    stopping = true;
                }
            }
        }
        event_loop.dispatch(Some(Duration::ZERO), &mut state)?;
        display.dispatch_clients(&mut state)?;
        state.apply_visibility();
        let now = Instant::now();
        if state.clock.ready(now, state.settings.fps) {
            let size = (state.settings.width as i32, state.settings.height as i32);
            let mut framebuffer = state.renderer.bind(&mut target)?;
            let mut elements: Vec<DesktopElement> = smithay::desktop::space::space_render_elements(
                &mut state.renderer,
                [&state.space],
                &state.output,
                1.,
            )?
            .into_iter()
            .map(DesktopElement::Window)
            .collect();
            if let CursorImageStatus::Surface(surface) = &state.cursor_status {
                let hotspot = smithay::wayland::compositor::with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .map(|attributes| attributes.lock().unwrap().hotspot)
                        .unwrap_or_default()
                });
                let location =
                    state.seat.get_pointer().unwrap().current_location() - hotspot.to_f64();
                let cursor: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                    render_elements_from_surface_tree(
                        &mut state.renderer,
                        surface,
                        (location.x.round() as i32, location.y.round() as i32),
                        1.,
                        1.,
                        Kind::Cursor,
                    );
                elements.splice(0..0, cursor.into_iter().map(DesktopElement::Cursor));
                smithay::desktop::utils::send_frames_surface_tree(
                    surface,
                    &state.output,
                    state.started.elapsed(),
                    Some(Duration::ZERO),
                    |_, _| Some(state.output.clone()),
                );
            }
            elements.push(DesktopElement::Wallpaper(
                MemoryRenderBufferRenderElement::from_buffer(
                    &mut state.renderer,
                    (0., 0.),
                    &wallpaper,
                    None,
                    None,
                    None,
                    Kind::Unspecified,
                )?,
            ));
            // Opaque wallpaper is last in front-to-back order and cached on GPU.
            let changed = damage
                .render_output(
                    &mut state.renderer,
                    &mut framebuffer,
                    1,
                    &elements,
                    [0., 0., 0., 1.],
                )?
                .damage
                .is_some();
            if changed || last_pixels.is_none() {
                let readback = Instant::now();
                let mapping = state.renderer.copy_framebuffer(
                    &framebuffer,
                    Rectangle::from_size(size.into()),
                    Fourcc::Argb8888,
                )?;
                let flipped = mapping.flipped();
                let raw = state.renderer.map_texture(&mapping)?;
                let mut pixels = raw.to_vec();
                // Smithay defines `flipped` relative to a lower-left origin: true
                // already means top-down rows, as required by GStreamer and KIT.
                if !flipped {
                    let stride = state.settings.width as usize * 4;
                    for y in 0..state.settings.height as usize / 2 {
                        let (top, bottom) =
                            pixels.split_at_mut((state.settings.height as usize - 1 - y) * stride);
                        top[y * stride..(y + 1) * stride].swap_with_slice(&mut bottom[..stride]);
                    }
                }
                last_pixels = Some(Arc::new(pixels));
                if std::env::var_os("STARMUX_RENDER_TIMING").is_some() {
                    eprintln!(
                        "Compositor frame: at_ns={} render_us={} readback_us={}",
                        crate::runtime::monotonic_ns(),
                        now.elapsed().as_micros(),
                        readback.elapsed().as_micros()
                    );
                }
            }
            let frame = Frame {
                pixels: last_pixels.as_ref().unwrap().clone(),
                width: state.settings.width,
                height: state.settings.height,
                time: now,
            };
            if changed {
                *latest.lock().unwrap() = Some(frame);
            }
            for w in state.space.elements() {
                w.send_frame(
                    &state.output,
                    state.started.elapsed(),
                    Some(Duration::ZERO),
                    |_, _| Some(state.output.clone()),
                );
            }
            state.space.refresh();
            state.popups.cleanup();
            // Wayland clients commit their configured size asynchronously.
            // Persist the final committed geometry even if it arrives after
            // the mouse release that ended a resize.
            if state.drag.is_none() {
                let layout = state.snapshot_layout();
                if layout != saved_layout {
                    saved_layout = layout.clone();
                    let _ = events.send(DesktopEvent::Layout(layout));
                }
            }
            let windows: Vec<_> = state.space.elements().map(|w| state.info(w)).collect();
            let focus = state
                .seat
                .get_keyboard()
                .and_then(|keyboard| keyboard.current_focus());
            let selection = state
                .space
                .elements()
                .enumerate()
                .find_map(|(index, window)| {
                    if !focus.as_ref().is_some_and(|focused| {
                        window
                            .wl_surface()
                            .is_some_and(|surface| surface.as_ref() == focused)
                    }) {
                        return None;
                    }
                    let location = state.space.element_location(window)?;
                    let size = window.geometry().size;
                    Some((index, [location.x, location.y, size.w, size.h]))
                });
            if selection != last_selection {
                last_selection = selection;
                let _ = events.send(DesktopEvent::Selection(selection));
            }
            let signature = format!("{windows:?}");
            if signature != window_names {
                window_names = signature;
                let _ = events.send(DesktopEvent::Windows(windows));
            }
            state.clock.rendered(now, state.settings.fps);
            state.children.retain_mut(|child| match child.try_wait() {
                Ok(Some(status)) => {
                    tracing::info!(pid = child.id(), %status, "Demo application exited");
                    false
                }
                Ok(None) => true,
                Err(error) => {
                    tracing::warn!(pid = child.id(), %error, "Could not check demo application");
                    true
                }
            });
        }
        display.flush_clients()?;
        std::thread::sleep(
            state
                .clock
                .next
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(2)),
        );
    }
    state.release();
    for child in &mut state.children {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

impl State {
    fn apply_visibility(&mut self) {
        let Some(ids) = &self.visible else { return };
        let windows = self
            .space
            .elements()
            .cloned()
            .map(|w| {
                let show = ids.contains(&self.info(&w).app_id);
                (w, show)
            })
            .collect::<Vec<_>>();
        for (w, show) in windows {
            let hidden = self
                .space
                .element_location(&w)
                .is_some_and(|p| p.x < -10000);
            if !show && !hidden {
                self.space.map_element(w, (-20000, -20000), false);
            } else if show && hidden {
                self.space.map_element(w.clone(), (0, 0), false);
                self.restore_window(&w);
            }
        }
    }
    fn set_output(&mut self) {
        let mode = Mode {
            size: (self.settings.width as i32, self.settings.height as i32).into(),
            refresh: self.settings.fps as i32 * 1000,
        };
        self.output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            None,
            Some((0, 0).into()),
        );
        self.output.set_preferred(mode);
        self.space.map_output(&self.output, (0, 0));
    }
    fn launch(&mut self, launch: Launch) {
        if self.display_number.is_none() && self.started.elapsed() < Duration::from_secs(3) {
            self.pending_launches.push(launch);
            return;
        }
        let result = (|| -> Result<Child> {
            let args = shell_words::split(&launch.command).context("Invalid command quoting")?;
            let program = args.first().context("Enter an application command")?;
            let mut command = Command::new(program);
            command
                .args(&args[1..])
                .current_dir(if launch.directory == std::path::Path::new("~") {
                    std::env::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"))
                } else if let Ok(rest) = launch.directory.strip_prefix("~/") {
                    std::env::home_dir().unwrap_or_default().join(rest)
                } else {
                    launch.directory.clone()
                })
                .env("WAYLAND_DISPLAY", &self.socket)
                .env("XDG_SESSION_TYPE", "wayland")
                .env("GDK_BACKEND", "wayland,x11")
                .env("QT_QPA_PLATFORM", "wayland;xcb")
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // A nested terminal must identify itself, rather than controlling
            // or inheriting graphics/SSH/multiplexer capabilities from SCREEN's host.
            for key in [
                "KITTY_WINDOW_ID",
                "KITTY_LISTEN_ON",
                "KITTY_PID",
                "WINDOWID",
                "TERM_PROGRAM",
                "TERM_PROGRAM_VERSION",
                "TMUX",
                "STY",
                "SSH_CONNECTION",
                "SSH_TTY",
                "WAYLAND_SOCKET",
                "XDG_ACTIVATION_TOKEN",
                "DESKTOP_STARTUP_ID",
            ] {
                command.env_remove(key);
            }
            if let Some(sink) = &self.audio_sink {
                command.env("PULSE_SINK", sink);
                if self.relay_audio {
                    command.env("PIPEWIRE_NODE", sink);
                }
            }
            if let Some(n) = self.display_number {
                command.env("DISPLAY", format!(":{n}"));
            } else {
                command.env_remove("DISPLAY");
            }
            command
                .spawn()
                .with_context(|| format!("Cannot launch {program}"))
        })();
        match result {
            Ok(child) => self.children.push(child),
            Err(e) => {
                let _ = self.events.send(DesktopEvent::Error(format!("{e:#}")));
            }
        }
    }
    fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space.element_under(pos).and_then(|(w, l)| {
            w.surface_under(pos - l.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + l).to_f64()))
        })
    }
    fn focus(&mut self, window: Window) {
        let surface = window.wl_surface().map(|s| s.into_owned());
        let keyboard = self.seat.get_keyboard().unwrap();
        let activation_changed = window.set_activated(true);
        // A gesture may request focus before and at its delayed mouse press.
        // Do not configure the same active client again during selection.
        if surface.is_some() && keyboard.current_focus() == surface && !activation_changed {
            return;
        }
        self.space.raise_element(&window, true);
        if let Some(x) = window.x11_surface()
            && let Some(wm) = &mut self.xwm
        {
            let _ = wm.raise_window(x);
        }
        keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
        for w in self.space.elements() {
            if let Some(t) = w.toplevel() {
                t.send_pending_configure();
            }
        }
    }
    fn info(&self, w: &Window) -> WindowInfo {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        w.hash(&mut hasher);
        let id = hasher.finish();
        if let Some(t) = w.toplevel() {
            smithay::wayland::compositor::with_states(t.wl_surface(), |states| {
                let d = states
                    .data_map
                    .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap();
                WindowInfo {
                    id,
                    name: d.title.clone().unwrap_or_else(|| "Application".into()),
                    app_id: d.app_id.clone().unwrap_or_default(),
                }
            })
        } else if let Some(x) = w.x11_surface() {
            WindowInfo {
                id,
                name: x.title(),
                app_id: x.class(),
            }
        } else {
            WindowInfo {
                id,
                name: "Application".into(),
                app_id: String::new(),
            }
        }
    }
    fn snapshot_layout(&self) -> Vec<WindowLayout> {
        self.space
            .elements()
            .filter_map(|w| {
                let pos = self.space.element_location(w)?;
                let size = w.geometry().size;
                Some(WindowLayout {
                    app_id: self.info(w).app_id,
                    x: pos.x,
                    y: pos.y,
                    width: size.w,
                    height: size.h,
                })
            })
            .collect()
    }
    fn restore_window(&mut self, w: &Window) {
        if let Some(l) = self
            .layout
            .iter()
            .find(|l| l.app_id == self.info(w).app_id)
            .cloned()
        {
            self.size_window(
                w,
                fit_window_geometry(
                    Rectangle::new(
                        (l.x, l.y).into(),
                        (l.width.max(100), l.height.max(80)).into(),
                    ),
                    &self.settings,
                ),
            );
        }
    }
    pub fn size_window(&mut self, window: &Window, geometry: Rectangle<i32, Logical>) {
        if let Some(t) = window.toplevel() {
            t.with_pending_state(|s| s.size = Some(geometry.size));
            t.send_pending_configure();
        }
        if let Some(x) = window.x11_surface() {
            let _ = x.configure(geometry);
        }
        self.space.map_element(window.clone(), geometry.loc, true);
    }
    fn input(&mut self, input: Input) {
        let pointer = self.seat.get_pointer().unwrap();
        let serial = SERIAL_COUNTER.next_serial();
        let time = self.started.elapsed().as_millis() as u32;
        match input {
            Input::Paste { text, mime } => {
                smithay::wayland::selection::data_device::set_data_device_selection(
                    &self.dh,
                    &self.seat,
                    vec![mime],
                    std::sync::Arc::new(text),
                );
                self.input(Input::NamedKey {
                    code: "char:v".into(),
                    pressed: true,
                    modifiers: 2,
                });
                self.input(Input::NamedKey {
                    code: "char:v".into(),
                    pressed: false,
                    modifiers: 2,
                });
                self.sync_modifiers(0);
            }
            Input::Motion { x, y } => {
                let pos: Point<f64, Logical> = (
                    x.clamp(0., self.settings.width as f64 - 1.),
                    y.clamp(0., self.settings.height as f64 - 1.),
                )
                    .into();
                let delta = pos - pointer.current_location();
                let under = self.surface_under(pointer.current_location());
                if self.window_drag.is_none() {
                    pointer.relative_motion(
                        self,
                        under.clone(),
                        &RelativeMotionEvent {
                            delta,
                            delta_unaccel: delta,
                            utime: self.started.elapsed().as_micros() as u64,
                        },
                    );
                }
                let locked = under.as_ref().is_some_and(|(s, _)| {
                    with_pointer_constraint(s, &pointer, |c| c.is_some_and(|c| c.is_active()))
                });
                if locked && self.window_drag.is_none() {
                    return;
                }
                if let Some(d) = &self.drag {
                    let delta = pos - d.pointer;
                    let mut geo = d.geometry;
                    let w = d.window.clone();
                    let edges = d.resize;
                    if edges == 0 {
                        geo.loc.x += delta.x as i32;
                        geo.loc.y += delta.y as i32;
                    } else {
                        if edges & 4 != 0 {
                            geo.loc.x += delta.x as i32;
                            geo.size.w -= delta.x as i32;
                        }
                        if edges & 8 != 0 {
                            geo.size.w += delta.x as i32;
                        }
                        if edges & 1 != 0 {
                            geo.loc.y += delta.y as i32;
                            geo.size.h -= delta.y as i32;
                        }
                        if edges & 2 != 0 {
                            geo.size.h += delta.y as i32;
                        }
                        geo.size.w = geo.size.w.max(100);
                        geo.size.h = geo.size.h.max(80);
                        if edges & 4 != 0 {
                            geo.loc.x = d.geometry.loc.x + d.geometry.size.w - geo.size.w;
                        }
                        if edges & 1 != 0 {
                            geo.loc.y = d.geometry.loc.y + d.geometry.size.h - geo.size.h;
                        }
                    }
                    if edges == 0 {
                        // Moving a window does not resize its client buffers.
                        if let Some(x) = w.x11_surface() {
                            let _ = x.configure(geo);
                        }
                        self.space.map_element(w, geo.loc, true);
                    } else {
                        self.size_window(&w, geo);
                    }
                }
                let under = self.surface_under(pos);
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time,
                    },
                );
                pointer.frame(self);
            }
            Input::Button { button, pressed } => {
                tracing::debug!(button,pressed,location=?pointer.current_location(),"Demo pointer button");
                // Compositor-owned drags work on borderless Wayland and X11
                // windows. Neither half of their button sequence reaches apps.
                if self.window_drag.is_some() {
                    if self.window_drag == Some(button) && !pressed {
                        self.window_drag = None;
                        self.drag = None;
                        let _ = self
                            .events
                            .send(DesktopEvent::Layout(self.snapshot_layout()));
                    }
                    return;
                }
                if pressed
                    && matches!(button, 272 | 273)
                    && self.buttons.is_empty()
                    && (self.keys.contains(&56) || self.keys.contains(&100))
                {
                    let found = self
                        .space
                        .element_under(pointer.current_location())
                        .map(|(w, _)| w.clone());
                    if let Some(window) = found {
                        let geometry = Rectangle::new(
                            self.space.element_location(&window).unwrap_or_default(),
                            window.geometry().size,
                        );
                        let position = pointer.current_location();
                        let resize = if button == 273 {
                            let horizontal =
                                if position.x < f64::from(geometry.loc.x + geometry.size.w / 2) {
                                    4
                                } else {
                                    8
                                };
                            let vertical =
                                if position.y < f64::from(geometry.loc.y + geometry.size.h / 2) {
                                    1
                                } else {
                                    2
                                };
                            horizontal | vertical
                        } else {
                            0
                        };
                        self.focus(window.clone());
                        self.drag = Some(Drag {
                            window,
                            pointer: position,
                            geometry,
                            resize,
                        });
                        self.window_drag = Some(button);
                    }
                    return;
                }
                if pressed {
                    self.buttons.insert(button);
                    if let Some(w) = self
                        .space
                        .element_under(pointer.current_location())
                        .map(|(w, _)| w.clone())
                    {
                        self.focus(w);
                    }
                } else {
                    if !self.buttons.remove(&button) {
                        return;
                    }
                    self.drag = None;
                }
                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: if pressed {
                            ButtonState::Pressed
                        } else {
                            ButtonState::Released
                        },
                        serial,
                        time,
                    },
                );
                pointer.frame(self);
            }
            Input::Modifiers { modifiers } => self.pointer_modifiers(modifiers),
            Input::NamedKey {
                code,
                pressed,
                modifiers,
            } => {
                let modifier = match code.as_str() {
                    "shift_left" => Some(42),
                    "shift_right" => Some(54),
                    "control_left" => Some(29),
                    "control_right" => Some(97),
                    "alt_left" => Some(56),
                    "alt_right" => Some(100),
                    "super_left" => Some(125),
                    "super_right" => Some(126),
                    _ => None,
                };
                if let Some(code) = modifier {
                    return self.input(Input::Key { code, pressed });
                }
                self.sync_modifiers(modifiers);
                use smithay::input::keyboard::xkb;
                let symbol = if let Some(c) =
                    code.strip_prefix("char:").and_then(|s| s.chars().next())
                {
                    xkb::utf32_to_keysym(c as u32)
                } else {
                    let name = match code.as_str() {
                        "enter" => "Return",
                        "escape" => "Escape",
                        "tab" | "backtab" => "Tab",
                        "backspace" => "BackSpace",
                        "delete" => "Delete",
                        "insert" => "Insert",
                        "left" => "Left",
                        "right" => "Right",
                        "up" => "Up",
                        "down" => "Down",
                        "home" => "Home",
                        "end" => "End",
                        "pageup" => "Prior",
                        "pagedown" => "Next",
                        "caps_lock" => "Caps_Lock",
                        "num_lock" => "Num_Lock",
                        _ => "",
                    };
                    let function = code.strip_prefix("f:").map(|n| format!("F{n}"));
                    xkb::keysym_from_name(function.as_deref().unwrap_or(name), xkb::KEYSYM_NO_FLAGS)
                };
                let keyboard = self.seat.get_keyboard().unwrap();
                let physical = keyboard.with_xkb_state(self, |ctx| {
                    let locked = ctx.xkb().lock().unwrap();
                    // SAFETY: the keymap is used only while its XKB context is locked.
                    let map = unsafe { locked.keymap() };
                    (map.min_keycode().raw()..=map.max_keycode().raw()).find_map(|k| {
                        (0..2)
                            .find(|level| {
                                map.key_get_syms_by_level(k.into(), 0, *level)
                                    .contains(&symbol)
                            })
                            .map(|level| (k, level))
                    })
                });
                if let Some((key, level)) = physical.filter(|_| symbol.raw() != 0) {
                    let character = code.strip_prefix("char:").and_then(|s| s.chars().next());
                    let caps = character.is_some_and(char::is_alphabetic)
                        && keyboard.modifier_state().caps_lock;
                    let needs_shift = (level == 1) ^ caps;
                    let effective = modifiers | u8::from(needs_shift || code == "backtab");
                    self.sync_modifiers(effective);
                    self.input(Input::Key {
                        code: key - 8,
                        pressed,
                    });
                    if !pressed {
                        self.sync_modifiers(modifiers);
                    }
                }
            }
            Input::Key { code, pressed } => {
                if pressed {
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
                self.seat.get_keyboard().unwrap().input::<(), _>(
                    self,
                    (code + 8).into(),
                    if pressed {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    serial,
                    time,
                    |_, _, _| FilterResult::Forward,
                );
            }
            Input::Scroll { x, y } => {
                pointer.axis(
                    self,
                    AxisFrame::new(time)
                        .source(AxisSource::Wheel)
                        .value(Axis::Horizontal, x * 15.)
                        .value(Axis::Vertical, y * 15.),
                );
                pointer.frame(self);
            }
            Input::Release => self.release(),
        }
    }
    fn pointer_modifiers(&mut self, modifiers: u8) {
        // A pointer's modifier mask is state, not a keyboard press. Sending
        // synthetic Shift keys to an enhanced-keyboard TUI can trigger a
        // repaint that clears the native selection on mouse release.
        let keyboard = self.seat.get_keyboard().unwrap();
        let mut state = keyboard.modifier_state();
        state.shift = modifiers & 1 != 0;
        state.ctrl = modifiers & 2 != 0;
        state.alt = modifiers & 4 != 0;
        state.logo = modifiers & 8 != 0;
        if keyboard.set_modifier_state(state) != 0
            && let Some(focus) = keyboard.current_focus()
        {
            let seat = self.seat.clone();
            focus.modifiers(
                &seat,
                self,
                keyboard.modifier_state(),
                SERIAL_COUNTER.next_serial(),
            );
        }
    }
    fn sync_modifiers(&mut self, modifiers: u8) {
        for (mask, keys) in [
            (1, [42, 54]),
            (2, [29, 97]),
            (4, [56, 100]),
            (8, [125, 126]),
        ] {
            let wanted = modifiers & mask != 0;
            if wanted && !keys.iter().any(|k| self.keys.contains(k)) {
                self.input(Input::Key {
                    code: keys[0],
                    pressed: true,
                });
            }
            if !wanted {
                for code in keys {
                    if self.keys.contains(&code) {
                        self.input(Input::Key {
                            code,
                            pressed: false,
                        });
                    }
                }
            }
        }
    }
    fn release(&mut self) {
        if self.window_drag.is_some() {
            let _ = self
                .events
                .send(DesktopEvent::Layout(self.snapshot_layout()));
        }
        let pointer = self.seat.get_pointer().unwrap();
        if let Some(s) = pointer.current_focus() {
            with_pointer_constraint(&s, &pointer, |c| {
                if let Some(c) = c {
                    c.deactivate();
                }
            });
        }
        for key in std::mem::take(&mut self.keys) {
            self.input(Input::Key {
                code: key,
                pressed: false,
            });
        }
        for button in std::mem::take(&mut self.buttons) {
            pointer.button(
                self,
                &ButtonEvent {
                    button,
                    state: ButtonState::Released,
                    serial: SERIAL_COUNTER.next_serial(),
                    time: self.started.elapsed().as_millis() as u32,
                },
            );
        }
        self.pointer_modifiers(0);
        pointer.motion(
            self,
            None,
            &MotionEvent {
                location: pointer.current_location(),
                serial: SERIAL_COUNTER.next_serial(),
                time: self.started.elapsed().as_millis() as u32,
            },
        );
        pointer.frame(self);
        self.seat
            .get_keyboard()
            .unwrap()
            .set_focus(self, None, SERIAL_COUNTER.next_serial());
        self.drag = None;
        self.window_drag = None;
        self.locked = false;
        let _ = self.events.send(DesktopEvent::Locked(false));
    }
}
