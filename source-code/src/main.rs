use std::error::Error;
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::path::PathBuf;

// Import from input crate directly
use input::event::pointer::PointerMotionEvent;

use smithay::{
    backend::{
        allocator::{dmabuf::Dmabuf, Fourcc, Modifier, gbm::{GbmAllocator, GbmBufferFlags, GbmDevice}},
        drm::{
            DrmDevice, DrmDeviceFd, DrmError, DrmEvent, NodeType,
        },
        egl::{EGLContext, EGLDisplay},
        input::{InputBackend, InputEvent, KeyState, KeyboardKeyEvent, AbsolutePositionEvent},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            utils::DamageTrackedRenderer,
            element::{AsRenderElements, RenderElement, surface::WaylandSurfaceRenderElement},
            glow::{GlowFrame, GlowRenderer},
            ImportDma, Renderer,
            gles::element::PixelShaderElement,
        },
        session::{Session, libseat::LibSeatSession},
        udev::{primary_gpu, UdevBackend, UdevEvent},
    },
    delegate_compositor, delegate_dmabuf, delegate_input_method_manager, delegate_keyboard_shortcuts_inhibit,
    delegate_layer_shell, delegate_output, delegate_seat, delegate_shm, delegate_xdg_activation,
    delegate_xdg_decoration, delegate_xdg_shell, delegate_data_device,
    desktop::{space::SpaceElement, LayerSurface, PopupKind, Space, Window, WindowSurfaceType},
    input::{
        keyboard::{FilterResult, KeyboardTarget, KeysymHandle, XkbConfig, KeyboardHandle},
        pointer::{CursorImageStatus, Focus, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{generic::Generic, timer::{Timer, TimeoutAction}, EventLoop, Interest, Mode, PostAction, Dispatcher, LoopHandle},
        input::{Device as InputDevice, Libinput, LibinputInterface},
        rustix::fs::OFlags,
        wayland_protocols::{
            wp::keyboard_shortcuts_inhibit::zv1::server::zwp_keyboard_shortcuts_inhibit_manager_v1,
            xdg::{
                activation::v1::server::xdg_activation_v1,
                decoration::zv1::server::zxdg_decoration_manager_v1,
            },
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
    utils::{Clock, DeviceFd, IsAlive, Logical, Monotonic, Physical, Point, Rectangle, Scale, Size, Serial, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorHandler, CompositorState},
        selection::data_device::{DataDeviceHandler, DataDeviceState},
        activation::{XdgActivationHandler, XdgActivationState, XdgActivationTokenData},
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        input_method::{InputMethodHandler, InputMethodManagerState, PopupSurface as ImPopupSurface},
        keyboard_shortcuts_inhibit::{KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState},
        output::{OutputHandler, OutputManagerState},
        seat::Capability,
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
        shell::xdg::{
            decoration::{XdgDecorationHandler, XdgDecorationState}, 
            XdgShell, XdgShellHandler, ToplevelSurface, PopupSurface, PositionerState
        },
    },
    xwayland::{XWayland, XWaylandEvent},
};
use tracing::{error, info, warn};

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
    keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    
    space: Space<Window>,
    layers: Vec<LayerSurface>,
    outputs: Vec<Output>,
    seats: Vec<Seat<GameFrameState>>,
    
    clock: Clock<Monotonic>,
    backend: Backend,
    xwayland: XWayland,
    loop_handle: LoopHandle<'static, GameFrameState>,
    
    // Renderer per DRM surface
    renderers: Vec<Option<(smithay::backend::drm::DrmSurface, DamageTrackedRenderer)>>,
    
    // Input state
    pointer_location: Point<f64, Physical>,
    running: bool,
}

#[derive(Debug)]
enum Backend {
    Udev(UdevBackendData),
    Headless, 
}

#[derive(Debug)]
struct UdevBackendData {
    session: LibSeatSession,
    udev_backend: UdevBackend,
    input_backend: LibinputInputBackend,
    primary_gpu: Option<String>,
}

impl GameFrameState {
    fn new(display: Display<GameFrameState>, loop_handle: LoopHandle<'static, GameFrameState>) -> Self {
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

        let xwayland = XWayland::new(&display.handle(), loop_handle.clone());

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
            backend: Backend::Headless,
            xwayland,
            loop_handle,
            renderers: Vec::new(),
            pointer_location: (0.0, 0.0).into(),
            running: true,
        }
    }
}

