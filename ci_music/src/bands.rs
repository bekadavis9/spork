/// Logarithmically-spaced frequency bands between `f_low` and `f_high`.
pub struct Bands {
    /// Inclusive lower edge of each band (Hz).
    pub edges: Vec<f32>,
    /// Center frequency of each band (Hz).
    pub centers: Vec<f32>,
}

impl Bands {
    /// Build `n` log-spaced bands between `f_low` and `f_high`.
    pub fn new(n: usize, f_low: f32, f_high: f32) -> Self {
        // n+1 edges define n bands
        let edges: Vec<f32> = (0..=n)
            .map(|i| f_low * (f_high / f_low).powf(i as f32 / n as f32))
            .collect();

        let centers: Vec<f32> = (0..n)
            .map(|i| (edges[i] * edges[i + 1]).sqrt()) // geometric mean
            .collect();

        Self { edges, centers }
    }

    pub fn len(&self) -> usize {
        self.centers.len()
    }

    /// Return the range of FFT bin indices (exclusive end) that fall within band `i`.
    /// `fft_size` is the full FFT length; `sample_rate` is in Hz.
    pub fn bin_range(&self, i: usize, fft_size: usize, sample_rate: u32) -> (usize, usize) {
        let bin_width = sample_rate as f32 / fft_size as f32;
        let lo = (self.edges[i] / bin_width).round() as usize;
        let hi = (self.edges[i + 1] / bin_width).round() as usize;
        let nyquist = fft_size / 2 + 1;
        (lo.min(nyquist), hi.clamp(lo + 1, nyquist))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_count() {
        let b = Bands::new(8, 70.0, 8500.0);
        assert_eq!(b.len(), 8);
        assert_eq!(b.edges.len(), 9);
    }

    #[test]
    fn edges_are_monotonic() {
        let b = Bands::new(8, 70.0, 8500.0);
        for w in b.edges.windows(2) {
            assert!(w[1] > w[0]);
        }
    }

    #[test]
    fn centers_within_edges() {
        let b = Bands::new(8, 70.0, 8500.0);
        for i in 0..b.len() {
            assert!(b.centers[i] > b.edges[i]);
            assert!(b.centers[i] < b.edges[i + 1]);
        }
    }

    /// The first and last edges must exactly match the F_LOW and F_HIGH
    /// constants. Any drift here shifts every filter center frequency.
    #[test]
    fn edges_span_exact_frequency_range() {
        let (f_low, f_high) = (70.0f32, 8500.0f32);
        let b = Bands::new(8, f_low, f_high);
        assert!(
            (b.edges[0] - f_low).abs() < 0.01,
            "first edge should be f_low={f_low}, got {:.4}", b.edges[0]
        );
        assert!(
            (b.edges[b.len()] - f_high).abs() < 0.01,
            "last edge should be f_high={f_high}, got {:.4}", b.edges[b.len()]
        );
    }

    /// Centers must be geometric means of their bounding edges.
    /// This is what makes the spacing perceptually uniform on a log scale.
    #[test]
    fn centers_are_geometric_means_of_edges() {
        let b = Bands::new(8, 70.0, 8500.0);
        for i in 0..b.len() {
            let expected = (b.edges[i] * b.edges[i + 1]).sqrt();
            assert!(
                (b.centers[i] - expected).abs() < 0.01,
                "center[{i}] should be geometric mean {expected:.4}, got {:.4}", b.centers[i]
            );
        }
    }

    /// Every bin_range must have lo < hi (non-empty range). An empty range
    /// means a band contributes no FFT bins and silently drops from the output.
    #[test]
    fn bin_ranges_are_non_empty() {
        let b = Bands::new(8, 70.0, 8500.0);
        for i in 0..b.len() {
            let (lo, hi) = b.bin_range(i, 1024, 44100);
            assert!(lo < hi, "band {i} has empty bin range: lo={lo}, hi={hi}");
        }
    }

    /// Adjacent bin_ranges must not have gaps — every FFT bin up to the last
    /// band's hi must be covered by exactly one band.
    #[test]
    fn bin_ranges_are_contiguous() {
        let b = Bands::new(8, 70.0, 8500.0);
        for i in 0..(b.len() - 1) {
            let (_, hi_curr) = b.bin_range(i, 1024, 44100);
            let (lo_next, _) = b.bin_range(i + 1, 1024, 44100);
            assert!(
                lo_next <= hi_curr + 1,
                "gap between band {i} (hi={hi_curr}) and band {} (lo={lo_next})", i + 1
            );
        }
    }

    /// bin_range must never exceed the Nyquist bin (fft_size/2 + 1).
    #[test]
    fn bin_ranges_stay_within_nyquist() {
        let b = Bands::new(8, 70.0, 8500.0);
        let nyquist = 1024 / 2 + 1;
        for i in 0..b.len() {
            let (_, hi) = b.bin_range(i, 1024, 44100);
            assert!(hi <= nyquist, "band {i} hi={hi} exceeds nyquist={nyquist}");
        }
    }

    /// bin_range must produce non-empty ranges at 22050 Hz (the lowest common
    /// sample rate). Bin width is halved vs 44100 Hz, so the lowest bands are
    /// at risk of collapsing to a single bin.
    #[test]
    fn bin_ranges_non_empty_at_22050_hz() {
        let b = Bands::new(8, 70.0, 8500.0);
        for i in 0..b.len() {
            let (lo, hi) = b.bin_range(i, 1024, 22050);
            assert!(lo < hi, "band {i} has empty bin range at 22050 Hz: lo={lo}, hi={hi}");
        }
    }

    /// bin_range must produce non-empty ranges at 48000 Hz.
    #[test]
    fn bin_ranges_non_empty_at_48000_hz() {
        let b = Bands::new(8, 70.0, 8500.0);
        for i in 0..b.len() {
            let (lo, hi) = b.bin_range(i, 1024, 48000);
            assert!(lo < hi, "band {i} has empty bin range at 48000 Hz: lo={lo}, hi={hi}");
        }
    }

    /// bin_range must not exceed Nyquist at 22050 Hz.
    #[test]
    fn bin_ranges_stay_within_nyquist_at_22050_hz() {
        let b = Bands::new(8, 70.0, 8500.0);
        let nyquist = 1024 / 2 + 1;
        for i in 0..b.len() {
            let (_, hi) = b.bin_range(i, 1024, 22050);
            assert!(hi <= nyquist, "band {i} hi={hi} exceeds nyquist at 22050 Hz");
        }
    }

    /// bin_range must not exceed Nyquist at 48000 Hz.
    #[test]
    fn bin_ranges_stay_within_nyquist_at_48000_hz() {
        let b = Bands::new(8, 70.0, 8500.0);
        let nyquist = 1024 / 2 + 1;
        for i in 0..b.len() {
            let (_, hi) = b.bin_range(i, 1024, 48000);
            assert!(hi <= nyquist, "band {i} hi={hi} exceeds nyquist at 48000 Hz");
        }
    }

    /// Bands must work correctly with channel counts other than 8 —
    /// used when the user selects 4, 12, or 16 channels.
    #[test]
    fn works_for_all_supported_channel_counts() {
        for &n in &[4usize, 8, 12, 16] {
            let b = Bands::new(n, 70.0, 8500.0);
            assert_eq!(b.len(), n, "wrong band count for n={n}");
            assert_eq!(b.edges.len(), n + 1);
            for w in b.edges.windows(2) {
                assert!(w[1] > w[0], "edges not monotonic for n={n}");
            }
        }
    }
}
