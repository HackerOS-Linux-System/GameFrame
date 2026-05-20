use smithay::{
    delegate_compositor, delegate_data_device, delegate_layer_shell,
    delegate_output, delegate_primary_selection, delegate_seat,
    delegate_shm, delegate_xdg_shell,
    desktop::{Space, Window},
    input::{pointer::CursorImageStatus, Seat, SeatState},
    reexports::{
        calloop::LoopHandle,
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer::WlBuffer, wl_surface::WlSurface},
            Display, DisplayHandle,
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
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
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface,
                XdgShellHandler, XdgShellState,
            },
        },
        shm::{ShmHandler, ShmState},
    },
};

use gameframe_input::InputManager;
use gameframe_overlay::Overlay;
use crate::config::Config;

// ── Central state struct ──────────────────────────────────────────────────────

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

    pub space:            Space<Window>,
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
            space: Space::default(),
            seat,
            cursor_status:    CursorImageStatus::default_named(),
            pointer_location: Point::from((0.0, 0.0)),
            config,
            overlay,
            input_manager,
            running:     true,
            clock,
            loop_handle,
            socket_name,
        }
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

// ── BufferHandler (required by ShmState + delegate_shm) ───────────────────────

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
        // Smithay 0.7: buffer import bookkeeping lives in backend::renderer::utils
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
    }
}

// ── XDG Shell handler ─────────────────────────────────────────────────────────

impl XdgShellHandler for GameframeState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());
        self.space.map_element(window, (0, 0), true);

        // FIX 4: ToplevelState::Fullscreen and ToplevelState::FULLSCREEN both
        // don't exist as names in Smithay 0.7. The correct way to request
        // fullscreen in gaming mode is:
        //   1. Set fullscreen_output in pending_state (None = current output)
        //   2. Call send_configure() to notify the client
        //
        // The client receives xdg_toplevel.configure with the fullscreen state
        // bit set automatically by Smithay when fullscreen_output is Some/None
        // and the surface enters fullscreen.
        //
        // For a gaming compositor we simply send a maximized configure which
        // all clients honour, then rely on the client requesting fullscreen
        // (Steam does this automatically in gamepadui mode).
        surface.with_pending_state(|pending| {
            // Request the client fill the output
            pending.size = None; // let compositor decide
            // Smithay sets fullscreen via the output field:
            // pending.fullscreen_output = Some(output) for a specific output
            // For now we leave it None and let Steam/client request fullscreen
        });
        surface.send_configure();

        self.overlay.push_toast("Application launched", 180);
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(
        &mut self,
        _surface: PopupSurface,
        _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
        _serial: Serial,
    ) {}

    // Required in Smithay 0.7 for xdg_popup repositioning (v3+)
    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {}
}

// ── Layer Shell handler ───────────────────────────────────────────────────────

impl WlrLayerShellHandler for GameframeState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

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

// ── Output handler ────────────────────────────────────────────────────────────

impl OutputHandler for GameframeState {}

// ── SHM ───────────────────────────────────────────────────────────────────────

impl ShmHandler for GameframeState {
    fn shm_state(&self) -> &ShmState { &self.shm_state }
}

// ── Seat ──────────────────────────────────────────────────────────────────────
// Smithay 0.7: Window doesn't implement KeyboardTarget/PointerTarget/TouchTarget.
// WlSurface does implement all three – use it as the focus type.

impl smithay::input::SeatHandler for GameframeState {
    type KeyboardFocus = WlSurface;
    type PointerFocus  = WlSurface;
    type TouchFocus    = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> { &mut self.seat_state }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }
}

// ── Data device / DnD / selection ────────────────────────────────────────────

impl SelectionHandler for GameframeState {
    type SelectionUserData = ();
}

// Both Dnd grab handlers required by DataDeviceHandler in Smithay 0.7
impl ClientDndGrabHandler for GameframeState {}
impl ServerDndGrabHandler for GameframeState {}

impl DataDeviceHandler for GameframeState {
    fn data_device_state(&self) -> &DataDeviceState { &self.data_device_state }
}

impl PrimarySelectionHandler for GameframeState {
    fn primary_selection_state(&self) -> &PrimarySelectionState { &self.primary_selection }
}