// Implement AsMut for states
impl AsMut<CompositorState> for GameFrameState {
    fn as_mut(&mut self) -> &mut CompositorState { &mut self.compositor_state }
}
impl AsMut<ShmState> for GameFrameState {
    fn as_mut(&mut self) -> &mut ShmState { &mut self.shm_state }
}
impl AsMut<OutputManagerState> for GameFrameState {
    fn as_mut(&mut self) -> &mut OutputManagerState { &mut self.output_manager_state }
}
impl AsMut<SeatState<GameFrameState>> for GameFrameState {
    fn as_mut(&mut self) -> &mut SeatState<GameFrameState> { &mut self.seat_state }
}
impl AsMut<DataDeviceState> for GameFrameState {
    fn as_mut(&mut self) -> &mut DataDeviceState { &mut self.data_device_state }
}
impl AsMut<DmabufState> for GameFrameState {
    fn as_mut(&mut self) -> &mut DmabufState { &mut self.dmabuf_state }
}
impl AsMut<XdgShell> for GameFrameState {
    fn as_mut(&mut self) -> &mut XdgShell { &mut self.xdg_shell_state }
}
impl AsMut<XdgDecorationState> for GameFrameState {
    fn as_mut(&mut self) -> &mut XdgDecorationState { &mut self.xdg_decoration_state }
}
impl AsMut<XdgActivationState> for GameFrameState {
    fn as_mut(&mut self) -> &mut XdgActivationState { &mut self.xdg_activation_state }
}
impl AsMut<InputMethodManagerState> for GameFrameState {
    fn as_mut(&mut self) -> &mut InputMethodManagerState { &mut self.input_method_manager_state }
}
impl AsMut<KeyboardShortcutsInhibitState> for GameFrameState {
    fn as_mut(&mut self) -> &mut KeyboardShortcutsInhibitState { &mut self.keyboard_shortcuts_inhibit_state }
}

// Delegaty
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
delegate_layer_shell!(GameFrameState);

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

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());
        
        // AUTO-MAXIMIZE LOGIC
        if let Some(output) = self.outputs.first() {
            let mode = output.current_mode().unwrap();
            let size = mode.size;
            
            surface.with_pending_state(|state| {
                state.size = Some(size.to_logical(1).to_i32_round());
            });
            surface.send_configure();
        }

        self.space.map_element(window, (0, 0), true);
        info!("New toplevel mapped");
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        if let Err(err) = self.space.map_popup(surface, positioner, None) {
            warn!("Failed to map popup: {}", err);
        }
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn reposition_request(&mut self, _surface: PopupSurface, _positioner: PositionerState, _token: u32) {
    }
}

impl XdgDecorationHandler for GameFrameState {
    fn new_decoration(&mut self, _toplevel: ToplevelSurface) {}
    fn request_mode(&mut self, _toplevel: ToplevelSurface, _mode: smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode) {}
    fn unset_mode(&mut self, _toplevel: ToplevelSurface) {}
}

impl XdgActivationHandler for GameFrameState {
    fn xdg_activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(&mut self, _token: smithay::reexports::wayland_protocols::xdg::activation::v1::server::xdg_activation_v1::XdgActivationV1, _token_data: XdgActivationTokenData, _surface: WlSurface) {}
}

impl InputMethodHandler for GameFrameState {
    fn new_popup(&mut self, _surface: ImPopupSurface) {}
    fn dismiss_popup(&mut self, _surface: ImPopupSurface) {}
    fn popup_repositioned(&mut self, _surface: ImPopupSurface) {}
    fn parent_geometry(&self, _parent: &WlSurface) -> Rectangle<i32, Logical> {
        Rectangle::from_loc_and_size((0,0), (0,0))
    }
}

impl KeyboardShortcutsInhibitHandler for GameFrameState {
    fn keyboard_shortcuts_inhibit_state(&mut self) -> &mut KeyboardShortcutsInhibitState {
        &mut self.keyboard_shortcuts_inhibit_state
    }
}

impl DmabufHandler for GameFrameState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, _dmabuf: Dmabuf, _notifier: ImportNotifier) {
        // Handle import
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
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<GameFrameState> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}
}

const INTEGER_SCALING_SHADER: &str = r#"
precision mediump float;
uniform sampler2D tex;
varying vec2 v_tex_coords;
uniform vec2 size;
uniform vec2 output_size;

