#!/usr/bin/env python3
"""
Generate app icons for PII Shield.

This script creates simple shield-themed icons in various sizes
required by Tauri for different platforms.

Requires: Pillow (pip install Pillow)
"""

import os
from pathlib import Path

try:
    from PIL import Image, ImageDraw
except ImportError:
    print("Pillow not installed. Install with: pip install Pillow")
    print("Creating placeholder icons instead...")

    # Create minimal placeholder icons
    icons_dir = Path(__file__).parent.parent / "src-tauri" / "icons"
    icons_dir.mkdir(parents=True, exist_ok=True)

    # Create a simple 1x1 purple pixel as placeholder
    placeholder = bytes([
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,  # PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,  # IHDR chunk
        0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20,  # 32x32
        0x08, 0x06, 0x00, 0x00, 0x00, 0x73, 0x7A, 0x7A,
        0xF4, 0x00, 0x00, 0x00, 0x01, 0x73, 0x52, 0x47,
        0x42, 0x00, 0xAE, 0xCE, 0x1C, 0xE9,
    ])

    for name in ["icon.png", "32x32.png", "128x128.png", "128x128@2x.png"]:
        (icons_dir / name).write_bytes(placeholder)

    exit(0)


def create_shield_icon(size: int) -> Image.Image:
    """Create a shield icon with the PII Shield design."""
    # Create image with transparency
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Calculate dimensions
    padding = size // 8
    shield_width = size - 2 * padding
    shield_height = size - 2 * padding

    # Shield color (indigo)
    shield_color = (99, 102, 241, 255)  # #6366f1
    highlight_color = (129, 140, 248, 255)  # #818cf8

    # Draw shield shape (simplified)
    cx = size // 2
    top = padding
    bottom = size - padding
    left = padding
    right = size - padding

    # Shield polygon points
    points = [
        (cx, top),  # top center
        (right, top + shield_height // 4),  # top right
        (right, top + shield_height // 2),  # middle right
        (cx, bottom),  # bottom center (point)
        (left, top + shield_height // 2),  # middle left
        (left, top + shield_height // 4),  # top left
    ]

    draw.polygon(points, fill=shield_color)

    # Draw inner highlight (smaller shield)
    inner_padding = padding + size // 16
    inner_points = [
        (cx, inner_padding + size // 32),
        (right - size // 16, inner_padding + shield_height // 4),
        (right - size // 16, inner_padding + shield_height // 2 - size // 16),
        (cx, bottom - size // 8),
        (left + size // 16, inner_padding + shield_height // 2 - size // 16),
        (left + size // 16, inner_padding + shield_height // 4),
    ]

    # Draw checkmark or lock symbol
    check_color = (255, 255, 255, 255)
    line_width = max(2, size // 16)

    # Simple checkmark
    check_start = (cx - size // 6, cx + size // 16)
    check_mid = (cx - size // 16, cx + size // 6)
    check_end = (cx + size // 5, cx - size // 8)

    draw.line([check_start, check_mid], fill=check_color, width=line_width)
    draw.line([check_mid, check_end], fill=check_color, width=line_width)

    return img


def main():
    """Generate all required icon sizes."""
    script_dir = Path(__file__).parent
    icons_dir = script_dir.parent / "src-tauri" / "icons"
    icons_dir.mkdir(parents=True, exist_ok=True)

    # Icon sizes required by Tauri
    sizes = {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 512,  # General purpose
    }

    for filename, size in sizes.items():
        icon = create_shield_icon(size)
        icon_path = icons_dir / filename
        icon.save(icon_path, "PNG")
        print(f"Created: {icon_path}")

    # Create ICO file for Windows
    icon_512 = create_shield_icon(512)
    icon_256 = create_shield_icon(256)
    icon_128 = create_shield_icon(128)
    icon_64 = create_shield_icon(64)
    icon_48 = create_shield_icon(48)
    icon_32 = create_shield_icon(32)
    icon_16 = create_shield_icon(16)

    ico_path = icons_dir / "icon.ico"
    icon_512.save(
        ico_path,
        format="ICO",
        sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    )
    print(f"Created: {ico_path}")

    # Note: ICNS for macOS would require additional tooling
    print("\nNote: For macOS, you may need to create icon.icns manually")
    print("or use a tool like iconutil to convert from PNG")


if __name__ == "__main__":
    main()
