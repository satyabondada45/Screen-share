#![windows_subsystem = "windows"]

use std::mem;
use std::os::windows::process::CommandExt;
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HINSTANCE, HWND, LPARAM, WPARAM,
    ERROR_ALREADY_EXISTS,
};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateSolidBrush, DeleteObject, Ellipse, GetStockObject, PAINTSTRUCT, HBRUSH,
    HGDIOBJ, COLOR_WINDOW,
};
use windows_sys::Win32::System::ApplicationInstallationAndServicing::{ActivateActCtx, CreateActCtxW, ACTCTXW};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Registry::{
    KEY_READ, KEY_WOW64_64KEY, KEY_WRITE, HKEY_CURRENT_USER, REG_SZ,
};
use windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows_sys::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, FindWindowW, GetCursorPos, LoadCursorW,
    RegisterClassW, SendMessageW, SetForegroundWindow,
    SetTimer, SetWindowLongPtrW, ShowWindow, TrackPopupMenu, WM_CLOSE, WM_COMMAND, WM_COPYDATA,
    WM_CREATE, WM_DESTROY, WM_DRAWITEM, WM_GETMINMAXINFO, WM_LBUTTONDBLCLK, WM_PAINT, WM_RBUTTONUP,
    WM_SETFONT, WM_SIZE, WM_TIMER, WM_USER, WNDCLASSW, CREATESTRUCTW, IDC_ARROW, SW_HIDE, SW_RESTORE, SW_SHOW,
    WS_CAPTION, WS_CHILD, WS_SYSMENU, WS_THICKFRAME, WS_TABSTOP, WS_VISIBLE,
    WS_MINIMIZEBOX, WS_MAXIMIZEBOX, GWLP_USERDATA, MSG, GetWindowLongPtrW, HICON, HMENU, MINMAXINFO,
    CreateIconIndirect, ICONINFO,
};
use windows_sys::Win32::System::DataExchange::{OpenClipboard, EmptyClipboard, CloseClipboard, SetClipboardData};

const WM_USER_TRAY: u32 = WM_USER + 1;

// Control style constants (defined as u32 to avoid mixing with WS_* u32 flags)
const SS_LEFT: u32 = 0;
const SS_OWNERDRAW: u32 = 0x000D;
const BS_PUSHBUTTON: u32 = 0;
const BS_AUTOCHECKBOX: u32 = 0x0003;
const BS_GROUPBOX: u32 = 0x0007;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const CBS_HASSTRINGS: u32 = 0x0200;
const ES_LEFT: u32 = 0;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_READONLY: u32 = 0x0800;
const MF_STRING: u32 = 0x0000;
const TPM_RIGHTBUTTON: u32 = 0x0002;

// Main view control ids
const ID_TITLE: i32 = 100;
const ID_STATUS_DOT: i32 = 101;
const ID_STATUS_TEXT: i32 = 102;
const ID_LBL_SYSID: i32 = 103;
const ID_VAL_SYSID: i32 = 104;
const ID_LBL_DEV: i32 = 105;
const ID_VAL_DEV: i32 = 106;
const ID_LBL_CONN: i32 = 107;
const ID_VAL_CONN: i32 = 108;
const ID_LBL_RELAY: i32 = 109;
const ID_VAL_RELAY: i32 = 110;
const ID_LBL_BACK: i32 = 111;
const ID_VAL_BACK: i32 = 112;
const ID_LBL_HB: i32 = 113;
const ID_VAL_HB: i32 = 114;
const ID_LBL_VER: i32 = 115;
const ID_VAL_VER: i32 = 116;
const ID_LBL_ENV: i32 = 117;
const ID_VAL_ENV: i32 = 118;
const ID_GRP_COMPUTER: i32 = 119;
const ID_GRP_CONN: i32 = 120;
const ID_GRP_ABOUT: i32 = 121;
const ID_BTN_SETTINGS: i32 = 122;
const ID_BTN_COPY: i32 = 123;
const ID_BTN_DASHBOARD: i32 = 124;
const ID_BTN_RESTART: i32 = 125;
const ID_TOAST: i32 = 126;

// Settings view control ids
const ID_GRP_SRV: i32 = 200;
const ID_LBL_BE: i32 = 201;
const ID_ED_BE: i32 = 202;
const ID_LBL_RL: i32 = 203;
const ID_ED_RL: i32 = 204;
const ID_LBL_EV: i32 = 205;
const ID_CB_EV: i32 = 206;
const ID_GRP_START: i32 = 207;
const ID_CHK_START: i32 = 208;
const ID_GRP_DEV: i32 = 209;
const ID_LBL_DN: i32 = 210;
const ID_ED_DN: i32 = 211;
const ID_LBL_SID: i32 = 212;
const ID_ST_SID: i32 = 213;
const ID_GRP_LOG: i32 = 214;
const ID_BTN_LOGS: i32 = 215;
const ID_BTN_CFG: i32 = 216;
const ID_BTN_SAVE: i32 = 217;
const ID_BTN_CANCEL: i32 = 218;

// Tray menu ids
const ID_TRAY_OPEN: i32 = 300;
const ID_TRAY_DASHBOARD: i32 = 301;
const ID_TRAY_COPY: i32 = 302;
const ID_TRAY_SETTINGS: i32 = 303;
const ID_TRAY_START: i32 = 304;
const ID_TRAY_STOP: i32 = 305;
const ID_TRAY_RESTART: i32 = 306;
const ID_TRAY_EXIT: i32 = 307;

// Combo / button messages
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETCURSEL: u32 = 0x0147;
const CB_GETLBTEXT: u32 = 0x0148;
const CB_GETLBTEXTLEN: u32 = 0x0149;
const BM_GETCHECK: u32 = 0x00F0;
const BST_CHECKED: isize = 0x0001;

const DEFAULT_GUI_FONT: i32 = 17;

struct AppState {
    hwnd: HWND,
    icon: HICON,
    main_controls: Vec<HWND>,
    settings_controls: Vec<HWND>,
    dpi_scale: f32,

    h_status_text: HWND,
    h_val_sysid: HWND,
    h_val_dev: HWND,
    h_val_conn: HWND,
    h_val_relay: HWND,
    h_val_back: HWND,
    h_val_hb: HWND,
    h_val_ver: HWND,
    h_val_env: HWND,
    h_toast: HWND,

    ed_backend: HWND,
    ed_relay: HWND,
    cb_env: HWND,
    chk_startup: HWND,
    ed_devname: HWND,
    st_sysid: HWND,

    system_id: String,
    device_name: String,
    status_str: String,
    backend_connected: bool,
    relay_connected: bool,
    last_heartbeat_ms: u64,
    version: String,

    backend_url: String,
    relay_url: String,
    environment: String,
    start_with_windows: bool,

    mode: i32,
    last_launch: Instant,
}

