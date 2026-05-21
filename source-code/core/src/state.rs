use smithay::{
    delegate_compositor, delegate_data_device, delegate_dmabuf,
    delegate_layer_shell, delegate_output, delegate_primary_selection,
    delegate_seat, delegate_shm, delegate_xdg_shell,
    desktop::{Space, Window},
    input::{pointer::CursorImageStatus, Seat, SeatState},
    reexports::{
        calloop::LoopHandle,
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer::WlBuffer, wl_surface::WlSurface},
            Display, DisplayHandle, Resource,   // FIX: Resource for .id()
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Serial, SERIAL_COUNTER},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        output::{OutputHandler, OutputManagerState},
        selection::{
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
                ServerDndGrabHandler,
            },
            primary_selection::{PrimarySelectionHandler, PrimarySelectionState},
            SelectionHandler,
        },
        shell::{
            wlr_layer::{Layer, LayerSurface, WlrLayerShellHandler, WlrLayerShellState},
            xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState},
        },
        shm::{ShmHandler, ShmState},
    },
    backend::allocator::Buffer,   // FIX: for dmabuf.format()
};

use gameframe_input::InputManager;
use gameframe_overlay::Overlay;
use crate::{config::Config, window::WindowStack};

// ── Central state ─────────────────────────────────────────────────────────────

pub struct GameframeState {
    pub display_handle:    DisplayHandle,
    pub compositor_state:  CompositorState,
    pub xdg_shell_state:   XdgShellState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state:         ShmState,
    pub output_manager:    OutputManagerState,
    pub seat_state:        SeatState<Self>,
    pub data_device_state: DataDeviceState,
    pub primary_selection: PrimarySelectionState,
    pub dmabuf_state:      DmabufState,
    pub dmabuf_global:     Option<DmabufGlobal>,

    pub space:            Space<Window>,
    pub window_stack:     WindowStack,
    pub seat:             Seat<Self>,
    pub cursor_status:    CursorImageStatus,
    pub pointer_location: Point<f64, Logical>,

    pub config:        Config,
    pub overlay:       Overlay,
    pub input_manager: InputManager,
    pub running:       bool,
    pub clock:         Clock<Monotonic>,
    pub loop_handle:   LoopHandle<'static, Self>,
    pub socket_name:   String,
    pub last_frame_us: u64,
}

impl GameframeState {
    pub fn new(
        display: &mut Display<Self>,
        loop_handle: LoopHandle<'static, Self>,
        config: Config,
        socket_name: String,
    ) -> Self {
        let dh    = display.handle();
        let clock = Clock::new();

        let compositor_state  = CompositorState::new::<Self>(&dh);
        let xdg_shell_state   = XdgShellState::new::<Self>(&dh);
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let shm_state         = ShmState::new::<Self>(&dh, vec![]);
        let output_manager    = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state    = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let primary_selection = PrimarySelectionState::new::<Self>(&dh);
        let seat              = seat_state.new_wl_seat(&dh, "gameframe-seat0");
        let dmabuf_state      = DmabufState::new();

        let overlay       = Overlay::new(config.overlay.width, config.overlay.height);
        let input_manager = InputManager::new(gameframe_input::default_keybindings())
            .expect("InputManager::new");

        Self {
            display_handle: dh,
            compositor_state,
            xdg_shell_state,
            layer_shell_state,
            shm_state,
            output_manager,
            seat_state,
            data_device_state,
            primary_selection,
            dmabuf_state,
            dmabuf_global: None,
            space: Space::default(),
            window_stack: WindowStack::new(),
            seat,
            cursor_status:    CursorImageStatus::default_named(),
            pointer_location: Point::from((0.0, 0.0)),
            config,
            overlay,
            input_manager,
            running:      true,
            clock,
            loop_handle,
            socket_name,
            last_frame_us: 0,
        }
    }

    /// Set keyboard focus to the topmost window.
    pub fn refresh_focus(&mut self) {
        // FIX: Serial::from(u32) not from Time<Monotonic>
        let serial = SERIAL_COUNTER.next_serial();
        // FIX: WaylandFocus in scope → top_surface() works
        if let Some(surface) = self.window_stack.top_surface() {
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(self, Some(surface), serial);
            }
        }
    }

    pub fn activate_window(&mut self, window: &Window) {
        self.window_stack.bring_to_top(window);
        self.refresh_focus();
    }
}

