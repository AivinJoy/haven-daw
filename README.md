# HAVEN DAW 🎵

A high-performance Digital Audio Workstation built with **Rust**, **Tauri**, and **SvelteKit**. 
Features a custom high-performance audio engine, non-destructive arrangement, and a real-time mixer.

![Version](https://img.shields.io/badge/version-0.1.0-blue) ![Rust](https://img.shields.io/badge/built_with-Rust-orange) ![Svelte](https://img.shields.io/badge/frontend-SvelteKit-red)

## 🚀 Features

* **Hybrid Audio Engine:** Built in Rust using `cpal` and `symphonia` for low-latency playback.
* **Visual Waveforms:** High-performance canvas rendering with smart caching (60fps).
* **Non-Destructive Arrangement:** Drag-and-drop clips, visual snapping, and resizing.
* **Real-time Mixer:** Volume, Pan, Mute, and Solo controls with absolute seeking.
* **BPM Sync:** Automatic BPM detection and grid alignment.

## 🛠️ Prerequisites

Before running the project, ensure you have the following installed:

1.  **Rust & Cargo:** [Install Rust](https://www.rust-lang.org/tools/install)
    ```bash
    curl --proto '=https' --tlsv1.2 -sSf [https://sh.rustup.rs](https://sh.rustup.rs) | sh
    ```
2.  **Node.js & npm (or pnpm):** [Install Node.js](https://nodejs.org/) (v16 or higher)
3.  **Tauri Dependencies:**
    * **Windows:** Install "C++ Build Tools" via Visual Studio Installer and the "WebView2" runtime.
    * **Linux:** `sudo apt install libwebkit2gtk-4.0-dev build-essential curl wget libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`
    * **macOS:** XCode Command Line Tools (`xcode-select --install`).

## 📦 Installation

1.  **Clone the repository:**
    ```bash
    git clone [https://github.com/YOUR_USERNAME/haven-daw.git](https://github.com/YOUR_USERNAME/haven-daw.git)
    cd haven-daw
    ```

2.  **Install Frontend Dependencies:**
    ```bash
    npm install
    # or
    pnpm install
    ```

3.  **Install Backend Dependencies:**
    ```bash
    cd src-tauri
    cargo check
    cd ..
    ```

## ▶️ Running Development Server

To start the app in development mode (with hot-reloading):

```bash
npm run tauri dev

The Frontend will load at localhost:1420.

The Rust Backend will compile and launch the native window.

🏗️ Building for Production
To create an optimized executable/installer for your OS:

Bash

npm run tauri build
The output will be in src-tauri/target/release/bundle/.

📂 Project Structure
src/ - Frontend (SvelteKit): UI components, Timeline logic, Canvas rendering.

src-tauri/ - Backend (Rust): Bridging, Window management, Command handlers.

daw_modules/ - Audio Engine: Core DSP, Decoder, Mixer, and Playback logic.

🐛 Common Issues
"No Audio Device Found": Ensure your OS default output device is active before launching.

"Waveform Mismatch": If the visual waveform looks misaligned, check that the sample rate matches the backend (default 44.1kHz or 48kHz).