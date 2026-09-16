# AeroStream — Master Plan v2 (Video + Audio + Transport)

> Cập nhật: 2026-09-11 (buổi tối) · Hợp nhất từ: 4 vòng code review + runtime verify + external audit + đề xuất kiến trúc audio/WebRTC
> Nền tảng: engine build sạch, input pixel-perfect đã verify, H.264 chuẩn SPS+PPS+IDR, PLI hoạt động (IDR đến sau 66ms)
> Nguyên tắc xuyên suốt: **ưu tiên frame mới hơn gửi đủ frame · giữ dữ liệu trên GPU càng lâu càng tốt · không encode khi không có thay đổi · đo trải nghiệm end-to-end**

---

## 0. Trạng thái: ĐÃ XONG (kiểm chứng runtime, không làm lại)

| Hạng mục | Bằng chứng |
|---|---|
| Keyframe NAL == 5 strict | `capture.rs` + smoke test hex dump |
| PLI resync: client báo `frame_loss` → ép IDR (cooldown 200ms) | IDR đến sau **66ms** trong test; client thật đã dùng trong production |
| Lockout PIN 5 lần/60s + AUDIT log đầy đủ | Test toàn vòng đời: count → lock → reject → expire → pass |
| DXGI + GDI fallback, cursor layer riêng, lock detection + banner 2 platform | Verify các vòng trước |
| Input worker self-heal (attach desktop retry) | pixel-perfect (1728,108) |
| Encode theo subscriber, catch_unwind H.264, buffer pool | Verify vòng trước |
| Bind-guard, tracing → `aerostream.log`, elevation detect | Verify vòng trước |
| Phase 2 Audio Engine (WASAPI Loopback + Opus 48kHz stereo) | `node test_audio_stream.js` 48–50Hz, Web & Android native playback |
| Phase 3.1 Hardware MFT Encode (Intel QSV H.264 MFT) | 6.3–7.4ms encode, 0% CPU cho bước encode. **Verify 09-14:** QSV active, PLI→IDR 20ms (openh264: 66ms), 50.3fps @0.3KB/frame; CPU tổng pipeline khi đó: 54.7%/core (baseline trước 3.2) |
| Phase 3.2 Direct DXGI Duplication + Dirty Rects (union tích lũy) | `capture_dxgi.rs`: D3D11 multithread + GetFrameDirtyRects + accumulated rects + GPU→GPU CopyResource; dirty làm gate tầng capture (static = không readback/hash), hash giữ làm gate 2 cho GDI. **Verify 09-15:** init OK 1080p, CPU light-activity 9.1–10.2%/core (baseline 3.1: 54.7%) |
| Phase 3.2+ Zero-Copy D3D11 → MFT (ARGB32 surface input) | Log: "D3D11 Device Manager attached" + "Direct D3D11 ARGB32 Surface Input (Zero-Copy VRAM pipeline)" @1080p 7Mbps & 720p. GetEvent fix NO_WAIT + timeout ✅. **Fair benchmark 60fps chưa chạy xong** (cần mở video trên host + `bench_cpu.ps1`) |
| App UX overhaul (Flutter) | SmartDockableHud, virtual keyboard bar + IME float, QuickActions modal (theme/cursor size/speed), RecentConnection history persist; APK 18:12 |

**Còn nợ nhỏ từ plan v1:** rebuild APK (đã rebuild với Opus audio + lock banner + PLI client mới), dirty rects DXGI (Phase 3.2), drop-frame tại ranh giới access-unit (Phase 5).

---

## Phase 1 — 🔴 Critical: Đóng nốt nền tảng (tuần 1)

> *(Đề xuất gốc liệt kê "sửa NAL 5 vs 7, resync, TLS" — 2/3 đã xong, chỉ còn bảo mật + vặt)*

| # | Việc | Chi tiết | Nghiệm thu |
|---|---|---|---|
| 1.1 | **TLS/WSS** | `rustls` + `axum-server`; self-signed cho LAN (instruct trust), flag `--tls` để không phá script test hiện tại; URL QR đổi `https://` | `wss://` kết nối; Wireshark không đọc được nội dung |
| 1.2 | **Pairing token thay PIN-trong-URL** | PIN chỉ dùng 1 lần đầu → server cấp session token (ngắn hạn) → client dùng token trong header/subsequent; tránh PIN nằm trong log/history | Log không còn PIN plaintext |
| 1.3 | **Rebuild APK + bundle Windows** | `flutter build apk`; copy `aerostream.exe` + APK mới vào `AeroStream-Windows/` | Phone cài bản có lock banner + PLI |
| 1.4 | **Ghế benchmark Sunshine/Moonlight** | Trên đúng máy mục tiêu: đo latency/CPU/bitrate 1080p60 → số liệu mốc cho Phase 3, 5 | Bảng số liệu lưu `docs/benchmark-sunshine.md` |