impl AppState {
    fn new() -> Self {
        AppState {
            hwnd: 0,
            icon: 0,
            main_controls: Vec::new(),
            settings_controls: Vec::new(),
            h_status_text: 0,
            h_val_sysid: 0,
            h_val_dev: 0,
            h_val_conn: 0,
            h_val_relay: 0,
            h_val_back: 0,
            h_val_hb: 0,
            h_val_ver: 0,
            h_val_env: 0,
            h_toast: 0,
            dpi_scale: 1.0f32,
            ed_backend: 0,
            ed_relay: 0,
            cb_env: 0,
            chk_startup: 0,
            ed_devname: 0,
            st_sysid: 0,
            system_id: String::new(),
            device_name: String::new(),
            status_str: "connecting".to_string(),
            backend_connected: false,
            relay_connected: false,
            last_heartbeat_ms: 0,
            version: env!("CARGO_PKG_VERSION").to_string(),
            backend_url: "http://127.0.0.1/Screen%20Share/backend/api".to_string(),
            relay_url: "127.0.0.1:9001".to_string(),
            environment: "Production".to_string(),
            start_with_windows: true,
            mode: 0,
            last_launch: Instant::now(),
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

fn set_text(hwnd: HWND, s: &str) {
    let w = to_wide(s);
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, w.as_ptr()) };
}

fn scale(s: f32) -> f32 {
    s
}

fn get_dpi_scale(hwnd: HWND) -> f32 {
    let hdc = unsafe { windows_sys::Win32::Graphics::Gdi::GetDC(hwnd) };
    if hdc == 0 {
        return 1.0;
    }
    let dpi_x = unsafe { windows_sys::Win32::Graphics::Gdi::GetDeviceCaps(hdc, windows_sys::Win32::Graphics::Gdi::LOGPIXELSX as i32) };
    let _ = unsafe { windows_sys::Win32::Graphics::Gdi::ReleaseDC(hwnd, hdc) };
    if dpi_x <= 0 { 1.0 } else { dpi_x as f32 / 96.0 }
}

fn sxi(st: &AppState, x: i32) -> i32 {
    (x as f32 * st.dpi_scale) as i32
}

fn syi(st: &AppState, y: i32) -> i32 {
    (y as f32 * st.dpi_scale) as i32
}

fn swi(st: &AppState, w: i32) -> i32 {
    ((w as f32 * st.dpi_scale).ceil()) as i32
}

fn shi(st: &AppState, h: i32) -> i32 {
    ((h as f32 * st.dpi_scale).ceil()) as i32
}

fn get_text(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 4096];
    let n = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, buf.as_mut_ptr(), 4096)
    };
    if n > 0 {
        String::from_utf16_lossy(&buf[..n as usize])
    } else {
        String::new()
    }
}

fn set_font(hwnd: HWND) {
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) } as usize;
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            WM_SETFONT,
            font,
            1,
        );
    }
}

