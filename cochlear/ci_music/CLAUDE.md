# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A **channel vocoder** CLI and web UI in Rust that simulates how cochlear implant users perceive music. It takes an audio file (WAV, MP3, FLAC, OGG/Vorbis, AAC, M4A), splits the spectrum into N logarithmically-spaced frequency bands (70–8,500 Hz), extracts amplitude envelopes via IIR bandpass filters, and reconstructs audio using noise or sine carriers — one per band. This destroys harmonic detail and timbre while preserving rhythm and timing.

## Build & Run

```bash
cargo build
cargo run -- process --input song.wav --output sim.wav --channels 8
cargo run -- process --input song.mp3 --output sim.wav --channels 4 --verbose
cargo run -- serve                  # web UI at http://localhost:3000
cargo run -- serve --port 3001      # custom port
cargo test
cargo test <test_name>
cargo clippy
```

## Dependencies

```toml
hound      = "3.5"   # WAV write (encode_wav_bytes, write_wav)
rustfft    = "6.2"   # FFT — used only by the fft strategy
clap       = { version = "4", features = ["derive"] }
axum       = { version = "0.8", features = ["multipart"] }
tokio      = { version = "1", features = ["rt-multi-thread", "macros"] }
symphonia  = { version = "0.5", features = ["mp3", "aac", "flac", "ogg", "vorbis", "wav", "pcm", "isomp4"] }
```

## Source Structure

```
src/
├── main.rs       # CLI entry point: `process` and `serve` subcommands
├── server.rs     # axum web server (GET /, POST /simulate)
├── index.html    # Web UI — embedded via include_str! into server.rs
├── audio.rs      # Multi-format decode via Symphonia (WAV/MP3/FLAC/OGG/AAC/M4A)
├── vocoder.rs    # CIS/FS4/FFT strategies + WAV encode (file and in-memory)
├── filter.rs     # Biquad IIR filter: bandpass (4th-order cascaded) + lowpass
└── bands.rs      # Log-spaced band edge + center frequency math
```

## Algorithm

Three strategies share the same band layout but differ in how they extract envelopes and synthesize output.

**CIS (CLI default):** For each band — 4th-order IIR bandpass → full-wave rectify → 400 Hz Butterworth lowpass (envelope) → multiply by noise or sine carrier → sum all bands. Stereo input is downmixed to mono at load time.

**FS4 (web UI default):** Same as CIS for basal channels (center freq > 950 Hz). Apical channels (≤ 950 Hz) use a sine carrier whose frequency tracks zero-crossings of the filtered signal, preserving pitch in the low-frequency region.

**FFT:** Hann-windowed overlap-add (1024-sample frames, 512-sample hop). Band amplitude estimated via RMS of FFT bins; resynthesized as a pure sine. Kept for comparison — not a realistic CI simulation.

**Band spacing:** Logarithmic between 70 and 8,500 Hz: `f_low * (f_high / f_low) ^ (i / N)`

**Stereo input:** Downmix to mono at load time: `(left + right) / 2.0`