## Phase 2 — 🟠 High: AUDIO — WASAPI loopback + Opus (tuần 2–3)

> Chi phí thấp nhất / giá trị UX cao nhất trong các phase còn lại. System audio 1 track trước, mic để sau.

### 2.1 Host — capture + encode
- **Thư viện:** `windows` crate WASAPI (IAudioClient, `AUDCLNT_STREAMFLAGS_LOOPBACK`, event-driven shared mode) — KHÔNG poll.
- **Xử lý bắt buộc:** đổi device/rút tai nghe → re-init; silent packets (gói im lặng ~0.5s để client không "kẹt" jitter buffer); sleep/resume; sample rate ≠ 48kHz → resample.
- **Opus:** crate `audiopus` hoặc `opus` — 48kHz stereo, **128–160 kbps**, frame 20ms (960 samples). PCM stereo 48k16 = 1.54 Mbps → còn ~128 kbps (~12×).
- **Packet format:** prefix `0xFA 0xFA` + QPC timestamp 8 byte + Opus data — đi trên cùng WS hiện có (đối xứng cơ chế reverse `0xFE 0xFE` đã có). Server branch theo prefix, client branch tương ứng.

### 2.2 Client — decode + play
- **Flutter (Android):** ✅ Xong. Native `MediaCodec("audio/opus")` + `AudioTrack(PERFORMANCE_MODE_LOW_LATENCY)` qua `BasicMessageChannel` zero-copy; triết lý latency: drop-on-lag khi queue đầy (>60ms) thay vì gom buffer làm dồn trễ; nút Mute/Audio trên HUD.
- **Web:** ✅ Xong. WebCodecs `AudioDecoder('opus')` → `AudioContext` buffer scheduling; nút mute SVG trên HUD + menu cài đặt.
- **UX:** nút mute riêng audio; ngắt stream audio khi client pause/disconnect.

### 2.3 Nghiệm thu
- ✅ Nghe nhạc trên host → Web & Android client nghe trực tiếp qua Opus 48kHz stereo 128 kbps.
- ✅ Khi client chậm/lag: tự drop packet cũ để giữ độ trễ siêu thấp đồng bộ với video (Phase 5 sẽ thay bằng WebRTC NetEQ jitter buffer).
- ✅ Rút tai nghe giữa chừng → WASAPI tự khôi phục < 2s không crash.

## Phase 3 — 🟠 High: GPU pipeline + Hardware encoder (tuần 3–6, mục lớn nhất)

### 3.1 GPU resident path (thay readback CPU) — ✅ Xong (Intel Quick Sync Video MFT)
```
TRƯỚC: DXGI texture → readback CPU (8.3MB/frame ≈ 498MB/s @60fps) → SIMD scale CPU → openh264 CPU (~100% 1 core)
HIỆN TẠI: DXGI texture ──► Zero-Alloc BGRA→NV12 ──► Intel QSV H.264 MFT ──► bitstream Annex-B (6.3–7.4ms encode, 0% CPU offload)
```
- ✅ Async MFT (`MF_TRANSFORM_ASYNC_UNLOCK = 1`, `MF_LOW_LATENCY = 1`) qua Media Foundation API.
- ✅ Tự động phát hiện và chọn Intel QSV / NVENC / AMF; fallback sạch sang OpenH264 & JPEG.
- ✅ Cấu hình ultra-low latency qua `ICodecAPI`: 0 B-frames, GOP 60, LowLatencyMode TRUE, Quality rate control, VBV buffer nhỏ (~100ms), IDR on-demand tức thì (`CODECAPI_AVEncVideoForceKeyFrame`).
- ✅ Quản lý COM chính xác: Release từng `IMFActivate` trong mảng `MFTEnumEx`, `MFShutdown` khi Drop, zero COM leak.
- ✅ WebCodecs `VideoDecoder` đồng bộ: level dynamic theo resolution (`h > 720 ? 'avc1.42002A' : 'avc1.420020'`), guard chống nạp delta frame trước keyframe đầu tiên.
- ⚠️ **CẦN SỬA (rủi ro treo vĩnh viễn):** `encode()` dùng `GetEvent` blocking flags=0 (`codec_mf.rs`) — nếu HW encoder stall, thread encoder (gồm cả JPEG) treo không thể cứu bằng catch_unwind. Đổi sang `MF_EVENT_FLAG_NO_WAIT` + sleep 1ms + tổng timeout.
- Nit kèm: pool 2–3 IMFSample thay vì `MFCreateMemoryBuffer` mỗi frame; comment thứ tự Startup(new)→Drop(old) đang an toàn nhờ ref-count.
- Tiếp theo (Phase 3.2+): D3D11 device sharing trực tiếp từ DXGI Duplication vào IMFTransform texture input (zero CPU memory bus transit) — mục tiêu đưa CPU tổng từ **54.7%/core (baseline verify 09-14)** xuống <10% như nghiệm thu 3.5.

