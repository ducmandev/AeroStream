import 'package:flutter/services.dart';

class AudioPlayerService {
  static const MethodChannel _audioControlChannel = MethodChannel('aerostream/audio_control');
  static const BasicMessageChannel<ByteData> _audioStreamChannel = BasicMessageChannel<ByteData>(
    'aerostream/audio_stream',
    BinaryCodec(),
  );

  bool _isMuted = false;
  bool get isMuted => _isMuted;

  void start() {
    _audioControlChannel.invokeMethod('start');
  }

  void stop() {
    _audioControlChannel.invokeMethod('stop');
  }

  void setMuted(bool muted) {
    _isMuted = muted;
    _audioControlChannel.invokeMethod('setMuted', {'muted': _isMuted});
  }

  bool toggleMute() {
    setMuted(!_isMuted);
    return _isMuted;
  }

  void feedOpusPayload(Uint8List payload) {
    if (!_isMuted && payload.isNotEmpty) {
      _audioStreamChannel.send(ByteData.view(payload.buffer));
    }
  }
}
