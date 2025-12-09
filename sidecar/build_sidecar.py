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


def get_spacy_model_path():
    """Get the path to the spaCy language model."""
    try:
        import spacy
        nlp = spacy.load("fr_core_news_lg")
        return Path(nlp._path)
    except OSError:
        # Try smaller models
        for model in ["fr_core_news_md", "fr_core_news_sm"]:
            try:
                nlp = spacy.load(model)
                print(f"Using spaCy model: {model}")
                return Path(nlp._path)
            except OSError:
                continue

        print("No spaCy model found. Please install one:")
        print("  python -m spacy download fr_core_news_lg")
        sys.exit(1)


def build_sidecar():
    """Build the sidecar executable using PyInstaller."""
    print("Building Presidio sidecar...")

    # Ensure we're in the sidecar directory
    script_dir = Path(__file__).parent
    os.chdir(script_dir)

    # Get spaCy model path
    model_path = get_spacy_model_path()
    model_name = model_path.name

    # Determine output name based on platform
    output_name = "presidio-sidecar"
    if platform.system() == "Windows":
        output_name = "presidio-sidecar.exe"

    # PyInstaller command
    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--onefile",
        "--name", "presidio-sidecar",
        "--clean",
        # Add spaCy model as data
        "--add-data", f"{model_path}{os.pathsep}{model_name}",
        # Hidden imports for Presidio and spaCy
        "--hidden-import", "presidio_analyzer",
        "--hidden-import", "presidio_anonymizer",
        "--hidden-import", "spacy",
        "--hidden-import", f"spacy.lang.en",
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
