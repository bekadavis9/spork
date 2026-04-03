pub mod bands;
pub mod filter;
pub mod vocoder;

// audio is part of the binary, but vocoder's integration tests decode WAV bytes
// via this module. Only compiled during `cargo test` on native targets.
#[cfg(test)]
pub mod audio;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Process audio samples through the CI vocoder and return WAV bytes.
///
/// This is the WASM entry point called from `web/index.html` on the GitHub Pages
/// build. The browser decodes the uploaded file to PCM via
/// `AudioContext.decodeAudioData`, downmixes to mono, then passes the raw f32
/// samples here. Returns a complete WAV file as a byte array, or an empty array
/// if WAV encoding fails (should not happen in normal operation).
///
/// `strategy`: `"cis"` (default), `"fs4"`, or `"fft"` — case-sensitive.
/// `carrier`:  `"noise"` (default) or `"sine"` — case-sensitive.
/// Unknown values fall back to the default for each parameter.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn process_audio(
    samples: &[f32],
    sample_rate: u32,
    channels: usize,
    strategy: &str,
    carrier: &str,
) -> Vec<u8> {
    run_vocoder(samples, sample_rate, channels, strategy, carrier)
}

/// Run the vocoder pipeline and return a complete WAV file as bytes.
///
/// Parses `strategy` and `carrier` strings (case-sensitive), invokes the vocoder,
/// and encodes the result as 32-bit float WAV.
/// Unknown strategy → CIS. Unknown carrier → noise.
/// Returns an empty `Vec<u8>` if WAV encoding fails (should not happen in practice).
///
/// This is the platform-independent core used by both the `#[wasm_bindgen]`
/// export (`process_audio`) and native tests, which cannot call the WASM export
/// directly.
pub fn run_vocoder(
    samples: &[f32],
    sample_rate: u32,
    channels: usize,
    strategy: &str,
    carrier: &str,
) -> Vec<u8> {
    let strat = match strategy {
        "fs4" => vocoder::Strategy::Fs4,
        "fft" => vocoder::Strategy::Fft,
        _ => vocoder::Strategy::Cis,
    };
    let carr = match carrier {
        "sine" => vocoder::Carrier::Sine,
        _ => vocoder::Carrier::Noise,
    };
    let output = vocoder::process(samples, sample_rate, channels, strat, carr);
    vocoder::encode_wav_bytes(&output, sample_rate).unwrap_or_default()
}

#[cfg(test)]
mod lib_tests {
    use super::*;

    fn sine_440(n: usize, sr: u32) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin())
            .collect()
    }

    /// run_vocoder must return a non-empty byte slice with a valid RIFF/WAV header.
    #[test]
    fn returns_valid_wav_header() {
        let samples = sine_440(44100, 44100);
        let out = run_vocoder(&samples, 44100, 8, "cis", "noise");
        assert!(out.len() > 44, "output should be larger than a bare WAV header");
        assert_eq!(&out[0..4], b"RIFF", "output must start with RIFF");
        assert_eq!(&out[8..12], b"WAVE", "bytes 8-12 must be WAVE");
    }

    /// Unknown strategy strings must fall back to CIS, not panic or return empty.
    #[test]
    fn unknown_strategy_falls_back_to_cis() {
        let samples = sine_440(4410, 44100);
        let out = run_vocoder(&samples, 44100, 4, "bogus_strategy", "noise");
        assert_eq!(&out[0..4], b"RIFF");
    }

    /// Unknown carrier strings must fall back to noise, not panic or return empty.
    #[test]
    fn unknown_carrier_falls_back_to_noise() {
        let samples = sine_440(4410, 44100);
        let out = run_vocoder(&samples, 44100, 4, "cis", "bogus_carrier");
        assert_eq!(&out[0..4], b"RIFF");
    }

    /// All three strategy strings must produce a valid WAV.
    #[test]
    fn all_strategies_produce_valid_wav() {
        let samples = sine_440(4410, 44100);
        for strat in &["cis", "fs4", "fft"] {
            let out = run_vocoder(&samples, 44100, 8, strat, "noise");
            assert_eq!(
                &out[0..4], b"RIFF",
                "strategy '{strat}' did not produce a valid WAV"
            );
        }
    }

    /// Both carrier strings must produce a valid WAV.
    #[test]
    fn both_carriers_produce_valid_wav() {
        let samples = sine_440(4410, 44100);
        for carrier in &["noise", "sine"] {
            let out = run_vocoder(&samples, 44100, 8, "cis", carrier);
            assert_eq!(
                &out[0..4], b"RIFF",
                "carrier '{carrier}' did not produce a valid WAV"
            );
        }
    }

    /// Empty input must return a valid (silent) WAV rather than panicking.
    #[test]
    fn empty_input_returns_valid_wav() {
        let out = run_vocoder(&[], 44100, 8, "cis", "noise");
        assert_eq!(&out[0..4], b"RIFF");
    }

    /// The `channels` parameter must flow through to vocoder::process.
    /// 4-channel and 8-channel outputs must differ (each has a different filter bank).
    #[test]
    fn channel_count_is_wired_through() {
        let samples = sine_440(44100, 44100);
        let out4  = run_vocoder(&samples, 44100, 4,  "cis", "noise");
        let out8  = run_vocoder(&samples, 44100, 8,  "cis", "noise");
        let out16 = run_vocoder(&samples, 44100, 16, "cis", "noise");
        assert_ne!(out4, out8,  "4ch and 8ch outputs must differ");
        assert_ne!(out8, out16, "8ch and 16ch outputs must differ");
    }

    /// run_vocoder must never return empty bytes for valid (non-empty) input.
    /// The JS guard `if (wavBytes.length === 0)` relies on this being the only
    /// way to detect an encode failure.
    #[test]
    fn non_empty_input_never_returns_empty_bytes() {
        let samples = sine_440(4410, 44100);
        for strat in &["cis", "fs4", "fft"] {
            for carrier in &["noise", "sine"] {
                let out = run_vocoder(&samples, 44100, 8, strat, carrier);
                assert!(
                    !out.is_empty(),
                    "run_vocoder returned empty bytes for strategy={strat} carrier={carrier}"
                );
            }
        }
    }

    /// The `sample_rate` parameter must be embedded correctly in the WAV header.
    /// WAV fmt chunk stores sample rate as a little-endian u32 at byte offset 24.
    #[test]
    fn sample_rate_is_embedded_in_wav_header() {
        let samples = sine_440(4410, 44100);
        for &sr in &[22050u32, 44100, 48000] {
            let out = run_vocoder(&samples, sr, 8, "cis", "noise");
            let embedded = u32::from_le_bytes(out[24..28].try_into().unwrap());
            assert_eq!(embedded, sr, "WAV header sample rate should be {sr}, got {embedded}");
        }
    }
}
