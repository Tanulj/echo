import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type RecordingMode = "idle" | "timed" | "ptt";
type ProcessingState = "idle" | "recording" | "transcribing" | "pasting";
type Tab = "record" | "history" | "settings";

interface Settings {
  hotkey: string;
  model: string;
  silence_duration: number;
  auto_paste: boolean;
}

interface Model {
  id: string;
  name: string;
  size: string;
  speed: string;
  accuracy: string;
  downloaded: boolean;
}

interface HistoryEntry {
  id: string;
  text: string;
  timestamp: string;
  duration_secs: number;
}

function App() {
  const [activeTab, setActiveTab] = useState<Tab>("record");
  const [recordingMode, setRecordingMode] = useState<RecordingMode>("idle");
  const [processingState, setProcessingState] = useState<ProcessingState>("idle");
  const [duration, setDuration] = useState(10);
  const [transcription, setTranscription] = useState("");
  const [error, setError] = useState("");
  const [pttSeconds, setPttSeconds] = useState(0);
  const [audioLevel, setAudioLevel] = useState(0);
  const [settings, setSettings] = useState<Settings>({
    hotkey: "super+shift+r",
    model: "medium.en",
    silence_duration: 3,
    auto_paste: true,
  });
  const [models, setModels] = useState<Model[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [recordingStartTime, setRecordingStartTime] = useState<number | null>(null);
  const [permissions, setPermissions] = useState<{ accessibility: boolean; microphone: boolean } | null>(null);
  const [pasteHint, setPasteHint] = useState("");

  const recordingModeRef = useRef<RecordingMode>("idle");
  const pttTimerRef = useRef<number | null>(null);

  useEffect(() => {
    recordingModeRef.current = recordingMode;
  }, [recordingMode]);

  useEffect(() => {
    loadSettings();
    loadModels();
    loadHistory();
    checkPermissions();
  }, []);

  // Re-check permissions whenever the window regains focus (user may have just granted access)
  useEffect(() => {
    const onFocus = () => checkPermissions();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  useEffect(() => {
    const unlisten = listen<number>("audio-level", (event) => {
      setAudioLevel(event.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  async function doPaste(text: string) {
    try {
      const status = await invoke<string>("paste_text", { text });
      if (status === "copied") {
        setPasteHint("📋 Copied to clipboard — press ⌘V to paste");
        setTimeout(() => setPasteHint(""), 8000);
      } else {
        setPasteHint("");
      }
    } catch (err) {
      setError(`Paste error: ${err}`);
    }
  }

  async function loadSettings() {
    try {
      const s = await invoke<Settings>("get_settings");
      setSettings(s);
    } catch (_) {}
  }

  async function checkPermissions() {
    try {
      const p = await invoke<{ accessibility: boolean; microphone: boolean }>("check_permissions");
      setPermissions(p);
    } catch (_) {}
  }

  async function loadModels() {
    try {
      const m = await invoke<Model[]>("get_available_models");
      setModels(m);
    } catch (_) {}
  }

  async function loadHistory() {
    try {
      const h = await invoke<HistoryEntry[]>("get_history");
      setHistory(h);
    } catch (_) {}
  }

  async function saveSettings(newSettings: Settings) {
    try {
      await invoke("save_settings", { settings: newSettings });
      setSettings(newSettings);
    } catch (err) {
      setError(`Failed to save settings: ${err}`);
    }
  }

  async function downloadModel(modelId: string) {
    setDownloading(modelId);
    try {
      await invoke("download_model", { modelId });
      await loadModels();
    } catch (err) {
      setError(`Failed to download model: ${err}`);
    } finally {
      setDownloading(null);
    }
  }

  async function stopPttAndProcess(_reason: string) {
    if (recordingModeRef.current !== "ptt") return;

    if (pttTimerRef.current) {
      clearInterval(pttTimerRef.current);
      pttTimerRef.current = null;
    }

    const recordDuration = recordingStartTime ? (Date.now() - recordingStartTime) / 1000 : 0;
    setRecordingMode("idle");
    recordingModeRef.current = "idle";
    setProcessingState("transcribing");

    try {
      const filePath: string = await invoke("stop_ptt_recording");
      const text: string = await invoke("transcribe_audio", { filePath });
      setTranscription(text);

      if (text && text.trim().length > 0) {
        const entry: HistoryEntry = {
          id: Date.now().toString(),
          text: text.trim(),
          timestamp: new Date().toISOString(),
          duration_secs: recordDuration,
        };
        await invoke("add_to_history", { entry });
        loadHistory();

        if (settings.auto_paste) {
          setProcessingState("pasting");
          await doPaste(text);
        }
      }
    } catch (err) {
      setError(`Error: ${err}`);
    } finally {
      setProcessingState("idle");
      setAudioLevel(0);
    }
  }

  useEffect(() => {
    const unlistenPress = listen("ptt-pressed", async () => {
      if (recordingModeRef.current !== "idle") return;

      setRecordingMode("ptt");
      recordingModeRef.current = "ptt";
      setProcessingState("recording");
      setError("");
      setTranscription("");
      setPttSeconds(0);
      setRecordingStartTime(Date.now());

      pttTimerRef.current = window.setInterval(() => {
        setPttSeconds((s) => s + 1);
      }, 1000);

      try {
        await invoke("start_ptt_recording");
      } catch (err) {
        setError(`Error starting PTT: ${err}`);
        setRecordingMode("idle");
        recordingModeRef.current = "idle";
        setProcessingState("idle");
        if (pttTimerRef.current) {
          clearInterval(pttTimerRef.current);
          pttTimerRef.current = null;
        }
      }
    });

    const unlistenRelease = listen("ptt-released", () => stopPttAndProcess("key released"));
    const unlistenSilence = listen("ptt-silence-stop", () => stopPttAndProcess("silence detected"));

    return () => {
      unlistenPress.then((fn) => fn());
      unlistenRelease.then((fn) => fn());
      unlistenSilence.then((fn) => fn());
    };
  }, [settings.auto_paste]);

  async function startTimedRecording() {
    if (recordingModeRef.current !== "idle") return;

    setRecordingMode("timed");
    recordingModeRef.current = "timed";
    setProcessingState("recording");
    setError("");
    setTranscription("");

    try {
      const filePath: string = await invoke("record_audio", { durationSecs: duration });
      setRecordingMode("idle");
      recordingModeRef.current = "idle";
      setProcessingState("transcribing");

      const text: string = await invoke("transcribe_audio", { filePath });
      setTranscription(text);

      if (text && text.trim().length > 0) {
        const entry: HistoryEntry = {
          id: Date.now().toString(),
          text: text.trim(),
          timestamp: new Date().toISOString(),
          duration_secs: duration,
        };
        await invoke("add_to_history", { entry });
        loadHistory();

        if (settings.auto_paste) {
          setProcessingState("pasting");
          await doPaste(text);
        }
      }
    } catch (err) {
      setError(`Error: ${err}`);
    } finally {
      setRecordingMode("idle");
      recordingModeRef.current = "idle";
      setProcessingState("idle");
    }
  }

  async function copyFromHistory(text: string) {
    await doPaste(text);
  }

  async function clearAllHistory() {
    try {
      await invoke("clear_history");
      setHistory([]);
    } catch (err) {
      setError(`Failed to clear history: ${err}`);
    }
  }

  const isRecording = recordingMode !== "idle";
  const isBusy = processingState !== "idle";

  const getStatusText = (state: ProcessingState) => {
    switch (state) {
      case "recording": return "Recording...";
      case "transcribing": return "Transcribing...";
      case "pasting": return "Pasting...";
      default: return "Ready";
    }
  };

  // Manage overlay - show when busy, hide when idle
  useEffect(() => {
    const statusText = getStatusText(processingState);

    if (processingState !== "idle") {
      // Show overlay and update status
      invoke("show_overlay", { status: statusText }).catch(() => {});
    } else {
      // Hide overlay when completely done
      invoke("hide_overlay").catch(() => {});
    }
  }, [processingState]);

  return (
    <main className="container">
        <h1>Echo</h1>

        {/* Permission banners */}
        {permissions && !permissions.accessibility && (
          <div className="permission-banner">
            ⚠️ <strong>Accessibility permission required</strong> for auto-paste.
            <button onClick={() => invoke("open_accessibility_settings")}>Open Settings</button>
          </div>
        )}
        {permissions && !permissions.microphone && (
          <div className="permission-banner">
            ⚠️ <strong>Microphone permission required</strong> for recording.
            <button onClick={() => invoke("open_microphone_settings")}>Open Settings</button>
          </div>
        )}

        <div className="tabs">
          <button
            className={activeTab === "record" ? "active" : ""}
            onClick={() => setActiveTab("record")}
          >
            Record
          </button>
          <button
            className={activeTab === "history" ? "active" : ""}
            onClick={() => setActiveTab("history")}
          >
            History
          </button>
          <button
            className={activeTab === "settings" ? "active" : ""}
            onClick={() => setActiveTab("settings")}
          >
            Settings
          </button>
        </div>

        {activeTab === "record" && (
          <div className="tab-content">
            <div className={`level-meter-container ${isRecording ? "recording" : ""}`}>
              <div className="level-meter">
                <div
                  className={`level-meter-fill ${isRecording ? "active" : ""}`}
                  style={{ width: `${Math.max(audioLevel * 100, isRecording ? 5 : 0)}%` }}
                />
              </div>
              <div className="level-info">
                <span className={`level-label ${isBusy ? "recording" : ""}`}>
                  {getStatusText(processingState)}
                </span>
                {isRecording && <span className="level-value">{Math.round(audioLevel * 100)}%</span>}
              </div>
            </div>

            <div className="recording-status">
              {recordingMode === "ptt" && (
                <div className="ptt-indicator">
                  <span className="recording-dot" />
                  PTT Recording: {pttSeconds}s
                </div>
              )}
              {recordingMode === "timed" && (
                <div className="timed-indicator">
                  <span className="recording-dot" />
                  Recording for {duration}s...
                </div>
              )}
            </div>

            <div className="control-group">
              <label>
                Duration: {duration}s
                <input
                  type="range"
                  min="5"
                  max="60"
                  value={duration}
                  onChange={(e) => setDuration(Number(e.target.value))}
                  disabled={isBusy}
                />
              </label>
            </div>

            <button
              onClick={startTimedRecording}
              disabled={isBusy}
              className={isRecording ? "recording" : ""}
            >
              {isBusy ? getStatusText(processingState) : "Start Timed Recording"}
            </button>

            <p className="hotkey-hint">
              <strong>Push-to-Talk:</strong> Hold {settings.hotkey.toUpperCase()} to record
            </p>

            {error && <div className="error">{error}</div>}
            {pasteHint && <div className="paste-hint">{pasteHint}</div>}

            {/* Show processing indicator during transcription */}
            {processingState === "transcribing" && (
              <div className="processing-indicator">
                <div className="spinner"></div>
                <span>Transcribing audio...</span>
              </div>
            )}

            {transcription && processingState === "idle" && (
              <div className="transcription">
                <h3>Transcription:</h3>
                <textarea readOnly value={transcription} />
                <button onClick={() => doPaste(transcription)}>
                  Paste to Cursor
                </button>
              </div>
            )}
          </div>
        )}

        {activeTab === "history" && (
          <div className="tab-content">
            <div className="history-header">
              <h3>Transcription History</h3>
              {history.length > 0 && (
                <button className="clear-btn" onClick={clearAllHistory}>
                  Clear All
                </button>
              )}
            </div>

            {history.length === 0 ? (
              <p className="empty-state">No transcriptions yet</p>
            ) : (
              <div className="history-list">
                {history.map((entry) => (
                  <div key={entry.id} className="history-item">
                    <div className="history-meta">
                      <span>{new Date(entry.timestamp).toLocaleString()}</span>
                      <span>{entry.duration_secs.toFixed(1)}s</span>
                    </div>
                    <p className="history-text">{entry.text}</p>
                    <button onClick={() => copyFromHistory(entry.text)}>
                      Paste
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {activeTab === "settings" && (
          <div className="tab-content">
            <h3>Settings</h3>

            <div className="setting-group">
              <label>Push-to-Talk Hotkey</label>
              <input
                type="text"
                value={settings.hotkey}
                onChange={(e) => setSettings({ ...settings, hotkey: e.target.value.toLowerCase() })}
                placeholder="e.g., super+shift+r or ctrl+shift+r"
              />
              <small>Format: modifier+modifier+key (e.g., super+shift+r for Cmd+Shift+R on Mac)</small>
            </div>

            <div className="setting-group">
              <label>Silence Duration: {settings.silence_duration}s</label>
              <input
                type="range"
                min="1"
                max="10"
                step="0.5"
                value={settings.silence_duration}
                onChange={(e) => setSettings({ ...settings, silence_duration: Number(e.target.value) })}
              />
              <small>Auto-stop recording after this many seconds of silence</small>
            </div>

            <div className="setting-group checkbox">
              <label>
                <input
                  type="checkbox"
                  checked={settings.auto_paste}
                  onChange={(e) => setSettings({ ...settings, auto_paste: e.target.checked })}
                />
                Auto-paste transcription to cursor
              </label>
            </div>

            <button onClick={() => saveSettings(settings)}>Save Settings</button>

            <hr />

            <h3>Whisper Model</h3>
            <div className="models-list">
              {models.map((model) => (
                <div
                  key={model.id}
                  className={`model-item ${settings.model === model.id ? "selected" : ""}`}
                >
                  <div className="model-info">
                    <strong>{model.name}</strong>
                    <span className="model-details">
                      {model.size} | {model.speed} | {model.accuracy}
                    </span>
                  </div>
                  <div className="model-actions">
                    {model.downloaded ? (
                      <button
                        className={settings.model === model.id ? "active" : ""}
                        onClick={() => saveSettings({ ...settings, model: model.id })}
                      >
                        {settings.model === model.id ? "Active" : "Use"}
                      </button>
                    ) : (
                      <button
                        onClick={() => downloadModel(model.id)}
                        disabled={downloading !== null}
                      >
                        {downloading === model.id ? "Downloading..." : "Download"}
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>

            {error && <div className="error">{error}</div>}
          </div>
        )}
    </main>
  );
}

export default App;
