use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use smithay::{
    backend::{
        allocator::{
            dmabuf::Dmabuf,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc, Modifier
        },
        drm::{DrmDevice, DrmDeviceFd, DrmNode, DrmSurface},
        egl::{EGLContext, EGLDisplay, EGLDevice},
        input::{
            InputBackend, InputEvent, KeyState, KeyboardKeyEvent,
            PointerMotionEvent,
            Event,
        },
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            damage::OutputDamageTracker,
            element::{AsRenderElements, RenderElement, surface::WaylandSurfaceRenderElement},
            glow::{GlowFrame, GlowRenderer},
            ImportDma, Renderer,
            gles::element::PixelShaderElement,
        },
        session::{Session, libseat::{LibSeatSession, LibSeatSessionNotifier}},
        udev::{primary_gpu, UdevBackend, UdevEvent},
    },
    delegate_compositor, delegate_dmabuf, delegate_input_method_manager,
    delegate_keyboard_shortcuts_inhibit, delegate_layer_shell, delegate_output,
    delegate_seat, delegate_shm, delegate_xdg_activation, delegate_xdg_decoration,
    delegate_xdg_shell, delegate_data_device,
    desktop::{space::SpaceElement, LayerSurface, PopupKind, Space, Window, WindowSurfaceType},
    input::{
        keyboard::{FilterResult, KeyboardTarget, KeysymHandle, XkbConfig, KeyboardHandle, Keycode, ModifiersState},
        pointer::{CursorImageStatus, Focus, PointerHandle, MotionEvent},
        Seat, SeatHandler, SeatState,
    },
    output::{Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            generic::Generic,
            timer::{Timer, TimeoutAction},
            EventLoop, Interest, Mode, PostAction, Dispatcher, LoopHandle, Readiness
        },
        input::{Device as InputDevice, Libinput, LibinputInterface},
        drm::control::Device as ControlDevice,
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
                wl_surface::WlSurface, wl_data_source::WlDataSource,
            },
            Client, Display, DisplayHandle, GlobalDispatch,
        },
        input::event::keyboard::KeyboardEventTrait,
        rustix::fs::OFlags,
    },
    utils::{Clock, DeviceFd, IsAlive, Logical, Monotonic, Physical, Point, Rectangle, Scale, Size, Serial, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorHandler, CompositorState, CompositorClientState},
        selection::{
            SelectionHandler,
            data_device::{DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler},
        },
        xdg_activation::{XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData},
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        input_method::{InputMethodHandler, InputMethodManagerState, PopupSurface as ImPopupSurface},
        keyboard_shortcuts_inhibit::{KeyboardShortcutsInhibitHandler, KeyboardShortcutsInhibitState},
        output::{OutputHandler, OutputManagerState},
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
        shell::{
            xdg::{
                decoration::{XdgDecorationHandler, XdgDecorationState},
                XdgShellHandler, ToplevelSurface, PopupSurface, PositionerState,
                XdgShellState,
            },
            wlr_layer::{WlrLayerShellState, WlrLayerShellHandler, Layer, LayerSurface as WlrLayerSurface},
        },
        seat::WaylandFocus,
    },
};
use tracing::{error, info};

#[derive(Debug, Default)]
struct ClientState {
    compositor_state: CompositorClientState,
}
impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

#[derive(Debug)]
struct GameFrameState {
    display_handle: DisplayHandle,

    compositor_state: CompositorState,
    shm_state: ShmState,
    output_manager_state: OutputManagerState,
    seat_state: SeatState<GameFrameState>,
    data_device_state: DataDeviceState,
    dmabuf_state: DmabufState,
    xdg_shell_state: XdgShellState,
    xdg_decoration_state: XdgDecorationState,
    xdg_activation_state: XdgActivationState,
    input_method_manager_state: InputMethodManagerState,
    keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    layer_shell_state: WlrLayerShellState,

    space: Space<Window>,
    layers: Vec<LayerSurface>,
    outputs: Vec<Output>,
    seats: Vec<Seat<GameFrameState>>,

    clock: Clock<Monotonic>,
    backend: Backend,
    loop_handle: LoopHandle<'static, GameFrameState>,

    renderers: Vec<Option<(DrmSurface, OutputDamageTracker)>>,

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
    primary_gpu: Option<String>,
}

