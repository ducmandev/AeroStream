# AeroStream // Ultra Low-Latency Remote Desktop & Audio Streaming

<p align="center">
  <img src="https://img.shields.io/badge/Platform-Windows%2010%20%7C%2011%20%28x64%29-0078D6?logo=windows&logoColor=white" alt="Windows" />
  <img src="https://img.shields.io/badge/Client-Android%20%7C%20Web%20%7C%20Windows-3DDC84?logo=android&logoColor=white" alt="Clients" />
  <img src="https://img.shields.io/badge/Backend-Rust%20%28Tokio%20%2B%20Axum%29-DEA584?logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/GUI-Flutter%203.22%2B-02569B?logo=flutter&logoColor=white" alt="Flutter" />
  <img src="https://img.shields.io/badge/Video-DXGI%20%2B%20Intel%20QSV%20H.264-0071C5?logo=intel&logoColor=white" alt="Intel QSV" />
  <img src="https://img.shields.io/badge/Audio-WASAPI%20%2B%20Opus%2048kHz-blue" alt="Opus Audio" />
  <img src="https://img.shields.io/badge/Latency-%3C%2015ms%20%28LAN%29-brightgreen" alt="Latency" />
</p>

---

## 🌟 Giới thiệu tổng quan / Overview

**AeroStream** là giải pháp điều khiển máy tính từ xa và truyền phát màn hình độ trễ siêu thấp (< 15ms trong mạng LAN) được xây dựng dành cho **Windows 10/11**. Dự án kết hợp sức mạnh phần cứng của **Rust** ở tầng backend (DXGI Desktop Duplication, Media Foundation Intel Quick Sync H.264, WASAPI Loopback Audio) và sự mượt mà của **Flutter** ở tầng giao diện điều khiển (Windows Desktop & Android Mobile) cùng client **Web HTML5/WebCodecs** chạy trực tiếp trên trình duyệt không cần cài đặt.

---

## ✨ Tính năng nổi bật / Key Features

### 1. 🖥️ Hình ảnh & Hiệu năng đỉnh cao (Ultra-Fast Video Pipeline)
- **DirectX Graphics Duplication (DXGI)**: Chụp màn hình trực tiếp từ bộ nhớ VRAM của GPU, tự động fallback về GDI32 khi chuyển đổi desktop bảo mật.
- **Hardware Acceleration (Intel QSV / Media Foundation)**: Mã hóa H.264 phần cứng bằng Intel Quick Sync Video (QSV MFT), độ trễ encode chỉ **6.3ms – 7.4ms**, giảm thiểu tối đa mức tiêu thụ CPU.
- **Picture Loss Indication (PLI) Resync**: Tự động phục hồi IDR Keyframe ngay lập tức khi mạng lag/mất gói chỉ trong vòng **20ms**.
- **Chế độ đa dạng**: Hỗ trợ độ phân giải linh hoạt (1080p, 720p, 540p), điều chỉnh FPS (30fps / 60fps) và chất lượng hình ảnh theo băng thông mạng.

### 2. 🔊 Âm thanh thực tế thời gian thực (Real-time Audio Streaming)
- **WASAPI Loopback Capture**: Thu trực tiếp toàn bộ âm thanh hệ thống (nhạc, game, video, tiếng thông báo) theo cơ chế event-driven, không tốn CPU polling.
- **Opus 48kHz Stereo**: Nén âm thanh chuẩn studio 128 kbps qua Opus codec, độ trễ âm thanh đồng bộ hoàn hảo với video.
- **Drop-on-Lag Guard**: Cơ chế tự động xả hàng đợi âm thanh khi phát hiện độ trễ mạng, tránh hiện tượng dồn buffer gây chậm tiếng.

