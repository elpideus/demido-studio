//! What Smart Turn is given: Whisper's log-mel features of the last 8 seconds, made as Smart Turn's own code
//! makes them (`transformers`' `WhisperFeatureExtractor(chunk_length=8)` with `do_normalize=True`, on audio
//! padded with silence at its beginning to 8 seconds). Checked against `transformers` in the tests below.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

pub const RATE: usize = 16_000;
pub const SECONDS: usize = 8;
pub const SAMPLES: usize = RATE * SECONDS;
const N_FFT: usize = 400;
const HOP: usize = 160;
pub const MELS: usize = 80;
pub const FRAMES: usize = SAMPLES / HOP;
const BINS: usize = N_FFT / 2 + 1;

/// The last 8 seconds, or silence before what there is, to 8 seconds.
pub fn last_eight_seconds(speech: &[f32]) -> Vec<f32> {
    if speech.len() >= SAMPLES {
        return speech[speech.len() - SAMPLES..].to_vec();
    }
    let mut out = vec![0.0; SAMPLES - speech.len()];
    out.extend_from_slice(speech);
    out
}

/// Zero mean and unit variance, over all of it (`zero_mean_unit_var_norm`).
fn normalized(x: &[f32]) -> Vec<f32> {
    let n = x.len() as f64;
    let mean = x.iter().map(|&v| v as f64).sum::<f64>() / n;
    let var = x.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n;
    let scale = 1.0 / (var + 1e-7).sqrt();
    x.iter().map(|&v| ((v as f64 - mean) * scale) as f32).collect()
}

fn hz_to_mel(f: f64) -> f64 {
    // Slaney's scale: linear below 1 kHz, logarithmic above.
    let logstep = 27.0 / 6.4f64.ln();
    if f >= 1000.0 { 15.0 + (f / 1000.0).ln() * logstep } else { 3.0 * f / 200.0 }
}

fn mel_to_hz(m: f64) -> f64 {
    let logstep = 6.4f64.ln() / 27.0;
    if m >= 15.0 { 1000.0 * (logstep * (m - 15.0)).exp() } else { 200.0 * m / 3.0 }
}

/// The triangular filters, Slaney-normalised (`mel_filter_bank(201, 80, 0, 8000, 16000, "slaney", "slaney")`):
/// `filters[bin][mel]`.
fn mel_filters() -> Vec<[f64; MELS]> {
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(8000.0));
    let edges: Vec<f64> = (0..MELS + 2).map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (MELS + 1) as f64)).collect();
    let fft_freqs: Vec<f64> = (0..BINS).map(|i| (RATE / 2) as f64 * i as f64 / (BINS - 1) as f64).collect();
    let mut filters = vec![[0.0f64; MELS]; BINS];
    for (bin, &f) in fft_freqs.iter().enumerate() {
        for m in 0..MELS {
            let down = (f - edges[m]) / (edges[m + 1] - edges[m]);
            let up = (edges[m + 2] - f) / (edges[m + 2] - edges[m + 1]);
            let enorm = 2.0 / (edges[m + 2] - edges[m]);
            filters[bin][m] = down.min(up).max(0.0) * enorm;
        }
    }
    filters
}