impl GameFrameState {
    fn new(display_handle: DisplayHandle, loop_handle: LoopHandle<'static, GameFrameState>) -> Self {
        let compositor_state = CompositorState::new::<Self>(&display_handle);
        let shm_state = ShmState::new::<Self>(&display_handle, vec![]);
        let output_manager_state = OutputManagerState::new();
        let seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&display_handle);
        let dmabuf_state = DmabufState::new();
        let xdg_shell_state = XdgShellState::new::<Self>(&display_handle);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&display_handle);
        let xdg_activation_state = XdgActivationState::new::<Self>(&display_handle);
        let input_method_manager_state = InputMethodManagerState::new::<Self, _>(&display_handle, |_| true);
        let keyboard_shortcuts_inhibit_state = KeyboardShortcutsInhibitState::new::<Self>(&display_handle);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&display_handle);

        let clock = Clock::new();

        Self {
            display_handle,
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
            layer_shell_state,
            space: Space::default(),
            layers: Vec::new(),
            outputs: Vec::new(),
            seats: Vec::new(),
            clock,
            backend: Backend::Headless,
            loop_handle,
            renderers: Vec::new(),
            pointer_location: (0.0, 0.0).into(),
            running: true,
        }
    }
}

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
impl AsMut<XdgShellState> for GameFrameState {
    fn as_mut(&mut self) -> &mut XdgShellState { &mut self.xdg_shell_state }
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
impl AsMut<WlrLayerShellState> for GameFrameState {
    fn as_mut(&mut self) -> &mut WlrLayerShellState { &mut self.layer_shell_state }
}

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

impl CompositorHandler for GameFrameState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        use smithay::backend::renderer::utils::on_commit_buffer_handler;
        on_commit_buffer_handler::<Self>(surface);
    }
}

impl OutputHandler for GameFrameState {}

impl WlrLayerShellHandler for GameFrameState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(&mut self, surface: WlrLayerSurface, _output: Option<WlOutput>, _layer: Layer, _namespace: String) {
        surface.send_configure();
    }

    fn layer_destroyed(&mut self, _surface: WlrLayerSurface) {}
}

impl XdgShellHandler for GameFrameState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());

        if let Some(output) = self.outputs.first() {
            let mode = output.current_mode().unwrap();
            let size = mode.size;

            surface.with_pending_state(|state| {
                state.size = Some(size.to_logical(1));
            });
            surface.send_configure();
        }

        self.space.map_element(window, (0, 0), true);
        info!("New toplevel mapped");
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
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
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        _token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        _surface: WlSurface
    ) {}
}

impl InputMethodHandler for GameFrameState {
    fn new_popup(&mut self, _surface: ImPopupSurface) {}
    fn dismiss_popup(&mut self, _surface: ImPopupSurface) {}
    fn popup_repositioned(&mut self, _surface: ImPopupSurface) {}
    fn parent_geometry(&self, _parent: &WlSurface) -> Rectangle<i32, Logical> {
        Rectangle::new((0,0).into(), (0,0).into())
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
    }
}

impl SelectionHandler for GameFrameState {
    type SelectionUserData = ();
}

impl WaylandDndGrabHandler for GameFrameState {
}

impl DataDeviceHandler for GameFrameState {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
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

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().init();

    let mut event_loop = EventLoop::try_new()?;
    let mut display = Display::new()?;
    let display_handle = display.handle();

    let mut state = GameFrameState::new(display_handle.clone(), event_loop.handle());

    init_udev_backend(&mut event_loop, &mut state)?;

    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_os_string();
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    info!("Listening on {:?}", socket_name);

    event_loop.handle().insert_source(socket, move |client, _, state: &mut GameFrameState| {
        state.display_handle.insert_client(client, Arc::new(ClientState::default())).unwrap();
    })?;

    let mut loop_signal = event_loop.get_signal();

    event_loop.run(None, &mut state, |state| {
        display.dispatch_clients(state).unwrap();
        display.flush_clients().unwrap();

        if !state.running {
            loop_signal.stop();
        }
    })?;

    Ok(())
}

