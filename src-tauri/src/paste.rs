use arboard::Clipboard;
use rdev::{simulate, EventType, Key as RdevKey, SimulateError};
use std::thread;
use std::time::Duration;

pub fn paste_text(text: &str) -> Result<(), String> {
    // Initialize clipboard
    let mut clipboard =
        Clipboard::new().map_err(|e| format!("Failed to initialize clipboard: {}", e))?;

    // Set transcribed text as clipboard content
    clipboard
        .set_text(text)
        .map_err(|e| format!("Failed to set clipboard: {}", e))?;

    eprintln!("Set clipboard content: {}", text);

    // Longer delay to let user's target window regain focus
    // The Echo window has focus after recording, so we need to wait
    thread::sleep(Duration::from_millis(300));

    // Simulate Ctrl+V to paste
    paste_with_rdev().map_err(|e| format!("Failed to paste: {:?}", e))?;

    Ok(())
}

fn send_key_event(event_type: &EventType) -> Result<(), SimulateError> {
    match simulate(event_type) {
        Ok(()) => {
            // Let the OS catch up
            thread::sleep(Duration::from_millis(50));
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to send event {:?}: {:?}", event_type, e);
            Err(e)
        }
    }
}

fn paste_with_rdev() -> Result<(), SimulateError> {
    eprintln!("Starting Windows paste simulation with rdev");

    // Add initial delay for reliability
    thread::sleep(Duration::from_millis(50));

    // Try paste with retry logic
    for attempt in 1..=2 {
        eprintln!("Windows paste attempt {}/2", attempt);

        let result = (|| {
            send_key_event(&EventType::KeyPress(RdevKey::ControlLeft))?;
            send_key_event(&EventType::KeyPress(RdevKey::KeyV))?;
            send_key_event(&EventType::KeyRelease(RdevKey::KeyV))?;
            send_key_event(&EventType::KeyRelease(RdevKey::ControlLeft))?;
            Ok::<(), SimulateError>(())
        })();

        match result {
            Ok(_) => {
                eprintln!("Windows paste simulation completed on attempt {}", attempt);
                return Ok(());
            }
            Err(e) if attempt < 2 => {
                eprintln!("Windows paste attempt {} failed: {:?}, retrying...", attempt, e);
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("Windows paste failed after 2 attempts: {:?}", e);
                return Err(e);
            }
        }
    }

    unreachable!()
}
