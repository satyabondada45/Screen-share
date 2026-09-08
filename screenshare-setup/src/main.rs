#![windows_subsystem = "windows"]

use std::env;
use std::mem;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows_sys::Win32::System::Registry::{
    HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, KEY_READ, KEY_WRITE,
    REG_SZ, RegDeleteTreeW,
};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetMessageW, LoadCursorW, PostMessageW, PostQuitMessage,
    RegisterClassW, SetWindowLongPtrW, ShowWindow, TranslateMessage, WM_CLOSE, WM_COMMAND,
    WM_CREATE, WM_DESTROY, WNDCLASSW, CREATESTRUCTW, IDC_ARROW, SW_HIDE, SW_SHOW, WS_CAPTION,
    WS_CHILD, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, GWLP_USERDATA, MSG, HMENU,
};

// Embed the already-built binaries directly into the installer.
static SCREENSHARE: &[u8] =
    include_bytes!("../../installer_staging/ScreenShare.exe");
static DESKTOP_AGENT: &[u8] =
    include_bytes!("../../installer_staging/desktop-agent.exe");
static RELAY_SERVER: &[u8] =
    include_bytes!("../../installer_staging/relay-server.exe");

const ID_LABEL: i32 = 1;
const ID_BTN: i32 = 2;

fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

fn set_text(hwnd: HWND, s: &str) {
    let w = to_wide(s);
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, w.as_ptr()) };
}

struct SetupState {
    hwnd: HWND,
    label: HWND,
    btn: HWND,
}

fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

fn elevate_and_relaunch(args: &[String]) {
    let exe = env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let argstr = args[1..].join(" ");
    let op = to_wide("runas");
    let e = to_wide(&exe);
    let a = to_wide(&argstr);
    unsafe {
        ShellExecuteW(0, op.as_ptr(), e.as_ptr(), a.as_ptr(), ptr::null(), 1);
    }
}

fn msgbox(title: &str, text: &str) {
    let t = to_wide(title);
    let x = to_wide(text);
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(0, x.as_ptr(), t.as_ptr(), 0) };
}

fn set_run_key(name: &str, value: &str) {
    let mut hkey = 0isize;
    let sub = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    unsafe {
        if windows_sys::Win32::System::Registry::RegCreateKeyW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            &mut hkey,
        ) == 0
        {
            let vn = to_wide(name);
            let d = to_wide(value);
            windows_sys::Win32::System::Registry::RegSetValueExW(
                hkey,
                vn.as_ptr(),
                0,
                REG_SZ,
                d.as_ptr() as *const u8,
                (d.len() * 2) as u32,
            );
            windows_sys::Win32::System::Registry::RegCloseKey(hkey);
        }
    }
}

fn delete_run_key(name: &str) {
    let mut hkey = 0isize;
    let sub = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    unsafe {
        if windows_sys::Win32::System::Registry::RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_WRITE | KEY_WOW64_64KEY,
            &mut hkey,
        ) == 0
        {
            let vn = to_wide(name);
            windows_sys::Win32::System::Registry::RegDeleteValueW(hkey, vn.as_ptr());
            windows_sys::Win32::System::Registry::RegCloseKey(hkey);
        }
    }
}

fn reg_set_hklm(subkey: &str, value_name: &str, data: &str) {
    let mut hkey = 0isize;
    let sk = to_wide(subkey);
    unsafe {
        if windows_sys::Win32::System::Registry::RegCreateKeyW(
            HKEY_LOCAL_MACHINE,
            sk.as_ptr(),
            &mut hkey,
        ) == 0
        {
            let vn = to_wide(value_name);
            let d = to_wide(data);
            windows_sys::Win32::System::Registry::RegSetValueExW(
                hkey,
                vn.as_ptr(),
                0,
                REG_SZ,
                d.as_ptr() as *const u8,
                (d.len() * 2) as u32,
            );
            windows_sys::Win32::System::Registry::RegCloseKey(hkey);
        }
    }
}

