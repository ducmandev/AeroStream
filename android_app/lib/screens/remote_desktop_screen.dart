import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import '../models/hud_dock_edge.dart';
import '../widgets/hardware_cursor.dart';

class RemoteDesktopScreen extends StatefulWidget {
  final String hostIp;
  final String port;
  final String pin;

  const RemoteDesktopScreen({
    super.key,
    required this.hostIp,
    required this.port,
    required this.pin,
  });

  @override
  State<RemoteDesktopScreen> createState() => _RemoteDesktopScreenState();
}

class _RemoteDesktopScreenState extends State<RemoteDesktopScreen> with WidgetsBindingObserver {
  WebSocket? _socket;
  bool _isConnected = false;
  bool _isReconnecting = false;
  final ValueNotifier<Uint8List?> _frameNotifier = ValueNotifier<Uint8List?>(null);
  final ValueNotifier<String> _hudNotifier = ValueNotifier<String>('-- ms  •  -- fps');
  final ValueNotifier<Offset?> _cursorNotifier = ValueNotifier<Offset?>(null);
  int _desktopWidth = 1920;
  int _desktopHeight = 1080;
  int _latencyMs = 5;
  int _fpsCount = 0;
  int _displayFps = 0;
  DateTime _lastFpsTime = DateTime.now();
  String _currentMode = 'balanced'; // 60 FPS 720p native default for buttery smooth response
  Timer? _pingTimer;

  bool _isDirectTouch = true;
  bool _isDesktopLocked = false;
  Timer? _islandTimer;
  bool _isHudCollapsed = false;
  HudDockEdge _hudDockEdge = HudDockEdge.top;
  bool _isKeyboardBarVisible = false;
  bool _isLightTheme = true; // Windows 11 Fluent Light Theme (default)

  // Floating Draggable IME Button state (Separate corner button, freely draggable & toggleable)
  bool _isFloatingImeVisible = true;
  Offset? _floatingImeOffset;

  // Ultra-responsive zero-latency raw touch pointer tracking
  int _activePointerCount = 0;
  final Map<int, Offset> _activePointers = {};
  DateTime? _primaryPointerDownTime;
  Offset? _primaryPointerDownPos;
  bool _primaryPointerMoved = false;
  Timer? _longPressTimer;
  DateTime? _lastTwoFingerTapTime;

  // Streaming quality parameters for detailed tuning
  int _targetStreamHeight = 720;
  int _targetFps = 60;
  int _targetQuality = 70;

  String _getModeLabel() {
    switch (_currentMode) {
      case 'auto':
        return 'Auto';
      case 'eco':
        return '540p';
      case 'balanced':
        return '720p';
      case 'quality':
      case 'high':
        return '1080p';
      case 'ultra':
        return 'Ultra';
      case '2k':
      case '1440p':
        return '2K';
      default:
        return _currentMode.toUpperCase();
    }
  }

  // Zoom & Pan screen state
  double _zoomScale = 1.0;
  Offset _zoomPan = Offset.zero;
  double _baseScale = 1.0;

  // Mouse speed & cursor scale customization
  double _mouseSpeed = 30.0; // Range: 1.0 to 100.0, default 30.0 (Chuẩn)
  bool _enableMouseAccel = true; // Dynamic precision acceleration
  String _cursorScaleMode = 'standard'; // 'native' (Windows 1:1), 'standard' (Vừa), 'large' (Lớn)
  double _accumulatedDx = 0.0;
  double _accumulatedDy = 0.0;

  double _calculateCursorHeight(double renderHeight) {
    final double baseHeight = _desktopHeight > 0 ? _desktopHeight.toDouble() : 1080.0;
    final double screenToDesktopRatio = renderHeight / baseHeight;
    double multiplier;
    switch (_cursorScaleMode) {
      case 'native':
        multiplier = 26.0; // Slender, true Windows 1:1 ratio
        break;
      case 'large':
        multiplier = 38.0; // Clear & visible
        break;
      case 'standard':
      default:
        multiplier = 30.0; // Slender, perfectly proportioned Windows default
        break;
    }
    final double rawHeight = screenToDesktopRatio * multiplier;
    final double zoomComp = _zoomScale > 1.05 ? math.pow(_zoomScale, 0.65).toDouble() : 1.0;
    return (rawHeight / zoomComp).clamp(7.5, 20.0);
  }

  void _resetIslandTimer({bool forceOpen = false}) {
    if (forceOpen) {
      if (mounted && _isHudCollapsed) {
        setState(() => _isHudCollapsed = false);
      }
    }
    if (_isHudCollapsed) return;

    _islandTimer?.cancel();
    _islandTimer = Timer(const Duration(seconds: 5), () {
      if (mounted && !_isHudCollapsed && !_isKeyboardBarVisible) {
        setState(() => _isHudCollapsed = true);
      }
    });
  }

  Offset _toSceneCoordinates(Offset touchPoint, Size containerSize) {
    if (_zoomScale <= 1.001) return touchPoint;
    final center = Offset(containerSize.width / 2.0, containerSize.height / 2.0);
    final sceneX = center.dx + (touchPoint.dx - center.dx - _zoomPan.dx) / _zoomScale;
    final sceneY = center.dy + (touchPoint.dy - center.dy - _zoomPan.dy) / _zoomScale;
    return Offset(sceneX, sceneY);
  }

  static const MethodChannel _audioControlChannel = MethodChannel('aerostream/audio_control');
  static const BasicMessageChannel<ByteData> _audioStreamChannel = BasicMessageChannel<ByteData>(
    'aerostream/audio_stream',
    BinaryCodec(),
  );
  bool _isAudioMuted = false;
  int _serverClockOffset = 0;

  void _toggleAudioMute() {
    setState(() => _isAudioMuted = !_isAudioMuted);
    try {
      _audioControlChannel.invokeMethod('setMuted', {'muted': _isAudioMuted});
    } catch (_) {}
  }
  final FocusNode _keyboardFocusNode = FocusNode();
  final TextEditingController _keyboardController = TextEditingController();
  final FocusNode _hardwareFocusNode = FocusNode();
  Offset _lastPanOffset = Offset.zero;

  void _refocusHardwareKeyboard() {
    if (mounted && !_hardwareFocusNode.hasFocus) {
      _hardwareFocusNode.requestFocus();
    }
  }

