# As Heard Through a Cochlear Implant

A Rust CLI that simulates how cochlear implant (CI) users perceive sound. It processes a WAV file through a channel vocoder — splitting the spectrum into N logarithmically-spaced bands, extracting amplitude envelopes, and reconstructing audio with band-limited noise or sine carriers. The result preserves rhythm and timing while stripping harmonic detail and timbre.

---

## Quickstart

```bash
cargo build --release
cargo run -- process --input song.wav --output sim.wav
```

Open `sim.wav` to hear the simulation.

---

## CLI

### `process` — simulate a WAV file

```bash
cargo run -- process --input <FILE> --output <FILE> [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `-i, --input` | *(required)* | Input WAV file |
| `-o, --output` | *(required)* | Output WAV file |
| `-c, --channels` | `8` | Number of frequency bands. 4 = low, 12 = typical CI, 24 = high |
| `--strategy` | `cis` | `cis` (standard CI envelope coding), `fs4` (MED-EL FineHearing — pitch-preserving), or `fft` (overlap-add, kept for comparison) |
| `--carrier` | `noise` | `noise` (band-limited noise, buzzy CI sound) or `sine` (tonal, bell-like). Ignored when `--strategy fft` is used (FFT always uses sine). |
| `-v, --verbose` | off | Print processing details |

**Examples**

```bash
# Typical CI simulation — 12 channels, noise carrier
cargo run -- process -i song.wav -o sim.wav --channels 12

# Low channel count
cargo run -- process -i song.wav -o sim_4ch.wav --channels 4

# Sine carrier (tonal, for comparison)
cargo run -- process -i song.wav -o sim_sine.wav --carrier sine --verbose

# MED-EL FS4 — pitch-preserving low-frequency channels
cargo run -- process -i song.wav -o sim_fs4.wav --strategy fs4

# Original FFT vocoder
cargo run -- process -i song.wav -o sim_fft.wav --strategy fft
```

---

### `serve` — web UI

```bash
cargo run -- serve [-p <PORT>] [--port <PORT>]
```

Starts a local server (default port 3000). Open `http://localhost:3000` to upload an audio file (WAV, MP3, FLAC, OGG, AAC, M4A), choose a channel count, and play the original alongside the simulation side-by-side.

---

## Algorithm

**CIS strategy** follows the Continuous Interleaved Sampling processing chain used in real CI hardware (Shannon 1995, Loizou 1999):

1. Split signal into N log-spaced bands between 70–8,500 Hz using 4th-order IIR bandpass filters (two cascaded 2nd-order biquads per channel).
2. Full-wave rectify each channel's output.
3. Extract amplitude envelope with a 400 Hz Butterworth lowpass filter — discards temporal fine structure (pitch, timbre) while preserving rhythm.
4. Modulate a carrier — band-limited noise or a continuous sine — by the envelope.
5. Sum all channels.

**FS4 strategy** (web UI default) simulates MED-EL's FineHearing sound coding (FS4 variant):

- **Apical channels** (center frequency ≤ 950 Hz): envelope extraction as in CIS, but the carrier is a sine whose frequency is updated on each positive zero-crossing of the band-limited signal. An exponential moving average smooths the period estimate. With 8 channels this covers the 4 lowest bands (~95–620 Hz), preserving voice pitch and instrument melody.
- **Basal channels** (> 950 Hz): standard CIS envelope × carrier.

The 950 Hz cutoff and 4-channel apical split match the real FS4 specification. Clinical studies show 93% of FS4/FS4-p users prefer it over CIS for music listening.

**FFT strategy** uses a Hann-windowed overlap-add vocoder (1024-sample frames, 512-sample hop). Each frame's band amplitude is estimated via RMS of FFT bins and resynthesized as a pure sine wave. Kept for A/B comparison only — it is not a realistic CI simulation.

> **Note:** The FFT output sounds tonal and bell-like by design. Pure sine synthesis is inherently tonal, and the Hann-windowed frames produce smooth amplitude ramps that resemble bell attacks and decays. Real CIs use noise or pulse-train carriers (as CIS and FS4 do), which produce the characteristic buzzy sound. If the FFT model were given a noise carrier it would be a slower, less accurate version of CIS, defeating its purpose as a spectral baseline.

---

## Frequency Bands

Bands are logarithmically spaced between **70 Hz and 8,500 Hz** — the usable range of most CI devices. Band edges follow:

```
f[i] = f_low × (f_high / f_low) ^ (i / N)
```

