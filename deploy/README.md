# AeroStream Deployment Package

Thư mục chứa toàn bộ các bản build mới nhất sẵn sàng chạy cho AeroStream (Host Windows & Client Android).

---

## 📱 1. Client Android (`AeroStream-Android.apk`)
- **File:** `AeroStream-Android.apk` (~49 MB)
- **Cài đặt:**
  - Cách 1: Copy file `AeroStream-Android.apk` vào điện thoại Android và mở file để cài đặt trực tiếp.
  - Cách 2: Bật Host máy tính, mở trình duyệt trên điện thoại truy cập `http://<IP_MÁY_TÍNH>:8080/app.apk` để tải file APK về cài đặt.
- **Kết nối:**
  - Mở ứng dụng AeroStream trên Android.
  - Nhập IP máy tính (ví dụ `10.97.36.227:8080`) và mã PIN hiển thị trên màn hình Host.

---

## 🖥️ 2. Host Windows (`aerostream.exe`)
- **File:** `aerostream.exe` + `aerostream.exe.manifest`
- **Khởi chạy máy chủ:**
  - Nhấp đúp chuột vào **`Start-AeroStream.bat`** (sẽ tự động xin quyền Administrator).
  - Máy chủ sẽ lắng nghe tại cổng `8080` (HTTP/HTTPS) và bắt đầu phát hình màn hình qua WebRTC.
- **Mở khóa Secure Desktop (Màn hình khóa Windows Lock / UAC):**
  - Chạy **`Spawn-SecureAgent.bat`** (quyền Admin) để kích hoạt agent quyền `NT AUTHORITY\SYSTEM` chạy ngầm.
  - Agent này cho phép chụp ảnh màn hình khóa Winlogon và nhập mật khẩu mở khóa từ thiết bị điều khiển từ xa.
- **Tường lửa Windows:**
  - Nếu điện thoại không kết nối được qua mạng LAN, nhấp đúp vào **`Install-FirewallRule.bat`** để mở cổng 8080 trong Windows Firewall.

---

## 📂 3. Danh mục tệp tin trong `deploy/`
| Tên tệp | Vai trò |
| :--- | :--- |
| `AeroStream-Android.apk` | Ứng dụng Android mới nhất (Flutter Release) |
| `aerostream.exe` | Động cơ Host + Secure Agent mới nhất (Rust Release) |
| `aerostream.exe.manifest` | File manifest yêu cầu quyền Administrator/UIPI |
| `Start-AeroStream.bat` | Script 1-click khởi động Host Server |
| `Spawn-SecureAgent.bat` | Script 1-click khởi chạy SYSTEM Secure Agent |
| `Install-FirewallRule.bat` | Mở cổng tường lửa 8080 cho kết nối mạng LAN |
| `cert.pem` / `key.pem` | Chứng chỉ bảo mật TLS persistent cho HTTPS / WebRTC |
| `Setup-SessionUser.bat` | Thiết lập tài khoản phụ cho Session Mode (tùy chọn) |
