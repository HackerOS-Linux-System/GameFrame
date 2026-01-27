use std::error::Error;
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd};
use std::process::Command;
use std::sync::{Arc, Mutex};

use calloop::{channel, EventLoop, Interest, Mode, PostAction};
use smithay::{
    backend::{
        allocator::{dmabuf::Dmabuf, Fourcc, Modifier},
        drm::{
            compositor::{DrmCompositor, PrimaryPlaneElement},
            DrmBackend, DrmDevice, DrmDeviceFd, DrmError, DrmEvent, DrmSurface,
        },
        egl::{EGLContext, EGLDisplay},
        input::InputBackend,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            damage::DamageTrackedRenderer,
            element::{AsRenderElements, PixelShaderElement, TextureShaderElement},
            glow::{GlowFrame, GlowRenderer},
            gles2::Gles2Renderer,
            ImportDma, Renderer, Transform,
        },
        session::{logind::LogindSession, Session, Signal as SessionSignal},
        udev::{primary_gpu, UdevBackend, UdevDeviceId, UdevEvent},
    },
    delegate_compositor, delegate_dmabuf, delegate_input_method_manager, delegate_keyboard_shortcuts_inhibit,
    delegate_layer_shell, delegate_output, delegate_seat, delegate_shm, delegate_xdg_activation,
    delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{space::SpaceElement, LayerSurface, PopupKind, Space, Window, WindowSurfaceType},
    input::{
        keyboard::{KeyboardTarget, KeysymHandle, XkbConfig},
        pointer::{CursorImageRole, PointerTarget},
        Seat, SeatHandler, SeatState,
    },
    reexports::{
        calloop::{generic::Generic, timer::Timer},
        input::{Device as InputDevice, Libinput},
        rustix::event::PollFlags,
        wayland_protocols::{
            wp::keyboard_shortcuts_inhibit::v1::server::wp_keyboard_shortcuts_inhibit_manager_v1,
            xdg::activation::v1::server::xdg_activation_v1,
            xdg::decoration::v1::server::zxdg_decoration_manager_v1,
        },
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{
                wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_data_device_manager::WlDataDeviceManager,
                wl_output::WlOutput, wl_seat::WlSeat, wl_shm::WlShm, wl_shm_pool::WlShmPool,
                wl_surface::WlSurface,
            },
            Client, Display, DisplayHandle, GlobalDispatch,
        },
    },
    utils::{Clock, DeviceFd, IsAlive, Logical, Monotonic, Physical, Point, Rectangle, Scale, Size},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorHandler, CompositorState},
        data_device::{DataDeviceHandler, DataDeviceState},
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState},
        input_method::{InputMethodHandler, InputMethodManagerState},
        keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitHandler,
        output::{Output, OutputHandler, OutputManagerState, OutputState},
        seat::{Capability, KeyboardHandle, PointerHandle},
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
        xdg_activation::{XdgActivationHandler, XdgActivationState},
        xdg_decoration::{XdgDecorationHandler, XdgDecorationState},
        xdg_shell::{xdg_shell::XdgShell, XdgShellHandler},
    },
    xwayland::{XWayland, XWaylandEvent, XWaylandHandler, XWaylandKeyboard},
};
use tracing::{error, info, warn};
use xkbcommon::xkb::{Keymap, XKB_CONTEXT_NO_FLAGS};

#[derive(Debug)]
struct GameFrameState {
    display: Display<GameFrameState>,
    compositor_state: CompositorState,
    shm_state: ShmState,
    output_manager_state: OutputManagerState,
    seat_state: SeatState<GameFrameState>,
    data_device_state: DataDeviceState,
    dmabuf_state: DmabufState,
    xdg_shell_state: XdgShell,
    xdg_decoration_state: XdgDecorationState,
    xdg_activation_state: XdgActivationState,
    input_method_manager_state: InputMethodManagerState,
    keyboard_shortcuts_inhibit_state: wp_keyboard_shortcuts_inhibit_manager_v1::WpKeyboardShortcutsInhibitManagerV1,
    space: Space<Window>,
    layers: Vec<LayerSurface>,
    outputs: Vec<Output>,
    seats: Vec<Seat<GameFrameState>>,
    clock: Clock<Monotonic>,
    backend: Backend,
    xwayland: XWayland<GameFrameState>,
    loop_handle: calloop::LoopHandle<'static, GameFrameState>,
    // Renderer i DTR per output
    renderers: Vec<(DrmSurface, DamageTrackedRenderer<GlowRenderer>)>,
}

