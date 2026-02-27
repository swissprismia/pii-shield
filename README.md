# PII Shield

[![CI](https://github.com/swissprismia/pii-shield/actions/workflows/ci.yml/badge.svg)](https://github.com/swissprismia/pii-shield/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/swissprismia/pii-shield)](https://github.com/swissprismia/pii-shield/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-lightgrey)](https://github.com/swissprismia/pii-shield/releases)

**The only native desktop app that protects your privacy system-wide — in every app, not just browsers.**

PII Shield sits in your system tray, silently monitoring your clipboard. The moment you copy text containing personal information, it detects and tokenizes PII using real NLP — then automatically replaces the clipboard before you paste into any AI assistant. When you copy the AI's response back, it restores your original data. 100% offline. Zero data leaves your machine.

---

## Why PII Shield?

| Tool | System-wide | Real NLP | Round-trip tokenization | Offline |
|------|:-----------:|:--------:|:-----------------------:|:-------:|
| **PII Shield** | ✅ | ✅ Presidio + spaCy | ✅ | ✅ |
| Browser extensions | ❌ Browser only | Varies | Partial | Varies |
| Enterprise agents | ✅ | ✅ | ❌ | ❌ Cloud |
| ChatGPT redactors | ❌ | ❌ Regex | ❌ | ✅ |

PII Shield is the only native system-tray app that works with **any app** (terminal, desktop apps, not just browsers), uses **real NLP** for high accuracy, and has a true **tokenize/de-tokenize round-trip** — all completely offline.

---

## How It Works

```
You copy:   "John Doe's email is john.doe@example.com"
                          ↓
PII Shield: Detects PERSON + EMAIL_ADDRESS via Presidio NLP
                          ↓
Clipboard:  "[FirstName1] [LastName1]'s email is [Email1]"
                          ↓
You paste into ChatGPT (or any AI) — no real PII sent
                          ↓
AI responds: "Hi [FirstName1], I'll send details to [Email1]"
                          ↓
You copy AI response — PII Shield detects tokens
                          ↓
Clipboard:  "Hi John, I'll send details to john.doe@example.com"
```

---

## Features

- **System-wide clipboard monitoring** — works in every app: terminals, IDEs, desktop apps, browsers
- **Real NLP detection** — Microsoft Presidio + spaCy, not just regex
- **Round-trip tokenization** — `[FirstName1]`, `[Email1]`, etc. automatically restored on copy-back
- **Secret detection** — catches API keys, AWS credentials, GitHub tokens, JWTs, private keys
- **Multi-language** — English and French PII detection out of the box
- **Settings UI** — configure monitored apps without editing JSON
- **History log** — in-memory session audit log (never written to disk)
- **Auto-paste protection** — intercepts Ctrl+V and right-click paste in configured apps
- **System tray** — runs silently in the background, zero interruptions
- **100% offline** — no telemetry, no cloud, no data leaves your machine

---

## Detected PII Types

| Personal | Financial | Technical | Secrets |
|----------|-----------|-----------|---------|
| Person names (first/last/middle) | Credit cards | IP addresses | OpenAI / Anthropic API keys |
| Email addresses | IBAN codes | URLs | AWS Access Keys |
| Phone numbers | Bank account numbers | Domain names | GitHub tokens |
| Locations | — | — | JWT tokens |
| Date / Time | — | — | Private keys (PEM) |
| Swiss AVS / AHV numbers | — | — | Generic API keys (32+ chars) |
| SSN, Passport, Medical License | — | — | — |

---

## Installation

### Download a Pre-Built Release (Recommended)

1. Go to the [Releases page](https://github.com/swissprismia/pii-shield/releases/latest)
2. Download the installer for your platform:
   - **Windows**: `PII.Shield_x.x.x_x64-setup.exe`
   - **macOS**: `PII.Shield_x.x.x_universal.dmg` (Apple Silicon + Intel)
3. Run the installer
4. Launch PII Shield — it will appear in your system tray

> **Note**: The app bundles a pre-compiled Python sidecar. You do not need Python installed.

### macOS Gatekeeper

On first launch on macOS, right-click the app and select **Open** to bypass Gatekeeper (the app is not notarized in pre-release builds).

---

## Configuration

PII Shield works out of the box. Use the **Settings panel** (click the tray icon → Settings tab) to customize:

- Which apps trigger auto-tokenization
- PII confidence threshold
- Language (English / French)
- Enable/disable monitoring

Advanced users can also edit `config.json` directly:

```json
{
  "language": "en",
  "score_threshold": 0.5,
  "auto_anonymize": {
    "browsers": ["chrome", "firefox", "edge", "safari", "brave", "opera", "vivaldi", "arc"],
    "ai_assistants": ["chatgpt", "claude", "gemini", "copilot", "openai", "anthropic", "perplexity", "poe"],
    "custom_apps": []
  }
}
```

The config is created automatically on first run. Add any app name or window title keyword to `custom_apps` to protect it.

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│            Tauri Desktop App (Rust)             │
│  ┌───────────────────────────────────────────┐  │
│  │  Core (lib.rs)                            │  │
│  │  • Clipboard polling loop (500ms)         │  │
│  │  • Global input listener (Ctrl+V, RClick) │  │
│  │  • Token vault (in-memory round-trip)     │  │
│  │  • Session history log                    │  │
│  └───────────────────────────────────────────┘  │
│                      ↕ JSON over stdin/stdout   │
│  ┌───────────────────────────────────────────┐  │
│  │  Python Sidecar (PyInstaller bundle)      │  │
│  │  • Presidio Analyzer + spaCy NLP          │  │
│  │  • Swiss AVS recognizer (custom)          │  │
│  │  • Secrets recognizer (regex patterns)    │  │
│  │  • Fallback regex (Presidio unavailable)  │  │
│  └───────────────────────────────────────────┘  │
│                      ↕ Tauri events/commands    │
│  ┌───────────────────────────────────────────┐  │
│  │  WebView UI (Vanilla JS)                  │  │
│  │  • Dashboard: scan stats, detected PII    │  │
│  │  • Token vault display                    │  │
│  │  • Settings panel                         │  │
│  │  • History / audit log                    │  │
│  └───────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

---

## Development Setup

See [CONTRIBUTING.md](CONTRIBUTING.md) for full setup instructions.

**Quick start:**

```bash
# Install Node.js deps
npm install

# Set up Python sidecar
cd sidecar && python3 -m venv venv && source venv/bin/activate
pip install -r requirements.txt
python -m spacy download en_core_web_lg
cd ..

# Run in dev mode (hot reload)
npm run tauri dev
```

### Prerequisites

- Node.js 18+, Rust 1.70+, Python 3.9+
- Windows: Visual Studio Build Tools, WebView2
- macOS: Xcode Command Line Tools

---

## Project Structure

```
app-presidio/
├── index.html              # Main HTML entry point
├── src/
│   ├── main.js             # Frontend: UI logic, event listeners, state
│   └── styles.css          # All UI styles
├── src-tauri/
│   ├── Cargo.toml          # Rust dependencies
│   ├── tauri.conf.json     # Tauri configuration
│   └── src/
│       ├── lib.rs          # Core: Tauri commands, app state, clipboard loop
│       ├── clipboard.rs    # Clipboard read/write, text hashing
│       ├── config.rs       # Config struct with load/save
│       ├── sidecar.rs      # Python sidecar IPC (stdin/stdout JSON)
│       └── window.rs       # Active window detection (platform-specific)
├── sidecar/
│   ├── presidio_sidecar.py # PII engine: analyze, tokenize, detokenize
│   ├── requirements.txt    # Python dependencies
│   └── build_sidecar.py   # PyInstaller packaging script
└── scripts/
    └── generate_icons.py   # Icon generation
```

---

## Security & Privacy

- All analysis is performed **locally** using spaCy NLP models bundled with the app
- No internet connection required, no API calls to external services
- Clipboard contents are **never written to disk** (processed in memory only)
- The history log is **in-memory only** and cleared on exit
- See [SECURITY.md](SECURITY.md) for the vulnerability reporting policy

---

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a PR.

- Bug reports: [GitHub Issues](https://github.com/swissprismia/pii-shield/issues) (use the bug report template)
- Feature requests: [GitHub Issues](https://github.com/swissprismia/pii-shield/issues) (use the feature request template)
- Security vulnerabilities: see [SECURITY.md](SECURITY.md) for private disclosure

---

## Contributors

<!-- ALL-CONTRIBUTORS-LIST:START -->
<!-- ALL-CONTRIBUTORS-LIST:END -->

---

## Acknowledgments

- [Tauri](https://tauri.app/) — Desktop app framework (Rust + WebView)
- [Microsoft Presidio](https://microsoft.github.io/presidio/) — PII detection and anonymization engine
- [spaCy](https://spacy.io/) — Industrial-strength NLP library

---

## License

[MIT](LICENSE) — Copyright (c) 2026 PII Shield Contributors
