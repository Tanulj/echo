mod audio;
mod paste;
mod silence;
mod whisper;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_store::StoreExt;
use serde::{Deserialize, Serialize};

// Settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub hotkey: String,
    pub model: String,
    pub silence_duration: f32,
    pub auto_paste: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "ctrl+shift+r".to_string(),
            model: "large-v3-turbo".to_string(),
            silence_duration: 3.0,
            auto_paste: true,
        }
    }
}

// Transcription history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    pub duration_secs: f32,
}

// State to hold PTT recording data
struct PttState {
    stop_flag: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: Arc<Mutex<u32>>,
    channels: Arc<Mutex<u16>>,
    is_recording: Arc<AtomicBool>,
    silence_detector: Arc<Mutex<silence::SilenceDetector>>,
    silence_stopped: Arc<AtomicBool>,
    current_hotkey: Arc<Mutex<String>>,
}

// Audio level mapping for voice
fn map_voice_level(rms: f32) -> f64 {
    const SILENCE_THRESHOLD: f32 = 0.001;
    const WHISPER_LEVEL: f32 = 0.005;
    const NORMAL_SPEECH: f32 = 0.02;
    const LOUD_SPEECH: f32 = 0.1;

    if rms < SILENCE_THRESHOLD {
        0.0
    } else if rms < WHISPER_LEVEL {
        let normalized = (rms - SILENCE_THRESHOLD) / (WHISPER_LEVEL - SILENCE_THRESHOLD);
        (normalized * 0.3) as f64
    } else if rms < NORMAL_SPEECH {
        let normalized = (rms - WHISPER_LEVEL) / (NORMAL_SPEECH - WHISPER_LEVEL);
        (0.3 + normalized * 0.4) as f64
    } else if rms < LOUD_SPEECH {
        let normalized = (rms - NORMAL_SPEECH) / (LOUD_SPEECH - NORMAL_SPEECH);
        (0.7 + normalized * 0.25) as f64
    } else {
        0.95
    }
}

#[tauri::command]
async fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let store = app.store("settings.json").map_err(|e| e.to_string())?;

    let settings = Settings {
        hotkey: store.get("hotkey")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "ctrl+shift+r".to_string()),
        model: store.get("model")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "large-v3-turbo".to_string()),
        silence_duration: store.get("silence_duration")
            .and_then(|v| v.as_f64())
            .unwrap_or(3.0) as f32,
        auto_paste: store.get("auto_paste")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    };

    Ok(settings)
}

