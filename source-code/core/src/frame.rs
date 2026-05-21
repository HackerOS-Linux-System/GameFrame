use std::time::{Duration, Instant};
use tracing::trace;

pub struct FramePacer {
    target_interval: Option<Duration>,
    last_frame:      Instant,
    frame_count:     u64,
    fps_smooth:      f32,
}

impl FramePacer {
    pub fn new(fps_cap: u32) -> Self {
        let target_interval = if fps_cap > 0 {
            Some(Duration::from_secs_f64(1.0 / fps_cap as f64))
        } else {
            None
        };
        Self {
            target_interval,
            last_frame: Instant::now(),
            frame_count: 0,
            fps_smooth: 0.0,
        }
    }

    /// Duration to sleep until the next frame slot.
    pub fn next_interval(&mut self) -> Duration {
        let elapsed = self.last_frame.elapsed();

        // Update smoothed FPS (exponential moving average, α=0.1)
        if elapsed.as_secs_f32() > 0.0 {
            let instant_fps = 1.0 / elapsed.as_secs_f32();
            self.fps_smooth = self.fps_smooth * 0.9 + instant_fps * 0.1;
        }
        self.frame_count += 1;
        self.last_frame = Instant::now();

        if let Some(interval) = self.target_interval {
            if elapsed < interval {
                let sleep = interval - elapsed;
                trace!(sleep_ms = sleep.as_millis(), "frame pacer sleep");
                return sleep;
            }
            return Duration::ZERO;
        }
        // VRR: yield for 1 ms; actual pace driven by DRM vblank
        Duration::from_millis(1)
    }

    /// Smoothed FPS (exponential moving average).
    pub fn smoothed_fps(&self) -> f32 { self.fps_smooth }

    /// Total frames rendered since start.
    pub fn frame_count(&self) -> u64 { self.frame_count }
}
