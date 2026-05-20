use anyhow::Result;
use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum InputEvent {
    Key      { key: u32, state: KeyState, mods: ModifierState },
    Pointer  { dx: f64, dy: f64 },
    Button   { button: u32, state: ButtonState },
    Scroll   { dx: f64, dy: f64 },
    Touch    { id: u32, x: f64, y: f64, phase: TouchPhase },
    Gamepad  { id: u8, event: GamepadEvent },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState     { Pressed, Released, Repeat }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState  { Pressed, Released }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase   { Begin, Update, End, Cancel }

#[derive(Debug, Clone)]
pub enum GamepadEvent {
    Button { button: u16, pressed: bool },
    Axis   { axis: u8, value: f32 },
    /// Steam/Guide button – triggers overlay
    Guide,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ModifierState: u8 {
        const SHIFT = 0b0001;
        const CTRL  = 0b0010;
        const ALT   = 0b0100;
        const SUPER = 0b1000;
    }
}

// ── Keybindings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keybinding {
    pub mods: u8,
    pub key:  u32,
    pub action: BindingAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindingAction {
    ToggleOverlay,
    KillSession,
    ScreenshotOutput,
    LaunchApp(String),
    SwitchVt(u8),
}

/// Default bindings matching Steam Gaming Mode conventions.
pub fn default_keybindings() -> Vec<Keybinding> {
    use BindingAction::*;
    vec![
        Keybinding { mods: ModifierState::SUPER.bits(),                     key: 0x1b, action: ToggleOverlay    }, // Super+Esc
        Keybinding { mods: (ModifierState::CTRL | ModifierState::ALT).bits(), key: 0xff08, action: KillSession  }, // Ctrl+Alt+Bksp
        Keybinding { mods: ModifierState::empty().bits(),                   key: 0xffc2, action: SwitchVt(2)    }, // F2
    ]
}

// ── Input manager ─────────────────────────────────────────────────────────────

pub struct InputManager {
    bindings:   Vec<Keybinding>,
    grabbed:    bool,
    mod_state:  ModifierState,
}

impl InputManager {
    pub fn new(bindings: Vec<Keybinding>) -> Result<Self> {
        info!("InputManager: {} keybindings loaded", bindings.len());
        Ok(Self { bindings, grabbed: false, mod_state: ModifierState::empty() })
    }

    /// Enable exclusive input grab – no events leak outside the session.
    pub fn grab(&mut self) -> Result<()> {
        self.grabbed = true;
        debug!("Input grab: ENABLED");
        Ok(())
    }

    /// Release grab – e.g. when overlay is showing.
    pub fn ungrab(&mut self) -> Result<()> {
        self.grabbed = false;
        debug!("Input grab: RELEASED");
        Ok(())
    }

    /// Check an incoming key event against keybindings.
    /// Returns the matching action if found.
    pub fn check_binding(&self, key: u32, mods: ModifierState) -> Option<&BindingAction> {
        self.bindings.iter().find_map(|b| {
            if b.key == key && ModifierState::from_bits_truncate(b.mods) == mods {
                Some(&b.action)
            } else {
                None
            }
        })
    }

    /// Update modifier tracking from a key event.
    pub fn update_modifiers(&mut self, key: u32, pressed: bool) {
        let bit = match key {
            0xffe1 | 0xffe2 => ModifierState::SHIFT,
            0xffe3 | 0xffe4 => ModifierState::CTRL,
            0xffe9 | 0xffea => ModifierState::ALT,
            0xffeb | 0xffec => ModifierState::SUPER,
            _ => return,
        };
        if pressed { self.mod_state |= bit; } else { self.mod_state &= !bit; }
    }

    pub fn mod_state(&self) -> ModifierState { self.mod_state }
    pub fn is_grabbed(&self) -> bool         { self.grabbed }
}
