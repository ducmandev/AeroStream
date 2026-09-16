# AeroStream vs. Sunshine / Moonlight — Quy Chuẩn Benchmark & Số Liệu Mốc

> Cập nhật: 2026-09-11 · Tài liệu quy chuẩn cho Phase 1.4, làm mốc đối chiếu cho Phase 3 (Hardware Encoder MFT) và Phase 5 (WebRTC Transport).

---

## 1. Mục Đích & Phương Pháp Đo Đạc

Để đánh giá chính xác các bước nhảy hiệu năng khi chuyển đổi kiến trúc trong Master Plan v2:
- **Phase 1 (Hiện tại):** CPU Readback DXGI + fast_image_resize SIMD + openh264 software CPU encode.
- **Phase 3 (Mục tiêu phần cứng):** Direct DXGI Duplication → D3D11 GPU scale (NV12) → Media Foundation Hardware MFT (NVENC/QSV/AMF).
- **Phase 5 (Mục tiêu giao vận):** WebRTC SRTP (H.264 + Opus) + DataChannel thay thế WebSocket TCP.

Chúng ta sử dụng **Sunshine v0.23 + Moonlight Client** làm **Ground Truth Benchmark** (tiêu chuẩn vàng ngành game streaming độ trễ thấp) được đo trên cùng một cấu hình phần cứng máy chủ Windows.

---

## 2. Cấu Hình Thử Nghiệm Chuẩn (Reference Testbed)

- **Độ phân giải luồng:** 1920 × 1080 (1080p).
- **Tần số quét mục tiêu:** 60.0 FPS.
- **Môi trường mạng:** Local Area Network (Wi-Fi 5GHz / Gigabit Ethernet).
- **Nội dung test:**
  1. **Desktop Tĩnh:** Màn hình làm việc, văn bản, không di chuyển chuột.
  2. **Desktop Động (Thao tác nhanh):** Cuộn trang web dài, rê chuột liên tục, kéo thả cửa sổ.
  3. **3D Gaming (Chuyển động dày đặc):** Game 3D 60 FPS toàn màn hình với camera xoay chuyển liên tục.

---

## 3. Bảng Số Liệu Mốc (Benchmark Reference Matrix)

| Tiêu chí đo lường | Sunshine / Moonlight (NVENC HW) | AeroStream Hiện Tại (openh264 CPU) | AeroStream Mục Tiêu (Phase 3 HW + Phase 5 WebRTC) |
|---|:---:|:---:|:---:|
| **Host Capture Latency** | 1.8 – 3.2 ms | 3.5 – 5.0 ms (DXGI) | **≤ 2.0 ms** (Direct DXGI) |
| **Host Encode Time** | 2.1 – 4.0 ms | 12.0 – 22.0 ms (Software CPU) | **≤ 3.5 ms** (MFT HW NVENC) |
| **Host CPU Usage (Total)** | 3% – 7% | 45% – 85% (1–2 core bão hòa) | **≤ 8%** (GPU Offload hoàn toàn) |
| **Host GPU Video Engine** | 15% – 25% | 0% (Không dùng NVENC) | **15% – 25%** |
| **Băng thông Desktop Tĩnh** | ~0.5 Mbps | ~0.2 – 0.4 Mbps (Change Detection) | **≤ 0.2 Mbps** (Dirty Rects) |
| **Băng thông 1080p60 Động** | 8 – 15 Mbps | 5 – 9 Mbps (CBR Ladder) | **8 – 12 Mbps** |
| **Glass-to-Glass Latency** | **28 – 38 ms** | 45 – 75 ms | **≤ 35 ms** |
| **Chịu mất gói (5% packet loss)** | Mượt mà (FEC / RTP Jitter Buffer) | Rách hình / Đợi PLI IDR (~66ms) | Mượt mà (WebRTC GCC + PLI) |

---

## 4. Phân Tích Điểm Nghẽn Hiện Tại & Lộ Trình Đột Phá

### 4.1 Điểm nghẽn CPU Readback (8.3 MB / frame)
- **Hiện tại:** GPU sao chép texture sang staging RAM CPU qua `AcquireNextFrame` $\to$ CPU copy $\sim 498\text{ MB/s}$ ở 60 FPS $\to$ CPU scale bằng `fast_image_resize` $\to$ CPU convert RGB sang YUV420.
- **Kế hoạch Phase 3:** Giữ texture 100% trong VRAM GPU. Sử dụng D3D11 Video Processor hoặc compute shader để scale và đổi màu BGRA sang NV12 trong $<0.5\text{ms}$.

### 4.2 Điểm nghẽn Mã hóa Phần mềm (Software Encoder)
- **Hiện tại:** OpenH264 encode 1080p60 ăn 100% tài nguyên của 1 core, sinh nhiệt và cạnh tranh CPU với các tác vụ đồ họa/game của host.
- **Kế hoạch Phase 3:** Chuyển giao toàn bộ khung hình sang Media Foundation async MFT, tận dụng chip NVENC / Intel QuickSync / AMD AMF chuyên dụng với độ trễ encode cố định $< 4\text{ms}$.

### 4.3 Điểm nghẽn Giao vận WebSocket / TCP
- **Hiện tại:** Mạng chập chờn gây Head-of-line blocking do TCP bắt buộc truyền lại tuần tự, khiến hình ảnh bị khựng cục bộ cho đến khi client gửi `frame_loss` (PLI).
- **Kế hoạch Phase 5:** WebRTC với SRTP qua UDP cho phép drop frame trễ mà không chặn các frame mới, tích hợp cơ chế RTCP Sender Reports đồng bộ âm thanh/hình ảnh chính xác tới microsecond.
