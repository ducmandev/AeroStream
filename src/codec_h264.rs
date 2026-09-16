use crate::codec_mf::MediaFoundationH264Encoder;
use openh264::OpenH264API;
use openh264::encoder::{Encoder, EncoderConfig, RateControlMode, UsageType};
use openh264::formats::YUVBuffer;
use tracing::{info, warn};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager;

pub struct H264EncoderWrapper {
    mf_encoder: Option<MediaFoundationH264Encoder>,
    openh264_encoder: Option<Encoder>,
    nv12_buf: Vec<u8>,
    yuv420_buf: Vec<u8>,
    width: usize,
    height: usize,
    using_hardware: bool,
    d3d_manager: Option<IMFDXGIDeviceManager>,
}

impl H264EncoderWrapper {
    pub fn new() -> Self {
        Self {
            mf_encoder: None,
            openh264_encoder: None,
            nv12_buf: Vec::new(),
            yuv420_buf: Vec::new(),
            width: 0,
            height: 0,
            using_hardware: false,
            d3d_manager: None,
        }
    }

    pub fn set_d3d_manager(&mut self, d3d_manager: IMFDXGIDeviceManager) {
        self.d3d_manager = Some(d3d_manager);
    }

    #[allow(dead_code)]
    pub fn is_d3d_aware(&self) -> bool {
        self.mf_encoder.as_ref().map(|mf| mf.is_d3d_aware()).unwrap_or(false)
    }

    fn calculate_bitrate(height: usize) -> u32 {
        if height <= 540 {
            2_000_000
        } else if height <= 720 {
            4_000_000
        } else if height <= 1080 {
            7_000_000
        } else {
            12_000_000
        }
    }

    pub fn init(&mut self, width: usize, height: usize) {
        let bitrate_bps = Self::calculate_bitrate(height);
        self.width = width;
        self.height = height;

        // 1. Primary path: Attempt Hardware Media Foundation MFT (Intel QSV / NVENC / AMF) with D3D11
        if let Some(mf) = MediaFoundationH264Encoder::new(width, height, bitrate_bps, self.d3d_manager.clone()) {
            info!(
                "[Phase 3.2+ HW MFT] Using Hardware Video Encoder: '{}' ({}x{} @ {:.1} Mbps, D3D11: {})",
                mf.hardware_name(),
                width,
                height,
                bitrate_bps as f64 / 1_000_000.0,
                mf.is_d3d_aware()
            );
            self.mf_encoder = Some(mf);
            self.openh264_encoder = None;
            self.using_hardware = true;
            return;
        }

        // 2. Secondary fallback: High-speed SIMD OpenH264
        info!(
            "[Phase 3.1] Hardware MFT not available; initializing OpenH264 software fallback for {}x{}",
            width, height
        );
        self.mf_encoder = None;
        self.openh264_encoder = Self::init_openh264(width, height, bitrate_bps);
        self.using_hardware = false;
    }

