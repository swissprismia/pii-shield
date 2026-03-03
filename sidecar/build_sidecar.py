#!/usr/bin/env python3
"""
Build script for creating a standalone Presidio sidecar binary using PyInstaller.

This script bundles the presidio_sidecar.py along with all dependencies
(Presidio, spaCy, and the language model) into a single executable.

Usage:
    python build_sidecar.py

Output:
    dist/presidio-sidecar (or presidio-sidecar.exe on Windows)
"""

import os
import sys
import subprocess
import platform
import shutil
from pathlib import Path


def get_spacy_model_paths():
    """Get paths to all available spaCy language models.

    English models are required; French models are optional (bundled if installed).
    Returns a list of (model_path, model_name) tuples for all found models.
    """
    import spacy

    # English models are required (primary language)
    english_models = ["en_core_web_lg", "en_core_web_md", "en_core_web_sm"]
    # French models are optional (for multi-language support)
    french_models = ["fr_core_news_lg", "fr_core_news_md", "fr_core_news_sm"]

    found = []
    for model_name in english_models + french_models:
        try:
            nlp = spacy.load(model_name)
            model_path = Path(nlp._path)
            found.append((model_path, model_name))
            print(f"Found spaCy model: {model_name} ({model_path})")
        except OSError:
            pass

    # Require at least one English model
    has_english = any(name in english_models for _, name in found)
    if not has_english:
        print("No English spaCy model found. Please install one:")
        print("  python -m spacy download en_core_web_lg")
        sys.exit(1)

    return found


def build_sidecar():
    """Build the sidecar executable using PyInstaller."""
    print("Building Presidio sidecar...")

    # Ensure we're in the sidecar directory
    script_dir = Path(__file__).parent
    os.chdir(script_dir)

    # Get all available spaCy model paths
    model_entries = get_spacy_model_paths()

    # PyInstaller command
    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--onefile",
        "--name", "presidio-sidecar",
        "--clean",
    ]

    # Add all available spaCy models as data
    for model_path, model_name in model_entries:
        cmd += ["--add-data", f"{model_path}{os.pathsep}{model_name}"]

    cmd += [
        # Hidden imports for Presidio and spaCy
        "--hidden-import", "presidio_analyzer",
        "--hidden-import", "presidio_anonymizer",
        "--hidden-import", "spacy",
        "--hidden-import", "spacy.lang.en",
        "--hidden-import", "thinc",
        "--hidden-import", "thinc.backends.numpy_ops",
        "--hidden-import", "cymem",
        "--hidden-import", "preshed",
        "--hidden-import", "murmurhash",
        "--hidden-import", "blis",
        "--hidden-import", "srsly",
        # Collect all submodules
        "--collect-all", "presidio_analyzer",
        "--collect-all", "presidio_anonymizer",
        "--collect-all", "spacy",
        # Main script
        "presidio_sidecar.py",
    ]

    print(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, check=False)

    if result.returncode != 0:
        print("PyInstaller build failed!")
        sys.exit(1)

    # Copy to Tauri binaries directory
    dist_path = script_dir / "dist" / "presidio-sidecar"
    if platform.system() == "Windows":
        dist_path = dist_path.with_suffix(".exe")

    tauri_binaries = script_dir.parent / "src-tauri" / "binaries"
    tauri_binaries.mkdir(parents=True, exist_ok=True)

    # Copy with platform-specific naming for Tauri
    target_triple = get_target_triple()
    target_name = f"presidio-sidecar-{target_triple}"
    if platform.system() == "Windows":
        target_name += ".exe"

    target_path = tauri_binaries / target_name

    if dist_path.exists():
        shutil.copy2(dist_path, target_path)
        print(f"Copied sidecar to: {target_path}")
    else:
        print(f"Warning: Built executable not found at {dist_path}")

    print("Build complete!")


def get_target_triple():
    """Get the Rust target triple for the current platform."""
    system = platform.system()
    machine = platform.machine().lower()

    if system == "Darwin":
        if machine == "arm64":
            return "aarch64-apple-darwin"
        return "x86_64-apple-darwin"
    elif system == "Windows":
        if machine == "amd64" or machine == "x86_64":
            return "x86_64-pc-windows-msvc"
        return "i686-pc-windows-msvc"
    elif system == "Linux":
        if machine == "aarch64":
            return "aarch64-unknown-linux-gnu"
        return "x86_64-unknown-linux-gnu"

    return "unknown"


if __name__ == "__main__":
    build_sidecar()