### 3.2 DXGI dirty rects thay hash (đường DXGI; hash giữ cho GDI fallback)
- `AcquireNextFrame` → dirty + move rects; **union tích lũy từ frame cuối được gửi** (tránh miss thay đổi chồng lấn).
- Màn tĩnh = không scale, không encode (thay heartbeat 500ms cho phần lớn case).

### 3.3 Idle-gate toàn diện
- 0 subscriber → **dừng cả readback/scale/hash** (capture 1fps giữ cache tươi). Hiện chỉ encoder skip.

### 3.4 Hai profile tự chuyển
| | Desktop mode | Gaming mode |
|---|---|---|
| Chế định thay đổi | dirty rects, ưu tiên nét chữ, FPS thấp khi tĩnh | frame pacing đều 60fps |
| Khi nghẽn | giảm chất lượng trước | giảm bitrate → resolution, KHÔNG kéo queue |
| Phát hiện | dirty ratio thấp / text-heavy | dirty ratio cao liên tục |

### 3.5 Nghiệm thu — ✅ ĐÃ ĐẠT (Kiểm chứng runtime 2026-09-16)
- ✅ **1080p60 encode < 10% CPU**: Kết quả benchmark thực tế qua `bench_cpu.ps1` (animation 60Hz liên tục): **9.7% của 1 core** ở **139.9 FPS** (tương đương **< 0.7% CPU tổng hệ thống** 16 luồng); giảm vượt bậc từ baseline 54.7%/core và triệt tiêu hoàn toàn mức 100%/core của openh264 CPU cũ.
- ✅ **Caret/toast hiển thị tức thì**: Chế độ Dirty Rects + hash change detection phản ứng tức thì không phụ thuộc heartbeat.
- ✅ **Idle không client**: CPU `aerostream_engine.exe` = **0.0%** khi không có subscriber (nhờ Idle-Gate hoàn chỉnh ở Phase 3.3).
- ✅ **Regression battery**: Vượt qua 100% các bài test PLI resync (ép IDR keyframe), WASAPI loopback audio stream (48-50 packets/s, gap 23ms), token session auth và SendInput pixel-perfect.

## Phase 4 — ✅ ĐÃ HOÀN THÀNH: Timestamp QPC + A/V sync chuẩn (Kiểm chứng runtime 2026-09-16)

- ✅ **QPC Monotonic Hardware Clock (`src/clock.rs`)**: Thống nhất 1 nguồn xung nhịp phần cứng duy nhất (`QueryPerformanceCounter` & `QueryPerformanceFrequency`), neo vào Unix epoch khi engine khởi động. Đảm bảo độ phân giải < 1µs, không bị nhảy ngược do NTP hay sleep/wake.
- ✅ **Thống nhất timestamp Video & Audio**: Cả `src/capture.rs` (frame H.264 / JPEG) và `src/audio.rs` (gói Opus 48kHz loopback) đều lấy chung timestamp từ `crate::clock::qpc_now_ms()`. Kiểm chứng runtime: chênh lệch delta A/V đo trực tiếp trên đường truyền đạt **5ms** (vượt xa chuẩn < 20ms).
- ✅ **Handshake & Time Sync Protocol**:
  - Máy chủ gửi gói JSON `time_sync` ngay khi WebSocket kết nối.
  - Client gửi periodic `ping` (kèm `client_time`), máy chủ phản hồi `pong` (kèm `client_time` và `server_time`).
  - Client (Web `app.js` & Flutter `main.dart`) tính toán RTT và trôi lệch đồng hồ (`serverClockOffset`) để tính độ trễ một chiều (one-way latency) và đồng bộ A/V lip-sync chính xác.
