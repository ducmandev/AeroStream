use tracing::{info, warn};
use windows::core::Interface;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;

#[derive(Debug)]
#[allow(dead_code)]
pub enum DxgiCaptureError {
    DeviceCreation(String),
    DuplicationFailed(String),
    AccessLost,
    Timeout,
    ResourceCast(String),
    MapFailed(String),
}

impl std::fmt::Display for DxgiCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for DxgiCaptureError {}

/// SendWrapper to safely send COM pointers across worker threads
#[repr(transparent)]
pub struct SendWrapper<T>(pub T);
unsafe impl<T> Send for SendWrapper<T> {}
unsafe impl<T> Sync for SendWrapper<T> {}

impl<T: Clone> Clone for SendWrapper<T> {
    fn clone(&self) -> Self {
        SendWrapper(self.0.clone())
    }
}

impl<T> std::ops::Deref for SendWrapper<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[allow(dead_code)]
pub struct DxgiFrame {
    pub texture: SendWrapper<ID3D11Texture2D>,
    pub width: u32,
    pub height: u32,
    pub has_changed: bool,
    pub dirty_rects: Vec<RECT>,
}

pub struct DxgiDuplicationCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    d3d_manager: IMFDXGIDeviceManager,
    captured_tex: ID3D11Texture2D,
    staging_tex: Option<ID3D11Texture2D>,
    width: u32,
    height: u32,
    accumulated_dirty_rects: Vec<RECT>,
}

#[allow(dead_code)]
impl DxgiDuplicationCapture {
    pub fn new() -> Result<Self, DxgiCaptureError> {
        unsafe {
            // Attach to current interactive input desktop (handles UAC / lock transition)
            let input_desk = windows_sys::Win32::System::StationsAndDesktops::OpenInputDesktop(0, 0, 0x10000000);
            if !input_desk.is_null() {
                windows_sys::Win32::System::StationsAndDesktops::SetThreadDesktop(input_desk);
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
            ).map_err(|e| DxgiCaptureError::DeviceCreation(format!("D3D11CreateDevice failed: {:?}", e)))?;

            let device = device.ok_or_else(|| DxgiCaptureError::DeviceCreation("No D3D11 device".into()))?;
            let context = context.ok_or_else(|| DxgiCaptureError::DeviceCreation("No D3D11 context".into()))?;

            // Enable multithread protection on D3D11 device for thread safety
            if let Ok(mt) = device.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }

            // Get DXGI output & monitor resolution
            let dxgi_device: IDXGIDevice = device.cast()
                .map_err(|e| DxgiCaptureError::DeviceCreation(format!("Cast IDXGIDevice: {:?}", e)))?;
            let adapter: IDXGIAdapter = dxgi_device.GetAdapter()
                .map_err(|e| DxgiCaptureError::DeviceCreation(format!("GetAdapter: {:?}", e)))?;

            let desc = adapter.GetDesc().map_err(|e| DxgiCaptureError::DeviceCreation(format!("GetDesc: {:?}", e)))?;
            let adapter_name = String::from_utf16_lossy(&desc.Description);
            let adapter_name = adapter_name.trim_matches('\0');

            let output: IDXGIOutput = adapter.EnumOutputs(0)
                .map_err(|e| DxgiCaptureError::DuplicationFailed(format!("EnumOutputs: {:?}", e)))?;
            let out_desc = output.GetDesc()
                .map_err(|e| DxgiCaptureError::DuplicationFailed(format!("Output GetDesc: {:?}", e)))?;

            let width = (out_desc.DesktopCoordinates.right - out_desc.DesktopCoordinates.left) as u32;
            let height = (out_desc.DesktopCoordinates.bottom - out_desc.DesktopCoordinates.top) as u32;

            let output1: IDXGIOutput1 = output.cast()
                .map_err(|e| DxgiCaptureError::DuplicationFailed(format!("Cast IDXGIOutput1: {:?}", e)))?;
            let duplication: IDXGIOutputDuplication = output1.DuplicateOutput(&device)
                .map_err(|e| DxgiCaptureError::DuplicationFailed(format!("DuplicateOutput error (already duplicate or locked): {:?}", e)))?;

            // Initialize Media Foundation DXGI Device Manager
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);

            let mut reset_token = 0u32;
            let mut dev_mgr_opt: Option<IMFDXGIDeviceManager> = None;
            MFCreateDXGIDeviceManager(&mut reset_token, &mut dev_mgr_opt)
                .map_err(|e| DxgiCaptureError::DeviceCreation(format!("MFCreateDXGIDeviceManager: {:?}", e)))?;

            let d3d_manager = dev_mgr_opt
                .ok_or_else(|| DxgiCaptureError::DeviceCreation("No IMFDXGIDeviceManager created".into()))?;
            d3d_manager.ResetDevice(&device, reset_token)
                .map_err(|e| DxgiCaptureError::DeviceCreation(format!("IMFDXGIDeviceManager ResetDevice: {:?}", e)))?;