#[derive(Debug)]
enum Backend {
    Udev(UdevBackendData),
    Winit(WinitBackendData),
}

#[derive(Debug)]
struct UdevBackendData {
    session: LogindSession,
    udev_backend: UdevBackend,
    input_backend: LibinputInputBackend,
    primary_gpu: Option<String>,
}

#[derive(Debug)]
struct WinitBackendData {
    // ... implement if needed for testing
}

impl GameFrameState {
    fn new(display: Display<GameFrameState>, loop_handle: calloop::LoopHandle<'static, GameFrameState>) -> Self {
        // Init states
        let compositor_state = CompositorState::new::<Self>(&display.handle());
        let shm_state = ShmState::new::<Self>(&display.handle(), vec![]);
        let output_manager_state = OutputManagerState::new::<Self>(&display.handle());
        let seat_state = SeatState::new::<Self>(&display.handle());
        let data_device_state = DataDeviceState::new::<Self>(&display.handle());
        let dmabuf_state = DmabufState::new::<Self>(&display.handle());
        let xdg_shell_state = XdgShell::new::<Self>(&display.handle());
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&display.handle());
        let xdg_activation_state = XdgActivationState::new::<Self>(&display.handle());
        let input_method_manager_state = InputMethodManagerState::new::<Self>(&display.handle());
        let keyboard_shortcuts_inhibit_state = KeyboardShortcutsInhibitState::new::<Self>(&display.handle());

        let clock = Clock::<Monotonic>::new().unwrap();

        let xwayland = XWayland::new::<Self>(&display.handle(), &loop_handle);

        Self {
            display,
            compositor_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            dmabuf_state,
            xdg_shell_state,
            xdg_decoration_state,
            xdg_activation_state,
            input_method_manager_state,
            keyboard_shortcuts_inhibit_state,
            space: Space::default(),
            layers: Vec::new(),
            outputs: Vec::new(),
            seats: Vec::new(),
            clock,
            backend: Backend::Udev(UdevBackendData { /* init later */ }),
            xwayland,
            loop_handle,
            renderers: Vec::new(),
        }
    }
}

// Delegaty - delegate_*! (State)

delegate_compositor!(GameFrameState);
delegate_shm!(GameFrameState);
delegate_output!(GameFrameState);
delegate_seat!(GameFrameState);
delegate_xdg_shell!(GameFrameState);
delegate_xdg_decoration!(GameFrameState);
delegate_xdg_activation!(GameFrameState);
delegate_input_method_manager!(GameFrameState);
delegate_keyboard_shortcuts_inhibit!(GameFrameState);
delegate_dmabuf!(GameFrameState);
delegate_data_device!(GameFrameState);
delegate_layer_shell!(GameFrameState); // Dodaj jeśli chcesz layer-shell

impl BufferHandler for GameFrameState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for GameFrameState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl XdgShellHandler for GameFrameState {
    fn xdg_shell_state(&mut self) -> &mut XdgShell {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: smithay::wayland::xdg_shell::ToplevelSurface) {
        let window = Window::new(surface);
        self.space.map_element(window, (0, 0), false);
        info!("New toplevel");
    }

    fn new_popup(&mut self, surface: smithay::wayland::xdg_shell::PopupSurface, positioner: smithay::wayland::xdg_shell::XdgPositioner) {
        surface.with_pending_state(|state| state.geometry = positioner.get_geometry());
        if let Err(err) = self.space.map_popup(surface, positioner, None) {
            warn!("Failed to map popup: {}", err);
        }
    }

    fn grab(&mut self, _surface: smithay::wayland::xdg_shell::PopupSurface, _seat: WlSeat, _serial: u32) {}
}

impl XdgDecorationHandler for GameFrameState {
    fn xdg_decoration_state(&self) -> &XdgDecorationState {
        &self.xdg_decoration_state
    }

    fn new_decoration(&mut self, _toplevel: smithay::wayland::xdg_shell::ToplevelSurface, _mode: zxdg_toplevel_decoration_v1::Mode) {}
}

impl XdgActivationHandler for GameFrameState {
    fn xdg_activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(&mut self, _token: xdg_activation_v1::XdgActivationTokenV1, _token_data: smithay::wayland::xdg_activation::XdgActivationTokenData, _surface: WlSurface) {}
}