/// Log-mel features, `[mel][frame]` flattened (80 × 800), of exactly 8 seconds of 16 kHz audio.
pub fn log_mel(audio: &[f32]) -> Vec<f32> {
    assert_eq!(audio.len(), SAMPLES);
    let x = normalized(audio);
    // Centred frames: the signal reflected by half a frame at each end.
    let pad = N_FFT / 2;
    let mut padded = Vec::with_capacity(SAMPLES + 2 * pad);
    padded.extend((1..=pad).rev().map(|i| x[i]));
    padded.extend_from_slice(&x);
    padded.extend((1..=pad).map(|i| x[SAMPLES - 1 - i]));
    // A periodic Hann window.
    let window: Vec<f32> = (0..N_FFT).map(|n| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos()) as f32).collect();
    let filters = mel_filters();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let frames = 1 + (padded.len() - N_FFT) / HOP; // 801; the last is dropped below, as Whisper does
    let mut mel = vec![0.0f64; MELS * FRAMES];
    let mut buffer = vec![Complex32::new(0.0, 0.0); N_FFT];
    for t in 0..frames.min(FRAMES) {
        let start = t * HOP;
        for n in 0..N_FFT {
            buffer[n] = Complex32::new(padded[start + n] * window[n], 0.0);
        }
        fft.process(&mut buffer);
        for (bin, filter) in filters.iter().enumerate() {
            let power = buffer[bin].norm_sqr() as f64;
            if power == 0.0 {
                continue;
            }
            for m in 0..MELS {
                mel[m * FRAMES + t] += filter[m] * power;
            }
        }
    }
    let mut log: Vec<f64> = mel.iter().map(|&v| v.max(1e-10).log10()).collect();
    let top = log.iter().cloned().fold(f64::MIN, f64::max);
    for v in &mut log {
        *v = (v.max(top - 8.0) + 4.0) / 4.0;
    }
    log.into_iter().map(|v| v as f32).collect()
}

/// What Smart Turn's model takes, from speech that ends where the person stopped.
pub fn smart_turn_input(speech: &[f32]) -> Vec<f32> {
    log_mel(&last_eight_seconds(speech))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test signal: 3 seconds of a rising tone with a little made-up noise, then padded as Smart Turn pads.
    fn signal() -> Vec<f32> {
        let n = 3 * RATE;
        let mut seed: u32 = 7;
        (0..n)
            .map(|i| {
                let t = i as f64 / RATE as f64;
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f64 / (1u32 << 24) as f64 - 0.5;
                (0.4 * (2.0 * std::f64::consts::PI * (200.0 + 300.0 * t) * t).sin() + 0.02 * noise) as f32
            })
            .collect()
    }

    #[test]
    fn the_features_are_whispers_as_transformers_makes_them() {
        let f = smart_turn_input(&signal());
        assert_eq!(f.len(), MELS * FRAMES);
        // From transformers 5.22.2's WhisperFeatureExtractor(chunk_length=8), do_normalize=True, on the same
        // signal (core/dev/voice-features-reference.py prints these).
        let at = |m: usize, t: usize| f[m * FRAMES + t];
        let mean = f.iter().map(|&v| v as f64).sum::<f64>() / f.len() as f64;
        let expected = include_str!("features-reference.txt");
        let numbers: Vec<f64> = expected.split_whitespace().filter_map(|w| w.parse().ok()).collect();
        let [want_mean, a, b, c, d, e] = numbers[..] else { panic!("features-reference.txt holds six numbers") };
        let close = |got: f64, want: f64| (got - want).abs() < 2e-3;
        assert!(close(mean, want_mean), "mean {mean} against {want_mean}");
        for (got, want, which) in [
            (at(0, 0), a, "mel 0, frame 0"),
            (at(10, 600), b, "mel 10, frame 600"),
            (at(40, 700), c, "mel 40, frame 700"),
            (at(79, 799), d, "mel 79, frame 799"),
            (at(20, 500), e, "mel 20, frame 500"),
        ] {
            assert!(close(got as f64, want), "{which}: {got} against {want}");
        }
    }

    #[test]
    fn more_than_eight_seconds_keeps_the_end_and_less_is_padded_at_the_start() {
        let long: Vec<f32> = (0..10 * RATE).map(|i| i as f32).collect();
        let kept = last_eight_seconds(&long);
        assert_eq!((kept.len(), kept[0], *kept.last().unwrap()), (SAMPLES, (2 * RATE) as f32, (10 * RATE - 1) as f32));
        let short = last_eight_seconds(&[1.0, 2.0]);
        assert_eq!((short.len(), short[0], short[SAMPLES - 2], short[SAMPLES - 1]), (SAMPLES, 0.0, 1.0, 2.0));
    }
}