fn make_static(parent: HWND, text: &str, id: i32, style: u32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("STATIC");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_static_hidden(parent: HWND, text: &str, id: i32, style: u32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("STATIC");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | style,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_edit(parent: HWND, id: i32, style: u32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("EDIT");
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | style,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_edit_hidden(parent: HWND, id: i32, style: u32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("EDIT");
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            ptr::null(),
            WS_CHILD | WS_TABSTOP | style,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_button(parent: HWND, text: &str, id: i32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("BUTTON");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_check(parent: HWND, text: &str, id: i32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("BUTTON");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_check_dpi(parent: HWND, text: &str, id: i32, x: i32, y: i32, w: i32, h: i32, scale: f32) -> HWND {
    let cls = to_wide("BUTTON");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
            (x as f32 * scale) as i32,
            (y as f32 * scale) as i32,
            ((w as f32 * scale).ceil()) as i32,
            ((h as f32 * scale).ceil()) as i32,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_group(parent: HWND, text: &str, id: i32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("BUTTON");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_GROUPBOX,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_combo(parent: HWND, id: i32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("COMBOBOX");
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST | CBS_HASSTRINGS,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_combo_hidden(parent: HWND, id: i32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let cls = to_wide("COMBOBOX");
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            ptr::null(),
            WS_CHILD | CBS_DROPDOWNLIST | CBS_HASSTRINGS,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_edit_hidden_dpi(parent: HWND, id: i32, style: u32, x: i32, y: i32, w: i32, h: i32, scale: f32) -> HWND {
    let cls = to_wide("EDIT");
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            ptr::null(),
            WS_CHILD | WS_TABSTOP | style,
            (x as f32 * scale) as i32,
            (y as f32 * scale) as i32,
            ((w as f32 * scale).ceil()) as i32,
            ((h as f32 * scale).ceil()) as i32,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_combo_hidden_dpi(parent: HWND, id: i32, x: i32, y: i32, w: i32, h: i32, scale: f32) -> HWND {
    let cls = to_wide("COMBOBOX");
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            ptr::null(),
            WS_CHILD | CBS_DROPDOWNLIST | CBS_HASSTRINGS,
            (x as f32 * scale) as i32,
            (y as f32 * scale) as i32,
            ((w as f32 * scale).ceil()) as i32,
            ((h as f32 * scale).ceil()) as i32,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn combo_add(hwnd: HWND, text: &str) {
    let w = to_wide(text);
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            CB_ADDSTRING,
            0,
            w.as_ptr() as LPARAM,
        );
    }
}

fn combo_select(hwnd: HWND, idx: i32) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            CB_SETCURSEL,
            idx as usize,
            0,
        );
    }
}

fn combo_selected(hwnd: HWND) -> String {
    let idx = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, CB_GETCURSEL, 0, 0)
    } as i32;
    if idx < 0 {
        return String::new();
    }
    let len = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            CB_GETLBTEXTLEN,
            idx as usize,
            0,
        )
    } as i32;
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            CB_GETLBTEXT,
            idx as usize,
            buf.as_mut_ptr() as LPARAM,
        );
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn is_checked(hwnd: HWND) -> bool {
    let r = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, BM_GETCHECK, 0, 0)
    };
    r == BST_CHECKED
}

// ------------------------------------------------------------
// JSON helpers (minimal, for our own simple payloads)
// ------------------------------------------------------------
fn json_get(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let idx = json.find(&pat)? + pat.len();
    let bytes = json.as_bytes();
    let mut i = idx;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r' || bytes[i] == b'\n') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    i += 1;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r' || bytes[i] == b'\n') {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'"' {
        i += 1;
        let start = i;
        let mut end = i;
        while end < bytes.len() && bytes[end] != b'"' {
            end += 1;
        }
        Some(json[start..end].to_string())
    } else if bytes[i] == b't' || bytes[i] == b'f' {
        if json[i..].starts_with("true") {
            Some("true".to_string())
        } else if json[i..].starts_with("false") {
            Some("false".to_string())
        } else {
            None
        }
    } else {
        let start = i;
        let mut end = i;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        Some(json[start..end].to_string())
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

// ------------------------------------------------------------
// Agent config (authoritative System ID lives here)
// ------------------------------------------------------------
fn agent_config_path() -> String {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    format!("{}\\DeskStream\\agent_config.json", local)
}

fn gui_dir() -> String {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    format!("{}\\Screen Share", local)
}

fn gui_config_path() -> String {
    format!("{}\\gui_config.json", gui_dir())
}

fn read_agent_config() -> (String, String) {
    if let Ok(s) = std::fs::read_to_string(agent_config_path()) {
        let id = json_get(&s, "system_id").unwrap_or_default();
        let name = json_get(&s, "name").unwrap_or_default();
        (id, name)
    } else {
        (String::new(), String::new())
    }
}

fn install_dir() -> String {
    std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string()) + "\\Screen Share"
}

fn read_server_config() -> Option<ServerConfig> {
    let path = format!("{}\\server-config.json", install_dir());
    let s = std::fs::read_to_string(&path).ok()?;
    let backend = json_get(&s, "backend_url").unwrap_or_default();
    let relay = json_get(&s, "relay_url").unwrap_or_default();
    let server_addr = json_get(&s, "server_addr").unwrap_or_default();
    if backend.is_empty() || relay.is_empty() {
        return None;
    }
    Some(ServerConfig {
        backend_url: backend,
        relay_url: relay,
        server_addr: server_addr,
    })
}

struct ServerConfig {
    backend_url: String,
    relay_url: String,
    server_addr: String,
}

fn read_gui_config() -> (String, String, String, bool, String) {
    let mut backend = "http://127.0.0.1/Screen%20Share/backend/api".to_string();
    let mut relay = "127.0.0.1:9001".to_string();
    let mut env = "Production".to_string();
    let mut start = true;
    let mut dev = String::new();
    if let Some(sc) = read_server_config() {
        backend = sc.backend_url;
        relay = sc.relay_url;
    }
    if let Ok(s) = std::fs::read_to_string(gui_config_path()) {
        if let Some(v) = json_get(&s, "backend_url") {
            backend = v;
        }
        if let Some(v) = json_get(&s, "relay_url") {
            relay = v;
        }
        if let Some(v) = json_get(&s, "environment") {
            env = v;
        }
        if let Some(v) = json_get(&s, "start_with_windows") {
            start = v == "true";
        }
        if let Some(v) = json_get(&s, "device_name") {
            dev = v;
        }
    }
    (backend, relay, env, start, dev)
}

fn write_gui_config(backend: &str, relay: &str, env: &str, start: bool, dev: &str) {
    let _ = std::fs::create_dir_all(gui_dir());
    let json = format!(
        "{{\n  \"backend_url\": \"{}\",\n  \"relay_url\": \"{}\",\n  \"environment\": \"{}\",\n  \"start_with_windows\": {},\n  \"device_name\": \"{}\"\n}}\n",
        escape_json(backend),
        escape_json(relay),
        escape_json(env),
        if start { "true" } else { "false" },
        escape_json(dev)
    );
    let _ = std::fs::write(gui_config_path(), json);
}

fn set_json_string_value(json: &mut String, key: &str, val: &str) {
    let pat = format!("\"{}\":\"", key);
    if let Some(pos) = json.find(&pat) {
        let start = pos + pat.len();
        if let Some(end) = json[start..].find('"') {
            let end = start + end;
            json.replace_range(start..end, &escape_json(val));
        }
    }
}

fn update_agent_config(name: &str, relay: &str) {
    if let Ok(mut s) = std::fs::read_to_string(agent_config_path()) {
        set_json_string_value(&mut s, "name", name);
        set_json_string_value(&mut s, "relay_addr", relay);
        let _ = std::fs::write(agent_config_path(), s);
    }
}

// ------------------------------------------------------------
// Registry (Run key for "Start with Windows")
// ------------------------------------------------------------
fn reg_open_run() -> (isize, bool) {
    let mut hkey = 0isize;
    let sub = to_wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let res = unsafe {
        windows_sys::Win32::System::Registry::RegOpenKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
            &mut hkey,
        )
    };
    (hkey, res == 0)
}

fn set_autostart(name: &str, value: &str) {
    let (hkey, ok) = reg_open_run();
    if !ok {
        return;
    }
    let n = to_wide(name);
    let v = to_wide(value);
    unsafe {
        windows_sys::Win32::System::Registry::RegSetValueExW(
            hkey,
            n.as_ptr(),
            0,
            REG_SZ,
            v.as_ptr() as *const u8,
            (v.len() * 2) as u32,
        );
        windows_sys::Win32::System::Registry::RegCloseKey(hkey);
    }
}

fn del_autostart(name: &str) {
    let (hkey, ok) = reg_open_run();
    if !ok {
        return;
    }
    let n = to_wide(name);
    unsafe {
        windows_sys::Win32::System::Registry::RegDeleteValueW(hkey, n.as_ptr());
        windows_sys::Win32::System::Registry::RegCloseKey(hkey);
    }
}

fn gui_exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

// ------------------------------------------------------------
// Agent process management (launched by the GUI, NOT a relay conn)
// ------------------------------------------------------------
fn find_agent_exe() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("desktop-agent.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
            let p2 = dir.join("../desktop-agent/target/release/desktop-agent.exe");
            if p2.exists() {
                if let Ok(c) = p2.canonicalize() {
                    return Some(c.to_string_lossy().to_string());
                }
            }
        }
    }
    if let Ok(out) = std::process::Command::new("where").arg("desktop-agent.exe").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        if let Some(line) = s.lines().next() {
            return Some(line.to_string());
        }
    }
    None
}

fn find_relay_exe() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("relay-server.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
            let p2 = dir.join("../relay-server/target/release/relay-server.exe");
            if p2.exists() {
                if let Ok(c) = p2.canonicalize() {
                    return Some(c.to_string_lossy().to_string());
                }
            }
        }
    }
    if let Ok(out) = std::process::Command::new("where").arg("relay-server.exe").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        if let Some(line) = s.lines().next() {
            return Some(line.to_string());
        }
    }
    None
}

fn is_relay_running() -> bool {
    let _ = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq relay-server.exe", "/NH", "/FO", "CSV"])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains("relay-server.exe")
        });
    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq relay-server.exe", "/NH", "/FO", "CSV"])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains("relay-server.exe")
        })
        .unwrap_or(false)
}

fn launch_relay() {
    if is_relay_running() {
        return;
    }
    if let Some(path) = find_relay_exe() {
        let _ = std::process::Command::new(&path)
            .creation_flags(0x00000200 | 0x08000000)
            .spawn();
    }
}

fn launch_agent(relay: &str, backend: &str) {
    // Ensure the relay is running first
    launch_relay();
    std::thread::sleep(Duration::from_millis(500));

    if let Some(path) = find_agent_exe() {
        let mut cmd = std::process::Command::new(&path);
        cmd.arg(relay);
        if !backend.is_empty() {
            cmd.arg(backend);
        }
        let _ = cmd
            .creation_flags(0x00000200 | 0x08000000)
            .spawn();
    }
}