// ── Per-client data ───────────────────────────────────────────────────────────

#[derive(Default)]
pub struct GameframeClientData {
    pub compositor: CompositorClientState,
}

impl ClientData for GameframeClientData {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

// ── Delegate macros ───────────────────────────────────────────────────────────

delegate_compositor!(GameframeState);
delegate_xdg_shell!(GameframeState);
delegate_layer_shell!(GameframeState);
delegate_shm!(GameframeState);
delegate_output!(GameframeState);
delegate_seat!(GameframeState);
delegate_data_device!(GameframeState);
delegate_primary_selection!(GameframeState);
delegate_dmabuf!(GameframeState);

// ── BufferHandler ─────────────────────────────────────────────────────────────

impl BufferHandler for GameframeState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

// ── CompositorHandler ─────────────────────────────────────────────────────────

impl CompositorHandler for GameframeState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }
    fn client_compositor_state<'a>(
        &self,
        client: &'a smithay::reexports::wayland_server::Client,
    ) -> &'a CompositorClientState {
        &client.get_data::<GameframeClientData>().unwrap().compositor
    }
    fn commit(&mut self, surface: &WlSurface) {
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
    }
}

// ── XDG Shell ─────────────────────────────────────────────────────────────────

impl XdgShellHandler for GameframeState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());
        self.window_stack.push(window.clone());
        self.space.map_element(window, (0, 0), true);
        surface.with_pending_state(|p| { p.size = None; });
        surface.send_configure();
        self.refresh_focus();
        self.overlay.push_toast("Application launched", 180);
        // FIX: use Resource trait for .id()
        tracing::info!(
            surface = ?surface.wl_surface().id(),
            "new toplevel – stack depth: {}", self.window_stack.len()
        );
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(
        &mut self,
        _surface: PopupSurface,
        _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
        _serial: Serial,
    ) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {}
}

// ── Layer Shell ───────────────────────────────────────────────────────────────

impl WlrLayerShellHandler for GameframeState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState { &mut self.layer_shell_state }
    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        tracing::debug!(%namespace, "new layer surface");
        let _ = surface;
    }
    fn layer_destroyed(&mut self, _surface: LayerSurface) {}
}

// ── Output ────────────────────────────────────────────────────────────────────

impl OutputHandler for GameframeState {}

// ── SHM ───────────────────────────────────────────────────────────────────────

impl ShmHandler for GameframeState {
    fn shm_state(&self) -> &ShmState { &self.shm_state }
}

// ── DMABUF ────────────────────────────────────────────────────────────────────

impl DmabufHandler for GameframeState {
    fn dmabuf_state(&mut self) -> &mut DmabufState { &mut self.dmabuf_state }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: ImportNotifier,
    ) {
        // FIX: Buffer trait in scope → .format() available
        tracing::debug!("dmabuf import: {:?}", dmabuf.format());
        drop(notifier); // dropping without .failed() = success
    }
}

// ── Seat ──────────────────────────────────────────────────────────────────────

impl smithay::input::SeatHandler for GameframeState {
    type KeyboardFocus = WlSurface;
    type PointerFocus  = WlSurface;
    type TouchFocus    = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> { &mut self.seat_state }

    fn focus_changed(&mut self, _seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // FIX: Resource in scope → .id() available
        tracing::debug!(surface = ?focused.map(|s| s.id()), "focus changed");
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

// ── Selection / DnD ──────────────────────────────────────────────────────────

impl SelectionHandler for GameframeState {
    type SelectionUserData = ();
}
impl ClientDndGrabHandler for GameframeState {}
impl ServerDndGrabHandler for GameframeState {}

impl DataDeviceHandler for GameframeState {
    fn data_device_state(&self) -> &DataDeviceState { &self.data_device_state }
}

impl PrimarySelectionHandler for GameframeState {
    fn primary_selection_state(&self) -> &PrimarySelectionState { &self.primary_selection }
}
