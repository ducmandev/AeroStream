import 'dart:convert';
import 'dart:io';
import 'package:flutter/material.dart';
import '../models/recent_connection.dart';
import 'remote_desktop_screen.dart';

/// ==========================================
/// 2. CLEAN MOBILE CONNECT SCREEN (ANDROID)
/// ==========================================
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
  String? _errorMessage;
  List<RecentConnection> _recentConnections = [];

  @override
  void initState() {
    super.initState();
    _loadRecentConnections();
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

  Future<void> _saveRecentConnection(String ip, String port, String pin) async {
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

    if (ip.isEmpty) {
      setState(() => _errorMessage = 'Vui lòng nhập địa chỉ IP máy chủ');
      return;
    }
    if (pin.length < 4) {
      setState(() => _errorMessage = 'Vui lòng nhập mã PIN 6 số của phiên');
      return;
    }

    _saveRecentConnection(ip, port, pin);

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
        ),
      ),
    ).then((_) {
      if (mounted) setState(() => _isConnecting = false);
    });
  }

  @override
  Widget build(BuildContext context) {
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
                  Container(
                    width: 68,
                    height: 68,
                    decoration: BoxDecoration(
                      color: const Color(0xFFEFF6FF),
                      borderRadius: BorderRadius.circular(20),
                      border: Border.all(color: const Color(0xFFBFDBFE)),
                    ),
                    child: const Icon(Icons.desktop_windows_rounded, size: 36, color: Color(0xFF2563EB)),
                  ),
                  const SizedBox(height: 14),
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
                  const Text('Simple & Fast Remote Desktop', style: TextStyle(fontSize: 13, color: Color(0xFF64748B), fontWeight: FontWeight.w500)),
                  const SizedBox(height: 24),

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
                        padding: const EdgeInsets.symmetric(vertical: 8),
                        child: Column(
                          children: [
                            for (int i = 0; i < _recentConnections.length; i++) ...[
                              if (i > 0) const Divider(height: 1, indent: 56, endIndent: 16, color: Color(0xFFF1F5F9)),
                              ListTile(
                                contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 2),
                                leading: Container(
                                  width: 40,
                                  height: 40,
                                  decoration: BoxDecoration(
                                    color: const Color(0xFFEFF6FF),
                                    borderRadius: BorderRadius.circular(10),
                                  ),
                                  child: const Icon(Icons.computer_rounded, color: Color(0xFF2563EB), size: 22),
                                ),
                                title: Text(
                                  _recentConnections[i].deviceName,
                                  style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 14, color: Color(0xFF0F172A)),
                                ),
                                subtitle: Text(
                                  '${_recentConnections[i].ip}:${_recentConnections[i].port} • PIN: ${_recentConnections[i].pin.isNotEmpty ? '••••••' : 'Chưa lưu'}',
                                  style: const TextStyle(fontSize: 12, color: Color(0xFF64748B)),
                                ),
                                trailing: Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    IconButton(
                                      icon: const Icon(Icons.close_rounded, size: 18, color: Color(0xFF94A3B8)),
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
                                        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
                                      ),
                                      child: const Text('Nối ngay', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w700)),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ],
                        ),
                      ),
                    ),
                    const SizedBox(height: 20),
                  ],

                  // 2. Manual Connect Form Card
                  Card(
                    child: Padding(
                      padding: const EdgeInsets.all(22),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
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
                          const SizedBox(height: 18),
                          Row(
                            children: [
                              Expanded(
                                flex: 3,
                                child: TextField(
                                  controller: _ipController,
                                  decoration: InputDecoration(
                                    labelText: 'IP máy chủ host',
                                    hintText: '192.168.1.x',
                                    prefixIcon: const Icon(Icons.wifi_rounded, size: 20),
                                    border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                                    contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 14),
                                  ),
                                  keyboardType: TextInputType.datetime,
                                ),
                              ),
                              const SizedBox(width: 10),
                              Expanded(
                                flex: 2,
                                child: TextField(
                                  controller: _portController,
                                  decoration: InputDecoration(
                                    labelText: 'Port',
                                    border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                                    contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 14),
                                  ),
                                  keyboardType: TextInputType.number,
                                ),
                              ),
                            ],
                          ),
                          const SizedBox(height: 14),
                          TextField(
                            controller: _pinController,
                            decoration: InputDecoration(
                              labelText: 'Mã PIN bảo mật 6 số',
                              hintText: '● ● ● ● ● ●',
                              prefixIcon: const Icon(Icons.lock_outline_rounded, size: 20),
                              border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                              contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 14),
                            ),
                            keyboardType: TextInputType.number,
                            maxLength: 6,
                            textAlign: TextAlign.center,
                            style: const TextStyle(
                              fontSize: 22,
                              fontWeight: FontWeight.w700,
                              letterSpacing: 6,
                              color: Color(0xFF2563EB),
                            ),
                          ),
                          if (_errorMessage != null) ...[
                            Text(_errorMessage!, style: const TextStyle(color: Color(0xFFEF4444), fontSize: 13, fontWeight: FontWeight.w500)),
                            const SizedBox(height: 12),
                          ],
                          const SizedBox(height: 8),
                          SizedBox(
                            width: double.infinity,
                            height: 50,
                            child: ElevatedButton(
                              onPressed: _isConnecting ? null : () => _handleConnect(),
                              style: ElevatedButton.styleFrom(
                                backgroundColor: const Color(0xFF2563EB),
                                foregroundColor: Colors.white,
                                elevation: 0,
                                shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                              ),
                              child: _isConnecting
                                  ? const SizedBox(width: 22, height: 22, child: CircularProgressIndicator(color: Colors.white, strokeWidth: 2.5))
                                  : const Row(
                                      mainAxisAlignment: MainAxisAlignment.center,
                                      children: [
                                        Text('Bắt Đầu Phiên Điều Khiển', style: TextStyle(fontSize: 15, fontWeight: FontWeight.w700)),
                                        SizedBox(width: 8),
                                        Icon(Icons.arrow_forward_rounded, size: 18),
                                      ],
                                    ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),

                  const SizedBox(height: 20),
                  const Text('Đảm bảo điện thoại và máy tính cùng kết nối chung một mạng Wi-Fi/LAN.', textAlign: TextAlign.center, style: TextStyle(fontSize: 12, color: Color(0xFF94A3B8))),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
