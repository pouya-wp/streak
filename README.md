<div align="center">

# ⚡ Streak

### System-Wide Real-Time GPU Cursor Effects for Windows

[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%7C%2011-0078D6?style=flat-square&logo=windows)](https://microsoft.com)
[![CI](https://github.com/pouya-wp/streak/actions/workflows/ci.yml/badge.svg)](https://github.com/pouya-wp/streak/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-DirectX%2011-DEA584?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE)
[![Author](https://img.shields.io/badge/Author-Pouya%20Sadeghpour-181717?style=flat-square&logo=github)](https://github.com/pouya-wp)

<p align="center">
  <b>Streak</b> brings fluid desktop aesthetics and high-end motion graphics to Windows. Powered by hardware-accelerated Direct3D 11, DirectComposition, and sub-frame mouse polling, it renders silky-smooth motion blur, ribbon trails, particle bursts, and ripple effects with zero desktop lag.
</p>

</div>

---

## ✨ Features

- 🏎️ **True Sub-Frame Motion Blur** — High-frequency velocity vector tracking with physical decay.
- 🎀 **Ribbon & Plasma Trails** — Custom-textured ribbon strips that follow fast cursor sweeps.
- 💥 **Click Ripples & Particles** — DirectComposition click triggers with customizable particle dispersal.
- 🔍 **Shake-to-Find** — Rapid cursor magnification when shaking your mouse.
- ⚡ **Dirty-Rect Frame Pacing** — Only repaints modified screen bounds; consumes <0.3% CPU at idle.
- 🎛️ **Live Settings UI** — Real-time tweaking via `egui` with presets (*Soft Pop*, *Cyber*, *Minimal*, *Vapor*).
- 🛡️ **Safe Cursor Restoration** — Low-level hooks with emergency auto-recovery and tray controls.

---

## 🏗️ Architecture

```
Streak Architecture
├── Low-Level Input Hook (Sub-frame path sampling via RawInput)
├── Direct3D 11 + HLSL Pixel Shader Pipeline (Dirty-rect presentation)
├── DirectComposition Visual Tree (Click-through transparent overlay)
└── Egui Configuration Window (Live preset switching & benchmark harness)
```

| Component | Responsibility | Technology |
|:---|:---|:---|
| `render.rs` / `shaders.hlsl` | D3D11 swapchain, dirty rect presents, alpha blending | Direct3D 11, HLSL |
| `effects.rs` | Velocity vector math, ribbon mesh generation, particle physics | Rust (Math/SIMD) |
| `overlay.rs` | Transparent fullscreen window, message loop, tray icon | Win32 API, DirectComposition |
| `input.rs` | Raw mouse hooks, sub-millisecond polling, movement interpolation | User32 Raw Input |
| `settings_ui.rs` | Immediate-mode GUI for live parameters and preset saving | egui |

---

## 🚀 Quick Start

### Prerequisites
- Windows 10 (version 1903+) or Windows 11
- Rust 1.75+ (MSVC toolchain)
- Windows SDK (for `rc.exe` resource compilation)

### Building from Source

```powershell
# Clone the repository
git clone https://github.com/pouya-wp/streak.git
cd streak

# Build the release binary and standalone installer
.\build.ps1
```

The compiled installer will be generated under `dist\StreakSetup.exe`.

---

## 💻 CLI Usage

```powershell
# Open live settings window
Streak.exe --settings

# Run synthetic performance benchmark
Streak.exe --bench 10 "Vapor Preset"

# Emergency cursor restore
Streak.exe --restore-cursor

# Silent background install
StreakSetup.exe /S
```

---

## 📄 License

Distributed under the MIT License.

---

<div align="center">
  <sub>Crafted with precision & passion by <b><a href="https://github.com/pouya-wp">Pouya Sadeghpour</a></b></sub>
</div>