fn stop_agent() {
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "desktop-agent.exe"])
        .output();
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "relay-server.exe"])
        .output();
}

// ------------------------------------------------------------
// IPC: poll the agent's existing local health server
// ------------------------------------------------------------
fn poll_agent() -> Option<AgentStatusView> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let mut stream = match TcpStream::connect("127.0.0.1:49182") {
        Ok(s) => s,
        Err(_) => return None,
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(800)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_millis(800)))
        .ok();
    let _ = stream.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    while let Ok(n) = stream.read(&mut tmp) {
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let text = String::from_utf8_lossy(&buf);
    let body = match text.find("\r\n\r\n") {
        Some(i) => &text[i + 4..],
        None => &text[..],
    };
    Some(AgentStatusView {
        system_id: json_get(body, "system_id").unwrap_or_default(),
        status: json_get(body, "status").unwrap_or_default(),
        backend_connected: json_get(body, "backend_connected").map(|v| v == "true").unwrap_or(false),
        relay_connected: json_get(body, "relay_connected").map(|v| v == "true").unwrap_or(false),
        last_heartbeat_ms: json_get(body, "last_heartbeat_ms")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0),
        device_name: json_get(body, "device_name").unwrap_or_default(),
        version: json_get(body, "version").unwrap_or_default(),
    })
}

struct AgentStatusView {
    system_id: String,
    status: String,
    backend_connected: bool,
    relay_connected: bool,
    last_heartbeat_ms: u64,
    device_name: String,
    version: String,
}

// ------------------------------------------------------------
// Visuals
// ------------------------------------------------------------
fn create_icon() -> Option<HICON> {
    let size = 32i32;
    let mut bgra: Vec<u8> = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let dx = x as f32 - (size as f32) / 2.0;
            let dy = y as f32 - (size as f32) / 2.0;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 14.0 {
                // Blue (BGR = 0x8C,0x4A,0x1F)
                bgra[idx] = 0x8c;
                bgra[idx + 1] = 0x4a;
                bgra[idx + 2] = 0x1f;
                bgra[idx + 3] = 0xff;
            } else {
                bgra[idx + 3] = 0;
            }
        }
    }
    let hbmp_color =
        unsafe { CreateBitmap(size, size, 1, 32, bgra.as_ptr() as *const std::ffi::c_void) };
    if hbmp_color == 0 {
        return None;
    }
    let mask = vec![0u8; (((size + 7) / 8) * size) as usize];
    let hbmp_mask =
        unsafe { CreateBitmap(size, size, 1, 1, mask.as_ptr() as *const std::ffi::c_void) };
    if hbmp_mask == 0 {
        unsafe { DeleteObject(hbmp_color as HGDIOBJ) };
        return None;
    }
    let ii = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbmp_mask,
        hbmColor: hbmp_color,
    };
    let hicon = unsafe { CreateIconIndirect(&ii) };
    unsafe {
        DeleteObject(hbmp_color as HGDIOBJ);
        DeleteObject(hbmp_mask as HGDIOBJ);
    }
    if hicon == 0 {
        None
    } else {
        Some(hicon)
    }
}

fn enable_visual_styles() {
    let manifest = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes'?><assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><dependency><dependentAssembly><assemblyIdentity type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'/></dependentAssembly></dependency></assembly>"#;
    let tmp = std::env::temp_dir().join("screenshare_visual.manifest");
    if std::fs::write(&tmp, manifest).is_ok() {
        if let Some(p) = tmp.to_str() {
            let w = to_wide(p);
            let mut actx: ACTCTXW = unsafe { mem::zeroed() };
            actx.cbSize = mem::size_of::<ACTCTXW>() as u32;
            actx.lpSource = w.as_ptr();
            let h = unsafe { CreateActCtxW(&actx) };
            if h != 0 && h != -1 {
                let mut cookie = 0usize;
                unsafe {
                    ActivateActCtx(h, &mut cookie);
                }
            }
        }
    }
}

fn set_dwm_corners(hwnd: HWND) {
    // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2
    let value: i32 = 2;
    unsafe {
        DwmSetWindowAttribute(hwnd, 33, &value as *const i32 as *const std::ffi::c_void, 4);
    }
}

