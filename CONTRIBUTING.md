# Contributing to PII Shield

Thank you for your interest in contributing! This document covers how to set up your development environment, submit changes, and what to expect from the process.

## Table of Contents

- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Making Changes](#making-changes)
- [Commit Style](#commit-style)
- [Pull Request Process](#pull-request-process)
- [Code Standards](#code-standards)

---

## Development Setup

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| [Node.js](https://nodejs.org/) | 18+ | Frontend build |
| [Rust](https://rustup.rs/) | 1.70+ | Tauri core |
| [Python](https://python.org/) | 3.9+ | Presidio sidecar |
| Visual Studio Build Tools | Latest | Windows only — C++ toolchain |
| WebView2 Runtime | Latest | Windows only |
| Xcode Command Line Tools | Latest | macOS only |

### 1. Clone the repository

```bash
git clone https://github.com/swissprismia/pii-shield.git
cd pii-shield
```

### 2. Install Node.js dependencies

```bash
npm install
```

### 3. Set up the Python sidecar

```bash
cd sidecar
python3 -m venv venv

# Windows
venv\Scripts\activate

# macOS / Linux
source venv/bin/activate

pip install -r requirements.txt

# Download the English spaCy model (required)
python -m spacy download en_core_web_lg

# Optional: French model for multi-language support
python -m spacy download fr_core_news_lg

cd ..
```

### 4. Generate app icons

```bash
pip install Pillow
python scripts/generate_icons.py
```

### 5. Run in development mode

```bash
npm run tauri dev
```

The app starts with hot-reload for the frontend. The Python sidecar is launched automatically.

---

## Project Structure

```
app-presidio/
├── index.html              # Main HTML entry point
├── src/
│   ├── main.js             # Frontend JavaScript (UI logic, event handlers)
│   └── styles.css          # All UI styles
├── src-tauri/
│   ├── Cargo.toml          # Rust dependencies
│   ├── tauri.conf.json     # Tauri configuration
│   └── src/
│       ├── lib.rs          # Core: Tauri commands, app state, clipboard loop
│       ├── clipboard.rs    # Clipboard read/write, text hashing
│       ├── config.rs       # Config struct, load/save logic
│       ├── sidecar.rs      # Python sidecar IPC (stdin/stdout JSON)
│       └── window.rs       # Active window detection (platform-specific)
├── sidecar/
│   ├── presidio_sidecar.py # PII analysis engine (Presidio + spaCy)
│   ├── requirements.txt    # Python dependencies
│   └── build_sidecar.py    # PyInstaller packaging script
└── scripts/
    └── generate_icons.py   # Icon generation
```

---

## Making Changes

### Rust (src-tauri/)

Run the linter and formatter before submitting:

```bash
cd src-tauri
cargo fmt
cargo clippy -- -D warnings
cargo test
```

### Python (sidecar/)

```bash
cd sidecar
pip install ruff pytest
ruff check presidio_sidecar.py
# Run tests if available
pytest tests/ -v
```

### Frontend (src/)

The frontend uses vanilla JS with Vite. No framework required. Keep it simple.

---

## Building for Production

### Build the Python sidecar (required before packaging)

```bash
cd sidecar
pip install pyinstaller
python build_sidecar.py
cd ..
```

### Build the desktop app

```bash
npm run tauri build
```

Outputs are in `src-tauri/target/release/bundle/`.

---

## Commit Style

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short description>

[optional body]
```

**Types:**

| Type | When to use |
|------|-------------|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `test` | Adding or updating tests |
| `chore` | Build process, tooling, dependencies |
| `perf` | Performance improvement |

**Examples:**

```
feat(sidecar): add French language PII detection
fix(clipboard): prevent double-tokenization on rapid paste
docs: update development setup for macOS ARM
chore(deps): update presidio-analyzer to 2.2.x
```

---

## Pull Request Process

1. **Fork** the repository and create a branch from `main`:
   ```bash
   git checkout -b feat/your-feature-name
   ```

2. **Make your changes** following the code standards below.

3. **Test locally**: Run `cargo test`, `cargo clippy`, and manually verify the feature.

4. **Open a PR** against `main`. The PR template will guide you through the checklist.

5. **CI must pass**: All GitHub Actions checks (Rust lint, Python lint, build) must be green.

6. **One approval required** from a maintainer before merging.

7. PRs are merged with **squash and merge** to keep history clean.

---

## Code Standards

### Rust

- Follow standard Rust idioms (use `?` for error propagation, prefer `Option` over nullable patterns)
- No `unwrap()` in production paths — use `map_err`, `ok_or`, or `if let`
- Keep `lib.rs` focused on Tauri commands and state; push logic into dedicated modules
- All public functions need doc comments

### Python

- Follow [PEP 8](https://peps.python.org/pep-0008/) — enforced by `ruff`
- Use type hints for all function signatures
- Keep `presidio_sidecar.py` self-contained (no external module imports beyond requirements.txt)
- All communication over stdin/stdout must be newline-delimited JSON

### Frontend

- Vanilla JS only — no frameworks
- Keep UI logic in `src/main.js`, styles in `src/styles.css`
- Use Tauri event listeners (`__TAURI__.event.listen`) for backend-to-frontend communication
- Use Tauri commands (`__TAURI__.core.invoke`) for frontend-to-backend calls

---

## Reporting Issues

Please use the [GitHub Issue Tracker](https://github.com/swissprismia/pii-shield/issues) with the appropriate template.

For security vulnerabilities, see [SECURITY.md](SECURITY.md).
