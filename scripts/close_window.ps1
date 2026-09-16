$code = @"
using System;
using System.Runtime.InteropServices;
public class WindowHelper {
    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
}
"@

Add-Type -TypeDefinition $code

$hwnd = [WindowHelper]::FindWindow("AeroStreamMainWindow", $null)
Write-Host "HWND: $hwnd"

if ($hwnd -ne [IntPtr]::Zero) {
    # Send WM_COMMAND with ID_BTN_EXIT = 103
    [WindowHelper]::SendMessage($hwnd, 0x0111, [IntPtr]103, [IntPtr]::Zero)
    # Also Post WM_CLOSE = 0x0010
    [WindowHelper]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    Write-Host "Sent Exit command and WM_CLOSE."
} else {
    Write-Host "Window AeroStreamMainWindow not found."
}
