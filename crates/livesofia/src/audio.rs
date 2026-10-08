//! Bounded microphone capture and model PCM playback on desktop devices.

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait};
use rodio::Source;
use rodio::buffer::SamplesBuffer;
use rodio::microphone::MicrophoneBuilder;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use sofia_protocol::AudioOutputDevice;

const CAPTURE_CHUNK_MS: u32 = 40;
const MODEL_OUTPUT_RATE: u32 = 24_000;

pub struct AudioChunk {
    pub capture_id: Uuid,
    pub pcm: Vec<u8>,
    pub sample_rate: u32,
    pub rms: f32,
}

pub struct MicCapture {
    pub id: Uuid,
    stop: Arc<AtomicBool>,
    worker: Option<tokio::task::JoinHandle<()>>,
}

impl MicCapture {
    pub async fn start(sender: mpsc::Sender<AudioChunk>) -> Result<Self, String> {
        let (ready, started) = oneshot::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let id = Uuid::new_v4();
        let worker = tokio::task::spawn_blocking(move || {
            let result = open_and_capture(sender, worker_stop, ready, id);
            if let Err(error) = result {
                tracing::warn!(%error, "microphone capture stopped");
            }
        });
        started
            .await
            .map_err(|_| "microphone worker stopped before startup".to_string())??;
        Ok(Self {
            id,
            stop,
            worker: Some(worker),
        })
    }

    pub async fn finish(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
        }
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn open_and_capture(
    sender: mpsc::Sender<AudioChunk>,
    stop: Arc<AtomicBool>,
    ready: oneshot::Sender<Result<(), String>>,
    capture_id: Uuid,
) -> Result<(), String> {
    let mut microphone = match MicrophoneBuilder::new()
        .default_device()
        .and_then(|builder| builder.default_config())
        .map_err(|error| error.to_string())
        .and_then(|builder| builder.open_stream().map_err(|error| error.to_string()))
    {
        Ok(microphone) => microphone,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let sample_rate = microphone.sample_rate().get();
    let channels = usize::from(microphone.channels().get());
    let frames_per_chunk = (sample_rate * CAPTURE_CHUNK_MS / 1000) as usize;
    let _ = ready.send(Ok(()));

    while !stop.load(Ordering::Acquire) && !sender.is_closed() {
        let mut samples = Vec::with_capacity(frames_per_chunk);
        let mut energy = 0.0_f32;
        for _ in 0..frames_per_chunk {
            let mut sum = 0.0_f32;
            for _ in 0..channels {
                let Some(value) = microphone.next() else {
                    return Err("microphone stream ended".into());
                };
                sum += value;
            }
            let sample = (sum / channels as f32).clamp(-1.0, 1.0);
            energy += sample * sample;
            samples.push(sample);
        }
        let rms = (energy / samples.len() as f32).sqrt();
        let pcm = encode_pcm(&samples);
        let _ = sender.try_send(AudioChunk {
            capture_id,
            pcm,
            sample_rate,
            rms,
        });
    }
    Ok(())
}

fn encode_pcm(samples: &[f32]) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }
    pcm
}

pub struct AudioPlayback {
    _device: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl AudioPlayback {
    pub fn open(device_id: Option<&str>) -> Result<Self, String> {
        let mut device = if let Some(device_id) = device_id {
            let requested = device_id
                .parse::<cpal::DeviceId>()
                .map_err(|error| error.to_string())?;
            let device = cpal::default_host()
                .output_devices()
                .map_err(|error| error.to_string())?
                .find(|device| device.id().ok().as_ref() == Some(&requested))
                .ok_or_else(|| format!("output device is unavailable: {device_id}"))?;
            rodio::DeviceSinkBuilder::from_device(device)
                .and_then(|builder| builder.open_stream())
                .map_err(|error| error.to_string())?
        } else {
            rodio::DeviceSinkBuilder::open_default_sink().map_err(|error| error.to_string())?
        };
        device.log_on_drop(false);
        let player = rodio::Player::connect_new(device.mixer());
        Ok(Self {
            _device: device,
            player,
        })
    }

    pub fn push_pcm(&self, pcm: &[u8]) -> Result<(), String> {
        if pcm.len() % 2 != 0 {
            return Err("model audio payload has an odd byte count".into());
        }
        if pcm.is_empty() {
            return Ok(());
        }
        let samples = pcm
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0)
            .collect::<Vec<_>>();
        if self.player.len() > 32 {
            self.player.clear();
            return Err("speaker queue exceeded 32 chunks and was cleared".into());
        }
        self.player.append(SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(MODEL_OUTPUT_RATE).unwrap(),
            samples,
        ));
        Ok(())
    }

    pub fn clear(&self) {
        self.player.clear();
    }

    pub fn is_idle(&self) -> bool {
        self.player.empty()
    }
}

pub fn list_output_devices() -> Result<Vec<AudioOutputDevice>, String> {
    let host = cpal::default_host();
    let default_id = host
        .default_output_device()
        .and_then(|device| device.id().ok());
    let mut devices = host
        .output_devices()
        .map_err(|error| error.to_string())?
        .filter_map(|device| {
            let id = device.id().ok()?;
            let description = device.description().ok()?;
            if description.driver() == Some("null") || id.1 == "null" {
                return None;
            }
            let name = description.name().to_string();
            Some(AudioOutputDevice {
                is_default: default_id.as_ref() == Some(&id),
                id: id.to_string(),
                name,
            })
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_samples_encode_as_little_endian_pcm() {
        assert_eq!(encode_pcm(&[0.0, 1.0, -1.0]), vec![0, 0, 255, 127, 1, 128]);
    }
}
