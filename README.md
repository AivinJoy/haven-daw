# Haven DAW

Haven is a modern, AI-assisted Digital Audio Workstation built with a focus on performance, simplicity, and intelligent workflow. The goal of this project is to combine the precision of traditional audio engineering with the flexibility of AI, so creators can spend less time tweaking and more time creating.

The application is powered by a Rust-based audio engine for speed and reliability, with a SvelteKit frontend wrapped using Tauri for a lightweight desktop experience.

---

## Features

### High-Performance Audio Engine

Haven uses a native Rust audio engine designed for low-latency playback, recording, and processing. It is built on top of `cpal` and `symphonia` to ensure stability across platforms.

### AI Mixing Assistant

You can interact with Haven using natural language. Instead of manually setting up effect chains, you can give simple instructions like:

> "Master this track to studio quality"

The system interprets your request and automatically applies EQ, compression, and reverb based on context.

### AI Stem Separation

Haven allows you to separate vocals, drums, bass, and other elements directly inside the timeline. This is powered by ONNX models running locally, so your audio stays on your system.

### DSP Effects

The DAW includes built-in effects such as:

* Parametric EQ
* Dynamic range compressor
* Algorithmic reverb

### Metering

Real-time peak, RMS, and hold metering is available through a dedicated master bus.

### Automation

Volume automation lanes allow precise control over dynamics during playback.

### Project Management

Projects can be saved, loaded, and exported. Waveforms are cached to improve performance and reduce load times.

---

## Tech Stack

**Frontend**

* Svelte 5
* SvelteKit
* Tailwind CSS
* Vite

**Backend / Desktop**

* Tauri
* Rust

**Audio DSP**

* daw_modules (custom)
* cpal
* rustfft
* biquad

**AI / ML**

* ONNX Runtime (ort)
* stem-splitter-core
* reqwest (Groq API integration)

---

## Installation

### Prerequisites

* Node.js (v18 or higher)
* npm
* Rust & Cargo (latest stable via [https://rustup.rs/](https://rustup.rs/))

---

### Linux Requirements

For Linux systems, install the required dependencies for Tauri and audio:

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.0-dev \
    build-essential \
    curl \
    wget \
    file \
    libssl-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    alsa-base \
    alsa-utils \
    libasound2-dev
```

If you are using Ubuntu 24.04 or newer, you may need `libwebkit2gtk-4.1-dev` instead.

---

## Getting Started

### Clone the repository

```bash
git clone https://github.com/AivinJoy/haven-daw.git
cd haven-daw/haven
```

### Install dependencies

```bash
npm install
```

### Environment setup

Create a `.env` file inside:

```
haven/src-tauri/
```

Add:

```env
GROQ_API_KEY=your_groq_api_key_here
```

### Run in development

```bash
npm run tauri dev
```

### Build for production

```bash
npm run tauri build
```