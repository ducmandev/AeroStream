import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'remote_desktop_screen.dart';

/// ==========================================
/// 1. ULTRA-MODERN WINDOWS DESKTOP DASHBOARD
/// ==========================================
class WindowsDashboardScreen extends StatefulWidget {
  const WindowsDashboardScreen({super.key});

  @override
  State<WindowsDashboardScreen> createState() => _WindowsDashboardScreenState();
}

class _WindowsDashboardScreenState extends State<WindowsDashboardScreen> {
  int _selectedTabIndex = 0;
  bool _isServerRunning = false;
  int _fps = 0;
  int _activeClients = 0;
  String _pin = '...';
  String _hostIp = '127.0.0.1';
  final int _port = 8080;
  Timer? _statusTimer;

  // Remote client tab controllers
  final _remoteIpController = TextEditingController(text: '127.0.0.1');
  final _remotePortController = TextEditingController(text: '8080');
  final _remotePinController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _detectLocalIp();
    _ensureRustServerRunning();
    _statusTimer = Timer.periodic(const Duration(seconds: 1), (_) => _fetchServerStatus());
  }

  @override
  void dispose() {
    _statusTimer?.cancel();
    _remoteIpController.dispose();
    _remotePortController.dispose();
    _remotePinController.dispose();
    super.dispose();
  }

  Future<void> _detectLocalIp() async {
    try {
      final interfaces = await NetworkInterface.list(type: InternetAddressType.IPv4);
      for (var interface in interfaces) {
        for (var addr in interface.addresses) {
          if (!addr.isLoopback) {
            setState(() {
              _hostIp = addr.address;
            });
            return;
          }
        }
      }
    } catch (_) {}
  }

  Future<void> _ensureRustServerRunning() async {
    await _fetchServerStatus();
    if (!_isServerRunning) {
      try {
        final currentDir = File(Platform.resolvedExecutable).parent.path;
        final candidates = [
          '$currentDir\\aerostream.exe',
          '$currentDir\\aerostream_engine.exe',
          'd:\\StreamApp\\target\\release\\aerostream.exe',
          'aerostream.exe',
        ];
        for (final exePath in candidates) {
          if (File(exePath).existsSync()) {
            await Process.start(exePath, [], mode: ProcessStartMode.detached);
            await Future.delayed(const Duration(milliseconds: 1200));
            await _fetchServerStatus();
            if (_isServerRunning) break;
          }
        }
      } catch (_) {}
    }
  }

  Future<void> _fetchServerStatus() async {
    try {
      final client = HttpClient()..connectionTimeout = const Duration(seconds: 1);
      final req = await client.getUrl(Uri.parse('http://127.0.0.1:$_port/api/status'));
      final resp = await req.close();
      if (resp.statusCode == 200) {
        final body = await resp.transform(utf8.decoder).join();
        final json = jsonDecode(body) as Map<String, dynamic>;
        if (mounted) {
          setState(() {
            _isServerRunning = json['status'] == 'online';
            _fps = json['fps'] ?? 0;
            _activeClients = json['active_clients'] ?? 0;
            if (json.containsKey('pin')) {
              _pin = json['pin'].toString();
            }
          });
        }
        return;
      }
    } catch (_) {}
    if (mounted && _isServerRunning) {
      setState(() => _isServerRunning = false);
    }
  }

  String get _clientUrl => 'http://$_hostIp:$_port/?pin=$_pin';
  String get _localUrl => 'http://localhost:$_port/?pin=$_pin';

  void _copyToClipboard(String text, String message) {
    Clipboard.setData(ClipboardData(text: text));
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Row(
          children: [
            const Icon(Icons.check_circle_rounded, color: Colors.white, size: 20),
            const SizedBox(width: 8),
            Text(message),
          ],
        ),
        backgroundColor: const Color(0xFF10B981),
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
        duration: const Duration(seconds: 2),
      ),
    );
  }

  void _openUrlInBrowser(String url) {
    Process.run('explorer.exe', [url]);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Row(
        children: [
          // Modern Left Navigation Rail
          Container(
            width: 260,
            decoration: const BoxDecoration(
              color: Colors.white,
              border: Border(right: BorderSide(color: Color(0xFFE2E8F0))),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // Brand Header
                Padding(
                  padding: const EdgeInsets.all(24),
                  child: Row(
                    children: [
                      Container(
                        padding: const EdgeInsets.all(10),
                        decoration: BoxDecoration(
                          color: const Color(0xFFEFF6FF),
                          borderRadius: BorderRadius.circular(12),
                          border: Border.all(color: const Color(0xFFBFDBFE)),
                        ),
                        child: const Icon(Icons.desktop_windows_rounded, color: Color(0xFF2563EB), size: 24),
                      ),
                      const SizedBox(width: 12),
                      Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          RichText(
                            text: const TextSpan(
                              style: TextStyle(fontSize: 18, fontWeight: FontWeight.w800, color: Color(0xFF0F172A)),
                              children: [
                                TextSpan(text: 'Aero'),
                                TextSpan(text: 'Stream', style: TextStyle(color: Color(0xFF2563EB))),
                              ],
                            ),
                          ),
                          const Text('Remote Desktop Hub', style: TextStyle(fontSize: 11, color: Color(0xFF64748B))),
                        ],
                      ),
                    ],
                  ),
                ),

                const Divider(height: 1, color: Color(0xFFE2E8F0)),
                const SizedBox(height: 12),

                // Navigation Items
                _buildNavItem(0, Icons.share_rounded, 'Share This Computer'),
                _buildNavItem(1, Icons.open_in_new_rounded, 'Control Remote PC'),
                _buildNavItem(2, Icons.info_outline_rounded, 'About & Gestures'),

                const Spacer(),

                // Server Status Pill Card
                Container(
                  margin: const EdgeInsets.all(16),
                  padding: const EdgeInsets.all(16),
                  decoration: BoxDecoration(
                    color: _isServerRunning ? const Color(0xFFF0FDF4) : const Color(0xFFFEF2F2),
                    borderRadius: BorderRadius.circular(14),
                    border: Border.all(
                      color: _isServerRunning ? const Color(0xFFBBF7D0) : const Color(0xFFFECACA),
                    ),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Container(
                            width: 8,
                            height: 8,
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: _isServerRunning ? const Color(0xFF10B981) : const Color(0xFFEF4444),
                            ),
                          ),
                          const SizedBox(width: 8),
                          Text(
                            _isServerRunning ? 'Server Running' : 'Server Offline',
                            style: TextStyle(
                              fontSize: 13,
                              fontWeight: FontWeight.w700,
                              color: _isServerRunning ? const Color(0xFF166534) : const Color(0xFF991B1B),
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 6),
                      Text(
                        _isServerRunning ? '$_fps FPS  •  $_activeClients Connected' : 'Checking background engine...',
                        style: TextStyle(
                          fontSize: 11,
                          color: _isServerRunning ? const Color(0xFF15803D) : const Color(0xFFB91C1C),
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),

          // Main Viewport Area
          Expanded(
            child: Container(
              color: const Color(0xFFF8FAFC),
              padding: const EdgeInsets.all(32),
              child: _selectedTabIndex == 0
                  ? _buildHostShareView()
                  : _selectedTabIndex == 1
                      ? _buildRemoteClientView()
                      : _buildAboutView(),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildNavItem(int index, IconData icon, String title) {
    final isSelected = _selectedTabIndex == index;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      child: Material(
        color: isSelected ? const Color(0xFFEFF6FF) : Colors.transparent,
        borderRadius: BorderRadius.circular(12),
        child: InkWell(
          onTap: () => setState(() => _selectedTabIndex = index),
          borderRadius: BorderRadius.circular(12),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
            child: Row(
              children: [
                Icon(icon, size: 20, color: isSelected ? const Color(0xFF2563EB) : const Color(0xFF64748B)),
                const SizedBox(width: 12),
                Text(
                  title,
                  style: TextStyle(
                    fontSize: 14,
                    fontWeight: isSelected ? FontWeight.w700 : FontWeight.w500,
                    color: isSelected ? const Color(0xFF2563EB) : const Color(0xFF334155),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// 1.1 Host Share View (The centerpiece of the Windows App)
  Widget _buildHostShareView() {
    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Header
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              const Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Share This Display',
                    style: TextStyle(fontSize: 24, fontWeight: FontWeight.w800, color: Color(0xFF0F172A)),
                  ),
                  SizedBox(height: 4),
                  Text(
                    'Control this computer from your Phone (iOS/Android) or any Web Browser',
                    style: TextStyle(fontSize: 13, color: Color(0xFF64748B)),
                  ),
                ],
              ),
              ElevatedButton.icon(
                onPressed: () => _openUrlInBrowser(_localUrl),
                icon: const Icon(Icons.open_in_browser_rounded, size: 18),
                label: const Text('Open Web Viewer'),
                style: ElevatedButton.styleFrom(
                  backgroundColor: const Color(0xFF2563EB),
                  foregroundColor: Colors.white,
                  elevation: 0,
                  padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 14),
                  shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                ),
              ),
            ],
          ),

          const SizedBox(height: 28),

          // Main 2-Column Layout: Credentials & QR Code
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              // Left Credentials Card
              Expanded(
                flex: 3,
                child: Column(
                  children: [
                    // PIN Card
                    Card(
                      child: Padding(
                        padding: const EdgeInsets.all(24),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            const Row(
                              mainAxisAlignment: MainAxisAlignment.spaceBetween,
                              children: [
                                Text(
                                  'SESSION SECURITY PIN',
                                  style: TextStyle(
                                    fontSize: 11,
                                    fontWeight: FontWeight.w700,
                                    letterSpacing: 1.2,
                                    color: Color(0xFF64748B),
                                  ),
                                ),
                                Icon(Icons.shield_outlined, size: 18, color: Color(0xFF2563EB)),
                              ],
                            ),
                            const SizedBox(height: 12),
                            Row(
                              children: [
                                Expanded(
                                  child: Container(
                                    padding: const EdgeInsets.symmetric(vertical: 14, horizontal: 20),
                                    decoration: BoxDecoration(
                                      color: const Color(0xFFF8FAFC),
                                      borderRadius: BorderRadius.circular(12),
                                      border: Border.all(color: const Color(0xFFE2E8F0)),
                                    ),
                                    child: Text(
                                      _pin,
                                      style: const TextStyle(
                                        fontSize: 32,
                                        fontWeight: FontWeight.w800,
                                        letterSpacing: 10,
                                        color: Color(0xFF2563EB),
                                      ),
                                      textAlign: TextAlign.center,
                                    ),
                                  ),
                                ),
                                const SizedBox(width: 12),
                                IconButton.filledTonal(
                                  onPressed: () => _copyToClipboard(_pin, 'PIN copied to clipboard!'),
                                  icon: const Icon(Icons.copy_rounded, size: 20),
                                  style: IconButton.styleFrom(
                                    backgroundColor: const Color(0xFFEFF6FF),
                                    foregroundColor: const Color(0xFF2563EB),
                                    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                                    padding: const EdgeInsets.all(16),
                                  ),
                                ),
                              ],
                            ),
                            const SizedBox(height: 12),
                            const Text(
                              'Enter this PIN on your phone or mobile app to grant remote desktop control.',
                              style: TextStyle(fontSize: 12, color: Color(0xFF94A3B8)),
                            ),
                          ],
                        ),
                      ),
                    ),

                    const SizedBox(height: 16),

                    // Connection URL Card
                    Card(
                      child: Padding(
                        padding: const EdgeInsets.all(24),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            const Text(
                              'MOBILE CLIENT CONNECTION LINK',
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: FontWeight.w700,
                                letterSpacing: 1.2,
                                color: Color(0xFF64748B),
                              ),
                            ),
                            const SizedBox(height: 12),
                            Container(
                              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                              decoration: BoxDecoration(
                                color: const Color(0xFFF8FAFC),
                                borderRadius: BorderRadius.circular(12),
                                border: Border.all(color: const Color(0xFFE2E8F0)),
                              ),
                              child: Row(
                                children: [
                                  const Icon(Icons.link_rounded, color: Color(0xFF64748B), size: 20),
                                  const SizedBox(width: 10),
                                  Expanded(
                                    child: Text(
                                      _clientUrl,
                                      style: const TextStyle(
                                        fontSize: 14,
                                        fontWeight: FontWeight.w600,
                                        color: Color(0xFF0F172A),
                                      ),
                                    ),
                                  ),
                                  TextButton.icon(
                                    onPressed: () => _copyToClipboard(_clientUrl, 'Connection link copied!'),
                                    icon: const Icon(Icons.copy_rounded, size: 16),
                                    label: const Text('Copy Link'),
                                    style: TextButton.styleFrom(
                                      foregroundColor: const Color(0xFF2563EB),
                                      textStyle: const TextStyle(fontWeight: FontWeight.w600),
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ],
                ),
              ),

              const SizedBox(width: 20),

              // Right QR Code Card
              Expanded(
                flex: 2,
                child: Card(
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.center,
                      children: [
                        const Text(
                          'SCAN WITH PHONE CAMERA',
                          style: TextStyle(
                            fontSize: 11,
                            fontWeight: FontWeight.w700,
                            letterSpacing: 1.2,
                            color: Color(0xFF64748B),
                          ),
                        ),
                        const SizedBox(height: 16),

                        // Rendered QR Code
                        Container(
                          padding: const EdgeInsets.all(16),
                          decoration: BoxDecoration(
                            color: Colors.white,
                            borderRadius: BorderRadius.circular(16),
                            border: Border.all(color: const Color(0xFFE2E8F0)),
                            boxShadow: [
                              BoxShadow(
                                color: const Color(0xFF0F172A).withOpacity(0.04),
                                blurRadius: 12,
                                offset: const Offset(0, 4),
                              ),
                            ],
                          ),
                          child: QrImageView(
                            data: _clientUrl,
                            version: QrVersions.auto,
                            size: 190.0,
                            eyeStyle: const QrEyeStyle(
                              eyeShape: QrEyeShape.square,
                              color: Color(0xFF0F172A),
                            ),
                            dataModuleStyle: const QrDataModuleStyle(
                              dataModuleShape: QrDataModuleShape.square,
                              color: Color(0xFF0F172A),
                            ),
                          ),
                        ),

                        const SizedBox(height: 16),
                        const Text(
                          'Open iPhone Safari or Android Camera to connect instantly without typing.',
                          style: TextStyle(fontSize: 12, color: Color(0xFF64748B), height: 1.4),
                          textAlign: TextAlign.center,
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  /// 1.2 Control Another PC (Client View)
  Widget _buildRemoteClientView() {
    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            'Connect to Remote Computer',
            style: TextStyle(fontSize: 24, fontWeight: FontWeight.w800, color: Color(0xFF0F172A)),
          ),
          const SizedBox(height: 4),
          const Text(
            'Enter the IP address and PIN of the computer you wish to view and control',
            style: TextStyle(fontSize: 13, color: Color(0xFF64748B)),
          ),
          const SizedBox(height: 24),

          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 480),
            child: Card(
              child: Padding(
                padding: const EdgeInsets.all(28),
                child: Column(
                  children: [
                    TextField(
                      controller: _remoteIpController,
                      decoration: InputDecoration(
                        labelText: 'Remote Host IP',
                        prefixIcon: const Icon(Icons.computer_rounded),
                        border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                      ),
                    ),
                    const SizedBox(height: 16),
                    TextField(
                      controller: _remotePinController,
                      decoration: InputDecoration(
                        labelText: '6-digit Security PIN',
                        prefixIcon: const Icon(Icons.lock_rounded),
                        border: OutlineInputBorder(borderRadius: BorderRadius.circular(12)),
                      ),
                      keyboardType: TextInputType.number,
                      maxLength: 6,
                    ),
                    const SizedBox(height: 16),
                    SizedBox(
                      width: double.infinity,
                      height: 48,
                      child: ElevatedButton.icon(
                        onPressed: () {
                          final ip = _remoteIpController.text.trim();
                          final pin = _remotePinController.text.trim();
                          if (ip.isNotEmpty && pin.isNotEmpty) {
                            Navigator.push(
                              context,
                              MaterialPageRoute(
                                builder: (_) => RemoteDesktopScreen(
                                  hostIp: ip,
                                  port: _remotePortController.text.trim().isEmpty ? '8080' : _remotePortController.text.trim(),
                                  pin: pin,
                                ),
                              ),
                            );
                          }
                        },
                        icon: const Icon(Icons.arrow_forward_rounded),
                        label: const Text('Connect to Remote PC', style: TextStyle(fontWeight: FontWeight.w700)),
                        style: ElevatedButton.styleFrom(
                          backgroundColor: const Color(0xFF2563EB),
                          foregroundColor: Colors.white,
                          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                          elevation: 0,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  /// 1.3 About & Gestures View
  Widget _buildAboutView() {
    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('About AeroStream', style: TextStyle(fontSize: 24, fontWeight: FontWeight.w800, color: Color(0xFF0F172A))),
          const SizedBox(height: 20),
          Card(
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text('💡 Mouse & Touch Navigation Guide:', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w700)),
                  const SizedBox(height: 16),
                  _buildGuideRow(Icons.touch_app_rounded, 'Single finger drag', 'Moves the mouse cursor smoothly across the display'),
                  _buildGuideRow(Icons.mouse_rounded, 'Single tap', 'Triggers Left-Click at current position'),
                  _buildGuideRow(Icons.pan_tool_rounded, 'Two finger tap / Long press', 'Triggers Right-Click (Context Menu)'),
                  _buildGuideRow(Icons.swap_vert_rounded, 'Two finger drag', 'Scrolls web pages and documents vertically'),
                  _buildGuideRow(Icons.keyboard_rounded, 'Keyboard button', 'Toggles native on-screen keyboard for text input'),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildGuideRow(IconData icon, String action, String description) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 20, color: const Color(0xFF2563EB)),
          const SizedBox(width: 14),
          Expanded(
            child: RichText(
              text: TextSpan(
                style: const TextStyle(fontSize: 13, color: Color(0xFF334155)),
                children: [
                  TextSpan(text: '$action: ', style: const TextStyle(fontWeight: FontWeight.w700, color: Color(0xFF0F172A))),
                  TextSpan(text: description),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
