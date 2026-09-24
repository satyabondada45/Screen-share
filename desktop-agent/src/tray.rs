// src/tray.rs
// Native Win32 system tray icon for DeskStream Agent.
//
// Uses a message-only Win32 window on a dedicated thread.
// Communicates exit intent back to the main thread via Arc<AtomicBool>.
// Does NOT depend on minifb or any third-party crate.
#![allow(unused_unsafe)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Win32 items needed for tray
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NOTIFYICONDATAW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ============================================================
// Tray message / menu IDs
// ============================================================
const WM_TRAYICON: u32 = WM_USER + 1;
const TRAY_UID: u32 = 1;

const IDM_OPEN: usize = 200;
const IDM_SETTINGS: usize = 201;
const IDM_LOGS: usize = 202;
const IDM_END_SESSION: usize = 203;
const IDM_RESTART: usize = 204;
const IDM_EXIT: usize = 205;

// ============================================================
// Shared state passed to tray thread
// ============================================================
pub struct TrayState {
    /// Set true by tray's Exit menu item to signal app quit
    pub quit: Arc<AtomicBool>,
}

// ============================================================
// Public entry point
// ============================================================

/// Spawn the tray icon on a dedicated background thread.
/// Returns a TrayState whose `quit` flag is set when the user picks "Exit".
pub fn start_tray(quit: Arc<AtomicBool>) {
    let q = quit.clone();
    thread::Builder::new()
        .name("deskstream-tray".into())
        .spawn(move || unsafe {
            tray_thread(q);
        })
        .expect("Failed to spawn tray thread");
}

// ============================================================
// Internal helpers
// ============================================================

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_to_fixed<const N: usize>(s: &str) -> [u16; N] {
    let mut buf = [0u16; N];
    for (i, c) in s.encode_utf16().enumerate().take(N - 1) {
        buf[i] = c;
    }
    buf
}

// ============================================================
// Window procedure for the message-only tray window
// ============================================================