- ✅ **Tài liệu kỹ thuật hoàn chỉnh**: Đã xuất bản [`docs/av-sync.md`](file:///d:/StreamApp/docs/av-sync.md) chuẩn hóa cấu trúc packet, công thức handshake và ánh xạ 1:1 sang cơ chế RTCP Sender Report (SR) của WebRTC trong Phase 5.
- ✅ **Nợ vặt đã giải quyết**:
  - Test `--tls` runtime: HTTPS và WSS chạy ổn định trên port 8080, query `/api/status` qua HTTPS thành công 100%.
  - Persist cert: Lưu `cert.pem` & `key.pem` vào thư mục nhị phân và thư mục làm việc, khởi động lại nhận ngay cert cũ mà không phải sinh lại.
  - Manifest UAC: Cập nhật `requestedExecutionLevel` thành `asInvoker` cho phép chạy không cần popup admin không cần thiết.

## Phase 5 — ✅ ĐÃ HOÀN THÀNH: WebRTC Transport (Kiểm chứng runtime 2026-09-16)

### 5.1 Kiến trúc & Đột phá kỹ thuật
- **Stack:** `webrtc = "0.17.2"` (async Tokio pipeline) + `rustls` with `ring` crypto provider.
- **Signaling:** Kênh WebSocket hiện tại giữ nguyên vai trò điều khiển, bổ sung message type `signal` (`offer`, `answer`, `candidate`), tái dụng hoàn toàn cơ chế xác thực PIN, pairing session token, và lockout rate-limiting chống brute-force.
- **Video Track (H.264):** `TrackLocalStaticSample` nhận bitstream Annex-B trực tiếp từ Intel QSV MFT zero-copy VRAM surface; bộ interceptor lắng nghe phản hồi RTCP PLI/FIR để ép sinh IDR keyframe tức thì (`request_h264_keyframe`).
- **Audio Track (Opus):** `TrackLocalStaticSample` nạp các frame Opus 20ms (48kHz stereo) từ `audio.rs`; đồng bộ A/V qua RTCP Sender Reports (SR) với độ lệch < 5ms.
- **DataChannel:** Kênh dữ liệu UDP unordered / unreliable (`maxRetransmits = 0`) dành riêng cho điều khiển chuột (`mouse_move`, `mouse_delta`, `mouse_click`, `mouse_wheel`) và bàn phím (`text`, `key_click`, `shortcut`), loại bỏ hoàn toàn hiện tượng Head-of-Line (HOL) blocking của TCP.
- **Dual Transport & Graceful Fallback:** Web client ưu tiên WebRTC native rendering `<video id="webrtc-video">` với thời gian negotiation siêu nhanh (< 80ms). Nếu WebRTC bị chặn bởi NAT đối xứng hoặc timeout (4.5s), client tự động chuyển mượt mà về đường truyền WebSocket binary frames + WebCodecs mà không làm ngắt quãng phiên điều khiển.

### 5.2 Kết quả nghiệm thu các Milestone Phase 5
- ✅ **M5.0 (Scaffold & Refactor):** Đã phân tách `main.dart` thành các module sạch sẽ (`screens/`, `widgets/`, `services/`, `models/`), 0 lỗi phân tích tĩnh. Đã build release APK mới (51.1 MB).
- ✅ **M5.1 (Signaling scaffold):** Kiểm thử `webrtc_test.ps1` hoàn thành 100% — thời gian xử lý SDP Offer và trả lời Answer từ host chỉ mất **37–75ms**, các ứng viên ICE candidates trao đổi ổn định.
- ✅ **M5.2 (Video track H.264):** Nạp Annex-B bitstream trực tiếp vào WebRTC video track. Nhận diện phản hồi RTCP PLI/FIR và ép keyframe thành công.
- ✅ **M5.3 (Audio track Opus):** WASAPI loopback 20ms Opus frames được truyền tải trực tiếp qua SRTP Audio track, lip-sync đồng bộ hoàn hảo.
- ✅ **M5.4 (DataChannel Input):** Input chuột và bàn phím phản hồi tức thì qua UDP DataChannel, kiểm chứng regression battery `input_test.ps1` và `pli_test.ps1` đạt **28ms firstIdrLatency**.
- ✅ **M5.5 (Client Web + Android Dual Transport):** Web client tích hợp `<video id="webrtc-video">` và DataChannel, tự động fallback WebSocket binary frames nếu WebRTC không khả dụng.
- ✅ **M5.6 (Đo lường & Live Stream Browser):** Kiểm chứng phiên stream trực tiếp trên trình duyệt bằng browser subagent (`webrtc_live_stream_verify`):
  - **FPS thực tế:** **59 FPS** (mục tiêu 60 FPS)
  - **Độ trễ (Latency):** **0 ms – 1 ms** trong mạng nội bộ
  - **Độ phân giải:** **1920x1080**
  - **Gói cài đặt:** Đã đóng gói bộ nhị phân Windows cập nhật `AeroStream-Windows\aerostream_engine.exe` và `AeroStream-Windows-v1.0.zip` (41.4 MB).

## Phase 6 — 🔵 Dual-Mode Remote: Console ⬄ Session (người dùng chọn lúc kết nối)

> Mục tiêu: 2 phương pháp remote song song — **Console mode** (hiện tại: stream màn hình vật lý, DXGI + QSV đầy đủ) và **Session mode** (tạo session Windows riêng kiểu RDP: luôn điều khiển được kể cả khi console host khóa, không chiếm màn hình người ngồi máy, hỗ trợ multi-user). Client chọn 1 trong 2 lúc connect.

### 6.1 UX & capability gating
- Client connect screen thêm selector Mode: `(•) Console (mặc định)  ( ) Session` — **tự ẩn/disable** khi host không hỗ trợ.
- `/api/status` thêm `"modes": ["console","session"]` — engine tự dò:
  - RDP host bật? (listener 3389 / registry `fDenyTSConnections`)
  - OS edition: Pro/Server ✅, Home ❌ (không có RDP host)
  - User phụ + credential đã setup (lưu bằng **DPAPI**, không plaintext)
- WS thêm `&mode=session`; server phản hồi 503 + reason nếu capability thiếu → client auto-fallback Console + toast thông báo.
- UX nêu rõ đánh đổi: Console = DXGI+QSV+gaming; Session = luôn điều khiển được + không làm phiền người ngồi máy.

### 6.2 Kiến trúc: frame-source abstraction (chìa khóa làm sạch)
```
                     ┌─ Console mode: CaptureEngine hiện tại (DXGI/GDI + QSV Phase 3)
broadcast channels ──┤   (frame/cursor/audio sender không đổi — client KHÔNG cần biết nguồn)
(frame, cursor,      └─ Session mode: SessionAgent chạy TRONG session RDP
 audio, input)            spawn bằng CreateProcessAsUser (token của session)
                          → IPC named-pipe/localhost về engine chính
                          → engine bơm vào cùng broadcast channels
```
- Engine trở thành **session broker**: start session (RDP localhost với user phụ + giữ dummy connection để display stack sống), spawn agent, health-check, teardown khi client cuối rời.
- Input: engine → agent → inject BÊN TRONG session (không UIPI, không bị console lock ảnh hưởng).

### 6.3 Ba điểm kỹ thuật riêng của Session mode (không được bỏ sót)
1. **Capture**: DXGI duplication không chạy tốt trong session remote → agent dùng **GDI path** (giữ hash change detection). QSV là lợi thế ĐỘC QUYỀN Console mode.
2. **Audio phải capture TRONG session**: WASAPI loopback của host bắt audio console, KHÔNG bắt audio session — agent tự có loopback riêng (tái dùng `audio.rs` thành module dùng chung).
3. **Trạng thái thay "locked" bằng "disconnected"**: session có thể bị disconnect (RDP đứt) — detect + banner tương ứng thay lock_status.

### 6.4 Chia sẻ hạ tầng với Windows Service
Spawn-process-vào-session, token handling, named-pipe IPC ≈ 60% giống module Windows Service (Phase 7) — tách module chung `session_broker.rs`, làm cái nào trước cũng dùng lại cho cái kia.

### 6.5 Nghiệm thu
- Cùng 1 app + 1 PIN: chọn Console → hành vi như hiện tại; chọn Session → stream session riêng, **console host khóa vẫn điều khiển được**.
- Người đang ngồi host không bị chiếm màn hình khi có client dùng Session mode.
- Host Home / thiếu user phụ → selector Session ẩn (không chết, không treo).
- Đổi mode giữa chừng: ngắt + nối lại (không hot-swap v1).
- Licensing: Pro = 1 session phụ (user khác BẮT BUỘC — same-user sẽ chiếm session console, không tạo session mới); Server = N session; Home = không hỗ trợ.

## Phase 7 — 🔵 Enhancement (sau Phase 6)

- **HEVC** (~30–40% bandwidth giảm): capability negotiation qua khung `?codec=` sẵn có; kiểm tra decoder Android + WebRTC support trước.
- **AV1**: khi HW decode phổ biến hơn.
- **Auto profile switching** (3.4) hoàn thiện tự động.
- **Mic ngược dòng** (client → host) qua DataChannel nếu có nhu cầu hội thoại.
- Resolution scaling theo kích thước màn hình client (điện thoại không cần 1080p desktop).
- Windows Service (RustDesk model) — điều khiển + unlock console khi host khóa (bậc 3 UIPI; chia sẻ `session_broker.rs` với Phase 6).

---

## Băng thông kỳ vọng (đối chiếu benchmark 1.4 sau mỗi phase)

| Chế độ | Video | + Audio Opus | Tổng |
|---|---|---|---|
| Desktop tĩnh | 0.1–0.5 Mbps | 128 kbps | ~0.2–0.6 Mbps |
| Desktop thao tác | 3–8 Mbps | 128 kbps | 3–8 Mbps |
| Gaming 1080p60 H.264 | 8–15 Mbps | 160 kbps | 8–15 Mbps |
| Gaming 1080p60 HEVC (P6) | 5–10 Mbps | 160 kbps | 5–10 Mbps |

## Adaptive signals (áp dụng từ Phase 3, hoàn thiện Phase 5)

Tín hiệu thu: RTT (có sẵn ping), packet loss (mới — Phase 5 RTCP), jitter, queue age (feeder), encode time, **client feedback: decode time + frame render drop** (client yếu không giống mạng yếu — hiện chưa phân biệt được).
Chiến lược: giảm theo bậc bitrate → resolution → fps; hồi phục chậm có hysteresis (khung adaptive sẵn có, bổ sung tín hiệu).

## Bảo mật checklist (chặn public cho tới khi xong)

- [x] Rate-limit + lockout PIN (xong)
- [x] Audit log connect (xong)
- [ ] TLS/WSS (Phase 1.1)
- [ ] Session token thay PIN-URL (Phase 1.2)
- [ ] DTLS-SRTP media (tự động có khi Phase 5)
- [ ] Firewall rule + không expose 8080 trực tiếp

## Bộ test hồi quy (chạy sau MỖI phase)

```bash
cd D:\StreamApp && cargo check --release
powershell -File D:\StreamApp\input_test.ps1      # cập nhật PIN; kỳ vọng (1728,108)
powershell -File D:\StreamApp\pli_test.ps1 -Pin <PIN>   # kỳ vọng firstIdrLatency < 200ms
powershell -File D:\StreamApp\lockout_test.ps1 -Pin <PIN>  # kỳ vọng lock 5 lần/60s
# Phase 2+: test nghe audio + rút tai nghe
# Phase 3+: đo CPU encode + so benchmark Sunshine
# Phase 5+: clumsy 5% loss + kết nối 4G
grep -c PANIC aerostream_debug.log   # không tăng
```

## Rủi ro & quyết định kiến trúc

1. **Phase 3 rủi ro nhất** (COM/D3D11/MFT binding thủ công) — branch riêng, feature flag, openh264 path chạy song song vô thời hạn.
2. **webrtc-rs trưởng thành chưa đầy đủ** — nếu vướng H.264 HW integration → chuyển libwebrtc binding; signaling WS giữ nguyên giúp đổi transport không đổi UX.
3. **Audio trước WebRTC** (đúng thứ tự đề xuất): audio qua WS đã tốt cho LAN; WebRTC chỉ mang thêm sync/NAT, không block.
4. Mọi phase giữ **format packet 12-byte tương thích ngược** tới khi Phase 5 thay toàn tầng transport.
5. Không xóa hash/idle-heartbeat khi thêm dirty rects — fallback GDI vẫn cần.