fn make_static_dpi(parent: HWND, text: &str, id: i32, style: u32, x: i32, y: i32, w: i32, h: i32, scale: f32) -> HWND {
    let cls = to_wide("STATIC");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            (x as f32 * scale) as i32,
            (y as f32 * scale) as i32,
            ((w as f32 * scale).ceil()) as i32,
            ((h as f32 * scale).ceil()) as i32,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_button_dpi(parent: HWND, text: &str, id: i32, x: i32, y: i32, w: i32, h: i32, scale: f32) -> HWND {
    let cls = to_wide("BUTTON");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON,
            (x as f32 * scale) as i32,
            (y as f32 * scale) as i32,
            ((w as f32 * scale).ceil()) as i32,
            ((h as f32 * scale).ceil()) as i32,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_group_dpi(parent: HWND, text: &str, id: i32, x: i32, y: i32, w: i32, h: i32, scale: f32) -> HWND {
    let cls = to_wide("BUTTON");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_GROUPBOX,
            (x as f32 * scale) as i32,
            (y as f32 * scale) as i32,
            ((w as f32 * scale).ceil()) as i32,
            ((h as f32 * scale).ceil()) as i32,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

fn make_static_hidden_dpi(parent: HWND, text: &str, id: i32, style: u32, x: i32, y: i32, w: i32, h: i32, scale: f32) -> HWND {
    let cls = to_wide("STATIC");
    let t = to_wide(text);
    let h = unsafe {
        CreateWindowExW(
            0,
            cls.as_ptr(),
            t.as_ptr(),
            WS_CHILD | style,
            (x as f32 * scale) as i32,
            (y as f32 * scale) as i32,
            ((w as f32 * scale).ceil()) as i32,
            ((h as f32 * scale).ceil()) as i32,
            parent,
            id as HMENU,
            0,
            ptr::null(),
        )
    };
    set_font(h);
    h
}

// ------------------------------------------------------------
// Control creation / layout
// ------------------------------------------------------------
fn create_controls(hwnd: HWND, st: &mut AppState) {
    let scale = st.dpi_scale;

    // Header
    st.main_controls.push(make_static_dpi(hwnd, "Screen Share", ID_TITLE, SS_LEFT, 20, 8, 360, 28, scale));
    st.main_controls.push(make_static_hidden_dpi(hwnd, "", ID_STATUS_DOT, SS_OWNERDRAW, 24, 32, 16, 16, scale));
    st.h_status_text = make_static_dpi(hwnd, "CONNECTING", ID_STATUS_TEXT, SS_LEFT, 46, 30, 320, 24, scale);
    st.main_controls.push(st.h_status_text);

    // Device section
    st.main_controls.push(make_group_dpi(hwnd, "This computer", ID_GRP_COMPUTER, 15, 58, 490, 140, scale));
    st.main_controls.push(make_static_dpi(hwnd, "System ID", ID_LBL_SYSID, SS_LEFT, 35, 86, 220, 22, scale));
    st.h_val_sysid = make_static_dpi(hwnd, "", ID_VAL_SYSID, SS_LEFT, 35, 110, 440, 28, scale);
    st.main_controls.push(st.h_val_sysid);
    st.main_controls.push(make_static_dpi(hwnd, "Device Name", ID_LBL_DEV, SS_LEFT, 35, 150, 220, 22, scale));
    st.h_val_dev = make_static_dpi(hwnd, "", ID_VAL_DEV, SS_LEFT, 35, 174, 440, 28, scale);
    st.main_controls.push(st.h_val_dev);

    // Connection status
    st.main_controls.push(make_group_dpi(hwnd, "Connection", ID_GRP_CONN, 15, 210, 490, 190, scale));
    st.main_controls.push(make_static_dpi(hwnd, "Agent", ID_LBL_CONN, SS_LEFT, 35, 234, 220, 22, scale));
    st.h_val_conn = make_static_dpi(hwnd, "Connected", ID_VAL_CONN, SS_LEFT, 270, 234, 200, 22, scale);
    st.main_controls.push(st.h_val_conn);
    st.main_controls.push(make_static_dpi(hwnd, "Relay", ID_LBL_RELAY, SS_LEFT, 35, 264, 220, 22, scale));
    st.h_val_relay = make_static_dpi(hwnd, "Connected", ID_VAL_RELAY, SS_LEFT, 270, 264, 200, 22, scale);
    st.main_controls.push(st.h_val_relay);
    st.main_controls.push(make_static_dpi(hwnd, "Backend", ID_LBL_BACK, SS_LEFT, 35, 294, 220, 22, scale));
    st.h_val_back = make_static_dpi(hwnd, "Connected", ID_VAL_BACK, SS_LEFT, 270, 294, 200, 22, scale);
    st.main_controls.push(st.h_val_back);
    st.main_controls.push(make_static_dpi(hwnd, "Last heartbeat", ID_LBL_HB, SS_LEFT, 35, 324, 220, 22, scale));
    st.h_val_hb = make_static_dpi(hwnd, "—", ID_VAL_HB, SS_LEFT, 270, 324, 200, 22, scale);
    st.main_controls.push(st.h_val_hb);

    // About section
    st.main_controls.push(make_group_dpi(hwnd, "About", ID_GRP_ABOUT, 15, 410, 490, 100, scale));
    st.main_controls.push(make_static_dpi(hwnd, "Version", ID_LBL_VER, SS_LEFT, 35, 430, 220, 22, scale));
    st.h_val_ver = make_static_dpi(hwnd, "", ID_VAL_VER, SS_LEFT, 270, 430, 200, 22, scale);
    st.main_controls.push(st.h_val_ver);
    st.main_controls.push(make_static_dpi(hwnd, "Environment", ID_LBL_ENV, SS_LEFT, 35, 460, 220, 22, scale));
    st.h_val_env = make_static_dpi(hwnd, "", ID_VAL_ENV, SS_LEFT, 270, 460, 200, 22, scale);
    st.main_controls.push(st.h_val_env);

    // Action buttons
    st.main_controls.push(make_button_dpi(hwnd, "Open Dashboard", ID_BTN_DASHBOARD, 35, 520, 150, 36, scale));
    st.main_controls.push(make_button_dpi(hwnd, "Settings", ID_BTN_SETTINGS, 210, 520, 110, 36, scale));
    st.main_controls.push(make_button_dpi(hwnd, "Copy System ID", ID_BTN_COPY, 345, 520, 150, 36, scale));
    st.main_controls.push(make_button_dpi(hwnd, "Restart Agent", ID_BTN_RESTART, 35, 566, 150, 36, scale));
    st.h_toast = make_static_dpi(hwnd, "", ID_TOAST, SS_LEFT, 35, 608, 440, 24, scale);
    st.main_controls.push(st.h_toast);

    // Settings view (hidden initially)
    st.settings_controls.push(make_group_dpi(hwnd, "Server Configuration", ID_GRP_SRV, 15, 58, 490, 210, scale));
    st.settings_controls.push(make_static_hidden_dpi(hwnd, "Backend URL", ID_LBL_BE, SS_LEFT, 35, 86, 220, 22, scale));
    st.ed_backend = make_edit_hidden_dpi(hwnd, ID_ED_BE, ES_LEFT | ES_AUTOHSCROLL, 35, 110, 440, 26, scale);
    st.settings_controls.push(st.ed_backend);
    st.settings_controls.push(make_static_hidden_dpi(hwnd, "Relay URL", ID_LBL_RL, SS_LEFT, 35, 150, 220, 22, scale));
    st.ed_relay = make_edit_hidden_dpi(hwnd, ID_ED_RL, ES_LEFT | ES_AUTOHSCROLL, 35, 174, 440, 26, scale);
    st.settings_controls.push(st.ed_relay);
    st.settings_controls.push(make_static_hidden_dpi(hwnd, "Environment", ID_LBL_EV, SS_LEFT, 35, 214, 220, 22, scale));
    st.cb_env = make_combo_hidden_dpi(hwnd, ID_CB_EV, 35, 238, 440, 200, scale);
    st.settings_controls.push(st.cb_env);
    combo_add(st.cb_env, "Production");
    combo_add(st.cb_env, "Development");

    st.settings_controls.push(make_group_dpi(hwnd, "Startup", ID_GRP_START, 15, 284, 490, 54, scale));
    st.chk_startup = make_check_dpi(hwnd, "Start Screen Share with Windows", ID_CHK_START, 35, 310, 430, 22, scale);
    st.settings_controls.push(st.chk_startup);

    st.settings_controls.push(make_group_dpi(hwnd, "Device", ID_GRP_DEV, 15, 348, 490, 110, scale));
    st.settings_controls.push(make_static_hidden_dpi(hwnd, "Device Name", ID_LBL_DN, SS_LEFT, 35, 372, 220, 22, scale));
    st.ed_devname = make_edit_hidden_dpi(hwnd, ID_ED_DN, ES_LEFT | ES_AUTOHSCROLL, 35, 396, 440, 26, scale);
    st.settings_controls.push(st.ed_devname);
    st.settings_controls.push(make_static_hidden_dpi(hwnd, "System ID (read-only)", ID_LBL_SID, SS_LEFT, 35, 432, 220, 22, scale));
    st.st_sysid = make_edit_hidden_dpi(hwnd, ID_ST_SID, ES_LEFT | ES_READONLY, 35, 456, 440, 26, scale);
    st.settings_controls.push(st.st_sysid);

    st.settings_controls.push(make_group_dpi(hwnd, "Logging", ID_GRP_LOG, 15, 468, 490, 60, scale));
    st.settings_controls.push(make_button_dpi(hwnd, "Open Logs Folder", ID_BTN_LOGS, 35, 492, 180, 30, scale));
    st.settings_controls.push(make_button_dpi(hwnd, "Open Configuration Folder", ID_BTN_CFG, 285, 492, 180, 30, scale));

    st.settings_controls.push(make_button_dpi(hwnd, "Save", ID_BTN_SAVE, 35, 540, 200, 36, scale));
    st.settings_controls.push(make_button_dpi(hwnd, "Cancel", ID_BTN_CANCEL, 285, 540, 200, 36, scale));
}

fn show_view(st: &mut AppState, mode: i32) {
    for &h in &st.main_controls {
        unsafe { ShowWindow(h, if mode == 0 { SW_SHOW } else { SW_HIDE }) };
    }
    for &h in &st.settings_controls {
        unsafe { ShowWindow(h, if mode == 1 { SW_SHOW } else { SW_HIDE }) };
    }
    st.mode = mode;
    set_text(
        st.hwnd,
        if mode == 1 { "Screen Share — Settings" } else { "Screen Share" },
    );
}

fn init_state(st: &mut AppState) {
    let (aid, aname) = read_agent_config();
    if !aid.is_empty() {
        st.system_id = aid;
    }
    if !aname.is_empty() {
        st.device_name = aname;
    }
    let (backend, relay, env, start, dev) = read_gui_config();
    st.backend_url = backend;
    st.relay_url = relay;
    st.environment = env;
    st.start_with_windows = start;
    if !dev.is_empty() {
        st.device_name = dev;
    }
    // Remove the agent's own (legacy) auto-start key so the GUI owns startup.
    del_autostart("ScreenShareAgent");

    set_text(st.ed_backend, &st.backend_url);
    set_text(st.ed_relay, &st.relay_url);
    combo_select(st.cb_env, if st.environment == "Development" { 1 } else { 0 });
    if st.start_with_windows {
        let _ = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                st.chk_startup,
                0x00F1, // BM_SETCHECK
                1,
                0,
            )
        };
    }
    set_text(st.ed_devname, &st.device_name);
    set_text(st.st_sysid, &st.system_id);

    show_view(st, 0);
    refresh_display(st);
}

fn refresh_display(st: &mut AppState) {
    let status_upper = match st.status_str.as_str() {
        "online" => "ONLINE",
        "connecting" => "CONNECTING",
        "error" => "ERROR",
        _ => "OFFLINE",
    };
    set_text(st.h_status_text, status_upper);
    set_text(st.h_val_sysid, &format_sysid(&st.system_id));
    set_text(st.h_val_dev, &st.device_name);
    set_text(
        st.h_val_conn,
        if st.relay_connected {
            "Connected"
        } else {
            "Disconnected"
        },
    );
    set_text(
        st.h_val_relay,
        if st.relay_connected {
            "Connected"
        } else {
            "Disconnected"
        },
    );
    set_text(
        st.h_val_back,
        if st.backend_connected {
            "Connected"
        } else {
            "Disconnected"
        },
    );
    set_text(st.h_val_hb, &format_heartbeat(st));
    set_text(st.h_val_ver, &format!("Screen Share Agent {}", st.version));
    set_text(st.h_val_env, &st.environment);
}

fn format_sysid(id: &str) -> String {
    let clean: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    if clean.len() == 9 {
        format!("{} {} {}", &clean[0..3], &clean[3..6], &clean[6..9])
    } else {
        clean
    }
}

fn format_heartbeat(st: &AppState) -> String {
    if !st.relay_connected || st.last_heartbeat_ms == 0 {
        return "—".to_string();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let diff = now.saturating_sub(st.last_heartbeat_ms);
    if diff < 2000 {
        "Just now".to_string()
    } else {
        format!("{}s ago", diff / 1000)
    }
}

fn poll_and_update(st: &mut AppState) {
    if let Some(v) = poll_agent() {
        if !v.system_id.is_empty() {
            st.system_id = v.system_id.clone();
        }
        if !v.device_name.is_empty() {
            st.device_name = v.device_name.clone();
        }
        if !v.version.is_empty() {
            st.version = v.version.clone();
        }
        st.status_str = v.status;
        st.backend_connected = v.backend_connected;
        st.relay_connected = v.relay_connected;
        st.last_heartbeat_ms = v.last_heartbeat_ms;
    } else {
        st.status_str = "offline".to_string();
        st.backend_connected = false;
        st.relay_connected = false;
    }
    refresh_display(st);
}

fn ensure_agent(st: &mut AppState) {
    if poll_agent().is_none() {
        if Instant::now().duration_since(st.last_launch) > Duration::from_secs(2) {
            launch_agent(&st.relay_url, &st.backend_url);
            st.last_launch = Instant::now();
        }
    }
}

fn copy_system_id(st: &AppState) {
    let id = if st.system_id.is_empty() {
        read_agent_config().0
    } else {
        st.system_id.clone()
    };
    // Use Windows clipboard API directly to avoid arboard DLL dependency issues
    let id_wide = to_wide(&id);
    unsafe {
        if OpenClipboard(0) != 0 {
            EmptyClipboard();
            let len = id_wide.len();
            let handle = GlobalAlloc(
                0x0042, // GMEM_MOVEABLE
                (len * 2) as usize
            );
            if !handle.is_null() {
                let ptr = GlobalLock(handle);
                if !ptr.is_null() {
                    ptr::copy_nonoverlapping(id_wide.as_ptr(), ptr as *mut u16, len);
                    GlobalUnlock(handle);
                    SetClipboardData(13, handle as isize); // CF_UNICODETEXT
                }
            }
            CloseClipboard();
        }
    }
    set_text(st.h_toast, "System ID copied");
    unsafe { SetTimer(st.hwnd, 2, 2000, None) };
}

fn open_dashboard() {
    let backend = {
        let (b, _, _, _, _) = read_gui_config();
        b
    };
    let url = if backend.is_empty() {
        "http://127.0.0.1/Screen%20Share/frontend/dashboard.php".to_string()
    } else {
        format!("{}/dashboard.php", backend.trim_end_matches('/'))
    };
    let url_w = to_wide(&url);
    let op = to_wide("open");
    unsafe {
        ShellExecuteW(0, op.as_ptr(), url_w.as_ptr(), ptr::null(), ptr::null(), 1);
    }
}

fn open_folder(path: &str) {
    let op = to_wide("open");
    let p = to_wide(path);
    unsafe {
        ShellExecuteW(0, op.as_ptr(), p.as_ptr(), ptr::null(), ptr::null(), 1);
    }
}

fn do_exit(st: &mut AppState) {
    stop_agent();
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = st.hwnd;
    nid.uID = 1;
    unsafe { windows_sys::Win32::UI::Shell::Shell_NotifyIconW(NIM_DELETE, &nid) };
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0) };
}

