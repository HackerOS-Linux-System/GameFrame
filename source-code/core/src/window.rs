use std::borrow::Cow;
use smithay::{
    desktop::Window,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::seat::WaylandFocus,
};

pub struct WindowStack {
    windows: Vec<Window>,
}

impl WindowStack {
    pub fn new() -> Self { Self { windows: Vec::new() } }

    pub fn push(&mut self, window: Window) {
        self.windows.retain(|w| w != &window);
        self.windows.insert(0, window);
    }

    pub fn bring_to_top(&mut self, window: &Window) {
        if let Some(pos) = self.windows.iter().position(|w| w == window) {
            let w = self.windows.remove(pos);
            self.windows.insert(0, w);
        }
    }

    /// Remove windows whose WlSurface matches.
    /// FIX: wl_surface() returns Option<Cow<'_, WlSurface>> – compare via as_ref()
    pub fn remove_by_wl_surface(&mut self, surface: &WlSurface) {
        self.windows.retain(|w| {
            w.wl_surface()
                .as_ref()
                .map(|cow| cow.as_ref() != surface)
                .unwrap_or(true)
        });
    }

    pub fn top(&self) -> Option<&Window> { self.windows.first() }

    /// Owned WlSurface of the topmost window.
    /// FIX: wl_surface() returns Cow<'_, WlSurface> – call .into_owned()
    pub fn top_surface(&self) -> Option<WlSurface> {
        self.top()
            .and_then(|w| w.wl_surface())
            .map(|cow| cow.into_owned())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Window> { self.windows.iter() }
    pub fn len(&self) -> usize { self.windows.len() }
    pub fn is_empty(&self) -> bool { self.windows.is_empty() }

    /// True if the given surface belongs to the active (topmost) window.
    /// FIX: compare WlSurface by value, not via as_deref() (WlSurface: !Deref)
    pub fn is_active_surface(&self, surface: &WlSurface) -> bool {
        self.top()
            .and_then(|w| w.wl_surface())
            .as_ref()
            .map(|cow| cow.as_ref() == surface)
            .unwrap_or(false)
    }
}

impl Default for WindowStack {
    fn default() -> Self { Self::new() }
}
