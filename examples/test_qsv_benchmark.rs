use std::mem::ManuallyDrop;
use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;

#[inline]
fn pack_2u32(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);

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
        if res.is_ok() && count > 0 {
            let activates = std::slice::from_raw_parts(activate_ptrs, count as usize);
            for act_opt in activates.iter() {
                if let Some(act) = act_opt {
                    let mut name_buf = [0u16; 256];
                    let mut name_len = 0u32;
                    let _ = act.GetString(
                        &MFT_FRIENDLY_NAME_Attribute,
                        &mut name_buf,
                        Some(&mut name_len),
                    );
                    let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);

                    if name.contains("Quick Sync") && name.contains("H.264") {
                        let mft: IMFTransform = act.ActivateObject().expect("ActivateObject failed");

                        if let Ok(attrs) = mft.GetAttributes() {
                            let _ = attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1);
                            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
                        }

                        let width = 1280u32;
                        let height = 720u32;

                        let out_type = MFCreateMediaType().expect("MFCreateMediaType out");
                        out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
                        out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).unwrap();
                        out_type.SetUINT32(&MF_MT_AVG_BITRATE, 4_000_000).unwrap();
                        out_type.SetUINT64(&MF_MT_FRAME_RATE, pack_2u32(60, 1)).unwrap();
                        out_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_2u32(width, height)).unwrap();
                        out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32).unwrap();
                        let set_out = mft.SetOutputType(0, &out_type, 0);
                        println!("SetOutputType H.264 Base profile: {:?}", set_out);

                        let in_type = MFCreateMediaType().expect("MFCreateMediaType in");
                        in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
                        in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).unwrap();
                        in_type.SetUINT64(&MF_MT_FRAME_RATE, pack_2u32(60, 1)).unwrap();
                        in_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_2u32(width, height)).unwrap();
                        in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).unwrap();
                        mft.SetInputType(0, &in_type, 0).expect("SetInputType failed");

                        let event_gen: IMFMediaEventGenerator = mft.cast().expect("Cast to IMFMediaEventGenerator");

                        let _ = mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
                        let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
                        let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);

                        let nv12_len = (width * height * 3 / 2) as usize;
                        let mut frames = 0;

                        while frames < 5 {
                            match event_gen.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                                Err(_) => {
                                    std::thread::sleep(std::time::Duration::from_millis(1));
                                    continue;
                                }
                                Ok(event) => {
                                    let ev_type = event.GetType().unwrap_or(0);
                                    if ev_type == METransformNeedInput.0 as u32 {
                                        let in_buffer = MFCreateMemoryBuffer(nv12_len as u32).unwrap();
                                        let in_sample = MFCreateSample().unwrap();
                                        in_sample.AddBuffer(&in_buffer).unwrap();

                                        let mut ptr: *mut u8 = std::ptr::null_mut();
                                        in_buffer.Lock(&mut ptr, None, None).unwrap();
                                        std::ptr::write_bytes(ptr, 128, (width * height) as usize);
                                        std::ptr::write_bytes(ptr.add((width * height) as usize), 128, (width * height / 2) as usize);
                                        in_buffer.Unlock().unwrap();
                                        in_buffer.SetCurrentLength(nv12_len as u32).unwrap();

                                        in_sample.SetSampleTime(frames as i64 * 166666).unwrap();
                                        in_sample.SetSampleDuration(166666).unwrap();

                                        if frames == 3 {
                                            println!("--- Forcing IDR keyframe at frame 3 via CODECAPI_AVEncVideoForceKeyFrame ---");
                                            let codec_api: windows::core::Result<ICodecAPI> = mft.cast();
                                            if let Ok(api) = codec_api {
                                                // Test Rate Control Mode: Quality / Low-Latency
                                                let mut rc_mode = windows::core::VARIANT::from(eAVEncCommonRateControlMode_Quality.0 as u32);
                                                let rc_res = api.SetValue(&CODECAPI_AVEncCommonRateControlMode, &mut rc_mode);
                                                println!("Set CODECAPI_AVEncCommonRateControlMode (Quality): {:?}", rc_res);

                                                let mut mean_br = windows::core::VARIANT::from(4_000_000u32);
                                                let br_res = api.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &mut mean_br);
                                                println!("Set CODECAPI_AVEncCommonMeanBitRate: {:?}", br_res);

                                                let mut vbv = windows::core::VARIANT::from(100_000u32); // small VBV buffer
                                                let vbv_res = api.SetValue(&CODECAPI_AVEncCommonBufferSize, &mut vbv);
                                                println!("Set CODECAPI_AVEncCommonBufferSize: {:?}", vbv_res);

                                                let mut var = windows::core::VARIANT::from(1u32);
                                                let idr_res = api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &mut var);
                                                println!("Set CODECAPI_AVEncVideoForceKeyFrame: {:?}", idr_res);
                                            }
                                        }

                                        let _ = mft.ProcessInput(0, &in_sample, 0);
                                        frames += 1;
                                    } else if ev_type == METransformHaveOutput.0 as u32 {
                                        let mut out_data = MFT_OUTPUT_DATA_BUFFER {
                                            dwStreamID: 0,
                                            pSample: ManuallyDrop::new(None),
                                            dwStatus: 0,
                                            pEvents: ManuallyDrop::new(None),
                                        };
                                        let mut status = 0u32;
                                        let out_res = mft.ProcessOutput(0, std::slice::from_mut(&mut out_data), &mut status);

                                        if out_res.as_ref().err().map(|e| e.code().0 as u32) == Some(0xC00D6D61) {
                                            if let Ok(new_type) = mft.GetOutputAvailableType(0, 0) {
                                                let _ = mft.SetOutputType(0, &new_type, 0);
                                            }
                                        } else if out_res.is_ok() {
                                            if let Some(ref sample) = *out_data.pSample {
                                                if let Ok(contig) = sample.ConvertToContiguousBuffer() {
                                                    let mut out_ptr: *mut u8 = std::ptr::null_mut();
                                                    let mut out_len = 0u32;
                                                    let _ = contig.Lock(&mut out_ptr, None, Some(&mut out_len));
                                                    let bitstream = std::slice::from_raw_parts(out_ptr, out_len as usize);
                                                    
                                                    let mut i = 0;
                                                    while i + 3 < bitstream.len() {
                                                        if bitstream[i] == 0 && bitstream[i+1] == 0 {
                                                            let nal_start = if bitstream[i+2] == 1 { i + 3 } else if i + 4 < bitstream.len() && bitstream[i+2] == 0 && bitstream[i+3] == 1 { i + 4 } else { i += 1; continue; };
                                                            let nal_type = bitstream[nal_start] & 0x1F;
                                                            let preview_len = (bitstream.len() - nal_start).min(12);
                                                            println!("   NAL type {}: {:02X?}", nal_type, &bitstream[nal_start..nal_start + preview_len]);
                                                            i = nal_start;
                                                            continue;
                                                        }
                                                        i += 1;
                                                    }
                                                    println!("Frame output (size: {} B)", bitstream.len());
                                                    let _ = contig.Unlock();
                                                }
                                            }
                                        }

                                        ManuallyDrop::drop(&mut out_data.pSample);
                                        ManuallyDrop::drop(&mut out_data.pEvents);
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                        break;
                    }
                }
            }
            CoTaskMemFree(Some(activate_ptrs as *const _));
        }

        let _ = MFShutdown();
        CoUninitialize();
    }
}
