use std::mem::ManuallyDrop;
use tracing::{info, warn};
use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;

#[inline]
fn pack_2u32(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
}

use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

pub struct MediaFoundationH264Encoder {
    mft: IMFTransform,
    event_gen: IMFMediaEventGenerator,
    codec_api: Option<ICodecAPI>,
    width: u32,
    height: u32,
    frame_count: i64,
    hardware_name: String,
    has_d3d: bool,
}

impl MediaFoundationH264Encoder {
    pub fn new(
        width: usize,
        height: usize,
        bitrate_bps: u32,
        d3d_manager: Option<IMFDXGIDeviceManager>,
    ) -> Option<Self> {
        let width = width as u32;
        let height = height as u32;

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_err() {
                warn!("[Phase 3 HW MFT] MFStartup failed");
                return None;
            }

            // Discover hardware video encoders (prefer Intel QSV, NVENC, AMF)
            let mut activate_ptrs: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count: u32 = 0;
            let flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;

            let res = MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                flags,
                None,
                None,
                &mut activate_ptrs,
                &mut count,
            );

            if res.is_err() || count == 0 {
                info!("[Phase 3 HW MFT] No hardware video encoders found in MFTEnumEx");
                return None;
            }

            let activates = std::slice::from_raw_parts_mut(activate_ptrs, count as usize);
            let mut selected_act = None;
            let mut selected_name = String::new();

            for act_opt in activates.iter_mut() {
                if let Some(act) = act_opt.take() {
                    let mut name_buf = [0u16; 256];
                    let mut name_len = 0u32;
                    let _ = act.GetString(
                        &MFT_FRIENDLY_NAME_Attribute,
                        &mut name_buf,
                        Some(&mut name_len),
                    );
                    let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                    let name_lower = name.to_lowercase();
                    if selected_act.is_none() && (name_lower.contains("h.264") || name_lower.contains("h264")) {
                        selected_name = name;
                        selected_act = Some(act);
                    }
                }
            }
            CoTaskMemFree(Some(activate_ptrs as *const _));

            let act = selected_act?;
            let mft: IMFTransform = match act.ActivateObject() {
                Ok(m) => m,
                Err(e) => {
                    warn!("[Phase 3 HW MFT] Failed to activate hardware MFT {}: {:?}", selected_name, e);
                    return None;
                }
            };

