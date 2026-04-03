# ci_music — Cochlear Implant Music Simulator

A channel vocoder that simulates how cochlear implant (CI) users perceive music. Feed it an audio file and it returns a degraded version that preserves rhythm and timing while destroying harmonic detail and timbre — mirroring the perceptual limitations of CI devices.

## Background

Cochlear implants work by mapping sound to a small number of electrode channels implanted in the cochlea. Modern devices typically use 8–22 channels covering roughly 70–8,500 Hz. This channel limitation is what makes music so difficult for CI users: pitch, harmonics, and timbre are largely lost, while rhythm and loudness contour survive. This tool makes that experience audible.

See `sources/` for relevant audiology literature.

## Install

```bash
git clone <repo>
cd ci_music
cargo build --release
```

## Usage

### CLI

```bash
cargo run -- process --input song.wav --output sim.wav --channels 8
cargo run -- process --input song.mp3 --output sim.wav --channels 4 --strategy fs4 --carrier sine --verbose
```

Supported input formats: WAV, MP3, FLAC, OGG/Vorbis, AAC, M4A.

**Flags:**

| Flag | Default | Description |
|---|---|---|
| `--channels` | `8` | Number of frequency channels (4 = very degraded, 16 = closer to normal) |
| `--strategy` | `cis` | `cis` (standard), `fs4` (pitch-preserving apical channels), `fft` (comparison baseline) |
| `--carrier` | `noise` | `noise` (classic CI buzzy sound) or `sine` (tonal) |
| `--verbose` | off | Print timing and processing details |

### Web UI

```bash
cargo run -- serve          # http://localhost:3000
cargo run -- serve --port 3001
```

Upload a WAV, MP3, FLAC, or OGG file, adjust channels and strategy, and compare original vs simulated audio in the browser.

## How It Works

**Primary pipeline (CIS/FS4):**

```
audio in → downmix mono → 4th-order IIR bandpass (per band)
         → full-wave rectify → 400 Hz lowpass (envelope)
         → × carrier → sum bands → normalize → WAV out
```

**Strategies:**

- **CIS** — Continuous Interleaved Sampling. The canonical CI simulation algorithm (Shannon 1995, Loizou 1999). Each band's envelope modulates a noise or sine carrier at the band's center frequency.
- **FS4** — Models MED-EL FineHearing. Apical channels (center ≤ 950 Hz) use zero-crossing-driven sine carriers to preserve pitch; basal channels use standard CIS.
- **FFT** — Hann-windowed overlap-add (1024-sample frames, 512-sample hop). Kept as a comparison baseline; not a realistic CI simulation.

**Band spacing:** Logarithmic between 70 and 8,500 Hz using geometric mean center frequencies.

## Development

```bash
cargo test
cargo clippy
```

## Stretch Goals

- **Live microphone input** — `cpal`-based `listen` subcommand; requires refactoring `vocoder::process()` into a streaming `VocoderStream` API with persistent IIR filter state across chunks.
- **WASM AudioWorklet** — Run the vocoder in-browser for real-time processing with no server round-trip.
