import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../models/recent_connection.dart';
import 'remote_desktop_screen.dart';

/// ====================================================================
/// AEROSTREAM MOBILE CONNECT SCREEN (ANDROID CLIENT)
/// Full-Featured Dual-Mode UI (Console 60fps & Isolated Session RDP)
/// With Dynamic Capability Probing, In-App Credentials Setup & LAN Scan
/// ====================================================================
class MobileConnectScreen extends StatefulWidget {
  const MobileConnectScreen({super.key});

  @override
  State<MobileConnectScreen> createState() => _MobileConnectScreenState();
}

class _MobileConnectScreenState extends State<MobileConnectScreen> {
  final _ipController = TextEditingController(text: '10.97.36.227');
  final _portController = TextEditingController(text: '8080');
  final _pinController = TextEditingController();

  bool _isConnecting = false;
  String _selectedMode = 'console';
  String? _errorMessage;
  List<RecentConnection> _recentConnections = [];

  // Dynamic host capability probing state
  bool _isProbing = false;
  bool _isHostOnline = false;
  int? _hostLatencyMs;
  String? _hostVersion;
  String? _hostEdition;
  List<String> _hostSupportedModes = ['console'];
  Map<String, dynamic>? _sessionCapability;
  Timer? _probeDebounceTimer;

