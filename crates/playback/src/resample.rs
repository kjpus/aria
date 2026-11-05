use rubato::{
    audioadapter_buffers::direct::InterleavedSlice,
    Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

use crate::PlaybackError;

/// Selects the target sample rate according to the playback rules:
/// 1. If and only if the track's sample rate is not supported, it should be re-sampled.
///    -> If the source rate is in supported_rates, return it directly (native bit-perfect).
/// 2. Up-sampling is preferred, but if only lower rate is supported, use down-sampling.
///    -> If any supported rate > source_rate exists, choose the nearest higher rate.
/// 3. Re-sample to the nearest supported sample rate.
///    -> If only lower supported rates exist, choose the nearest lower rate.
pub fn select_resample_rate(source_rate: u32, supported_rates: &[u32]) -> u32 {
    if supported_rates.is_empty() {
        return source_rate;
    }

    if supported_rates.contains(&source_rate) {
        return source_rate;
    }

    let higher_rates: Vec<u32> = supported_rates
        .iter()
        .copied()
        .filter(|&r| r > source_rate)
        .collect();

    if !higher_rates.is_empty() {
        return *higher_rates
            .iter()
            .min_by_key(|&&r| r - source_rate)
            .unwrap();
    }

    let lower_rates: Vec<u32> = supported_rates
        .iter()
        .copied()
        .filter(|&r| r < source_rate)
        .collect();

    if !lower_rates.is_empty() {
        return *lower_rates
            .iter()
            .min_by_key(|&&r| source_rate - r)
            .unwrap();
    }

    *supported_rates
        .iter()
        .min_by_key(|&&r| (r as i64 - source_rate as i64).abs())
        .unwrap()
}

pub struct AudioResampler {
    resampler: Option<Async<f32>>,
    channels: usize,
    in_rate: u32,
    out_rate: u32,
    in_buffer: Vec<f32>,
    chunk_frames_in: usize,
    max_chunk_frames_out: usize,
    delay_frames_to_drop: usize,
}

impl AudioResampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize) -> Result<Self, PlaybackError> {
        if in_rate == out_rate || channels == 0 {
            return Ok(Self {
                resampler: None,
                channels: channels.max(1),
                in_rate,
                out_rate,
                in_buffer: Vec::new(),
                chunk_frames_in: 0,
                max_chunk_frames_out: 0,
                delay_frames_to_drop: 0,
            });
        }

        let f_ratio = out_rate as f64 / in_rate as f64;
        let chunk_frames_in = 1024;
        let params = SincInterpolationParameters::new(64, WindowFunction::BlackmanHarris2)
            .oversampling_factor(128)
            .interpolation(SincInterpolationType::Linear);

        let resampler = Async::<f32>::new_sinc(
            f_ratio,
            1.1,
            &params,
            chunk_frames_in,
            channels,
            FixedAsync::Input,
        )
        .map_err(|e| PlaybackError::Output(format!("Failed to create audio resampler: {e}")))?;

        let max_chunk_frames_out = resampler.output_frames_max();
        let delay_frames_to_drop = resampler.output_delay();

