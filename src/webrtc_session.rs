use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info};

use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::media::Sample;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;

use crate::input::InputManager;

#[allow(dead_code)]
pub struct WebRtcSession {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub video_track: Arc<TrackLocalStaticSample>,
    pub audio_track: Arc<TrackLocalStaticSample>,
}

impl WebRtcSession {
    pub async fn new(
        ws_sender: mpsc::Sender<String>,
        h264_receiver: broadcast::Receiver<bytes::Bytes>,
        audio_receiver: broadcast::Receiver<bytes::Bytes>,
        request_h264_keyframe: Arc<AtomicBool>,
        input_manager: Arc<InputManager>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // 1. Configure MediaEngine with H.264 & Opus codecs
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;

        // 2. Configure Interceptor Registry (NACK, PLI, RTCP reports)
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;

        // 3. Build WebRTC API instance
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        // 4. Configure ICE servers (Google Public STUN)
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // 5. Create PeerConnection
        let peer_connection = Arc::new(api.new_peer_connection(config).await?);

        // 6. Create Video Track (H.264)
        let video_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_string(),
                ..Default::default()
            },
            "video".to_string(),
            "aerostream".to_string(),
        ));

        let video_transceiver = peer_connection
            .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        // 7. Handle RTCP PLI/FIR feedback on video track to trigger IDR keyframes
        let rtcp_keyframe_flag = Arc::clone(&request_h264_keyframe);
        tokio::spawn(async move {
            while let Ok((rtcp_packets, _)) = video_transceiver.read_rtcp().await {
                for pkt in rtcp_packets {
                    // Check if packet is PLI (Picture Loss Indication) or FIR (Full Intra Request)
                    if pkt.as_any().downcast_ref::<webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication>().is_some()
                        || pkt.as_any().downcast_ref::<webrtc::rtcp::payload_feedbacks::full_intra_request::FullIntraRequest>().is_some()
                    {
                        info!("[WebRTC] Received PLI/FIR packet from client -> forcing IDR keyframe");
                        rtcp_keyframe_flag.store(true, Ordering::SeqCst);
                    }
                }
            }
        });

        // 8. Create Audio Track (Opus 48kHz stereo)
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_string(),
                ..Default::default()
            },
            "audio".to_string(),
            "aerostream".to_string(),
        ));

        let _audio_transceiver = peer_connection
            .add_track(Arc::clone(&audio_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        // 9. Setup local ICE Candidate forwarding to WebSocket
        let ws_sender_ice = ws_sender.clone();
        peer_connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let ws_sender = ws_sender_ice.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(json_cand) = candidate.to_json() {
                        let msg = serde_json::json!({
                            "type": "signal",
                            "action": "candidate",
                            "candidate": json_cand
                        });
                        let _ = ws_sender.send(msg.to_string()).await;
                    }
                }
            })
        }));

        // 10. PeerConnection state change monitoring
        peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            info!("[WebRTC] PeerConnection state changed: {:?}", state);
            Box::pin(async {})
        }));

        // 11. Handle DataChannel for zero HOL-blocking input & control
        let input_mgr_dc = input_manager.clone();
        peer_connection.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
            let label = data_channel.label().to_string();
            info!("[WebRTC] Client opened DataChannel: '{}'", label);
            let input_mgr = input_mgr_dc.clone();

            Box::pin(async move {
                let dc_clone = Arc::clone(&data_channel);
                data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
                    let text = String::from_utf8_lossy(&msg.data).to_string();
                    let dc = Arc::clone(&dc_clone);
                    let input_mgr = input_mgr.clone();

                    Box::pin(async move {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                            let msg_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match msg_type {
                                "ping" => {
                                    let client_time = json.get("client_time")
                                        .or_else(|| json.get("t"))
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);
                                    let pong = serde_json::json!({
                                        "type": "pong",
                                        "client_time": client_time,
                                        "server_time": crate::clock::qpc_now_ms(),
                                    });
                                    let _ = dc.send_text(pong.to_string()).await;
                                }
                                "echo_test" => {
                                    let echo = serde_json::json!({
                                        "type": "echo_response",
                                        "payload": json.get("payload"),
                                        "server_time": crate::clock::qpc_now_ms(),
                                    });
                                    let _ = dc.send_text(echo.to_string()).await;
                                }
                                "mouse_move" | "mouse_delta" | "mouse_down" | "mouse_up" | "mouse_click" | "mouse_double" | "mouse_wheel" | "key_down" | "key_up" | "key_click" | "shortcut" | "text" => {
                                    // Inject input directly with zero TCP blocking
                                    if let Ok(input_msg) = serde_json::from_str::<crate::input::InputMessage>(&text) {
                                        input_mgr.handle_input(input_msg);
                                    }
                                }
                                _ => {
                                    // Forward other control messages if needed
                                    debug!("[WebRTC DataChannel] Handled message: {}", text);
                                }
                            }
                        }
                    })
                }));
            })
        }));

        // 12. Spawn Video Streaming Worker (h264_sender -> TrackLocalStaticSample)
        let v_track = Arc::clone(&video_track);
        let mut h264_rx = h264_receiver;
        tokio::spawn(async move {
            info!("[WebRTC] Started Video Track pumping worker");
            loop {
                let frame = match h264_rx.recv().await {
                    Ok(f) => f,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };

                // Video packet format: [ts (8B), w (2B), h (2B), Annex-B NAL...]
                if frame.len() > 12 {
                    let annex_b_data = bytes::Bytes::copy_from_slice(&frame[12..]);
                    let sample = Sample {
                        data: annex_b_data,
                        duration: Duration::from_micros(16666), // ~60fps
                        ..Default::default()
                    };
                    if let Err(e) = v_track.write_sample(&sample).await {
                        // If track closed or subscriber departed, exit
                        debug!("[WebRTC] video write_sample error: {:?}", e);
                        break;
                    }
                }
            }
            info!("[WebRTC] Video Track pumping worker stopped");
        });

        // 13. Spawn Audio Streaming Worker (audio_sender -> TrackLocalStaticSample)
        let a_track = Arc::clone(&audio_track);
        let mut audio_rx = audio_receiver;
        tokio::spawn(async move {
            info!("[WebRTC] Started Audio Track pumping worker");
            loop {
                let packet = match audio_rx.recv().await {
                    Ok(p) => p,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };

                // Audio packet format: [0xFA, 0xFA, ts (8B), Opus payload...]
                if packet.len() > 10 && packet[0] == 0xFA && packet[1] == 0xFA {
                    let opus_data = bytes::Bytes::copy_from_slice(&packet[10..]);
                    let sample = Sample {
                        data: opus_data,
                        duration: Duration::from_millis(20), // 20ms Opus frames
                        ..Default::default()
                    };
                    if let Err(e) = a_track.write_sample(&sample).await {
                        debug!("[WebRTC] audio write_sample error: {:?}", e);
                        break;
                    }
                }
            }
            info!("[WebRTC] Audio Track pumping worker stopped");
        });

        Ok(Self {
            peer_connection,
            video_track,
            audio_track,
        })
    }

    pub async fn handle_offer(&self, sdp_offer: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let offer = RTCSessionDescription::offer(sdp_offer.to_string())?;
        self.peer_connection.set_remote_description(offer).await?;

        let answer = self.peer_connection.create_answer(None).await?;
        self.peer_connection.set_local_description(answer.clone()).await?;

        Ok(answer.sdp)
    }

    pub async fn add_ice_candidate(&self, candidate_init: RTCIceCandidateInit) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.peer_connection.add_ice_candidate(candidate_init).await?;
        Ok(())
    }
}
