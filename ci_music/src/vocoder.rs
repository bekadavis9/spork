use rustfft::{FftPlanner, num_complex::Complex};
use crate::bands::Bands;
use crate::filter;

const FRAME_SIZE: usize = 1024;
const HOP_SIZE: usize = 512;
const F_LOW: f32 = 70.0;
const F_HIGH: f32 = 8500.0;

/// Envelope low-pass cutoff (Hz).
/// Real CIs discard temporal modulations above ~300 Hz (Kasdan et al. 2024).
/// 400 Hz is a standard research compromise: accurate to CI hardware, preserves
/// enough attack detail for rhythm perception, and avoids the harshness of higher cutoffs.
const ENVELOPE_CUTOFF: f32 = 400.0;

/// FS4 fine-structure cutoff (Hz).
/// Apical channels (center freq ≤ this) use zero-crossing driven carriers;
/// basal channels use standard CIS envelope coding.
/// MED-EL FS4 covers ~70–950 Hz with fine structure (4 channels at 8-channel default).
const FS4_CUTOFF_HZ: f32 = 950.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Strategy {
    /// FFT-based overlap-add vocoder (original implementation).
    Fft,
    /// Time-domain CIS vocoder: bandpass → rectify → LPF envelope → carrier.
    /// More accurate to the CI signal processing chain (Shannon 1995, Loizou 1999).
    Cis,
    /// FS4-style vocoder (MED-EL FineHearing).
    /// Apical channels (≤ 950 Hz) use zero-crossing driven sine carriers that track
    /// instantaneous pitch. Basal channels use standard CIS envelope coding.
    /// Preserves voice and instrument pitch in the low-frequency range.
    Fs4,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Carrier {
    /// Sine wave at each band's center frequency. Produces a tonal, organ-like sound.
    /// Closer to what CIs actually do (place pitch via electrode position).
    Sine,
    /// Band-limited noise per channel. Produces the buzzy, rushing sound most people
    /// associate with CI simulations (Shannon et al. 1995 noise vocoder).
    Noise,
}

/// Run the channel vocoder on `samples` (mono, f32) and return the processed samples.
pub fn process(
    samples: &[f32],
    sample_rate: u32,
    num_channels: usize,
    strategy: Strategy,
    carrier: Carrier,
) -> Vec<f32> {
    match strategy {
        Strategy::Fft => process_fft(samples, sample_rate, num_channels),
        Strategy::Cis => process_cis(samples, sample_rate, num_channels, carrier),
        Strategy::Fs4 => process_fs4(samples, sample_rate, num_channels, carrier),
    }
}

/// Simple xorshift64 PRNG — avoids a `rand` dependency.
fn xorshift(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    // Map to [-1, 1]
    (*state as i64 as f32) / (i64::MAX as f32)
}

/// CIS (Continuous Interleaved Sampling) vocoder — canonical CI simulation algorithm.
///
/// Per channel: 4th-order bandpass → full-wave rectify → LPF envelope → carrier.
/// Carrier is either a continuous sine (tonal) or band-limited noise (buzzy).
fn process_cis(samples: &[f32], sample_rate: u32, num_channels: usize, carrier: Carrier) -> Vec<f32> {
    let bands = Bands::new(num_channels, F_LOW, F_HIGH);
    let fs = sample_rate as f32;

    // Build per-channel Q from band geometry
    let qs: Vec<f32> = (0..num_channels)
        .map(|i| bands.centers[i] / (bands.edges[i + 1] - bands.edges[i]))
        .collect();

    // Two cascaded 2nd-order BPFs per channel → 4th-order rolloff on the input signal
    let mut bp1: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();
    let mut bp2: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();

    // Separate bandpass filter bank for noise carrier (only used when carrier = Noise)
    let mut noise_bp1: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();
    let mut noise_bp2: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();

    // Envelope LPF per channel
    let mut env_lp: Vec<filter::Biquad> = (0..num_channels)
        .map(|_| filter::lowpass(ENVELOPE_CUTOFF, fs))
        .collect();

    // Sine carrier phase accumulators
    let phase_inc: Vec<f32> = bands
        .centers
        .iter()
        .map(|&f| 2.0 * std::f32::consts::PI * f / fs)
        .collect();
    let mut phases = vec![0.0f32; num_channels];

    // Noise PRNG state
    let mut rng_state: u64 = 0xdeadbeefcafebabe;

    let mut output = vec![0.0f32; samples.len()];

    for (n, &x) in samples.iter().enumerate() {
        let mut y = 0.0f32;
        let noise_sample = xorshift(&mut rng_state);

        for ch in 0..num_channels {
            // Extract envelope from the input signal
            let filtered = bp2[ch].process(bp1[ch].process(x));
            let envelope = env_lp[ch].process(filtered.abs()).max(0.0);

            // Generate carrier
            let carrier_sample = match carrier {
                Carrier::Sine => {
                    let s = phases[ch].sin();
                    phases[ch] += phase_inc[ch];
                    if phases[ch] >= 2.0 * std::f32::consts::PI {
                        phases[ch] -= 2.0 * std::f32::consts::PI;
                    }
                    s
                }
                Carrier::Noise => {
                    // Bandpass-filter white noise to the channel's frequency band
                    noise_bp2[ch].process(noise_bp1[ch].process(noise_sample))
                }
            };

            y += envelope * carrier_sample;
        }

        output[n] = y;
    }

    normalize(output)
}

/// FS4-style vocoder — MED-EL FineHearing acoustic simulation.
///
/// Splits channels into apical (center ≤ FS4_CUTOFF_HZ) and basal:
///   Apical:  bandpass → envelope LPF + zero-crossing pitch tracking → FM sine × envelope
///   Basal:   standard CIS (bandpass → envelope LPF → carrier × envelope)
///
/// With 8 channels the split is naturally 4/4, matching the real FS4 specification.
/// Zero-crossing period estimation uses an exponential moving average to track
/// instantaneous frequency smoothly without overshooting on transients.
fn process_fs4(samples: &[f32], sample_rate: u32, num_channels: usize, carrier: Carrier) -> Vec<f32> {
    let bands = Bands::new(num_channels, F_LOW, F_HIGH);
    let fs = sample_rate as f32;

    // Apical channels are the lowest-frequency ones whose center falls within the FS4 range.
    let apical_count = bands.centers.iter().filter(|&&c| c <= FS4_CUTOFF_HZ).count();

    let qs: Vec<f32> = (0..num_channels)
        .map(|i| bands.centers[i] / (bands.edges[i + 1] - bands.edges[i]))
        .collect();

    let mut bp1: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();
    let mut bp2: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();

    // Noise bandpass filters for basal CIS noise carrier
    let mut noise_bp1: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();
    let mut noise_bp2: Vec<filter::Biquad> = (0..num_channels)
        .map(|i| filter::bandpass(bands.centers[i], qs[i], fs))
        .collect();

    let mut env_lp: Vec<filter::Biquad> = (0..num_channels)
        .map(|_| filter::lowpass(ENVELOPE_CUTOFF, fs))
        .collect();

    // Basal CIS fixed-frequency carrier state
    let phase_inc: Vec<f32> = bands
        .centers
        .iter()
        .map(|&f| 2.0 * std::f32::consts::PI * f / fs)
        .collect();
    let mut phases = vec![0.0f32; num_channels];

    // Apical fine-structure state: zero-crossing period tracker + independent phase
    let mut prev_filtered = vec![0.0f32; apical_count];
    let mut last_zc = vec![0usize; apical_count];
    // Seed period estimate from center frequency so the carrier starts at the right pitch
    let mut zc_period: Vec<f32> = (0..apical_count).map(|i| fs / bands.centers[i]).collect();
    let mut fs_phase = vec![0.0f32; apical_count];
    let mut fs_phase_inc: Vec<f32> = (0..apical_count)
        .map(|i| 2.0 * std::f32::consts::PI * bands.centers[i] / fs)
        .collect();

    let mut rng_state: u64 = 0xdeadbeefcafebabe;
    let mut output = vec![0.0f32; samples.len()];

    for (n, &x) in samples.iter().enumerate() {
        let mut y = 0.0f32;
        let noise_sample = xorshift(&mut rng_state);

        for ch in 0..num_channels {
            let filtered = bp2[ch].process(bp1[ch].process(x));
            let envelope = env_lp[ch].process(filtered.abs()).max(0.0);

            let carrier_sample = if ch < apical_count {
                let ach = ch;

                // Positive zero crossing → update period estimate
                if prev_filtered[ach] < 0.0 && filtered >= 0.0 {
                    let period = n.saturating_sub(last_zc[ach]);
                    // Guard against implausible periods (below lowest band edge or > 1 s)
                    if period > 0 && period < sample_rate as usize {
                        // Exponential moving average: 25% new, 75% history
                        zc_period[ach] = zc_period[ach] * 0.75 + period as f32 * 0.25;
                        let inst_freq = (fs / zc_period[ach])
                            .clamp(bands.edges[ach], bands.edges[ach + 1]);
                        fs_phase_inc[ach] = 2.0 * std::f32::consts::PI * inst_freq / fs;
                    }
                    last_zc[ach] = n;
                }
                prev_filtered[ach] = filtered;

                let s = fs_phase[ach].sin();
                fs_phase[ach] += fs_phase_inc[ach];
                if fs_phase[ach] >= 2.0 * std::f32::consts::PI {
                    fs_phase[ach] -= 2.0 * std::f32::consts::PI;
                }
                s
            } else {
                // Basal: standard CIS carrier
                match carrier {
                    Carrier::Sine => {
                        let s = phases[ch].sin();
                        phases[ch] += phase_inc[ch];
                        if phases[ch] >= 2.0 * std::f32::consts::PI {
                            phases[ch] -= 2.0 * std::f32::consts::PI;
                        }
                        s
                    }
                    Carrier::Noise => noise_bp2[ch].process(noise_bp1[ch].process(noise_sample)),
                }
            };

            y += envelope * carrier_sample;
        }

        output[n] = y;
    }

    normalize(output)
}

/// FFT overlap-add vocoder (original implementation, kept for comparison).
fn process_fft(samples: &[f32], sample_rate: u32, num_channels: usize) -> Vec<f32> {
    let bands = Bands::new(num_channels, F_LOW, F_HIGH);
    let hann = hann_window(FRAME_SIZE);

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FRAME_SIZE);

    let mut output = vec![0.0f32; samples.len() + FRAME_SIZE];

    let num_frames = (samples.len().saturating_sub(FRAME_SIZE)) / HOP_SIZE + 1;

    for frame_idx in 0..num_frames {
        let start = frame_idx * HOP_SIZE;
        let end = (start + FRAME_SIZE).min(samples.len());

        // Build windowed frame, zero-padded if near the end
        let mut buf: Vec<Complex<f32>> = (0..FRAME_SIZE)
            .map(|i| {
                let s = if start + i < end { samples[start + i] } else { 0.0 };
                Complex::new(s * hann[i], 0.0)
            })
            .collect();

        fft.process(&mut buf);

        // Synthesize: one sine per band, scaled by RMS amplitude
        let mut frame_out = vec![0.0f32; FRAME_SIZE];
        for band_i in 0..bands.len() {
            let (lo, hi) = bands.bin_range(band_i, FRAME_SIZE, sample_rate);
            if lo >= hi {
                continue;
            }

            // RMS amplitude of bins in this band
            let energy: f32 = buf[lo..hi].iter().map(|c| c.norm_sqr()).sum();
            let amplitude = (energy / (hi - lo) as f32).sqrt();

            // Add sine wave at band center for each sample in the frame
            let center = bands.centers[band_i];
            for (t, sample) in frame_out.iter_mut().enumerate() {
                let phase = 2.0 * std::f32::consts::PI * center * (start + t) as f32
                    / sample_rate as f32;
                *sample += amplitude * phase.sin();
            }
        }

        // Apply Hann window to synthesized frame and overlap-add
        for i in 0..FRAME_SIZE {
            output[start + i] += frame_out[i] * hann[i];
        }
    }

    output.truncate(samples.len());
    normalize(output)
}

