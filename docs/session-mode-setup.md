# Hướng dẫn Thiết Lập Session Mode (Isolated Windows Desktop)

## 1. Giới thiệu
Session Mode là tính năng độc quyền trên AeroStream cho phép tạo một phiên làm việc Windows độc lập (Multi-Session) bên cạnh phiên làm việc vật lý hiện tại của máy tính:
- **Không chiếm màn hình:** Người ngồi tại máy tính vật lý vẫn sử dụng bình thường, chuột và phím của phiên từ xa không can thiệp vào phiên Console.
- **Không bị khóa khi màn hình Console khóa:** Khi người ngồi máy bấm `Win + L`, phiên từ xa vẫn hoạt động 100% không bị ngắt quãng.

---

## 2. Yêu cầu hệ thống
1. **Hệ điều hành:** Windows 10/11 Pro, Windows 10/11 Enterprise, hoặc Windows Server. *(Windows Home không hỗ trợ tính năng RDP Host)*.
2. **Dịch vụ Remote Desktop (TermService):** Phải được kích hoạt (`fDenyTSConnections = 0`).
3. **Tài khoản người dùng phụ:** Bắt buộc phải có một tài khoản phụ thuộc nhóm `Remote Desktop Users` và **không được trùng** với tài khoản đang đăng nhập màn hình vật lý.

---

## 3. Cách thiết lập nhanh (Tự động)
1. Mở thư mục chứa AeroStream (ví dụ `AeroStream-Windows`).
2. Nhấp đúp vào tệp **`Setup-SessionUser.bat`** (hoặc nhấp chuột phải chọn **Run as administrator**).
3. Hộp thoại Windows UAC sẽ yêu cầu xác nhận quyền Quản trị viên -> bấm **Yes**.
4. Kịch bản sẽ tự động:
   - Tạo tài khoản phụ `aerostream_remote` với mật khẩu an toàn `AeroStream#2026`.
   - Cấp quyền thành viên nhóm `Remote Desktop Users`.
   - Bật dịch vụ Remote Desktop và mở cổng 3389 trên Windows Defender Firewall.

---

## 4. Cách thiết lập thủ công (Qua PowerShell / CMD Administrator)
Nếu muốn tự cấu hình hoặc sử dụng tài khoản tùy chỉnh:
```powershell
# 1. Tạo tài khoản phụ
net user aerostream_remote <MatKhauCuaBan> /add /comment:"AeroStream Session Mode User" /passwordchg:no

# 2. Thêm vào nhóm Remote Desktop Users
net localgroup "Remote Desktop Users" aerostream_remote /add

# 3. Kích hoạt Remote Desktop trên Windows
reg add "HKLM\SYSTEM\CurrentControlSet\Control\Terminal Server" /v fDenyTSConnections /t REG_DWORD /d 0 /f
sc config TermService start= auto
net start TermService

# 4. Mở cổng Firewall 3389
netsh advfirewall firewall add rule name="AeroStream Remote Desktop (Port 3389)" dir=in action=allow protocol=TCP localport=3389 profile=any
```

---

## 5. Lưu thông tin xác thực vào AeroStream Engine
Sau khi tài khoản phụ đã sẵn sàng, thông tin tài khoản được lưu an toàn qua cơ chế mã hóa Windows DPAPI bằng cách gọi API:
```bash
POST http://127.0.0.1:8080/api/session/config
Content-Type: application/json

{
  "username": "aerostream_remote",
  "password": "AeroStream#2026"
}
```
Thông tin mật khẩu sẽ được mã hóa bằng DPAPI và ghi vào tệp nhị phân `data/aerostream-session-creds.bin`. Mật khẩu tuyệt đối không được ghi dạng plaintext hay xuất ra tệp nhật ký (log).
