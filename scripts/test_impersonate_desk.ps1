Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Diagnostics;
using System.Security.Principal;

public class ImpersonateTest {
    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool OpenProcessToken(IntPtr ProcessHandle, uint DesiredAccess, out IntPtr TokenHandle);

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern bool LookupPrivilegeValueW(string lpSystemName, string lpName, out long lpLuid);

    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool AdjustTokenPrivileges(IntPtr TokenHandle, bool DisableAllPrivileges, IntPtr NewState, uint BufferLength, IntPtr PreviousState, IntPtr ReturnLength);

    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool DuplicateTokenEx(
        IntPtr hExistingToken,
        uint dwDesiredAccess,
        IntPtr lpTokenAttributes,
        int ImpersonationLevel,
        int TokenType,
        out IntPtr phNewToken);

    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool ImpersonateLoggedOnUser(IntPtr hToken);

    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool RevertToSelf();

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr OpenProcess(uint processAccess, bool bInheritHandle, int processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr OpenDesktopW([MarshalAs(UnmanagedType.LPWStr)] string lpszDesktop, uint dwFlags, bool fInherit, uint dwDesiredAccess);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool CloseDesktop(IntPtr hDesktop);

    [DllImport("wtsapi32.dll", SetLastError = true)]
    public static extern uint WTSGetActiveConsoleSessionId();

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    public struct TOKEN_PRIVILEGES {
        public int PrivilegeCount;
        public long Luid;
        public int Attributes;
    }

    public static void Test() {
        uint sessionId = WTSGetActiveConsoleSessionId();
        Console.WriteLine("Active Console SessionId: " + sessionId);

        Process[] procs = Process.GetProcessesByName("winlogon");
        Console.WriteLine("Winlogon processes found: " + procs.Length);
        int targetPid = 0;
        foreach (Process p in procs) {
            Console.WriteLine("  PID: " + p.Id + ", Session: " + p.SessionId);
            if (p.SessionId == (int)sessionId) {
                targetPid = p.Id;
            }
        }

        if (targetPid == 0 && procs.Length > 0) targetPid = procs[0].Id;
        Console.WriteLine("Target Winlogon PID: " + targetPid);

        IntPtr hProc = OpenProcess(0x1000 /* PROCESS_QUERY_LIMITED_INFORMATION */, false, targetPid);
        Console.WriteLine("OpenProcess(0x1000): " + hProc + ", LastErr: " + Marshal.GetLastWin32Error());
        if (hProc == IntPtr.Zero) {
            hProc = OpenProcess(0x0400 /* PROCESS_QUERY_INFORMATION */, false, targetPid);
            Console.WriteLine("OpenProcess(0x0400): " + hProc + ", LastErr: " + Marshal.GetLastWin32Error());
        }

        if (hProc != IntPtr.Zero) {
            IntPtr hToken;
            bool ok = OpenProcessToken(hProc, 0x0002 /* TOKEN_DUPLICATE */ | 0x0008 /* TOKEN_QUERY */, out hToken);
            Console.WriteLine("OpenProcessToken: " + ok + ", LastErr: " + Marshal.GetLastWin32Error());
            if (ok) {
                IntPtr hDup;
                bool dupOk = DuplicateTokenEx(hToken, 0x02000000, IntPtr.Zero, 2, 2, out hDup);
                Console.WriteLine("DuplicateTokenEx: " + dupOk + ", LastErr: " + Marshal.GetLastWin32Error());
                if (dupOk) {
                    bool impOk = ImpersonateLoggedOnUser(hDup);
                    Console.WriteLine("ImpersonateLoggedOnUser: " + impOk + ", LastErr: " + Marshal.GetLastWin32Error());
                    Console.WriteLine("Current Identity: " + WindowsIdentity.GetCurrent().Name);

                    IntPtr hWinlogon = OpenDesktopW("Winlogon", 0, false, 0x02000000);
                    Console.WriteLine("OpenDesktopW('Winlogon') after impersonation: " + hWinlogon + ", LastErr: " + Marshal.GetLastWin32Error());
                    if (hWinlogon != IntPtr.Zero) CloseDesktop(hWinlogon);

                    RevertToSelf();
                    CloseHandle(hDup);
                }
                CloseHandle(hToken);
            }
            CloseHandle(hProc);
        }
    }
}
"@
[ImpersonateTest]::Test()
