import 'package:flutter/material.dart';

class HardwareCursorPainter extends CustomPainter {
  const HardwareCursorPainter();

  @override
  void paint(Canvas canvas, Size size) {
    final w = size.width;
    final h = size.height;

    // Authentic slender Windows Aero arrow geometry
    // Hotspot is at (0, 0)
    final path = Path()
      ..moveTo(0, 0)
      ..lineTo(0, h * 0.76)
      ..lineTo(w * 0.28, h * 0.63)
      ..lineTo(w * 0.56, h * 0.96)
      ..lineTo(w * 0.70, h * 0.90)
      ..lineTo(w * 0.44, h * 0.57)
      ..lineTo(w * 0.92, h * 0.57)
      ..close();

    // 1. Subtle, clean hairline drop shadow (thin, crisp, no muddy blur halo)
    final shadowPaint = Paint()
      ..color = Colors.black.withOpacity(0.24)
      ..style = PaintingStyle.fill;
    canvas.save();
    canvas.translate(0.55, 0.75);
    canvas.drawPath(path, shadowPaint);
    canvas.restore();

    // 2. Pure crisp white fill
    final fillPaint = Paint()
      ..color = Colors.white
      ..style = PaintingStyle.fill;
    canvas.drawPath(path, fillPaint);

    // 3. Slender hairline dark stroke (0.65px width, miter join for razor-sharp tips)
    final strokePaint = Paint()
      ..color = const Color(0xFF0F172A)
      ..strokeWidth = 0.65
      ..strokeJoin = StrokeJoin.miter
      ..strokeMiterLimit = 3.0
      ..style = PaintingStyle.stroke;
    canvas.drawPath(path, strokePaint);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