### 3. 📱 Trải nghiệm điều khiển di động thế hệ mới (Dynamic Island Mobile UI)
- **Dynamic Island HUD**: Thanh điều khiển nổi phong cách hiện đại, tự động thu nhỏ thành viên thuốc nhỏ gọn, hỗ trợ chạm hoặc vuốt nhẹ từ cạnh màn hình để mở rộng.
- **Touchpad ảo cực mượt**: Hỗ trợ tùy chỉnh tốc độ chuột từ **1 đến 100**, con trỏ chuột thanh mảnh chuẩn Windows, hỗ trợ click trái, click phải, cuộn trang (scroll), kéo thả (drag & drop).
- **Phím tắt Windows nhanh**: Tích hợp thanh phím tắt ngang (⊞ Win, Esc, Alt+Tab, Task Manager, Ctrl+C, Ctrl+V, Ctrl+Z, Win+D, Enter, Tab, Del, 4 phím mũi tên).
- **Hỗ trợ gõ tiếng Việt (IME)**: Nút chuyển đổi IME độc lập giúp nhập liệu bàn phím tiếng Việt có dấu chính xác trên các ứng dụng máy tính.
- **Zoom & Pan đa điểm**: Dùng 2 ngón tay để phóng to/thu nhỏ chi tiết màn hình hoặc di chuyển linh hoạt.

### 4. 🌐 Web Client không cần cài đặt (Zero-Install HTML5 Client)
- Máy chủ tích hợp sẵn Web Server phục vụ client HTML5/JavaScript hiện đại tại cổng `8080`.
- Giải mã phần cứng ngay trên trình duyệt bằng **WebCodecs API** (`VideoDecoder` H.264 & `AudioDecoder` Opus) kết hợp HTML5 Canvas và Web Audio Context.
- Tương thích tốt với mọi thiết bị: iPhone, iPad, MacBook, điện thoại Android khác hoặc máy tính phụ.

### 5. 🛡️ Bảo mật & Tin cậy (Security & Reliability)
- **Mã PIN 6 số ngẫu nhiên**: Mỗi phiên chạy tự động tạo mã PIN bảo mật ngẫu nhiên.
- **Chống Brute-force**: Khóa tự động kết nối sau 5 lần nhập sai PIN trong 60 giây và ghi log bảo mật (Audit Log).
- **Nhận diện quyền hạn Windows (UIPI Awareness)**: Tự động phát hiện phiên chạy quyền Administrator để điều khiển các ứng dụng bảo mật cao như Task Manager, cửa sổ UAC.

---

## 📦 Bộ cài đặt Windows cho các máy khác / Portable Windows Distribution

Dự án đã được đóng gói thành bộ cài đặt độc lập (Portable Standalone Package) trong thư mục **`AeroStream-Windows/`** và file nén **`AeroStream-Windows-v1.0.zip`**.

### Cấu trúc bộ cài đặt:
```
AeroStream-Windows/
├── AeroStream.exe               # Ứng dụng Dashboard giao diện Windows Fluent
├── aerostream_engine.exe        # Động cơ máy chủ Rust hiệu năng cao (DXGI + QSV + WASAPI)
├── aerostream.exe               # Alias cho engine máy chủ
├── flutter_windows.dll          # Thư viện đồ họa Flutter Windows
├── data/                        # Tài nguyên giao diện, font, shaders
├── AeroStream-Android.apk       # File cài đặt app Android cho điện thoại
├── Start-AeroStream.bat         # Script khởi chạy nhanh 1-click
├── Install-FirewallRule.bat     # Script tự động mở port 8080 trên Windows Defender Firewall
├── Uninstall-FirewallRule.bat   # Script gỡ bỏ rule tường lửa nếu cần
└── HUONG_DAN_SU_DUNG.txt        # Hướng dẫn chi tiết bằng tiếng Việt
```

---

## 🚀 Hướng dẫn chạy trên máy tính Windows khác (Quick Start)

### Bước 1: Tải và giải nén
1. Sao chép file **`AeroStream-Windows-v1.0.zip`** sang máy tính cần điều khiển.
2. Giải nén vào một thư mục bất kỳ (ví dụ: `C:\AeroStream` hoặc ngoài `Desktop`).