    fn init_openh264(width: usize, height: usize, bitrate_bps: u32) -> Option<Encoder> {
        let config = EncoderConfig::new()
            .set_bitrate_bps(bitrate_bps)
            .max_frame_rate(60.0)
            .rate_control_mode(RateControlMode::Quality)
            .usage_type(UsageType::ScreenContentRealTime);

        match Encoder::with_api_config(OpenH264API::from_source(), config) {
            Ok(enc) => {
                info!(
                    "H.264 ScreenContentRealTime OpenH264 initialized for {}x{} at {:.1} Mbps (60 FPS)",
                    width,
                    height,
                    bitrate_bps as f64 / 1_000_000.0
                );
                Some(enc)
            }
            Err(e) => {
                warn!("Failed to initialize OpenH264 encoder: {:?}", e);
                None
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_hardware_accelerated(&self) -> bool {
        self.using_hardware
    }

    pub fn force_intra_frame(&mut self) {
        if let Some(ref mut mf) = self.mf_encoder {
            mf.force_idr();
        }
        if let Some(ref mut enc) = self.openh264_encoder {
            enc.force_intra_frame();
        }
    }

    /// High-speed single-pass BGRA -> NV12 converter (Y plane + interleaved UV plane)
    #[inline(always)]
    pub fn convert_bgra_to_nv12(bgra: &[u8], width: usize, height: usize, out_nv12: &mut [u8]) {
        let y_plane_size = width * height;
        let (y_plane, uv_plane) = out_nv12.split_at_mut(y_plane_size);

        for y in (0..height).step_by(2) {
            let row1_bgra_idx = y * width * 4;
            let row2_bgra_idx = (y + 1) * width * 4;
            let row1_y_idx = y * width;
            let row2_y_idx = (y + 1) * width;
            let uv_row_idx = (y / 2) * width;

            for x in (0..width).step_by(2) {
                let p00 = row1_bgra_idx + x * 4;
                let p01 = row1_bgra_idx + (x + 1) * 4;
                let p10 = row2_bgra_idx + x * 4;
                let p11 = row2_bgra_idx + (x + 1) * 4;

                let b0 = bgra[p00] as i32;
                let g0 = bgra[p00 + 1] as i32;
                let r0 = bgra[p00 + 2] as i32;
                y_plane[row1_y_idx + x] = (((66 * r0 + 129 * g0 + 25 * b0 + 128) >> 8) + 16) as u8;

                let b1 = bgra[p01] as i32;
                let g1 = bgra[p01 + 1] as i32;
                let r1 = bgra[p01 + 2] as i32;
                y_plane[row1_y_idx + x + 1] = (((66 * r1 + 129 * g1 + 25 * b1 + 128) >> 8) + 16) as u8;

                let b2 = bgra[p10] as i32;
                let g2 = bgra[p10 + 1] as i32;
                let r2 = bgra[p10 + 2] as i32;
                y_plane[row2_y_idx + x] = (((66 * r2 + 129 * g2 + 25 * b2 + 128) >> 8) + 16) as u8;

                let b3 = bgra[p11] as i32;
                let g3 = bgra[p11 + 1] as i32;
                let r3 = bgra[p11 + 2] as i32;
                y_plane[row2_y_idx + x + 1] = (((66 * r3 + 129 * g3 + 25 * b3 + 128) >> 8) + 16) as u8;

                let ravg = (r0 + r1 + r2 + r3) >> 2;
                let gavg = (g0 + g1 + g2 + g3) >> 2;
                let bavg = (b0 + b1 + b2 + b3) >> 2;

                let u = (((-38 * ravg - 74 * gavg + 112 * bavg + 128) >> 8) + 128) as u8;
                let v = (((112 * ravg - 94 * gavg - 18 * bavg + 128) >> 8) + 128) as u8;

                let uv_idx = uv_row_idx + x;
                uv_plane[uv_idx] = u;
                uv_plane[uv_idx + 1] = v;
            }
        }
    }

    /// High-speed single-pass BGRA -> YUV420p converter (planar Y, U, V)
    #[inline(always)]
    pub fn convert_bgra_to_yuv420p(bgra: &[u8], width: usize, height: usize, out_yuv: &mut [u8]) {
        let y_plane_size = width * height;
        let uv_plane_size = (width / 2) * (height / 2);
        let (y_plane, rest) = out_yuv.split_at_mut(y_plane_size);
        let (u_plane, v_plane) = rest.split_at_mut(uv_plane_size);

        for y in (0..height).step_by(2) {
            let row1_bgra_idx = y * width * 4;
            let row2_bgra_idx = (y + 1) * width * 4;
            let row1_y_idx = y * width;
            let row2_y_idx = (y + 1) * width;
            let uv_idx = (y / 2) * (width / 2);

            for x in (0..width).step_by(2) {
                let p00 = row1_bgra_idx + x * 4;
                let p01 = row1_bgra_idx + (x + 1) * 4;
                let p10 = row2_bgra_idx + x * 4;
                let p11 = row2_bgra_idx + (x + 1) * 4;

                let b0 = bgra[p00] as i32;
                let g0 = bgra[p00 + 1] as i32;
                let r0 = bgra[p00 + 2] as i32;
                y_plane[row1_y_idx + x] = (((66 * r0 + 129 * g0 + 25 * b0 + 128) >> 8) + 16) as u8;

                let b1 = bgra[p01] as i32;
                let g1 = bgra[p01 + 1] as i32;
                let r1 = bgra[p01 + 2] as i32;
                y_plane[row1_y_idx + x + 1] = (((66 * r1 + 129 * g1 + 25 * b1 + 128) >> 8) + 16) as u8;

                let b2 = bgra[p10] as i32;
                let g2 = bgra[p10 + 1] as i32;
                let r2 = bgra[p10 + 2] as i32;
                y_plane[row2_y_idx + x] = (((66 * r2 + 129 * g2 + 25 * b2 + 128) >> 8) + 16) as u8;

                let b3 = bgra[p11] as i32;
                let g3 = bgra[p11 + 1] as i32;
                let r3 = bgra[p11 + 2] as i32;
                y_plane[row2_y_idx + x + 1] = (((66 * r3 + 129 * g3 + 25 * b3 + 128) >> 8) + 16) as u8;

                let ravg = (r0 + r1 + r2 + r3) >> 2;
                let gavg = (g0 + g1 + g2 + g3) >> 2;
                let bavg = (b0 + b1 + b2 + b3) >> 2;

                let col = x / 2;
                u_plane[uv_idx + col] = (((-38 * ravg - 74 * gavg + 112 * bavg + 128) >> 8) + 128) as u8;
                v_plane[uv_idx + col] = (((112 * ravg - 94 * gavg - 18 * bavg + 128) >> 8) + 128) as u8;
            }
        }
    }

    /// Direct D3D11 Texture Encoding (Zero-Copy GPU pipeline)
    pub fn encode_texture(&mut self, texture: &ID3D11Texture2D, width: usize, height: usize) -> Option<Vec<u8>> {
        if self.width != width
            || self.height != height
            || (self.using_hardware && self.mf_encoder.is_none())
        {
            self.init(width, height);
        }

        if self.using_hardware {
            if let Some(ref mut mf) = self.mf_encoder {
                if mf.is_d3d_aware() {
                    if let Some(bitstream) = mf.encode_surface(texture) {
                        return Some(bitstream);
                    }
                }
            }
        }

        None
    }

    pub fn encode(&mut self, bgra: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
        if self.width != width
            || self.height != height
            || (self.using_hardware && self.mf_encoder.is_none())
            || (!self.using_hardware && self.openh264_encoder.is_none())
        {
            self.init(width, height);
        }

        // Primary: Hardware MFT Encoding
        if self.using_hardware {
            let required_len = width * height * 3 / 2;
            if self.nv12_buf.len() != required_len {
                self.nv12_buf.resize(required_len, 0);
            }
            Self::convert_bgra_to_nv12(bgra, width, height, &mut self.nv12_buf);

            if let Some(ref mut mf) = self.mf_encoder {
                if let Some(bitstream) = mf.encode(&self.nv12_buf) {
                    return Some(bitstream);
                }
            }
            warn!("[Phase 3.1] Hardware MFT frame encode returned None, falling back to OpenH264");
        }

        // Secondary fallback: OpenH264 Software Encoding
        if self.openh264_encoder.is_none() {
            let bitrate_bps = Self::calculate_bitrate(height);
            self.openh264_encoder = Self::init_openh264(width, height, bitrate_bps);
        }

        let encoder = self.openh264_encoder.as_mut()?;
        let required_len = width * height * 3 / 2;
        if self.yuv420_buf.len() != required_len {
            self.yuv420_buf.resize(required_len, 0);
        }
        Self::convert_bgra_to_yuv420p(bgra, width, height, &mut self.yuv420_buf);

        let yuv = YUVBuffer::from_vec(self.yuv420_buf.clone(), width, height);

        match encoder.encode(&yuv) {
            Ok(bitstream) => {
                let vec = bitstream.to_vec();
                if !vec.is_empty() {
                    Some(vec)
                } else {
                    None
                }
            }
            Err(e) => {
                warn!("OpenH264 encode frame error: {:?}", e);
                None
            }
        }
    }
}
