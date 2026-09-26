use tokio::sync::broadcast;
use tracing::{info, warn};

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW,
    LoadCursorW, LoadIconW, PostMessageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TrackPopupMenu,
    TranslateMessage, CW_USEDEFAULT, GWLP_USERDATA, HICON, IDC_ARROW, IDI_APPLICATION,
    LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, MSG, SC_RESTORE, SW_MINIMIZE, SW_SHOWMINNOACTIVE,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WA_INACTIVE, WM_ACTIVATE, WM_APP,
    WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL,
    WM_RBUTTONUP, WM_SYSCOMMAND, WNDCLASSEXW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP,
};

#[cfg(target_os = "windows")]
const SW_HIDE: i32 = 0;

#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "system" {
    fn UpdateWindow(hwnd: HWND) -> i32;
    fn OpenDesktopW(
        lpsz_desktop: *const u16,
        dw_flags: u32,
        f_inherit: i32,
        dw_desired_access: u32,
    ) -> isize;
    fn SetThreadDesktop(h_desktop: isize) -> i32;
}

#[cfg(target_os = "windows")]
const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
#[cfg(target_os = "windows")]
const IDM_OPEN_WEB: usize = 1001;
#[cfg(target_os = "windows")]
const IDM_EXIT: usize = 1002;

#[cfg(target_os = "windows")]
struct TrayContext {
    main_hwnd: HWND,
    taskbar_hwnd: HWND,
    nid: NOTIFYICONDATAW,
    wm_taskbar_created: u32,
    url: String,
    shutdown_sender: broadcast::Sender<()>,
}

#[derive(Clone)]
pub struct TrayHandle {
    #[cfg(target_os = "windows")]
    main_hwnd: HWND,
}

unsafe impl Send for TrayHandle {}
unsafe impl Sync for TrayHandle {}

