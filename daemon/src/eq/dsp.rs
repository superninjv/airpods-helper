//! Biquad filters (RBJ Audio EQ Cookbook), used by the PulseAudio backend to
//! process audio in-process and by tests to check what a preset actually does.
//! The PipeWire backend uses PipeWire's own builtin biquads, which implement
//! the same cookbook formulas.

use super::preset::{EqBand, EqPreset, FilterType};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coeffs {
    pub const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    pub fn for_band(band: &EqBand, sample_rate: f64) -> Self {
        let nyquist = sample_rate / 2.0;
        // Filters at/above Nyquist are degenerate; treat as a pass-through.
        if band.freq <= 0.0 || band.freq >= nyquist * 0.999 {
            return Self::IDENTITY;
        }
        let w0 = 2.0 * PI * band.freq / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * band.q);
        let a = 10f64.powf(band.gain / 40.0);
        let sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let (b0, b1, b2, a0, a1, a2) = match band.filter_type {
            FilterType::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterType::Lowshelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + sqrt_a_alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - sqrt_a_alpha),
                (a + 1.0) + (a - 1.0) * cos + sqrt_a_alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - sqrt_a_alpha,
            ),
            FilterType::Highshelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + sqrt_a_alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - sqrt_a_alpha),
                (a + 1.0) - (a - 1.0) * cos + sqrt_a_alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - sqrt_a_alpha,
            ),
            FilterType::Lowpass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterType::Highpass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterType::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// |H(e^jw)| in dB at `freq`.
    pub fn magnitude_db(&self, freq: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * PI * freq / sample_rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = num_re * num_re + num_im * num_im;
        let den = den_re * den_re + den_im * den_im;
        10.0 * (num / den).log10()
    }
}

/// Total response of a preset (preamp + all bands) in dB at `freq`.
pub fn response_db(preset: &EqPreset, freq: f64, sample_rate: f64) -> f64 {
    preset.preamp
        + preset
            .bands
            .iter()
            .map(|b| Coeffs::for_band(b, sample_rate).magnitude_db(freq, sample_rate))
            .sum::<f64>()
}

