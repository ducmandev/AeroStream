Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.IO;

public class LockDiag {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr OpenInputDesktop(uint dwFlags, bool fInherit, uint dwDesiredAccess);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr OpenDesktopW([MarshalAs(UnmanagedType.LPWStr)] string lpszDesktop, uint dwFlags, bool fInherit, uint dwDesiredAccess);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetThreadDesktop(IntPtr hDesktop);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr GetThreadDesktop(uint dwThreadId);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool CloseDesktop(IntPtr hDesktop);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetUserObjectInformationW(IntPtr hObj, int nIndex, IntPtr pvInfo, uint nLength, out uint lpnLengthNeeded);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);

    [DllImport("kernel32.dll")]
    public static extern uint GetCurrentThreadId();

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint MapVirtualKeyW(uint uCode, uint uMapType);

    public const int UOI_NAME = 2;

    public static string GetDesktopName(IntPtr hDesk) {
        if (hDesk == IntPtr.Zero) return "(null)";
        uint needed = 0;
        GetUserObjectInformationW(hDesk, UOI_NAME, IntPtr.Zero, 0, out needed);
        if (needed == 0) return "(err:" + Marshal.GetLastWin32Error() + ")";
        IntPtr buf = Marshal.AllocHGlobal((int)needed);
        try {
            if (GetUserObjectInformationW(hDesk, UOI_NAME, buf, needed, out needed)) {
                return Marshal.PtrToStringUni(buf);
            }
            return "(err:" + Marshal.GetLastWin32Error() + ")";
        } finally {
            Marshal.FreeHGlobal(buf);
        }
    }

    public static void RunLockTest(string logPath) {
        using (StreamWriter sw = new StreamWriter(logPath, false)) {
            sw.WriteLine("=== Lock Diag Test Started ===");
            IntPtr curDesk = GetThreadDesktop(GetCurrentThreadId());
            sw.WriteLine("Initial ThreadDesktop: " + curDesk + " Name: " + GetDesktopName(curDesk));

            IntPtr inputDesk = OpenInputDesktop(0, false, 0x02000000);
            int err = Marshal.GetLastWin32Error();
            sw.WriteLine("OpenInputDesktop(0x02000000): " + inputDesk + " (err: " + err + ") Name: " + GetDesktopName(inputDesk));
            if (inputDesk != IntPtr.Zero) {
                bool setRes = SetThreadDesktop(inputDesk);
                sw.WriteLine("SetThreadDesktop to inputDesk: " + setRes + " (err: " + Marshal.GetLastWin32Error() + ")");
            }

            IntPtr winlogonDesk = OpenDesktopW("Winlogon", 0, false, 0x02000000);
            sw.WriteLine("OpenDesktopW('Winlogon'): " + winlogonDesk + " (err: " + Marshal.GetLastWin32Error() + ") Name: " + GetDesktopName(winlogonDesk));

            // Test keybd_event
            sw.WriteLine("Sending Space via keybd_event...");
            uint scan = MapVirtualKeyW(0x20, 0);
            keybd_event(0x20, (byte)scan, 0, UIntPtr.Zero);
            keybd_event(0x20, (byte)scan, 2 /* KEYUP */, UIntPtr.Zero);
            sw.WriteLine("keybd_event completed, LastError: " + Marshal.GetLastWin32Error());
            sw.Flush();
        }
    }
}
"@