#[tauri::command]
async fn save_settings(app: tauri::AppHandle, settings: Settings, state: tauri::State<'_, PttState>) -> Result<(), String> {
    let store = app.store("settings.json").map_err(|e| e.to_string())?;

    // Update silence detector duration
    state.silence_detector.lock().unwrap().set_duration(settings.silence_duration);

    // Update hotkey only if it changed
    let current_hotkey = state.current_hotkey.lock().unwrap().clone();
    let hotkey_changed = current_hotkey != settings.hotkey;

    if hotkey_changed {
        // Validate new hotkey format first
        let new_shortcut: Shortcut = settings.hotkey.parse()
            .map_err(|_| "Invalid hotkey format. Use format like: ctrl+shift+r")?;

        // Unregister old hotkey
        if let Ok(old_shortcut) = current_hotkey.parse::<Shortcut>() {
            let _ = app.global_shortcut().unregister(old_shortcut);
        }

        // Register new hotkey
        app.global_shortcut().register(new_shortcut)
            .map_err(|e| format!("Failed to register hotkey: {}", e))?;

        *state.current_hotkey.lock().unwrap() = settings.hotkey.clone();
    }

    // Save all settings to store
    store.set("hotkey", serde_json::json!(settings.hotkey));
    store.set("model", serde_json::json!(settings.model));
    store.set("silence_duration", serde_json::json!(settings.silence_duration));
    store.set("auto_paste", serde_json::json!(settings.auto_paste));
    store.save().map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_available_models() -> Result<Vec<serde_json::Value>, String> {
    let models_dir = std::path::PathBuf::from("d:\\ws\\echo\\whisper.cpp\\models");

    let available_models = vec![
        serde_json::json!({
            "id": "base.en",
            "name": "Base English",
            "size": "142 MB",
            "speed": "Fast",
            "accuracy": "Good",
            "downloaded": models_dir.join("ggml-base.en.bin").exists()
        }),
        serde_json::json!({
            "id": "small.en",
            "name": "Small English",
            "size": "466 MB",
            "speed": "Medium",
            "accuracy": "Better",
            "downloaded": models_dir.join("ggml-small.en.bin").exists()
        }),
        serde_json::json!({
            "id": "medium.en",
            "name": "Medium English",
            "size": "1.5 GB",
            "speed": "Slow",
            "accuracy": "Great",
            "downloaded": models_dir.join("ggml-medium.en.bin").exists()
        }),
        serde_json::json!({
            "id": "large-v3-turbo",
            "name": "Large V3 Turbo",
            "size": "1.6 GB",
            "speed": "Medium",
            "accuracy": "Best",
            "downloaded": models_dir.join("ggml-large-v3-turbo.bin").exists()
        }),
    ];

    Ok(available_models)
}

#[tauri::command]
async fn download_model(model_id: String) -> Result<(), String> {
    let models_dir = std::path::PathBuf::from("d:\\ws\\echo\\whisper.cpp\\models");

    let url = match model_id.as_str() {
        "base.en" => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        "small.en" => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        "medium.en" => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin",
        "large-v3-turbo" => "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        _ => return Err("Unknown model".to_string()),
    };

    let output_path = models_dir.join(format!("ggml-{}.bin", model_id));

    let response = reqwest::get(url).await.map_err(|e| format!("Download failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let bytes = response.bytes().await.map_err(|e| format!("Failed to read response: {}", e))?;

    tokio::fs::write(&output_path, bytes).await
        .map_err(|e| format!("Failed to save model: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn get_history(app: tauri::AppHandle) -> Result<Vec<HistoryEntry>, String> {
    let store = app.store("history.json").map_err(|e| e.to_string())?;

    let history: Vec<HistoryEntry> = store.get("entries")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    Ok(history)
}

#[tauri::command]
async fn add_to_history(app: tauri::AppHandle, entry: HistoryEntry) -> Result<(), String> {
    let store = app.store("history.json").map_err(|e| e.to_string())?;

    let mut history: Vec<HistoryEntry> = store.get("entries")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // Add new entry at the beginning
    history.insert(0, entry);

    // Keep only last 50 entries
    history.truncate(50);

    store.set("entries", serde_json::json!(history));
    store.save().map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn clear_history(app: tauri::AppHandle) -> Result<(), String> {
    let store = app.store("history.json").map_err(|e| e.to_string())?;
    store.set("entries", serde_json::json!([]));
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn record_audio(duration_secs: u64) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mut recorder = audio::AudioRecorder::new()?;
        let stream = recorder.start_recording()?;
        std::thread::sleep(std::time::Duration::from_secs(duration_secs));
        drop(stream);
        std::thread::sleep(std::time::Duration::from_millis(100));
        recorder.save_recording()
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
async fn start_ptt_recording(app: tauri::AppHandle, state: tauri::State<'_, PttState>) -> Result<(), String> {
    if state.is_recording.load(Ordering::SeqCst) {
        return Err("Already recording".to_string());
    }

    state.is_recording.store(true, Ordering::SeqCst);
    state.stop_flag.store(false, Ordering::SeqCst);
    state.silence_stopped.store(false, Ordering::SeqCst);
    state.silence_detector.lock().unwrap().reset();
    state.samples.lock().unwrap().clear();

    let stop_flag = Arc::clone(&state.stop_flag);
    let samples = Arc::clone(&state.samples);
    let sample_rate_storage = Arc::clone(&state.sample_rate);
    let channels_storage = Arc::clone(&state.channels);
    let is_recording = Arc::clone(&state.is_recording);
    let silence_detector = Arc::clone(&state.silence_detector);
    let silence_stopped = Arc::clone(&state.silence_stopped);
    let app_handle = app.clone();

    tokio::task::spawn_blocking(move || {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = match host.default_input_device() {
            Some(d) => d,
            None => {
                is_recording.store(false, Ordering::SeqCst);
                return;
            }
        };

        let config = match device.default_input_config() {
            Ok(c) => c,
            Err(_) => {
                is_recording.store(false, Ordering::SeqCst);
                return;
            }
        };

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let update_interval = (sample_rate as usize) / 15; // 15 updates per second for smoother animation
        let sample_count = Arc::new(Mutex::new(0usize));
        let smoothed_level = Arc::new(Mutex::new(0.0f32));

        *sample_rate_storage.lock().unwrap() = sample_rate;
        *channels_storage.lock().unwrap() = channels;

        let samples_clone = Arc::clone(&samples);
        let app_for_callback = app_handle.clone();
        let sample_count_clone = Arc::clone(&sample_count);
        let smoothed_level_clone = Arc::clone(&smoothed_level);

        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Store samples
                {
                    let mut samples = samples_clone.lock().unwrap();
                    samples.extend_from_slice(data);
                }

                // Calculate RMS
                let sum: f32 = data.iter().map(|x| x * x).sum();
                let rms = (sum / data.len() as f32).sqrt();

                // Smooth the level with faster response
                {
                    let mut level = smoothed_level_clone.lock().unwrap();
                    // Faster attack (0.5), slower decay (0.8) for better visual feedback
                    if rms > *level {
                        *level = *level * 0.5 + rms * 0.5;
                    } else {
                        *level = *level * 0.8 + rms * 0.2;
                    }
                }

                // Send level update at intervals
                {
                    let mut count = sample_count_clone.lock().unwrap();
                    *count += data.len();
                    if *count >= update_interval {
                        *count = 0;
                        let level = *smoothed_level_clone.lock().unwrap();
                        let display_level = map_voice_level(level);
                        let _ = app_for_callback.emit("audio-level", display_level);
                    }
                }

                // Silence detection is disabled during manual PTT recording
                // Only the user releasing the hotkey will stop recording
                // This allows unlimited recording duration when holding PTT
            },
            |_err| {},
            None,
        );

        let stream = match stream {
            Ok(s) => s,
            Err(_) => {
                is_recording.store(false, Ordering::SeqCst);
                return;
            }
        };

        if stream.play().is_err() {
            is_recording.store(false, Ordering::SeqCst);
            return;
        }

        while !stop_flag.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        drop(stream);
        let _ = app_handle.emit("audio-level", 0.0f64);
        is_recording.store(false, Ordering::SeqCst);
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Ok(())
}

#[tauri::command]
async fn stop_ptt_recording(state: tauri::State<'_, PttState>) -> Result<String, String> {
    state.stop_flag.store(true, Ordering::SeqCst);

    // Wait for recording to actually stop (no timeout - wait as long as needed)
    while state.is_recording.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let samples = state.samples.lock().unwrap();
    let sample_rate = *state.sample_rate.lock().unwrap();
    let channels = *state.channels.lock().unwrap();

    if samples.is_empty() {
        return Err("No audio recorded".to_string());
    }

    let peak = samples.iter().map(|&s| s.abs()).fold(0.0f32, |a, b| a.max(b));
    let normalized: Vec<f32> = if peak >= 0.001 {
        let gain = 0.9 / peak;
        samples.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect()
    } else {
        samples.clone()
    };

    drop(samples);

    let path = std::path::PathBuf::from("d:\\ws\\echo\\echo_recording.wav");

    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|e| format!("Failed to create WAV file: {}", e))?;

    for &sample in &normalized {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_sample = (clamped * 32767.0) as i16;
        writer.write_sample(i16_sample)
            .map_err(|e| format!("Failed to write sample: {}", e))?;
    }

    writer.finalize()
        .map_err(|e| format!("Failed to finalize WAV: {}", e))?;

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn transcribe_audio(app: tauri::AppHandle, file_path: String) -> Result<String, String> {
    // Get current model from settings
    let store = app.store("settings.json").map_err(|e| e.to_string())?;
    let model = store.get("model")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "base.en".to_string());

    whisper::transcribe_with_model(&file_path, &model).await
}

#[tauri::command]
async fn paste_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.minimize();
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    tokio::task::spawn_blocking(move || {
        paste::paste_text(&text)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
async fn show_overlay(app: tauri::AppHandle, status: String) -> Result<(), String> {
    // Check if overlay already exists and just update status
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = app.emit_to("overlay", "overlay-status", status);
        let _ = window.show();
        return Ok(());
    }

    // Get primary monitor for positioning
    let primary_monitor = app.primary_monitor().map_err(|e| e.to_string())?;

    let (screen_width, screen_height, _scale) = if let Some(m) = primary_monitor {
        let size = m.size();
        let scale = m.scale_factor();
        (size.width as f64 / scale, size.height as f64 / scale, scale)
    } else {
        (1920.0, 1080.0, 1.0)
    };

    let window_width = 80.0;
    let window_height = 80.0;
    let x = (screen_width - window_width) / 2.0;
    let y = screen_height - window_height - 60.0; // 60px from bottom

    let _overlay = WebviewWindowBuilder::new(
        &app,
        "overlay",
        WebviewUrl::App("overlay.html".into())
    )
    .title("Echo Overlay")
    .inner_size(window_width, window_height)
    .position(x, y)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .resizable(false)
    .visible(true)
    .build()
    .map_err(|e| e.to_string())?;

    // Send initial status after window loads
    let app_clone = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = app_clone.emit_to("overlay", "overlay-status", status);
    });

    Ok(())
}

#[tauri::command]
async fn hide_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.close();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    let state = event.state();
                    match state {
                        ShortcutState::Pressed => {
                            let _ = app.emit("ptt-pressed", ());
                        }
                        ShortcutState::Released => {
                            let _ = app.emit("ptt-released", ());
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(PttState {
            stop_flag: Arc::new(AtomicBool::new(false)),
            samples: Arc::new(Mutex::new(Vec::new())),
            sample_rate: Arc::new(Mutex::new(44100)),
            channels: Arc::new(Mutex::new(2)),
            is_recording: Arc::new(AtomicBool::new(false)),
            silence_detector: Arc::new(Mutex::new(silence::SilenceDetector::new(3.0))),
            silence_stopped: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(Mutex::new("ctrl+shift+r".to_string())),
        })
        .setup(|app| {
            // Load settings and register hotkey
            let hotkey = if let Ok(store) = app.store("settings.json") {
                store.get("hotkey")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "ctrl+shift+r".to_string())
            } else {
                "ctrl+shift+r".to_string()
            };

            let shortcut: Shortcut = hotkey.parse().expect("Failed to parse shortcut");
            let _ = app.global_shortcut().register(shortcut);

            // IMPORTANT: Update PttState with the actual loaded hotkey
            let state: tauri::State<PttState> = app.state();
            *state.current_hotkey.lock().unwrap() = hotkey.clone();

            // Build system tray menu
            let show_item = MenuItem::with_id(app, "show", "Show Echo", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

            let tray_menu = Menu::with_items(app, &[&show_item, &separator, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            // Always show and focus - more reliable than toggle
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .tooltip("Echo - Voice to Text")
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            record_audio,
            start_ptt_recording,
            stop_ptt_recording,
            transcribe_audio,
            paste_text,
            get_settings,
            save_settings,
            get_available_models,
            download_model,
            get_history,
            add_to_history,
            clear_history,
            show_overlay,
            hide_overlay
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
