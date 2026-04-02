/// Multi-format audio decoding via Symphonia.
///
/// Supports WAV, MP3, FLAC, OGG/Vorbis, AAC, and M4A. All formats are decoded
/// to mono f32 samples at the file's native sample rate. Stereo and multi-channel
/// inputs are downmixed by arithmetic mean.
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decode an audio file from disk. Supports WAV, MP3, FLAC, OGG/Vorbis, AAC, M4A.
/// Returns (mono f32 samples, sample_rate).
pub fn load_audio_file(path: &str) -> Result<(Vec<f32>, u32), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    decode(mss, hint)
}

/// Decode audio from raw bytes. Optionally provide a file extension hint (e.g. `"mp3"`,
/// `"flac"`) to improve format detection — particularly useful for MP3, which has no
/// magic bytes. Falls back to content probing if no hint is given.
/// Returns (mono f32 samples, sample_rate).
pub fn decode_audio_bytes(data: &[u8], ext_hint: Option<&str>) -> Result<(Vec<f32>, u32), String> {
    let cursor = std::io::Cursor::new(data.to_vec());
    let mut hint = Hint::new();
    if let Some(ext) = ext_hint {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    decode(mss, hint)
}

fn decode(mss: MediaSourceStream, hint: Hint) -> Result<(Vec<f32>, u32), String> {
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("unrecognised audio format: {e}"))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "no supported audio track found".to_string())?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "audio track has no sample rate".to_string())?;

    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("unsupported codec: {e}"))?;

    let mut all_samples: Vec<f32> = Vec::new();
    let mut actual_channels: usize = 1;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(format!("error reading audio: {e}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                actual_channels = decoded.spec().channels.count();
                let spec = *decoded.spec();
                let mut sample_buf =
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                sample_buf.copy_interleaved_ref(decoded);
                all_samples.extend_from_slice(sample_buf.samples());
            }
            Err(SymphoniaError::DecodeError(_)) => continue, // skip malformed frames
            Err(e) => return Err(format!("decode error: {e}")),
        }
    }

    if all_samples.is_empty() {
        return Err("audio file contained no decodable samples".to_string());
    }

    // Downmix interleaved multi-channel to mono
    let mono = if actual_channels == 1 {
        all_samples
    } else {
        all_samples
            .chunks(actual_channels)
            .map(|frame| frame.iter().sum::<f32>() / actual_channels as f32)
            .collect()
    };

    Ok((mono, sample_rate))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod audio_tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 44100;

    /// Build a synthetic mono 32-bit float WAV using hound.
    fn make_wav(samples: &[f32], channels: u16, sr: u32, bits: u16) -> Vec<u8> {
        use hound::{SampleFormat, WavSpec, WavWriter};
        let spec = WavSpec {
            channels,
            sample_rate: sr,
            bits_per_sample: bits,
            sample_format: if bits == 32 && samples.iter().any(|s| s.fract() != 0.0) {
                SampleFormat::Float
            } else {
                SampleFormat::Float // always f32 for simplicity in test helpers
            },
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = WavWriter::new(&mut cursor, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        cursor.into_inner()
    }

    fn sine(freq: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / SR as f32).sin())
            .collect()
    }

    // ── Basic decode ─────────────────────────────────────────────────────────

    /// Decoding a 32-bit float WAV must return the right sample rate and count.
    #[test]
    fn decode_wav_returns_correct_metadata() {
        let wav = make_wav(&sine(440.0, 4410), 1, SR, 32);
        let (samples, sr) = decode_audio_bytes(&wav, Some("wav")).unwrap();
        assert_eq!(sr, SR, "sample rate must survive decode");
        assert_eq!(samples.len(), 4410, "sample count must survive decode");
    }

    /// Format probing from magic bytes alone must work when no extension hint is given.
    #[test]
    fn decode_wav_without_hint_succeeds() {
        let wav = make_wav(&sine(440.0, 4410), 1, SR, 32);
        let (samples, sr) = decode_audio_bytes(&wav, None).unwrap();
        assert_eq!(sr, SR);
        assert_eq!(samples.len(), 4410);
    }

    /// Garbage bytes must return Err, not panic.
    #[test]
    fn decode_garbage_bytes_returns_error() {
        let result = decode_audio_bytes(b"this is not audio at all", None);
        assert!(result.is_err(), "garbage bytes must produce Err");
    }

    /// Empty byte slice must return Err.
    #[test]
    fn decode_empty_bytes_returns_error() {
        let result = decode_audio_bytes(&[], None);
        assert!(result.is_err(), "empty input must produce Err");
    }

    // ── Round-trip fidelity ───────────────────────────────────────────────────

    /// Encoding as 32-bit float WAV and decoding back must be lossless.
    #[test]
    fn decode_wav_roundtrip_is_lossless() {
        let original = sine(440.0, 4410);
        let wav = make_wav(&original, 1, SR, 32);
        let (decoded, _) = decode_audio_bytes(&wav, None).unwrap();
        assert_eq!(decoded.len(), original.len());
        for (i, (&orig, &dec)) in original.iter().zip(decoded.iter()).enumerate() {
            assert!(
                (orig - dec).abs() < 1e-6,
                "round-trip error at sample {i}: {orig} vs {dec}"
            );
        }
    }

    // ── Stereo downmix ───────────────────────────────────────────────────────

    /// Stereo WAV must be downmixed to mono: sample count halves.
    #[test]
    fn decode_stereo_wav_downmixes_to_mono() {
        // Interleave two channels: L and R identical
        let frames = 1000usize;
        let mut stereo = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = (2.0 * PI * 440.0 * i as f32 / SR as f32).sin();
            stereo.push(s); // L
            stereo.push(s); // R
        }
        let wav = make_wav(&stereo, 2, SR, 32);
        let (mono, _) = decode_audio_bytes(&wav, None).unwrap();
        assert_eq!(mono.len(), frames, "stereo → mono: frame count must halve");
    }

    /// Stereo downmix: L = 1.0, R = −1.0 must cancel to silence.
    #[test]
    fn decode_stereo_cancellation_produces_silence() {
        let frames = 100usize;
        let stereo: Vec<f32> = (0..frames).flat_map(|_| [1.0f32, -1.0f32]).collect();
        let wav = make_wav(&stereo, 2, SR, 32);
        let (mono, _) = decode_audio_bytes(&wav, None).unwrap();
        assert_eq!(mono.len(), frames);
        for (i, &s) in mono.iter().enumerate() {
            assert!(
                s.abs() < 1e-5,
                "L=1 R=−1 should cancel to 0.0; got {s} at frame {i}"
            );
        }
    }

    /// Stereo downmix: L = 0.8, R = 0.4 must average to 0.6.
    #[test]
    fn decode_stereo_averaging_is_correct() {
        let frames = 100usize;
        let stereo: Vec<f32> = (0..frames).flat_map(|_| [0.8f32, 0.4f32]).collect();
        let wav = make_wav(&stereo, 2, SR, 32);
        let (mono, _) = decode_audio_bytes(&wav, None).unwrap();
        for (i, &s) in mono.iter().enumerate() {
            assert!(
                (s - 0.6).abs() < 1e-5,
                "stereo average should be 0.6, got {s} at frame {i}"
            );
        }
    }

    // ── Integer format decoding ───────────────────────────────────────────────

    /// 16-bit integer WAV: i16::MAX must decode to ≈ +1.0, i16::MIN to −1.0,
    /// zero to 0.0. Tolerance 5e-4 accounts for symphonia's PCM normalisation.
    #[test]
    fn decode_16bit_int_wav_normalises_correctly() {
        use hound::{SampleFormat, WavSpec, WavWriter};
        let spec = WavSpec {
            channels: 1,
            sample_rate: SR,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut writer = WavWriter::new(&mut cursor, spec).unwrap();
        writer.write_sample(i16::MAX).unwrap(); // 32767
        writer.write_sample(i16::MIN).unwrap(); // −32768
        writer.write_sample(0i16).unwrap();
        writer.finalize().unwrap();

        let (samples, _) = decode_audio_bytes(cursor.get_ref(), Some("wav")).unwrap();
        assert_eq!(samples.len(), 3);
        // Standard PCM normalisation: value / 2^(bits-1)
        assert!(
            (samples[0] - 1.0).abs() < 5e-4,
            "i16::MAX should be near +1.0, got {:.6}", samples[0]
        );
        assert!(
            (samples[1] - (-1.0)).abs() < 5e-4,
            "i16::MIN should be near −1.0, got {:.6}", samples[1]
        );
        assert!(
            samples[2].abs() < 1e-6,
            "zero must decode to 0.0, got {:.6}", samples[2]
        );
    }
}
