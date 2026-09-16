use audiopus::{coder::Encoder, Application, Bitrate, Channels, SampleRate};
use bytes::{BufMut, Bytes, BytesMut};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

pub struct AudioEngine {
    audio_sender: broadcast::Sender<Bytes>,
    running: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn new(audio_sender: broadcast::Sender<Bytes>) -> Self {
        Self {
            audio_sender,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let audio_sender = self.audio_sender.clone();
        let running = self.running.clone();

        std::thread::Builder::new()
            .name("aerostream-audio-worker".into())
            .spawn(move || {
                Self::run_audio_worker(audio_sender, running);
            })
            .expect("Failed to spawn audio worker thread");
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn run_audio_worker(
        audio_sender: broadcast::Sender<Bytes>,
        running: Arc<AtomicBool>,
    ) {
        info!("AeroStream Audio Engine started (WASAPI Loopback + Opus 48kHz Stereo)");

        while running.load(Ordering::Relaxed) {
            let host = cpal::default_host();
            let device = match host.default_output_device() {
                Some(d) => d,
                None => {
                    warn!("No default audio output device found for loopback. Retrying in 2s...");
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let device_name = device.name().unwrap_or_else(|_| "Unknown Device".into());
            let default_config = match device.default_output_config() {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to query default output config for '{}': {:?}. Retrying in 2s...", device_name, e);
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            info!(
                "Initializing WASAPI Loopback on '{}': {} Hz, {} channels, format {:?}",
                device_name,
                default_config.sample_rate().0,
                default_config.channels(),
                default_config.sample_format()
            );

            let in_sample_rate = default_config.sample_rate().0 as f64;
            let in_channels = default_config.channels() as usize;

            // Channel to transfer audio buffers from real-time callback to encoder
            let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(32);

            let err_fn = |err| {
                warn!("WASAPI loopback stream error: {:?}", err);
            };

            let stream_config: cpal::StreamConfig = default_config.into();
            let stream = match device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = tx.try_send(data.to_vec());
                },
                err_fn,
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to build WASAPI loopback stream on '{}': {:?}. Retrying in 2s...", device_name, e);
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            if let Err(e) = stream.play() {
                warn!("Failed to start playing WASAPI loopback stream: {:?}. Retrying in 2s...", e);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }

            info!("WASAPI Loopback stream is now ACTIVE on '{}'", device_name);

            // Initialize Opus Encoder: 48kHz, Stereo, 128 kbps
            let opus_encoder = match Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio) {
                Ok(mut enc) => {
                    let _ = enc.set_bitrate(Bitrate::BitsPerSecond(128_000));
                    enc
                }
                Err(e) => {
                    error!("Failed to initialize Opus encoder: {:?}", e);
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let target_sample_rate = 48000.0f64;
            let resample_ratio = in_sample_rate / target_sample_rate; // e.g. 44100 / 48000 = 0.91875

            // Accumulator for 48kHz stereo samples
            // 20ms frame = 960 samples per channel = 1920 interleaved f32 samples
            let frame_size = 960;
            let target_frame_samples = frame_size * 2; // 1920
            let mut stereo_48k_acc: Vec<f32> = Vec::with_capacity(target_frame_samples * 2);
            let mut opus_out = vec![0u8; 1024];

            let mut last_packet_time = Instant::now();
            let silence_pcm = vec![0.0f32; target_frame_samples];

            // Resampling state
            let mut in_pos = 0.0f64;
            let mut raw_stereo_buffer: Vec<(f32, f32)> = Vec::with_capacity(4096);

            while running.load(Ordering::Relaxed) {
                // Poll for audio data with 20ms timeout
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(raw_chunk) => {
                        // Downmix to Stereo (Left, Right)
                        let frames_count = raw_chunk.len() / in_channels;
                        for i in 0..frames_count {
                            let idx = i * in_channels;
                            let (l, r) = if in_channels == 1 {
                                let s = raw_chunk[idx];
                                (s, s)
                            } else if in_channels == 2 {
                                (raw_chunk[idx], raw_chunk[idx + 1])
                            } else if in_channels >= 6 {
                                // 5.1 downmix: L = FL + 0.707*C + 0.707*SL, R = FR + 0.707*C + 0.707*SR
                                let fl = raw_chunk[idx];
                                let fr = raw_chunk[idx + 1];
                                let c = raw_chunk[idx + 2];
                                let sl = raw_chunk[idx + 4];
                                let sr = raw_chunk[idx + 5];
                                (fl + 0.707 * c + 0.707 * sl, fr + 0.707 * c + 0.707 * sr)
                            } else {
                                (raw_chunk[idx], raw_chunk[idx + 1])
                            };
                            raw_stereo_buffer.push((l, r));
                        }

                        // Resample from in_sample_rate to 48000 Hz
                        if (in_sample_rate - target_sample_rate).abs() < 1.0 {
                            // Direct 48kHz
                            for (l, r) in raw_stereo_buffer.drain(..) {
                                stereo_48k_acc.push(l);
                                stereo_48k_acc.push(r);
                            }
                            in_pos = 0.0;
                        } else {
                            // Linear interpolation resampler
                            while (in_pos as usize) + 1 < raw_stereo_buffer.len() {
                                let idx = in_pos as usize;
                                let frac = (in_pos - idx as f64) as f32;
                                let (l1, r1) = raw_stereo_buffer[idx];
                                let (l2, r2) = raw_stereo_buffer[idx + 1];

                                let l = l1 + frac * (l2 - l1);
                                let r = r1 + frac * (r2 - r1);

                                stereo_48k_acc.push(l);
                                stereo_48k_acc.push(r);

                                in_pos += resample_ratio;
                            }

                            // Discard processed raw samples
                            let processed = in_pos as usize;
                            if processed > 0 && processed < raw_stereo_buffer.len() {
                                raw_stereo_buffer.drain(..processed);
                                in_pos -= processed as f64;
                            } else if processed >= raw_stereo_buffer.len() {
                                raw_stereo_buffer.clear();
                                in_pos = 0.0;
                            }
                        }

                        // Encode complete 20ms frames (1920 f32 samples)
                        while stereo_48k_acc.len() >= target_frame_samples {
                            let frame: Vec<f32> = stereo_48k_acc.drain(..target_frame_samples).collect();
                            if audio_sender.receiver_count() > 0 {
                                if let Ok(encoded_len) = opus_encoder.encode_float(&frame, &mut opus_out) {
                                    let now_ms = crate::clock::qpc_now_ms();

                                    let mut packet = BytesMut::with_capacity(10 + encoded_len);
                                    packet.put_u8(0xFA);
                                    packet.put_u8(0xFA);
                                    packet.put_u64(now_ms);
                                    packet.extend_from_slice(&opus_out[..encoded_len]);

                                    let _ = audio_sender.send(packet.freeze());
                                    last_packet_time = Instant::now();
                                }
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // When host is silent, WASAPI loopback callback doesn't fire.
                        // Emit comfort silence packet every 20ms if subscribers exist to prevent client jitter buffer starvation.
                        if audio_sender.receiver_count() > 0 && last_packet_time.elapsed() >= Duration::from_millis(20) {
                            if let Ok(encoded_len) = opus_encoder.encode_float(&silence_pcm, &mut opus_out) {
                                let now_ms = crate::clock::qpc_now_ms();

                                let mut packet = BytesMut::with_capacity(10 + encoded_len);
                                packet.put_u8(0xFA);
                                packet.put_u8(0xFA);
                                packet.put_u64(now_ms);
                                packet.extend_from_slice(&opus_out[..encoded_len]);

                                let _ = audio_sender.send(packet.freeze());
                                last_packet_time = Instant::now();
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        warn!("WASAPI loopback channel disconnected (device change or unplugged). Reconnecting...");
                        break;
                    }
                }
            }

            drop(stream);
            info!("WASAPI loopback stream closed on '{}'. Preparing re-init...", device_name);
            std::thread::sleep(Duration::from_millis(500));
        }

        info!("AeroStream Audio Engine stopped");
    }
}