impl InputMethodHandler for GameFrameState {
    fn input_method_manager_state(&self) -> &InputMethodManagerState {
        &self.input_method_manager_state
    }
}

impl KeyboardShortcutsInhibitHandler for GameFrameState {
    fn keyboard_shortcuts_inhibit_state(&self) -> &wp_keyboard_shortcuts_inhibit_manager_v1::WpKeyboardShortcutsInhibitManagerV1 {
        &self.keyboard_shortcuts_inhibit_state
    }
}

impl DmabufHandler for GameFrameState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, _dmabuf: Dmabuf) -> Result<(), smithay::wayland::dmabuf::ImportError> {
        Ok(())
    }
}

impl DataDeviceHandler for GameFrameState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl SeatHandler for GameFrameState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<GameFrameState> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: smithay::input::pointer::CursorImageStatus) {}
    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}
}

impl XWaylandHandler for GameFrameState {
    fn xwayland_state(&mut self) -> &mut smithay::xwayland::XWaylandState {
        self.xwayland.state()
    }

    fn xwm_ready(&mut self, _conn: x11rb::connection::Connection) {}
    fn new_window(&mut self, _surface: smithay::xwayland::x11::X11Surface) {
        // Map X windows to space
    }
}

// Custom GLSL for integer scaling (nearest with sharp bilinear for non-integer)
const INTEGER_SCALING_SHADER: &str = r#"
#version 100
precision mediump float;

varying vec2 v_texcoord;
uniform sampler2D tex;
uniform vec2 tex_size;
uniform vec2 output_size;

void main() {
    vec2 scale = output_size / tex_size;
    vec2 int_scale = floor(scale);
    vec2 frac_scale = fract(scale);

    vec2 uv = v_texcoord * scale;
    vec2 int_uv = floor(uv);
    vec2 frac_uv = fract(uv);

    // Sharp bilinear for fractional part
    vec2 offset = frac_uv * (1.0 - frac_scale) + frac_scale * 0.5;
    vec2 sample_uv = (int_uv + offset) / scale;

    gl_FragColor = texture2D(tex, sample_uv);
}
"#;

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().init();

    let mut event_loop = EventLoop::try_new()?;

    let display = Display::new()?;
    let mut state = GameFrameState::new(display, event_loop.handle());

    // Parse args for backend
    let args: Vec<String> = std::env::args().collect();
    let use_winit = args.contains(&"--winit".to_string());

    if use_winit {
        // Implement winit backend for testing
        unimplemented!("Winit for testing - add if needed");
    } else {
        init_udev_backend(&mut event_loop, &mut state)?;
    }

    // XWayland start
    state.xwayland.start(&state.loop_handle)?;

    // Socket
    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_os_string();
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    event_loop.handle().insert_source(socket, |client, _, state| {
        state.display.insert_client(client, Arc::new(ClientState)).unwrap();
    })?;

    // Spawn test client
    Command::new("weston-terminal").env("WAYLAND_DISPLAY", socket_name).spawn()?;

    event_loop.run(None, &mut state, |state| {
        state.display.dispatch_clients(state).unwrap();
        state.display.flush_clients().unwrap();
    })?;

    Ok(())
}

fn init_udev_backend(event_loop: &mut EventLoop<GameFrameState>, state: &mut GameFrameState) -> Result<(), Box<dyn Error>> {
    let session = LogindSession::new()?;
    let udev_backend = UdevBackend::new(session.clone())?;

    let primary_gpu = primary_gpu(&session.seat().0).ok().flatten();

    let input_backend = Libinput::new_with_udev::<LibinputSessionInterface<LogindSession>>(session.clone().into());
    let mut libinput_backend = LibinputInputBackend::new(input_backend);

    state.backend = Backend::Udev(UdevBackendData {
        session,
        udev_backend,
        input_backend: libinput_backend,
        primary_gpu,
    });

    // Insert sources
    if let Backend::Udev(data) = &mut state.backend {
        event_loop.handle().insert_source(data.udev_backend.clone(), move |event, _, state| {
            match event {
                UdevEvent::Added { device_id, path } => {
                    if data.primary_gpu.as_ref().map_or(false, |gpu| gpu == path.to_str().unwrap()) {
                        if let Err(err) = init_drm_device(state, device_id) {
                            error!("Failed to init DRM: {}", err);
                        }
                    }
                }
                _ => {},
            }
        })?;

        event_loop.handle().insert_source(Generic::new(data.input_backend.clone(), Interest::READ, Mode::Level), |_, libinput, state| {
            libinput.dispatch()?;
            Ok(PostAction::Continue)
        })?;
    }

    Ok(())
}

