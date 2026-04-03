# CI Sim — Planning Conversation
_Conversation between Rebekah, ChatGPT, and Claude Code._

---

## Project Summary

A **channel vocoder** CLI tool in Rust (`ci_music`) that simulates how cochlear implant users perceive music. It takes a WAV file, splits the spectrum into N logarithmically-spaced frequency bands (70–8,500 Hz), extracts the amplitude envelope of each band via FFT, and reconstructs the audio using pure sine waves — one per band. This destroys harmonic detail and timbre while preserving rhythm and timing, mirroring the perceptual limitations of a CI device. The number of bands is configurable (`--channels`); 8 approximates a typical modern implant. Demo shows an A/B comparison at 4, 8, and 16 channels.

### Crates

```toml
[dependencies]
hound    = "3.5"   # WAV read/write
rustfft  = "6.2"   # FFT
clap     = { version = "4", features = ["derive"] }
```

---

## ChatGPT Brainstorm

**User:** I'm working on a project in Rust and I'm interested in Cochlear Implants and their musical limitations, but I have no idea how to integrate that into a project, and I don't have much DSP experience. Could you help me brainstorm a few ideas?

**ChatGPT:** This is a really cool constraint set: Rust + something meaningful (not just another CRUD tool). And the cochlear-implant/music angle is genuinely unique.

I'll give you a few ideas grouped by difficulty, biased toward things that look impressive in a demo even if the underlying tech is simple.

**1) "How does music sound with a cochlear implant?" simulator (most compelling demo)**

Build a small CLI or web tool that:
- Takes an audio file (MP3/WAV)
- Reduces it to a small number of frequency bands (e.g., 8–12)
- Reconstructs a simplified version of the audio
- Lets the user compare: original vs simulated

You don't need deep signal processing, just:
- Split frequencies into bands
- Keep amplitude envelope
- Throw away fine detail (which is exactly what happens in cochlear implants)

Rust angle: use Rust for audio processing (fast + impressive). Bonus: run it in WASM and make a small browser demo.

Demo moment: Play "Here's what this song sounds like to a normal listener… and here's what it sounds like with a cochlear implant." That alone will grab people emotionally.

**2) Music "CI readability" analyzer**

A tool that scores a song based on how CI-friendly it is. Output example:
```
Song: Imagine - John Lennon
CI Music Score: 8.2 / 10
Reason:
- Strong melody
- Low harmonic overlap
- Clear vocal dominance
```

**3) Real-time "simplified music" stream**

A Rust app that takes microphone input, simplifies the sound in real time, and outputs a "CI-style" audio stream.

**4) "Music for cochlear implant listeners" recommender**

Build a small recommender that takes a song, extracts simple features, and suggests similar songs that are more CI-friendly.

**5) Accessibility-focused angle**

A tool that tells you "Your audio experience is not accessible to cochlear-implant users." — essentially an accessibility linter for audio.

**Honest recommendation:** If you want maximum impact with minimum risk, go with option #1 (simple cochlear-implant music simulator + side-by-side audio player). Emotionally powerful, very demo-friendly, technically achievable in 2–3 days, unique topic.

---

**User:** I was actually planning to do your #1 suggestion. I've done something similar during my undergraduate career, but that was a long time ago and it was messy. I like your recommendation. Can you give me some ideas of how I could get started, technical requirements, and what a demo may look like?

**ChatGPT:**

**Simplest possible version:**

What your tool should do (minimum viable scope):
- Input: a .wav file
- Processing: split audio into N frequency bands (start with 8), extract the amplitude envelope in each band, replace each band with a single tone, sum the tones back together
- Output: a new .wav file + an A/B player (original vs simulated)

**Conceptual pipeline:**
```
WAV file
   ↓
Short-time FFT (chunk the audio into frames)
   ↓
Group frequencies into 8 bands
   ↓
For each band:
    keep only the amplitude
    throw away the fine pitch detail
   ↓
Reconstruct signal using simple sine tones
   ↓
Write new WAV file
```

**Rust crates:**
- `hound` → read/write WAV files
- `rustfft` → FFT (very fast, super easy to use)
- `clap` → CLI interface

**Easiest first milestone:**

Step 1: Write a Rust program that loads a WAV and prints sample rate, number of samples, duration.

Step 2: Add FFT on small chunks (like 1024 samples) — split audio into frames, run FFT, print the largest frequency in each frame.

**How the "cochlear implant simulation" actually works:**

For each frequency band:
- compute its average energy
- convert that energy into a single sine wave
- mix the sine waves together

This gives: rhythm ✔️, timing ✔️, rough melody ✔️, harmonic richness ❌ (which is exactly what you want)

**Quick-start plan:**

