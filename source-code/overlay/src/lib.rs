use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use tracing::debug;

// ── Telemetry snapshot ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Telemetry {
    pub fps:       f32,
    pub frame_ms:  f32,
    pub gpu_temp:  Option<u32>,   // °C
    pub gpu_usage: Option<u32>,   // 0-100 %
    pub cpu_usage: Option<f32>,   // 0-100 %
    pub ram_used:  Option<u64>,   // MiB
    pub ram_total: Option<u64>,   // MiB
    pub vram_used: Option<u64>,   // MiB
}

// ── Notification toast ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Toast {
    pub message:    String,
    pub ttl_frames: u32,  // decrements each frame
}

// ── Overlay state ─────────────────────────────────────────────────────────────

pub struct Overlay {
    pub visible:    bool,
    pub menu_open:  bool,
    pub width:      u32,
    pub height:     u32,
    pub telemetry:  Telemetry,
    pub toasts:     Vec<Toast>,
    pub pixmap:     Pixmap,  // ARGB8888 pixel buffer
}

impl Overlay {
    pub fn new(width: u32, height: u32) -> Self {
        let pixmap = Pixmap::new(width, height).expect("Pixmap alloc");
        debug!(width, height, "Overlay created");
        Self {
            visible: false,
            menu_open: false,
            width,
            height,
            telemetry: Telemetry::default(),
            toasts: Vec::new(),
            pixmap,
        }
    }

    pub fn show(&mut self)   { self.visible = true;  debug!("Overlay: shown"); }
    pub fn hide(&mut self)   { self.visible = false; debug!("Overlay: hidden"); }
    pub fn toggle(&mut self) { self.visible = !self.visible; }

    pub fn open_menu(&mut self)  { self.menu_open = true; }
    pub fn close_menu(&mut self) { self.menu_open = false; }

    pub fn push_toast(&mut self, message: impl Into<String>, ttl_frames: u32) {
        self.toasts.push(Toast { message: message.into(), ttl_frames });
        if self.toasts.len() > 4 { self.toasts.remove(0); }
    }

    pub fn update_telemetry(&mut self, t: Telemetry) {
        self.telemetry = t;
    }

    // ── Frame tick ────────────────────────────────────────────────────────────

    /// Called once per compositor frame.
    /// Decrements toast TTLs and re-renders if visible.
    pub fn tick(&mut self) {
        self.toasts.retain_mut(|t| { t.ttl_frames = t.ttl_frames.saturating_sub(1); t.ttl_frames > 0 });
        if self.visible || !self.toasts.is_empty() {
            self.render();
        }
    }

    // ── tiny-skia rendering ───────────────────────────────────────────────────

    fn render(&mut self) {
        self.pixmap.fill(Color::TRANSPARENT);

        if self.visible {
            self.draw_hud();
        }
        if self.menu_open {
            self.draw_menu();
        }
        self.draw_toasts();
    }

    /// HUD: FPS + GPU/CPU/RAM bars (top-left corner).
    fn draw_hud(&mut self) {
        let mut paint = Paint::default();
        let tf = Transform::identity();

        // ── Semi-transparent background pill ─────────────────────────────────
        paint.set_color_rgba8(0, 0, 0, 180);
        let bg = Rect::from_xywh(8.0, 8.0, 200.0, 120.0).unwrap();
        self.pixmap.fill_rect(bg, &paint, tf, None);

        // ── FPS counter ───────────────────────────────────────────────────────
        // tiny-skia has no text renderer – in production we'd use fontdue/rusttype.
        // We draw the FPS as coloured bars to indicate performance tier.
        let fps = self.telemetry.fps;
        let fps_color = if fps >= 55.0 {
            Color::from_rgba8(80, 200, 80, 255)   // green: smooth
        } else if fps >= 29.0 {
            Color::from_rgba8(240, 180, 40, 255)  // amber: playable
        } else {
            Color::from_rgba8(220, 60, 60, 255)   // red: bad
        };

        // FPS bar fill (scale: 0-120 fps → 0-180 px)
        let bar_w = (fps / 120.0 * 180.0).min(180.0);
        paint.set_color(fps_color);
        let fps_bar = Rect::from_xywh(12.0, 12.0, bar_w, 18.0).unwrap();
        self.pixmap.fill_rect(fps_bar, &paint, tf, None);

        // GPU usage bar (green → red)
        if let Some(gpu) = self.telemetry.gpu_usage {
            let w = gpu as f32 / 100.0 * 180.0;
            let c = usage_color(gpu);
            paint.set_color(c);
            let r = Rect::from_xywh(12.0, 36.0, w, 12.0).unwrap();
            self.pixmap.fill_rect(r, &paint, tf, None);
        }

        // GPU temp bar
        if let Some(temp) = self.telemetry.gpu_temp {
            let w = (temp as f32 / 100.0 * 180.0).min(180.0);
            let c = temp_color(temp);
            paint.set_color(c);
            let r = Rect::from_xywh(12.0, 54.0, w, 12.0).unwrap();
            self.pixmap.fill_rect(r, &paint, tf, None);
        }

        // CPU usage bar
        if let Some(cpu) = self.telemetry.cpu_usage {
            let w = cpu / 100.0 * 180.0;
            let c = usage_color(cpu as u32);
            paint.set_color(c);
            let r = Rect::from_xywh(12.0, 72.0, w, 12.0).unwrap();
            self.pixmap.fill_rect(r, &paint, tf, None);
        }

        // RAM usage bar
        if let (Some(used), Some(total)) = (self.telemetry.ram_used, self.telemetry.ram_total) {
            if total > 0 {
                let w = used as f32 / total as f32 * 180.0;
                paint.set_color(Color::from_rgba8(100, 160, 240, 220));
                let r = Rect::from_xywh(12.0, 90.0, w, 12.0).unwrap();
                self.pixmap.fill_rect(r, &paint, tf, None);
            }
        }

        // Bar outlines
        paint.set_color(Color::from_rgba8(200, 200, 200, 60));
        let mut stroke = Stroke::default();
        stroke.width = 0.5;
        for y in [12.0f32, 36.0, 54.0, 72.0, 90.0] {
            let path = PathBuilder::from_rect(Rect::from_xywh(12.0, y, 180.0, if y == 12.0 { 18.0 } else { 12.0 }).unwrap());
            self.pixmap.stroke_path(&path, &paint, &stroke, tf, None);
        }
    }