fn reg_delete_hklm(subkey_full: &str) {
    let (parent, leaf) = match subkey_full.rsplit_once('\\') {
        Some((p, l)) => (p, l),
        None => ("", subkey_full),
    };
    let mut hkey = 0isize;
    let pk = to_wide(parent);
    unsafe {
        if windows_sys::Win32::System::Registry::RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            pk.as_ptr(),
            0,
            KEY_WRITE | KEY_WOW64_64KEY,
            &mut hkey,
        ) == 0
        {
            let lk = to_wide(leaf);
            windows_sys::Win32::System::Registry::RegDeleteKeyExW(
                hkey,
                lk.as_ptr(),
                KEY_WOW64_64KEY,
                0,
            );
            windows_sys::Win32::System::Registry::RegCloseKey(hkey);
        }
    }
}

fn register_protocol(agent_path: &str) {
    let cmd = format!("\"{}\"", agent_path);
    // HKEY_CLASSES_ROOT\deskstream
    {
        let mut hkey = 0isize;
        let sk = to_wide("deskstream");
        unsafe {
            if windows_sys::Win32::System::Registry::RegCreateKeyW(
                HKEY_CLASSES_ROOT,
                sk.as_ptr(),
                &mut hkey,
            ) == 0
            {
                let d = to_wide("URL:DeskStream Protocol");
                windows_sys::Win32::System::Registry::RegSetValueExW(
                    hkey,
                    ptr::null(),
                    0,
                    REG_SZ,
                    d.as_ptr() as *const u8,
                    ((d.len() - 1) * 2) as u32,
                );
                let empty = to_wide("");
                windows_sys::Win32::System::Registry::RegSetValueExW(
                    hkey,
                    to_wide("URL Protocol").as_ptr(),
                    0,
                    REG_SZ,
                    empty.as_ptr() as *const u8,
                    0,
                );
                windows_sys::Win32::System::Registry::RegCloseKey(hkey);
            }
        }
    }
    // HKEY_CLASSES_ROOT\deskstream\shell\open\command
    {
        let mut hkey = 0isize;
        let sk = to_wide("deskstream\\shell\\open\\command");
        unsafe {
            if windows_sys::Win32::System::Registry::RegCreateKeyW(
                HKEY_CLASSES_ROOT,
                sk.as_ptr(),
                &mut hkey,
            ) == 0
            {
                let d = to_wide(&cmd);
                windows_sys::Win32::System::Registry::RegSetValueExW(
                    hkey,
                    ptr::null(),
                    0,
                    REG_SZ,
                    d.as_ptr() as *const u8,
                    ((d.len() - 1) * 2) as u32,
                );
                windows_sys::Win32::System::Registry::RegCloseKey(hkey);
            }
        }
    }
}

fn unregister_protocol() {
    unsafe {
        RegDeleteTreeW(HKEY_CLASSES_ROOT, to_wide("deskstream").as_ptr());
    }
}

fn create_shortcut(lnk: &str, target: &str) {
    let dir = std::path::Path::new(target)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let ps = format!(
        "$ws=New-Object -ComObject WScript.Shell; $l=$ws.CreateShortcut('{}'); $l.TargetPath='{}'; $l.WorkingDirectory='{}'; $l.Save()",
        lnk.replace('\'', "''"),
        target.replace('\'', "''"),
        dir.replace('\'', "''")
    );
    let tmp = std::env::temp_dir().join("ss_mklink.ps1");
    if std::fs::write(&tmp, ps).is_ok() {
        let _ = std::process::Command::new("powershell")
            .args(["-ExecutionPolicy", "Bypass", "-File", tmp.to_str().unwrap_or("")])
            .output();
    }
}

fn program_dir() -> String {
    let pf = env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
    format!("{}\\Screen Share", pf)
}

fn write_server_config(dir: &str, server_addr: &str) {
    let config_path = format!("{}\\server-config.json", dir);
    let json = format!(
        "{{\n  \"server_addr\": \"{}\",\n  \"backend_url\": \"http://{}/Screen%20Share/backend/api\",\n  \"relay_url\": \"127.0.0.1:9001\"\n}}\n",
        server_addr, server_addr,
    );
    let _ = std::fs::write(&config_path, json);
}

