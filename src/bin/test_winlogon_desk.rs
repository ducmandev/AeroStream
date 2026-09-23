use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::System::RemoteDesktop::*;
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::System::Threading::*;

unsafe fn enable_privileges() {
    let mut token: HANDLE = std::ptr::null_mut();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) != 0 {
        for priv_name in &[
            "SeDebugPrivilege",
            "SeImpersonatePrivilege",
            "SeTcbPrivilege",
            "SeAssignPrimaryTokenPrivilege",
            "SeIncreaseQuotaPrivilege",
        ] {
            let wide: Vec<u16> = priv_name.encode_utf16().chain(std::iter::once(0)).collect();
            let mut luid: LUID = std::mem::zeroed();
            if LookupPrivilegeValueW(std::ptr::null_mut(), wide.as_ptr(), &mut luid) != 0 {
                let mut tp = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES {
                        Luid: luid,
                        Attributes: SE_PRIVILEGE_ENABLED,
                    }],
                };
                let _ = AdjustTokenPrivileges(
                    token,
                    0,
                    &mut tp as *mut _ as *mut _,
                    std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
        CloseHandle(token);
    }
}

unsafe fn get_desktop_name(h_desk: HDESK) -> String {
    if h_desk.is_null() {
        return "(null)".into();
    }
    let mut needed = 0;
    GetUserObjectInformationW(h_desk, UOI_NAME, std::ptr::null_mut(), 0, &mut needed);
    if needed == 0 {
        return format!("(err:{})", GetLastError());
    }
    let mut buf = vec![0u16; (needed as usize) / 2 + 1];
    if GetUserObjectInformationW(
        h_desk,
        UOI_NAME,
        buf.as_mut_ptr() as _,
        needed,
        &mut needed,
    ) != 0
    {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    } else {
        format!("(err:{})", GetLastError())
    }
}

fn main() {
    unsafe {
        println!("=== Winlogon Desktop Attachment Diagnostic ===");
        enable_privileges();

        let cur_thread_desk = GetThreadDesktop(GetCurrentThreadId());
        println!("Current Thread Desktop: {:?}, name: {}", cur_thread_desk, get_desktop_name(cur_thread_desk));

        let input_desk = OpenInputDesktop(0, 0, 0x02000000);
        println!("OpenInputDesktop: {:?}, name: {}", input_desk, get_desktop_name(input_desk));

        // Try direct OpenDesktopW("Winlogon")
        let winlogon_name: Vec<u16> = "Winlogon\0".encode_utf16().collect();
        let direct_winlogon = OpenDesktopW(winlogon_name.as_ptr(), 0, 0, 0x02000000);
        println!("Direct OpenDesktopW('Winlogon'): {:?} (GetLastError: {})", direct_winlogon, GetLastError());

        // Try impersonation
        let session_id = WTSGetActiveConsoleSessionId();
        println!("Active Console Session: {}", session_id);

        let mut p_proc_info: *mut WTS_PROCESS_INFOW = std::ptr::null_mut();
        let mut count: u32 = 0;
        let mut winlogon_pid = None;

        if WTSEnumerateProcessesW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut p_proc_info, &mut count) != 0 && !p_proc_info.is_null() {
            let procs = std::slice::from_raw_parts(p_proc_info, count as usize);
            for p in procs {
                if p.SessionId == session_id && !p.pProcessName.is_null() {
                    let mut len = 0;
                    while *p.pProcessName.add(len) != 0 { len += 1; }
                    let name = String::from_utf16_lossy(std::slice::from_raw_parts(p.pProcessName, len));
                    if name.eq_ignore_ascii_case("winlogon.exe") {
                        winlogon_pid = Some(p.ProcessId);
                        println!("Found winlogon.exe PID: {}", p.ProcessId);
                        break;
                    }
                }
            }
            WTSFreeMemory(p_proc_info as _);
        }

        if let Some(pid) = winlogon_pid {
            let h_proc = OpenProcess(0x1000 | 0x0400, 0, pid);
            println!("OpenProcess on winlogon: {:?} (GetLastError: {})", h_proc, GetLastError());
            if !h_proc.is_null() {
                let mut h_token: HANDLE = std::ptr::null_mut();
                let tok_res = OpenProcessToken(h_proc, TOKEN_DUPLICATE | TOKEN_QUERY, &mut h_token);
                println!("OpenProcessToken: {} (GetLastError: {})", tok_res, GetLastError());
                if tok_res != 0 {
                    let mut h_dup_token: HANDLE = std::ptr::null_mut();
                    let dup_res = DuplicateTokenEx(
                        h_token,
                        0x02000000,
                        std::ptr::null_mut(),
                        SecurityImpersonation,
                        TokenImpersonation,
                        &mut h_dup_token,
                    );
                    println!("DuplicateTokenEx: {} (GetLastError: {})", dup_res, GetLastError());
                    if dup_res != 0 {
                        let imp_res = ImpersonateLoggedOnUser(h_dup_token);
                        println!("ImpersonateLoggedOnUser: {} (GetLastError: {})", imp_res, GetLastError());

                        let winlogon_as_system = OpenDesktopW(winlogon_name.as_ptr(), 0, 0, 0x00040000 | 0x02000000);
                        println!("OpenDesktopW('Winlogon') AS SYSTEM: {:?} (GetLastError: {})", winlogon_as_system, GetLastError());
                        if !winlogon_as_system.is_null() {
                            println!("Winlogon Desktop Name: {}", get_desktop_name(winlogon_as_system));
                            // Set NULL DACL while running as SYSTEM to grant full access to everyone
                            let sec_res = SetSecurityInfo(
                                winlogon_as_system as _,
                                SE_WINDOW_OBJECT,
                                DACL_SECURITY_INFORMATION,
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                            );
                            println!("SetSecurityInfo on Winlogon: {} (0 = SUCCESS)", sec_res);

                            RevertToSelf();
                            println!("Reverted impersonation to self");

                            let set_res = SetThreadDesktop(winlogon_as_system);
                            println!("SetThreadDesktop to Winlogon: {} (GetLastError: {})", set_res, GetLastError());
                            CloseDesktop(winlogon_as_system);
                        } else {
                            RevertToSelf();
                        }
                        CloseHandle(h_dup_token);
                    }
                    CloseHandle(h_token);
                }
                CloseHandle(h_proc);
            }
        }
    }
}