    /// Quick-access menu panel (centre of screen).
    fn draw_menu(&mut self) {
        let mut paint = Paint::default();
        let tf = Transform::identity();

        let cx = self.width as f32 / 2.0;
        let cy = self.height as f32 / 2.0;
        let w = 400.0f32;
        let h = 300.0f32;

        // Backdrop
        paint.set_color_rgba8(0, 0, 0, 220);
        let bg = Rect::from_xywh(cx - w / 2.0, cy - h / 2.0, w, h).unwrap();
        self.pixmap.fill_rect(bg, &paint, tf, None);

        // Border
        paint.set_color_rgba8(80, 140, 200, 200);
        let mut stroke = Stroke::default();
        stroke.width = 1.5;
        let path = PathBuilder::from_rect(bg);
        self.pixmap.stroke_path(&path, &paint, &stroke, tf, None);

        // Menu items (drawn as placeholder coloured bands)
        let items = [
            "Return to Game",
            "Steam Library",
            "Screenshot",
            "Settings",
            "Exit Session",
        ];
        for (i, _label) in items.iter().enumerate() {
            let item_y = cy - h / 2.0 + 40.0 + i as f32 * 44.0;
            paint.set_color_rgba8(40, 80, 140, if i == 0 { 160 } else { 80 });
            let item_r = Rect::from_xywh(cx - w / 2.0 + 16.0, item_y, w - 32.0, 36.0).unwrap();
            self.pixmap.fill_rect(item_r, &paint, tf, None);
        }
    }

    /// Toast notifications (bottom-right corner).
    fn draw_toasts(&mut self) {
        if self.toasts.is_empty() { return; }
        let mut paint = Paint::default();
        let tf = Transform::identity();
        let base_y = self.height as f32 - 16.0;

        for (i, toast) in self.toasts.iter().enumerate() {
            let alpha = ((toast.ttl_frames.min(30) as f32 / 30.0) * 220.0) as u8;
            paint.set_color_rgba8(20, 20, 30, alpha);
            let y = base_y - (i as f32 + 1.0) * 44.0;
            let r = Rect::from_xywh(self.width as f32 - 316.0, y, 300.0, 36.0).unwrap();
            self.pixmap.fill_rect(r, &paint, tf, None);
            // Accent bar
            paint.set_color_rgba8(80, 140, 200, alpha);
            let accent = Rect::from_xywh(self.width as f32 - 316.0, y, 4.0, 36.0).unwrap();
            self.pixmap.fill_rect(accent, &paint, tf, None);
            let _ = toast.message.as_str(); // used by text renderer in production
        }
    }

    /// Raw ARGB8888 bytes, ready to write into a Wayland SHM buffer.
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data()
    }
}

// ── Colour helpers ────────────────────────────────────────────────────────────

fn usage_color(pct: u32) -> Color {
    if pct < 70      { Color::from_rgba8(80, 200, 80, 220)  }
    else if pct < 90 { Color::from_rgba8(240, 180, 40, 220) }
    else             { Color::from_rgba8(220, 60, 60, 220)  }
}

fn temp_color(temp: u32) -> Color {
    if temp < 70      { Color::from_rgba8(80, 200, 80, 220)  }
    else if temp < 85 { Color::from_rgba8(240, 180, 40, 220) }
    else              { Color::from_rgba8(220, 60, 60, 220)  }
}