fn do_install(label: HWND) {
    let dir = program_dir();
    set_text(label, "Installing Screen Share...");
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "desktop-agent.exe"])
        .output();
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "ScreenShare.exe"])
        .output();
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "relay-server.exe"])
        .output();
    std::thread::sleep(Duration::from_millis(500));

    let _ = std::fs::create_dir_all(&dir);

    let gui_path = format!("{}\\ScreenShare.exe", dir);
    let agent_path = format!("{}\\desktop-agent.exe", dir);
    let relay_path = format!("{}\\relay-server.exe", dir);
    let _ = std::fs::write(&gui_path, SCREENSHARE);
    let _ = std::fs::write(&agent_path, DESKTOP_AGENT);
    let _ = std::fs::write(&relay_path, RELAY_SERVER);

    // Resolve production server address from installer argument or config file
    let server_addr = resolve_server_addr();
    if server_addr.is_empty() {
        // No server address provided — this is likely a local dev install.
        // Write a default localhost config so the GUI still works locally.
        write_server_config(&dir, "127.0.0.1");
    } else {
        write_server_config(&dir, &server_addr);
    }

    // Start with Windows (HKCU Run -> the GUI owns the agent lifecycle)
    set_run_key("ScreenShare", &format!("\"{}\" --hidden", gui_path));

    // Register the deskstream:// custom URL protocol (points directly to the agent)
    register_protocol(&agent_path);

    // Uninstall entry (HKLM)
    let setup_exe = env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let uninstall_str = format!("\"{}\" /uninstall", setup_exe);
    let base = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ScreenShare";
    reg_set_hklm(base, "DisplayName", "Screen Share");
    reg_set_hklm(base, "UninstallString", &uninstall_str);
    reg_set_hklm(base, "DisplayIcon", &gui_path);
    reg_set_hklm(base, "InstallLocation", &dir);
    reg_set_hklm(base, "Publisher", "Screen Share");
    reg_set_hklm(base, "NoModify", "1");
    reg_set_hklm(base, "NoRepair", "1");

    // Shortcuts
    let progdata = env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    let start_menu = format!(
        "{}\\Microsoft\\Windows\\Start Menu\\Programs\\Screen Share.lnk",
        progdata
    );
    create_shortcut(&start_menu, &gui_path);
    if let Ok(user) = env::var("USERPROFILE") {
        let desktop = format!("{}\\Desktop\\Screen Share.lnk", user);
        create_shortcut(&desktop, &gui_path);
    }

    set_text(
        label,
        "Installation complete.\n\nScreen Share has been installed and will start with Windows.\nClick Finish to open it.",
    );

    // Launch the application
    let _ = std::process::Command::new(&gui_path).spawn();
}

fn resolve_server_addr() -> String {
    // Check for an existing server-config.json in the install directory (preserved across reinstalls)
    let existing_config = format!(
        "{}\\server-config.json",
        program_dir()
    );
    if let Ok(contents) = std::fs::read_to_string(&existing_config) {
        if let Some(addr) = extract_server_addr(&contents) {
            if !addr.is_empty() && !addr.starts_with("127.0.0.1") && !addr.starts_with("localhost") {
                return addr;
            }
        }
    }

    // Check CLI args
    let args: Vec<String> = env::args().collect();
    for arg in &args[1..] {
        if arg.starts_with("--server-addr=") {
            let val = arg.trim_start_matches("--server-addr=").to_string();
            if !val.is_empty() {
                return val;
            }
        }
    }

    // Check for a config file next to the installer
    if let Some(installer_dir) = env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let cfg_path = installer_dir.join("server-addr.txt");
        if let Ok(addr) = std::fs::read_to_string(&cfg_path) {
            let trimmed = addr.trim().to_string();
            if !trimmed.is_empty() && !trimmed.starts_with("127.0.0.1") && !trimmed.starts_with("localhost") {
                return trimmed;
            }
        }
    }

    String::new()
}

fn extract_server_addr(contents: &str) -> Option<String> {
    let key = "\"server_addr\":";
    let pos = contents.find(key)?;
    let after = &contents[pos + key.len()..];
    let start = after.find('"')? + 1;
    let end = after[start..].find('"')? + start;
    Some(after[start..end].to_string())
}