            // Configure Async MFT attributes & Low-Latency mode
            if let Ok(attrs) = mft.GetAttributes() {
                let _ = attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1);
                let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            }

            // Bind D3D11 Device Manager if provided (Phase 3.2+ zero-copy pipeline)
            let mut has_d3d = false;
            if let Some(ref dm) = d3d_manager {
                let msg_res = mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, dm.as_raw() as usize);
                if msg_res.is_ok() {
                    has_d3d = true;
                    info!("[Phase 3.2+ HW MFT] D3D11 Device Manager attached to hardware MFT '{}'", selected_name);
                } else {
                    warn!("[Phase 3.2+ HW MFT] MFT_MESSAGE_SET_D3D_MANAGER failed: {:?}", msg_res);
                }
            }

            // Configure ICodecAPI for ultra-low latency & zero frame lag
            let codec_api: Option<ICodecAPI> = mft.cast().ok();
            if let Some(ref api) = codec_api {
                let mut true_var = windows::core::VARIANT::from(-1i16); // VARIANT_TRUE
                let _ = api.SetValue(&CODECAPI_AVLowLatencyMode, &mut true_var);

                let mut b_frames = windows::core::VARIANT::from(0u32); // 0 B-frames
                let _ = api.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &mut b_frames);

                let mut gop = windows::core::VARIANT::from(60u32); // 1 IDR/s at 60 FPS
                let _ = api.SetValue(&CODECAPI_AVEncMPVGOPSize, &mut gop);

                let mut quality = windows::core::VARIANT::from(75u32); // High visual quality
                let _ = api.SetValue(&CODECAPI_AVEncCommonQuality, &mut quality);

                let mut rate_control = windows::core::VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32);
                let _ = api.SetValue(&CODECAPI_AVEncCommonRateControlMode, &mut rate_control);

                let mut mean_bitrate = windows::core::VARIANT::from(bitrate_bps);
                let _ = api.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &mut mean_bitrate);

                let mut max_bitrate = windows::core::VARIANT::from(bitrate_bps * 3 / 2);
                let _ = api.SetValue(&CODECAPI_AVEncCommonMaxBitRate, &mut max_bitrate);

                let mut real_time = windows::core::VARIANT::from(-1i16);
                let _ = api.SetValue(&CODECAPI_AVEncCommonRealTime, &mut real_time);
            }

            // 1. Configure Output Media Type (H.264 Baseline Profile)
            {
                let out_type = match MFCreateMediaType() {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("[Phase 3 HW MFT] MFCreateMediaType out error: {:?}", e);
                        return None;
                    }
                };
                out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
                out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).ok()?;
                out_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_bps).ok()?;
                out_type.SetUINT64(&MF_MT_FRAME_RATE, pack_2u32(60, 1)).ok()?;
                out_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_2u32(width, height)).ok()?;
                out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).ok()?;
                let _ = out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32);

                if let Err(e) = mft.SetOutputType(0, &out_type, 0) {
                    warn!("[Phase 3 HW MFT] SetOutputType H.264 failed: {:?}", e);
                    return None;
                }
            }

            // 2. Configure Input Media Type
            // When D3D11 manager is attached, prefer ARGB32 (desktop duplication format) to eliminate CPU conversion
            let mut in_configured = false;
            if has_d3d {
                if let Ok(in_argb) = MFCreateMediaType() {
                    let _ = in_argb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
                    let _ = in_argb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32);
                    let _ = in_argb.SetUINT64(&MF_MT_FRAME_RATE, pack_2u32(60, 1));
                    let _ = in_argb.SetUINT64(&MF_MT_FRAME_SIZE, pack_2u32(width, height));
                    let _ = in_argb.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32);
                    if mft.SetInputType(0, &in_argb, 0).is_ok() {
                        in_configured = true;
                        info!("[Phase 3.2+ HW MFT] Using Direct D3D11 ARGB32 Surface Input (Zero-Copy VRAM pipeline)");
                    }
                }
            }

            if !in_configured {
                let in_type = match MFCreateMediaType() {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("[Phase 3 HW MFT] MFCreateMediaType in error: {:?}", e);
                        return None;
                    }
                };
                in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
                in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).ok()?;
                in_type.SetUINT64(&MF_MT_FRAME_RATE, pack_2u32(60, 1)).ok()?;
                in_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_2u32(width, height)).ok()?;
                in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).ok()?;

                if let Err(e) = mft.SetInputType(0, &in_type, 0) {
                    warn!("[Phase 3 HW MFT] SetInputType NV12 failed: {:?}", e);
                    return None;
                }
            }

            let event_gen: IMFMediaEventGenerator = match mft.cast() {
                Ok(eg) => eg,
                Err(e) => {
                    warn!("[Phase 3 HW MFT] Failed to cast IMFMediaEventGenerator: {:?}", e);
                    return None;
                }
            };

            // Start streaming notify
            let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);

            info!(
                "[Phase 3 HW MFT] Hardware Video Encoder ACTIVE: '{}' for {}x{} @ {:.1} Mbps (60 FPS, D3D11: {})",
                selected_name,
                width,
                height,
                bitrate_bps as f64 / 1_000_000.0,
                has_d3d
            );

            Some(Self {
                mft,
                event_gen,
                codec_api,
                width,
                height,
                frame_count: 0,
                hardware_name: selected_name,
                has_d3d,
            })
        }
    }

    pub fn hardware_name(&self) -> &str {
        &self.hardware_name
    }

    pub fn is_d3d_aware(&self) -> bool {
        self.has_d3d
    }

    pub fn force_idr(&mut self) {
        if let Some(ref api) = self.codec_api {
            unsafe {
                let mut var = windows::core::VARIANT::from(1u32);
                let _ = api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &mut var);
            }
        }
    }

    /// Direct D3D11 Surface Encoding (Phase 3.2+ zero-copy GPU VRAM pipeline)
    pub fn encode_surface(&mut self, texture: &ID3D11Texture2D) -> Option<Vec<u8>> {
        unsafe {
            let surf_buffer = match MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false) {
                Ok(b) => b,
                Err(e) => {
                    warn!("[Phase 3.2+ HW MFT] MFCreateDXGISurfaceBuffer failed: {:?}", e);
                    return None;
                }
            };

            let in_sample = match MFCreateSample() {
                Ok(s) => s,
                Err(_) => return None,
            };

            in_sample.AddBuffer(&surf_buffer).ok()?;

            let sample_time = self.frame_count * 166666;
            self.frame_count += 1;
            let _ = in_sample.SetSampleTime(sample_time);
            let _ = in_sample.SetSampleDuration(166666);

            self.process_sample(&in_sample)
        }
    }

    /// Fallback CPU Memory Buffer Encoding (NV12)
    pub fn encode(&mut self, nv12: &[u8]) -> Option<Vec<u8>> {
        let expected_size = (self.width * self.height * 3 / 2) as usize;
        if nv12.len() < expected_size {
            return None;
        }

        unsafe {
            let in_buffer = MFCreateMemoryBuffer(expected_size as u32).ok()?;
            let in_sample = match MFCreateSample() {
                Ok(s) => s,
                Err(_) => return None,
            };

            in_sample.AddBuffer(&in_buffer).ok()?;

            let mut ptr: *mut u8 = std::ptr::null_mut();
            in_buffer.Lock(&mut ptr, None, None).ok()?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), ptr, expected_size);
            let _ = in_buffer.Unlock();
            let _ = in_buffer.SetCurrentLength(expected_size as u32);

            let sample_time = self.frame_count * 166666;
            self.frame_count += 1;
            let _ = in_sample.SetSampleTime(sample_time);
            let _ = in_sample.SetSampleDuration(166666);

            self.process_sample(&in_sample)
        }
    }

    /// Shared non-blocking event-driven MFT output loop
    unsafe fn process_sample(&mut self, in_sample: &IMFSample) -> Option<Vec<u8>> {
        let mut input_sent = false;

        // Pump event loop with non-blocking poll (MF_EVENT_FLAG_NO_WAIT)
        // Timeout after 25 iterations (25ms max) with sleep(1ms) to eliminate freeze risk
        for _ in 0..25 {
            let event = match self.event_gen.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                Ok(ev) => ev,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
            };

            let ev_type = event.GetType().unwrap_or(0);

            if ev_type == METransformNeedInput.0 as u32 && !input_sent {
                let in_res = self.mft.ProcessInput(0, in_sample, 0);
                if in_res.is_ok() {
                    input_sent = true;
                }
            } else if ev_type == METransformHaveOutput.0 as u32 {
                let mut out_data = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(None),
                    dwStatus: 0,
                    pEvents: ManuallyDrop::new(None),
                };

                let mut status = 0u32;
                let out_res = self.mft.ProcessOutput(0, std::slice::from_mut(&mut out_data), &mut status);

                if out_res.as_ref().err().map(|e| e.code().0 as u32) == Some(0xC00D6D61) {
                    // Stream format change (initial SPS/PPS parameter generation)
                    if let Ok(new_type) = self.mft.GetOutputAvailableType(0, 0) {
                        let _ = self.mft.SetOutputType(0, &new_type, 0);
                    }
                } else if out_res.is_ok() {
                    let mut result = None;
                    if let Some(ref sample) = *out_data.pSample {
                        if let Ok(contig) = sample.ConvertToContiguousBuffer() {
                            let mut out_ptr: *mut u8 = std::ptr::null_mut();
                            let mut out_len = 0u32;
                            if contig.Lock(&mut out_ptr, None, Some(&mut out_len)).is_ok() {
                                if out_len > 0 {
                                    let bitstream = std::slice::from_raw_parts(out_ptr, out_len as usize);
                                    result = Some(bitstream.to_vec());
                                }
                                let _ = contig.Unlock();
                            }
                        }
                    }

                    ManuallyDrop::drop(&mut out_data.pSample);
                    ManuallyDrop::drop(&mut out_data.pEvents);

                    if result.is_some() {
                        return result;
                    }
                } else {
                    ManuallyDrop::drop(&mut out_data.pSample);
                    ManuallyDrop::drop(&mut out_data.pEvents);
                }
            }
        }

        None
    }
}


