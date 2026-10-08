//! A compact logarithmic FFT frame for UI visualization.

use std::sync::{Arc, OnceLock};

use rustfft::{Fft, FftPlanner, num_complex::Complex32};

pub const BANDS: usize = sofia_protocol::AUDIO_SPECTRUM_BANDS;
const FFT_SIZE: usize = 1024;

fn fft() -> &'static Arc<dyn Fft<f32>> {
    static FFT: OnceLock<Arc<dyn Fft<f32>>> = OnceLock::new();
    FFT.get_or_init(|| FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE))
}

pub fn pcm_i16_bands(pcm: &[u8], sample_rate: u32) -> Vec<f32> {
    if sample_rate == 0 || pcm.len() < 2 {
        return vec![0.0; BANDS];
    }
    let samples = pcm
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0)
        .collect::<Vec<_>>();
    bands(&samples, sample_rate)
}

fn bands(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let mut input = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
    let take = samples.len().min(FFT_SIZE);
    for (index, &sample) in samples[samples.len() - take..].iter().enumerate() {
        let window =
            0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / (FFT_SIZE - 1) as f32).cos();
        input[index].re = sample * window;
    }
    fft().process(&mut input);

    let nyquist = sample_rate as f32 / 2.0;
    let max_frequency = nyquist.min(8_000.0);
    let min_frequency: f32 = 80.0;
    (0..BANDS)
        .map(|band| {
            let low =
                min_frequency * (max_frequency / min_frequency).powf(band as f32 / BANDS as f32);
            let high = min_frequency
                * (max_frequency / min_frequency).powf((band + 1) as f32 / BANDS as f32);
            let lo_bin = ((low * FFT_SIZE as f32 / sample_rate as f32).floor() as usize).max(1);
            let hi_bin =
                ((high * FFT_SIZE as f32 / sample_rate as f32).ceil() as usize).min(FFT_SIZE / 2);
            let peak = input[lo_bin..=hi_bin]
                .iter()
                .map(|value| value.norm() * 4.0 / FFT_SIZE as f32)
                .fold(0.0_f32, f32::max);
            if peak < 0.0001 {
                0.0
            } else {
                ((20.0 * peak.log10() + 70.0) / 65.0).clamp(0.0, 1.0)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero_and_tone_has_a_frequency_peak() {
        assert!(
            bands(&vec![0.0; FFT_SIZE], 24_000)
                .iter()
                .all(|&v| v == 0.0)
        );
        let tone = (0..FFT_SIZE)
            .map(|n| (std::f32::consts::TAU * 1000.0 * n as f32 / 24_000.0).sin() * 0.5)
            .collect::<Vec<_>>();
        let values = bands(&tone, 24_000);
        let peak = values
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert!((9..=13).contains(&peak), "1 kHz peak was in band {peak}");
    }
}