Day 1: Load WAV, split into frames, run FFT, group into 8 bands, reconstruct simple tones, write output WAV.

Day 2: Add CLI options (`--bands 8`, `--bands 12`, `--bands 4`), process time under 1 second, optional ASCII frequency band visualizer.

Day 3: Demo polish. Clean CLI + audio playback. Optional: ASCII band visualizer.

**Demo flow:**
1. Emotional hook: "Cochlear implants are amazing for speech — but music is much harder." Play a clean song clip.
2. Run program live: `$ ci_music --input song.wav --channels 8`
3. Play the output — rhythm still there, melody barely recognizable, harmonies destroyed.
4. Show the "band slider" with `--bands 4` (awful) and `--bands 12` (slightly better).

---

## Claude Code Planning

### What You're Building

A **channel vocoder** — the exact technique used in CI simulation research:

1. Slice audio into short overlapping frames (~23ms / 1024 samples)
2. FFT each frame
3. Split the spectrum into **N logarithmically-spaced bands** (70–8,500 Hz)
4. For each band, extract the **amplitude envelope** (RMS energy)
5. Replace each band's content with a **pure sine wave** at the band's center frequency, scaled by that envelope
6. Sum all sine waves → overlap-add → output WAV

What this destroys: pitch detail, harmonics, timbre.
What this preserves: rhythm, timing, loudness contour — exactly the CI experience.

### Project Structure

```
ci_music/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI entry point (clap)
│   ├── vocoder.rs       # Core algorithm
│   ├── filter.rs        # Biquad IIR filters
│   ├── bands.rs         # Frequency band math (log spacing)
│   ├── server.rs        # axum web server
│   └── index.html       # Web UI (embedded)
├── samples/             # Test WAV files
└── README.md
```

### Crates

```toml
[dependencies]
hound    = "3.5"   # WAV read/write
rustfft  = "6.2"   # FFT
clap     = { version = "4", features = ["derive"] }
```

### CLI Interface

```
ci_music --input song.wav --output sim.wav --channels 8
ci_music --input song.wav --output sim.wav --channels 4   # sounds worse → better demo
ci_music --input song.wav --output sim.wav --channels 16  # sounds better
```

With `--verbose`:
```
Cochlear Implant Simulator
  Channels  : 8
  Freq range: 70 – 8500 Hz
  Duration  : 3m 42s
  Processing... done in 380ms
```

### Algorithm Details

**Frame parameters:**
- Frame size: 1024 samples
- Hop size: 512 samples (50% overlap)
- Window: Hann window (reduces spectral splatter)

**Band spacing:** Logarithmic between 70 and 8,500 Hz. With 8 channels, approximate bands:
```
Band 0:    70  –   145 Hz
Band 1:   145  –   300 Hz
Band 2:   300  –   620 Hz
Band 3:   620  –  1280 Hz
Band 4:  1280  –  2650 Hz
Band 5:  2650  –  5480 Hz
Band 6:  5480  –  8500 Hz
```
Formula: `f_low * (f_high / f_low) ^ (i / N)`

**Per-frame synthesis:**
For each band `i`:
- Sum squared magnitudes of FFT bins in that band → take sqrt → `amplitude[i]`
- Synthesize: `amplitude[i] * sin(2π * center_freq[i] * t)`

Apply Hann window to synthesized frame before overlap-add to avoid clicks.

**Mono simplification:** Downmix stereo to mono at load time: `(left + right) / 2.0`

### Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Output sounds like pure static (not recognizable) | Tune hop size, try larger frames (2048), add slight envelope smoothing |
| `rustfft` API confusion | It uses a planner — allocate once, reuse per frame |
| WAV is mono but test files are stereo | Downmix: `(left + right) / 2.0` at load |
| Songs don't land emotionally | Pick a slow, single-instrument piece (solo piano works best) |

**Song recommendation:** "Clair de Lune" or a Beatles ballad — clear melody, not too much percussion, hits harder emotionally than a pop track.

---

## Stretch Goals

- **Live microphone input** — Add a `listen` subcommand using `cpal` for cross-platform audio I/O. Requires refactoring `vocoder::process()` into a streaming `VocoderStream` API (chunk-by-chunk, filter state persists across calls) and replacing the post-hoc normalization pass with a running gain estimate. CIS/FS4 are well-suited — IIR filters are inherently sample-by-sample. Estimated effort: 1–2 days.
  - Longer-term: compile vocoder to WASM and run it in a browser `AudioWorklet` for in-page real-time processing with no server round-trip. Harder (~3–5 days) but the right product direction.
- **MP3 input** — Add `symphonia` for decoding MP3/AAC/FLAC; WAV-only is the current limitation.