impl Drop for MediaFoundationH264Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
        }
    }
}

pub struct MediaFoundationEncoder {
    active_hardware_name: Option<String>,
}

impl MediaFoundationEncoder {
    pub fn new() -> Self {
        let mut active_hardware_name = None;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_ok() {
                let mut activate_ptrs: *mut Option<IMFActivate> = std::ptr::null_mut();
                let mut count: u32 = 0;
                let flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;

                if MFTEnumEx(
                    MFT_CATEGORY_VIDEO_ENCODER,
                    flags,
                    None,
                    None,
                    &mut activate_ptrs,
                    &mut count,
                ).is_ok() && count > 0 {
                    let activates = std::slice::from_raw_parts_mut(activate_ptrs, count as usize);
                    for act_opt in activates.iter_mut() {
                        if let Some(act) = act_opt.take() {
                            let mut name_buf = [0u16; 256];
                            let mut name_len = 0u32;
                            let _ = act.GetString(
                                &MFT_FRIENDLY_NAME_Attribute,
                                &mut name_buf,
                                Some(&mut name_len),
                            );
                            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                            if active_hardware_name.is_none() && (name.to_lowercase().contains("h.264") || name.to_lowercase().contains("h264")) {
                                info!("[Phase 3.1 HW MFT] Hardware Video Encoder available: '{}'", name);
                                active_hardware_name = Some(name);
                            }
                        }
                    }
                    CoTaskMemFree(Some(activate_ptrs as *const _));
                }
            }
        }

        Self {
            active_hardware_name,
        }
    }

    pub fn hardware_name(&self) -> Option<&str> {
        self.active_hardware_name.as_deref()
    }
}

impl Drop for MediaFoundationEncoder {
    fn drop(&mut self) {
    }
}
