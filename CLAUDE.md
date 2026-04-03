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

# WASM build for GitHub Pages (requires wasm-pack)
wasm-pack build --target web --out-dir ../_site/pkg
```

CLI flags for `process`: `--channels` (default 8), `--strategy cis|fs4|fft` (default `cis`), `--carrier noise|sine` (default `noise`), `--verbose`.

## Architecture

The `ci_music` crate has two targets:
- **`[lib]`** (`src/lib.rs`): exports vocoder/filter/bands modules and the `process_audio` WASM entry point
- **`[[bin]]`** (`src/main.rs`): CLI + local axum web server, depends on the lib

Primary pipeline (CIS/FS4): load audio → downmix mono → 4th-order IIR bandpass per band → rectify → 400 Hz lowpass envelope → multiply carrier → sum → write WAV. FFT strategy uses Hann-windowed overlap-add (1024-sample frames, 512-sample hop) as a comparison baseline — not a realistic CI simulation.

**FS4 vs CIS:** Apical channels (center freq ≤ 950 Hz) use zero-crossing-driven sine carriers to preserve pitch; basal channels use standard CIS envelope coding.

**Two UIs:**
- `src/index.html` — local server UI, served by axum via `include_str!`; POSTs audio to `POST /simulate`, which decodes via Symphonia and processes server-side
- `web/index.html` — GitHub Pages UI; loads the WASM lib, decodes audio via `AudioContext.decodeAudioData`, processes entirely in-browser via WASM

**GitHub Pages deployment:** `.github/workflows/pages.yml` builds with wasm-pack and deploys on every push to `main`. Enable in repo Settings → Pages → Source → GitHub Actions.

**Multi-channel downmix:** Both the local server (Symphonia) and the GitHub Pages build (Web Audio API + JS) downmix to mono by averaging all channels arithmetically, producing identical results for the same input.

**Carrier:** Always noise in both web UIs; sine is CLI-only (`--carrier sine`). **Default channels:** CLI defaults to 8; both web UIs default to 4 (more dramatic first impression). **Case sensitivity:** CLI lowercases strategy/carrier before matching; `run_vocoder`/`process_audio` match case-sensitively — web UIs always send lowercase so no practical difference.

Crates: `hound` + `rustfft` (all targets), `symphonia` + `clap` + `axum` + `tokio` (native only), `wasm-bindgen` (WASM only).

Source layout: `lib.rs` (library root + WASM entry point), `main.rs` (`process`/`serve` subcommands), `server.rs` (axum routes + `POST /simulate`), `index.html` (local server UI), `audio.rs` (Symphonia decode, native only), `vocoder.rs` (all three strategies + WAV encode), `filter.rs` (biquad IIR), `bands.rs` (log-spaced frequencies). GitHub Pages source: `web/index.html`.
