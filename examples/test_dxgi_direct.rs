use std::time::Duration;
use windows::core::Interface;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Testing Direct DXGI Duplication & Dirty Rects ---");

    unsafe {
        let desk = windows_sys::Win32::System::StationsAndDesktops::OpenInputDesktop(0, 0, 0x10000000);
        if !desk.is_null() {
            windows_sys::Win32::System::StationsAndDesktops::SetThreadDesktop(desk);
        }
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut feature_level = D3D_FEATURE_LEVEL_11_0;

        let create_flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];

        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            create_flags,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )?;

        let device = device.ok_or("No device created")?;
        let context = context.ok_or("No context created")?;
        println!("[+] D3D11Device created with feature level: {:?}", feature_level);

        let multithread: Result<windows::Win32::Graphics::Direct3D11::ID3D11Multithread, _> = device.cast();
        println!("[+] ID3D11Multithread supported: {:?}", multithread.is_ok());
        if let Ok(ref mt) = multithread {
            mt.SetMultithreadProtected(true);
            println!("[+] ID3D11Multithread SetMultithreadProtected(true) enabled!");
        }

        let video_device: Result<ID3D11VideoDevice, _> = device.cast();
        let video_context: Result<ID3D11VideoContext, _> = context.cast();
        println!("[+] ID3D11VideoDevice: {:?}, ID3D11VideoContext: {:?}", video_device.is_ok(), video_context.is_ok());
        if let (Ok(ref vd), Ok(ref _vc)) = (video_device, video_context) {
            let enum_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                InputWidth: 1920,
                InputHeight: 1080,
                OutputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                OutputWidth: 1280,
                OutputHeight: 720,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            if let Ok(e) = vd.CreateVideoProcessorEnumerator(&enum_desc) {
                if let Ok(vp) = vd.CreateVideoProcessor(&e, 0) {
                    println!("[+] VideoProcessor created successfully!");

                    // Create destination texture 1280x720
                    let dst_desc = D3D11_TEXTURE2D_DESC {
                        Width: 1280,
                        Height: 720,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                        ..Default::default()
                    };
                    let mut scaled_tex: Option<ID3D11Texture2D> = None;
                    if device.CreateTexture2D(&dst_desc, None, Some(&mut scaled_tex)).is_ok() {
                        let out_v_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                            },
                        };
                        let mut out_view: Option<ID3D11VideoProcessorOutputView> = None;
                        let out_res = vd.CreateVideoProcessorOutputView(&scaled_tex.unwrap(), &e, &out_v_desc, Some(&mut out_view));
                        println!("[+] CreateVideoProcessorOutputView: {:?}, is_some: {:?}", out_res, out_view.is_some());
                    }
                }
            }
        }

        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = dxgi_device.GetAdapter()?;
        let desc = adapter.GetDesc()?;
        let name = String::from_utf16_lossy(&desc.Description);
        println!("[+] DXGI Adapter: {}", name.trim_matches('\0'));

        let output: IDXGIOutput = adapter.EnumOutputs(0)?;
        let out_desc = output.GetDesc()?;
        println!(
            "[+] Monitor resolution: {}x{}",
            out_desc.DesktopCoordinates.right - out_desc.DesktopCoordinates.left,
            out_desc.DesktopCoordinates.bottom - out_desc.DesktopCoordinates.top
        );

        let output1: IDXGIOutput1 = output.cast()?;
        let duplication: IDXGIOutputDuplication = output1.DuplicateOutput(&device)?;
        println!("[+] IDXGIOutputDuplication acquired successfully!");

        println!("[*] Capturing 5 frames to test dirty rects metadata...");
        for i in 0..5 {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut desktop_resource: Option<IDXGIResource> = None;

            match duplication.AcquireNextFrame(500, &mut frame_info, &mut desktop_resource) {
                Ok(()) => {
                    let mut dirty_count = 0;
                    if frame_info.TotalMetadataBufferSize > 0 {
                        let max_rects = frame_info.TotalMetadataBufferSize as usize / std::mem::size_of::<RECT>();
                        let mut dirty_rects = vec![RECT::default(); max_rects.max(1)];
                        let mut dirty_size = 0u32;
                        if duplication.GetFrameDirtyRects(
                            (dirty_rects.len() * std::mem::size_of::<RECT>()) as u32,
                            dirty_rects.as_mut_ptr(),
                            &mut dirty_size,
                        ).is_ok() {
                            dirty_count = dirty_size as usize / std::mem::size_of::<RECT>();
                        }
                    }

                    println!(
                        "   Frame #{}: Accumulated: {}, Dirty Rects: {}, MouseMove: {}",
                        i,
                        frame_info.AccumulatedFrames,
                        dirty_count,
                        frame_info.LastMouseUpdateTime
                    );

                    if let Some(ref res) = desktop_resource {
                        let tex: windows::core::Result<ID3D11Texture2D> = res.cast();
                        if let Ok(ref t) = tex {
                            let mut desc = D3D11_TEXTURE2D_DESC::default();
                            t.GetDesc(&mut desc);
                            println!("      Texture: {}x{}, format: {:?}", desc.Width, desc.Height, desc.Format);

                            let surface_buf = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, t, 0, false);
                            println!("      MFCreateDXGISurfaceBuffer result: {:?}", surface_buf.is_ok());
                        }
                    }

                    let _ = duplication.ReleaseFrame();
                }
                Err(e) => {
                    println!("   Frame #{}: AcquireNextFrame returned: {:?}", i, e);
                }
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        println!("[+] Direct DXGI Duplication test completed successfully!");

        // Test Media Foundation D3D11 Device Manager with QSV
        use windows::Win32::Media::MediaFoundation::*;
        use windows::Win32::System::Com::*;
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_ok() {
            let mut reset_token = 0u32;
            let mut dev_mgr: Option<IMFDXGIDeviceManager> = None;
            let mgr_res = MFCreateDXGIDeviceManager(&mut reset_token, &mut dev_mgr);
            println!("[+] MFCreateDXGIDeviceManager: {:?}", mgr_res);
            if let Some(dm) = dev_mgr {
                let reset_res = dm.ResetDevice(&device, reset_token);
                println!("[+] IMFDXGIDeviceManager ResetDevice: {:?}", reset_res);

                // Find QSV
                let mut activate_ptrs: *mut Option<IMFActivate> = std::ptr::null_mut();
                let mut count = 0u32;
                if MFTEnumEx(
                    MFT_CATEGORY_VIDEO_ENCODER,
                    MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                    None,
                    None,
                    &mut activate_ptrs,
                    &mut count,
                ).is_ok() && count > 0 {
                    let acts = std::slice::from_raw_parts_mut(activate_ptrs, count as usize);
                    if let Some(act) = acts[0].take() {
                        let mft: IMFTransform = act.ActivateObject().expect("activate");
                        if let Ok(attrs) = mft.GetAttributes() {
                            let _ = attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1);
                            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
                            let d3d11_aware = attrs.GetUINT32(&MF_SA_D3D11_AWARE);
                            println!("[+] QSV MFT MF_SA_D3D11_AWARE: {:?}", d3d11_aware);
                            let d3d_aware = attrs.GetUINT32(&MF_SA_D3D_AWARE);
                            println!("[+] QSV MFT MF_SA_D3D_AWARE: {:?}", d3d_aware);
                        }

                        // Send D3D manager to MFT
                        let set_mgr_res = mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, dm.as_raw() as usize);
                        println!("[+] QSV MFT MFT_MESSAGE_SET_D3D_MANAGER: {:?}", set_mgr_res);

                        // Enumerate output types from MFT
                        println!("[*] Available output types:");
                        let mut o_idx = 0;
                        while let Ok(ot) = mft.GetOutputAvailableType(0, o_idx) {
                            let sub = ot.GetGUID(&MF_MT_SUBTYPE);
                            println!("    Output type #{}: {:?}", o_idx, sub);
                            o_idx += 1;
                            if o_idx > 5 { break; }
                        }

                        // Configure ICodecAPI
                        let codec_api: Option<ICodecAPI> = mft.cast().ok();
                        if let Some(ref api) = codec_api {
                            let mut true_var = windows::core::VARIANT::from(-1i16);
                            let _ = api.SetValue(&CODECAPI_AVLowLatencyMode, &mut true_var);
                            let mut b_frames = windows::core::VARIANT::from(0u32);
                            let _ = api.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &mut b_frames);
                            let mut gop = windows::core::VARIANT::from(60u32);
                            let _ = api.SetValue(&CODECAPI_AVEncMPVGOPSize, &mut gop);
                            let mut rc = windows::core::VARIANT::from(eAVEncCommonRateControlMode_Quality.0 as u32);
                            let _ = api.SetValue(&CODECAPI_AVEncCommonRateControlMode, &mut rc);
                            let mut br = windows::core::VARIANT::from(4_000_000u32);
                            let _ = api.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &mut br);
                        }

                        if let Ok(ot0) = mft.GetOutputAvailableType(0, 0) {
                            println!("[+] ot0 major type: {:?}", ot0.GetGUID(&MF_MT_MAJOR_TYPE));
                            println!("[+] ot0 subtype: {:?}", ot0.GetGUID(&MF_MT_SUBTYPE));
                            let mut ot_configured = ot0;
                            ot_configured.SetUINT32(&MF_MT_AVG_BITRATE, 4_000_000).unwrap();
                            ot_configured.SetUINT64(&MF_MT_FRAME_RATE, ((60u64) << 32) | 1).unwrap();
                            ot_configured.SetUINT64(&MF_MT_FRAME_SIZE, ((1920u64) << 32) | 1080).unwrap();
                            ot_configured.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).unwrap();
                            let set_out = mft.SetOutputType(0, &ot_configured, 0);
                            println!("[+] SetOutputType ot_configured: {:?}", set_out);
                        }

                        // Select ARGB32 as input type!
                        let in_argb = MFCreateMediaType().unwrap();
                        in_argb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
                        in_argb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32).unwrap();
                        in_argb.SetUINT64(&MF_MT_FRAME_RATE, ((60u64) << 32) | 1).unwrap();
                        in_argb.SetUINT64(&MF_MT_FRAME_SIZE, ((1920u64) << 32) | 1080).unwrap();
                        in_argb.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).unwrap();
                        let set_in_argb = mft.SetInputType(0, &in_argb, 0);
                        println!("[+] SetInputType explicit ARGB32: {:?}", set_in_argb);

                        // Start streaming
                        let _ = mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
                        let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
                        let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);

                        let event_gen: IMFMediaEventGenerator = mft.cast().unwrap();

                        // Acquire 1 frame and encode it
                        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
                        let mut desktop_resource: Option<IDXGIResource> = None;
                        if duplication.AcquireNextFrame(500, &mut frame_info, &mut desktop_resource).is_ok() {
                            if let Some(res) = desktop_resource {
                                let tex: ID3D11Texture2D = res.cast().unwrap();
                                let surf_buf = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &tex, 0, false).unwrap();
                                let sample = MFCreateSample().unwrap();
                                sample.AddBuffer(&surf_buf).unwrap();
                                sample.SetSampleTime(0).unwrap();
                                sample.SetSampleDuration(166666).unwrap();

                                println!("[*] Pumping MFT with ARGB32 D3D11 surface buffer...");
                                let mut input_sent = false;
                                for _ in 0..25 {
                                    let ev = match event_gen.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                                        Ok(e) => e,
                                        Err(_) => {
                                            std::thread::sleep(Duration::from_millis(1));
                                            continue;
                                        }
                                    };
                                    let ev_type = ev.GetType().unwrap_or(0);
                                    if ev_type == METransformNeedInput.0 as u32 && !input_sent {
                                        let in_res = mft.ProcessInput(0, &sample, 0);
                                        println!("    ProcessInput with D3D11 ARGB32 surface: {:?}", in_res);
                                        if in_res.is_ok() { input_sent = true; }
                                    } else if ev_type == METransformHaveOutput.0 as u32 {
                                        let mut out_data = MFT_OUTPUT_DATA_BUFFER {
                                            dwStreamID: 0,
                                            pSample: std::mem::ManuallyDrop::new(None),
                                            dwStatus: 0,
                                            pEvents: std::mem::ManuallyDrop::new(None),
                                        };
                                        let mut status = 0u32;
                                        let out_res = mft.ProcessOutput(0, std::slice::from_mut(&mut out_data), &mut status);
                                        println!("    ProcessOutput result: {:?}", out_res);
                                        if let Some(ref out_sample) = *out_data.pSample {
                                            let buf = out_sample.ConvertToContiguousBuffer().unwrap();
                                            let len = buf.GetCurrentLength().unwrap();
                                            println!("    [SUCCESS!] Produced H.264 packet: {} bytes from direct D3D11 surface!", len);
                                        }
                                        break;
                                    }
                                }
                            }
                            let _ = duplication.ReleaseFrame();
                        }
                    }
                    CoTaskMemFree(Some(activate_ptrs as *const _));
                }
            }
            let _ = MFShutdown();
        }
    }

    Ok(())
}