fn create_tray(hwnd: HWND, icon: HICON) -> bool {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_USER_TRAY;
    nid.hIcon = icon;
    let tip = "Screen Share";
    let tw: Vec<u16> = tip.encode_utf16().collect();
    for (i, c) in tw.iter().enumerate() {
        nid.szTip[i] = *c;
    }
    unsafe { windows_sys::Win32::UI::Shell::Shell_NotifyIconW(NIM_ADD, &nid) != 0 }
}

fn show_tray_menu(hwnd: HWND) {
    let menu = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::CreatePopupMenu() };
    if menu == 0 {
        return;
    }
    let st = unsafe { get_state(hwnd) };
    let agent_offline = st.status_str == "offline";
    let items: Vec<(i32, String)> = vec![
        (ID_TRAY_OPEN, "Open Screen Share".to_string()),
        (ID_TRAY_DASHBOARD, "Open Dashboard".to_string()),
        (ID_TRAY_COPY, "Copy System ID".to_string()),
        (ID_TRAY_SETTINGS, "Settings".to_string()),
        if agent_offline { (ID_TRAY_START, "Start Agent".to_string()) } else { (ID_TRAY_STOP, "Stop Agent".to_string()) },
        (ID_TRAY_RESTART, "Restart Agent".to_string()),
        (ID_TRAY_EXIT, "Exit".to_string()),
    ];
    for (id, label) in items.iter() {
        let w = to_wide(label);
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::AppendMenuW(
                menu,
                MF_STRING,
                *id as usize,
                w.as_ptr(),
            );
        }
    }
    let mut pt: windows_sys::Win32::Foundation::POINT = unsafe { mem::zeroed() };
    unsafe {
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, ptr::null());
        windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd, 0, 0, 0);
        windows_sys::Win32::UI::WindowsAndMessaging::DestroyMenu(menu);
    }
}