            // Create persistent GPU captured texture
            let tex_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let mut captured_tex_opt: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&tex_desc, None, Some(&mut captured_tex_opt))
                .map_err(|e| DxgiCaptureError::DeviceCreation(format!("CreateTexture2D for captured_tex: {:?}", e)))?;

            let captured_tex = captured_tex_opt
                .ok_or_else(|| DxgiCaptureError::DeviceCreation("captured_tex is None".into()))?;

            info!(
                "[Phase 3.2 Direct DXGI Duplication] Initialized on '{}' ({}x{}) with D3D11 Device Manager & Multithread Lock",
                adapter_name, width, height
            );

            Ok(Self {
                device,
                context,
                duplication,
                d3d_manager,
                captured_tex,
                staging_tex: None,
                width,
                height,
                accumulated_dirty_rects: Vec::new(),
            })
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn d3d_manager(&self) -> SendWrapper<IMFDXGIDeviceManager> {
        SendWrapper(self.d3d_manager.clone())
    }

    pub fn device(&self) -> SendWrapper<ID3D11Device> {
        SendWrapper(self.device.clone())
    }

    pub fn context(&self) -> SendWrapper<ID3D11DeviceContext> {
        SendWrapper(self.context.clone())
    }

    pub fn take_accumulated_dirty_rects(&mut self) -> Vec<RECT> {
        std::mem::take(&mut self.accumulated_dirty_rects)
    }

    /// Acquire next frame from DXGI Desktop Duplication.
    /// Returns Ok(None) when timeout (static screen with 0 changes).
    /// Returns Ok(Some(DxgiFrame)) with GPU texture and dirty rects metadata.
    pub fn acquire_frame(&mut self, timeout_ms: u32) -> Result<Option<DxgiFrame>, DxgiCaptureError> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut desktop_resource: Option<IDXGIResource> = None;

            match self.duplication.AcquireNextFrame(timeout_ms, &mut frame_info, &mut desktop_resource) {
                Ok(()) => {
                    let mut dirty_rects = Vec::new();
                    if frame_info.TotalMetadataBufferSize > 0 {
                        let max_rects = frame_info.TotalMetadataBufferSize as usize / std::mem::size_of::<RECT>();
                        let mut rect_buf = vec![RECT::default(); max_rects.max(1)];
                        let mut dirty_size = 0u32;
                        if self.duplication.GetFrameDirtyRects(
                            (rect_buf.len() * std::mem::size_of::<RECT>()) as u32,
                            rect_buf.as_mut_ptr(),
                            &mut dirty_size,
                        ).is_ok() {
                            let count = (dirty_size as usize / std::mem::size_of::<RECT>()).min(rect_buf.len());
                            rect_buf.truncate(count);
                            dirty_rects = rect_buf;
                        }
                    }

                    let has_changed = frame_info.AccumulatedFrames > 0 || !dirty_rects.is_empty();

                    if has_changed {
                        self.accumulated_dirty_rects.extend_from_slice(&dirty_rects);
                    }

                    if let Some(res) = desktop_resource {
                        let desktop_tex: ID3D11Texture2D = res.cast()
                            .map_err(|e| DxgiCaptureError::ResourceCast(format!("Cast desktop texture: {:?}", e)))?;

                        // Copy directly into persistent GPU texture (GPU VRAM to GPU VRAM in ~0.02ms)
                        self.context.CopyResource(&self.captured_tex, &desktop_tex);
                    }

                    // Release DXGI duplication frame lock immediately
                    let _ = self.duplication.ReleaseFrame();

                    Ok(Some(DxgiFrame {
                        texture: SendWrapper(self.captured_tex.clone()),
                        width: self.width,
                        height: self.height,
                        has_changed,
                        dirty_rects,
                    }))
                }
                Err(e) => {
                    let code = e.code();
                    if code == DXGI_ERROR_WAIT_TIMEOUT {
                        Ok(None)
                    } else if code == DXGI_ERROR_ACCESS_LOST || code == E_ACCESSDENIED {
                        warn!("[Phase 3.2 Direct DXGI] Access lost or denied ({:?}), desktop might be locked", e);
                        Err(DxgiCaptureError::AccessLost)
                    } else {
                        warn!("[Phase 3.2 Direct DXGI] AcquireNextFrame error: {:?}", e);
                        Err(DxgiCaptureError::DuplicationFailed(format!("{:?}", e)))
                    }
                }
            }
        }
    }

    /// Readback CPU BGRA pixels lazily ONLY when JPEG fallback or initial preview is needed.
    /// For standard H.264 streaming, this is never called!
    pub fn read_cpu_pixels(&mut self, out_bgra: &mut Vec<u8>) -> Result<(), DxgiCaptureError> {
        unsafe {
            if self.staging_tex.is_none() {
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Width: self.width,
                    Height: self.height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    ..Default::default()
                };
                let mut st: Option<ID3D11Texture2D> = None;
                self.device.CreateTexture2D(&staging_desc, None, Some(&mut st))
                    .map_err(|e| DxgiCaptureError::DeviceCreation(format!("Create staging texture: {:?}", e)))?;
                self.staging_tex = st;
            }

            let staging = self.staging_tex.as_ref().unwrap();
            self.context.CopyResource(staging, &self.captured_tex);

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| DxgiCaptureError::MapFailed(format!("Map staging texture: {:?}", e)))?;

            let row_bytes = (self.width * 4) as usize;
            let total_bytes = row_bytes * (self.height as usize);
            if out_bgra.len() != total_bytes {
                out_bgra.resize(total_bytes, 0);
            }

            let src_pitch = mapped.RowPitch as usize;
            let src_ptr = mapped.pData as *const u8;
            let dst_ptr = out_bgra.as_mut_ptr();

            for y in 0..(self.height as usize) {
                let src_row = src_ptr.add(y * src_pitch);
                let dst_row = dst_ptr.add(y * row_bytes);
                std::ptr::copy_nonoverlapping(src_row, dst_row, row_bytes);
            }

            self.context.Unmap(staging, 0);
            Ok(())
        }
    }
}