fn init_udev_backend(event_loop: &mut EventLoop<GameFrameState>, state: &mut GameFrameState) -> Result<(), Box<dyn Error>> {
    let (session, notifier) = LibSeatSession::new().map_err(|e| Box::new(e) as Box<dyn Error>)?;
    event_loop.handle().insert_source(notifier, |_, _, _| {})?;

    let udev_backend = UdevBackend::new(&session.seat())?;
    let primary_gpu = primary_gpu(&session.seat()).ok().flatten().and_then(|p| p.to_str().map(|s| s.to_owned()));

    let libinput_interface = LibinputSessionInterface::from(session.clone());
    let input_backend = Libinput::new_with_udev(libinput_interface);
    let libinput_backend = LibinputInputBackend::new(input_backend);

    state.backend = Backend::Udev(UdevBackendData {
        session,
        primary_gpu,
    });

    event_loop.handle().insert_source(udev_backend, move |event, _, state| {
        match event {
            UdevEvent::Added { device_id, path } => {
                if let Backend::Udev(data) = &mut state.backend {
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

    Ok(())
}

fn init_drm_device(state: &mut GameFrameState, _device_id: dev_t, path: std::path::PathBuf) -> Result<(), Box<dyn Error>> {
    let fd = {
        if let Backend::Udev(data) = &mut state.backend {
            data.session.open(path.as_path(), OFlags::RDWR | OFlags::CLOEXEC)?
        } else {
            return Err("Not Udev".into());
        }
    };

    let device_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm, drm_notifier) = DrmDevice::new(device_fd.clone(), true)?;

    state.loop_handle.insert_source(drm_notifier, |_, _, _| {})?;

    let gbm = GbmDevice::new(device_fd.clone())?;
    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let egl_context = EGLContext::new(&egl_display)?;

    let _renderer = unsafe { GlowRenderer::new(egl_context)? };

    let res_handles = drm.device_fd().resource_handles()?;
    let connector_handle = res_handles.connectors().iter().find(|&&c| {
        let info = drm.device_fd().get_connector(c, true).ok();
        info.map(|i| i.state() == smithay::reexports::drm::control::connector::State::Connected).unwrap_or(false)
    }).copied().ok_or("No connected connector")?;

    let connector_info = drm.device_fd().get_connector(connector_handle, true)?;
    let mode = connector_info.modes().first().ok_or("No mode found")?;

    let surface = drm.create_surface(
        res_handles.crtcs()[0],
                                     *mode,
                                     &[connector_handle]
    )?;

    let output = Output::new(
        "DRM-1".to_string(),
                             PhysicalProperties {
                                 size: (mode.size().0 as i32, mode.size().1 as i32).into(),
                             subpixel: Subpixel::Unknown,
                             make: "GameFrame".into(),
                             model: "DRM".into(),
                             serial_number: "123".to_string(),
                             }
    );

    let tracker = OutputDamageTracker::from_output(&output);
    state.renderers.push(Some((surface, tracker)));

    output.create_global::<GameFrameState>(&state.display_handle);
    output.change_current_state(Some((*mode).into()), None, None, Some((0, 0).into()));
    output.set_preferred((*mode).into());

    state.outputs.push(output.clone());
    state.space.map_output(&output, (0, 0));

    let mut seat = state.seat_state.new_wl_seat(&state.display_handle, "seat0");
    seat.add_keyboard(XkbConfig::default(), 200, 25)?;
    seat.add_pointer();
    state.seats.push(seat);

    let drm_event_source = drm_source(drm, state.renderers.len() - 1);
    state.loop_handle.insert_source(drm_event_source, |event, _, state| {
        match event {
            Readiness { readable: true, .. } => {
                let time = state.clock.now();
                render_output(state, 0, time).ok();
                Ok(PostAction::Continue)
            }
            _ => Ok(PostAction::Continue),
        }
    })?;

    render_output(state, 0, state.clock.now())?;

    Ok(())
}

fn drm_source(
    drm: DrmDevice,
    _id: usize
) -> Generic<DrmDevice> {
    Generic::new(
        drm,
        Interest::READ,
        Mode::Level
    )
}

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
                    Keycode::from(key),
                               state,
                               serial,
                               time,
                               |data, modifiers, keysym| {
                                   if modifiers.ctrl && keysym.modified_sym().raw() == xkbcommon::xkb::keysyms::KEY_q {
                                       if state == KeyState::Pressed {
                                           info!("Ctrl+Q pressed - Quitting");
                                           data.running = false;
                                       }
                                       return FilterResult::Intercept(());
                                   }
                                   FilterResult::Forward
                               }
                );
            },
            InputEvent::PointerMotion { event, .. } => {
                let pointer = seat.get_pointer().unwrap();
                let delta = (event.delta_x(), event.delta_y());

                self.pointer_location.x += delta.0;
                self.pointer_location.y += delta.1;

                if let Some(output) = self.outputs.first() {
                    let size = output.current_mode().unwrap().size;
                    self.pointer_location.x = self.pointer_location.x.max(0.0).min(size.w as f64);
                    self.pointer_location.y = self.pointer_location.y.max(0.0).min(size.h as f64);
                }

                let serial = Serial::from(0);
                let under = self.space.element_under(self.pointer_location.to_logical(1.0));

                if let Some((window, loc)) = under {
                    let ptr_logical = self.pointer_location.to_logical(1.0);
                    let loc_f64 = loc.to_f64();
                    let relative_logical = ptr_logical - loc_f64;

                    let motion_event = MotionEvent {
                        location: relative_logical,
                        serial,
                        time: event.time_msec(),
                    };

                    pointer.motion(
                        self,
                        Some((window.wl_surface().unwrap().into_owned(), relative_logical)),
                                   &motion_event
                    );
                } else {
                    let motion_event = MotionEvent {
                        location: (0.0, 0.0).into(),
                        serial,
                        time: event.time_msec(),
                    };
                    pointer.motion(self, None, &motion_event);
                }
            },
            _ => {}
        }
    }
}

fn render_output(state: &mut GameFrameState, renderer_idx: usize, time: smithay::utils::Time<smithay::utils::Monotonic>) -> Result<(), Box<dyn Error>> {
    let (surface, _dtr) = state.renderers[renderer_idx].as_mut().unwrap();

    state.space.elements().for_each(|window| {
        if let Some(output) = state.outputs.get(renderer_idx) {
            let duration: Duration = time.into();
            window.send_frame(
                output,
                duration,
                None,
                |_, _| Some(output.clone())
            );
        }
    });

    Ok(())
}