const DESKSTREAM_CLASS: &str = "ScreenShareCls";
const SINGLE_INSTANCE_MUTEX: &str = "DeskStreamGUI_SingleInstance";

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(lpMutexAttributes: *const std::ffi::c_void, bInitialOwner: i32, lpName: *const u16) -> isize;
}

#[repr(C)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
struct COPYDATASTRUCT {
    dwData: usize,
    cbData: u32,
    lpData: *mut std::ffi::c_void,
}

fn acquire_single_instance() -> bool {
    let name = to_wide(SINGLE_INSTANCE_MUTEX);
    let mutex = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
    if mutex == 0 {
        return true;
    }
    let err = unsafe { GetLastError() };
    if err == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(mutex) };
        false
    } else {
        let _ = mutex; // Keep handle alive for process lifetime (auto-released on exit)
        true
    }
}

fn wake_existing(hwnd_to_find: &str) -> bool {
    let class_name = to_wide(hwnd_to_find);
    let hwnd = unsafe { FindWindowW(class_name.as_ptr(), ptr::null()) };
    if hwnd == 0 {
        return false;
    }
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
    true
}

fn forward_to_existing(uri: &str) -> bool {
    let class_name = to_wide(DESKSTREAM_CLASS);
    let hwnd = unsafe { FindWindowW(class_name.as_ptr(), ptr::null()) };
    if hwnd == 0 {
        return false;
    }
    let data = to_wide(uri);
    let cds = COPYDATASTRUCT {
        dwData: 1,
        cbData: ((data.len() - 1) * 2) as u32,
        lpData: data.as_ptr() as *mut _,
    };
    unsafe {
        SendMessageW(hwnd, WM_COPYDATA, 0, &cds as *const COPYDATASTRUCT as LPARAM);
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
    true
}

fn show_window(st: &AppState) {
    unsafe {
        ShowWindow(st.hwnd, SW_SHOW);
        SetForegroundWindow(st.hwnd);
    }
}

unsafe fn get_state(hwnd: HWND) -> &'static mut AppState {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
    &mut *p
}