fn normalize(mut output: Vec<f32>) -> Vec<f32> {
    let peak = output
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max)
        .max(1e-9);
    output.iter_mut().for_each(|s| *s /= peak);
    output
}

/// Load a WAV file and return (mono f32 samples, sample_rate).
/// Write mono f32 samples to a WAV file.
pub fn write_wav(path: &str, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &s in samples {
        writer.write_sample(s).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}

/// Encode mono f32 samples to WAV bytes.
pub fn encode_wav_bytes(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
    for &s in samples {
        writer.write_sample(s).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}

fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (size - 1) as f32).cos())
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────
// The test suite is organised into four tiers:
//
//  1. Software invariants — output length, normalization, finiteness,
//     edge-case robustness.  These must hold for every strategy and carrier.
//
//  2. Strategy/carrier distinctness — different configurations must produce
//     audibly and measurably different outputs from the same input.
//
//  3. WAV I/O correctness — encode/decode round-trip, stereo downmix formula.
//
//  4. Acoustic/perceptual claims — the two core scientific assertions:
//       a. CIS preserves temporal envelope (rhythm) while stripping fine structure.
//       b. FS4 additionally preserves low-frequency pitch via zero-crossing tracking.
//
//  NOTE on Tier 4 (FS4 pitch tracking): our zero-crossing acoustic simulation
//  has no published reference implementation to validate against. The threshold
//  values below are conservative (autocorrelation > 0.3 after convergence) and
//  grounded in the expected behaviour, but not yet calibrated against Loizou's
//  UT Dallas code or Riss et al. (2014). This is the primary literature gap.