/// Peak gain of the preset over the audible range — anything above 0 dB can
/// clip at full-scale input.
pub fn peak_gain_db(preset: &EqPreset, sample_rate: f64) -> f64 {
    (0..=240)
        .map(|i| 20.0 * 1000f64.powf(i as f64 / 240.0))
        .map(|f| response_db(preset, f, sample_rate))
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Transposed direct-form II biquad state for one channel.
#[derive(Debug, Clone, Copy, Default)]
struct State {
    z1: f64,
    z2: f64,
}

/// Stereo (or N-channel) processor for interleaved f32 audio.
pub struct Processor {
    gain: f64,
    coeffs: Vec<Coeffs>,
    /// state[channel][band]
    state: Vec<Vec<State>>,
    channels: usize,
}

impl Processor {
    pub fn new(preset: &EqPreset, sample_rate: f64, channels: usize) -> Self {
        let coeffs: Vec<Coeffs> = preset
            .bands
            .iter()
            .map(|b| Coeffs::for_band(b, sample_rate))
            .collect();
        Self {
            gain: 10f64.powf(preset.preamp / 20.0),
            state: vec![vec![State::default(); coeffs.len()]; channels],
            coeffs,
            channels,
        }
    }

    /// Process interleaved samples in place. Output is hard-limited to
    /// [-1, 1] so a hot preset degrades to clipping rather than wrapping.
    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(self.channels) {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let input = *sample as f64;
                if !input.is_finite() {
                    // One NaN would poison the filter state forever.
                    self.state[ch].iter_mut().for_each(|s| *s = State::default());
                    *sample = 0.0;
                    continue;
                }
                let mut x = input * self.gain;
                for (c, s) in self.coeffs.iter().zip(self.state[ch].iter_mut()) {
                    let y = c.b0 * x + s.z1;
                    s.z1 = c.b1 * x - c.a1 * y + s.z2;
                    s.z2 = c.b2 * x - c.a2 * y;
                    x = y;
                }
                *sample = x.clamp(-1.0, 1.0) as f32;
            }
        }
        // Flush denormals that build up in the feedback path during silence.
        for s in self.state.iter_mut().flatten() {
            if s.z1.abs() < 1e-20 {
                s.z1 = 0.0;
            }
            if s.z2.abs() < 1e-20 {
                s.z2 = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 48_000.0;

    fn band(t: FilterType, freq: f64, q: f64, gain: f64) -> EqBand {
        EqBand {
            filter_type: t,
            freq,
            q,
            gain,
        }
    }

    fn preset(preamp: f64, bands: Vec<EqBand>) -> EqPreset {
        EqPreset {
            id: "t".into(),
            name: "t".into(),
            description: String::new(),
            preamp,
            bands,
        }
    }

    #[test]
    fn peaking_hits_gain_at_center() {
        let c = Coeffs::for_band(&band(FilterType::Peaking, 1000.0, 1.0, 6.0), FS);
        assert!((c.magnitude_db(1000.0, FS) - 6.0).abs() < 0.01);
        assert!(c.magnitude_db(50.0, FS).abs() < 0.2);
        assert!(c.magnitude_db(15_000.0, FS).abs() < 0.2);
    }

    #[test]
    fn shelves_reach_gain_far_from_corner() {
        let ls = Coeffs::for_band(&band(FilterType::Lowshelf, 200.0, 0.707, 6.0), FS);
        assert!((ls.magnitude_db(20.0, FS) - 6.0).abs() < 0.1);
        assert!(ls.magnitude_db(10_000.0, FS).abs() < 0.1);
        let hs = Coeffs::for_band(&band(FilterType::Highshelf, 4000.0, 0.707, -4.0), FS);
        assert!((hs.magnitude_db(20_000.0, FS) + 4.0).abs() < 0.2);
        assert!(hs.magnitude_db(50.0, FS).abs() < 0.1);
    }

    #[test]
    fn lowpass_is_minus_3db_at_corner_for_butterworth_q() {
        let lp = Coeffs::for_band(
            &band(
                FilterType::Lowpass,
                1000.0,
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
            ),
            FS,
        );
        assert!((lp.magnitude_db(1000.0, FS) + 3.01).abs() < 0.05);
        assert!(lp.magnitude_db(10_000.0, FS) < -30.0);
    }

    #[test]
    fn response_sums_preamp_and_bands() {
        let p = preset(-3.0, vec![band(FilterType::Peaking, 1000.0, 1.0, 6.0)]);
        assert!((response_db(&p, 1000.0, FS) - 3.0).abs() < 0.01);
        assert!((peak_gain_db(&p, FS) - 3.0).abs() < 0.05);
    }

    #[test]
    fn degenerate_band_is_identity() {
        let c = Coeffs::for_band(&band(FilterType::Peaking, 30_000.0, 1.0, 6.0), FS);
        assert_eq!(c, Coeffs::IDENTITY);
    }

    /// Feed a sine through the processor and measure steady-state gain; this
    /// checks the time-domain implementation agrees with the analytic response.
    #[test]
    fn processor_matches_analytic_response() {
        let p = preset(
            -2.0,
            vec![
                band(FilterType::Lowshelf, 105.0, 0.7, 6.0),
                band(FilterType::Peaking, 3000.0, 2.0, -4.0),
            ],
        );
        for freq in [60.0, 1000.0, 3000.0] {
            let mut proc = Processor::new(&p, FS, 2);
            let n = 48_000;
            let mut buf: Vec<f32> = (0..n)
                .flat_map(|i| {
                    let v = 0.25 * (2.0 * PI * freq * i as f64 / FS).sin();
                    [v as f32, v as f32]
                })
                .collect();
            proc.process(&mut buf);
            // RMS of the second half (after transients), left channel.
            let tail: Vec<f64> = buf[n..].iter().step_by(2).map(|&s| s as f64).collect();
            let rms = (tail.iter().map(|s| s * s).sum::<f64>() / tail.len() as f64).sqrt();
            let measured = 20.0 * (rms / (0.25 / 2f64.sqrt())).log10();
            let expected = response_db(&p, freq, FS);
            assert!(
                (measured - expected).abs() < 0.1,
                "{freq} Hz: measured {measured:.2} expected {expected:.2}"
            );
        }
    }

    #[test]
    fn nan_input_does_not_poison_the_filter() {
        let p = preset(0.0, vec![band(FilterType::Peaking, 1000.0, 1.0, 6.0)]);
        let mut proc = Processor::new(&p, FS, 1);
        let mut buf = vec![0.5f32, f32::NAN, 0.5, 0.25];
        proc.process(&mut buf);
        assert_eq!(buf[1], 0.0);
        assert!(buf.iter().all(|s| s.is_finite()), "{buf:?}");
    }
}
