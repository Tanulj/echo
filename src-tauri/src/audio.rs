use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample;
use std::sync::{Arc, Mutex};

pub struct AudioRecorder {
    pub samples: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioRecorder {
    pub fn new() -> Result<Self, String> {
        Ok(AudioRecorder {
            samples: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 0, // Will be set to device's native sample rate
            channels: 0,    // Will be set to device's native channel count
        })
    }

    pub fn start_recording(&mut self) -> Result<cpal::Stream, String> {
        let host = cpal::default_host();

        // List all available input devices for debugging
        eprintln!("Available audio input devices:");
        if let Ok(devices) = host.input_devices() {
            for (idx, device) in devices.enumerate() {
                if let Ok(name) = device.name() {
                    eprintln!("  {}. {}", idx, name);
                }
            }
        }

        let device = host
            .default_input_device()
            .ok_or("No input device available")?;

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        eprintln!("Using device: {}", device_name);

        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default config: {}", e))?;

        self.sample_rate = config.sample_rate().0;
        self.channels = config.channels();
        eprintln!("Recording at native sample rate: {} Hz", self.sample_rate);
        eprintln!("Channels: {}, Format: {:?}", self.channels, config.sample_format());

        let samples = Arc::clone(&self.samples);

        // Clear previous samples
        samples.lock().unwrap().clear();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => self.build_stream::<f32>(&device, &config.into(), samples),
            cpal::SampleFormat::I16 => self.build_stream::<i16>(&device, &config.into(), samples),
            cpal::SampleFormat::U16 => self.build_stream::<u16>(&device, &config.into(), samples),
            _ => return Err("Unsupported sample format".to_string()),
        }?;

        stream.play().map_err(|e| format!("Failed to play stream: {}", e))?;

        Ok(stream)
    }

    fn build_stream<T>(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        samples: Arc<Mutex<Vec<f32>>>,
    ) -> Result<cpal::Stream, String>
    where
        T: cpal::Sample + cpal::SizedSample,
    {
        let stream = device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    let mut samples = samples.lock().unwrap();
                    // Convert samples to f32 with proper normalization
                    for &sample in data {
                        let f32_sample = sample.to_float_sample().to_sample::<f32>();
                        samples.push(f32_sample);
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )
            .map_err(|e| format!("Failed to build stream: {}", e))?;

        Ok(stream)
    }

    pub fn save_recording(&self) -> Result<String, String> {
        let samples = self.samples.lock().unwrap();

        if samples.is_empty() {
            return Err("No audio recorded".to_string());
        }

        // Apply audio normalization/gain to boost quiet recordings
        let normalized_samples = self.normalize_audio(&samples);

        // Save to WAV file in the system temp directory
        let path = std::env::temp_dir().join("echo_recording.wav");
        self.save_wav(&path, &normalized_samples)?;

        Ok(path.to_string_lossy().to_string())
    }

    fn normalize_audio(&self, samples: &[f32]) -> Vec<f32> {
        // Find peak amplitude
        let peak = samples.iter()
            .map(|&s| s.abs())
            .fold(0.0f32, |a, b| a.max(b));

        if peak < 0.001 {
            // Audio is too quiet, return original
            eprintln!("Warning: Audio signal too weak (peak: {})", peak);
            return samples.to_vec();
        }

        // Calculate gain to normalize to 0.9 (leaving some headroom)
        let target_peak = 0.9;
        let gain = target_peak / peak;

        eprintln!("Audio normalization - Peak: {:.4}, Gain: {:.2}x", peak, gain);

        // Apply gain to all samples
        samples.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect()
    }

    fn save_wav(&self, path: &std::path::Path, samples: &[f32]) -> Result<(), String> {
        let spec = hound::WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = hound::WavWriter::create(path, spec)
            .map_err(|e| format!("Failed to create WAV file: {}", e))?;

        // Convert F32 to I16 with proper clamping to avoid distortion (like VoiceTypr)
        for &sample in samples {
            // Clamp to avoid overflow and use 32767.0 for symmetric conversion
            let clamped = sample.clamp(-1.0, 1.0);
            let i16_sample = (clamped * 32767.0) as i16;
            writer
                .write_sample(i16_sample)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {}", e))?;

        Ok(())
    }
}