void main() {
    vec2 scale = output_size / size;
    vec2 pixel_pos = v_tex_coords * size;
    vec2 adjusted_pos = floor(pixel_pos) + 0.5;
    gl_FragColor = texture2D(tex, adjusted_pos / size);
}
"#;

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().init();

    let mut event_loop = EventLoop::try_new()?;
    let mut display = Display::new()?;
    let mut state = GameFrameState::new(display, event_loop.handle());

    init_udev_backend(&mut event_loop, &mut state)?;

    // Start XWayland
    state.xwayland.start(state.loop_handle.clone())?;

    // Socket
    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_os_string();
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    info!("Listening on {:?}", socket_name);
    
    event_loop.handle().insert_source(socket, |client, _, state| {
        state.display.insert_client(client, Arc::new(ClientState)).unwrap();
    })?;

    let mut loop_signal = event_loop.get_signal();
    
    event_loop.run(None, &mut state, |state| {
        state.display.dispatch_clients(state).unwrap();
        state.display.flush_clients().unwrap();
        
        if !state.running {
            loop_signal.stop();
        }
    })?;

    Ok(())
}

fn init_udev_backend(event_loop: &mut EventLoop<GameFrameState>, state: &mut GameFrameState) -> Result<(), Box<dyn Error>> {
    // Use LibSeatSession directly
    let session = LibSeatSession::new().map_err(|e| Box::new(e) as Box<dyn Error>)?;
    let udev_backend = UdevBackend::new(&session.seat_name()).unwrap();

    let primary_gpu = primary_gpu(&session.seat_name()).ok().flatten();

    let input_backend = Libinput::new_with_session(session.clone());
    let libinput_backend = LibinputInputBackend::new(input_backend);

    state.backend = Backend::Udev(UdevBackendData {
        session,
        udev_backend: udev_backend.clone(),
        input_backend: libinput_backend.clone(),
        primary_gpu,
    });

    event_loop.handle().insert_source(udev_backend, move |event, _, state| {
        match event {
            UdevEvent::Added { device_id, path } => {
                if let Backend::Udev(data) = &state.backend {
                     if data.primary_gpu.as_ref().map_or(false, |gpu| gpu == path.to_str().unwrap()) {
                        if let Err(err) = init_drm_device(state, device_id, path) {
                            error!("Failed to init DRM: {}", err);
                        }
                     }
                }
            }
            _ => {},
        }
    })?;

    event_loop.handle().insert_source(libinput_backend, move |event, _, state| {
        match event {
             InputEvent::DeviceAdded { device } => {
                state.backend_input_device_added(device);
             },
             InputEvent::DeviceRemoved { device } => {
                state.backend_input_device_removed(device);
             },
             _ => {
                 state.process_input_event(event);
             }
        }
    })?;

    let source = state.xwayland.event_source();
    event_loop.handle().insert_source(source, |event, _, state| {
        if let XWaylandEvent::WindowAdded(surface) = event {
            state.new_window(surface);
        }
    })?;

    Ok(())
}

