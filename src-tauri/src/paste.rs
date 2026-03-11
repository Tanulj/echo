use std::io::Write;

/// Set clipboard via pbcopy then simulate Cmd+V via osascript.
/// Returns "pasted" if auto-paste succeeded, "copied" if Accessibility is blocked.
pub fn paste_text(text: &str) -> Result<String, String> {
    // Write to clipboard via pbcopy (no Accessibility needed)
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn pbcopy: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())
            .map_err(|e| format!("Failed to write to pbcopy: {}", e))?;
    }
    child.wait().map_err(|e| format!("pbcopy failed: {}", e))?;
    eprintln!("Clipboard set via pbcopy ({} chars)", text.len());

    // Attempt Cmd+V via osascript — requires Accessibility permission
    let script = r#"delay 0.6
tell application "System Events" to keystroke "v" using {command down}"#;

    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output();

    match output {
        Ok(o) if o.status.success() => {
            eprintln!("osascript paste succeeded");
            Ok("pasted".to_string())
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            eprintln!("osascript keystroke blocked: {}", err.trim());
            // Clipboard is already set — user can press Cmd+V manually
            Ok("copied".to_string())
        }
        Err(e) => {
            eprintln!("Failed to run osascript: {}", e);
            Ok("copied".to_string())
        }
    }
}