### Bước 2: Cấu hình tường lửa (Chỉ làm 1 lần)
1. Nhấp chuột phải vào file **`Install-FirewallRule.bat`** chọn **"Run as administrator"** (*Chạy với quyền quản trị viên*).
2. Script sẽ tự động thêm quy tắc cho phép cổng **8080 TCP** trên Windows Defender Firewall để các thiết bị trong mạng LAN kết nối được.

### Bước 3: Khởi chạy AeroStream
1. Nhấp đúp vào file **`AeroStream.exe`** (hoặc chạy **`Start-AeroStream.bat`**).
2. Cửa sổ giao diện **AeroStream Remote Desktop** sẽ hiển thị:
   - **Địa chỉ IP nội bộ**: Ví dụ `192.168.1.15`
   - **Port**: `8080`
   - **Mã PIN bảo mật**: 6 chữ số
   - **Mã QR Code**: Để quét kết nối tức thì bằng điện thoại
   - **Trạng thái máy chủ**: Online, số FPS hiện tại, số client đang kết nối.

---

## 📲 Hướng dẫn kết nối từ Client

### Cách 1: Kết nối bằng App Android (Khuyên dùng)
1. Chép file **`AeroStream-Android.apk`** từ thư mục sang điện thoại Android và tiến hành cài đặt.
2. Mở app **AeroStream** trên điện thoại:
   - Nhấn nút **"Quét mã QR"** để quét mã hiển thị trên màn hình máy tính, **HOẶC**
   - Nhập trực tiếp địa chỉ IP và mã PIN rồi nhấn **"Kết nối"**.
3. Sau khi kết nối, màn hình máy tính sẽ hiển thị mượt mà trên điện thoại cùng âm thanh thực tế.

### Cách 2: Kết nối bằng Trình duyệt Web (iPhone / iPad / Mac / PC khác)
1. Đảm bảo thiết bị đang kết nối cùng mạng Wi-Fi với máy tính chạy AeroStream.
2. Mở trình duyệt bất kỳ (Chrome, Safari, Edge) và truy cập đường dẫn:
   ```text
   http://<IP-MÁY-TÍNH>:8080/?pin=<MÃ-PIN>
   ```
   *(Ví dụ: `http://192.168.1.15:8080/?pin=123456`)*
3. Trình duyệt sẽ phát trực tiếp màn hình 60 FPS có âm thanh và hỗ trợ chuột, phím đầy đủ.

---

## 🏗 Kiến trúc hệ thống / Architecture Overview

```mermaid
graph TD
    subgraph Host["Windows Host (AeroStream Engine)"]
        A[Desktop Display] -->|DXGI OutputDuplication| B[Capture Engine]
        B -->|BGRA to NV12| C[Intel QSV MFT / OpenH264]
        C -->|Annex-B H.264 NALs| D[WebSocket Video Streamer]
        
        E[WASAPI System Audio] -->|Audio Loopback PCM| F[Opus Encoder 48kHz]
        F -->|Opus Packets 0xFA| D
        
        G[Input Injection Worker] -->|SendInput Win32| A
        D -->|Remote Mouse/Key/IME| G
    end

    subgraph Clients["AeroStream Clients"]
        H["Flutter Windows App (AeroStream.exe)"]
        I["Flutter Android App (AeroStream-Android.apk)"]
        J["HTML5 / WebCodecs Browser Client"]
    end

    D <===>|Port 8080 TCP (WS/HTTP)| H
    D <===>|Port 8080 TCP (WS/HTTP)| I
    D <===>|Port 8080 TCP (WS/HTTP)| J
```

---

## 🛠 Hướng dẫn Build từ mã nguồn / Building from Source

### Yêu cầu môi trường (Prerequisites):
- **Hệ điều hành**: Windows 10 hoặc Windows 11 (64-bit)
- **Rust toolchain**: Phiên bản stable mới nhất (`rustup default stable`)
- **Flutter SDK**: Phiên bản 3.22 trở lên
- **Visual Studio 2022**: Cài đặt gói *"Desktop development with C++"* (MSVC, Windows 10/11 SDK, CMake)
- **Java JDK 17 & Android SDK**: (Nếu muốn biên dịch APK Android)

