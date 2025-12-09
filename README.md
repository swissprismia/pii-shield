# PII Shield

A lightweight desktop application that automatically detects and anonymizes sensitive information before users paste it into AI assistants.

## Features

- **Clipboard Monitoring**: Automatically detects when text is copied
- **PII Detection**: Uses Microsoft Presidio to identify sensitive information
- **Anonymization**: Replaces PII with safe placeholders like `[PERSON]`, `[EMAIL]`
- **System Tray**: Runs quietly in the background
- **Toast Notifications**: Alerts when PII is detected
- **Cross-Platform**: Supports Windows and macOS

## Detected PII Types

| Personal | Financial | Technical |
|----------|-----------|-----------|
| Person names | Credit cards | IP addresses |
| Email addresses | IBAN codes | URLs |
| Phone numbers | Bank numbers | Domain names |
| Locations | SSN | NRP |
| Date/Time | Passport | Medical License |

## Architecture

```
┌─────────────────────────────────────────────────┐
│            Tauri Desktop App                    │
│  ┌───────────────────────────────────────────┐  │
│  │  Rust Core                                │  │
│  │  • Clipboard watcher (arboard)            │  │
│  │  • Active window detector                 │  │
│  │  • Sidecar manager                        │  │
│  └───────────────────────────────────────────┘  │
│                      ↕                          │
│  ┌───────────────────────────────────────────┐  │
│  │  Python Sidecar (PyInstaller bundle)      │  │
│  │  • Presidio Analyzer                      │  │
│  │  • Presidio Anonymizer                    │  │
│  │  • spaCy NLP model                        │  │
│  └───────────────────────────────────────────┘  │
│                      ↕                          │
│  ┌───────────────────────────────────────────┐  │
│  │  WebView UI                               │  │
│  │  • System tray icon                       │  │
│  │  • Toast notifications                    │  │
│  │  • Quick-action popup                     │  │
│  └───────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

## Prerequisites

- **Node.js** 18+ and npm
- **Rust** 1.70+ with Cargo
- **Python** 3.9+ (for sidecar development/building)
- Platform-specific requirements:
  - **Windows**: Visual Studio Build Tools, WebView2
  - **macOS**: Xcode Command Line Tools

## Development Setup

### 1. Install Dependencies

```bash
# Install Node.js dependencies
npm install

# Install Rust dependencies (handled by Cargo)
cd src-tauri && cargo fetch && cd ..
```

### 2. Set Up Python Sidecar (Optional - for full Presidio support)

```bash
# Create virtual environment
cd sidecar
python3 -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install dependencies
pip install -r requirements.txt

# Download spaCy model
python -m spacy download fr_core_news_lg

# Return to project root
cd ..
```

### 3. Generate Icons

```bash
# Requires Pillow
pip install Pillow
python scripts/generate_icons.py
```

### 4. Run in Development Mode

```bash
npm run tauri dev
```

The app will start with hot-reload enabled for the frontend.

## Building for Production

### Build the Sidecar

```bash
cd sidecar
pip install pyinstaller
python build_sidecar.py
cd ..
```

### Build the Desktop App

```bash
npm run tauri build
```

The built application will be in `src-tauri/target/release/bundle/`.

## User Flow

1. **Copy**: User copies text from any application
2. **Detect**: App detects clipboard change
3. **Analyze**: Presidio scans for PII entities
4. **Alert**: Toast notification shows detected PII count
5. **Anonymize**: User clicks to replace clipboard with anonymized text
6. **Paste**: User pastes safely into AI assistant

## Configuration

The app runs with sensible defaults for the PoC. Configuration options will be added in future versions.

## Project Structure

```
app-presidio/
├── index.html              # Main HTML entry point
├── package.json            # Node.js dependencies
├── vite.config.js          # Vite bundler config
├── src/
│   ├── main.js             # Frontend JavaScript
│   └── styles.css          # UI styles
├── src-tauri/
│   ├── Cargo.toml          # Rust dependencies
│   ├── tauri.conf.json     # Tauri configuration
│   ├── icons/              # App icons
│   └── src/
│       ├── main.rs         # App entry point
│       ├── lib.rs          # Core Tauri setup
│       ├── clipboard.rs    # Clipboard monitoring
│       ├── sidecar.rs      # Presidio sidecar management
│       └── window.rs       # Active window detection
├── sidecar/
│   ├── presidio_sidecar.py # Python PII analyzer
│   ├── requirements.txt    # Python dependencies
│   └── build_sidecar.py    # PyInstaller build script
└── scripts/
    └── generate_icons.py   # Icon generation script
```

## License

MIT License - see LICENSE file for details.

## Acknowledgments

- [Tauri](https://tauri.app/) - Desktop app framework
- [Microsoft Presidio](https://microsoft.github.io/presidio/) - PII detection engine
- [spaCy](https://spacy.io/) - NLP library
