# HAVEN DAW 🎵

A high-performance Digital Audio Workstation built with **Rust**, **Tauri**, and **SvelteKit**. 
Features a custom high-performance audio engine, non-destructive arrangement, an integrated AI Music Director, and a real-time lock-free mixer.

![Version](https://img.shields.io/badge/version-0.1.0-blue) ![Rust](https://img.shields.io/badge/built_with-Rust-orange) ![Svelte](https://img.shields.io/badge/frontend-Svelte 5-red)

## 🚀 Key Features

* **Hybrid Audio Engine:** Built in Rust using `cpal` and `symphonia` for low-latency playback. Employs lock-free multi-producer, single-consumer (MPSC) channels for seamless UI-to-engine thread communication.
* **AI Music Director:** An intelligent, context-aware chatbot capable of executing complex audio commands naturally (e.g., "Merge clip 1 and 2", "Apply a high-pass EQ to track 1", "Mute the drums", or "Separate stems").
* **Real-Time Device Hot-Swapping:** Dynamically detects OS hardware changes. Plucking headphones out or switching default outputs gracefully re-hooks the audio thread without crashing the application.
* **Non-Destructive Clip Editing:** Split, merge, delete, trim, and move clips dynamically. The Rust backend maintains true contiguous source durations for instant timeline recovery.
* **Instantaneous Undo/Redo System:** Built entirely on a Command Manager pattern. Heavy file operations (like restoring deleted audio) execute in under a millisecond via RAM caching.
* **Integrated Recording Pipeline:** Multi-track audio recording with live waveform generation and hardware monitoring controls.
* **Real-time Mixer & FX:** Dynamic volume, parametric EQ, and panning nodes. Gain staging includes accurate Peak and RMS metering shared instantly via lock-free atomic buffers.
* **High-Performance Visuals:** 60fps canvas waveform rendering mapped smoothly across Svelte 5 Reactive Runes (`$state`, `$derived`).

## 🛠️ Prerequisites

Before running the project, ensure you have the following installed:

1.  **Rust & Cargo:** [Install Rust](https://www.rust-lang.org/tools/install)
    ```bash
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
    ```
2.  **Node.js & Package Manager:** [Install Node.js](https://nodejs.org/) (v16 or higher).
3.  **Tauri OS Dependencies:**
    * **Windows:** Install "C++ Build Tools" via Visual Studio Installer and the "WebView2" runtime.
    * **Mac/Linux:** See [Tauri Prerequisites](https://tauri.app/v1/guides/getting-started/prerequisites/).

## 📦 Installation

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/AivinJoy/haven-daw.git
    cd haven
    ```

2.  **Install Dependencies:**
    ```bash
    npm install
    # or
    pnpm install
    ```

## ▶️ Running Development Server

To start the app in development mode (with hot-reloading):

```bash
# In the root 'haven' directory
npm run tauri dev