### 1. Build động cơ máy chủ Rust (Release)
```powershell
cargo build --release --bin aerostream
```
File thực thi tạo ra tại: `target\release\aerostream.exe`

### 2. Build ứng dụng Flutter Windows Dashboard
```powershell
cd android_app
flutter build windows --release
```
File thực thi tạo ra tại: `android_app\build\windows\x64\runner\Release\android_app.exe`

### 3. Build ứng dụng Android APK
```powershell
cd android_app
flutter build apk --release
```
File APK tạo ra tại: `android_app\build\app\outputs\flutter-apk\app-release.apk`

### 4. Build tự động toàn bộ gói phân phối (1-Click Build All)
Chạy script PowerShell tự động hóa toàn bộ các bước trên và nén file ZIP:
```powershell
powershell -ExecutionPolicy Bypass -File scripts\build_bundle.ps1
```
Script sẽ tự động kiểm tra biên dịch, thu thập các file cần thiết vào thư mục `AeroStream-Windows` và xuất file nén `AeroStream-Windows-v1.0.zip`.

---

## ⚙️ Cấu hình nâng cao / Advanced Configuration

| Tham số / Thiết lập | Mặc định | Mô tả |
|---|---|---|
| **Cổng mạng (Port)** | `8080` | Cổng dịch vụ HTTP Server và WebSocket |
| **Mã hóa video** | `Intel QSV H.264` | Tự động chọn encoder phần cứng MFT tốt nhất, fallback OpenH264 / JPEG |
| **Tốc độ khung hình (FPS)** | `60` | Hỗ trợ tùy chỉnh 30 FPS / 60 FPS qua giao diện điều khiển |
| **Tần số âm thanh** | `48000 Hz Stereo` | Chuẩn âm thanh thời gian thực Opus 128 kbps |
| **Chế độ mã hóa bảo mật** | `--tls` | Chạy máy chủ qua HTTPS/WSS với chứng chỉ SSL tự ký |

---

## ❓ Câu hỏi thường gặp & Khắc phục sự cố / Troubleshooting

### 1. Điện thoại báo lỗi không thể kết nối tới máy tính?
- Đảm bảo điện thoại và máy tính đang cùng kết nối vào một mạng Wi-Fi hoặc cùng lớp mạng LAN.
- Chạy script `Install-FirewallRule.bat` bằng quyền Administrator trên máy tính.
- Kiểm tra xem phần mềm diệt virus của bên thứ ba (Kaspersky, Avast, v.v.) có đang chặn cổng 8080 hay không.

### 2. Không nghe thấy âm thanh trên điện thoại hoặc trình duyệt?
- Đảm bảo máy tính đang phát âm thanh ra thiết bị phát mặc định (Speakers / Headphones).
- Kiểm tra nút Loa trên thanh Dynamic Island ở giao diện client xem có đang ở chế độ Mute hay không.

### 3. Chuột di chuyển trên điện thoại quá nhanh hoặc quá chậm?
- Nhấn vào biểu tượng Cài đặt (bánh răng) trên thanh Dynamic Island.
- Kéo thanh trượt **Tốc độ chuột (Mouse Speed)** từ giá trị 1 đến 100 để đạt độ nhạy ưng ý nhất.

### 4. Không click được vào Task Manager hoặc bảng UAC?
- Chạy `AeroStream.exe` (hoặc `aerostream_engine.exe`) bằng quyền **Administrator** (`Run as administrator`) để hệ thống Windows cho phép mô phỏng thao tác chuột trên các cửa sổ có mức đặc quyền bảo mật cao (UIPI).

---

## 📄 Bản quyền / License
Dự án được phát hành phục vụ mục đích nghiên cứu, phát triển công nghệ truyền phát màn hình độ trễ thấp và sử dụng nội bộ.
Mọi đóng góp và báo cáo lỗi xin vui lòng tạo Issue hoặc Pull Request trên kho mã nguồn.