fn init_drm_device(state: &mut GameFrameState, device_id: UdevDeviceId) -> Result<(), Box<dyn Error>> {
    if let Backend::Udev(data) = &state.backend {
        let fd = data.session.open_device(&device_id)?;
        let device_fd = DrmDeviceFd::new(DeviceFd(fd));

        let egl_display = EGLDisplay::new(device_fd.clone())?;
        let egl_context = EGLContext::new_with_config(&egl_display, egl::Config::builder()
            .api(egl::Api::GLES2) // OpenGL ES 2.0 for old GPUs
            .build()?
        )?;

        let renderer = unsafe { GlowRenderer::new(egl_context)? };
        renderer.bind_wl_display(&state.display.handle())?;

        // Set nearest for textures
        renderer.with_context(|gl| {
            unsafe {
                gl.tex_parameter_i(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST as i32);
                gl.tex_parameter_i(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST as i32);
            }
        })?;

        let surface = DrmSurface::new(device_fd.clone(), renderer.format())?;
        let compositor = DrmCompositor::new(&surface, renderer.clone())?;

        let mut dtr = DamageTrackedRenderer::from_compositor(compositor);

        state.renderers.push((surface, dtr));

        let mode = surface.current_mode();
        let size = mode.size();

        let output = Output::new("DRM-1", PhysicalProperties {
            size: (0, 0).into(),
            subpixel: wl_output::Subpixel::Unknown,
            make: "GameFrame".into(),
            model: "DRM".into(),
        });
        output.create_global::<GameFrameState>(&state.display.handle());
        output.change_current_state(Some(mode), None, None, Some((0, 0).into()));

        state.outputs.push(output.clone());
        state.space.add_output(Some(output));

        // Seat
        let seat = state.seat_state.new_wl_seat(&state.display.handle(), "seat0");
        let keyboard = seat.add_keyboard(XkbConfig::default(), 200, 25)?;
        let pointer = seat.add_pointer();

        state.seats.push(seat);

        Ok(())
    } else {
        Err("Not Udev".into())
    }
}

// Rendering loop w event_loop callback
// W event_loop.run, dla każdego output renderuj
// Użyj draw_surface_tree z custom shader jeśli integer scaling

fn render_output(state: &mut GameFrameState, surface: &DrmSurface, dtr: &mut DamageTrackedRenderer<GlowRenderer>, time: u32) -> Result<(), Box<dyn Error>> {
    dtr.render_output(surface, time, |renderer, frame| {
        frame.clear_color(0.0, 0.0, 0.0, 1.0)?;

        let output_size: Size<i32, Physical> = surface.resolution().into();

        if let Some(main_window) = state.space.elements().next() { // Assume single game window
            let window_size = main_window.bbox().size;
            let scale_x = (output_size.w / window_size.w) as f32;
            let scale_y = (output_size.h / window_size.h) as f32;
            let int_scale = scale_x.min(scale_y).floor() as i32;
            let scaled_size = Size::from((window_size.w * int_scale, window_size.h * int_scale));
            let loc = Point::from(((output_size.w - scaled_size.w) / 2, (output_size.h - scaled_size.h) / 2));

            // Użyj custom shader
            let elements = main_window.render_elements::<PixelShaderElement<GlowRenderer>>(renderer, loc.to_f64().to_physical(1.0), Scale::from(1.0), 1.0);
            for elem in elements {
                elem.draw(frame, Rectangle::from_loc_and_size(loc, scaled_size), &INTEGER_SCALING_SHADER, &[])?;
            }
        } else {
            // Fallback floating
            for window in state.space.elements() {
                let loc = state.space.element_location(window).unwrap_or_default();
                draw_surface_tree(frame, window.wl_surface(), loc, 1.0, &[])?;
            }
        }

        Ok(())
    })?;

    surface.queue_buffer()?;

    Ok(())
}

// Dodaj handler dla keybindings w KeyboardTarget itp.
// ...

#[derive(Debug)]
struct ClientState;

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

// ... inne impl