// ------------------------------------------------------------
// Window procedure
// ------------------------------------------------------------
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match msg {
        WM_CREATE => {
            let cs = lparam as *const CREATESTRUCTW;
            let st = (*cs).lpCreateParams as *mut AppState;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, st as isize);
            let st = &mut *st;
            st.hwnd = hwnd;
            st.dpi_scale = get_dpi_scale(hwnd);
            set_dwm_corners(hwnd);
            create_controls(hwnd, st);
            create_tray(hwnd, st.icon);
            init_state(st);
            SetTimer(hwnd, 1, 1500, None);
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as i32;
            let st = get_state(hwnd);
            match id {
                ID_BTN_SETTINGS => {
                    set_text(st.ed_backend, &st.backend_url);
                    set_text(st.ed_relay, &st.relay_url);
                    combo_select(st.cb_env, if st.environment == "Development" { 1 } else { 0 });
                    if st.start_with_windows {
                        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                            st.chk_startup, 0x00F1, 1, 0,
                        );
                    } else {
                        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                            st.chk_startup, 0x00F1, 0, 0,
                        );
                    }
                    set_text(st.ed_devname, &st.device_name);
                    set_text(st.st_sysid, &st.system_id);
                    show_view(st, 1);
                }
                ID_BTN_DASHBOARD => {
                    open_dashboard();
                }
                ID_BTN_COPY => copy_system_id(st),
                ID_BTN_RESTART => {
                    stop_agent();
                    st.last_launch = Instant::now() - Duration::from_secs(10);
                }
                ID_BTN_SAVE => {
                    let backend = get_text(st.ed_backend);
                    let relay = get_text(st.ed_relay);
                    let env = combo_selected(st.cb_env);
                    let start = is_checked(st.chk_startup);
                    let dev = get_text(st.ed_devname);
                    st.backend_url = backend.clone();
                    st.relay_url = relay.clone();
                    st.environment = env.clone();
                    st.start_with_windows = start;
                    st.device_name = dev.clone();
                    write_gui_config(&backend, &relay, &env, start, &dev);
                    update_agent_config(&dev, &relay);
                    if start {
                        let exe = gui_exe_path();
                        set_autostart("ScreenShare", &format!("\"{}\" --hidden", exe));
                    } else {
                        del_autostart("ScreenShare");
                    }
                    del_autostart("ScreenShareAgent");
                    stop_agent();
                    st.last_launch = Instant::now();
                    show_view(st, 0);
                    refresh_display(st);
                }
                ID_BTN_CANCEL => {
                    show_view(st, 0);
                }
                ID_BTN_LOGS => {
                    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
                    open_folder(&format!("{}\\DeskStream", local));
                }
                ID_BTN_CFG => {
                    open_folder(&gui_dir());
                }
                ID_TRAY_OPEN => show_window(st),
                ID_TRAY_DASHBOARD => {
                    open_dashboard();
                }
                ID_TRAY_COPY => copy_system_id(st),
                ID_TRAY_SETTINGS => {
                    set_text(st.ed_backend, &st.backend_url);
                    set_text(st.ed_relay, &st.relay_url);
                    combo_select(st.cb_env, if st.environment == "Development" { 1 } else { 0 });
                    set_text(st.ed_devname, &st.device_name);
                    set_text(st.st_sysid, &st.system_id);
                    show_window(st);
                    show_view(st, 1);
                }
                ID_TRAY_RESTART => {
                    stop_agent();
                    st.last_launch = Instant::now() - Duration::from_secs(10);
                }
                ID_TRAY_START => {
                    st.last_launch = Instant::now() - Duration::from_secs(10);
                }
                ID_TRAY_STOP => {
                    stop_agent();
                }
                ID_TRAY_EXIT => do_exit(st),
                _ => {}
            }
            0
        }
        WM_TIMER => {
            let st = get_state(hwnd);
            if wparam == 1 {
                poll_and_update(st);
                ensure_agent(st);
            } else if wparam == 2 {
                set_text(st.h_toast, "");
            }
            0
        }
        WM_COPYDATA => {
            let cds = lparam as *const COPYDATASTRUCT;
            if !cds.is_null() {
                let cd = unsafe { &*cds };
                if cd.cbData > 0 && !cd.lpData.is_null() {
                    let bytes = unsafe {
                        std::slice::from_raw_parts(cd.lpData as *const u8, cd.cbData as usize)
                    };
                    let u16s: Vec<u16> = bytes
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    if let Ok(_uri) = String::from_utf16(&u16s) {
                        let st = get_state(hwnd);
                        ShowWindow(hwnd, SW_RESTORE);
                        SetForegroundWindow(hwnd);
                        // Force agent launch check
                        st.last_launch = Instant::now() - Duration::from_secs(10);
                        set_text(st.h_toast, "DeskStream protocol: agent starting...");
                        unsafe { SetTimer(hwnd, 2, 2000, None) };
                    }
                }
            }
            0
        }
        WM_DRAWITEM => {
            let di = lparam as *const DRAWITEMSTRUCT;
            let ctl = (*di).CtlID;
            if ctl as i32 == ID_STATUS_DOT {
                let hdc = (*di).hDC;
                let rc = (*di).rcItem;
                let color: u32 = match get_state(hwnd).status_str.as_str() {
                    "online" => 0x44a82eu32,
                    "connecting" => 0x00a0e0u32,
                    "error" => 0x0000c0u32,
                    _ => 0x808080u32,
                };
                let brush = CreateSolidBrush(color);
                let old = windows_sys::Win32::Graphics::Gdi::SelectObject(hdc, brush as HGDIOBJ);
                Ellipse(hdc, rc.left, rc.top, rc.right, rc.bottom);
                windows_sys::Win32::Graphics::Gdi::SelectObject(hdc, old);
                DeleteObject(brush as HGDIOBJ);
            }
            0
        }
        WM_USER_TRAY => {
            let m = (lparam as u32) & 0xffff;
            let st = get_state(hwnd);
            if m == WM_LBUTTONDBLCLK as u32 {
                show_window(st);
            } else if m == WM_RBUTTONUP as u32 || m == 0x007B {
                show_tray_menu(hwnd);
            }
            0
        }
        WM_CLOSE => {
            // Minimize to tray instead of exiting; agent keeps running.
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_DESTROY => {
            let st = get_state(hwnd);
            do_exit(st);
            0
        }
        WM_GETMINMAXINFO => {
            let info = lparam as *mut MINMAXINFO;
            let st = get_state(hwnd);
            let min_w = (380.0 * st.dpi_scale).ceil() as i32;
            let min_h = (450.0 * st.dpi_scale).ceil() as i32;
            (*info).ptMinTrackSize.x = min_w;
            (*info).ptMinTrackSize.y = min_h;
            0
        }
        WM_SIZE => {
            // On resize, controls stay at their original positions (fixed layout).
            // The window has a large enough default size that no clipping occurs at 100%.
            // At higher DPI, the DPI-scaled coordinates handle the layout.
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = mem::zeroed();
            let _hdc = windows_sys::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
            windows_sys::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    // enable_visual_styles(); // Disabled to prevent potential crash
    // unsafe {
    //     windows_sys::Win32::UI::WindowsAndMessaging::SetProcessDPIAware();
    // }

    let args: Vec<String> = std::env::args().collect();
    let launch_uri: Option<String> = args
        .iter()
        .skip(1)
        .find(|a| a.to_lowercase().starts_with("deskstream://"))
        .cloned();
    let start_hidden = args
        .iter()
        .any(|a| a == "--hidden" || a == "/hidden");

    // Single-instance guard: only one GUI process allowed.
    if !acquire_single_instance() {
        if let Some(uri) = &launch_uri {
            forward_to_existing(uri);
        } else {
            wake_existing(DESKSTREAM_CLASS);
        }
        return;
    }

    let icon = match create_icon() {
        Some(i) => i,
        None => {
            eprintln!("Failed to create icon, using default");
            0 as HICON
        }
    };

    let mut state = Box::new(AppState::new());
    state.icon = icon;
    // Safe read of agent config with fallback
    let (system_id, _name) = read_agent_config();
    state.system_id = system_id;
    let state_ptr = Box::into_raw(state) as *mut std::ffi::c_void;

    let class_name = to_wide(DESKSTREAM_CLASS);
    let hinst: HINSTANCE = unsafe { GetModuleHandleW(ptr::null()) };
    if hinst == 0 {
        eprintln!("Failed to get module handle");
        return;
    }

    let mut wc: WNDCLASSW = unsafe { mem::zeroed() };
    wc.style = 0x0002 | 0x0001; // CS_HREDRAW | CS_VREDRAW
    wc.lpfnWndProc = Some(window_proc);
    wc.hInstance = hinst;
    wc.hCursor = unsafe { LoadCursorW(0, IDC_ARROW) };
    wc.hbrBackground = (COLOR_WINDOW + 1) as HBRUSH;
    wc.hIcon = icon;
    wc.lpszClassName = class_name.as_ptr();
    unsafe { RegisterClassW(&wc) };

    let title = to_wide("Screen Share");
    // Disable GetSystemMetrics to prevent potential crash
    let sw = 1920i32;
    let sh = 1080i32;
    let base_w = 540i32;
    let base_h = 680i32;
    // Disable DPI calculation to prevent potential crash
    let init_scale = 1.0f32;
    let w = base_w;
    let h = base_h;
    let x = (sw - w) / 2;
    let y = (sh - h) / 2;

    let window_style = if start_hidden {
        WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX
    } else {
        WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_VISIBLE
    };

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            window_style,
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

    if hwnd == 0 {
        eprintln!("Failed to create window");
        return;
    }

    // If launched via deskstream://open as first instance, ensure agent starts
    unsafe {
        ShowWindow(hwnd, if start_hidden { SW_HIDE } else { SW_SHOW });
        SetForegroundWindow(hwnd);
    }

    let mut msg: MSG = unsafe { mem::zeroed() };
    unsafe {
        while windows_sys::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, 0, 0, 0) > 0 {
            windows_sys::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
            windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
        }
    }
}
