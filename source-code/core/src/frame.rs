use std::time::{Duration, Instant};
use tracing::trace;

pub struct FramePacer {
    target_interval: Option<Duration>,
    last_frame:      Instant,
}

impl FramePacer {
    pub fn new(fps_cap: u32) -> Self {
        let target_interval = if fps_cap > 0 {
            Some(Duration::from_secs_f64(1.0 / fps_cap as f64))
        } else {
            None  // VRR: pacing driven by DRM vblank
        };
        Self { target_interval, last_frame: Instant::now() }
    }

    /// Duration to sleep until the next frame slot.
    pub fn next_interval(&mut self) -> Duration {
        let elapsed = self.last_frame.elapsed();
        self.last_frame = Instant::now();

        if let Some(interval) = self.target_interval {
            if elapsed < interval {
                let sleep = interval - elapsed;
                trace!(sleep_ms = sleep.as_millis(), "frame pacer sleep");
                return sleep;
            }
            return Duration::ZERO;
        }
        // VRR: yield for 1 ms, actual pace comes from DRM vblank
        Duration::from_millis(1)
    }

    /// Instantaneous FPS based on last frame duration.
    pub fn instant_fps(&self) -> f32 {
        let elapsed = self.last_frame.elapsed().as_secs_f32();
        if elapsed > 0.0 { 1.0 / elapsed } else { 0.0 }
    }
}