Center frequencies are geometric means of adjacent edges. Q factors are derived from `f_center / bandwidth`.

---

## Demo Day Checklist

- [ ] `cargo build --release` passes clean
- [ ] `cargo run --release -- serve` — web UI at http://localhost:3000
- [ ] Upload demo song → hit **Simulate**
- [ ] Play original (~10 s) — let the room hear clean audio
- [ ] Play simulation (12 channels, Classic CI) — let the room react
- [ ] Switch to 4 channels → re-simulate — noticeably worse, that's the point
- [ ] Switch to 24 channels → re-simulate — noticeably better
- [ ] Back to 12 channels → toggle **Classic CI** vs **Advanced CI** at the same channel count to show pitch difference
- [ ] Closing line ready: *"This is the same channel-vocoder model used in CI audiology research. Rust processes a 4-minute song in under a second."*

---

## Sources

**Algorithm and signal processing**

- Shannon, R.V., Zeng, F.G., Kamath, V., Wygonski, J., & Ekelid, M. (1995). "Speech recognition with primarily temporal cues." *Science*, 270(5234), 303–304. *(foundational CIS vocoder paper)*
- Loizou, P.C. (1999). "Introduction to cochlear implants." *IEEE Engineering in Medicine and Biology Magazine*, 18(1), 32–42. *(CIS processing chain and band spacing)*

**Music perception with cochlear implants**

- McDermott, H.J. (2004). "Music perception with cochlear implants: A review." *Trends In Amplification*, 8(2), 49–82. *(rhythm preserved; melody and timbre largely lost)*
- McDermott, H., Sucher, C., & Simpson, A. (2009). "Electro-acoustic stimulation." *Audiology & Neurotology*, 14(Suppl 1), 31–38. DOI: 10.1159/000206489. *(frequency-place mismatch; low-frequency perception improves with brain plasticity over time)*
- Kasdan, A.V., Butera, I.M., DeFreese, A.J., Rowland, J., Hilbun, A.L., Gordon, R.L., Wallace, M.T., & Giffard, R.H. (2024). "Cochlear implant users experience the sound-to-music effect." *Auditory Perception & Cognition*, 7(3), 179–202. DOI: 10.1080/25742442.2024.2313430.

**FS4 sound coding strategy**

- Riss, D., Hamzavi, J.S., Blineder, M., Honeder, C., Ehrenreich, I., Kaider, A., Baumgartner, W.D., Gstoettner, W., & Arnoldner, C. (2014). "FS4, FS4-p, and FSP: A 4-month crossover study of 3 fine structure sound-coding strategies." *Ear and Hearing*, 35(6), e272–e281. DOI: 10.1097/AUD.0000000000000063. *(source of the 93% FS4/FS4-p preference-over-CIS statistic)*

**Web and device documentation**

- MED-EL. "What Does a Cochlear Implant Sound Like? Reaching Closest to Natural Hearing." medel.com. https://www.medel.com/en-us/hearing-solutions/cochlear-implants/closest-to-natural-hearing *(450° vs 720° cochlear coverage; virtual channel count)*
- MED-EL. "Hear the Lowest Sounds with a Cochlear Implant." *The MED-EL Blog*. July 8, 2015 (updated August 22, 2024). https://blog.medel.com/technology/hear-the-lowest-sounds-with-a-cochlear-implant/

**Demo reference**

- danielperren. "Cochlear implant: simulation on speech and music." YouTube, March 21, 2011. https://www.youtube.com/watch?v=SpKKYBkJ9Hw *(demonstrates 1, 4, 8, 12, and 20 channel vocoder simulations)*

---

## Demo Audio

The demo clip (`src/input/yesterday_demo.wav`) is a 22-second excerpt from:

> "Yesterday." Written by John Lennon and Paul McCartney. Performed by The Beatles. *1* (2000 compilation), Apple Records. Original recording © 1965 Sony Music Entertainment. Short excerpt used for non-commercial educational demonstration only.

---

## Dependencies

```toml
hound      = "3.5"   # WAV write (file and in-memory)
rustfft    = "6.2"   # FFT (used by fft strategy)
clap       = { version = "4", features = ["derive"] }
axum       = { version = "0.8", features = ["multipart"] }
tokio      = { version = "1", features = ["rt-multi-thread", "macros"] }
symphonia  = { version = "0.5", features = ["mp3", "aac", "flac", "ogg", "vorbis", "wav", "pcm", "isomp4"] }
```
