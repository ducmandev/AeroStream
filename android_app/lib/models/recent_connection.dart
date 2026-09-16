class RecentConnection {
  final String ip;
  final String port;
  final String pin;
  final String deviceName;
  final DateTime lastUsed;

  RecentConnection({
    required this.ip,
    required this.port,
    required this.pin,
    required this.deviceName,
    required this.lastUsed,
  });

  Map<String, dynamic> toJson() => {
    'ip': ip,
    'port': port,
    'pin': pin,
    'deviceName': deviceName,
    'lastUsed': lastUsed.toIso8601String(),
  };

  factory RecentConnection.fromJson(Map<String, dynamic> json) => RecentConnection(
    ip: json['ip'] as String? ?? '127.0.0.1',
    port: json['port'] as String? ?? '8080',
    pin: json['pin'] as String? ?? '',
    deviceName: json['deviceName'] as String? ?? 'Windows PC',
    lastUsed: DateTime.tryParse(json['lastUsed'] as String? ?? '') ?? DateTime.now(),
  );
}