fn do_uninstall() {
    // Stop running processes
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "desktop-agent.exe"])
        .output();
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "ScreenShare.exe"])
        .output();
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "relay-server.exe"])
        .output();
    std::thread::sleep(Duration::from_millis(500));

    // Remove the deskstream:// custom URL protocol registration
    unregister_protocol();

    // Remove startup entries (GUI owns these)
    delete_run_key("ScreenShare");
    delete_run_key("ScreenShareAgent");

    // Remove installed files (but preserve server-config.json for reinstall)
    let dir = program_dir();
    let server_config = std::fs::read_to_string(format!("{}\\server-config.json", dir)).ok();
    let _ = std::fs::remove_dir_all(&dir);

    // Remove shortcuts
    if let Ok(pd) = env::var("ProgramData") {
        let _ = std::fs::remove_file(format!(
            "{}\\Microsoft\\Windows\\Start Menu\\Programs\\Screen Share.lnk",
            pd
        ));
    }
    if let Ok(user) = env::var("USERPROFILE") {
        let _ = std::fs::remove_file(format!("{}\\Desktop\\Screen Share.lnk", user));
    }

    // Remove uninstall registry entry
    reg_delete_hklm("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ScreenShare");

    // Restore server-config.json for future reinstalls (preserves production server address)
    if let Some(cfg) = server_config {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(format!("{}\\server-config.json", dir), cfg);
    }

    // NOTE: %LOCALAPPDATA% data (System ID, config, logs) is intentionally kept.
    msgbox(
        "Screen Share",
        "Screen Share has been uninstalled.\n\nYour device identity and logs were kept.",
    );
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match msg {
        WM_CREATE => {
            let cs = lparam as *const CREATESTRUCTW;
            let st = (*cs).lpCreateParams as *mut SetupState;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, st as isize);
            let st = &mut *st;
            st.hwnd = hwnd;

            let cls = to_wide("STATIC");
            let t = to_wide("");
            st.label = CreateWindowExW(
                0,
                cls.as_ptr(),
                t.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                20,
                20,
                420,
                180,
                hwnd,
                ID_LABEL as HMENU,
                0,
                ptr::null(),
            );
            set_font(st.label);

            let bcls = to_wide("BUTTON");
            let bt = to_wide("Finish");
            st.btn = CreateWindowExW(
                0,
                bcls.as_ptr(),
                bt.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                170,
                220,
                140,
                34,
                hwnd,
                ID_BTN as HMENU,
                0,
                ptr::null(),
            );
            set_font(st.btn);

            do_install(st.label);
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as i32;
            if id == ID_BTN {
                PostQuitMessage(0);
            }
            0
        }
        WM_CLOSE => {
            PostQuitMessage(0);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn set_font(hwnd: HWND) {
    let font = unsafe { windows_sys::Win32::Graphics::Gdi::GetStockObject(17) } as usize;
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, 0x0030, font, 1);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let uninstall = args
        .iter()
        .any(|a| a.eq_ignore_ascii_case("/uninstall") || a.eq_ignore_ascii_case("/u"));

    if !is_admin() {
        elevate_and_relaunch(&args);
        return;
    }

    if uninstall {
        do_uninstall();
        return;
    }

    let silent = args
        .iter()
        .any(|a| a.eq_ignore_ascii_case("/S") || a.eq_ignore_ascii_case("/silent"));

    if silent {
        do_install(0);
        return;
    }

    // Install UI
    let mut state = Box::new(SetupState {
        hwnd: 0,
        label: 0,
        btn: 0,
    });
    let state_ptr = Box::into_raw(state) as *mut std::ffi::c_void;

    let class_name = to_wide("ScreenShareSetupCls");
    let hinst: HINSTANCE = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(ptr::null()) };

    let mut wc: WNDCLASSW = unsafe { mem::zeroed() };
    wc.style = 0x0002 | 0x0001;
    wc.lpfnWndProc = Some(wndproc);
    wc.hInstance = hinst;
    wc.hCursor = unsafe { LoadCursorW(0, IDC_ARROW) };
    wc.hbrBackground = (COLOR_WINDOW + 1) as HBRUSH;
    wc.lpszClassName = class_name.as_ptr();
    unsafe { RegisterClassW(&wc) };

    let title = to_wide("Screen Share Setup");
    let sw = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(0) };
    let sh = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(1) };
    let w = 460i32;
    let h = 300i32;
    let x = (sw - w) / 2;
    let y = (sh - h) / 2;

    let _hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            x,
            y,
            w,
            h,
            0,
            0,
            hinst,
            state_ptr,
        )
    };

    let mut msg: MSG = unsafe { mem::zeroed() };
    unsafe {
        while windows_sys::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, 0, 0, 0) > 0 {
            windows_sys::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
            windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
        }
    }

    // Keep the agent/GUI running; just ensure nothing is left hanging.
    let _ = SW_HIDE;
}