impl TrayHandle {
    pub fn close(&self) {
        #[cfg(target_os = "windows")]
        unsafe {
            if !self.main_hwnd.is_null() {
                PostMessageW(self.main_hwnd, WM_CLOSE, 0, 0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn to_wide_chars(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn load_icon_from_bytes(bytes: &[u8]) -> Option<HICON> {
    if bytes.len() < 6 {
        return None;
    }
    let reserved = u16::from_le_bytes([bytes[0], bytes[1]]);
    let icon_type = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    if reserved != 0 || icon_type != 1 || count == 0 || bytes.len() < 6 + 16 * count {
        return None;
    }

    let mut best_index = 0;
    let mut best_score = i32::MAX;
    for i in 0..count {
        let entry = 6 + 16 * i;
        let mut width = bytes[entry] as i32;
        if width == 0 {
            width = 256;
        }
        let score = if width < 16 {
            width + 1000
        } else if width > 32 {
            (width - 32) * 4
        } else {
            32 - width
        };
        if score < best_score {
            best_score = score;
            best_index = i;
        }
    }

    let entry = 6 + 16 * best_index;
    let bytes_in_res = u32::from_le_bytes([bytes[entry + 8], bytes[entry + 9], bytes[entry + 10], bytes[entry + 11]]) as usize;
    let offset = u32::from_le_bytes([bytes[entry + 12], bytes[entry + 13], bytes[entry + 14], bytes[entry + 15]]) as usize;

    if offset + bytes_in_res > bytes.len() {
        return None;
    }

    let img_data = &bytes[offset..offset + bytes_in_res];
    unsafe {
        let hicon = CreateIconFromResourceEx(
            img_data.as_ptr(),
            img_data.len() as u32,
            1, // 1 = icon
            0x00030000,
            0,
            0,
            LR_DEFAULTCOLOR,
        );
        if !hicon.is_null() {
            Some(hicon)
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn load_tray_icon() -> HICON {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let cx_sm = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSMICON);
        let cy_sm = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSMICON);
        let hicon = windows_sys::Win32::UI::WindowsAndMessaging::LoadImageW(
            instance,
            1 as _,
            windows_sys::Win32::UI::WindowsAndMessaging::IMAGE_ICON,
            cx_sm,
            cy_sm,
            LR_DEFAULTCOLOR,
        ) as HICON;
        if !hicon.is_null() {
            return hicon;
        }

        let hicon = LoadIconW(instance, 1 as _);
        if !hicon.is_null() {
            return hicon;
        }

        const ICON_BYTES: &[u8] = include_bytes!("../assets/tray.ico");
        if let Some(hicon) = load_icon_from_bytes(ICON_BYTES) {
            return hicon;
        }

        LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayContext;
    if ctx_ptr.is_null() {
        if msg == WM_CREATE {
            let cs = lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
            if !cs.is_null() && !(*cs).lpCreateParams.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
            }
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let ctx = &mut *ctx_ptr;

    if msg == ctx.wm_taskbar_created && ctx.wm_taskbar_created != 0 {
        let _ = Shell_NotifyIconW(NIM_ADD, &mut ctx.nid);
        return 0;
    }

    match msg {
        WM_TRAY_CALLBACK => {
            match lparam as u32 {
                WM_RBUTTONUP => {
                    let mut pt = POINT { x: 0, y: 0 };
                    GetCursorPos(&mut pt);
                    SetForegroundWindow(ctx.main_hwnd);

                    let hmenu = CreatePopupMenu();
                    if !hmenu.is_null() {
                        let open_str = to_wide_chars("打开网页");
                        let exit_str = to_wide_chars("退出 code-Manager-rust");
                        AppendMenuW(hmenu, MF_STRING, IDM_OPEN_WEB, open_str.as_ptr());
                        AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
                        AppendMenuW(hmenu, MF_STRING, IDM_EXIT, exit_str.as_ptr());

                        TrackPopupMenu(
                            hmenu,
                            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                            pt.x,
                            pt.y,
                            0,
                            ctx.main_hwnd,
                            std::ptr::null(),
                        );
                        DestroyMenu(hmenu);
                        PostMessageW(ctx.main_hwnd, WM_NULL, 0, 0);
                    }
                    0
                }
                WM_LBUTTONDBLCLK => {
                    let _ = open::that(&ctx.url);
                    0
                }
                WM_LBUTTONUP => {
                    // A single left click intentionally does nothing.
                    0
                }
                _ => 0,
            }
        }
        WM_COMMAND => {
            let id = wparam as usize;
            if id == IDM_OPEN_WEB {
                let _ = open::that(&ctx.url);
            } else if id == IDM_EXIT {
                let _ = ctx.shutdown_sender.send(());
                PostMessageW(ctx.main_hwnd, WM_CLOSE, 0, 0);
            }
            0
        }
        WM_SYSCOMMAND => {
            if hwnd == ctx.taskbar_hwnd && (wparam & 0xFFF0) as u32 == SC_RESTORE {
                let _ = open::that(&ctx.url);
                ShowWindow(ctx.taskbar_hwnd, SW_MINIMIZE);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ACTIVATE => {
            if hwnd == ctx.taskbar_hwnd && (wparam as u16) != WA_INACTIVE as u16 {
                ShowWindow(ctx.taskbar_hwnd, SW_MINIMIZE);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            if hwnd == ctx.taskbar_hwnd {
                DestroyWindow(ctx.taskbar_hwnd);
                return 0;
            }
            if !ctx.taskbar_hwnd.is_null() {
                DestroyWindow(ctx.taskbar_hwnd);
                ctx.taskbar_hwnd = std::ptr::null_mut();
            }
            DestroyWindow(ctx.main_hwnd);
            0
        }
        WM_DESTROY => {
            if hwnd == ctx.taskbar_hwnd {
                ctx.taskbar_hwnd = std::ptr::null_mut();
                return 0;
            }
            if !ctx.nid.hIcon.is_null() {
                Shell_NotifyIconW(NIM_DELETE, &mut ctx.nid);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn start_tray(
    shutdown_sender: broadcast::Sender<()>,
    launch_url: String,
) -> Option<TrayHandle> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (shutdown_sender, launch_url);
        None
    }

    #[cfg(target_os = "windows")]
    {
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::Builder::new()
            .name("code-manager-tray".into())
            .spawn(move || unsafe {
                // Ensure thread is attached to the interactive "Default" desktop
                let default_desktop = OpenDesktopW(
                    to_wide_chars("Default").as_ptr(),
                    0,
                    0,
                    0x10000000,
                );
                if default_desktop != 0 {
                    let _ = SetThreadDesktop(default_desktop);
                }

                info!("tray thread: 正在获取模块句柄与加载图标...");
                let instance = GetModuleHandleW(std::ptr::null());
                let icon = load_tray_icon();
                info!("tray thread: 图标加载完成 (is_null={})，正在注册窗口类...", icon.is_null());
                let class_name = to_wide_chars("CodeManagerRustTrayClass");

                let mut wcex: WNDCLASSEXW = std::mem::zeroed();
                wcex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
                wcex.lpfnWndProc = Some(wnd_proc);
                wcex.hInstance = instance;
                wcex.hIcon = icon;
                wcex.hIconSm = icon;
                wcex.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
                wcex.lpszClassName = class_name.as_ptr();

                RegisterClassExW(&wcex);

                let taskbar_created_msg = RegisterWindowMessageW(to_wide_chars("TaskbarCreated").as_ptr());

                let ctx = Box::new(TrayContext {
                    main_hwnd: std::ptr::null_mut(),
                    taskbar_hwnd: std::ptr::null_mut(),
                    nid: std::mem::zeroed(),
                    wm_taskbar_created: taskbar_created_msg,
                    url: launch_url,
                    shutdown_sender,
                });

                let ctx_ptr = Box::into_raw(ctx);

                info!("tray thread: 正在创建隐藏主窗口...");
                let main_hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    to_wide_chars("").as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    ctx_ptr as *const std::ffi::c_void,
                );

                if main_hwnd.is_null() {
                    warn!("创建托盘隐藏主窗口失败");
                    let _ = Box::from_raw(ctx_ptr);
                    let _ = tx.send(None);
                    return;
                }

                (*ctx_ptr).main_hwnd = main_hwnd;
                SetWindowLongPtrW(main_hwnd, GWLP_USERDATA, ctx_ptr as isize);
                ShowWindow(main_hwnd, SW_HIDE);
                UpdateWindow(main_hwnd);

                info!("tray thread: 主隐藏窗口创建完成，正在初始化托盘图标...");
                let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = main_hwnd;
                nid.uID = 100;
                nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                nid.uCallbackMessage = WM_TRAY_CALLBACK;
                nid.hIcon = icon;
                let tip = to_wide_chars("code-Manager-rust 本地网关");
                let tip_len = tip.len().min(nid.szTip.len() - 1);
                nid.szTip[..tip_len].copy_from_slice(&tip[..tip_len]);

                let mut added = Shell_NotifyIconW(NIM_ADD, &mut nid) != 0;
                if !added {
                    for _ in 1..=5 {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        if Shell_NotifyIconW(NIM_ADD, &mut nid) != 0 {
                            added = true;
                            break;
                        }
                    }
                }

                if added {
                    info!("系统托盘图标已就绪（提示: code-Manager-rust 本地网关）");
                } else {
                    let err = GetLastError();
                    warn!("未能向 Windows 通知区域添加托盘图标 (Win32 Error: {})，将保持主服务继续运行", err);
                }

                (*ctx_ptr).nid = nid;

                // Create Taskbar Window (WS_EX_APPWINDOW)
                let taskbar_title = to_wide_chars("code-Manager-rust");
                let taskbar_hwnd = CreateWindowExW(
                    WS_EX_APPWINDOW,
                    class_name.as_ptr(),
                    taskbar_title.as_ptr(),
                    WS_POPUP,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    1,
                    1,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    ctx_ptr as *const std::ffi::c_void,
                );

                if !taskbar_hwnd.is_null() {
                    (*ctx_ptr).taskbar_hwnd = taskbar_hwnd;
                    SetWindowLongPtrW(taskbar_hwnd, GWLP_USERDATA, ctx_ptr as isize);
                    ShowWindow(taskbar_hwnd, SW_SHOWMINNOACTIVE);
                    info!("Windows 底部任务栏图标按钮已就绪");
                } else {
                    warn!("创建任务栏按钮窗口失败");
                }

                let handle = TrayHandle { main_hwnd };
                let _ = tx.send(Some(handle));

                // Message Loop
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                // Clean up context
                let _ = Box::from_raw(ctx_ptr);
                info!("系统托盘与任务栏服务已安全退出");
            })
            .ok()?;

        rx.recv().ok().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn test_tray_icon_registration() {
        unsafe {
            let default_desktop = OpenDesktopW(
                to_wide_chars("Default").as_ptr(),
                0,
                0,
                0x10000000,
            );
            if default_desktop != 0 {
                let _ = SetThreadDesktop(default_desktop);
            }

            let class_name = to_wide_chars("TestTrayClassRust");
            let mut wcex: WNDCLASSEXW = std::mem::zeroed();
            wcex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wcex.lpfnWndProc = Some(DefWindowProcW);
            wcex.hInstance = GetModuleHandleW(std::ptr::null());
            wcex.lpszClassName = class_name.as_ptr();
            RegisterClassExW(&wcex);

            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                to_wide_chars("").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                wcex.hInstance,
                std::ptr::null(),
            );
            assert!(!hwnd.is_null());

            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 100;
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_APP + 1;
            nid.hIcon = load_tray_icon();
            let tip = to_wide_chars("code-Manager-rust 本地网关");
            let tip_len = tip.len().min(nid.szTip.len() - 1);
            nid.szTip[..tip_len].copy_from_slice(&tip[..tip_len]);

            let add_ret = Shell_NotifyIconW(NIM_ADD, &mut nid);
            assert_ne!(add_ret, 0, "Shell_NotifyIconW NIM_ADD should succeed on Default desktop");

            let del_ret = Shell_NotifyIconW(NIM_DELETE, &mut nid);
            assert_ne!(del_ret, 0, "Shell_NotifyIconW NIM_DELETE should succeed");

            DestroyWindow(hwnd);
        }
    }
}