  @override
  void initState() {
    super.initState();
    _loadRecentConnections();

    // Auto-probe initial host on launch
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _probeHostCapabilities();
    });

    // Debounced probing on IP or Port changes
    _ipController.addListener(_onHostInputChanged);
    _portController.addListener(_onHostInputChanged);
  }

  @override
  void dispose() {
    _probeDebounceTimer?.cancel();
    _ipController.removeListener(_onHostInputChanged);
    _portController.removeListener(_onHostInputChanged);
    _ipController.dispose();
    _portController.dispose();
    _pinController.dispose();
    super.dispose();
  }

  void _onHostInputChanged() {
    _probeDebounceTimer?.cancel();
    _probeDebounceTimer = Timer(const Duration(milliseconds: 600), () {
      if (mounted) _probeHostCapabilities();
    });
  }

  // Probe host capabilities via GET /api/status using native HttpClient
  Future<void> _probeHostCapabilities({String? targetIp, String? targetPort}) async {
    final ip = (targetIp ?? _ipController.text).trim();
    final port = (targetPort ?? _portController.text).trim();
    if (ip.isEmpty) return;

    if (mounted) {
      setState(() {
        _isProbing = true;
      });
    }

    final stopwatch = Stopwatch()..start();
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 2);
    try {
      final p = int.tryParse(port) ?? 8080;
      final request = await client.getUrl(Uri.parse('http://$ip:$p/api/status'));
      final response = await request.close().timeout(const Duration(seconds: 2));
      stopwatch.stop();

      if (response.statusCode == 200) {
        final body = await response.transform(utf8.decoder).join();
        final data = jsonDecode(body) as Map<String, dynamic>;
        final modes = (data['modes'] as List<dynamic>?)?.map((e) => e.toString()).toList() ?? ['console'];
        final cap = data['session_capability'] as Map<String, dynamic>?;
        final edition = cap?['edition'] as String? ?? (data['edition'] as String?);

        if (mounted) {
          setState(() {
            _isProbing = false;
            _isHostOnline = true;
            _hostLatencyMs = stopwatch.elapsedMilliseconds;
            _hostVersion = data['version'] as String? ?? '0.6.0';
            _hostEdition = edition ?? 'Windows';
            _hostSupportedModes = modes;
            _sessionCapability = cap;
          });
        }
        return;
      }
    } catch (_) {
      // Host unreachable or timeout
    } finally {
      client.close(force: true);
    }

    if (mounted) {
      setState(() {
        _isProbing = false;
        _isHostOnline = false;
        _hostLatencyMs = null;
        _hostVersion = null;
        _hostEdition = null;
        _hostSupportedModes = ['console'];
        _sessionCapability = null;
      });
    }
  }

  Future<File> _getStorageFile() async {
    try {
      if (Platform.isAndroid) {
        final dir = Directory('/data/user/0/com.aerostream.android_app/app_flutter');
        if (!await dir.exists()) await dir.create(recursive: true);
        return File('${dir.path}/recent_devices.json');
      }
    } catch (_) {}
    return File('recent_devices.json');
  }

  Future<void> _loadRecentConnections() async {
    try {
      final file = await _getStorageFile();
      if (await file.exists()) {
        final content = await file.readAsString();
        final list = jsonDecode(content) as List<dynamic>;
        if (mounted) {
          setState(() {
            _recentConnections = list
                .map((e) => RecentConnection.fromJson(e as Map<String, dynamic>))
                .toList();
          });
        }
      }
    } catch (e) {
      debugPrint('Failed to load recent connections: $e');
    }
  }

  Future<void> _saveRecentConnection(String ip, String port, String pin, String mode) async {
    try {
      _recentConnections.removeWhere((c) => c.ip == ip && c.port == port);
      _recentConnections.insert(
        0,
        RecentConnection(
          ip: ip,
          port: port,
          pin: pin,
          deviceName: 'PC ($ip)',
          lastUsed: DateTime.now(),
          mode: mode,
        ),
      );
      if (_recentConnections.length > 4) {
        _recentConnections = _recentConnections.sublist(0, 4);
      }
      final file = await _getStorageFile();
      await file.writeAsString(jsonEncode(_recentConnections.map((e) => e.toJson()).toList()));
      if (mounted) setState(() {});
    } catch (e) {
      debugPrint('Failed to save recent connections: $e');
    }
  }

  Future<void> _deleteRecentConnection(int index) async {
    try {
      setState(() {
        _recentConnections.removeAt(index);
      });
      final file = await _getStorageFile();
      await file.writeAsString(jsonEncode(_recentConnections.map((e) => e.toJson()).toList()));
    } catch (_) {}
  }

  void _handleConnect({RecentConnection? recent}) {
    final ip = (recent?.ip ?? _ipController.text).trim();
    final port = (recent?.port ?? _portController.text).trim();
    final pin = (recent?.pin ?? _pinController.text).trim();
    final modeToUse = recent?.mode ?? _selectedMode;

    if (ip.isEmpty) {
      setState(() => _errorMessage = 'Vui lòng nhập địa chỉ IP máy chủ');
      return;
    }
    if (pin.length < 4) {
      setState(() => _errorMessage = 'Vui lòng nhập mã PIN bảo mật máy chủ');
      return;
    }

    _saveRecentConnection(ip, port, pin, modeToUse);

    setState(() {
      _isConnecting = true;
      _errorMessage = null;
    });

    Navigator.push(
      context,
      MaterialPageRoute(
        builder: (_) => RemoteDesktopScreen(
          hostIp: ip,
          port: port.isEmpty ? '8080' : port,
          pin: pin,
          mode: modeToUse,
        ),
      ),
    ).then((_) {
      if (mounted) setState(() => _isConnecting = false);
    });
  }

  // In-App Setup Dialog for Windows User (Session Mode: Current User with Auto-Lock or Secondary User)
  void _showSessionConfigDialog() {
    final currentConsoleUser = (_sessionCapability?['current_console_user'] as String?) ?? 'crayn';
    bool useCurrentAccount = true;
    final userCtrl = TextEditingController(text: currentConsoleUser);
    final passCtrl = TextEditingController();
    final pinCtrl = TextEditingController(text: _pinController.text);
    bool obscurePass = true;
    bool isSubmitting = false;
    String? localError;

    showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: Colors.transparent,
      builder: (ctx) => StatefulBuilder(
        builder: (context, setModalState) => Container(
          padding: EdgeInsets.only(
            left: 20,
            right: 20,
            top: 24,
            bottom: MediaQuery.of(context).viewInsets.bottom + 24,
          ),
          decoration: const BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.vertical(top: Radius.circular(24)),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Container(
                    padding: const EdgeInsets.all(10),
                    decoration: BoxDecoration(
                      color: const Color(0xFFF5F3FF),
                      borderRadius: BorderRadius.circular(14),
                    ),
                    child: const Icon(Icons.person_pin_rounded, color: Color(0xFF7C3AED), size: 26),
                  ),
                  const SizedBox(width: 14),
                  const Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Thiết Lập Session Mode',
                          style: TextStyle(fontSize: 18, fontWeight: FontWeight.w800, color: Color(0xFF0F172A)),
                        ),
                        SizedBox(height: 2),
                        Text(
                          'Màn hình riêng biệt • Cơ chế Windows RDP',
                          style: TextStyle(fontSize: 12, color: Color(0xFF64748B)),
                        ),
                      ],
                    ),
                  ),
                  IconButton(
                    icon: const Icon(Icons.close_rounded, color: Color(0xFF94A3B8)),
                    onPressed: () => Navigator.pop(ctx),
                  ),
                ],
              ),
              const SizedBox(height: 16),

              // Account Type Selector Chips
              Row(
                children: [
                  Expanded(
                    child: GestureDetector(
                      onTap: () {
                        setModalState(() {
                          useCurrentAccount = true;
                          userCtrl.text = currentConsoleUser;
                        });
                      },
                      child: Container(
                        padding: const EdgeInsets.symmetric(vertical: 9, horizontal: 8),
                        decoration: BoxDecoration(
                          color: useCurrentAccount ? const Color(0xFFF5F3FF) : Colors.white,
                          borderRadius: BorderRadius.circular(10),
                          border: Border.all(
                            color: useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFFE2E8F0),
                            width: useCurrentAccount ? 1.6 : 1.0,
                          ),
                        ),
                        child: Column(
                          children: [
                            Row(
                              mainAxisAlignment: MainAxisAlignment.center,
                              children: [
                                Icon(Icons.lock_rounded, size: 14, color: useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFF64748B)),
                                const SizedBox(width: 4),
                                Text(
                                  'Tài khoản chính',
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight: FontWeight.w700,
                                    color: useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFF64748B),
                                  ),
                                ),
                              ],
                            ),
                            const SizedBox(height: 2),
                            Text(
                              '($currentConsoleUser)',
                              style: TextStyle(fontSize: 10.5, color: useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFF94A3B8)),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: GestureDetector(
                      onTap: () {
                        setModalState(() {
                          useCurrentAccount = false;
                          userCtrl.text = 'aerostream_remote';
                        });
                      },
                      child: Container(
                        padding: const EdgeInsets.symmetric(vertical: 9, horizontal: 8),
                        decoration: BoxDecoration(
                          color: !useCurrentAccount ? const Color(0xFFF5F3FF) : Colors.white,
                          borderRadius: BorderRadius.circular(10),
                          border: Border.all(
                            color: !useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFFE2E8F0),
                            width: !useCurrentAccount ? 1.6 : 1.0,
                          ),
                        ),
                        child: Column(
                          children: [
                            Row(
                              mainAxisAlignment: MainAxisAlignment.center,
                              children: [
                                Icon(Icons.group_add_rounded, size: 14, color: !useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFF64748B)),
                                const SizedBox(width: 4),
                                Text(
                                  'Tài khoản phụ',
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight: FontWeight.w700,
                                    color: !useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFF64748B),
                                  ),
                                ),
                              ],
                            ),
                            const SizedBox(height: 2),
                            Text(
                              '(Phiên nền riêng)',
                              style: TextStyle(fontSize: 10.5, color: !useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFF94A3B8)),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 14),

              // Dynamic Explanatory Box
              Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: const Color(0xFFF8FAFC),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: const Color(0xFFE2E8F0)),
                ),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Icon(
                      useCurrentAccount ? Icons.security_rounded : Icons.info_outline_rounded,
                      color: useCurrentAccount ? const Color(0xFF7C3AED) : const Color(0xFF2563EB),
                      size: 18,
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        useCurrentAccount
                            ? '🔹 Cơ chế RDP tự khóa: Khi kết nối từ điện thoại bằng tài khoản $currentConsoleUser, màn hình PC sẽ TỰ ĐỘNG KHÓA để người ngoài không thấy thao tác, trong khi điện thoại điều khiển toàn quyền desktop của bạn.'
                            : '🔹 Chế độ Tài khoản phụ: Tạo một phiên làm việc Windows độc lập trong nền. Màn hình máy tính của người ngồi máy không bị chiếm và không thấy thao tác của bạn.',
                        style: const TextStyle(fontSize: 12, color: Color(0xFF475569), height: 1.4),
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 14),

              TextField(
                controller: userCtrl,
                enabled: !useCurrentAccount,
                decoration: InputDecoration(
                  labelText: useCurrentAccount ? 'Tài khoản Windows hiện tại' : 'Tên tài khoản Windows phụ',
                  hintText: useCurrentAccount ? currentConsoleUser : 'aerostream_remote',
                  prefixIcon: const Icon(Icons.account_circle_outlined, size: 20),
                  border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                  contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: passCtrl,
                obscureText: obscurePass,
                decoration: InputDecoration(
                  labelText: useCurrentAccount ? 'Mật khẩu Windows của $currentConsoleUser' : 'Mật khẩu tài khoản phụ',
                  helperText: 'Mật khẩu đăng nhập Windows của tài khoản này',
                  prefixIcon: const Icon(Icons.lock_outline_rounded, size: 20),
                  suffixIcon: IconButton(
                    icon: Icon(obscurePass ? Icons.visibility_off_rounded : Icons.visibility_rounded, size: 20),
                    onPressed: () => setModalState(() => obscurePass = !obscurePass),
                  ),
                  border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                  contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: pinCtrl,
                keyboardType: TextInputType.number,
                maxLength: 6,
                decoration: InputDecoration(
                  labelText: 'Mã PIN bảo mật của host (6 số)',
                  counterText: '',
                  prefixIcon: const Icon(Icons.vpn_key_outlined, size: 20),
                  border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                  contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
                ),
              ),
              if (localError != null) ...[
                const SizedBox(height: 8),
                Text(localError!, style: const TextStyle(color: Color(0xFFEF4444), fontSize: 13, fontWeight: FontWeight.w500)),
              ],
              const SizedBox(height: 16),
              SizedBox(
                width: double.infinity,
                height: 48,
                child: ElevatedButton(
                  onPressed: isSubmitting
                      ? null
                      : () async {
                          final user = userCtrl.text.trim();
                          final pass = passCtrl.text;
                          final pin = pinCtrl.text.trim();
                          if (user.isEmpty || pass.isEmpty || pin.isEmpty) {
                            setModalState(() => localError = 'Vui lòng điền đầy đủ thông tin');
                            return;
                          }

                          setModalState(() {
                            isSubmitting = true;
                            localError = null;
                          });

                          final ip = _ipController.text.trim();
                          final port = _portController.text.trim().isEmpty ? '8080' : _portController.text.trim();
                          final messenger = ScaffoldMessenger.of(context);

                          final client = HttpClient()..connectionTimeout = const Duration(seconds: 4);
                          try {
                            final req = await client.postUrl(Uri.parse('http://$ip:$port/api/session/config'));
                            req.headers.set('content-type', 'application/json');
                            req.add(utf8.encode(jsonEncode({
                              'username': user,
                              'password': pass,
                              'pin': pin,
                            })));
                            final resp = await req.close();
                            final respBody = await resp.transform(utf8.decoder).join();
                            final respJson = jsonDecode(respBody) as Map<String, dynamic>;

                            if (resp.statusCode == 200 && respJson['status'] == 'ok') {
                              if (ctx.mounted) {
                                Navigator.pop(ctx);
                              }
                              if (mounted) {
                                messenger.showSnackBar(
                                  const SnackBar(
                                    content: Text('✓ Đã kích hoạt Session Mode thành công!'),
                                    backgroundColor: Color(0xFF10B981),
                                  ),
                                );
                                setState(() => _selectedMode = 'session');
                                _probeHostCapabilities();
                              }
                            } else {
                              setModalState(() {
                                isSubmitting = false;
                                localError = respJson['message'] ?? respJson['error'] ?? 'Lỗi khi lưu cấu hình';
                              });
                            }
                          } catch (e) {
                            setModalState(() {
                              isSubmitting = false;
                              localError = 'Không thể kết nối máy chủ: $e';
                            });
                          } finally {
                            client.close(force: true);
                          }
                        },
                  style: ElevatedButton.styleFrom(
                    backgroundColor: const Color(0xFF7C3AED),
                    foregroundColor: Colors.white,
                    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                    elevation: 0,
                  ),
                  child: isSubmitting
                      ? const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(color: Colors.white, strokeWidth: 2))
                      : const Text('Lưu & Kích Hoạt Session Mode', style: TextStyle(fontSize: 15, fontWeight: FontWeight.w700)),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  // Fast LAN Scanner to auto-discover running AeroStream instances
  void _showLanDiscoveryDialog() {
    bool isScanning = true;
    final foundDevices = <Map<String, dynamic>>[];

    showModalBottomSheet(
      context: context,
      backgroundColor: Colors.transparent,
      builder: (ctx) => StatefulBuilder(
        builder: (context, setModalState) {
          // Trigger scan once
          if (isScanning && foundDevices.isEmpty) {
            Future.microtask(() async {
              try {
                final currentIp = _ipController.text.trim();
                final parts = currentIp.split('.');
                if (parts.length == 4) {
                  final prefix = '${parts[0]}.${parts[1]}.${parts[2]}';
                  final candidates = <String>{};
                  candidates.add(currentIp);
                  candidates.add('$prefix.1');
                  for (int i = 2; i <= 25; i++) {
                    candidates.add('$prefix.$i');
                  }

                  for (final ip in candidates) {
                    final client = HttpClient()..connectionTimeout = const Duration(milliseconds: 400);
                    try {
                      final req = await client.getUrl(Uri.parse('http://$ip:8080/api/status'));
                      final resp = await req.close().timeout(const Duration(milliseconds: 500));
                      if (resp.statusCode == 200) {
                        final body = await resp.transform(utf8.decoder).join();
                        final json = jsonDecode(body) as Map<String, dynamic>;
                        if (json['status'] == 'online' || json['engine'] == 'aerostream-rust') {
                          foundDevices.add({
                            'ip': ip,
                            'port': '8080',
                            'name': 'Windows PC ($ip)',
                            'edition': json['session_capability']?['edition'] ?? 'Windows',
                          });
                          if (ctx.mounted) setModalState(() {});
                        }
                      }
                    } catch (_) {}
                    client.close(force: true);
                  }
                }
              } catch (_) {}
              if (ctx.mounted) setModalState(() => isScanning = false);
            });
          }

          return Container(
            padding: const EdgeInsets.all(20),
            decoration: const BoxDecoration(
              color: Colors.white,
              borderRadius: BorderRadius.vertical(top: Radius.circular(24)),
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    const Text('Dò Tìm Máy Tính Trong LAN', style: TextStyle(fontSize: 17, fontWeight: FontWeight.w800, color: Color(0xFF0F172A))),
                    if (isScanning)
                      const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2, color: Color(0xFF2563EB)))
                    else
                      IconButton(
                        icon: const Icon(Icons.refresh_rounded, size: 20, color: Color(0xFF2563EB)),
                        onPressed: () {
                          setModalState(() {
                            foundDevices.clear();
                            isScanning = true;
                          });
                        },
                      ),
                  ],
                ),
                const SizedBox(height: 12),
                if (foundDevices.isEmpty)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 30),
                    child: Center(
                      child: Text(
                        isScanning ? 'Đang quét mạng cục bộ Wi-Fi/LAN...' : 'Không tìm thấy máy tính nào đang mở AeroStream',
                        style: const TextStyle(color: Color(0xFF64748B), fontSize: 13),
                      ),
                    ),
                  )
                else
                  Flexible(
                    child: ListView.separated(
                      shrinkWrap: true,
                      itemCount: foundDevices.length,
                      separatorBuilder: (_, _) => const Divider(height: 1, color: Color(0xFFF1F5F9)),
                      itemBuilder: (_, i) {
                        final d = foundDevices[i];
                        return ListTile(
                          contentPadding: EdgeInsets.zero,
                          leading: Container(
                            padding: const EdgeInsets.all(8),
                            decoration: BoxDecoration(color: const Color(0xFFEFF6FF), borderRadius: BorderRadius.circular(10)),
                            child: const Icon(Icons.computer_rounded, color: Color(0xFF2563EB), size: 22),
                          ),
                          title: Text(d['name'], style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 14)),
                          subtitle: Text('${d['ip']}:${d['port']} • ${d['edition']}', style: const TextStyle(fontSize: 12, color: Color(0xFF64748B))),
                          trailing: ElevatedButton(
                            onPressed: () {
                              _ipController.text = d['ip'];
                              _portController.text = d['port'];
                              Navigator.pop(ctx);
                              _probeHostCapabilities();
                            },
                            style: ElevatedButton.styleFrom(
                              backgroundColor: const Color(0xFF2563EB),
                              foregroundColor: Colors.white,
                              elevation: 0,
                              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                            ),
                            child: const Text('Chọn', style: TextStyle(fontSize: 12, fontWeight: FontWeight.bold)),
                          ),
                        );
                      },
                    ),
                  ),
              ],
            ),
          );
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final isSessionReady = _hostSupportedModes.contains('session');

    return Scaffold(
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 20),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 440),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  // App Branding Header
                  Container(
                    width: 68,
                    height: 68,
                    decoration: BoxDecoration(
                      gradient: const LinearGradient(
                        colors: [Color(0xFFEFF6FF), Color(0xFFDBEAFE)],
                        begin: Alignment.topLeft,
                        end: Alignment.bottomRight,
                      ),
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: const Color(0xFFBFDBFE)),
                      boxShadow: const [
                        BoxShadow(color: Color(0x0C2563EB), blurRadius: 12, offset: Offset(0, 4)),
                      ],
                    ),
                    child: const Icon(Icons.desktop_windows_rounded, size: 36, color: Color(0xFF2563EB)),
                  ),
                  const SizedBox(height: 12),
                  RichText(
                    text: const TextSpan(
                      style: TextStyle(fontSize: 26, fontWeight: FontWeight.w800, color: Color(0xFF0F172A)),
                      children: [
                        TextSpan(text: 'Aero'),
                        TextSpan(text: 'Stream', style: TextStyle(color: Color(0xFF2563EB))),
                      ],
                    ),
                  ),
                  const SizedBox(height: 4),
                  const Text(
                    'High Performance Dual-Mode Remote Desktop',
                    style: TextStyle(fontSize: 12.5, color: Color(0xFF64748B), fontWeight: FontWeight.w500),
                  ),
                  const SizedBox(height: 20),

                  // 1. Recent Connections Section (1-tap Connect)
                  if (_recentConnections.isNotEmpty) ...[
                    Align(
                      alignment: Alignment.centerLeft,
                      child: Padding(
                        padding: const EdgeInsets.only(left: 4, bottom: 8),
                        child: Row(
                          children: [
                            const Icon(Icons.history_rounded, size: 16, color: Color(0xFF2563EB)),
                            const SizedBox(width: 6),
                            const Text(
                              'THIẾT BỊ GẦN ĐÂY',
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: FontWeight.w700,
                                letterSpacing: 1.0,
                                color: Color(0xFF64748B),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                    Card(
                      child: Padding(
                        padding: const EdgeInsets.symmetric(vertical: 6),
                        child: Column(
                          children: [
                            for (int i = 0; i < _recentConnections.length; i++) ...[
                              if (i > 0) const Divider(height: 1, indent: 56, endIndent: 16, color: Color(0xFFF1F5F9)),
                              ListTile(
                                contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 2),
                                leading: Container(
                                  width: 38,
                                  height: 38,
                                  decoration: BoxDecoration(
                                    color: _recentConnections[i].mode == 'session'
                                        ? const Color(0xFFF5F3FF)
                                        : const Color(0xFFEFF6FF),
                                    borderRadius: BorderRadius.circular(10),
                                  ),
                                  child: Icon(
                                    _recentConnections[i].mode == 'session'
                                        ? Icons.person_pin_rounded
                                        : Icons.computer_rounded,
                                    color: _recentConnections[i].mode == 'session'
                                        ? const Color(0xFF7C3AED)
                                        : const Color(0xFF2563EB),
                                    size: 20,
                                  ),
                                ),
                                title: Row(
                                  children: [
                                    Expanded(
                                      child: Text(
                                        _recentConnections[i].deviceName,
                                        style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 13.5, color: Color(0xFF0F172A)),
                                      ),
                                    ),
                                    Container(
                                      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                                      decoration: BoxDecoration(
                                        color: _recentConnections[i].mode == 'session'
                                            ? const Color(0xFFF5F3FF)
                                            : const Color(0xFFEFF6FF),
                                        borderRadius: BorderRadius.circular(6),
                                      ),
                                      child: Text(
                                        _recentConnections[i].mode == 'session' ? 'Session' : 'Console',
                                        style: TextStyle(
                                          fontSize: 10,
                                          fontWeight: FontWeight.w700,
                                          color: _recentConnections[i].mode == 'session'
                                              ? const Color(0xFF7C3AED)
                                              : const Color(0xFF2563EB),
                                        ),
                                      ),
                                    ),
                                  ],
                                ),
                                subtitle: Text(
                                  '${_recentConnections[i].ip}:${_recentConnections[i].port} • PIN: ${_recentConnections[i].pin.isNotEmpty ? '••••••' : 'Chưa lưu'}',
                                  style: const TextStyle(fontSize: 11.5, color: Color(0xFF64748B)),
                                ),
                                trailing: Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    IconButton(
                                      icon: const Icon(Icons.close_rounded, size: 16, color: Color(0xFF94A3B8)),
                                      onPressed: () => _deleteRecentConnection(i),
                                      tooltip: 'Xóa khỏi lịch sử',
                                    ),
                                    ElevatedButton(
                                      onPressed: _isConnecting ? null : () => _handleConnect(recent: _recentConnections[i]),
                                      style: ElevatedButton.styleFrom(
                                        backgroundColor: const Color(0xFF2563EB),
                                        foregroundColor: Colors.white,
                                        elevation: 0,
                                        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                                        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                                      ),
                                      child: const Text('Nối ngay', style: TextStyle(fontSize: 11.5, fontWeight: FontWeight.w700)),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                    ),
                    const SizedBox(height: 16),
                  ],

                  // 2. Manual Connect Form Card
                  Card(
                    child: Padding(
                      padding: const EdgeInsets.all(20),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Row(
                            mainAxisAlignment: MainAxisAlignment.spaceBetween,
                            children: [
                              Row(
                                children: [
                                  const Icon(Icons.add_link_rounded, size: 18, color: Color(0xFF0F172A)),
                                  const SizedBox(width: 8),
                                  Text(
                                    _recentConnections.isEmpty ? 'Kết Nối Đến Máy Tính' : 'Hoặc Kết Nối Địa Chỉ Mới',
                                    style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w700, color: Color(0xFF0F172A)),
                                  ),
                                ],
                              ),
                              TextButton.icon(
                                onPressed: _showLanDiscoveryDialog,
                                icon: const Icon(Icons.radar_rounded, size: 15),
                                label: const Text('Quét LAN', style: TextStyle(fontSize: 12, fontWeight: FontWeight.bold)),
                                style: TextButton.styleFrom(
                                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                                  foregroundColor: const Color(0xFF2563EB),
                                ),
                              ),
                            ],
                          ),
                          const SizedBox(height: 14),

                          // IP and Port Input Row
                          Row(
                            children: [
                              Expanded(
                                flex: 3,
                                child: TextField(
                                  controller: _ipController,
                                  decoration: InputDecoration(
                                    labelText: 'IP máy chủ host',
                                    hintText: '192.168.1.x',
                                    prefixIcon: const Icon(Icons.wifi_rounded, size: 18),
                                    border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                                    contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                                  ),
                                  keyboardType: TextInputType.datetime,
                                ),
                              ),
                              const SizedBox(width: 8),
                              Expanded(
                                flex: 2,
                                child: TextField(
                                  controller: _portController,
                                  decoration: InputDecoration(
                                    labelText: 'Port',
                                    border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                                    contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                                  ),
                                  keyboardType: TextInputType.number,
                                ),
                              ),
                            ],
                          ),

                          // Live Host Status / Capability Banner
                          Padding(
                            padding: const EdgeInsets.only(top: 8, bottom: 4),
                            child: InkWell(
                              onTap: () => _probeHostCapabilities(),
                              borderRadius: BorderRadius.circular(8),
                              child: Container(
                                padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                                decoration: BoxDecoration(
                                  color: _isProbing
                                      ? const Color(0xFFF1F5F9)
                                      : (_isHostOnline ? const Color(0xFFECFDF5) : const Color(0xFFFEF2F2)),
                                  borderRadius: BorderRadius.circular(8),
                                  border: Border.all(
                                    color: _isProbing
                                        ? const Color(0xFFE2E8F0)
                                        : (_isHostOnline ? const Color(0xFFA7F3D0) : const Color(0xFFFECACA)),
                                  ),
                                ),
                                child: Row(
                                  children: [
                                    if (_isProbing) ...[
                                      const SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 2, color: Color(0xFF64748B))),
                                      const SizedBox(width: 8),
                                      const Expanded(child: Text('Đang kiểm tra máy chủ...', style: TextStyle(fontSize: 11.5, color: Color(0xFF64748B)))),
                                    ] else if (_isHostOnline) ...[
                                      Container(
                                        width: 8,
                                        height: 8,
                                        decoration: const BoxDecoration(color: Color(0xFF10B981), shape: BoxShape.circle),
                                      ),
                                      const SizedBox(width: 8),
                                      Expanded(
                                        child: Text(
                                          'Online • ${_hostLatencyMs ?? 0}ms • ${_hostEdition ?? "Windows"} (v${_hostVersion ?? "0.6"})',
                                          style: const TextStyle(fontSize: 11.5, color: Color(0xFF065F46), fontWeight: FontWeight.w600),
                                        ),
                                      ),
                                      const Icon(Icons.refresh_rounded, size: 14, color: Color(0xFF059669)),
                                    ] else ...[
                                      Container(
                                        width: 8,
                                        height: 8,
                                        decoration: const BoxDecoration(color: Color(0xFFEF4444), shape: BoxShape.circle),
                                      ),
                                      const SizedBox(width: 8),
                                      const Expanded(
                                        child: Text(
                                          'Không tìm thấy máy chủ (nhấn để thử lại)',
                                          style: TextStyle(fontSize: 11.5, color: Color(0xFF991B1B), fontWeight: FontWeight.w500),
                                        ),
                                      ),
                                      const Icon(Icons.refresh_rounded, size: 14, color: Color(0xFFDC2626)),
                                    ],
                                  ],
                                ),
                              ),
                            ),
                          ),

                          const SizedBox(height: 10),
                          // PIN Code Field with Clipboard Paste
                          TextField(
                            controller: _pinController,
                            decoration: InputDecoration(
                              labelText: 'Mã PIN bảo mật phiên',
                              hintText: '● ● ● ● ● ●',
                              prefixIcon: const Icon(Icons.lock_outline_rounded, size: 18),
                              suffixIcon: IconButton(
                                icon: const Icon(Icons.content_paste_rounded, size: 18, color: Color(0xFF64748B)),
                                onPressed: () async {
                                  final data = await Clipboard.getData(Clipboard.kTextPlain);
                                  if (data?.text != null) {
                                    final cleanPin = data!.text!.replaceAll(RegExp(r'\D'), '');
                                    if (cleanPin.isNotEmpty) {
                                      _pinController.text = cleanPin.length > 6 ? cleanPin.substring(0, 6) : cleanPin;
                                    }
                                  }
                                },
                                tooltip: 'Dán PIN từ clipboard',
                              ),
                              border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                              contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
                            ),
                            keyboardType: TextInputType.number,
                            maxLength: 6,
                            textAlign: TextAlign.center,
                            style: const TextStyle(
                              fontSize: 20,
                              fontWeight: FontWeight.w700,
                              letterSpacing: 6,
                              color: Color(0xFF2563EB),
                            ),
                          ),

                          if (_errorMessage != null) ...[
                            Text(_errorMessage!, style: const TextStyle(color: Color(0xFFEF4444), fontSize: 12.5, fontWeight: FontWeight.w500)),
                            const SizedBox(height: 10),
                          ],

                          const SizedBox(height: 4),
                          // Section Label: Connection Mode
                          const Text(
                            'CHỌN CHẾ ĐỘ ĐIỀU KHIỂN',
                            style: TextStyle(fontSize: 11, fontWeight: FontWeight.w700, letterSpacing: 0.8, color: Color(0xFF64748B)),
                          ),
                          const SizedBox(height: 8),

                          // Phase 6: Dual-Mode Interactive Selector Cards
                          Row(
                            children: [
                              // 1. Console Mode Card
                              Expanded(
                                child: GestureDetector(
                                  onTap: () => setState(() => _selectedMode = 'console'),
                                  child: Container(
                                    padding: const EdgeInsets.all(12),
                                    decoration: BoxDecoration(
                                      color: _selectedMode == 'console' ? const Color(0xFFEFF6FF) : Colors.white,
                                      borderRadius: BorderRadius.circular(14),
                                      border: Border.all(
                                        color: _selectedMode == 'console' ? const Color(0xFF2563EB) : const Color(0xFFE2E8F0),
                                        width: _selectedMode == 'console' ? 1.8 : 1.0,
                                      ),
                                    ),
                                    child: Column(
                                      crossAxisAlignment: CrossAxisAlignment.start,
                                      children: [
                                        Row(
                                          mainAxisAlignment: MainAxisAlignment.spaceBetween,
                                          children: [
                                            Container(
                                              padding: const EdgeInsets.all(6),
                                              decoration: BoxDecoration(
                                                color: _selectedMode == 'console' ? const Color(0xFFDBEAFE) : const Color(0xFFF1F5F9),
                                                borderRadius: BorderRadius.circular(8),
                                              ),
                                              child: Icon(
                                                Icons.desktop_windows_rounded,
                                                size: 18,
                                                color: _selectedMode == 'console' ? const Color(0xFF2563EB) : const Color(0xFF64748B),
                                              ),
                                            ),
                                            if (_selectedMode == 'console')
                                              const Icon(Icons.check_circle_rounded, size: 18, color: Color(0xFF2563EB)),
                                          ],
                                        ),
                                        const SizedBox(height: 8),
                                        const Text(
                                          'Console Mode',
                                          style: TextStyle(fontWeight: FontWeight.w800, fontSize: 13.5, color: Color(0xFF0F172A)),
                                        ),
                                        const SizedBox(height: 2),
                                        const Text(
                                          '60 FPS DXGI/QSV',
                                          style: TextStyle(fontSize: 11, fontWeight: FontWeight.w600, color: Color(0xFF2563EB)),
                                        ),
                                        const SizedBox(height: 4),
                                        const Text(
                                          'Stream màn hình thật, độ trễ ~1ms',
                                          style: TextStyle(fontSize: 10.5, color: Color(0xFF64748B)),
                                          maxLines: 2,
                                          overflow: TextOverflow.ellipsis,
                                        ),
                                      ],
                                    ),
                                  ),
                                ),
                              ),

                              const SizedBox(width: 10),

                              // 2. Session Mode Card (RDP)
                              Expanded(
                                child: GestureDetector(
                                  onTap: () {
                                    if (isSessionReady) {
                                      setState(() => _selectedMode = 'session');
                                    } else {
                                      _showSessionConfigDialog();
                                    }
                                  },
                                  child: Container(
                                    padding: const EdgeInsets.all(12),
                                    decoration: BoxDecoration(
                                      color: _selectedMode == 'session' ? const Color(0xFFF5F3FF) : Colors.white,
                                      borderRadius: BorderRadius.circular(14),
                                      border: Border.all(
                                        color: _selectedMode == 'session' ? const Color(0xFF7C3AED) : const Color(0xFFE2E8F0),
                                        width: _selectedMode == 'session' ? 1.8 : 1.0,
                                      ),
                                    ),
                                    child: Column(
                                      crossAxisAlignment: CrossAxisAlignment.start,
                                      children: [
                                        Row(
                                          mainAxisAlignment: MainAxisAlignment.spaceBetween,
                                          children: [
                                            Container(
                                              padding: const EdgeInsets.all(6),
                                              decoration: BoxDecoration(
                                                color: _selectedMode == 'session' ? const Color(0xFFEDE9FE) : const Color(0xFFF1F5F9),
                                                borderRadius: BorderRadius.circular(8),
                                              ),
                                              child: Icon(
                                                Icons.person_pin_rounded,
                                                size: 18,
                                                color: _selectedMode == 'session' ? const Color(0xFF7C3AED) : const Color(0xFF64748B),
                                              ),
                                            ),
                                            if (_selectedMode == 'session')
                                              const Icon(Icons.check_circle_rounded, size: 18, color: Color(0xFF7C3AED))
                                            else if (!isSessionReady)
                                              Container(
                                                padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 2),
                                                decoration: BoxDecoration(
                                                  color: const Color(0xFFFEF3C7),
                                                  borderRadius: BorderRadius.circular(6),
                                                ),
                                                child: const Text('Cài đặt', style: TextStyle(fontSize: 9.5, fontWeight: FontWeight.w700, color: Color(0xFFD97706))),
                                              ),
                                          ],
                                        ),
                                        const SizedBox(height: 8),
                                        const Text(
                                          'Session Mode',
                                          style: TextStyle(fontWeight: FontWeight.w800, fontSize: 13.5, color: Color(0xFF0F172A)),
                                        ),
                                        const SizedBox(height: 2),
                                        Text(
                                          isSessionReady
                                              ? (_sessionCapability?['configured_username'] != null
                                                  ? (_sessionCapability?['is_current_user'] == true
                                                      ? 'User: ${_sessionCapability!['configured_username']} (Tự khóa)'
                                                      : 'User: ${_sessionCapability!['configured_username']} (Phiên riêng)')
                                                  : 'Windows RDP Riêng')
                                              : 'Chưa cấu hình',
                                          style: TextStyle(
                                            fontSize: 11,
                                            fontWeight: FontWeight.w600,
                                            color: isSessionReady ? const Color(0xFF7C3AED) : const Color(0xFFD97706),
                                          ),
                                        ),
                                        const SizedBox(height: 4),
                                        Text(
                                          _sessionCapability?['is_current_user'] == true
                                              ? 'Tự động khóa màn hình PC khi kết nối (giống RDP)'
                                              : 'Phiên desktop riêng, không chiếm máy',
                                          style: const TextStyle(fontSize: 10.5, color: Color(0xFF64748B)),
                                          maxLines: 2,
                                          overflow: TextOverflow.ellipsis,
                                        ),
                                      ],
                                    ),
                                  ),
                                ),
                              ),
                            ],
                          ),

                          const SizedBox(height: 18),

                          // Primary Connect Action Button
                          SizedBox(
                            width: double.infinity,
                            height: 50,
                            child: ElevatedButton(
                              onPressed: _isConnecting ? null : () => _handleConnect(),
                              style: ElevatedButton.styleFrom(
                                backgroundColor: _selectedMode == 'session' ? const Color(0xFF7C3AED) : const Color(0xFF2563EB),
                                foregroundColor: Colors.white,
                                elevation: 0,
                                shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                              ),
                              child: _isConnecting
                                  ? const SizedBox(width: 22, height: 22, child: CircularProgressIndicator(color: Colors.white, strokeWidth: 2.5))
                                  : Row(
                                      mainAxisAlignment: MainAxisAlignment.center,
                                      children: [
                                        Text(
                                          _selectedMode == 'session' ? 'Bắt Đầu Session Mode' : 'Bắt Đầu Console Mode',
                                          style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w700),
                                        ),
                                        const SizedBox(width: 8),
                                        const Icon(Icons.arrow_forward_rounded, size: 18),
                                      ],
                                    ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),

                  const SizedBox(height: 16),
                  const Text(
                    'AeroStream v0.6 • Đảm bảo điện thoại và PC cùng kết nối một mạng Wi-Fi/LAN.',
                    textAlign: TextAlign.center,
                    style: TextStyle(fontSize: 11.5, color: Color(0xFF94A3B8)),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
