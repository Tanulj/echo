/// Find whisper executable path
fn find_whisper_path() -> Result<std::path::PathBuf, String> {
    let whisper_path = which::which("whisper-cli")
        .or_else(|_| which::which("whisper-cli.exe"))
        .or_else(|_| which::which("whisper"))
        .or_else(|_| which::which("whisper.exe"))
        .or_else(|_| which::which("main"))
        .or_else(|_| which::which("main.exe"));

    match whisper_path {
        Ok(path) => Ok(path),
        Err(_) => {
            let fallback = std::path::PathBuf::from("d:\\ws\\echo\\whisper.cpp\\build\\bin\\Release\\whisper-cli.exe");
            if fallback.exists() {
                Ok(fallback)
            } else {
                Err("Whisper executable not found. Please install whisper.cpp.".to_string())
            }
        }
    }
}

/// Get model path for given model ID
fn get_model_path(model_id: &str) -> Result<std::path::PathBuf, String> {
    let model_path = std::path::PathBuf::from(format!("d:\\ws\\echo\\whisper.cpp\\models\\ggml-{}.bin", model_id));

    if !model_path.exists() {
        return Err(format!("Model {} not found. Please download it from Settings.", model_id));
    }

    Ok(model_path)
}

/// Get optimal thread count
fn get_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(4)
}

/// Transcribe audio using local whisper.cpp with specified model
pub async fn transcribe_with_model(audio_path: &str, model_id: &str) -> Result<String, String> {
    let whisper_path = find_whisper_path()?;
    let model_path = get_model_path(model_id)?;
    let num_threads = get_thread_count();

    let mut cmd = tokio::process::Command::new(&whisper_path);
    cmd.arg("-f")
        .arg(audio_path)
        .arg("-m")
        .arg(&model_path)
        .arg("-nt")      // No timestamps
        .arg("-t")
        .arg(num_threads.to_string())
        .arg("-l")
        .arg("en")       // English only
        .arg("-np");     // No prints for faster output

    // On Windows, hide the console window
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output()
        .await
        .map_err(|e| format!("Failed to run whisper: {}", e))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Whisper failed: {}", error));
    }

    let full_output = String::from_utf8_lossy(&output.stdout);

    let transcription = full_output
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('['))
        .collect::<Vec<&str>>()
        .join(" ");

    Ok(transcription.trim().to_string())
}
