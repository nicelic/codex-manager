#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub id: u32,
    pub name: String,
}

pub fn new_silent_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

fn matches_process_name(exe_name: &str, target: &str) -> bool {
    if exe_name.eq_ignore_ascii_case(target) {
        return true;
    }
    if let Some(target_base) = target.strip_suffix(".exe") {
        if exe_name.eq_ignore_ascii_case(target_base) {
            return true;
        }
    } else {
        let with_exe = format!("{}.exe", target);
        if exe_name.eq_ignore_ascii_case(&with_exe) {
            return true;
        }
    }
    false
}

pub fn is_process_running(name: &str) -> bool {
    find_process_id(name).is_some()
}

pub fn find_process_id(name: &str) -> Option<u32> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return None;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]);

                if matches_process_name(&exe_name, name) {
                    CloseHandle(snapshot);
                    return Some(entry.th32ProcessID);
                }

                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
    }
    None
}

pub fn find_all_process_ids(name: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return pids;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]);

                if matches_process_name(&exe_name, name) {
                    pids.push(entry.th32ProcessID);
                }

                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
    }
    pids
}

pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

        const SYNCHRONIZE: u32 = 0x00100000;
        const WAIT_TIMEOUT: u32 = 0x00000102;

        let handle = OpenProcess(SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let wait_res = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        wait_res == WAIT_TIMEOUT
    }
    #[cfg(not(target_os = "windows"))]
    false
}

pub fn wait_for_process_exit(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

        const SYNCHRONIZE: u32 = 0x00100000;
        const INFINITE: u32 = 0xFFFFFFFF;

        let handle = OpenProcess(SYNCHRONIZE, 0, pid);
        if !handle.is_null() {
            WaitForSingleObject(handle, INFINITE);
            CloseHandle(handle);
        }
    }
}

pub fn kill_process_by_pid(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        // 1. 先使用 Win32 API 立即请求终止目标进程
        unsafe {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::{
                OpenProcess, TerminateProcess, PROCESS_TERMINATE,
            };

            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                let _ = TerminateProcess(handle, 1);
                CloseHandle(handle);
            }
        }

        // 2. 结合 taskkill.exe /PID <pid> /T /F 强制清理所有子进程树
        let _ = new_silent_command("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();

        // 3. 等待并确认进程已彻底销毁（最多 500ms）
        for _ in 0..10 {
            if !is_pid_alive(pid) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    !is_pid_alive(pid)
}

pub fn kill_process_by_name(name: &str) -> bool {
    let mut any_killed = false;
    let exe_name = if name.ends_with(".exe") {
        name.to_string()
    } else {
        format!("{}.exe", name)
    };

    // 1. 先使用 taskkill.exe /F /IM <name.exe> /T 强力杀掉进程树（覆盖其他方式启动的脱钩/多实例进程）
    #[cfg(target_os = "windows")]
    {
        let output = new_silent_command("taskkill.exe")
            .args(["/F", "/IM", &exe_name, "/T"])
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                any_killed = true;
            }
        }
    }

    // 2. 遍历 Toolhelp32 快照中所有匹配的 PID，使用 Win32 API TerminateProcess 再次逐个确认强杀
    let pids = find_all_process_ids(name);
    for pid in pids {
        if kill_process_by_pid(pid) {
            any_killed = true;
        }
    }

    any_killed
}

pub fn kill_all_processes_by_name(name: &str) -> bool {
    kill_process_by_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_process_explorer() {
        #[cfg(target_os = "windows")]
        {
            let running = is_process_running("explorer.exe");
            assert!(running, "explorer.exe should be running on Windows");
            let pid = find_process_id("explorer.exe");
            assert!(pid.is_some(), "explorer.exe should have a valid PID");
            assert!(pid.unwrap() > 0);

            assert!(!is_process_running("non_existent_process_1234567.exe"));
            assert_eq!(find_process_id("non_existent_process_1234567.exe"), None);
            assert!(!is_pid_alive(99999999));
        }
    }

    #[test]
    fn test_spawn_and_kill_process() {
        #[cfg(target_os = "windows")]
        {
            let mut cmd = std::process::Command::new("powershell.exe");
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            cmd.args(["-NoLogo", "-NoProfile", "-Command", "Start-Sleep -Seconds 60"]);
            if let Ok(child) = cmd.spawn() {
                let pid = child.id();
                assert!(is_pid_alive(pid), "Process should be alive after spawn");
                let killed = kill_process_by_pid(pid);
                assert!(killed, "kill_process_by_pid should return true");
                assert!(!is_pid_alive(pid), "Process should not be alive after kill");
            }
        }
    }
}
