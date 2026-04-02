/// Biquad IIR filter using transposed direct form II.
/// Numerically robust and suitable for audio-rate processing.
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl Biquad {
    fn new(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// 2nd-order Butterworth bandpass (Audio EQ Cookbook, constant 0 dB peak gain).
/// `q = f_center / bandwidth`
pub fn bandpass(f_center: f32, q: f32, sample_rate: f32) -> Biquad {
    let w0 = 2.0 * std::f32::consts::PI * f_center / sample_rate;
    let alpha = w0.sin() / (2.0 * q);
    Biquad::new(
        alpha,
        0.0,
        -alpha,
        1.0 + alpha,
        -2.0 * w0.cos(),
        1.0 - alpha,
    )
}

/// 2nd-order Butterworth lowpass. Used for envelope extraction.
/// Q = 1/√2 (maximally flat / Butterworth): gain is exactly -3 dB at f_cutoff.
pub fn lowpass(f_cutoff: f32, sample_rate: f32) -> Biquad {
    let w0 = 2.0 * std::f32::consts::PI * f_cutoff / sample_rate;
    let alpha = w0.sin() * std::f32::consts::FRAC_1_SQRT_2; // Q = 1/√2: alpha = sin(w0)/(2*Q) = sin(w0)*√2/2
    let cos_w0 = w0.cos();
    Biquad::new(
        (1.0 - cos_w0) / 2.0,
        1.0 - cos_w0,
        (1.0 - cos_w0) / 2.0,
        1.0 + alpha,
        -2.0 * cos_w0,
        1.0 - alpha,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────
// Strategy: feed known sinusoids into each filter and measure the steady-state
// peak amplitude after a fixed warmup period. This directly verifies the
// frequency-response guarantees from the Audio EQ Cookbook and Butterworth
// filter theory.
//
// All tests use FS = 44100 Hz and a 100 ms (4410-sample) warmup, which is
// conservative for all filters in this codebase (worst-case ring-down at
// Q ≈ 4, 1000 Hz centre is well under 10 ms).

#[cfg(test)]
mod filter_tests {
    use super::*;
    use std::f32::consts::PI;

    const FS: f32 = 44100.0;
    const WARMUP: usize = 4410; // 100 ms

    /// Feed a sinusoid at `freq` Hz into `filter`, discard the transient
    /// (WARMUP samples), then return the peak amplitude over 5 full cycles.
    fn gain_at(filter: &mut Biquad, freq: f32) -> f32 {
        let inc = 2.0 * PI * freq / FS;
        let measure = ((FS / freq) * 5.0) as usize + 1;
        for i in 0..WARMUP {
            filter.process((inc * i as f32).sin());
        }
        let mut peak = 0.0f32;
        for i in WARMUP..(WARMUP + measure) {
            peak = peak.max(filter.process((inc * i as f32).sin()).abs());
        }
        peak
    }

    // ── Bandpass ─────────────────────────────────────────────────────────────

    /// Audio EQ Cookbook "constant 0 dB peak gain" BPF: gain must equal 1.0
    /// at f_center by design.
    #[test]
    fn bandpass_unity_gain_at_center_frequency() {
        let mut f = bandpass(1000.0, 4.0, FS);
        let g = gain_at(&mut f, 1000.0);
        assert!(
            (g - 1.0).abs() < 0.05,
            "expected 0 dB gain at center frequency, got {g:.4}"
        );
    }

    #[test]
    fn bandpass_attenuates_one_octave_above_center() {
        let mut f = bandpass(1000.0, 4.0, FS);
        let g = gain_at(&mut f, 2000.0);
        assert!(g < 0.5, "expected >6 dB attenuation one octave above center, got {g:.4}");
    }

    #[test]
    fn bandpass_attenuates_one_octave_below_center() {
        let mut f = bandpass(1000.0, 4.0, FS);
        let g = gain_at(&mut f, 500.0);
        assert!(g < 0.5, "expected >6 dB attenuation one octave below center, got {g:.4}");
    }

    /// 2nd-order BPF rolls off at ~40 dB/decade outside the passband.
    /// Two octaves away should give well over 10 dB attenuation (gain < 0.15).
    #[test]
    fn bandpass_attenuates_two_octaves_away() {
        let mut f_hi = bandpass(1000.0, 4.0, FS);
        let mut f_lo = bandpass(1000.0, 4.0, FS);
        let g_hi = gain_at(&mut f_hi, 4000.0);
        let g_lo = gain_at(&mut f_lo, 250.0);
        assert!(g_hi < 0.15, "expected strong attenuation 2 octaves above center, got {g_hi:.4}");
        assert!(g_lo < 0.15, "expected strong attenuation 2 octaves below center, got {g_lo:.4}");
    }

    /// A bandpass filter's H(0) = 0; after feeding DC (constant 1.0) the
    /// output must converge to zero.
    #[test]
    fn bandpass_rejects_dc() {
        let mut f = bandpass(1000.0, 4.0, FS);
        let mut last = 0.0f32;
        for _ in 0..22050 {
            last = f.process(1.0);
        }
        assert!(
            last.abs() < 0.001,
            "bandpass must reject DC; output after 22050 constant-1 samples was {last:.6}"
        );
    }

    /// The CIS and FS4 strategies cascade two identical 2nd-order BPFs per
    /// channel. The composite 4th-order filter must still satisfy the 0 dB
    /// peak-gain guarantee at f_center (each stage contributes 0 dB, so the
    /// product is still 0 dB).
    #[test]
    fn bandpass_cascaded_unity_gain_at_center() {
        let center = 1000.0;
        let mut f1 = bandpass(center, 4.0, FS);
        let mut f2 = bandpass(center, 4.0, FS);
        let inc = 2.0 * PI * center / FS;
        let measure = ((FS / center) * 5.0) as usize + 1;
        for i in 0..WARMUP {
            f2.process(f1.process((inc * i as f32).sin()));
        }
        let mut peak = 0.0f32;
        for i in WARMUP..(WARMUP + measure) {
            peak = peak.max(f2.process(f1.process((inc * i as f32).sin())).abs());
        }
        assert!(
            (peak - 1.0).abs() < 0.05,
            "cascaded 4th-order BPF should have 0 dB gain at center, got {peak:.4}"
        );
    }

    /// Cascading two 2nd-order BPFs produces a 4th-order filter whose rolloff
    /// must be steeper than a single 2nd-order filter at the same off-centre
    /// frequency.
    #[test]
    fn bandpass_cascaded_rolls_off_steeper_than_single() {
        let center = 1000.0;
        let test_freq = 2000.0; // one octave above

        let mut single = bandpass(center, 4.0, FS);
        let g_single = gain_at(&mut single, test_freq);

        let mut f1 = bandpass(center, 4.0, FS);
        let mut f2 = bandpass(center, 4.0, FS);
        let inc = 2.0 * PI * test_freq / FS;
        let measure = ((FS / test_freq) * 5.0) as usize + 1;
        for i in 0..WARMUP {
            f2.process(f1.process((inc * i as f32).sin()));
        }
        let mut g_cascade = 0.0f32;
        for i in WARMUP..(WARMUP + measure) {
            g_cascade = g_cascade.max(f2.process(f1.process((inc * i as f32).sin())).abs());
        }
        assert!(
            g_cascade < g_single,
            "4th-order cascade must attenuate more than 2nd-order at {test_freq} Hz; \
             single={g_single:.4}, cascade={g_cascade:.4}"
        );
    }

    /// Stability check: IIR filters can blow up with bad coefficients or
    /// accumulated rounding error. 5 s of sustained audio must not produce
    /// NaN or inf.
    #[test]
    fn bandpass_is_stable_over_long_run() {
        let mut f = bandpass(1000.0, 4.0, FS);
        for i in 0..220_500usize {
            let y = f.process(((2.0 * PI * 1000.0 / FS) * i as f32).sin() * 0.5);
            assert!(y.is_finite(), "bandpass produced non-finite output at sample {i}");
        }
    }

    // ── Lowpass ──────────────────────────────────────────────────────────────

    /// 50 Hz is a decade below the 400 Hz envelope cutoff; gain must be close
    /// to 1.0 — slow amplitude modulations (rhythm) must pass through intact.
    #[test]
    fn lowpass_near_unity_gain_well_below_cutoff() {
        let mut f = lowpass(400.0, FS);
        let g = gain_at(&mut f, 50.0);
        assert!(g > 0.95, "LPF should pass frequencies well below cutoff, got {g:.4}");
    }

    /// Butterworth Q = 1/√2 design guarantee: gain at f_cutoff is exactly
    /// 1/√2 ≈ 0.707 (−3 dB). This is the definition of cutoff frequency.
    #[test]
    fn lowpass_minus3db_at_cutoff() {
        let cutoff = 400.0;
        let mut f = lowpass(cutoff, FS);
        let g = gain_at(&mut f, cutoff);
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (g - expected).abs() < 0.05,
            "Butterworth LPF must be −3 dB (0.707) at cutoff, got {g:.4}"
        );
    }

    #[test]
    fn lowpass_attenuates_one_octave_above_cutoff() {
        let cutoff = 400.0;
        let mut f = lowpass(cutoff, FS);
        let g = gain_at(&mut f, cutoff * 2.0);
        assert!(g < 0.4, "LPF should attenuate 1 octave above cutoff, got {g:.4}");
    }

    /// 2nd-order Butterworth: −12 dB/octave asymptotic rolloff.
    /// At 2 octaves above cutoff, attenuation should be ≥ 24 dB → gain < 0.063.
    #[test]
    fn lowpass_strongly_attenuates_two_octaves_above_cutoff() {
        let cutoff = 400.0;
        let mut f = lowpass(cutoff, FS);
        let g = gain_at(&mut f, cutoff * 4.0);
        assert!(g < 0.1, "LPF should strongly attenuate 2 octaves above cutoff, got {g:.4}");
    }

    /// 2nd-order Butterworth LPF with a 400 Hz cutoff must virtually silence
    /// near-Nyquist content. At 50× the cutoff the asymptotic −12 dB/octave
    /// rolloff gives well over 60 dB of attenuation (gain < 0.001). This is
    /// the regime where envelope LPF artefacts would be most audible.
    #[test]
    fn lowpass_heavily_attenuates_near_nyquist() {
        let mut f = lowpass(400.0, FS);
        let g = gain_at(&mut f, 20000.0);
        assert!(
            g < 0.001,
            "LPF should heavily attenuate near-Nyquist (20000 Hz), got {g:.6}"
        );
    }

    #[test]
    fn lowpass_is_stable_over_long_run() {
        let mut f = lowpass(400.0, FS);
        for i in 0..220_500usize {
            let y = f.process(((2.0 * PI * 200.0 / FS) * i as f32).sin() * 0.5);
            assert!(y.is_finite(), "lowpass produced non-finite output at sample {i}");
        }
    }

    // ── Biquad state and algebraic properties ────────────────────────────────

    /// A filter initialised to zero state must output only zeros when fed
    /// only zeros (no phantom energy from uninitialised state variables).
    #[test]
    fn biquad_zero_input_produces_zero_output() {
        let mut f = bandpass(1000.0, 4.0, FS);
        for n in 0..1000 {
            let y = f.process(0.0);
            assert_eq!(y, 0.0, "zero input must produce zero output at sample {n}");
        }
    }

    /// After a single non-zero sample (impulse), output must decay back to
    /// essentially zero. Divergence here indicates an unstable filter.
    #[test]
    fn biquad_impulse_response_decays_to_zero() {
        let mut f = bandpass(1000.0, 4.0, FS);
        f.process(1.0); // impulse
        let mut last = 0.0f32;
        for _ in 0..44100 {
            last = f.process(0.0);
        }
        assert!(
            last.abs() < 1e-6,
            "bandpass impulse response must decay to near zero; value after 44100 zeros was {last:.2e}"
        );
    }

    /// Two identically-constructed filters fed the same input must produce
    /// bit-identical output (no hidden mutable global state).
    #[test]
    fn biquad_is_deterministic() {
        let signal: Vec<f32> = (0..1000)
            .map(|i| ((2.0 * PI * 440.0 / FS) * i as f32).sin())
            .collect();
        let mut f1 = bandpass(1000.0, 4.0, FS);
        let mut f2 = bandpass(1000.0, 4.0, FS);
        for (i, &x) in signal.iter().enumerate() {
            let y1 = f1.process(x);
            let y2 = f2.process(x);
            assert_eq!(y1, y2, "identical filters diverged at sample {i}: {y1} vs {y2}");
        }
    }

    /// Biquad filters are LTI: scaling the input by α must scale the output
    /// by the same α (homogeneity). Tests f(α·x[n]) == α·f(x[n]) for all n,
    /// starting from zero state.
    #[test]
    fn biquad_is_linear() {
        let a = 0.5_f32;
        let signal: Vec<f32> = (0..500)
            .map(|i| ((2.0 * PI * 500.0 / FS) * i as f32).sin())
            .collect();
        let mut f1 = bandpass(1000.0, 4.0, FS);
        let scaled_input_out: Vec<f32> = signal.iter().map(|&x| f1.process(x * a)).collect();
        let mut f2 = bandpass(1000.0, 4.0, FS);
        let scaled_output_out: Vec<f32> = signal.iter().map(|&x| f2.process(x) * a).collect();
        for (i, (&y1, &y2)) in scaled_input_out.iter().zip(scaled_output_out.iter()).enumerate() {
            assert!(
                (y1 - y2).abs() < 1e-5,
                "LTI homogeneity violated at sample {i}: f(a·x)={y1:.8}, a·f(x)={y2:.8}"
            );
        }
    }
}