#[cfg(test)]
mod vocoder_tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 44100;

    // ── Test signal helpers ───────────────────────────────────────────────────

    /// 1-second mix of four sinusoids spanning the CI frequency range:
    /// two in the FS4 apical range (200 Hz, 500 Hz) and two in the basal
    /// range (2000 Hz, 4000 Hz).  Used for most invariant tests.
    fn mixed_signal() -> Vec<f32> {
        let n = SR as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                0.25 * (2.0 * PI * 200.0 * t).sin()
                    + 0.25 * (2.0 * PI * 500.0 * t).sin()
                    + 0.25 * (2.0 * PI * 2000.0 * t).sin()
                    + 0.25 * (2.0 * PI * 4000.0 * t).sin()
            })
            .collect()
    }

    /// Pure sinusoid at `freq` Hz for `secs` seconds.
    fn pure_tone(freq: f32, secs: f32) -> Vec<f32> {
        let n = (SR as f32 * secs) as usize;
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / SR as f32).sin())
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    fn rms_diff(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len().min(b.len());
        (a[..n]
            .iter()
            .zip(b[..n].iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            / n as f32)
            .sqrt()
    }

    fn peak(s: &[f32]) -> f32 {
        s.iter().map(|x| x.abs()).fold(0.0f32, f32::max)
    }

    /// Normalised autocorrelation at a given sample lag.
    /// Returns corr(signal, signal shifted by `lag`) / energy(signal).
    /// A pure sine at frequency f has autocorrelation ≈ cos(2π f lag/SR) at that lag.
    fn normalised_autocorr(s: &[f32], lag: usize) -> f32 {
        if lag >= s.len() {
            return 0.0;
        }
        let n = s.len() - lag;
        let num: f32 = s[..n].iter().zip(s[lag..].iter()).map(|(a, b)| a * b).sum();
        let denom: f32 = s[..n].iter().map(|x| x * x).sum::<f32>().max(1e-12);
        num / denom
    }

    // ── Tier 1: Software invariants ───────────────────────────────────────────

    /// output.len() must equal input.len() for every strategy.  The vocoder
    /// is a sample-length-preserving transform — callers rely on this.
    #[test]
    fn output_length_matches_input_cis() {
        let sig = mixed_signal();
        let out = process(&sig, SR, 8, Strategy::Cis, Carrier::Noise);
        assert_eq!(out.len(), sig.len(), "CIS output length must match input");
    }

    #[test]
    fn output_length_matches_input_fs4() {
        let sig = mixed_signal();
        let out = process(&sig, SR, 8, Strategy::Fs4, Carrier::Noise);
        assert_eq!(out.len(), sig.len(), "FS4 output length must match input");
    }

    #[test]
    fn output_length_matches_input_fft() {
        let sig = mixed_signal();
        let out = process(&sig, SR, 8, Strategy::Fft, Carrier::Noise);
        assert_eq!(out.len(), sig.len(), "FFT output length must match input");
    }

    /// Peak amplitude must not exceed 1.0 (would clip on playback).
    #[test]
    fn output_peak_does_not_clip_cis() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Cis, Carrier::Noise));
        assert!(p <= 1.0 + 1e-5, "CIS output clipped: peak={p:.6}");
    }

    #[test]
    fn output_peak_does_not_clip_fs4() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Fs4, Carrier::Noise));
        assert!(p <= 1.0 + 1e-5, "FS4 output clipped: peak={p:.6}");
    }

    #[test]
    fn output_peak_does_not_clip_fft() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Fft, Carrier::Noise));
        assert!(p <= 1.0 + 1e-5, "FFT output clipped: peak={p:.6}");
    }

    /// normalize() must bring the peak to near 1.0, not just prevent clipping.
    /// A peak below 0.5 means energy was lost somewhere in the pipeline.
    #[test]
    fn output_is_normalised_to_near_full_scale_cis() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Cis, Carrier::Noise));
        assert!(p > 0.5, "CIS output is unexpectedly quiet after normalisation: peak={p:.4}");
    }

    #[test]
    fn output_is_normalised_to_near_full_scale_fs4() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Fs4, Carrier::Noise));
        assert!(p > 0.5, "FS4 output is unexpectedly quiet after normalisation: peak={p:.4}");
    }

    #[test]
    fn output_is_normalised_to_near_full_scale_fft() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Fft, Carrier::Noise));
        assert!(p > 0.5, "FFT output is unexpectedly quiet after normalisation: peak={p:.4}");
    }

    /// normalize() must bring the peak to within 1% of 1.0, not merely
    /// prevent clipping. A peak of 0.6 would pass the > 0.5 check above
    /// but indicate a bug in the normalization path.
    #[test]
    fn output_peak_is_normalised_to_one_cis() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Cis, Carrier::Noise));
        assert!((p - 1.0).abs() < 0.01, "CIS peak should be ≈1.0 after normalisation, got {p:.6}");
    }

    #[test]
    fn output_peak_is_normalised_to_one_fs4() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Fs4, Carrier::Noise));
        assert!((p - 1.0).abs() < 0.01, "FS4 peak should be ≈1.0 after normalisation, got {p:.6}");
    }

    #[test]
    fn output_peak_is_normalised_to_one_fft() {
        let p = peak(&process(&mixed_signal(), SR, 8, Strategy::Fft, Carrier::Noise));
        assert!((p - 1.0).abs() < 0.01, "FFT peak should be ≈1.0 after normalisation, got {p:.6}");
    }

    /// A non-zero input must produce a non-zero output (pipeline not broken).
    #[test]
    fn non_zero_input_produces_non_silent_output_cis() {
        let r = rms(&process(&mixed_signal(), SR, 8, Strategy::Cis, Carrier::Noise));
        assert!(r > 0.01, "CIS produced silence for non-zero input");
    }

    #[test]
    fn non_zero_input_produces_non_silent_output_fs4() {
        let r = rms(&process(&mixed_signal(), SR, 8, Strategy::Fs4, Carrier::Noise));
        assert!(r > 0.01, "FS4 produced silence for non-zero input");
    }

    #[test]
    fn non_zero_input_produces_non_silent_output_fft() {
        let r = rms(&process(&mixed_signal(), SR, 8, Strategy::Fft, Carrier::Noise));
        assert!(r > 0.01, "FFT produced silence for non-zero input");
    }

    /// All-zero input must produce all-zero output.  The noise carrier has
    /// zero amplitude when the envelope is zero, so silence must pass through.
    #[test]
    fn silence_in_silence_out_cis() {
        let silence = vec![0.0f32; SR as usize];
        let r = rms(&process(&silence, SR, 8, Strategy::Cis, Carrier::Noise));
        assert!(r < 1e-6, "CIS should output silence for silent input; rms={r:.2e}");
    }

    #[test]
    fn silence_in_silence_out_fs4() {
        let silence = vec![0.0f32; SR as usize];
        let r = rms(&process(&silence, SR, 8, Strategy::Fs4, Carrier::Noise));
        assert!(r < 1e-6, "FS4 should output silence for silent input; rms={r:.2e}");
    }

    #[test]
    fn silence_in_silence_out_fft() {
        let silence = vec![0.0f32; SR as usize];
        let r = rms(&process(&silence, SR, 8, Strategy::Fft, Carrier::Noise));
        assert!(r < 1e-6, "FFT should output silence for silent input; rms={r:.2e}");
    }

    /// No strategy or carrier may produce NaN or inf — these would corrupt
    /// downstream audio and silently break WAV encoding.
    #[test]
    fn output_is_finite_cis() {
        for (i, s) in process(&mixed_signal(), SR, 8, Strategy::Cis, Carrier::Noise)
            .iter()
            .enumerate()
        {
            assert!(s.is_finite(), "CIS output non-finite at sample {i}: {s}");
        }
    }

    #[test]
    fn output_is_finite_fs4() {
        for (i, s) in process(&mixed_signal(), SR, 8, Strategy::Fs4, Carrier::Noise)
            .iter()
            .enumerate()
        {
            assert!(s.is_finite(), "FS4 output non-finite at sample {i}: {s}");
        }
    }

    #[test]
    fn output_is_finite_fft() {
        for (i, s) in process(&mixed_signal(), SR, 8, Strategy::Fft, Carrier::Noise)
            .iter()
            .enumerate()
        {
            assert!(s.is_finite(), "FFT output non-finite at sample {i}: {s}");
        }
    }

    /// The PRNG is seeded with a fixed constant, so noise-carrier outputs must
    /// be bit-identical across runs for the same input.
    #[test]
    fn cis_noise_carrier_is_deterministic() {
        let sig = mixed_signal();
        let a = process(&sig, SR, 8, Strategy::Cis, Carrier::Noise);
        let b = process(&sig, SR, 8, Strategy::Cis, Carrier::Noise);
        assert_eq!(a, b, "CIS noise carrier must be deterministic (fixed PRNG seed)");
    }

    #[test]
    fn fs4_noise_carrier_is_deterministic() {
        let sig = mixed_signal();
        let a = process(&sig, SR, 8, Strategy::Fs4, Carrier::Noise);
        let b = process(&sig, SR, 8, Strategy::Fs4, Carrier::Noise);
        assert_eq!(a, b, "FS4 noise carrier must be deterministic (fixed PRNG seed)");
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn handles_empty_input() {
        let out = process(&[], SR, 8, Strategy::Cis, Carrier::Noise);
        assert_eq!(out.len(), 0, "empty input must produce empty output");
    }

    #[test]
    fn handles_single_sample() {
        for strat in [Strategy::Cis, Strategy::Fs4, Strategy::Fft] {
            let out = process(&[0.5], SR, 8, strat, Carrier::Noise);
            assert_eq!(out.len(), 1, "single-sample input must produce single-sample output ({strat:?})");
            assert!(out[0].is_finite(), "single-sample output must be finite ({strat:?})");
        }
    }

    #[test]
    fn handles_single_channel() {
        let out = process(&mixed_signal(), SR, 1, Strategy::Cis, Carrier::Noise);
        assert_eq!(out.len(), SR as usize);
        assert!(out.iter().all(|x| x.is_finite()), "single-channel output contains non-finite values");
    }

    #[test]
    fn handles_maximum_channel_count() {
        let out = process(&mixed_signal(), SR, 64, Strategy::Cis, Carrier::Noise);
        assert_eq!(out.len(), SR as usize);
        assert!(out.iter().all(|x| x.is_finite()), "64-channel output contains non-finite values");
    }

    #[test]
    fn works_at_22050_hz_sample_rate() {
        let sr = 22050u32;
        let sig: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let out = process(&sig, sr, 8, Strategy::Cis, Carrier::Noise);
        assert_eq!(out.len(), sig.len());
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn works_at_48000_hz_sample_rate() {
        let sr = 48000u32;
        let sig: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let out = process(&sig, sr, 8, Strategy::Cis, Carrier::Noise);
        assert_eq!(out.len(), sig.len());
        assert!(out.iter().all(|x| x.is_finite()));
    }

    // ── Tier 2: Strategy and carrier distinctness ─────────────────────────────

    #[test]
    fn cis_and_fs4_produce_distinct_outputs() {
        let sig = mixed_signal();
        let d = rms_diff(
            &process(&sig, SR, 8, Strategy::Cis, Carrier::Noise),
            &process(&sig, SR, 8, Strategy::Fs4, Carrier::Noise),
        );
        assert!(d > 0.01, "CIS and FS4 should produce distinct outputs; RMS diff={d:.6}");
    }

    #[test]
    fn cis_and_fft_produce_distinct_outputs() {
        let sig = mixed_signal();
        let d = rms_diff(
            &process(&sig, SR, 8, Strategy::Cis, Carrier::Noise),
            &process(&sig, SR, 8, Strategy::Fft, Carrier::Noise),
        );
        assert!(d > 0.01, "CIS and FFT should produce distinct outputs; RMS diff={d:.6}");
    }

    #[test]
    fn fs4_and_fft_produce_distinct_outputs() {
        let sig = mixed_signal();
        let d = rms_diff(
            &process(&sig, SR, 8, Strategy::Fs4, Carrier::Noise),
            &process(&sig, SR, 8, Strategy::Fft, Carrier::Noise),
        );
        assert!(d > 0.01, "FS4 and FFT should produce distinct outputs; RMS diff={d:.6}");
    }

    #[test]
    fn sine_and_noise_carriers_are_distinct_for_cis() {
        let sig = mixed_signal();
        let d = rms_diff(
            &process(&sig, SR, 8, Strategy::Cis, Carrier::Sine),
            &process(&sig, SR, 8, Strategy::Cis, Carrier::Noise),
        );
        assert!(d > 0.01, "CIS sine vs noise carrier should differ; RMS diff={d:.6}");
    }

    #[test]
    fn sine_and_noise_carriers_are_distinct_for_fs4() {
        // FS4 apical channels always use a sine; basal channels follow the carrier flag.
        // The outputs must still differ overall.
        let sig = mixed_signal();
        let d = rms_diff(
            &process(&sig, SR, 8, Strategy::Fs4, Carrier::Sine),
            &process(&sig, SR, 8, Strategy::Fs4, Carrier::Noise),
        );
        assert!(d > 0.01, "FS4 sine vs noise carrier should differ; RMS diff={d:.6}");
    }

    #[test]
    fn different_channel_counts_produce_different_outputs() {
        let sig = mixed_signal();
        let ch4 = process(&sig, SR, 4, Strategy::Cis, Carrier::Noise);
        let ch8 = process(&sig, SR, 8, Strategy::Cis, Carrier::Noise);
        let ch16 = process(&sig, SR, 16, Strategy::Cis, Carrier::Noise);
        assert!(rms_diff(&ch4, &ch8) > 0.01, "4ch and 8ch should differ");
        assert!(rms_diff(&ch8, &ch16) > 0.01, "8ch and 16ch should differ");
    }

    // ── Tier 3: WAV I/O correctness ───────────────────────────────────────────

    /// Encoding as 32-bit float WAV and decoding back must be bit-lossless.
    #[test]
    fn wav_encode_decode_roundtrip_is_lossless() {
        let original = mixed_signal();
        let encoded = encode_wav_bytes(&original, SR).expect("encode failed");
        let (decoded, rate) = crate::audio::decode_audio_bytes(&encoded, None).expect("decode failed");
        assert_eq!(rate, SR, "sample rate must survive WAV round-trip");
        assert_eq!(decoded.len(), original.len(), "sample count must survive WAV round-trip");
        for (i, (&orig, &dec)) in original.iter().zip(decoded.iter()).enumerate() {
            assert!(
                (orig - dec).abs() < 1e-6,
                "WAV round-trip error at sample {i}: original={orig}, decoded={dec}"
            );
        }
    }

    /// Stereo downmix formula: mono[i] = (L[i] + R[i]) / 2.
    /// L = 1.0, R = −1.0 → mono = 0.0.
    #[test]
    fn wav_stereo_cancellation_downmixes_to_silence() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SR,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for _ in 0..100 {
            writer.write_sample(1.0f32).unwrap();
            writer.write_sample(-1.0f32).unwrap();
        }
        writer.finalize().unwrap();
        let (mono, _) = crate::audio::decode_audio_bytes(cursor.get_ref(), None).expect("decode failed");
        assert_eq!(mono.len(), 100);
        for (i, &s) in mono.iter().enumerate() {
            assert!(s.abs() < 1e-6, "L=1 R=−1 should downmix to 0; got {s} at frame {i}");
        }
    }

    /// Stereo downmix average: L = 0.8, R = 0.4 → mono = 0.6.
    #[test]
    fn wav_stereo_averaging_is_correct() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SR,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for _ in 0..100 {
            writer.write_sample(0.8f32).unwrap();
            writer.write_sample(0.4f32).unwrap();
        }
        writer.finalize().unwrap();
        let (mono, _) = crate::audio::decode_audio_bytes(cursor.get_ref(), None).expect("decode failed");
        for (i, &s) in mono.iter().enumerate() {
            assert!(
                (s - 0.6).abs() < 1e-6,
                "stereo average should be 0.6, got {s} at frame {i}"
            );
        }
    }

    /// Nearly all real-world WAV files are 16-bit integer. The decode path
    /// normalises by dividing by 2^(bits−1) = 32768. Verify the boundary
    /// values: MAX → ≈+1.0, MIN → −1.0, zero → 0.0.
    #[test]
    fn wav_decode_16bit_int_is_correct() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SR,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        writer.write_sample(i16::MAX).unwrap(); // 32767 / 32768 ≈ +1.0
        writer.write_sample(i16::MIN).unwrap(); // −32768 / 32768 = −1.0
        writer.write_sample(0i16).unwrap();     // 0 / 32768 = 0.0
        writer.finalize().unwrap();

        let (samples, rate) = crate::audio::decode_audio_bytes(cursor.get_ref(), Some("wav")).expect("16-bit decode failed");
        assert_eq!(rate, SR);
        assert_eq!(samples.len(), 3);
        // Standard PCM normalisation: value / 2^(bits-1); tolerance covers minor decoder variation
        assert!(
            (samples[0] - 1.0).abs() < 5e-4,
            "i16::MAX should decode near +1.0, got {:.6}", samples[0]
        );
        assert!(
            (samples[1] - (-1.0)).abs() < 5e-4,
            "i16::MIN should decode near −1.0, got {:.6}", samples[1]
        );
        assert!(
            samples[2].abs() < 1e-5,
            "zero should decode to 0.0, got {:.6}", samples[2]
        );
    }

    /// The RIFF/WAVE header must be present so any compliant audio decoder
    /// can read the output without knowing it came from ci_music.
    #[test]
    fn wav_encode_produces_valid_riff_header() {
        let encoded = encode_wav_bytes(&mixed_signal(), SR).expect("encode failed");
        assert_eq!(&encoded[0..4], b"RIFF", "encoded WAV must start with RIFF");
        assert_eq!(&encoded[8..12], b"WAVE", "encoded WAV must contain WAVE marker");
    }

    // ── Tier 4a: CIS preserves temporal envelope ─────────────────────────────
    //
    // The defining claim of CIS (Shannon 1995): the amplitude envelope of the
    // output should track the amplitude envelope of the input. We test this by
    // feeding a signal that is active in the first half of a window and silent
    // in the second half. After CIS processing, the first half must have
    // substantially higher RMS than the second half.

    #[test]
    fn cis_preserves_temporal_envelope_rhythm() {
        // Signal: 500 Hz tone for the first 0.4 s, silence for the next 0.6 s.
        // 500 Hz is in an apical band — reliably processed by all strategies.
        let n = SR as usize;
        let sig: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                if t < 0.4 { (2.0 * PI * 500.0 * t).sin() } else { 0.0 }
            })
            .collect();

        let out = process(&sig, SR, 8, Strategy::Cis, Carrier::Noise);

        // Active window: 0–0.3 s (well within the 0.4 s active region, avoiding
        // the onset transient at the start and the decay at 0.4 s).
        let active_start = (SR as f32 * 0.05) as usize;
        let active_end   = (SR as f32 * 0.35) as usize;
        let silent_start = (SR as f32 * 0.55) as usize; // 150 ms after cutoff, filter settled
        let silent_end   = n;

        let rms_active = rms(&out[active_start..active_end]);
        let rms_silent = rms(&out[silent_start..silent_end]);

        assert!(
            rms_active > 10.0 * rms_silent,
            "CIS must preserve temporal envelope: active RMS={rms_active:.4}, \
             silent RMS={rms_silent:.4} (expected ratio > 10×)"
        );
    }

    /// Same on/off envelope claim for FS4. The apical zero-crossing path must
    /// not bleed energy into the silent region after the tone stops.
    #[test]
    fn fs4_preserves_temporal_envelope_rhythm() {
        let n = SR as usize;
        let sig: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                if t < 0.4 { (2.0 * PI * 500.0 * t).sin() } else { 0.0 }
            })
            .collect();

        let out = process(&sig, SR, 8, Strategy::Fs4, Carrier::Noise);

        let active_start = (SR as f32 * 0.05) as usize;
        let active_end   = (SR as f32 * 0.35) as usize;
        let silent_start = (SR as f32 * 0.55) as usize;

        let rms_active = rms(&out[active_start..active_end]);
        let rms_silent = rms(&out[silent_start..]);

        assert!(
            rms_active > 10.0 * rms_silent,
            "FS4 must preserve temporal envelope: active RMS={rms_active:.4}, \
             silent RMS={rms_silent:.4} (expected ratio > 10×)"
        );
    }

    // ── Tier 4b: FS4 tracks low-frequency pitch ───────────────────────────────
    //
    // The defining claim of FS4: the zero-crossing detector must converge to
    // the input frequency for apical channels, so the carrier oscillates at
    // the input pitch rather than the fixed band-centre frequency.
    //
    // We verify this with normalised autocorrelation. A signal oscillating at
    // frequency f has high autocorrelation at lag ≈ SR/f. After convergence
    // (first 0.5 s discarded), the FS4 output for a 200 Hz tone should have
    // positive autocorrelation at lag 220 (≈ SR/200).
    //
    // ⚠ LITERATURE GAP: the threshold (> 0.3) is conservative and internally
    // grounded. It has not been calibrated against Loizou's UT Dallas reference
    // implementation or any published FS4 acoustic simulation. Tighten this
    // threshold once a reference comparison is available.

    #[test]
    fn fs4_apical_carrier_tracks_input_frequency() {
        // 200 Hz falls in band 1 (centre ≈ 178 Hz at 8 channels) — apical.
        // The EMA converges within ~40 zero-crossings (≈ 0.2 s at 200 Hz);
        // we skip the first 0.5 s to be conservative.
        let tone = pure_tone(200.0, 2.0);
        let out = process(&tone, SR, 8, Strategy::Fs4, Carrier::Sine);

        let skip = SR as usize; // discard first 1 s for convergence
        let window = &out[skip..];

        // Expected period at 200 Hz: 44100 / 200 = 220.5 → lag 220 or 221
        let lag_200hz = (SR as f32 / 200.0).round() as usize;
        let autocorr = normalised_autocorr(window, lag_200hz);

        assert!(
            autocorr > 0.3,
            "FS4 apical channel must track 200 Hz input; normalised autocorr at lag \
             {lag_200hz} was {autocorr:.4} (expected > 0.3). \
             Low value indicates the carrier frequency did not converge to the input pitch."
        );
    }

    /// Complementary check: CIS uses a fixed-frequency carrier at the band
    /// centre (≈ 178 Hz for band 1). For a 200 Hz input, the CIS output
    /// must NOT have strong autocorrelation at lag 220 — instead it should
    /// have its peak near lag 247 (≈ SR / 178.5).
    /// This distinguishes FS4 pitch tracking from CIS frequency-independent coding.
    #[test]
    fn cis_sine_carrier_does_not_track_input_frequency() {
        let tone = pure_tone(200.0, 2.0);
        let out = process(&tone, SR, 8, Strategy::Cis, Carrier::Sine);

        let skip = SR as usize;
        let window = &out[skip..];

        // Lag for input frequency (200 Hz)
        let lag_input = (SR as f32 / 200.0).round() as usize;
        // Lag for band-centre frequency — computed from Bands so it stays
        // correct if F_LOW, F_HIGH, or channel count defaults ever change.
        let centre_freq = Bands::new(8, F_LOW, F_HIGH).centers[1];
        let lag_centre = (SR as f32 / centre_freq).round() as usize;

        let ac_input  = normalised_autocorr(window, lag_input);
        let ac_centre = normalised_autocorr(window, lag_centre);

        assert!(
            ac_centre > ac_input,
            "CIS sine carrier should resonate at the band-centre freq (lag {lag_centre}), \
             not the input freq (lag {lag_input}); \
             ac_centre={ac_centre:.4}, ac_input={ac_input:.4}"
        );
    }

    // ── Tier 3 (extended): WAV I/O edge cases ────────────────────────────────

    /// 24-bit integer WAV is the default export format for Audacity and GarageBand.
    /// Standard PCM normalisation divides by 2^(bits-1), so 24-bit boundary values
    /// must map to approximately ±1.0.
    #[test]
    fn wav_decode_24bit_int_is_correct() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SR,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let max24 = (1i32 << 23) - 1; // 8_388_607
        let min24 = -(1i32 << 23);    // -8_388_608
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        writer.write_sample(max24).unwrap();
        writer.write_sample(min24).unwrap();
        writer.write_sample(0i32).unwrap();
        writer.finalize().unwrap();

        let (samples, rate) = crate::audio::decode_audio_bytes(cursor.get_ref(), Some("wav")).expect("24-bit decode failed");
        assert_eq!(rate, SR);
        assert_eq!(samples.len(), 3);

        // Tolerance 5e-4 covers minor decoder variation in normalisation
        assert!(
            (samples[0] - 1.0).abs() < 5e-4,
            "24-bit max should decode near +1.0, got {:.6}", samples[0]
        );
        assert!(
            (samples[1] - (-1.0)).abs() < 5e-4,
            "24-bit min should decode near −1.0, got {:.6}", samples[1]
        );
        assert!(
            samples[2].abs() < 1e-5,
            "zero sample should decode to 0.0, got {:.6}", samples[2]
        );
    }

    /// Multi-channel (> 2) WAV files can come from DAW exports or surround audio.
    /// The downmix formula is the arithmetic mean of all channels.
    /// With 4 channels summing to 1.0, the mono result must be 0.25.
    #[test]
    fn wav_4channel_downmix_averages_correctly() {
        let spec = hound::WavSpec {
            channels: 4,
            sample_rate: SR,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        // 10 frames: channels [1.0, 0.0, 0.5, -0.5] → mean = 0.25
        for _ in 0..10 {
            writer.write_sample(1.0f32).unwrap();
            writer.write_sample(0.0f32).unwrap();
            writer.write_sample(0.5f32).unwrap();
            writer.write_sample(-0.5f32).unwrap();
        }
        writer.finalize().unwrap();

        let (mono, _) = crate::audio::decode_audio_bytes(cursor.get_ref(), None).expect("4-channel decode failed");
        assert_eq!(mono.len(), 10);
        for (i, &s) in mono.iter().enumerate() {
            assert!(
                (s - 0.25).abs() < 1e-5,
                "4-channel average should be 0.25, got {s} at frame {i}"
            );
        }
    }

    /// Encoding an empty sample slice must succeed and produce a valid RIFF header.
    /// This guards against callers passing a zero-length signal (e.g. very short clips).
    #[test]
    fn wav_encode_empty_samples_produces_valid_header() {
        let encoded = encode_wav_bytes(&[], SR).expect("encoding empty samples must not fail");
        assert!(
            encoded.len() >= 44,
            "WAV header must be at least 44 bytes even with no samples; got {}",
            encoded.len()
        );
        assert_eq!(&encoded[0..4], b"RIFF", "empty WAV must start with RIFF");
        assert_eq!(&encoded[8..12], b"WAVE", "empty WAV must contain WAVE marker");
    }

    /// Garbage bytes must not panic — they must return Err so the server
    /// can return a 500 rather than crashing.
    #[test]
    fn wav_decode_invalid_bytes_returns_error() {
        let garbage = b"this is definitely not a WAV file";
        assert!(
            crate::audio::decode_audio_bytes(garbage, None).is_err(),
            "decode_audio_bytes must return Err for invalid input, not panic"
        );
    }

    /// A truncated-but-valid WAV (valid RIFF header, data chunk cut short) yields
    /// partial samples rather than a hard error. Symphonia decodes what it can and
    /// breaks on UnexpectedEof, so callers get whatever samples were decoded.
    #[test]
    fn wav_decode_truncated_file_returns_partial_data() {
        let encoded = encode_wav_bytes(&mixed_signal(), SR).expect("encode failed");
        let truncated = &encoded[..encoded.len() / 2];
        let result = crate::audio::decode_audio_bytes(truncated, None);
        assert!(result.is_ok(), "truncated WAV should return partial data, not an error");
        let (samples, _) = result.unwrap();
        assert!(!samples.is_empty(), "truncated WAV must yield at least some samples");
        assert!(
            samples.len() < mixed_signal().len(),
            "truncated WAV should yield fewer samples than the complete signal"
        );
    }

    /// The FFT strategy zero-pads frames when the signal is shorter than FRAME_SIZE
    /// (1024 samples). Output length must still equal input length.
    #[test]
    fn process_fft_signal_shorter_than_frame_size_has_correct_output_length() {
        // 100 samples << FRAME_SIZE=1024
        let short: Vec<f32> = (0..100)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / SR as f32).sin())
            .collect();
        let out = process(&short, SR, 8, Strategy::Fft, Carrier::Noise);
        assert_eq!(
            out.len(), short.len(),
            "FFT output length must match input for signals shorter than FRAME_SIZE"
        );
        assert!(
            out.iter().all(|x| x.is_finite()),
            "FFT short-signal output must be finite"
        );
    }

    /// All four supported channel counts must produce finite, normalised output
    /// through both the CIS and FS4 strategies. This is a combined smoke test
    /// that exercises the full matrix used in the web UI.
    #[test]
    fn all_channel_counts_produce_valid_output_for_both_strategies() {
        let sig = mixed_signal();
        for &n in &[4usize, 8, 12, 16] {
            for strat in [Strategy::Cis, Strategy::Fs4] {
                let out = process(&sig, SR, n, strat, Carrier::Noise);
                assert_eq!(out.len(), sig.len(), "{strat:?} n={n}: output length mismatch");
                assert!(
                    out.iter().all(|x| x.is_finite()),
                    "{strat:?} n={n}: output contains non-finite values"
                );
                let p = peak(&out);
                assert!(
                    (p - 1.0).abs() < 0.01,
                    "{strat:?} n={n}: peak should be ≈1.0 after normalisation, got {p:.6}"
                );
            }
        }
    }
}
