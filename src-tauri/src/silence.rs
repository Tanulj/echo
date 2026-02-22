use std::time::{Duration, Instant};

/// Simple silence detector based on audio RMS level
pub struct SilenceDetector {
    /// Last time voice was detected
    last_voice_time: Instant,
    /// How long silence before triggering
    silence_duration: Duration,
    /// RMS threshold for voice detection (0.005 = 0.5%)
    voice_threshold: f32,
    /// Has voice ever been detected?
    has_detected_voice: bool,
}

impl SilenceDetector {
    pub fn new(silence_duration_secs: f32) -> Self {
        Self {
            last_voice_time: Instant::now(),
            silence_duration: Duration::from_secs_f32(silence_duration_secs),
            voice_threshold: 0.01, // 1% - slightly higher than whisper.cpp's 0.5%
            has_detected_voice: false,
        }
    }

    /// Update with current RMS level, returns true if silence threshold exceeded
    pub fn update(&mut self, rms: f32) -> bool {
        if rms > self.voice_threshold {
            // Voice detected
            self.last_voice_time = Instant::now();
            self.has_detected_voice = true;
            false
        } else {
            // Only trigger silence stop if we've heard voice before
            // This prevents stopping immediately when recording starts
            self.has_detected_voice && self.last_voice_time.elapsed() > self.silence_duration
        }
    }

    /// Reset the detector for a new recording session
    pub fn reset(&mut self) {
        self.last_voice_time = Instant::now();
        self.has_detected_voice = false;
    }

    /// Update silence duration
    pub fn set_duration(&mut self, secs: f32) {
        self.silence_duration = Duration::from_secs_f32(secs);
    }
}