  void _updateHud() {
    _hudNotifier.value = '$_latencyMs ms  •  $_displayFps fps';
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _connectWebSocket();
    _startPingTimer();
    _resetIslandTimer();
    try {
      _audioControlChannel.invokeMethod('start');
    } catch (_) {}
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      if (_socket == null || _socket!.readyState != WebSocket.open) {
        _connectWebSocket();
      } else {
        try {
          _audioControlChannel.invokeMethod('start');
        } catch (_) {}
        _sendInput({'type': 'ping', 't': DateTime.now().millisecondsSinceEpoch});
        _sendInput({'type': 'set_mode', 'mode': _currentMode});
      }
    } else if (state == AppLifecycleState.paused) {
      try {
        _audioControlChannel.invokeMethod('stop');
      } catch (_) {}
    }
  }

  void _startPingTimer() {
    _pingTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (_socket != null && _socket!.readyState == WebSocket.open) {
        _sendInput({'type': 'ping', 't': DateTime.now().millisecondsSinceEpoch});
      }
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _islandTimer?.cancel();
    _pingTimer?.cancel();
    _longPressTimer?.cancel();
    _socket?.close();
    try {
      _audioControlChannel.invokeMethod('stop');
    } catch (_) {}
    _keyboardFocusNode.dispose();
    _keyboardController.dispose();
    _hardwareFocusNode.dispose();
    _frameNotifier.dispose();
    _hudNotifier.dispose();
    _cursorNotifier.dispose();
    super.dispose();
  }

  Future<void> _connectWebSocket() async {
    if (_isReconnecting) return;
    _isReconnecting = true;
    final url = 'ws://${widget.hostIp}:${widget.port}/ws?pin=${Uri.encodeComponent(widget.pin)}';
    try {
      final ws = await WebSocket.connect(url).timeout(const Duration(seconds: 5));
      if (!mounted) return;

      setState(() {
        _socket = ws;
        _isConnected = true;
        _isReconnecting = false;
      });
      _audioControlChannel.invokeMethod('start');
      _updateHud();

      // Default mobile to Eco mode (1.5 Mbps, 540p) to guarantee smooth, low-latency streaming
      _sendInput({'type': 'set_mode', 'mode': _currentMode});
      _sendInput({'type': 'ping', 't': DateTime.now().millisecondsSinceEpoch});

      ws.listen(
        (data) {
          if (data is List<int>) {
            _handleBinaryFrame(data is Uint8List ? data : Uint8List.fromList(data));
          } else if (data is String) {
            try {
              final json = jsonDecode(data);
              if (json['type'] == 'pong') {
                final t = (json['client_time'] ?? json['t']) as int?;
                if (t != null) {
                  final now = DateTime.now().millisecondsSinceEpoch;
                  final rtt = (now - t).clamp(1, 999);
                  _latencyMs = rtt;
                  final serverTime = json['server_time'] as int?;
                  if (serverTime != null) {
                    final measuredOffset = (serverTime + (rtt ~/ 2)) - now;
                    _serverClockOffset = _serverClockOffset == 0 ? measuredOffset : ((_serverClockOffset * 7 + measuredOffset * 3) ~/ 10);
                  }
                  _updateHud();
                }
              } else if (json['type'] == 'time_sync') {
                final serverTime = json['server_time'] as int?;
                if (serverTime != null) {
                  _serverClockOffset = serverTime - DateTime.now().millisecondsSinceEpoch;
                  _sendInput({'type': 'ping', 't': DateTime.now().millisecondsSinceEpoch});
                }
              } else if (json['type'] == 'lock_status') {
                final locked = json['locked'] as bool? ?? false;
                if (_isDesktopLocked != locked && mounted) {
                  setState(() {
                    _isDesktopLocked = locked;
                  });
                }
              } else if (json['type'] == 'cursor') {
                final x = (json['x'] as num?)?.toDouble();
                final y = (json['y'] as num?)?.toDouble();
                final visible = json['visible'] as bool? ?? true;
                if (x != null && y != null && visible) {
                  // Only overwrite local cursor position when user is not actively dragging on trackpad
                  if (_activePointerCount == 0) {
                    _cursorNotifier.value = Offset(x, y);
                  }
                } else if (!visible) {
                  _cursorNotifier.value = null;
                }
              }
            } catch (_) {}
          }
        },
        onError: (err) {
          if (mounted) {
            setState(() {
              _isConnected = false;
              _isReconnecting = false;
            });
            _updateHud();
            _audioControlChannel.invokeMethod('stop');
          }
          _scheduleReconnect();
        },
        onDone: () {
          if (mounted) {
            setState(() {
              _isConnected = false;
              _isReconnecting = false;
            });
            _updateHud();
            _audioControlChannel.invokeMethod('stop');
          }
          _scheduleReconnect();
        },
        cancelOnError: true,
      );
    } catch (e) {
      if (mounted) {
        setState(() {
          _isConnected = false;
          _isReconnecting = false;
        });
        _updateHud();
        _audioControlChannel.invokeMethod('stop');
      }
      _scheduleReconnect();
    }
  }

  void _scheduleReconnect() {
    if (mounted && !_isReconnecting) {
      Future.delayed(const Duration(seconds: 2), () {
        if (mounted && !_isConnected) {
          _connectWebSocket();
        }
      });
    }
  }

  void _handleBinaryFrame(Uint8List buffer) {
    try {
      // 1. Forward Opus audio packets (0xFA 0xFA prefix) to native low-latency decoder FIRST
      if (buffer.length >= 10 && buffer[0] == 0xFA && buffer[1] == 0xFA) {
        if (!_isAudioMuted && buffer.length > 10) {
          try {
            final opusBytes = Uint8List.fromList(buffer.sublist(10));
            _audioStreamChannel.send(ByteData.sublistView(opusBytes));
          } catch (_) {}
        }
        return;
      }

      if (buffer.length < 12) return;

      // Read real desktop dimensions & synchronized QPC timestamp from 12-byte packet header
      final bd = ByteData.sublistView(buffer, 0, 12);
      final timestamp = bd.getUint64(0, Endian.big);
      if (timestamp > 0 && _serverClockOffset != 0) {
        final hostNow = DateTime.now().millisecondsSinceEpoch + _serverClockOffset;
        final frameLatency = (hostNow - timestamp).clamp(1, 999);
        _latencyMs = frameLatency;
      }
      final w = bd.getUint16(8, Endian.big);
      final h = bd.getUint16(10, Endian.big);
      if (w > 0 && h > 0 && (_desktopWidth != w || _desktopHeight != h)) {
        _desktopWidth = w;
        _desktopHeight = h;
      }

      // Zero-allocation sublist view: shares memory directly
      final frameBytes = Uint8List.sublistView(buffer, 12);
      _frameNotifier.value = frameBytes;

      _fpsCount++;
      final nowTime = DateTime.now();
      if (nowTime.difference(_lastFpsTime).inMilliseconds >= 1000) {
        _displayFps = _fpsCount;
        _fpsCount = 0;
        _lastFpsTime = nowTime;
        _updateHud();
      }
    } catch (e) {
      debugPrint('Binary frame error: $e');
    }
  }

  int _lastFrameLossReportTime = 0;
  void _reportFrameLoss() {
    final now = DateTime.now().millisecondsSinceEpoch;
    if (now - _lastFrameLossReportTime >= 200) {
      _lastFrameLossReportTime = now;
      _sendInput({'type': 'frame_loss'});
    }
  }

  void _switchMode(String mode) {
    setState(() => _currentMode = mode);
    _sendInput({'type': 'set_mode', 'mode': mode});
  }

  void _setResolution(int height) {
    setState(() => _targetStreamHeight = height);
    _sendInput({'type': 'set_resolution', 'height': height});
  }

  void _setFps(int fps) {
    setState(() => _targetFps = fps);
    _sendInput({'type': 'set_fps', 'fps': fps});
  }

  void _setQuality(int quality) {
    setState(() => _targetQuality = quality);
    _sendInput({'type': 'set_quality', 'quality': quality});
  }

  void _showModeSelector() {
    final bool isLight = _isLightTheme;
    showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: isLight ? Colors.white : const Color(0xFF0F172A),
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(20)),
      ),
      builder: (ctx) => StatefulBuilder(
        builder: (context, setModalState) => SafeArea(
          child: SingleChildScrollView(
            physics: const BouncingScrollPhysics(),
            padding: const EdgeInsets.fromLTRB(16, 12, 16, 20),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // Header with Close Button
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Row(
                      children: [
                        Icon(Icons.wifi_tethering_rounded, size: 18, color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8)),
                        const SizedBox(width: 8),
                        Text(
                          'Chế Độ Băng Thông & Độ Phân Giải',
                          style: TextStyle(
                            fontSize: 15,
                            fontWeight: FontWeight.w700,
                            color: isLight ? const Color(0xFF0F172A) : Colors.white,
                          ),
                        ),
                      ],
                    ),
                    IconButton(
                      icon: Icon(Icons.close_rounded, color: isLight ? const Color(0xFF64748B) : Colors.white60, size: 18),
                      padding: EdgeInsets.zero,
                      constraints: const BoxConstraints(),
                      onPressed: () => Navigator.pop(ctx),
                    ),
                  ],
                ),
                const SizedBox(height: 12),

                // 6 Presets Grid (2 columns in mobile portrait, 3 in landscape)
                LayoutBuilder(
                  builder: (context, constraints) {
                    final int crossAxisCount = constraints.maxWidth > 580 ? 3 : 2;
                    final cards = [
                      _buildCompactModeCard(
                        mode: 'auto',
                        badge: 'Auto',
                        title: 'Tự thích ứng',
                        bandwidth: '~1 - 15 Mbps',
                        desc: 'Tự đổi nét & bitrate theo mạng',
                        icon: Icons.auto_mode_rounded,
                        color: const Color(0xFF0284C7),
                        onTap: () {
                          _switchMode('auto');
                          setModalState(() {});
                        },
                      ),
                      _buildCompactModeCard(
                        mode: 'eco',
                        badge: '540p',
                        title: 'Tiết kiệm (Eco)',
                        bandwidth: '~1.5 Mbps',
                        desc: 'Siêu nhẹ cho 4G / Wi-Fi yếu',
                        icon: Icons.bolt_rounded,
                        color: const Color(0xFF10B981),
                        onTap: () {
                          _switchMode('eco');
                          _targetStreamHeight = 540;
                          _targetFps = 60;
                          _targetQuality = 55;
                          setModalState(() {});
                        },
                      ),
                      _buildCompactModeCard(
                        mode: 'balanced',
                        badge: '720p',
                        title: 'HD 60 FPS',
                        bandwidth: '~3.5 Mbps',
                        desc: 'Cân bằng tối ưu (Khuyên dùng)',
                        icon: Icons.high_quality_rounded,
                        color: isLight ? const Color(0xFF0067C0) : const Color(0xFF2563EB),
                        onTap: () {
                          _switchMode('balanced');
                          _targetStreamHeight = 720;
                          _targetFps = 60;
                          _targetQuality = 70;
                          setModalState(() {});
                        },
                      ),
                      _buildCompactModeCard(
                        mode: 'quality',
                        badge: '1080p',
                        title: 'Full HD Sắc nét',
                        bandwidth: '~7 Mbps',
                        desc: 'Sắc nét 1:1 khi Wi-Fi 5GHz',
                        icon: Icons.hd_rounded,
                        color: const Color(0xFF8B5CF6),
                        onTap: () {
                          _switchMode('quality');
                          _targetStreamHeight = 1080;
                          _targetFps = 60;
                          _targetQuality = 80;
                          setModalState(() {});
                        },
                      ),
                      _buildCompactModeCard(
                        mode: 'ultra',
                        badge: 'Ultra',
                        title: 'Chất lượng tối đa',
                        bandwidth: '~12 Mbps',
                        desc: 'Đồ họa nguyên bản, nét từng pixel',
                        icon: Icons.diamond_rounded,
                        color: const Color(0xFFEC4899),
                        onTap: () {
                          _switchMode('ultra');
                          _targetStreamHeight = 1080;
                          _targetFps = 60;
                          _targetQuality = 92;
                          setModalState(() {});
                        },
                      ),
                      _buildCompactModeCard(
                        mode: '2k',
                        badge: '1440p (2K)',
                        title: 'QHD Siêu nét',
                        bandwidth: '~18 Mbps',
                        desc: 'Cho tablet & màn hình 2K',
                        icon: Icons.monitor_rounded,
                        color: const Color(0xFFF59E0B),
                        onTap: () {
                          _switchMode('2k');
                          _targetStreamHeight = 1440;
                          _targetFps = 60;
                          _targetQuality = 85;
                          setModalState(() {});
                        },
                      ),
                    ];

                    return GridView.count(
                      crossAxisCount: crossAxisCount,
                      shrinkWrap: true,
                      physics: const NeverScrollableScrollPhysics(),
                      crossAxisSpacing: 8,
                      mainAxisSpacing: 8,
                      childAspectRatio: crossAxisCount == 3 ? 1.65 : 1.75,
                      children: cards,
                    );
                  },
                ),

                const SizedBox(height: 16),
                // Detailed Manual Customization Section
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: isLight ? const Color(0xFFF8FAFC) : Colors.white.withOpacity(0.04),
                    borderRadius: BorderRadius.circular(14),
                    border: Border.all(
                      color: isLight ? const Color(0xFFE2E8F0) : Colors.white10,
                    ),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Icon(Icons.tune_rounded, size: 16, color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8)),
                          const SizedBox(width: 6),
                          Text(
                            'Tùy chỉnh thủ công (Nâng cao)',
                            style: TextStyle(
                              fontSize: 12.5,
                              fontWeight: FontWeight.w700,
                              color: isLight ? const Color(0xFF0F172A) : Colors.white,
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 10),

                      // Resolution Segmented Row
                      Row(
                        children: [
                          Text(
                            'Độ phân giải:',
                            style: TextStyle(
                              fontSize: 11.5,
                              fontWeight: FontWeight.w600,
                              color: isLight ? const Color(0xFF475569) : Colors.white70,
                            ),
                          ),
                          const Spacer(),
                          Wrap(
                            spacing: 4,
                            children: [540, 720, 1080, 1440].map((h) {
                              final bool isAct = _targetStreamHeight == h;
                              return InkWell(
                                onTap: () {
                                  HapticFeedback.selectionClick();
                                  _setResolution(h);
                                  setModalState(() {});
                                },
                                borderRadius: BorderRadius.circular(6),
                                child: Container(
                                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                                  decoration: BoxDecoration(
                                    color: isAct
                                        ? (isLight ? const Color(0xFF0067C0) : const Color(0xFF2563EB))
                                        : (isLight ? Colors.white : Colors.white10),
                                    borderRadius: BorderRadius.circular(6),
                                    border: Border.all(
                                      color: isAct
                                          ? Colors.transparent
                                          : (isLight ? const Color(0xFFCBD5E1) : Colors.white12),
                                    ),
                                  ),
                                  child: Text(
                                    '${h}p',
                                    style: TextStyle(
                                      fontSize: 11,
                                      fontWeight: FontWeight.w700,
                                      color: isAct ? Colors.white : (isLight ? const Color(0xFF334155) : Colors.white70),
                                    ),
                                  ),
                                ),
                              );
                            }).toList(),
                          ),
                        ],
                      ),

                      const SizedBox(height: 10),
                      // Target FPS Row
                      Row(
                        children: [
                          Text(
                            'Khung hình (FPS):',
                            style: TextStyle(
                              fontSize: 11.5,
                              fontWeight: FontWeight.w600,
                              color: isLight ? const Color(0xFF475569) : Colors.white70,
                            ),
                          ),
                          const Spacer(),
                          Wrap(
                            spacing: 4,
                            children: [30, 60].map((f) {
                              final bool isAct = _targetFps == f;
                              return InkWell(
                                onTap: () {
                                  HapticFeedback.selectionClick();
                                  _setFps(f);
                                  setModalState(() {});
                                },
                                borderRadius: BorderRadius.circular(6),
                                child: Container(
                                  padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                                  decoration: BoxDecoration(
                                    color: isAct
                                        ? const Color(0xFF10B981)
                                        : (isLight ? Colors.white : Colors.white10),
                                    borderRadius: BorderRadius.circular(6),
                                    border: Border.all(
                                      color: isAct
                                          ? Colors.transparent
                                          : (isLight ? const Color(0xFFCBD5E1) : Colors.white12),
                                    ),
                                  ),
                                  child: Text(
                                    '$f FPS',
                                    style: TextStyle(
                                      fontSize: 11,
                                      fontWeight: FontWeight.w700,
                                      color: isAct ? Colors.white : (isLight ? const Color(0xFF334155) : Colors.white70),
                                    ),
                                  ),
                                ),
                              );
                            }).toList(),
                          ),
                        ],
                      ),

                      const SizedBox(height: 10),
                      // Quality Slider Row
                      Row(
                        children: [
                          Text(
                            'Chất lượng ảnh: $_targetQuality%',
                            style: TextStyle(
                              fontSize: 11.5,
                              fontWeight: FontWeight.w600,
                              color: isLight ? const Color(0xFF475569) : Colors.white70,
                            ),
                          ),
                          Expanded(
                            child: SliderTheme(
                              data: SliderThemeData(
                                trackHeight: 3,
                                thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 6),
                                activeTrackColor: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                                inactiveTrackColor: isLight ? const Color(0xFFE2E8F0) : Colors.white12,
                                thumbColor: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                              ),
                              child: Slider(
                                value: _targetQuality.toDouble(),
                                min: 45,
                                max: 95,
                                divisions: 10,
                                onChanged: (v) {
                                  setModalState(() => _targetQuality = v.round());
                                },
                                onChangeEnd: (v) {
                                  _setQuality(v.round());
                                },
                              ),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildCompactModeCard({
    required String mode,
    required String badge,
    required String title,
    required String bandwidth,
    required String desc,
    required IconData icon,
    required Color color,
    required VoidCallback onTap,
  }) {
    final isSelected = _currentMode == mode;
    final bool isLight = _isLightTheme;
    final Color bgColor = isLight
        ? (isSelected ? const Color(0xFFEFF6FF) : const Color(0xFFF8FAFC))
        : (isSelected ? color.withOpacity(0.18) : const Color(0xFF1E293B).withOpacity(0.7));
    final Color borderColor = isSelected
        ? color
        : (isLight ? const Color(0xFFE2E8F0) : Colors.white.withOpacity(0.1));

    return Material(
      color: bgColor,
      borderRadius: BorderRadius.circular(12),
      child: InkWell(
        onTap: () {
          HapticFeedback.selectionClick();
          onTap();
          Navigator.pop(context);
        },
        borderRadius: BorderRadius.circular(12),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(12),
            border: Border.all(
              color: borderColor,
              width: isSelected ? 1.5 : 1.0,
            ),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Container(
                    padding: const EdgeInsets.all(4),
                    decoration: BoxDecoration(
                      color: isLight ? color.withOpacity(0.12) : color.withOpacity(0.2),
                      borderRadius: BorderRadius.circular(7),
                    ),
                    child: Icon(icon, color: color, size: 15),
                  ),
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                    decoration: BoxDecoration(
                      color: isSelected
                          ? color
                          : (isLight ? const Color(0xFFE2E8F0) : Colors.white10),
                      borderRadius: BorderRadius.circular(5),
                    ),
                    child: Text(
                      badge,
                      style: TextStyle(
                        fontSize: 9.5,
                        fontWeight: FontWeight.w700,
                        color: isSelected
                            ? Colors.white
                            : (isLight ? const Color(0xFF475569) : Colors.white70),
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 5),
              Text(
                title,
                style: TextStyle(
                  fontSize: 11.5,
                  fontWeight: isSelected ? FontWeight.w700 : FontWeight.w600,
                  color: isLight ? const Color(0xFF0F172A) : Colors.white,
                ),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
              const SizedBox(height: 1),
              Row(
                children: [
                  Text(
                    bandwidth,
                    style: TextStyle(fontSize: 10, fontWeight: FontWeight.w600, color: color),
                  ),
                  if (isSelected) ...[
                    const Spacer(),
                    Icon(Icons.check_circle_rounded, size: 13, color: color),
                  ],
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _sendInput(Map<String, dynamic> payload) {
    if (_socket != null && _socket!.readyState == WebSocket.open) {
      _socket!.add(jsonEncode(payload));
    }
  }

  void _sendShortcut(List<String> keys) {
    _sendInput({'type': 'shortcut', 'keys': keys});
  }

  void _sendKey(String key) {
    _sendInput({'type': 'key_click', 'key': key});
  }

  Offset? _toNormalizedCoordinates(Offset localPosition, Size containerSize) {
    if (containerSize.width <= 0 || containerSize.height <= 0) return null;

    final double desktopAspect = _desktopHeight > 0 ? (_desktopWidth / _desktopHeight) : (16.0 / 9.0);
    final double containerAspect = containerSize.width / containerSize.height;

    double renderWidth;
    double renderHeight;
    double offsetX;
    double offsetY;

    if (containerAspect > desktopAspect) {
      renderHeight = containerSize.height;
      renderWidth = renderHeight * desktopAspect;
      offsetX = (containerSize.width - renderWidth) / 2.0;
      offsetY = 0.0;
    } else {
      renderWidth = containerSize.width;
      renderHeight = renderWidth / desktopAspect;
      offsetX = 0.0;
      offsetY = (containerSize.height - renderHeight) / 2.0;
    }

    final double relX = (localPosition.dx - offsetX) / renderWidth;
    final double relY = (localPosition.dy - offsetY) / renderHeight;

    if (relX < 0.0 || relX > 1.0 || relY < 0.0 || relY > 1.0) {
      return null;
    }
    return Offset(relX.clamp(0.0, 1.0), relY.clamp(0.0, 1.0));
  }

  String? _mapKeyLabel(LogicalKeyboardKey key) {
    if (key == LogicalKeyboardKey.enter || key == LogicalKeyboardKey.numpadEnter) return 'enter';
    if (key == LogicalKeyboardKey.backspace) return 'backspace';
    if (key == LogicalKeyboardKey.escape) return 'escape';
    if (key == LogicalKeyboardKey.tab) return 'tab';
    if (key == LogicalKeyboardKey.space) return 'space';
    if (key == LogicalKeyboardKey.delete) return 'delete';
    if (key == LogicalKeyboardKey.arrowUp) return 'up';
    if (key == LogicalKeyboardKey.arrowDown) return 'down';
    if (key == LogicalKeyboardKey.arrowLeft) return 'left';
    if (key == LogicalKeyboardKey.arrowRight) return 'right';
    if (key == LogicalKeyboardKey.controlLeft || key == LogicalKeyboardKey.controlRight) return 'ctrl';
    if (key == LogicalKeyboardKey.shiftLeft || key == LogicalKeyboardKey.shiftRight) return 'shift';
    if (key == LogicalKeyboardKey.altLeft || key == LogicalKeyboardKey.altRight) return 'alt';
    if (key == LogicalKeyboardKey.metaLeft || key == LogicalKeyboardKey.metaRight) return 'win';
    if (key == LogicalKeyboardKey.home) return 'home';
    if (key == LogicalKeyboardKey.end) return 'end';
    if (key == LogicalKeyboardKey.pageUp) return 'pageup';
    if (key == LogicalKeyboardKey.pageDown) return 'pagedown';
    if (key == LogicalKeyboardKey.f1) return 'f1';
    if (key == LogicalKeyboardKey.f2) return 'f2';
    if (key == LogicalKeyboardKey.f3) return 'f3';
    if (key == LogicalKeyboardKey.f4) return 'f4';
    if (key == LogicalKeyboardKey.f5) return 'f5';
    if (key == LogicalKeyboardKey.f6) return 'f6';
    if (key == LogicalKeyboardKey.f7) return 'f7';
    if (key == LogicalKeyboardKey.f8) return 'f8';
    if (key == LogicalKeyboardKey.f9) return 'f9';
    if (key == LogicalKeyboardKey.f10) return 'f10';
    if (key == LogicalKeyboardKey.f11) return 'f11';
    if (key == LogicalKeyboardKey.f12) return 'f12';
    if (key.keyLabel.isNotEmpty && key.keyLabel.length == 1) {
      return key.keyLabel.toLowerCase();
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    return Focus(
      focusNode: _hardwareFocusNode,
      autofocus: true,
      onKeyEvent: (node, event) {
        if (event is KeyDownEvent) {
          final label = _mapKeyLabel(event.logicalKey);
          if (label != null) {
            _sendInput({'type': 'key_down', 'key': label});
            return KeyEventResult.handled;
          }
        } else if (event is KeyUpEvent) {
          final label = _mapKeyLabel(event.logicalKey);
          if (label != null) {
            _sendInput({'type': 'key_up', 'key': label});
            return KeyEventResult.handled;
          }
        }
        return KeyEventResult.ignored;
      },
      child: Scaffold(
        backgroundColor: Colors.black,
        body: Stack(
          children: [
            // Stream Display & Interactive Gesture Layer
            LayoutBuilder(
              builder: (context, constraints) {
                final renderSize = Size(constraints.maxWidth, constraints.maxHeight);
                return Listener(
                  onPointerHover: (event) {
                    final norm = _toNormalizedCoordinates(event.localPosition, renderSize);
                    if (norm != null) {
                      _cursorNotifier.value = norm;
                      _sendInput({'type': 'mouse_move', 'x': norm.dx, 'y': norm.dy});
                    }
                  },
                  onPointerDown: (event) {
                    _refocusHardwareKeyboard();
                    if (event.kind == PointerDeviceKind.mouse) {
                      final norm = _toNormalizedCoordinates(event.localPosition, renderSize);
                      final btn = event.buttons == 2 ? 'right' : event.buttons == 4 ? 'middle' : 'left';
                      _sendInput({
                        'type': 'mouse_down',
                        'button': btn,
                        if (norm != null) 'x': norm.dx,
                        if (norm != null) 'y': norm.dy,
                      });
                    } else if (event.kind == PointerDeviceKind.touch) {
                      _activePointerCount++;
                      _activePointers[event.pointer] = event.localPosition;
                      if (_activePointerCount == 1) {
                        _primaryPointerDownTime = DateTime.now();
                        _primaryPointerDownPos = event.localPosition;
                        _primaryPointerMoved = false;
                        _accumulatedDx = 0.0;
                        _accumulatedDy = 0.0;

                        _longPressTimer?.cancel();
                        _longPressTimer = Timer(const Duration(milliseconds: 520), () {
                          if (mounted && _activePointerCount == 1 && !_primaryPointerMoved) {
                            HapticFeedback.heavyImpact();
                            if (_isDirectTouch) {
                              final scenePoint = _toSceneCoordinates(event.localPosition, renderSize);
                              final norm = _toNormalizedCoordinates(scenePoint, renderSize);
                              if (norm != null) {
                                _sendInput({'type': 'mouse_click', 'button': 'right', 'x': norm.dx, 'y': norm.dy});
                              }
                            } else {
                              _sendInput({'type': 'mouse_click', 'button': 'right'});
                            }
                            _primaryPointerMoved = true;
                          }
                        });
                      } else {
                        _longPressTimer?.cancel();
                      }
                    }
                  },
                  onPointerMove: (event) {
                    if (event.kind == PointerDeviceKind.mouse) {
                      final norm = _toNormalizedCoordinates(event.localPosition, renderSize);
                      if (norm != null) {
                        _cursorNotifier.value = norm;
                        _sendInput({'type': 'mouse_move', 'x': norm.dx, 'y': norm.dy});
                      }
                    } else if (event.kind == PointerDeviceKind.touch) {
                      final lastPos = _activePointers[event.pointer];
                      _activePointers[event.pointer] = event.localPosition;

                      if (_activePointerCount == 1 && lastPos != null) {
                        final delta = event.localPosition - lastPos;
                        final totalDist = _primaryPointerDownPos != null
                            ? (event.localPosition - _primaryPointerDownPos!).distance
                            : 0.0;
                        if (totalDist > 5.0) {
                          _primaryPointerMoved = true;
                          _longPressTimer?.cancel();
                        }

                        if (_isDirectTouch) {
                          if (_primaryPointerMoved) {
                            final scenePoint = _toSceneCoordinates(event.localPosition, renderSize);
                            final norm = _toNormalizedCoordinates(scenePoint, renderSize);
                            if (norm != null) {
                              _cursorNotifier.value = norm;
                              _sendInput({'type': 'mouse_move', 'x': norm.dx, 'y': norm.dy});
                            }
                          }
                        } else {
                          // BUTTERY SMOOTH 120 FPS TRACKPAD WITH ZERO NETWORK LATENCY
                          final dist = delta.distance;
                          double accel = 1.0;
                          if (_enableMouseAccel && dist > 1.5) {
                            accel = (1.0 + (dist - 1.5) * 0.075).clamp(1.0, 3.2);
                          }
                          final double speedFactor = _mouseSpeed * 0.095;
                          final double movePxX = delta.dx * speedFactor * accel;
                          final double movePxY = delta.dy * speedFactor * accel;

                          // 1. Instant local client cursor update (0ms lag!)
                          final currentPos = _cursorNotifier.value ?? const Offset(0.5, 0.5);
                          final double normDx = movePxX / (_desktopWidth > 0 ? _desktopWidth : 1920);
                          final double normDy = movePxY / (_desktopHeight > 0 ? _desktopHeight : 1080);
                          _cursorNotifier.value = Offset(
                            (currentPos.dx + normDx).clamp(0.0, 1.0),
                            (currentPos.dy + normDy).clamp(0.0, 1.0),
                          );

                          // 2. Transmit delta with sub-pixel accumulator
                          _accumulatedDx += movePxX;
                          _accumulatedDy += movePxY;
                          final int sendDx = _accumulatedDx.round();
                          final int sendDy = _accumulatedDy.round();
                          if (sendDx != 0 || sendDy != 0) {
                            _accumulatedDx -= sendDx;
                            _accumulatedDy -= sendDy;
                            _sendInput({'type': 'mouse_delta', 'dx': sendDx, 'dy': sendDy});
                          }
                        }
                      }
                    }
                  },
                  onPointerUp: (event) {
                    if (event.kind == PointerDeviceKind.mouse) {
                      final norm = _toNormalizedCoordinates(event.localPosition, renderSize);
                      final btn = event.buttons == 2 ? 'right' : event.buttons == 4 ? 'middle' : 'left';
                      _sendInput({
                        'type': 'mouse_up',
                        'button': btn,
                        if (norm != null) 'x': norm.dx,
                        if (norm != null) 'y': norm.dy,
                      });
                    } else if (event.kind == PointerDeviceKind.touch) {
                      _longPressTimer?.cancel();
                      _activePointers.remove(event.pointer);
                      final int prevCount = _activePointerCount;
                      _activePointerCount = math.max(0, _activePointerCount - 1);

                      if (prevCount == 1 && !_primaryPointerMoved) {
                        final downTime = _primaryPointerDownTime;
                        if (downTime != null && DateTime.now().difference(downTime).inMilliseconds < 400) {
                          HapticFeedback.selectionClick();
                          if (_isDirectTouch) {
                            final scenePoint = _toSceneCoordinates(event.localPosition, renderSize);
                            final norm = _toNormalizedCoordinates(scenePoint, renderSize);
                            if (norm != null) {
                              _sendInput({'type': 'mouse_click', 'button': 'left', 'x': norm.dx, 'y': norm.dy});
                            }
                          } else {
                            _sendInput({'type': 'mouse_click', 'button': 'left'});
                          }
                        }
                      } else if (prevCount == 2 && !_primaryPointerMoved) {
                        // 2-finger double tap detection to recall/toggle HUD
                        final now = DateTime.now();
                        if (_lastTwoFingerTapTime != null && now.difference(_lastTwoFingerTapTime!).inMilliseconds < 450) {
                          HapticFeedback.mediumImpact();
                          setState(() => _isHudCollapsed = !_isHudCollapsed);
                          if (!_isHudCollapsed) _resetIslandTimer();
                          _lastTwoFingerTapTime = null;
                        } else {
                          _lastTwoFingerTapTime = now;
                        }
                      }
                    }
                  },
                  onPointerCancel: (event) {
                    _longPressTimer?.cancel();
                    _activePointers.remove(event.pointer);
                    _activePointerCount = math.max(0, _activePointerCount - 1);
                  },
                  onPointerSignal: (event) {
                    if (event is PointerScrollEvent) {
                      _sendInput({
                        'type': 'mouse_wheel',
                        'delta_x': event.scrollDelta.dx.round(),
                        'delta_y': -event.scrollDelta.dy.round(),
                      });
                    }
                  },
                  child: GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onDoubleTap: () {
                      if (_isHudCollapsed) {
                        HapticFeedback.lightImpact();
                        _resetIslandTimer(forceOpen: true);
                      }
                    },
                    onScaleStart: (details) {
                      _baseScale = _zoomScale;
                      _lastPanOffset = details.focalPoint;
                    },
                    onScaleUpdate: (details) {
                      final delta = details.focalPoint - _lastPanOffset;
                      _lastPanOffset = details.focalPoint;

                      if (details.pointerCount == 2) {
                        // Pinch to zoom in/out & pan viewport when zoomed
                        final newScale = (_baseScale * details.scale).clamp(1.0, 5.0);
                        if (newScale > 1.01) {
                          final maxPanX = math.max(0.0, (renderSize.width * (newScale - 1.0)) / 2.0);
                          final maxPanY = math.max(0.0, (renderSize.height * (newScale - 1.0)) / 2.0);
                          setState(() {
                            _zoomScale = newScale;
                            _zoomPan = Offset(
                              (_zoomPan.dx + delta.dx).clamp(-maxPanX, maxPanX),
                              (_zoomPan.dy + delta.dy).clamp(-maxPanY, maxPanY),
                            );
                          });
                        } else {
                          setState(() {
                            _zoomScale = 1.0;
                            _zoomPan = Offset.zero;
                          });
                          final dy = (delta.dy * 2.5).round();
                          if (dy != 0) {
                            _sendInput({'type': 'mouse_wheel', 'delta_x': 0, 'delta_y': dy});
                          }
                        }
                      }
                    },
                    child: Transform(
                      transform: Matrix4.identity()
                        ..translate(renderSize.width / 2.0 + _zoomPan.dx, renderSize.height / 2.0 + _zoomPan.dy)
                        ..scale(_zoomScale, _zoomScale)
                        ..translate(-renderSize.width / 2.0, -renderSize.height / 2.0),
                      alignment: Alignment.topLeft,
                      child: Stack(
                        fit: StackFit.expand,
                        children: [
                          Center(
                            child: ValueListenableBuilder<Uint8List?>(
                              valueListenable: _frameNotifier,
                              builder: (context, frame, _) {
                                if (frame == null) {
                                  return const Center(
                                    child: CircularProgressIndicator(color: Color(0xFF38BDF8)),
                                  );
                                }
                                return Image.memory(
                                  frame,
                                  gaplessPlayback: true,
                                  fit: BoxFit.contain,
                                  errorBuilder: (context, error, stackTrace) {
                                    _reportFrameLoss();
                                    return const SizedBox.shrink();
                                  },
                                );
                              },
                            ),
                          ),
                          // Local Hardware Cursor Overlay (Calibrated to default Windows UI scale)
                          ValueListenableBuilder<Offset?>(
                            valueListenable: _cursorNotifier,
                            builder: (context, cursorNorm, _) {
                              if (cursorNorm == null) return const SizedBox.shrink();
                              final double desktopAspect = _desktopHeight > 0 ? (_desktopWidth / _desktopHeight) : (16.0 / 9.0);
                              final double containerAspect = renderSize.width / renderSize.height;
                              double rw, rh, ox, oy;
                              if (containerAspect > desktopAspect) {
                                rh = renderSize.height;
                                rw = rh * desktopAspect;
                                ox = (renderSize.width - rw) / 2.0;
                                oy = 0.0;
                              } else {
                                rw = renderSize.width;
                                rh = rw / desktopAspect;
                                ox = 0.0;
                                oy = (renderSize.height - rh) / 2.0;
                              }
                              final cx = ox + cursorNorm.dx * rw;
                              final cy = oy + cursorNorm.dy * rh;
                              final double cursorH = _calculateCursorHeight(rh);
                              final double cursorW = cursorH * 0.62;
                              return Positioned(
                                left: cx,
                                top: cy,
                                child: IgnorePointer(
                                  child: CustomPaint(
                                    size: Size(cursorW, cursorH),
                                    painter: const HardwareCursorPainter(),
                                  ),
                                ),
                              );
                            },
                          ),
                        ],
                      ),
                    ),
                  ),
                );
              },
            ),

            // Top Host Desktop Lock Status Banner
            if (_isDesktopLocked)
              SafeArea(
                child: Align(
                  alignment: Alignment.topCenter,
                  child: Container(
                    margin: const EdgeInsets.only(top: 50),
                    padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
                    decoration: BoxDecoration(
                      color: const Color(0xFFEF4444).withOpacity(0.95),
                      borderRadius: BorderRadius.circular(12),
                      boxShadow: [
                        BoxShadow(color: Colors.black.withOpacity(0.25), blurRadius: 12, offset: const Offset(0, 3)),
                      ],
                    ),
                    child: const Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(Icons.lock_rounded, color: Colors.white, size: 16),
                        SizedBox(width: 8),
                        Text(
                          'Máy host đang khóa — Chỉ xem được (UIPI chặn thao tác từ xa)',
                          style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: Colors.white),
                        ),
                      ],
                    ),
                  ),
                ),
              ),

            // Edge Swipe Detectors: Invisible edge touch zones to pull down/in the HUD
            Positioned(
              top: 0,
              left: 0,
              right: 0,
              height: 32,
              child: GestureDetector(
                behavior: HitTestBehavior.translucent,
                onVerticalDragUpdate: (details) {
                  if (details.primaryDelta != null && details.primaryDelta! > 3) {
                    HapticFeedback.lightImpact();
                    _resetIslandTimer(forceOpen: true);
                  }
                },
              ),
            ),
            Positioned(
              left: 0,
              top: 50,
              bottom: 50,
              width: 24,
              child: GestureDetector(
                behavior: HitTestBehavior.translucent,
                onHorizontalDragUpdate: (details) {
                  if (details.primaryDelta != null && details.primaryDelta! > 3) {
                    HapticFeedback.lightImpact();
                    _resetIslandTimer(forceOpen: true);
                  }
                },
              ),
            ),
            Positioned(
              right: 0,
              top: 50,
              bottom: 50,
              width: 24,
              child: GestureDetector(
                behavior: HitTestBehavior.translucent,
                onHorizontalDragUpdate: (details) {
                  if (details.primaryDelta != null && details.primaryDelta! < -3) {
                    HapticFeedback.lightImpact();
                    _resetIslandTimer(forceOpen: true);
                  }
                },
              ),
            ),

            // Zero-Occlusion Smart Dockable HUD (Hides 100% off-screen when idle or swiped away)
            _buildSmartDockableHud(),

            // Horizontal Windows Keyboard Accessory Bar
            _buildHorizontalKeyboardBar(),

            // Dedicated Floating Draggable IME Button (Corner placed, draggable & toggleable)
            _buildFloatingImeButton(MediaQuery.of(context).size),

            // Hidden Input for Soft Keyboard (Standard, normal keyboard - not secure/incognito)
            Positioned(
              left: -100,
              top: -100,
              child: SizedBox(
                width: 1,
                height: 1,
                child: TextField(
                  focusNode: _keyboardFocusNode,
                  controller: _keyboardController,
                  keyboardType: TextInputType.text,
                  enableSuggestions: true,
                  autocorrect: true,
                  textCapitalization: TextCapitalization.none,
                  textInputAction: TextInputAction.send,
                  onSubmitted: (text) {
                    _sendKey('enter');
                  },
                  onChanged: (text) {
                    if (text.isNotEmpty) {
                      _sendInput({'type': 'text', 'text': text});
                      _keyboardController.clear();
                    }
                  },
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildSmartDockableHud() {
    Offset slideOffset;
    switch (_hudDockEdge) {
      case HudDockEdge.top:
        slideOffset = _isHudCollapsed ? const Offset(0, -1.6) : Offset.zero;
        break;
      case HudDockEdge.left:
        slideOffset = _isHudCollapsed ? const Offset(-1.6, 0) : Offset.zero;
        break;
      case HudDockEdge.right:
        slideOffset = _isHudCollapsed ? const Offset(1.6, 0) : Offset.zero;
        break;
    }

    final bool isLight = _isLightTheme;
    final capsule = GestureDetector(
      onVerticalDragEnd: (details) {
        if (_hudDockEdge == HudDockEdge.top && details.primaryVelocity != null && details.primaryVelocity! < -100) {
          HapticFeedback.selectionClick();
          setState(() => _isHudCollapsed = true);
        }
      },
      onHorizontalDragEnd: (details) {
        if (details.primaryVelocity != null) {
          if (details.primaryVelocity! < -200) {
            HapticFeedback.selectionClick();
            if (_hudDockEdge == HudDockEdge.left) {
              setState(() => _isHudCollapsed = true);
            } else {
              setState(() => _hudDockEdge = HudDockEdge.left);
            }
          } else if (details.primaryVelocity! > 200) {
            HapticFeedback.selectionClick();
            if (_hudDockEdge == HudDockEdge.right) {
              setState(() => _isHudCollapsed = true);
            } else {
              setState(() => _hudDockEdge = HudDockEdge.right);
            }
          }
        }
      },
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: isLight ? const Color(0xF7FFFFFF) : const Color(0xFF0F172A).withOpacity(0.94),
          borderRadius: BorderRadius.circular(22),
          border: Border.all(
            color: isLight ? const Color(0x1A000000) : Colors.white.withOpacity(0.14),
          ),
          boxShadow: [
            BoxShadow(
              color: isLight ? const Color(0x18000000) : Colors.black.withOpacity(0.45),
              blurRadius: 16,
              offset: const Offset(0, 4),
            ),
          ],
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              width: 8,
              height: 8,
              decoration: BoxDecoration(
                color: _isConnected ? const Color(0xFF10B981) : const Color(0xFFF59E0B),
                shape: BoxShape.circle,
              ),
            ),
            const SizedBox(width: 8),
            ValueListenableBuilder<String>(
              valueListenable: _hudNotifier,
              builder: (context, hudText, _) {
                return Text(
                  _isConnected ? hudText : 'Connecting...',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: _isConnected
                        ? (isLight ? const Color(0xFF0F172A) : const Color(0xFFF8FAFC))
                        : const Color(0xFFF59E0B),
                  ),
                );
              },
            ),

            // Integrated Zoom Indicator & 1-Tap Reset Button (shown only when zoomed)
            if (_zoomScale > 1.05) ...[
              const SizedBox(width: 8),
              InkWell(
                onTap: () {
                  HapticFeedback.selectionClick();
                  setState(() {
                    _zoomScale = 1.0;
                    _zoomPan = Offset.zero;
                  });
                  _resetIslandTimer();
                },
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 3),
                  decoration: BoxDecoration(
                    color: isLight ? const Color(0xFFE0F2FE) : const Color(0xFF0284C7).withOpacity(0.3),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: isLight ? const Color(0xFF38BDF8) : const Color(0xFF38BDF8).withOpacity(0.6)),
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.zoom_in_rounded, size: 13, color: isLight ? const Color(0xFF0284C7) : const Color(0xFF38BDF8)),
                      const SizedBox(width: 3),
                      Text(
                        '${_zoomScale.toStringAsFixed(1)}x Thu nhỏ',
                        style: TextStyle(
                          fontSize: 10,
                          fontWeight: FontWeight.w700,
                          color: isLight ? const Color(0xFF0284C7) : const Color(0xFF38BDF8),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ],

            const SizedBox(width: 8),
            // Mode Pill
            InkWell(
              onTap: () {
                _resetIslandTimer(forceOpen: true);
                _showModeSelector();
              },
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 3),
                decoration: BoxDecoration(
                  color: isLight ? const Color(0xFFEFF6FF) : Colors.white.withOpacity(0.08),
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: isLight ? const Color(0xFFBFDBFE) : Colors.white.withOpacity(0.1)),
                ),
                child: Text(
                  _getModeLabel(),
                  style: TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w700,
                    color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            // Audio Mute Toggle
            InkWell(
              onTap: () {
                HapticFeedback.selectionClick();
                _toggleAudioMute();
                _resetIslandTimer(forceOpen: true);
              },
              child: Icon(
                !_isAudioMuted ? Icons.volume_up_rounded : Icons.volume_off_rounded,
                size: 16,
                color: !_isAudioMuted ? const Color(0xFF10B981) : const Color(0xFFEF4444),
              ),
            ),
            const SizedBox(width: 8),
            // Toggle Horizontal Keyboard Toolbar
            InkWell(
              onTap: () {
                HapticFeedback.selectionClick();
                setState(() => _isKeyboardBarVisible = !_isKeyboardBarVisible);
                _resetIslandTimer(forceOpen: true);
              },
              child: Container(
                padding: const EdgeInsets.all(3),
                decoration: BoxDecoration(
                  color: _isKeyboardBarVisible
                      ? (isLight ? const Color(0xFFE0F2FE) : const Color(0xFF0284C7).withOpacity(0.35))
                      : Colors.transparent,
                  borderRadius: BorderRadius.circular(6),
                ),
                child: Icon(
                  Icons.keyboard_rounded,
                  size: 16,
                  color: _isKeyboardBarVisible
                      ? (isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8))
                      : (isLight ? const Color(0xFF475569) : Colors.white70),
                ),
              ),
            ),
            const SizedBox(width: 8),
            // Direct Touch vs Trackpad
            InkWell(
              onTap: () {
                HapticFeedback.selectionClick();
                setState(() => _isDirectTouch = !_isDirectTouch);
                _resetIslandTimer(forceOpen: true);
                ScaffoldMessenger.of(context).removeCurrentSnackBar();
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(
                    content: Text(
                      _isDirectTouch ? '👉 Đã chuyển sang: Chạm trực tiếp' : '🖱️ Đã chuyển sang: Bàn rê chuột Trackpad (${_mouseSpeed.toStringAsFixed(1)}x)',
                      style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600),
                    ),
                    duration: const Duration(milliseconds: 1400),
                    behavior: SnackBarBehavior.floating,
                    backgroundColor: isLight ? const Color(0xFF0F172A) : const Color(0xFF0F172A).withOpacity(0.95),
                    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                  ),
                );
              },
              child: Container(
                padding: const EdgeInsets.all(3),
                decoration: BoxDecoration(
                  color: !_isDirectTouch
                      ? (isLight ? const Color(0xFFE0F2FE) : const Color(0xFF0284C7).withOpacity(0.3))
                      : Colors.transparent,
                  borderRadius: BorderRadius.circular(6),
                ),
                child: Icon(
                  _isDirectTouch ? Icons.touch_app_rounded : Icons.mouse_rounded,
                  size: 16,
                  color: !_isDirectTouch
                      ? (isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8))
                      : (isLight ? const Color(0xFF475569) : Colors.white70),
                ),
              ),
            ),
            const SizedBox(width: 8),
            // Quick Tools & Actions Launcher (replaces floating FAB)
            InkWell(
              onTap: () {
                HapticFeedback.mediumImpact();
                _resetIslandTimer(forceOpen: true);
                _showQuickActionsModal();
              },
              child: const Icon(
                Icons.flash_on_rounded,
                size: 16,
                color: Color(0xFFD97706),
              ),
            ),
            const SizedBox(width: 8),
            // Edge Dock Switcher (Top -> Right -> Left -> Top)
            InkWell(
              onTap: () {
                HapticFeedback.selectionClick();
                setState(() {
                  if (_hudDockEdge == HudDockEdge.top) {
                    _hudDockEdge = HudDockEdge.right;
                  } else if (_hudDockEdge == HudDockEdge.right) {
                    _hudDockEdge = HudDockEdge.left;
                  } else {
                    _hudDockEdge = HudDockEdge.top;
                  }
                });
                _resetIslandTimer(forceOpen: true);
              },
              child: Icon(
                _hudDockEdge == HudDockEdge.top
                    ? Icons.vertical_align_top_rounded
                    : _hudDockEdge == HudDockEdge.left
                        ? Icons.arrow_back_rounded
                        : Icons.arrow_forward_rounded,
                size: 15,
                color: isLight ? const Color(0xFF64748B) : Colors.white60,
              ),
            ),
            const SizedBox(width: 8),
            // Instant Hide Button
            InkWell(
              onTap: () {
                HapticFeedback.selectionClick();
                setState(() => _isHudCollapsed = true);
              },
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 3),
                decoration: BoxDecoration(
                  color: isLight ? const Color(0xFFF1F5F9) : Colors.white.withOpacity(0.08),
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(color: isLight ? const Color(0xFFE2E8F0) : Colors.white.withOpacity(0.12)),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.keyboard_arrow_up_rounded, size: 14, color: isLight ? const Color(0xFF475569) : Colors.white70),
                    const SizedBox(width: 2),
                    Text('Ẩn', style: TextStyle(fontSize: 10.5, fontWeight: FontWeight.w600, color: isLight ? const Color(0xFF475569) : Colors.white70)),
                  ],
                ),
              ),
            ),
            const SizedBox(width: 8),
            // Close Session
            InkWell(
              onTap: () => Navigator.pop(context),
              child: Icon(Icons.close_rounded, size: 16, color: isLight ? const Color(0xFF64748B) : Colors.white60),
            ),
          ],
        ),
      ),
    );

    Widget positionedCapsule;
    if (_hudDockEdge == HudDockEdge.top) {
      positionedCapsule = SafeArea(
        child: Align(
          alignment: Alignment.topCenter,
          child: Padding(
            padding: const EdgeInsets.only(top: 8),
            child: capsule,
          ),
        ),
      );
    } else if (_hudDockEdge == HudDockEdge.left) {
      positionedCapsule = SafeArea(
        child: Align(
          alignment: Alignment.topLeft,
          child: Padding(
            padding: const EdgeInsets.only(left: 10, top: 40),
            child: capsule,
          ),
        ),
      );
    } else {
      positionedCapsule = SafeArea(
        child: Align(
          alignment: Alignment.topRight,
          child: Padding(
            padding: const EdgeInsets.only(right: 10, top: 40),
            child: capsule,
          ),
        ),
      );
    }

    Widget pullBar;
    if (_hudDockEdge == HudDockEdge.top) {
      pullBar = SafeArea(
        child: Align(
          alignment: Alignment.topCenter,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () {
              HapticFeedback.selectionClick();
              _resetIslandTimer(forceOpen: true);
            },
            onVerticalDragUpdate: (details) {
              if (details.primaryDelta != null && details.primaryDelta! > 2) {
                HapticFeedback.lightImpact();
                _resetIslandTimer(forceOpen: true);
              }
            },
            child: Container(
              margin: const EdgeInsets.only(top: 4),
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
              decoration: BoxDecoration(
                color: isLight ? const Color(0xF6FFFFFF) : const Color(0xF00F172A),
                borderRadius: BorderRadius.circular(16),
                border: Border.all(
                  color: isLight ? const Color(0x22000000) : Colors.white.withOpacity(0.18),
                  width: 1.0,
                ),
                boxShadow: [
                  BoxShadow(
                    color: isLight ? Colors.black.withOpacity(0.12) : Colors.black.withOpacity(0.45),
                    blurRadius: 10,
                    offset: const Offset(0, 3),
                  ),
                ],
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 7,
                    height: 7,
                    decoration: BoxDecoration(
                      color: _isConnected ? const Color(0xFF10B981) : const Color(0xFFF59E0B),
                      shape: BoxShape.circle,
                    ),
                  ),
                  const SizedBox(width: 6),
                  ValueListenableBuilder<String>(
                    valueListenable: _hudNotifier,
                    builder: (context, hudText, _) => Text(
                      hudText,
                      style: TextStyle(
                        fontSize: 10.5,
                        fontWeight: FontWeight.w600,
                        color: isLight ? const Color(0xFF334155) : Colors.white70,
                      ),
                    ),
                  ),
                  const SizedBox(width: 4),
                  Icon(
                    Icons.keyboard_arrow_down_rounded,
                    size: 16,
                    color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                  ),
                ],
              ),
            ),
          ),
        ),
      );
    } else if (_hudDockEdge == HudDockEdge.left) {
      pullBar = SafeArea(
        child: Align(
          alignment: Alignment.centerLeft,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () {
              HapticFeedback.selectionClick();
              _resetIslandTimer(forceOpen: true);
            },
            onHorizontalDragUpdate: (details) {
              if (details.primaryDelta != null && details.primaryDelta! > 2) {
                HapticFeedback.lightImpact();
                _resetIslandTimer(forceOpen: true);
              }
            },
            child: Container(
              margin: const EdgeInsets.only(left: 4),
              padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 8),
              decoration: BoxDecoration(
                color: isLight ? const Color(0xF6FFFFFF) : const Color(0xF00F172A),
                borderRadius: BorderRadius.circular(14),
                border: Border.all(
                  color: isLight ? const Color(0x22000000) : Colors.white.withOpacity(0.18),
                  width: 1.0,
                ),
                boxShadow: [
                  BoxShadow(
                    color: isLight ? Colors.black.withOpacity(0.12) : Colors.black.withOpacity(0.45),
                    blurRadius: 8,
                    offset: const Offset(2, 0),
                  ),
                ],
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 7,
                    height: 7,
                    decoration: BoxDecoration(
                      color: _isConnected ? const Color(0xFF10B981) : const Color(0xFFF59E0B),
                      shape: BoxShape.circle,
                    ),
                  ),
                  const SizedBox(height: 6),
                  Icon(
                    Icons.chevron_right_rounded,
                    size: 16,
                    color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                  ),
                ],
              ),
            ),
          ),
        ),
      );
    } else {
      pullBar = SafeArea(
        child: Align(
          alignment: Alignment.centerRight,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () {
              HapticFeedback.selectionClick();
              _resetIslandTimer(forceOpen: true);
            },
            onHorizontalDragUpdate: (details) {
              if (details.primaryDelta != null && details.primaryDelta! < -2) {
                HapticFeedback.lightImpact();
                _resetIslandTimer(forceOpen: true);
              }
            },
            child: Container(
              margin: const EdgeInsets.only(right: 4),
              padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 8),
              decoration: BoxDecoration(
                color: isLight ? const Color(0xF6FFFFFF) : const Color(0xF00F172A),
                borderRadius: BorderRadius.circular(14),
                border: Border.all(
                  color: isLight ? const Color(0x22000000) : Colors.white.withOpacity(0.18),
                  width: 1.0,
                ),
                boxShadow: [
                  BoxShadow(
                    color: isLight ? Colors.black.withOpacity(0.12) : Colors.black.withOpacity(0.45),
                    blurRadius: 8,
                    offset: const Offset(-2, 0),
                  ),
                ],
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 7,
                    height: 7,
                    decoration: BoxDecoration(
                      color: _isConnected ? const Color(0xFF10B981) : const Color(0xFFF59E0B),
                      shape: BoxShape.circle,
                    ),
                  ),
                  const SizedBox(height: 6),
                  Icon(
                    Icons.chevron_left_rounded,
                    size: 16,
                    color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                  ),
                ],
              ),
            ),
          ),
        ),
      );
    }

    return Stack(
      clipBehavior: Clip.none,
      children: [
        // Subtle edge pull bar indicator when HUD is collapsed
        AnimatedOpacity(
          duration: const Duration(milliseconds: 200),
          opacity: _isHudCollapsed ? 1.0 : 0.0,
          child: IgnorePointer(
            ignoring: !_isHudCollapsed,
            child: pullBar,
          ),
        ),

        // Full Interactive HUD Capsule (smoothly slides in/out)
        AnimatedSlide(
          duration: const Duration(milliseconds: 280),
          curve: Curves.easeOutCubic,
          offset: slideOffset,
          child: AnimatedOpacity(
            duration: const Duration(milliseconds: 240),
            opacity: _isHudCollapsed ? 0.0 : 1.0,
            child: IgnorePointer(
              ignoring: _isHudCollapsed,
              child: positionedCapsule,
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildHorizontalKeyboardBar() {
    if (!_isKeyboardBarVisible) return const SizedBox.shrink();
    final bool isLight = _isLightTheme;
    return SafeArea(
      top: false,
      child: Align(
        alignment: Alignment.bottomCenter,
        child: Container(
          margin: const EdgeInsets.fromLTRB(10, 0, 10, 12),
          height: 48,
          decoration: BoxDecoration(
            color: isLight ? const Color(0xF4F1F5F9) : const Color(0xEE0F172A),
            borderRadius: BorderRadius.circular(14),
            border: Border.all(
              color: isLight ? const Color(0x18000000) : Colors.white.withOpacity(0.15),
            ),
            boxShadow: [
              BoxShadow(
                color: isLight ? const Color(0x14000000) : Colors.black.withOpacity(0.45),
                blurRadius: 16,
                offset: const Offset(0, 4),
              ),
            ],
          ),
          child: Row(
            children: [
              Padding(
                padding: const EdgeInsets.only(left: 8, right: 4),
                child: Icon(Icons.keyboard_rounded, size: 16, color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8)),
              ),
              Expanded(
                child: ListView(
                  scrollDirection: Axis.horizontal,
                  physics: const BouncingScrollPhysics(),
                  padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 6),
                  children: [
                    _buildKeycap('⌨ IME', () {
                      _keyboardFocusNode.requestFocus();
                    }, isSpecial: true, isAccent: true),
                    _buildKeycap('⊞ Win', () => _sendKey('win')),
                    _buildKeycap('Esc', () => _sendKey('escape')),
                    _buildKeycap('Tab', () => _sendKey('tab')),
                    _buildKeycap('Ctrl+Alt+Del', () => _sendShortcut(['ctrl', 'alt', 'delete']), isDanger: true),
                    _buildKeycap('Alt+Tab', () => _sendShortcut(['alt', 'tab'])),
                    _buildKeycap('Taskmgr', () => _sendShortcut(['ctrl', 'shift', 'escape'])),
                    _buildKeycap('Win+D', () => _sendShortcut(['win', 'd'])),
                    _buildKeycap('Enter', () => _sendKey('enter')),
                    _buildKeycap('Del', () => _sendKey('del')),
                    _buildKeycap('Ctrl+C', () => _sendShortcut(['ctrl', 'c'])),
                    _buildKeycap('Ctrl+V', () => _sendShortcut(['ctrl', 'v'])),
                    _buildKeycap('Ctrl+Z', () => _sendShortcut(['ctrl', 'z'])),
                    _buildKeycap('◄', () => _sendKey('left')),
                    _buildKeycap('▲', () => _sendKey('up')),
                    _buildKeycap('▼', () => _sendKey('down')),
                    _buildKeycap('►', () => _sendKey('right')),
                  ],
                ),
              ),
              IconButton(
                icon: Icon(Icons.close_rounded, size: 18, color: isLight ? const Color(0xFF64748B) : Colors.white60),
                onPressed: () {
                  HapticFeedback.lightImpact();
                  setState(() => _isKeyboardBarVisible = false);
                },
                tooltip: 'Ẩn phím tắt',
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildFloatingImeButton(Size screenSize) {
    if (!_isFloatingImeVisible) return const SizedBox.shrink();

    final defaultPos = Offset(math.max(8.0, screenSize.width - 64.0), math.max(8.0, screenSize.height - 116.0));
    final currentPos = _floatingImeOffset ?? defaultPos;
    final bool isLight = _isLightTheme;
    final double maxX = math.max(8.0, screenSize.width - 56.0);
    final double maxY = math.max(8.0, screenSize.height - 56.0);

    return Positioned(
      left: currentPos.dx.clamp(8.0, maxX),
      top: currentPos.dy.clamp(8.0, maxY),
      child: GestureDetector(
        onPanUpdate: (details) {
          setState(() {
            final cur = _floatingImeOffset ?? defaultPos;
            _floatingImeOffset = Offset(
              (cur.dx + details.delta.dx).clamp(8.0, maxX),
              (cur.dy + details.delta.dy).clamp(8.0, maxY),
            );
          });
        },
        onTap: () {
          HapticFeedback.selectionClick();
          _keyboardFocusNode.requestFocus();
        },
        onLongPress: () {
          HapticFeedback.mediumImpact();
          setState(() => _isFloatingImeVisible = false);
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: const Text('Đã ẩn phím IME nổi. Bạn có thể bật lại trong Quick Actions.'),
              duration: const Duration(seconds: 3),
              action: SnackBarAction(
                label: 'Hoàn tác',
                onPressed: () => setState(() => _isFloatingImeVisible = true),
              ),
            ),
          );
        },
        child: Material(
          elevation: 6,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(24),
            side: BorderSide(
              color: isLight ? const Color(0x330067C0) : const Color(0x5538BDF8),
              width: 1.5,
            ),
          ),
          color: isLight ? Colors.white : const Color(0xF20F172A),
          child: Container(
            width: 48,
            height: 48,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(24),
              gradient: LinearGradient(
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                colors: isLight
                    ? [const Color(0xFFFFFFFF), const Color(0xFFEEF5FD)]
                    : [const Color(0xFF1E293B), const Color(0xFF0F172A)],
              ),
              boxShadow: [
                BoxShadow(
                  color: isLight ? const Color(0x200067C0) : Colors.black.withOpacity(0.4),
                  blurRadius: 10,
                  offset: const Offset(0, 3),
                ),
              ],
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(
                  Icons.keyboard_rounded,
                  size: 20,
                  color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                ),
                Text(
                  'IME',
                  style: TextStyle(
                    fontSize: 8.5,
                    fontWeight: FontWeight.w800,
                    color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                    height: 1.0,
                    letterSpacing: 0.3,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildKeycap(String label, VoidCallback onTap, {bool isAccent = false, bool isDanger = false, bool isSpecial = false}) {
    final bool isLight = _isLightTheme;
    Color bg;
    Color borderColor;
    Color textColor;
    List<BoxShadow> shadows;

    if (isLight) {
      if (isDanger) {
        bg = const Color(0xFFFEE2E2);
        borderColor = const Color(0xFFFCA5A5);
        textColor = const Color(0xFFDC2626);
        shadows = [
          const BoxShadow(color: Color(0x14000000), blurRadius: 2, offset: Offset(0, 1.5)),
        ];
      } else if (isAccent) {
        bg = const Color(0xFF0067C0);
        borderColor = const Color(0xFF005A9E);
        textColor = Colors.white;
        shadows = [
          const BoxShadow(color: Color(0x24000000), blurRadius: 3, offset: Offset(0, 1.5)),
        ];
      } else if (isSpecial) {
        bg = const Color(0xFFE2E8F0);
        borderColor = const Color(0xFFCBD5E1);
        textColor = const Color(0xFF334155);
        shadows = [
          const BoxShadow(color: Color(0x10000000), blurRadius: 2, offset: Offset(0, 1.5)),
        ];
      } else {
        bg = Colors.white;
        borderColor = const Color(0xFFE2E8F0);
        textColor = const Color(0xFF1E293B);
        shadows = [
          const BoxShadow(color: Color(0x14000000), blurRadius: 2, offset: Offset(0, 1.5)),
        ];
      }
    } else {
      if (isDanger) {
        bg = const Color(0xFF7F1D1D).withOpacity(0.85);
        borderColor = const Color(0xFFEF4444).withOpacity(0.5);
        textColor = const Color(0xFFFCA5A5);
      } else if (isAccent) {
        bg = const Color(0xFF0369A1).withOpacity(0.85);
        borderColor = const Color(0xFF38BDF8).withOpacity(0.5);
        textColor = const Color(0xFFE0F2FE);
      } else if (isSpecial) {
        bg = const Color(0xFF334155);
        borderColor = Colors.white.withOpacity(0.12);
        textColor = const Color(0xFFF1F5F9);
      } else {
        bg = const Color(0xFF1E293B);
        borderColor = Colors.white.withOpacity(0.12);
        textColor = const Color(0xFFF1F5F9);
      }
      shadows = [
        BoxShadow(color: Colors.black.withOpacity(0.25), blurRadius: 3, offset: const Offset(0, 1)),
      ];
    }

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 2.5),
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          onTap: () {
            HapticFeedback.selectionClick();
            _resetIslandTimer();
            onTap();
            _refocusHardwareKeyboard();
          },
          borderRadius: BorderRadius.circular(8),
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
            decoration: BoxDecoration(
              color: bg,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: borderColor),
              boxShadow: shadows,
            ),
            alignment: Alignment.center,
            child: Text(
              label,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                fontFamily: 'monospace',
                color: textColor,
              ),
            ),
          ),
        ),
      ),
    );
  }

  void _showQuickActionsModal() {
    final bool isLight = _isLightTheme;
    showModalBottomSheet(
      context: context,
      backgroundColor: isLight ? Colors.white : const Color(0xFF0F172A),
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(24))),
      builder: (ctx) => StatefulBuilder(
        builder: (context, setModalState) {
          return Padding(
            padding: const EdgeInsets.fromLTRB(20, 16, 20, 24),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Center(
                    child: Container(
                      width: 40,
                      height: 4,
                      decoration: BoxDecoration(
                        color: isLight ? const Color(0xFFCBD5E1) : Colors.white24,
                        borderRadius: BorderRadius.circular(2),
                      ),
                    ),
                  ),
                  const SizedBox(height: 16),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        '⚡ Quick Actions & Tools',
                        style: TextStyle(
                          fontSize: 16,
                          fontWeight: FontWeight.w700,
                          color: isLight ? const Color(0xFF0F172A) : Colors.white,
                        ),
                      ),
                      IconButton(
                        icon: Icon(Icons.close_rounded, color: isLight ? const Color(0xFF64748B) : Colors.white60, size: 20),
                        onPressed: () => Navigator.pop(ctx),
                      ),
                    ],
                  ),
                  const SizedBox(height: 12),

                  // 0. Theme Selector Row (Windows 11 Light vs Dark)
                  Container(
                    margin: const EdgeInsets.only(bottom: 12),
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                    decoration: BoxDecoration(
                      color: isLight ? const Color(0xFFF1F5F9) : Colors.white.withOpacity(0.06),
                      borderRadius: BorderRadius.circular(12),
                      border: Border.all(color: isLight ? const Color(0xFFE2E8F0) : Colors.white12),
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        Row(
                          children: [
                            Icon(
                              _isLightTheme ? Icons.light_mode_rounded : Icons.dark_mode_rounded,
                              size: 16,
                              color: _isLightTheme ? const Color(0xFFD97706) : const Color(0xFF38BDF8),
                            ),
                            const SizedBox(width: 8),
                            Text(
                              'Giao diện điều khiển',
                              style: TextStyle(
                                fontSize: 12.5,
                                fontWeight: FontWeight.w700,
                                color: isLight ? const Color(0xFF0F172A) : Colors.white,
                              ),
                            ),
                          ],
                        ),
                        Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            _buildThemeToggleBtn('☀️ Sáng (Win 11)', true, setModalState),
                            const SizedBox(width: 6),
                            _buildThemeToggleBtn('🌙 Tối', false, setModalState),
                          ],
                        ),
                      ],
                    ),
                  ),

                  // Touch mode toggle + Keyboard button row
                  Row(
                    children: [
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: () {
                            HapticFeedback.selectionClick();
                            setState(() => _isDirectTouch = !_isDirectTouch);
                            setModalState(() {});
                          },
                          icon: Icon(
                            _isDirectTouch ? Icons.touch_app_rounded : Icons.mouse_rounded,
                            size: 18,
                            color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                          ),
                          label: Text(
                            _isDirectTouch ? 'Chế độ: Chạm' : 'Chế độ: Trackpad',
                            style: TextStyle(
                              color: isLight ? const Color(0xFF1E293B) : Colors.white,
                              fontSize: 12,
                            ),
                          ),
                          style: OutlinedButton.styleFrom(
                            side: BorderSide(color: isLight ? const Color(0xFFE2E8F0) : Colors.white.withOpacity(0.15)),
                            backgroundColor: isLight ? Colors.white : Colors.transparent,
                            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                            padding: const EdgeInsets.symmetric(vertical: 12),
                          ),
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: () {
                            Navigator.pop(ctx);
                            _keyboardFocusNode.requestFocus();
                          },
                          icon: const Icon(Icons.keyboard_rounded, size: 18, color: Color(0xFF10B981)),
                          label: Text(
                            'Bàn phím ảo',
                            style: TextStyle(
                              color: isLight ? const Color(0xFF1E293B) : Colors.white,
                              fontSize: 12,
                            ),
                          ),
                          style: OutlinedButton.styleFrom(
                            side: BorderSide(color: isLight ? const Color(0xFFE2E8F0) : Colors.white.withOpacity(0.15)),
                            backgroundColor: isLight ? Colors.white : Colors.transparent,
                            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
                            padding: const EdgeInsets.symmetric(vertical: 12),
                          ),
                        ),
                      ),
                    ],
                  ),
                  // Floating Draggable IME Button Switch
                  Container(
                    margin: const EdgeInsets.only(top: 10),
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                    decoration: BoxDecoration(
                      color: isLight ? const Color(0xFFF1F5F9) : Colors.white.withOpacity(0.06),
                      borderRadius: BorderRadius.circular(12),
                      border: Border.all(color: isLight ? const Color(0xFFE2E8F0) : Colors.white12),
                    ),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        Row(
                          children: [
                            Icon(
                              Icons.smart_button_rounded,
                              size: 16,
                              color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                            ),
                            const SizedBox(width: 8),
                            Text(
                              'Phím IME nổi (Di chuyển / Ẩn)',
                              style: TextStyle(
                                fontSize: 12.5,
                                fontWeight: FontWeight.w600,
                                color: isLight ? const Color(0xFF0F172A) : Colors.white,
                              ),
                            ),
                          ],
                        ),
                        Switch(
                          value: _isFloatingImeVisible,
                          activeThumbColor: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                          onChanged: (val) {
                            HapticFeedback.selectionClick();
                            setState(() => _isFloatingImeVisible = val);
                            setModalState(() {});
                          },
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(height: 16),
                  Text(
                    'Thao Tác Chuột',
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                      color: isLight ? const Color(0xFF64748B) : Colors.white54,
                    ),
                  ),
                  const SizedBox(height: 8),
                  Row(
                    children: [
                      Expanded(
                        child: ElevatedButton(
                          onPressed: () {
                            HapticFeedback.lightImpact();
                            _sendInput({'type': 'mouse_click', 'button': 'left'});
                          },
                          style: ElevatedButton.styleFrom(
                            backgroundColor: isLight ? Colors.white : const Color(0xFF1E293B),
                            foregroundColor: isLight ? const Color(0xFF1E293B) : Colors.white,
                            side: isLight ? const BorderSide(color: Color(0xFFE2E8F0)) : BorderSide.none,
                            elevation: isLight ? 1 : 0,
                            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                            padding: const EdgeInsets.symmetric(vertical: 10),
                          ),
                          child: const Text('L-Click', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: ElevatedButton(
                          onPressed: () {
                            HapticFeedback.lightImpact();
                            _sendInput({'type': 'mouse_click', 'button': 'right'});
                          },
                          style: ElevatedButton.styleFrom(
                            backgroundColor: isLight ? Colors.white : const Color(0xFF1E293B),
                            foregroundColor: isLight ? const Color(0xFF1E293B) : Colors.white,
                            side: isLight ? const BorderSide(color: Color(0xFFE2E8F0)) : BorderSide.none,
                            elevation: isLight ? 1 : 0,
                            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                            padding: const EdgeInsets.symmetric(vertical: 10),
                          ),
                          child: const Text('R-Click', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: ElevatedButton(
                          onPressed: () {
                            HapticFeedback.mediumImpact();
                            _sendInput({'type': 'mouse_double', 'button': 'left'});
                          },
                          style: ElevatedButton.styleFrom(
                            backgroundColor: isLight ? Colors.white : const Color(0xFF1E293B),
                            foregroundColor: isLight ? const Color(0xFF1E293B) : Colors.white,
                            side: isLight ? const BorderSide(color: Color(0xFFE2E8F0)) : BorderSide.none,
                            elevation: isLight ? 1 : 0,
                            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                            padding: const EdgeInsets.symmetric(vertical: 10),
                          ),
                          child: const Text('2x Click', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),
                  // Mouse Speed & Cursor Size Configuration Card
                  Container(
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: isLight ? const Color(0xFFF8FAFC) : const Color(0xFF1E293B).withOpacity(0.6),
                      borderRadius: BorderRadius.circular(14),
                      border: Border.all(color: isLight ? const Color(0xFFE2E8F0) : Colors.white.withOpacity(0.12)),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          mainAxisAlignment: MainAxisAlignment.spaceBetween,
                          children: [
                            Row(
                              children: [
                                Icon(Icons.speed_rounded, size: 16, color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8)),
                                const SizedBox(width: 6),
                                Text(
                                  'Tốc độ chuột Trackpad',
                                  style: TextStyle(
                                    fontSize: 12.5,
                                    fontWeight: FontWeight.w700,
                                    color: isLight ? const Color(0xFF0F172A) : Colors.white,
                                  ),
                                ),
                              ],
                            ),
                            Container(
                              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
                              decoration: BoxDecoration(
                                color: isLight ? const Color(0xFFE0F2FE) : const Color(0xFF0284C7).withOpacity(0.3),
                                borderRadius: BorderRadius.circular(6),
                                border: Border.all(
                                  color: isLight ? const Color(0xFF38BDF8) : const Color(0xFF38BDF8).withOpacity(0.5),
                                ),
                              ),
                              child: Text(
                                'Mức: ${_mouseSpeed.round()} / 100',
                                style: TextStyle(
                                  fontSize: 11,
                                  fontWeight: FontWeight.w700,
                                  color: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                                ),
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 4),
                        SliderTheme(
                          data: SliderThemeData(
                            trackHeight: 3,
                            thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 7),
                            activeTrackColor: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                            inactiveTrackColor: isLight ? const Color(0xFFE2E8F0) : Colors.white12,
                            thumbColor: isLight ? const Color(0xFF0067C0) : Colors.white,
                            overlayShape: const RoundSliderOverlayShape(overlayRadius: 14),
                          ),
                          child: Slider(
                            value: _mouseSpeed.clamp(1.0, 100.0),
                            min: 1.0,
                            max: 100.0,
                            divisions: 99,
                            onChanged: (val) {
                              setState(() => _mouseSpeed = val);
                              setModalState(() {});
                            },
                          ),
                        ),
                        Row(
                          mainAxisAlignment: MainAxisAlignment.spaceBetween,
                          children: [
                            _buildSpeedPresetChip('15 Chậm', 15.0, setModalState),
                            _buildSpeedPresetChip('30 Chuẩn', 30.0, setModalState),
                            _buildSpeedPresetChip('60 Nhanh', 60.0, setModalState),
                            _buildSpeedPresetChip('100 Tối đa', 100.0, setModalState),
                          ],
                        ),
                        const SizedBox(height: 12),
                        Divider(color: isLight ? const Color(0xFFE2E8F0) : Colors.white12, height: 1),
                        const SizedBox(height: 12),
                        Row(
                          children: [
                            Icon(Icons.ads_click_rounded, size: 16, color: isLight ? const Color(0xFF10B981) : const Color(0xFF4ADE80)),
                            const SizedBox(width: 6),
                            Text(
                              'Kích thước con trỏ chuột',
                              style: TextStyle(
                                fontSize: 12.5,
                                fontWeight: FontWeight.w700,
                                color: isLight ? const Color(0xFF0F172A) : Colors.white,
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 8),
                        Row(
                          children: [
                            Expanded(
                              child: _buildCursorSizeOption(
                                label: 'Chuẩn Win',
                                sublabel: 'Tỷ lệ 1:1',
                                mode: 'native',
                                isSelected: _cursorScaleMode == 'native',
                                onTap: () {
                                  HapticFeedback.selectionClick();
                                  setState(() => _cursorScaleMode = 'native');
                                  setModalState(() {});
                                },
                              ),
                            ),
                            const SizedBox(width: 8),
                            Expanded(
                              child: _buildCursorSizeOption(
                                label: 'Vừa',
                                sublabel: 'Mặc định',
                                mode: 'standard',
                                isSelected: _cursorScaleMode == 'standard',
                                onTap: () {
                                  HapticFeedback.selectionClick();
                                  setState(() => _cursorScaleMode = 'standard');
                                  setModalState(() {});
                                },
                              ),
                            ),
                            const SizedBox(width: 8),
                            Expanded(
                              child: _buildCursorSizeOption(
                                label: 'Lớn',
                                sublabel: 'Dễ nhìn',
                                mode: 'large',
                                isSelected: _cursorScaleMode == 'large',
                                onTap: () {
                                  HapticFeedback.selectionClick();
                                  setState(() => _cursorScaleMode = 'large');
                                  setModalState(() {});
                                },
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 8),
                        Row(
                          mainAxisAlignment: MainAxisAlignment.spaceBetween,
                          children: [
                            Text(
                              'Gia tốc chuột (Precision Acceleration)',
                              style: TextStyle(
                                fontSize: 11,
                                color: isLight ? const Color(0xFF475569) : Colors.white70,
                              ),
                            ),
                            Switch(
                              value: _enableMouseAccel,
                              activeThumbColor: isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8),
                              materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                              onChanged: (val) {
                                HapticFeedback.selectionClick();
                                setState(() => _enableMouseAccel = val);
                                setModalState(() {});
                              },
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(height: 16),
                  Text(
                    'Phím Tắt Windows',
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                      color: isLight ? const Color(0xFF64748B) : Colors.white54,
                    ),
                  ),
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      _buildModalKeyBtn('⊞ Win', () => _sendKey('win'), isAccent: true),
                      _buildModalKeyBtn('Esc', () => _sendKey('escape')),
                      _buildModalKeyBtn('Ctrl+Alt+Del', () => _sendShortcut(['ctrl', 'alt', 'delete']), isDanger: true),
                      _buildModalKeyBtn('Alt+Tab', () => _sendShortcut(['alt', 'tab'])),
                      _buildModalKeyBtn('Taskmgr', () => _sendShortcut(['ctrl', 'shift', 'escape'])),
                      _buildModalKeyBtn('Win+D', () => _sendShortcut(['win', 'd'])),
                      _buildModalKeyBtn('Enter', () => _sendKey('enter')),
                      _buildModalKeyBtn('Tab', () => _sendKey('tab')),
                      _buildModalKeyBtn('Del', () => _sendKey('del')),
                      _buildModalKeyBtn('Ctrl+C', () => _sendShortcut(['ctrl', 'c'])),
                      _buildModalKeyBtn('Ctrl+V', () => _sendShortcut(['ctrl', 'v'])),
                      _buildModalKeyBtn('Ctrl+Z', () => _sendShortcut(['ctrl', 'z'])),
                      _buildModalKeyBtn('▲', () => _sendKey('up')),
                      _buildModalKeyBtn('▼', () => _sendKey('down')),
                      _buildModalKeyBtn('◄', () => _sendKey('left')),
                      _buildModalKeyBtn('►', () => _sendKey('right')),
                    ],
                  ),
                  const SizedBox(height: 20),
                  Row(
                    children: [
                      Expanded(
                        child: OutlinedButton.icon(
                          onPressed: () {
                            Navigator.pop(ctx);
                            _showModeSelector();
                          },
                          icon: Icon(
                            Icons.tune_rounded,
                            size: 16,
                            color: isLight ? const Color(0xFF475569) : Colors.white70,
                          ),
                          label: Text(
                            'Chất lượng (${_getModeLabel()})',
                            style: TextStyle(
                              color: isLight ? const Color(0xFF475569) : Colors.white70,
                              fontSize: 12,
                            ),
                          ),
                          style: OutlinedButton.styleFrom(
                            side: BorderSide(color: isLight ? const Color(0xFFCBD5E1) : Colors.white.withOpacity(0.12)),
                            backgroundColor: isLight ? Colors.white : Colors.transparent,
                            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                          ),
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: ElevatedButton.icon(
                          onPressed: () {
                            Navigator.pop(ctx);
                            Navigator.pop(context);
                          },
                          icon: const Icon(Icons.power_settings_new_rounded, size: 16),
                          label: const Text('Ngắt kết nối', style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
                          style: ElevatedButton.styleFrom(
                            backgroundColor: const Color(0xFFEF4444).withOpacity(0.9),
                            foregroundColor: Colors.white,
                            shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
                          ),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }

  Widget _buildThemeToggleBtn(String label, bool isLightTarget, StateSetter setModalState) {
    final bool isSelected = _isLightTheme == isLightTarget;
    return InkWell(
      onTap: () {
        HapticFeedback.selectionClick();
        setState(() => _isLightTheme = isLightTarget);
        setModalState(() {});
      },
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
        decoration: BoxDecoration(
          color: isSelected
              ? (_isLightTheme ? const Color(0xFF0067C0) : const Color(0xFF0284C7))
              : (_isLightTheme ? Colors.white : Colors.white.withOpacity(0.06)),
          borderRadius: BorderRadius.circular(6),
          border: Border.all(
            color: isSelected
                ? (_isLightTheme ? const Color(0xFF005A9E) : const Color(0xFF38BDF8))
                : (_isLightTheme ? const Color(0xFFE2E8F0) : Colors.white12),
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 11,
            fontWeight: isSelected ? FontWeight.w700 : FontWeight.w500,
            color: isSelected
                ? Colors.white
                : (_isLightTheme ? const Color(0xFF475569) : Colors.white70),
          ),
        ),
      ),
    );
  }

  Widget _buildSpeedPresetChip(String label, double speed, StateSetter setModalState) {
    final bool isSelected = (_mouseSpeed - speed).abs() < 1.0;
    final bool isLight = _isLightTheme;
    return InkWell(
      onTap: () {
        HapticFeedback.selectionClick();
        setState(() => _mouseSpeed = speed);
        setModalState(() {});
      },
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 4),
        decoration: BoxDecoration(
          color: isSelected
              ? (isLight ? const Color(0xFFE0F2FE) : const Color(0xFF0284C7).withOpacity(0.35))
              : (isLight ? Colors.white : Colors.white.withOpacity(0.06)),
          borderRadius: BorderRadius.circular(6),
          border: Border.all(
            color: isSelected
                ? (isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8))
                : (isLight ? const Color(0xFFE2E8F0) : Colors.white12),
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 10,
            fontWeight: isSelected ? FontWeight.w700 : FontWeight.w500,
            color: isSelected
                ? (isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8))
                : (isLight ? const Color(0xFF475569) : Colors.white70),
          ),
        ),
      ),
    );
  }

  Widget _buildCursorSizeOption({
    required String label,
    required String sublabel,
    required String mode,
    required bool isSelected,
    required VoidCallback onTap,
  }) {
    final bool isLight = _isLightTheme;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(8),
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
        decoration: BoxDecoration(
          color: isSelected
              ? (isLight ? const Color(0xFFE0F2FE) : const Color(0xFF0284C7).withOpacity(0.3))
              : (isLight ? Colors.white : Colors.white.withOpacity(0.05)),
          borderRadius: BorderRadius.circular(8),
          border: Border.all(
            color: isSelected
                ? (isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8))
                : (isLight ? const Color(0xFFE2E8F0) : Colors.white12),
          ),
        ),
        child: Column(
          children: [
            Text(
              label,
              style: TextStyle(
                fontSize: 11,
                fontWeight: isSelected ? FontWeight.w700 : FontWeight.w600,
                color: isSelected
                    ? (isLight ? const Color(0xFF0067C0) : const Color(0xFF38BDF8))
                    : (isLight ? const Color(0xFF0F172A) : Colors.white),
              ),
            ),
            const SizedBox(height: 2),
            Text(
              sublabel,
              style: TextStyle(
                fontSize: 9,
                color: isSelected
                    ? (isLight ? const Color(0xFF0284C7) : const Color(0xFF93C5FD))
                    : (isLight ? const Color(0xFF64748B) : Colors.white38),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildModalKeyBtn(String label, VoidCallback onTap, {bool isAccent = false, bool isDanger = false}) {
    final bool isLight = _isLightTheme;
    Color bg;
    Color fg;
    Border? border;

    if (isLight) {
      if (isAccent) {
        bg = const Color(0xFF0067C0);
        fg = Colors.white;
      } else if (isDanger) {
        bg = const Color(0xFFFEE2E2);
        fg = const Color(0xFFDC2626);
        border = Border.all(color: const Color(0xFFFCA5A5));
      } else {
        bg = Colors.white;
        fg = const Color(0xFF1E293B);
        border = Border.all(color: const Color(0xFFE2E8F0));
      }
    } else {
      if (isAccent) {
        bg = const Color(0xFF2563EB);
        fg = Colors.white;
      } else if (isDanger) {
        bg = const Color(0xFF991B1B);
        fg = Colors.white;
      } else {
        bg = const Color(0xFF1E293B);
        fg = Colors.white;
      }
    }

    return Material(
      color: bg,
      borderRadius: BorderRadius.circular(8),
      child: InkWell(
        onTap: () {
          HapticFeedback.lightImpact();
          onTap();
          _refocusHardwareKeyboard();
        },
        borderRadius: BorderRadius.circular(8),
        child: Container(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(8),
            border: border,
            boxShadow: isLight && !isAccent
                ? [const BoxShadow(color: Color(0x10000000), blurRadius: 2, offset: Offset(0, 1))]
                : null,
          ),
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          child: Text(
            label,
            style: TextStyle(fontSize: 12, fontWeight: FontWeight.w700, color: fg),
          ),
        ),
      ),
    );
  }
}