fn init_drm_device(state: &mut GameFrameState, _device_id: dev_t, path: std::path::PathBuf) -> Result<(), Box<dyn Error>> {
    let fd = {
        if let Backend::Udev(data) = &state.backend {
            data.session.open(path.as_path(), OFlags::RDWR | OFlags::CLOEXEC)?
        } else {
            return Err("Not Udev".into());
        }
    };
    
    let device_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let mut drm = DrmDevice::new(device_fd.clone(), true)?;

    let egl_display = EGLDisplay::new(device_fd.clone())?;
    
    // Explicitly select an EGL config (simple selection)
    let egl_context = EGLContext::new(&egl_display)?;

    let renderer = unsafe { GlowRenderer::new(egl_context)? };
    
    let (connector, mode) = drm.resources()?.connectors.into_iter()
        .filter_map(|c| drm.connector_properties(c).ok())
        .find(|c| c.connection_state == smithay::reexports::drm::control::connector::State::Connected)
        .and_then(|c| c.modes.first().map(|m| (c.handle, *m)))
        .ok_or("No connector found")?;

    let surface = drm.create_surface(
        drm.resources()?.crtcs[0],
        mode, 
        &[connector]
    )?;

    // Simplified renderer initialization for now to fix compilation
    // In a real app, you would pass the specific renderer type
    let dtr = DamageTrackedRenderer::new((800, 600).into(), 1.0.into()); 
    
    state.renderers.push(Some((surface.clone(), dtr)));

    let output = Output::new("DRM-1", PhysicalProperties {
        size: (mode.size().0 as i32, mode.size().1 as i32).into(),
        subpixel: Subpixel::Unknown,
        make: "GameFrame".into(),
        model: "DRM".into(),
    });
    output.create_global::<GameFrameState>(&state.display.handle());
    output.change_current_state(Some(mode), None, None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.outputs.push(output.clone());
    state.space.map_output(&output, (0, 0));

    let mut seat = state.seat_state.new_wl_seat(&state.display.handle(), "seat0");
    seat.add_keyboard(XkbConfig::default(), 200, 25)?;
    seat.add_pointer();
    state.seats.push(seat);

    let drm_event_source = drm_source(drm, surface.clone(), state.renderers.len() - 1);
    state.loop_handle.insert_source(drm_event_source, |event, _, state| {
        match event {
             DrmEvent::VBlank(_crtc) => {
                 let time = state.clock.now();
                 render_output(state, 0, time.try_into().unwrap()).ok();
             }
             DrmEvent::Error(_) => {}
        }
    })?;
    
    render_output(state, 0, 0)?;

    Ok(())
}

fn drm_source(
    drm: DrmDevice, 
    _surface: smithay::backend::drm::DrmSurface, 
    _id: usize
) -> Generic<DeviceFd> {
    Generic::new(
        drm.device_fd().clone(), 
        Interest::READ, 
        Mode::Level
    )
}

// Helper alias for dev_t since UdevBackend uses it
#[allow(non_camel_case_types)]
type dev_t = u64; 

impl GameFrameState {
    fn backend_input_device_added(&mut self, device: InputDevice) {
        info!("Input device added: {:?}", device.name());
    }
    
    fn backend_input_device_removed(&mut self, _device: InputDevice) {}
    
    fn process_input_event(&mut self, event: InputEvent<LibinputInputBackend>) {
        let seat = self.seats.get_mut(0).unwrap();
        
        match event {
            InputEvent::Keyboard { event, .. } => {
                 let key = event.key();
                 let state = event.state();
                 let serial = Serial::from(0);
                 let time = event.time_msec();
                 
                 let keyboard = seat.get_keyboard().unwrap();
                 
                 keyboard.input(
                     self, 
                     key, 
                     state, 
                     serial, 
                     time, 
                     |modifiers, keysym, state| {
                         if modifiers.ctrl && keysym == xkbcommon::xkb::keysyms::KEY_q && state == KeyState::Pressed {
                             return FilterResult::Intercept(());
                         }
                         FilterResult::Forward
                     }
                 );
            },
            InputEvent::PointerMotion { event, .. } => {
                 let pointer = seat.get_pointer().unwrap();
                 let delta = event.delta();
                 
                 self.pointer_location += delta;
                 
                 if let Some(output) = self.outputs.first() {
                     let size = output.current_mode().unwrap().size;
                     self.pointer_location.x = self.pointer_location.x.max(0.0).min(size.w as f64);
                     self.pointer_location.y = self.pointer_location.y.max(0.0).min(size.h as f64);
                 }
                 
                 let serial = Serial::from(0);
                 let under = self.space.element_under(self.pointer_location);
                 
                 if let Some((window, loc)) = under {
                     pointer.motion(
                         self, 
                         Some((window.wl_surface().unwrap().clone(), (self.pointer_location - loc.to_f64()).to_point())), 
                         &PointerMotionEvent {
                            time: event.time_msec(),
                            serial,
                            focus: Some((window.wl_surface().unwrap().clone(), (self.pointer_location - loc.to_f64()).to_point()))
                         }
                     );
                 } else {
                      pointer.motion(self, None, &PointerMotionEvent {
                            time: event.time_msec(),
                            serial,
                            focus: None
                      });
                 }
            },
            _ => {}
        }
    }
    
    fn new_window(&mut self, surface: smithay::backend::x11::X11Surface) {
        info!("New X11 Window created");
        let window = Window::new_x11_window(surface);
        self.space.map_element(window, (0, 0), true);
    }
}


fn render_output(state: &mut GameFrameState, renderer_idx: usize, _time: u32) -> Result<(), Box<dyn Error>> {
    let (surface, dtr) = state.renderers[renderer_idx].as_mut().unwrap();
    
    state.space.elements().for_each(|window| {
        window.send_frame(
             surface.current_mode().map(|m| m.size).unwrap_or_default().into(),
             state.clock.now(),
             None,
             |_, _| Some(surface.clone())
        );
    });
    
    // Stub render logic
    // dtr.render_output(...)
    
    surface.queue_buffer(None, None)?;

    Ok(())
}

#[derive(Debug)]
struct ClientState;
impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
