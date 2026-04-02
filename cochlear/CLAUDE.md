# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

The Rust project is named `ci_music` and lives in `ci_music/`. Research papers are in `sources/`. Planning notes are in `ci_music/planning/conversation.md`.

The goal is a **channel vocoder** CLI and web UI: takes an audio file (WAV, MP3, FLAC, OGG/Vorbis, AAC, M4A), splits the spectrum into N logarithmically-spaced bands (70–8,500 Hz) using IIR bandpass filters, extracts amplitude envelopes, and reconstructs with noise or sine carriers. Destroys harmonic detail while preserving rhythm — simulating cochlear implant music perception. Three strategies: CIS (standard), FS4 (pitch-preserving apical channels), FFT (comparison baseline).

## Build & Run

```bash
cd ci_music
cargo build
cargo run -- process --input song.wav --output sim.wav --channels 8
cargo run -- process --input song.mp3 --output sim.wav --channels 4 --strategy fs4 --carrier sine --verbose
cargo run -- serve          # web UI at http://localhost:3000
cargo run -- serve --port 3001
cargo test
cargo test <test_name>      # run a single test
cargo clippy
```

CLI flags for `process`: `--channels` (default 8), `--strategy cis|fs4|fft` (default `cis`), `--carrier noise|sine` (default `noise`), `--verbose`.

## Architecture

Primary pipeline (CIS/FS4): load audio (via Symphonia) → downmix mono → 4th-order IIR bandpass per band → rectify → 400 Hz lowpass envelope → multiply carrier → sum → write WAV. FFT strategy uses Hann-windowed overlap-add (1024-sample frames, 512-sample hop) as a comparison baseline — not a realistic CI simulation.

**FS4 vs CIS:** Apical channels (center freq ≤ 950 Hz) use zero-crossing-driven sine carriers to preserve pitch; basal channels use standard CIS envelope coding.

Crates: `hound` (WAV write), `symphonia` (multi-format decode: WAV/MP3/FLAC/OGG/AAC/M4A), `rustfft` (FFT, used by fft strategy only), `clap` (CLI), `axum` + `tokio` (web server).

Source layout: `main.rs` (`process`/`serve` subcommands), `server.rs` (axum routes + `POST /simulate`), `index.html` (web UI, embedded via `include_str!`), `audio.rs` (multi-format decode via Symphonia), `vocoder.rs` (all three strategies + WAV encode), `filter.rs` (biquad IIR: 4th-order bandpass + lowpass), `bands.rs` (log-spaced band edge/center frequency math).
