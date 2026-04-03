# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

A **channel vocoder** CLI and web UI in Rust that simulates how cochlear implant users perceive music. It takes an audio file (WAV, MP3, FLAC, OGG/Vorbis, AAC, M4A), splits the spectrum into N logarithmically-spaced frequency bands (70–8,500 Hz), extracts amplitude envelopes via IIR bandpass filters, and reconstructs audio using noise or sine carriers — one per band. This destroys harmonic detail and timbre while preserving rhythm and timing.

## Build & Run

```bash
# Native CLI + local web server
cargo build
cargo run -- process --input song.wav --output sim.wav --channels 8
cargo run -- process --input song.mp3 --output sim.wav --channels 4 --verbose
cargo run -- serve                  # web UI at http://localhost:3000
cargo run -- serve --port 3001      # custom port
cargo test
cargo test <test_name>
cargo clippy

# WASM build (for GitHub Pages — requires wasm-pack)
wasm-pack build --target web --out-dir ../_site/pkg
# Then copy web/index.html to _site/index.html and serve _site/
```

## Dependencies

All targets:
```toml
hound      = "3.5"   # WAV write (encode_wav_bytes, write_wav)
rustfft    = "6.2"   # FFT — used only by the fft strategy
```

Native only (`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`):
```toml
clap       = { version = "4", features = ["derive"] }
axum       = { version = "0.8", features = ["multipart"] }
tokio      = { version = "1", features = ["rt-multi-thread", "macros"] }
symphonia  = { version = "0.5", features = ["mp3", "aac", "flac", "ogg", "vorbis", "wav", "pcm", "isomp4"] }
```

WASM only (`[target.'cfg(target_arch = "wasm32")'.dependencies]`):
```toml
wasm-bindgen = "0.2"
```

## Source Structure

The crate has both a `[lib]` and a `[[bin]]` target. The library exports the
vocoder/filter/bands modules and the WASM entry point; the binary adds the CLI
and local web server on top.

```
src/
├── lib.rs        # Library root: pub mod declarations + process_audio WASM entry point
├── main.rs       # Binary root: CLI entry point (`process` and `serve` subcommands)
├── server.rs     # axum web server (GET /, POST /simulate) — native only
├── index.html    # Local server UI — embedded via include_str! into server.rs
├── audio.rs      # Multi-format decode via Symphonia — native only (browser decodes in WASM build)
├── vocoder.rs    # CIS/FS4/FFT strategies + WAV encode (file and in-memory)
├── filter.rs     # Biquad IIR filter: bandpass (4th-order cascaded) + lowpass
└── bands.rs      # Log-spaced band edge + center frequency math

../web/
└── index.html    # GitHub Pages UI — loads WASM, decodes audio via Web Audio API
```

## GitHub Pages deployment

Handled automatically by `.github/workflows/pages.yml` on every push to `main`.
The workflow installs wasm-pack, compiles the lib to WASM (`wasm32-unknown-unknown`),
copies `web/index.html`, and deploys via `actions/deploy-pages`.

To enable: go to repo Settings → Pages → Source → **GitHub Actions**.

### Browser format support

The GitHub Pages build uses `AudioContext.decodeAudioData` for format decoding
instead of Symphonia:
- WAV, MP3: all browsers
- FLAC, OGG: Chrome and Firefox; not Safari
- AAC/M4A: Chrome, Safari, Edge; not Firefox

### Multi-channel downmix

Both paths average all channels arithmetically:
- Local server (Symphonia): `frame.iter().sum() / num_channels`
- GitHub Pages (JS): iterates `audioBuffer.getChannelData(c)` for all `c`

They produce identical mono output for the same input.

### Performance

Processing runs synchronously on the browser's main thread. `web/index.html`
yields via `setTimeout` before calling WASM so the loading spinner renders first.
Short clips (< 2 min) are fast; very long files may freeze the tab briefly.

### Carrier and channel defaults

The carrier is **always noise** in both web UIs (local server `POST /simulate`
and GitHub Pages). The sine carrier is only available via the CLI `--carrier sine`
flag. Reason: noise is the canonical CI simulation sound; sine is a secondary
comparison mode not exposed in the UI.

Default channel counts differ by entry point — intentional, not a bug:
- CLI: 8 channels (`--channels` default)
- Web UIs: 4 channels (pre-selected in the UI for a more dramatic first impression)

### Case sensitivity of strategy and carrier strings

The CLI (`main.rs`) lowercases strategy and carrier before matching, so
`--strategy CIS` and `--strategy cis` are equivalent.

`run_vocoder` / `process_audio` (the library and WASM entry point) match
**case-sensitively**: only `"cis"`, `"fs4"`, `"fft"`, `"noise"`, `"sine"` are
recognised. Anything else silently falls back to the default. The web UIs always
send lowercase, so this difference has no practical effect there.

## Algorithm

Three strategies share the same band layout but differ in how they extract envelopes and synthesize output.

**CIS (CLI default):** For each band — 4th-order IIR bandpass → full-wave rectify → 400 Hz Butterworth lowpass (envelope) → multiply by noise or sine carrier → sum all bands. Stereo input is downmixed to mono at load time.

**FS4 (web UI default):** Same as CIS for basal channels (center freq > 950 Hz). Apical channels (≤ 950 Hz) use a sine carrier whose frequency tracks zero-crossings of the filtered signal, preserving pitch in the low-frequency region.

**FFT:** Hann-windowed overlap-add (1024-sample frames, 512-sample hop). Band amplitude estimated via RMS of FFT bins; resynthesized as a pure sine. Kept for comparison — not a realistic CI simulation.

**Band spacing:** Logarithmic between 70 and 8,500 Hz: `f_low * (f_high / f_low) ^ (i / N)`

**Stereo input:** Downmix to mono at load time: `(left + right) / 2.0`
