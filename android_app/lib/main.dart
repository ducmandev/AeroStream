import 'dart:io';
import 'package:flutter/material.dart';
import 'screens/windows_dashboard_screen.dart';
import 'screens/mobile_connect_screen.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const AeroStreamApp());
}

class AeroStreamApp extends StatelessWidget {
  const AeroStreamApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'AeroStream // Remote Desktop',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        fontFamily: Platform.isWindows ? 'Segoe UI' : null,
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF2563EB),
          primary: const Color(0xFF2563EB),
          surface: Colors.white,
          brightness: Brightness.light,
        ),
        scaffoldBackgroundColor: const Color(0xFFF1F5F9),
        cardTheme: CardThemeData(
          elevation: 0,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(16),
            side: const BorderSide(color: Color(0xFFE2E8F0)),
          ),
          color: Colors.white,
        ),
      ),
      home: Platform.isWindows ? const WindowsDashboardScreen() : const MobileConnectScreen(),
    );
  }
}