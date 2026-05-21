use smithay::{
    backend::input::{
        // FIX: Axis and ButtonState live in backend::input (confirmed by compiler note)
        Axis, AxisSource, ButtonState,
        InputEvent, KeyState, KeyboardKeyEvent,
        PointerAxisEvent, PointerButtonEvent,
        PointerMotionEvent, PointerMotionAbsoluteEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::seat::WaylandFocus,
};
use std::borrow::Cow;
use tracing::info;

use crate::state::GameframeState;
use gameframe_input::BindingAction;

// ── Public entry point ────────────────────────────────────────────────────────

pub fn process_input_event<B>(state: &mut GameframeState, event: InputEvent<B>)
where
    B: smithay::backend::input::InputBackend,
{
    match event {
        InputEvent::Keyboard { event }              => handle_keyboard(state, event),
        InputEvent::PointerMotion { event }         => handle_pointer_motion(state, event),
        InputEvent::PointerMotionAbsolute { event } => handle_pointer_abs(state, event),
        InputEvent::PointerButton { event }         => handle_pointer_button(state, event),
        InputEvent::PointerAxis { event }           => handle_pointer_axis(state, event),
        _ => {}
    }
}

// ── Keyboard ─────────────────────────────────────────────────────────────────

fn handle_keyboard<B, E>(state: &mut GameframeState, event: E)
where
    B: smithay::backend::input::InputBackend,
    E: KeyboardKeyEvent<B>,
{
    let serial = SERIAL_COUNTER.next_serial();
    let time   = event.time_msec();
    let key    = event.key_code();
    let ks     = event.state();

    let kb = match state.seat.get_keyboard() { Some(k) => k, None => return };

    kb.input::<(), _>(state, key, ks, serial, time, |state, mods, keysym_handle| {
        if ks == KeyState::Pressed {
            if let Some(action) = check_binding(mods, keysym_handle.modified_sym()) {
                execute_binding(state, action);
                return FilterResult::Intercept(());
            }
        }
        FilterResult::Forward
    });
}

fn check_binding(mods: &ModifiersState, sym: Keysym) -> Option<BindingAction> {
    use gameframe_input::ModifierState;
    let mut mb = ModifierState::empty();
    if mods.shift { mb |= ModifierState::SHIFT; }
    if mods.ctrl  { mb |= ModifierState::CTRL; }
    if mods.alt   { mb |= ModifierState::ALT; }
    if mods.logo  { mb |= ModifierState::SUPER; }

    match (mb.bits(), u32::from(sym)) {
        (s, 0xff1b) if s == ModifierState::SUPER.bits() => Some(BindingAction::ToggleOverlay),
        (s, 0xff08) if s == (ModifierState::CTRL | ModifierState::ALT).bits() => Some(BindingAction::KillSession),
        _ => None,
    }
}

fn execute_binding(state: &mut GameframeState, action: BindingAction) {
    match action {
        BindingAction::ToggleOverlay => {
            state.overlay.toggle();
            info!("Overlay toggled (visible={})", state.overlay.visible);
        }
        BindingAction::KillSession => {
            info!("Kill binding – stopping session");
            state.running = false;
        }
        BindingAction::ScreenshotOutput => info!("Screenshot (not yet implemented)"),
        BindingAction::LaunchApp(cmd) => {
            let _ = std::process::Command::new("sh").args(["-c", &cmd]).spawn();
        }
        BindingAction::SwitchVt(vt) => info!(vt, "VT switch requested"),
    }
}

// ── Pointer motion ────────────────────────────────────────────────────────────

fn handle_pointer_motion<B, E>(state: &mut GameframeState, event: E)
where
    B: smithay::backend::input::InputBackend,
    E: PointerMotionEvent<B>,
{
    let serial = SERIAL_COUNTER.next_serial();
    let delta: Point<f64, Logical> = (event.delta_x(), event.delta_y()).into();
    state.pointer_location = state.pointer_location + delta;
    clamp_pointer(state);

    let pointer = match state.seat.get_pointer() { Some(p) => p, None => return };
    let focus   = pointer_focus(state);
    pointer.motion(state, focus, &MotionEvent {
        location: state.pointer_location,
        serial,
        time: event.time_msec(),
    });
}

fn handle_pointer_abs<B, E>(state: &mut GameframeState, event: E)
where
    B: smithay::backend::input::InputBackend,
    E: PointerMotionAbsoluteEvent<B>,
{
    let serial = SERIAL_COUNTER.next_serial();
    state.pointer_location = (event.x_transformed(1920), event.y_transformed(1080)).into();

    let pointer = match state.seat.get_pointer() { Some(p) => p, None => return };
    let focus   = pointer_focus(state);
    pointer.motion(state, focus, &MotionEvent {
        location: state.pointer_location,
        serial,
        time: event.time_msec(),
    });
}

fn handle_pointer_button<B, E>(state: &mut GameframeState, event: E)
where
    B: smithay::backend::input::InputBackend,
    E: PointerButtonEvent<B>,
{
    let serial = SERIAL_COUNTER.next_serial();

    if event.state() == ButtonState::Pressed {
        let loc = state.pointer_location;
        if let Some((window, _)) = state.space.element_under(loc) {
            let window = window.clone();
            state.activate_window(&window);
        }
    }

    let pointer = match state.seat.get_pointer() { Some(p) => p, None => return };
    let focus   = pointer_focus(state);

    // FIX: ButtonState lives in backend::input. ButtonEvent wants the same type.
    // Smithay 0.7: pointer::ButtonEvent.state is smithay::backend::input::ButtonState
    pointer.button(state, &ButtonEvent {
        serial,
        time:   event.time_msec(),
        button: event.button_code(),
        state:  event.state(),   // backend::input::ButtonState – same type, no conversion needed
    });
}

fn handle_pointer_axis<B, E>(state: &mut GameframeState, event: E)
where
    B: smithay::backend::input::InputBackend,
    E: PointerAxisEvent<B>,
{
    let pointer = match state.seat.get_pointer() { Some(p) => p, None => return };

    // FIX: AxisFrame::v120/value take smithay::backend::input::Axis (same Axis from imports)
    // wl_pointer::Axis is a different type – do NOT use it here.
    let mut frame = AxisFrame::new(event.time_msec()).source(AxisSource::Wheel);

    if let Some(v) = event.amount_v120(Axis::Vertical) {
        frame = frame.v120(Axis::Vertical, v as i32);
    }
    if let Some(v) = event.amount(Axis::Vertical) {
        frame = frame.value(Axis::Vertical, v);
    }
    if let Some(v) = event.amount_v120(Axis::Horizontal) {
        frame = frame.v120(Axis::Horizontal, v as i32);
    }
    if let Some(v) = event.amount(Axis::Horizontal) {
        frame = frame.value(Axis::Horizontal, v);
    }

    pointer.axis(state, frame);
    pointer.frame(state);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the WlSurface under the pointer and its position.
/// FIX: WaylandFocus::wl_surface() returns Option<Cow<'_, WlSurface>>.
/// We extract an owned WlSurface via .into_owned() on the Cow.
fn pointer_focus(state: &GameframeState) -> Option<(WlSurface, Point<f64, Logical>)> {
    state.space
        .element_under(state.pointer_location)
        .and_then(|(window, loc)| {
            // wl_surface() returns Option<Cow<'_, WlSurface>>
            window.wl_surface().map(|cow| (cow.into_owned(), loc.to_f64()))
        })
}

fn clamp_pointer(state: &mut GameframeState) {
    state.pointer_location.x = state.pointer_location.x.max(0.0).min(1919.0);
    state.pointer_location.y = state.pointer_location.y.max(0.0).min(1079.0);
}