        Ok(Self {
            resampler: Some(resampler),
            channels,
            in_rate,
            out_rate,
            in_buffer: Vec::new(),
            chunk_frames_in,
            max_chunk_frames_out,
            delay_frames_to_drop,
        })
    }

    pub fn is_resampling(&self) -> bool {
        self.resampler.is_some()
    }

    #[allow(dead_code)]
    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }

    #[allow(dead_code)]
    pub fn out_rate(&self) -> u32 {
        self.out_rate
    }

    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut Vec<f32>,
    ) -> Result<(), PlaybackError> {
        let Some(ref mut resampler) = self.resampler else {
            output.extend_from_slice(input);
            return Ok(());
        };

        self.in_buffer.extend_from_slice(input);

        let mut temp_out = vec![0.0f32; self.max_chunk_frames_out * self.channels];

        while self.in_buffer.len() / self.channels >= self.chunk_frames_in {
            let chunk_samples = self.chunk_frames_in * self.channels;
            let in_adapter = InterleavedSlice::new(
                &self.in_buffer[..chunk_samples],
                self.channels,
                self.chunk_frames_in,
            )
            .map_err(|e| PlaybackError::Output(e.to_string()))?;

            let mut out_adapter = InterleavedSlice::new_mut(
                &mut temp_out,
                self.channels,
                self.max_chunk_frames_out,
            )
            .map_err(|e| PlaybackError::Output(e.to_string()))?;

            let (consumed, produced) = resampler
                .process_into_buffer(&in_adapter, &mut out_adapter, None)
                .map_err(|e| PlaybackError::Output(e.to_string()))?;

            self.in_buffer.drain(..consumed * self.channels);

            let produced_samples = produced * self.channels;
            let produced_slice = &temp_out[..produced_samples];

            if self.delay_frames_to_drop > 0 {
                let samples_to_drop =
                    (self.delay_frames_to_drop * self.channels).min(produced_slice.len());
                output.extend_from_slice(&produced_slice[samples_to_drop..]);
                self.delay_frames_to_drop -= samples_to_drop / self.channels;
            } else {
                output.extend_from_slice(produced_slice);
            }
        }

        Ok(())
    }

    pub fn flush(&mut self, output: &mut Vec<f32>) -> Result<(), PlaybackError> {
        let Some(ref mut resampler) = self.resampler else {
            return Ok(());
        };

        let rem_frames = self.in_buffer.len() / self.channels;
        if rem_frames > 0 {
            let padding_frames = self.chunk_frames_in.saturating_sub(rem_frames);
            if padding_frames > 0 {
                self.in_buffer.resize(self.in_buffer.len() + padding_frames * self.channels, 0.0);
            }

            let mut temp_out = vec![0.0f32; self.max_chunk_frames_out * self.channels];

            let in_adapter = InterleavedSlice::new(
                &self.in_buffer[..self.chunk_frames_in * self.channels],
                self.channels,
                self.chunk_frames_in,
            )
            .map_err(|e| PlaybackError::Output(e.to_string()))?;

            let mut out_adapter = InterleavedSlice::new_mut(
                &mut temp_out,
                self.channels,
                self.max_chunk_frames_out,
            )
            .map_err(|e| PlaybackError::Output(e.to_string()))?;

            let indexing = Indexing::new().partial_len(rem_frames);

            let (_consumed, produced) = resampler
                .process_into_buffer(&in_adapter, &mut out_adapter, Some(&indexing))
                .map_err(|e| PlaybackError::Output(e.to_string()))?;

            self.in_buffer.clear();

            let valid_produced = ((rem_frames as f64 * self.out_rate as f64 / self.in_rate as f64)
                .round() as usize)
                .min(produced);

            let produced_samples = valid_produced * self.channels;
            let produced_slice = &temp_out[..produced_samples];

            if self.delay_frames_to_drop > 0 {
                let samples_to_drop =
                    (self.delay_frames_to_drop * self.channels).min(produced_slice.len());
                output.extend_from_slice(&produced_slice[samples_to_drop..]);
                self.delay_frames_to_drop -= samples_to_drop / self.channels;
            } else {
                output.extend_from_slice(produced_slice);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_resample_rate_exact_match() {
        // If track sample rate is supported, DO NOT re-sample!
        let supported = [44100, 48000, 88200, 96000, 192000];
        assert_eq!(select_resample_rate(44100, &supported), 44100);
        assert_eq!(select_resample_rate(48000, &supported), 48000);
        assert_eq!(select_resample_rate(96000, &supported), 96000);
        assert_eq!(select_resample_rate(192000, &supported), 192000);
    }

    #[test]
    fn test_select_resample_rate_prefer_upsampling() {
        // If track sample rate is not supported, up-sampling is preferred:
        // Nearest supported sample rate above source rate.
        let supported = [48000, 96000, 192000];
        assert_eq!(select_resample_rate(44100, &supported), 48000);

        let supported = [44100, 48000, 96000, 192000];
        assert_eq!(select_resample_rate(88200, &supported), 96000);
    }

    #[test]
    fn test_select_resample_rate_only_lower_supported_uses_downsampling() {
        // If only lower bit-rate/sample rate is supported, use down-sampling:
        // Re-sample to nearest supported sample rate below source rate.
        let supported = [44100, 48000];
        assert_eq!(select_resample_rate(96000, &supported), 48000);
        assert_eq!(select_resample_rate(88200, &supported), 48000);
        assert_eq!(select_resample_rate(192000, &supported), 48000);

        let supported = [44100, 48000, 96000];
        assert_eq!(select_resample_rate(192000, &supported), 96000);
    }

    #[test]
    fn test_audio_resampler_downsample_streaming() {
        let mut resampler = AudioResampler::new(96000, 48000, 2).unwrap();
        assert!(resampler.is_resampling());

        let total_frames_in = 96000;
        let mut output = Vec::new();

        let chunk_size_samples = 1024;
        let dummy_input = vec![0.25f32; chunk_size_samples];

        let mut samples_fed = 0;
        while samples_fed < total_frames_in * 2 {
            let to_feed = (total_frames_in * 2 - samples_fed).min(chunk_size_samples);
            resampler
                .process_interleaved(&dummy_input[..to_feed], &mut output)
                .unwrap();
            samples_fed += to_feed;
        }

        resampler.flush(&mut output).unwrap();

        let output_frames = output.len() / 2;
        let expected = 48000;
        let diff = (output_frames as i64 - expected as i64).abs();
        assert!(
            diff <= 100,
            "Expected ~48000 frames from 96k->48k downsample, got {output_frames} (diff {diff})"
        );
    }

    #[test]
    fn test_audio_resampler_passthrough() {
        let mut resampler = AudioResampler::new(44100, 44100, 2).unwrap();
        assert!(!resampler.is_resampling());

        let input = vec![0.1f32, 0.2, 0.3, 0.4];
        let mut output = Vec::new();
        resampler.process_interleaved(&input, &mut output).unwrap();
        resampler.flush(&mut output).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn test_audio_resampler_upsample_streaming() {
        let mut resampler = AudioResampler::new(44100, 48000, 2).unwrap();
        assert!(resampler.is_resampling());

        // Feed 1 second of audio in small chunks of 256 frames (512 samples)
        let total_frames_in = 44100;
        let mut output = Vec::new();

        let chunk_size_samples = 512;
        let dummy_input = vec![0.5f32; chunk_size_samples];

        let mut samples_fed = 0;
        while samples_fed < total_frames_in * 2 {
            let to_feed = (total_frames_in * 2 - samples_fed).min(chunk_size_samples);
            resampler
                .process_interleaved(&dummy_input[..to_feed], &mut output)
                .unwrap();
            samples_fed += to_feed;
        }

        resampler.flush(&mut output).unwrap();

        let output_frames = output.len() / 2;
        // Output should be very close to 48000 frames
        let expected = 48000;
        let diff = (output_frames as i64 - expected as i64).abs();
        assert!(
            diff <= 100,
            "Expected ~48000 frames, got {output_frames} (diff {diff})"
        );
    }
}
