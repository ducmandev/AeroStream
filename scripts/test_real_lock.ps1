Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.IO;
using System.Threading;

public class RealLockTest {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr OpenInputDesktop(uint dwFlags, bool fInherit, uint dwDesiredAccess);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr OpenDesktopW([MarshalAs(UnmanagedType.LPWStr)] string lpszDesktop, uint dwFlags, bool fInherit, uint dwDesiredAccess);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetUserObjectInformationW(IntPtr hObj, int nIndex, IntPtr pvInfo, uint nLength, out uint lpnLengthNeeded);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool CloseDesktop(IntPtr hDesktop);

    public const int UOI_NAME = 2;

    public static string GetName(IntPtr hDesk) {
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

    public static void Run(string logPath) {
        using (StreamWriter sw = new StreamWriter(logPath, false)) {
            sw.WriteLine("--- Test 1: Unlocked ---");
            IntPtr h1 = OpenInputDesktop(0, false, 0x02000000);
            sw.WriteLine("OpenInputDesktop: " + h1 + ", LastErr: " + Marshal.GetLastWin32Error() + ", Name: " + GetName(h1));
            if (h1 != IntPtr.Zero) CloseDesktop(h1);

            IntPtr hw1 = OpenDesktopW("Winlogon", 0, false, 0x02000000);
            sw.WriteLine("OpenDesktop(Winlogon): " + hw1 + ", LastErr: " + Marshal.GetLastWin32Error() + ", Name: " + GetName(hw1));
            if (hw1 != IntPtr.Zero) CloseDesktop(hw1);
            sw.Flush();
        }
    }
}
"@
[RealLockTest]::Run("D:\StreamApp\real_lock.log")
Get-Content D:\StreamApp\real_lock.log
