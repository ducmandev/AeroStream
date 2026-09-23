Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.IO;

public class LockTest {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr OpenInputDesktop(uint dwFlags, bool fInherit, uint dwDesiredAccess);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetUserObjectInformationW(IntPtr hObj, int nIndex, IntPtr pvInfo, uint nLength, out uint lpnLengthNeeded);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool CloseDesktop(IntPtr hDesktop);

    public const int UOI_NAME = 2;

    public static string Check(string logPath) {
        IntPtr hDesk = OpenInputDesktop(0, false, 0x02000000);
        int err = Marshal.GetLastWin32Error();
        string name = "(null)";
        if (hDesk != IntPtr.Zero) {
            uint needed = 0;
            GetUserObjectInformationW(hDesk, UOI_NAME, IntPtr.Zero, 0, out needed);
            if (needed > 0) {
                IntPtr buf = Marshal.AllocHGlobal((int)needed);
                if (GetUserObjectInformationW(hDesk, UOI_NAME, buf, needed, out needed)) {
                    name = Marshal.PtrToStringUni(buf);
                }
                Marshal.FreeHGlobal(buf);
            }
            CloseDesktop(hDesk);
        }
        string res = "OpenInputDesktop handle: " + hDesk + " (err: " + err + "), Name: " + name;
        File.AppendAllText(logPath, DateTime.Now.ToString("o") + " " + res + Environment.NewLine);
        return res;
    }
}
"@
[LockTest]::Check("D:\StreamApp\locked_desk.log")
