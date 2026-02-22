# Echo - Setup Guide

Echo is a voice-to-text application built with Tauri and React. This guide will help you set up transcription.

## Transcription Options

Echo supports two methods for speech-to-text transcription:

### Option 1: OpenAI API (Easiest)

Use OpenAI's Whisper API for cloud-based transcription.

**Setup:**
1. Get an API key from https://platform.openai.com/api-keys
2. Set environment variable:
   - **Windows CMD:** `setx OPENAI_API_KEY "your-api-key-here"`
   - **PowerShell:** `$env:OPENAI_API_KEY = "your-api-key-here"`
   - **Permanent (Windows):** Add to System Environment Variables

**Cost:** ~$0.006 per minute of audio

### Option 2: Local Whisper (Free, Private)

Run Whisper locally for offline, private transcription.

**Setup:**
1. Install whisper.cpp:
   ```bash
   git clone https://github.com/ggerganov/whisper.cpp
   cd whisper.cpp
   make
   ```

2. Download a model:
   ```bash
   bash ./models/download-ggml-model.sh base.en
   ```

3. Add whisper to your PATH:
   - Copy `main.exe` (or `main` on Linux/Mac) to a folder in PATH
   - Or add whisper.cpp folder to PATH

4. Models location:
   - Create `models` folder next to Echo executable
   - Place `ggml-base.en.bin` (or other model) there

**Models:**
- `tiny.en` - Fastest, less accurate (~75MB)
- `base.en` - Good balance (~142MB) **Recommended**
- `small.en` - Better accuracy (~466MB)
- `medium.en` - High accuracy (~1.5GB)

## Building Echo from Source

```bash
# Install dependencies
pnpm install

# Run in development
pnpm tauri dev

# Build for production
pnpm tauri build
```

## Features

- 🎙️ Audio recording with adjustable duration
- 🔊 High-quality WAV export (16kHz for Whisper)
- 🤖 AI transcription (local or cloud)
- ⌨️ Auto-paste to cursor position
- 🎨 Clean, modern UI

## Troubleshooting

**"No input device available"**
- Check microphone permissions in Windows Settings
- Ensure a microphone is connected

**"Whisper executable not found"**
- Install whisper.cpp OR set OPENAI_API_KEY

**"OPENAI_API_KEY not set"**
- Follow Option 1 setup above

**Compilation errors**
- Ensure Rust, Node.js, and VS Build Tools are installed
- Run in Developer Command Prompt (Windows)

## Technical Details

- **Backend:** Rust + Tauri
- **Frontend:** React + TypeScript
- **Audio:** cpal (cross-platform audio I/O)
- **Transcription:** OpenAI Whisper API / whisper.cpp
- **Text injection:** enigo (keyboard automation)

## License

This is a free, open-source clone of VoiceTypr for learning purposes.
