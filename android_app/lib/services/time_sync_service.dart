class TimeSyncService {
  int _serverClockOffset = 0;
  int _latencyMs = 5;

  int get serverClockOffset => _serverClockOffset;
  int get latencyMs => _latencyMs;

  int get hostCurrentTime => DateTime.now().millisecondsSinceEpoch + _serverClockOffset;

  void handleTimeSync(int? serverTime) {
    if (serverTime != null) {
      _serverClockOffset = serverTime - DateTime.now().millisecondsSinceEpoch;
    }
  }

  void handlePong({int? clientTime, int? serverTime}) {
    if (clientTime == null) return;
    final now = DateTime.now().millisecondsSinceEpoch;
    final rtt = (now - clientTime).clamp(1, 999);
    _latencyMs = rtt;

    if (serverTime != null) {
      final measuredOffset = (serverTime + (rtt ~/ 2)) - now;
      _serverClockOffset = _serverClockOffset == 0
          ? measuredOffset
          : ((_serverClockOffset * 7 + measuredOffset * 3) ~/ 10);
    }
  }

  int calculateOneWayLatency(int packetServerTimestamp) {
    if (packetServerTimestamp <= 0 || _serverClockOffset == 0) {
      return _latencyMs;
    }
    final oneWay = hostCurrentTime - packetServerTimestamp;
    return oneWay.clamp(1, 999);
  }
}