static QUIT_SIGNAL: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            let event = (lparam & 0xFFFF) as u32;
            match event {
                WM_LBUTTONDBLCLK | WM_LBUTTONUP => {
                    // Show/restore the DeskStream Agent window
                    let title = to_wide("DeskStream Agent");
                    let main_hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                    if main_hwnd != 0 {
                        ShowWindow(main_hwnd, SW_RESTORE);
                        SetForegroundWindow(main_hwnd);
                    }
                }
                WM_RBUTTONUP => {
                    // Show context menu at cursor position
                    let hmenu = CreatePopupMenu();
                    if hmenu != 0 {
                        let open_str = to_wide("Open DeskStream Agent\0");
                        let settings_str = to_wide("Settings\0");
                        let logs_str = to_wide("View Logs\0");
                        let sep = to_wide("\0");
                        let end_str = to_wide("End Remote Session\0");
                        let restart_str = to_wide("Restart Agent\0");
                        let exit_str = to_wide("Exit\0");

                        AppendMenuW(hmenu, MF_STRING, IDM_OPEN, open_str.as_ptr());
                        AppendMenuW(hmenu, MF_SEPARATOR, 0, sep.as_ptr());
                        AppendMenuW(hmenu, MF_STRING, IDM_SETTINGS, settings_str.as_ptr());
                        AppendMenuW(hmenu, MF_STRING, IDM_LOGS, logs_str.as_ptr());
                        AppendMenuW(hmenu, MF_SEPARATOR, 0, sep.as_ptr());
                        AppendMenuW(hmenu, MF_STRING, IDM_END_SESSION, end_str.as_ptr());
                        AppendMenuW(hmenu, MF_STRING, IDM_RESTART, restart_str.as_ptr());
                        AppendMenuW(hmenu, MF_SEPARATOR, 0, sep.as_ptr());
                        AppendMenuW(hmenu, MF_STRING, IDM_EXIT, exit_str.as_ptr());

                        // Required for popup menus in system tray
                        SetForegroundWindow(hwnd);

                        let mut pt = POINT { x: 0, y: 0 };
                        GetCursorPos(&mut pt);

                        TrackPopupMenu(
                            hmenu,
                            TPM_RIGHTALIGN | TPM_BOTTOMALIGN | TPM_LEFTBUTTON,
                            pt.x,
                            pt.y,
                            0,
                            hwnd,
                            std::ptr::null(),
                        );
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                        DestroyMenu(hmenu);
                    }
                }
                _ => {}
            }
            0
        }

        WM_COMMAND => {
            let cmd = (wparam & 0xFFFF) as usize;
            match cmd {
                IDM_OPEN => {
                    let title = to_wide("DeskStream Agent");
                    let main_hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                    if main_hwnd != 0 {
                        ShowWindow(main_hwnd, SW_RESTORE);
                        SetForegroundWindow(main_hwnd);
                    }
                }
                IDM_SETTINGS => {
                    // Open the Settings tab — bring window to front
                    let title = to_wide("DeskStream Agent");
                    let main_hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                    if main_hwnd != 0 {
                        ShowWindow(main_hwnd, SW_RESTORE);
                        SetForegroundWindow(main_hwnd);
                    }
                }
                IDM_LOGS => {
                    let title = to_wide("DeskStream Agent");
                    let main_hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                    if main_hwnd != 0 {
                        ShowWindow(main_hwnd, SW_RESTORE);
                        SetForegroundWindow(main_hwnd);
                    }
                }
                IDM_END_SESSION => {
                    // TODO: signal agent engine to end session
                }
                IDM_RESTART => {
                    // Restart: bring window up; the agent reconnects automatically
                    let title = to_wide("DeskStream Agent");
                    let main_hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
                    if main_hwnd != 0 {
                        ShowWindow(main_hwnd, SW_RESTORE);
                        SetForegroundWindow(main_hwnd);
                    }
                }
                IDM_EXIT => {
                    // Signal quit to main thread
                    if let Some(q) = QUIT_SIGNAL.get() {
                        q.store(true, Ordering::Relaxed);
                    }
                    PostQuitMessage(0);
                }
                _ => {}
            }
            0
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ============================================================
// Tray thread
// ============================================================

unsafe fn tray_thread(quit: Arc<AtomicBool>) {
    // Store quit signal in static so the wndproc can access it
    let _ = QUIT_SIGNAL.set(quit.clone());

    let hinstance = GetModuleHandleW(std::ptr::null());

    // Register a message-only window class for tray messages
    let class_name = to_wide("DeskStream_TrayClass_v2");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(tray_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: 0,
        hCursor: 0,
        hbrBackground: 0,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: 0,
    };

    RegisterClassExW(&wc);

    // Create a message-only hidden window (parent = HWND_MESSAGE)
    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        to_wide("DeskStream Tray").as_ptr(),
        WS_OVERLAPPEDWINDOW, // style (window won't be shown)
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        HWND_MESSAGE, // message-only, not visible
        0,
        hinstance,
        std::ptr::null(),
    );

    if hwnd == 0 {
        return;
    }

    // Load icon from the EXE's embedded resources (set by build.rs / winres)
    let hicon = LoadIconW(hinstance, IDI_APPLICATION as *const u16);
    // If that fails, use the system default
    let hicon = if hicon == 0 {
        LoadIconW(0, IDI_APPLICATION as *const u16)
    } else {
        hicon
    };

    // Register the tray icon
    let mut nid = std::mem::zeroed::<NOTIFYICONDATAW>();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = hicon;
    // Tooltip text
    let tip = wide_to_fixed::<128>("DeskStream Agent - Connected");
    nid.szTip = tip;

    Shell_NotifyIconW(NIM_ADD, &nid);

    // Message loop
    let mut msg = std::mem::zeroed::<MSG>();
    loop {
        // Check if main thread asked us to quit
        if quit.load(Ordering::Relaxed) {
            Shell_NotifyIconW(NIM_DELETE, &nid);
            break;
        }

        while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_QUIT {
                Shell_NotifyIconW(NIM_DELETE, &nid);
                return;
            }
        }

        thread::sleep(Duration::from_millis(50));
    }
}
