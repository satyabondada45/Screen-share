#![windows_subsystem = "windows"]

// Hardware H264 Video Pipeline (120 FPS Ultra-Low Latency)
pub mod registration {
    pub mod backend_client;
}
pub mod encoder;
pub mod identity;
pub mod network;
pub mod relay;
pub mod status;
pub mod tray;
pub mod ui;
pub mod webview_app;
pub mod local_server;
pub mod updater;
// TEMPORARY SESSION DIAGNOSTICS: remove with session_debug.rs after the A/B trace.
pub mod session_debug;


use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use enigo::Key;
use encoder::HardwareH264Encoder;
use screenshots::Screen;
use sha2::{Digest, Sha256};
use rand::Rng;

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ============================================================
// VIDEO SETTINGS (120 FPS Ultra-Low Latency)
// ============================================================

const TARGET_FPS: u32 = 60;
#[allow(dead_code)]
const TARGET_BITRATE: u32 = 8_000_000;
const FRAME_INTERVAL_MICROS: u64 = 16667; // 60 FPS ~ 16.667 ms per frame
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const MAX_CLIPBOARD_PACKET_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECOVERABLE_PACKET_DISCARD_BYTES: usize = 32 * 1024 * 1024;

// FIXED: Type-2/Type-3 PIN auth uses a raw 32-byte SHA-256 digest of the UTF-8 PIN.
// Never generate an all-zero digest as a stand-in for a missing PIN.
#[allow(dead_code)]
const AUTH_PIN_MISSING: &str = "AUTH_PIN_MISSING";
#[allow(dead_code)]
const AUTH_PIN_INVALID: &str = "AUTH_PIN_INVALID";
const AUTH_SESSION_REJECTED: &str = "AUTH_SESSION_REJECTED";
#[allow(dead_code)]
const AUTH_PROTOCOL_MISMATCH: &str = "AUTH_PROTOCOL_MISMATCH";
const AUTH_RELAY_DISCONNECTED: &str = "AUTH_RELAY_DISCONNECTED";

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewerAuthResult {
    Accepted,
    Rejected(&'static str),
}

fn is_all_zero_auth_hash(hash: &[u8; 32]) -> bool {
    hash.iter().copied().fold(0u8, |acc, byte| acc | byte) == 0
}

#[allow(dead_code)]
fn hashes_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[allow(dead_code)]
fn viewer_pin_sha256(pin: &str) -> [u8; 32] {
    Sha256::digest(pin.trim().as_bytes()).into()
}

#[allow(dead_code)]
fn authenticate_viewer_pin(pin: &str, supplied_hash: &[u8; 32]) -> ViewerAuthResult {
    let pin = pin.trim();

    if pin.is_empty() {
        return ViewerAuthResult::Rejected(AUTH_PIN_MISSING);
    }

    // Configured PIN: require exact digest. Never accept all-zero as a bypass.
    if is_all_zero_auth_hash(supplied_hash) {
        return ViewerAuthResult::Rejected(AUTH_PROTOCOL_MISMATCH);
    }
    let expected = viewer_pin_sha256(pin);
    if hashes_equal(&expected, supplied_hash) {
        ViewerAuthResult::Accepted
    } else {
        ViewerAuthResult::Rejected(AUTH_PIN_INVALID)
    }
}

#[allow(dead_code)]
fn verify_viewer_pin(pin: &str, supplied_hash: &[u8; 32]) -> bool {
    matches!(
        authenticate_viewer_pin(pin, supplied_hash),
        ViewerAuthResult::Accepted
    )
}

// ============================================================
// DPI
// ============================================================

#[cfg(windows)]
fn set_process_dpi_aware() {
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

#[cfg(not(windows))]
fn set_process_dpi_aware() {}

// ============================================================
// NATIVE MOUSE
// ============================================================

#[cfg(windows)]
fn set_native_cursor_pos(x: i32, y: i32) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
}

#[cfg(not(windows))]
fn set_native_cursor_pos(_x: i32, _y: i32) {}

// FIXED: native input helpers are Windows-only; non-Windows stubs keep call sites compiling.
#[cfg(windows)]
fn send_native_mouse_click(flags: u32) {
    unsafe {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::mouse_event(flags, 0, 0, 0, 0);
    }
}

#[cfg(not(windows))]
fn send_native_mouse_click(_flags: u32) {}

#[cfg(windows)]
fn send_native_mouse_wheel(scroll_y: i16) {
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_WHEEL;
        windows_sys::Win32::UI::Input::KeyboardAndMouse::mouse_event(MOUSEEVENTF_WHEEL, 0, 0, scroll_y as i32, 0);
    }
}

#[cfg(not(windows))]
fn send_native_mouse_wheel(_scroll_y: i16) {}

#[cfg(windows)]
fn send_native_key(vk: u32, is_up: bool) {
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
        };
        let mut flags = if is_up { KEYEVENTF_KEYUP } else { 0 };
        // FIXED: extended keys include arrows, nav cluster, Win, NumLock, ScrollLock
        if matches!(vk, 33..=46 | 91..=93 | 144 | 145) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        keybd_event(vk as u8, 0, flags, 0);
    }
}

#[cfg(not(windows))]
fn send_native_key(_vk: u32, _is_up: bool) {}

#[cfg(windows)]
fn attach_thread_to_input_desktop() {
    unsafe {
        let hdesktop = windows_sys::Win32::System::StationsAndDesktops::OpenInputDesktop(
            0,
            0,
            0x10000000u32, // GENERIC_ALL
        );
        if hdesktop != 0 {
            windows_sys::Win32::System::StationsAndDesktops::SetThreadDesktop(hdesktop);
            windows_sys::Win32::System::StationsAndDesktops::CloseDesktop(hdesktop);
        }
    }
}

#[cfg(not(windows))]
fn attach_thread_to_input_desktop() {}


// ============================================================
// HIDE CONSOLE & LOGGING
// ============================================================

#[cfg(windows)]
#[allow(dead_code)]
fn hide_console_window() {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        let window = GetConsoleWindow();
        if window != 0 {
            ShowWindow(window, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn hide_console_window() {}

macro_rules! agent_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            use std::io::Write;
            let _ = writeln!(std::io::stdout(), "{}", msg);
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("C:\\Users\\Public\\deskstream_agent.log") {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let _ = writeln!(file, "[{}] {}", now, msg);
            }
        }
    };
}

macro_rules! println {
    ($($arg:tt)*) => {
        agent_log!($($arg)*)
    };
}

macro_rules! eprintln {
    ($($arg:tt)*) => {
        agent_log!($($arg)*)
    };
}

// ============================================================
// CONNECTION DIALOG
// ============================================================



// ============================================================
// SHA256
// ============================================================

#[allow(dead_code)]
fn compute_sha256(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

// ============================================================
// TIME
// ============================================================

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn next_heartbeat_id(previous: &mut u64) -> u64 {
    *previous = current_time_millis().max(previous.saturating_add(1));
    *previous
}

fn log_health_transition(identity: &str, channel: &str, transition: Option<(String, String)>) {
    if let Some((old_state, new_state)) = transition {
        crate::session_debug::log(
            identity,
            &format!(
                "[HEALTH TRACE] timestamp_ms={} channel={} stage=STATE_CHANGED old={} new={}",
                crate::status::now_ms(),
                channel,
                old_state,
                new_state
            ),
        );
    }
}

fn reconnect_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(3);
    let base_ms = 1_000u64 << exponent;
    let jitter_ms = rand::thread_rng().gen_range(0..=base_ms / 4);
    Duration::from_millis(base_ms + jitter_ms)
}

fn next_reconnect_attempt(attempt: &mut u32) -> u32 {
    *attempt = attempt.saturating_add(1);
    *attempt
}

fn record_reconnect_failure(identity: &str, attempt: u32, stage: &str, reason: &str) {
    crate::status::set_relay_state(false);
    let disconnect_reason = if stage == "registration_rejected" {
        "authorization_rejected"
    } else {
        "unexpected_socket_loss"
    };
    if crate::status::set_disconnect_reason_if_unknown(disconnect_reason) {
        crate::session_debug::log(
            identity,
            &format!(
                "[DISCONNECT TRACE] timestamp_ms={} stage=FIRST_FAILURE source={} reason={}",
                crate::status::now_ms(), stage, disconnect_reason
            ),
        );
    }
    crate::session_debug::log(
        identity,
        &format!(
            "[RECONNECT TRACE] timestamp_ms={} stage=ATTEMPT_FAILED attempt={} retry_limit=unbounded source={} reason={}",
            crate::status::now_ms(), attempt, stage, reason
        ),
    );
    crate::status::set_reconnect_state("Reconnecting", attempt, 0);
    let delay = reconnect_delay(attempt);
    crate::session_debug::log(
        identity,
        &format!(
            "[RECONNECT TRACE] timestamp_ms={} stage=RETRY_SCHEDULED attempt={} retry_limit=unbounded delay_ms={}",
            crate::status::now_ms(), attempt, delay.as_millis()
        ),
    );
    thread::sleep(delay);
}

fn stage_log(identity: &str, stage_event: &str, detail: &str) {
    let timestamp_ms = current_time_millis();
    println!(
        "[STAGE] timestamp_ms={} device_id={} event={} {}",
        timestamp_ms, identity, stage_event, detail
    );
    crate::session_debug::log(
        identity,
        &format!(
            "event={} timestamp_ms={} {}",
            stage_event, timestamp_ms, detail
        ),
    );
}

fn native_auth_trace(identity: &str, event: &str, detail: &str) {
    let timestamp_ms = current_time_millis();
    println!(
        "[NATIVE AUTH TRACE] timestamp_ms={} device_id={} event={} detail={}",
        timestamp_ms, identity, event, detail
    );
    crate::session_debug::log(
        identity,
        &format!(
            "event={} timestamp_ms={} detail={}",
            event, timestamp_ms, detail
        ),
    );
}

struct SessionReadStream {
    inner: TcpStream,
    identity: String,
    is_connected: Arc<AtomicBool>,
    is_in_session: Arc<AtomicBool>,
    intentional_reconnect: bool,
}

struct LifecycleTcpStream {
    inner: TcpStream,
    identity: String,
    function: &'static str,
    socket: &'static str,
    event: &'static str,
    is_connected: Option<Arc<AtomicBool>>,
    is_in_session: Option<Arc<AtomicBool>>,
    intentional_reconnect: bool,
}

impl LifecycleTcpStream {
    fn new(
        inner: TcpStream,
        identity: String,
        function: &'static str,
        socket: &'static str,
        event: &'static str,
        is_connected: Option<Arc<AtomicBool>>,
        is_in_session: Option<Arc<AtomicBool>>,
        intentional_reconnect: bool,
    ) -> Self {
        Self {
            inner,
            identity,
            function,
            socket,
            event,
            is_connected,
            is_in_session,
            intentional_reconnect,
        }
    }
}

impl std::ops::Deref for LifecycleTcpStream {
    type Target = TcpStream;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for LifecycleTcpStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Drop for LifecycleTcpStream {
    fn drop(&mut self) {
        crate::session_debug::lifecycle(
            &self.identity,
            self.event,
            self.function,
            self.socket,
            self.is_connected.as_deref(),
            self.is_in_session.as_deref(),
            self.intentional_reconnect,
            "TcpStream handle dropped; no explicit shutdown requested here",
        );
    }
}

impl Read for SessionReadStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buffer)
    }

    fn read_exact(&mut self, buffer: &mut [u8]) -> std::io::Result<()> {
        let result = self.inner.read_exact(buffer);
        if let Err(error) = &result {
            let detail = format!("error={:?}", error);
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                crate::session_debug::lifecycle(
                    &self.identity,
                    "SOCKET_EOF",
                    "session_input_reader",
                    "cloned_session_stream",
                    Some(self.is_connected.as_ref()),
                    Some(self.is_in_session.as_ref()),
                    self.intentional_reconnect,
                    &detail,
                );
            }
            crate::session_debug::lifecycle(
                &self.identity,
                "SOCKET_READ_ERROR",
                "session_input_reader",
                "cloned_session_stream",
                Some(self.is_connected.as_ref()),
                Some(self.is_in_session.as_ref()),
                self.intentional_reconnect,
                &detail,
            );
        }
        result
    }
}

impl Drop for SessionReadStream {
    fn drop(&mut self) {
        crate::session_debug::lifecycle(
            &self.identity,
            "SESSION_STREAM_DROP",
            "session_input_reader",
            "cloned_session_reader_stream",
            Some(self.is_connected.as_ref()),
            Some(self.is_in_session.as_ref()),
            self.intentional_reconnect,
            "reader TcpStream clone dropped",
        );
    }
}

fn mark_session_disconnected(
    identity: &str,
    function: &str,
    socket: &str,
    is_connected: &AtomicBool,
    is_in_session: &AtomicBool,
    intentional_reconnect: bool,
    detail: &str,
) {
    let lower_detail = detail.to_ascii_lowercase();
    let (disconnect_reason, transport_lost) = if detail.contains("Type 99 from relay") {
        ("remote_disconnect_ambiguous", false)
    } else if ["read", "write", "socket", "eof"]
        .iter()
        .any(|needle| lower_detail.contains(needle))
    {
        ("unexpected_socket_loss", true)
    } else {
        ("unknown_termination", false)
    };
    if crate::status::set_disconnect_reason_if_unknown(disconnect_reason) {
        crate::session_debug::log(
            identity,
            &format!(
                "[DISCONNECT TRACE] timestamp_ms={} stage=FIRST_FAILURE source={} reason={} detail={}",
                crate::status::now_ms(), function, disconnect_reason, detail
            ),
        );
    }
    if transport_lost {
        log_health_transition(
            identity,
            "SESSION_END_TO_END",
            crate::status::mark_heartbeat_lost(true, "native_socket_failure"),
        );
    }
    crate::session_debug::lifecycle(
        identity,
        "SESSION_END_REQUESTED",
        function,
        socket,
        Some(is_connected),
        Some(is_in_session),
        intentional_reconnect,
        detail,
    );
    if is_connected.swap(false, Ordering::SeqCst) {
        crate::session_debug::lifecycle(
            identity,
            "IS_CONNECTED_FALSE",
            function,
            socket,
            Some(is_connected),
            Some(is_in_session),
            intentional_reconnect,
            detail,
        );
    }
}

fn mark_session_packet_read_failed(
    identity: &str,
    packet_type: u8,
    stage: &str,
    error: &std::io::Error,
    is_connected: &AtomicBool,
    is_in_session: &AtomicBool,
) {
    let detail = format!(
        "packet_type={} stage={} read_exact failed: {:?}",
        packet_type, stage, error
    );
    crate::session_debug::lifecycle(
        identity,
        "PACKET_READ_FAILED",
        "session_input_reader",
        "cloned_session_reader_stream",
        Some(is_connected),
        Some(is_in_session),
        false,
        &detail,
    );
    mark_session_disconnected(
        identity,
        "session_input_reader",
        "cloned_session_reader_stream",
        is_connected,
        is_in_session,
        false,
        &detail,
    );
}

// ============================================================
// KEY MAPPING
// ============================================================

// FIXED: expanded VK map covering arrows, F-keys, nav cluster, and modifiers.
#[allow(dead_code)]
fn map_key_code(code: u32) -> Option<Key> {
    match code {
        65 => Some(Key::Unicode('a')),
        66 => Some(Key::Unicode('b')),
        67 => Some(Key::Unicode('c')),
        68 => Some(Key::Unicode('d')),
        69 => Some(Key::Unicode('e')),
        70 => Some(Key::Unicode('f')),
        71 => Some(Key::Unicode('g')),
        72 => Some(Key::Unicode('h')),
        73 => Some(Key::Unicode('i')),
        74 => Some(Key::Unicode('j')),
        75 => Some(Key::Unicode('k')),
        76 => Some(Key::Unicode('l')),
        77 => Some(Key::Unicode('m')),
        78 => Some(Key::Unicode('n')),
        79 => Some(Key::Unicode('o')),
        80 => Some(Key::Unicode('p')),
        81 => Some(Key::Unicode('q')),
        82 => Some(Key::Unicode('r')),
        83 => Some(Key::Unicode('s')),
        84 => Some(Key::Unicode('t')),
        85 => Some(Key::Unicode('u')),
        86 => Some(Key::Unicode('v')),
        87 => Some(Key::Unicode('w')),
        88 => Some(Key::Unicode('x')),
        89 => Some(Key::Unicode('y')),
        90 => Some(Key::Unicode('z')),

        48 => Some(Key::Unicode('0')),
        49 => Some(Key::Unicode('1')),
        50 => Some(Key::Unicode('2')),
        51 => Some(Key::Unicode('3')),
        52 => Some(Key::Unicode('4')),
        53 => Some(Key::Unicode('5')),
        54 => Some(Key::Unicode('6')),
        55 => Some(Key::Unicode('7')),
        56 => Some(Key::Unicode('8')),
        57 => Some(Key::Unicode('9')),

        32 => Some(Key::Space),
        13 => Some(Key::Return),
        8 => Some(Key::Backspace),
        9 => Some(Key::Tab),
        27 => Some(Key::Escape),

        16 | 160 | 161 => Some(Key::Shift),
        17 | 162 | 163 => Some(Key::Control),
        18 | 164 | 165 => Some(Key::Alt),
        91 | 92 => Some(Key::Meta),

        37 => Some(Key::LeftArrow),
        38 => Some(Key::UpArrow),
        39 => Some(Key::RightArrow),
        40 => Some(Key::DownArrow),

        33 => Some(Key::PageUp),
        34 => Some(Key::PageDown),
        35 => Some(Key::End),
        36 => Some(Key::Home),
        45 => Some(Key::Insert),
        46 => Some(Key::Delete),

        112 => Some(Key::F1),
        113 => Some(Key::F2),
        114 => Some(Key::F3),
        115 => Some(Key::F4),
        116 => Some(Key::F5),
        117 => Some(Key::F6),
        118 => Some(Key::F7),
        119 => Some(Key::F8),
        120 => Some(Key::F9),
        121 => Some(Key::F10),
        122 => Some(Key::F11),
        123 => Some(Key::F12),

        186 => Some(Key::Unicode(';')),
        187 => Some(Key::Unicode('=')),
        188 => Some(Key::Unicode(',')),
        189 => Some(Key::Unicode('-')),
        190 => Some(Key::Unicode('.')),
        191 => Some(Key::Unicode('/')),
        192 => Some(Key::Unicode('`')),
        219 => Some(Key::Unicode('[')),
        220 => Some(Key::Unicode('\\')),
        221 => Some(Key::Unicode(']')),
        222 => Some(Key::Unicode('\'')),

        _ => None,
    }
}

// ============================================================
// FRAME DATA
// ============================================================

struct FrameData {
    width: usize,
    height: usize,
    raw_pixels: Vec<u8>,
    captured_at_ms: u64,
}

#[cfg(windows)]
fn capture_screen_gdi() -> Option<FrameData> {
    unsafe {
        let hdesktop = windows_sys::Win32::System::StationsAndDesktops::OpenInputDesktop(
            0,
            0,
            0x10000000u32, // GENERIC_ALL
        );
        if hdesktop != 0 {
            windows_sys::Win32::System::StationsAndDesktops::SetThreadDesktop(hdesktop);
            windows_sys::Win32::System::StationsAndDesktops::CloseDesktop(hdesktop);
        }

        let width = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) as usize;
        let height = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) as usize;
        if width == 0 || height == 0 {
            return None;
        }

        let hdc_screen = windows_sys::Win32::Graphics::Gdi::GetDC(0 as _);
        if hdc_screen == 0 {
            return None;
        }
        let hdc_mem = windows_sys::Win32::Graphics::Gdi::CreateCompatibleDC(hdc_screen);
        if hdc_mem == 0 {
            windows_sys::Win32::Graphics::Gdi::ReleaseDC(0 as _, hdc_screen);
            return None;
        }

        let mut bmi: windows_sys::Win32::Graphics::Gdi::BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32); // Top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = windows_sys::Win32::Graphics::Gdi::BI_RGB;

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm = windows_sys::Win32::Graphics::Gdi::CreateDIBSection(
            hdc_screen,
            &bmi,
            windows_sys::Win32::Graphics::Gdi::DIB_RGB_COLORS,
            &mut bits_ptr,
            0 as _,
            0,
        );

        if hbm == 0 || bits_ptr.is_null() {
            windows_sys::Win32::Graphics::Gdi::DeleteDC(hdc_mem);
            windows_sys::Win32::Graphics::Gdi::ReleaseDC(0 as _, hdc_screen);
            return None;
        }

        let old_obj = windows_sys::Win32::Graphics::Gdi::SelectObject(hdc_mem, hbm as _);
        let blt_res = windows_sys::Win32::Graphics::Gdi::BitBlt(
            hdc_mem,
            0,
            0,
            width as i32,
            height as i32,
            hdc_screen,
            0,
            0,
            windows_sys::Win32::Graphics::Gdi::SRCCOPY | windows_sys::Win32::Graphics::Gdi::CAPTUREBLT,
        );

        let buf_size = width * height * 4;
        let mut buffer = vec![0u8; buf_size];

        if blt_res != 0 {
            let src_slice = std::slice::from_raw_parts(bits_ptr as *const u8, buf_size);
            // Convert BGRA to RGBA
            for (dst_px, src_px) in buffer.chunks_exact_mut(4).zip(src_slice.chunks_exact(4)) {
                dst_px[0] = src_px[2]; // R
                dst_px[1] = src_px[1]; // G
                dst_px[2] = src_px[0]; // B
                dst_px[3] = 255;       // A
            }
        }

        windows_sys::Win32::Graphics::Gdi::SelectObject(hdc_mem, old_obj);
        windows_sys::Win32::Graphics::Gdi::DeleteObject(hbm as _);
        windows_sys::Win32::Graphics::Gdi::DeleteDC(hdc_mem);
        windows_sys::Win32::Graphics::Gdi::ReleaseDC(0 as _, hdc_screen);

        if blt_res == 0 {
            return None;
        }

        Some(FrameData {
            width,
            height,
            raw_pixels: buffer,
            captured_at_ms: current_time_millis(),
        })
    }
}

// ============================================================
// AUDIO
// ============================================================

// FIXED: Convert CPAL input samples to the Float32 PCM representation used by Type 17.
fn build_audio_capture_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_rate: u32,
    channels: u16,
    write_stream: SyncSender<Vec<u8>>,
    is_running: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::FromSample;

    let mut captured_packets = 0u64;
    let mut dropped_packets = 0u64;
    let mut disconnected_logged = false;
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            if !is_running.load(Ordering::Relaxed)
                || !crate::status::session_audio_enabled()
                || data.is_empty()
            {
                return;
            }
            let byte_len = data.len().saturating_mul(std::mem::size_of::<f32>());
            let Ok(byte_len_u32) = u32::try_from(byte_len) else {
                return;
            };
            let mut packet = Vec::with_capacity(11 + byte_len);
            packet.push(17);
            packet.extend_from_slice(&byte_len_u32.to_be_bytes());
            packet.extend_from_slice(&sample_rate.to_be_bytes());
            packet.extend_from_slice(&channels.to_be_bytes());
            for sample in data {
                let sample: f32 = f32::from_sample_(*sample);
                packet.extend_from_slice(&sample.to_le_bytes());
            }
            match write_stream.try_send(packet) {
                Ok(()) => {
                    captured_packets = captured_packets.saturating_add(1);
                    if captured_packets == 1 || captured_packets % 100 == 0 {
                        eprintln!(
                            "[AUDIO TX] stage=queued packets={} sample_rate={} channels={} samples={}",
                            captured_packets,
                            sample_rate,
                            channels,
                            data.len()
                        );
                    }
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    dropped_packets = dropped_packets.saturating_add(1);
                    if dropped_packets == 1 || dropped_packets % 100 == 0 {
                        eprintln!(
                            "[AUDIO TX] stage=queue_drop reason=writer_queue_full dropped_packets={} sample_rate={} channels={} samples={}",
                            dropped_packets,
                            sample_rate,
                            channels,
                            data.len()
                        );
                    }
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    if !disconnected_logged {
                        eprintln!(
                            "[AUDIO TX] stage=queue_drop reason=writer_disconnected sample_rate={} channels={}",
                            sample_rate,
                            channels
                        );
                        disconnected_logged = true;
                    }
                }
            }
        },
        |error| eprintln!("[Audio] Input stream failed: {error}"),
        None,
    )
}

// FIXED: Use the selected device's native CPAL sample format for capture.
fn start_audio_capture(
    write_stream: std::sync::mpsc::SyncSender<Vec<u8>>,
    is_running: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| "No default microphone is available".to_string())?;
    let supported_config = device
        .default_input_config()
        .map_err(|error| format!("Could not read microphone configuration: {error}"))?;
    let sample_format = supported_config.sample_format();
    let stream_config: cpal::StreamConfig = supported_config.into();
    let sample_rate = stream_config.sample_rate.0;
    let channels = stream_config.channels;
    eprintln!(
        "[AUDIO TX] stage=capture_configured sample_format={:?} sample_rate={} channels={} buffer_size={:?}",
        sample_format,
        sample_rate,
        channels,
        stream_config.buffer_size
    );

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_audio_capture_stream::<f32>(
            &device, &stream_config, sample_rate, channels, write_stream, is_running,
        ),
        cpal::SampleFormat::I16 => build_audio_capture_stream::<i16>(
            &device, &stream_config, sample_rate, channels, write_stream, is_running,
        ),
        cpal::SampleFormat::U16 => build_audio_capture_stream::<u16>(
            &device, &stream_config, sample_rate, channels, write_stream, is_running,
        ),
        format => {
            return Err(format!("Unsupported microphone sample format: {format:?}"));
        }
    }
    .map_err(|error| format!("Could not create microphone stream: {error}"))?;

    stream
        .play()
        .map_err(|error| format!("Could not start microphone stream: {error}"))?;
    Ok(stream)
}

// FIXED: Convert Float32 playback samples into the output device's native CPAL format.
fn build_audio_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
    output_channels: usize,
    output_rate: u32,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let fade_frames = (output_rate as f32 * 0.2).max(1.0);
    let mut fade_level = 0.0f32;
    let mut last_sample = 0.0f32;

    device.build_output_stream(
        config,
        move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
            let Ok(mut samples) = queue.lock() else {
                output.fill(T::from_sample_(0.0));
                fade_level = 0.0;
                last_sample = 0.0;
                return;
            };

            for frame in output.chunks_mut(output_channels.max(1)) {
                let sample = match samples.pop_front() {
                    Some(sample) => {
                        fade_level = 1.0;
                        last_sample = sample;
                        sample
                    }
                    None => {
                        fade_level = (fade_level - 1.0 / fade_frames).max(0.0);
                        last_sample * fade_level
                    }
                };
                frame.fill(T::from_sample_(sample));
            }
        },
        |error| eprintln!("[Audio] Output stream failed: {error}"),
        None,
    )
}

// FIXED: single playback path — VecDeque<f32>, per-output-channel frames, linear resample
// upstream, and 200ms fade-out when the queue underruns.
fn setup_session_audio_playback() -> (Option<cpal::Stream>, Arc<Mutex<VecDeque<f32>>>, u32) {
    let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let device = match cpal::default_host().default_output_device() {
        Some(device) => device,
        None => {
            eprintln!("[Audio] No output device available for session playback.");
            return (None, queue, 0);
        }
    };
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[Audio] Failed to read output configuration: {}", error);
            return (None, queue, 0);
        }
    };
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.clone().into();
    let output_rate = stream_config.sample_rate.0;
    let output_channels = stream_config.channels.max(1) as usize;
    eprintln!(
        "[AUDIO RX] stage=playback_configured sample_format={:?} sample_rate={} channels={}",
        sample_format,
        output_rate,
        output_channels
    );
    let output_stream = match sample_format {
        cpal::SampleFormat::F32 => build_audio_output_stream::<f32>(
            &device, &stream_config, Arc::clone(&queue), output_channels, output_rate,
        ),
        cpal::SampleFormat::I16 => build_audio_output_stream::<i16>(
            &device, &stream_config, Arc::clone(&queue), output_channels, output_rate,
        ),
        cpal::SampleFormat::U16 => build_audio_output_stream::<u16>(
            &device, &stream_config, Arc::clone(&queue), output_channels, output_rate,
        ),
        format => {
            eprintln!("[Audio] Unsupported output sample format: {format:?}");
            return (None, queue, 0);
        }
    };
    let stream = match output_stream {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[Audio] Failed to create output stream: {}", error);
            return (None, queue, 0);
        }
    };
    if let Err(error) = stream.play() {
        eprintln!("[Audio] Failed to start output stream: {}", error);
        return (None, queue, 0);
    }
    (Some(stream), queue, output_rate)
}

// FIXED: downmix to mono then resample capture→output rate with linear interpolation.
fn decode_audio_packet(
    payload: &[u8],
    sample_rate: u32,
    channels: u16,
    output_rate: u32,
) -> Option<Vec<f32>> {
    if payload.is_empty()
        || payload.len() % 4 != 0
        || sample_rate == 0
        || sample_rate > 192_000
        || channels == 0
        || channels > 32
        || output_rate == 0
    {
        return None;
    }
    let channel_count = channels as usize;
    let samples = payload
        .chunks_exact(4)
        .map(|bytes| {
            let sample = f32::from_le_bytes(bytes.try_into().ok()?);
            Some(if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if samples.len() % channel_count != 0 {
        return None;
    }

    let mono = samples
        .chunks_exact(channel_count)
        .map(|frame| frame.iter().copied().sum::<f32>() / channel_count as f32)
        .collect::<Vec<_>>();

    if sample_rate == output_rate {
        return Some(mono);
    }

    let output_len = ((mono.len() as u64 * output_rate as u64) / sample_rate as u64) as usize;
    if output_len == 0 || mono.is_empty() {
        return Some(Vec::new());
    }

    let last = mono.len() - 1;
    let ratio = sample_rate as f64 / output_rate as f64;
    Some(
        (0..output_len)
            .map(|index| {
                let src = index as f64 * ratio;
                let i0 = src.floor() as usize;
                let i1 = (i0 + 1).min(last);
                let t = (src - i0 as f64) as f32;
                let s0 = mono[i0.min(last)];
                let s1 = mono[i1];
                s0 + (s1 - s0) * t
            })
            .collect(),
    )
}

#[cfg(test)]
mod audio_packet_tests {
    use super::decode_audio_packet;

    fn pcm(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn decodes_mono_float_pcm_without_changing_rate() {
        let decoded = decode_audio_packet(&pcm(&[-0.5, 0.0, 0.75]), 48_000, 1, 48_000)
            .expect("valid mono packet should decode");

        assert_eq!(decoded, vec![-0.5, 0.0, 0.75]);
    }

    #[test]
    fn downmixes_stereo_and_clamps_out_of_range_samples() {
        let decoded = decode_audio_packet(&pcm(&[1.5, -0.5, 0.5, 0.5]), 44_100, 2, 44_100)
            .expect("valid stereo packet should decode");

        assert_eq!(decoded, vec![0.25, 0.5]);
    }

    #[test]
    fn replaces_non_finite_pcm_samples_with_silence() {
        let decoded = decode_audio_packet(&pcm(&[f32::NAN, f32::INFINITY]), 48_000, 1, 48_000)
            .expect("valid packet with non-finite samples should decode safely");

        assert_eq!(decoded, vec![0.0, 0.0]);
    }

    #[test]
    fn resamples_to_the_output_rate() {
        let decoded = decode_audio_packet(&pcm(&[0.0, 1.0]), 24_000, 1, 48_000)
            .expect("valid packet should resample");

        assert_eq!(decoded.len(), 4);
        assert_eq!(decoded[0], 0.0);
        assert_eq!(decoded[1], 0.5);
        assert_eq!(decoded[2], 1.0);
        assert_eq!(decoded[3], 1.0);
    }

    #[test]
    fn rejects_malformed_packet_metadata_and_frames() {
        assert!(decode_audio_packet(&[0; 3], 48_000, 1, 48_000).is_none());
        assert!(decode_audio_packet(&pcm(&[0.0]), 0, 1, 48_000).is_none());
        assert!(decode_audio_packet(&pcm(&[0.0]), 48_000, 0, 48_000).is_none());
        assert!(decode_audio_packet(&pcm(&[0.0]), 48_000, 2, 48_000).is_none());
    }
}

fn read_next_file_chunk<R: Read>(reader: &mut R, buffer: &mut [u8]) -> std::io::Result<Option<usize>> {
    match reader.read(buffer)? {
        0 => Ok(None),
        bytes_read => Ok(Some(bytes_read)),
    }
}

fn read_packet_payload<R: Read>(reader: &mut R, payload_len: usize) -> std::io::Result<Vec<u8>> {
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

fn discard_packet_bytes<R: Read>(reader: &mut R, mut remaining: usize) -> std::io::Result<()> {
    let mut buffer = [0u8; 8192];
    while remaining > 0 {
        let chunk_len = remaining.min(buffer.len());
        reader.read_exact(&mut buffer[..chunk_len])?;
        remaining -= chunk_len;
    }
    Ok(())
}

fn validate_relative_transfer_path(path: &str) -> Result<PathBuf, String> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.contains('\0')
        || normalized.len() > 4096
    {
        return Err("Transfer path must be a safe relative path".to_string());
    }

    let mut relative = PathBuf::new();
    for part in normalized.split('/') {
        let upper = part.to_ascii_uppercase();
        let base = upper.split('.').next().unwrap_or_default();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.len() > 240
            || part.ends_with('.')
            || part.ends_with(' ')
            || part
                .chars()
                .any(|ch| ch.is_control() || "<>\"|?*".contains(ch))
            || matches!(base, "CON" | "PRN" | "AUX" | "NUL")
            || (base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && matches!(base.as_bytes()[3], b'1'..=b'9'))
        {
            return Err("Transfer path contains an unsafe component".to_string());
        }
        relative.push(part);
    }
    Ok(relative)
}

fn create_session_transfer_file(
    drop_dir: &Path,
    canonical_drop_dir: &Path,
    relative_filename: &str,
) -> std::io::Result<(File, PathBuf)> {
    let relative_path = validate_relative_transfer_path(relative_filename)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let mut destination = drop_dir.join(&relative_path);
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid destination path")
    })?;
    fs::create_dir_all(parent)?;
    let canonical_parent = parent.canonicalize()?;
    if !canonical_parent.starts_with(canonical_drop_dir) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Destination escapes the DeskStream download folder",
        ));
    }
    
    let file_stem = relative_path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let extension = relative_path.extension().map(|e| e.to_string_lossy().into_owned());
    let mut counter = 1;
    while destination.exists() {
        let new_name = if let Some(ext) = &extension {
            format!("{} ({}).{}", file_stem, counter, ext)
        } else {
            format!("{} ({})", file_stem, counter)
        };
        destination = destination.with_file_name(new_name);
        counter += 1;
    }

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    Ok((file, destination))
}


fn is_transfer_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn should_create_empty_transfer_file(
    current_transfer_matches: bool,
    accepted: bool,
    final_size: u64,
    file_is_open: bool,
) -> bool {
    current_transfer_matches && accepted && final_size == 0 && !file_is_open
}

fn collect_transfer_files(root: &Path) -> std::io::Result<Vec<(PathBuf, String)>> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
        let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if is_transfer_reparse_point(&metadata) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Folder transfer does not follow symbolic links or reparse points: {}",
                        path.display()
                    ),
                ));
            }
            if metadata.is_dir() {
                visit(root, &path, files)?;
            } else if metadata.is_file() {
                let relative = path.strip_prefix(root).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?;
                let relative_str = relative.to_string_lossy().replace('\\', "/");
                validate_relative_transfer_path(&relative_str)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                files.push((path, relative_str));
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Folder transfer encountered a non-regular file: {}",
                        path.display()
                    ),
                ));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    let folder_name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Folder".to_string());
    for (_, relative) in &mut files {
        *relative = format!("{folder_name}/{relative}");
        validate_relative_transfer_path(relative)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    }
    Ok(files)
}

fn send_session_file(
    writer: &SyncSender<Vec<u8>>,
    session_id: &str,
    source_path: &Path,
    relative_filename: &str,
) -> Result<(), String> {
    let filename = validate_relative_transfer_path(relative_filename)?;
    let filename = filename.to_string_lossy().replace('\\', "/");
    let name_bytes = filename.as_bytes();
    if name_bytes.len() > 4096 {
        return Err("Transfer path is too long".to_string());
    }

    let transfer_id: u64 = rand::random();
    let mut file = File::open(source_path).map_err(|error| {
        crate::session_debug::log(
            session_id,
            &format!(
                "[FILE SOURCE READ FAILED] session_id={} transfer_id={} side=B operation=File::open filename={} requested_bytes=0 actual_bytes=0 error_kind={:?} os_error={:?} error_message={}",
                session_id, transfer_id, filename, error.kind(), error.raw_os_error(), error
            ),
        );
        format!("Transfer {transfer_id}: failed to open source file: {error}")
    })?;
    let file_size = file.metadata().map_err(|error| {
        crate::session_debug::log(
            session_id,
            &format!(
                "[FILE SOURCE READ FAILED] session_id={} transfer_id={} side=B operation=File::metadata filename={} requested_bytes=0 actual_bytes=0 error_kind={:?} os_error={:?} error_message={}",
                session_id, transfer_id, filename, error.kind(), error.raw_os_error(), error
            ),
        );
        format!("Transfer {transfer_id}: failed to read source metadata: {error}")
    })?.len();
    let mut offer = vec![20u8];
    offer.extend_from_slice(&transfer_id.to_be_bytes());
    offer.extend_from_slice(&file_size.to_be_bytes());
    offer.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    offer.extend_from_slice(name_bytes);

    let response = crate::status::register_session_file_response(transfer_id);
    crate::session_debug::log(
        session_id,
        &format!(
            "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=B receiver=A stage=OFFER_CREATED filename={} file_bytes={}",
            session_id, transfer_id, filename, file_size
        ),
    );
    if writer.send(offer).is_err() {
        crate::status::clear_session_file_response(transfer_id);
        crate::session_debug::log(
            session_id,
            &format!(
                "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=B receiver=A stage=OFFER_SEND_FAILED",
                session_id, transfer_id
            ),
        );
        return Err(format!("Transfer {transfer_id}: offer send failed"));
    }
    println!(
        "[FILE TX START] direction=B->A transfer_id={} filename={} total_bytes={}",
        transfer_id, filename, file_size
    );
    crate::session_debug::log(
        session_id,
        &format!(
            "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=B receiver=A stage=WAITING_FOR_APPROVAL timeout_ms=60000",
            session_id, transfer_id
        ),
    );

    match response.recv_timeout(Duration::from_secs(60)) {
        Ok(26) => {
            crate::session_debug::log(
                session_id,
                &format!(
                    "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=B receiver=A stage=APPROVAL_RECEIVED decision=ALLOW",
                    session_id, transfer_id
                ),
            );
        }
        Ok(25) => {
            crate::session_debug::log(
                session_id,
                &format!(
                    "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=B receiver=A stage=APPROVAL_RECEIVED decision=REJECT",
                    session_id, transfer_id
                ),
            );
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!("Transfer {transfer_id} was rejected"));
        }
        Ok(other) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: unexpected offer response {other}"
            ));
        }
        Err(error) => {
            crate::session_debug::log(
                session_id,
                &format!(
                    "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=B receiver=A stage=APPROVAL_WAIT_FAILED reason={}",
                    session_id, transfer_id, error
                ),
            );
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: no acceptance response: {error}"
            ));
        }
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    let mut chunk_idx = 0u32;
    let mut bytes_sent = 0u64;
    loop {
        let bytes_read = match read_next_file_chunk(&mut file, &mut buffer) {
            Ok(None) => break,
            Ok(Some(bytes_read)) => bytes_read,
            Err(error) => {
                let message = format!("Failed to read source file: {error}");
                crate::session_debug::log(
                    session_id,
                    &format!(
                        "[FILE SOURCE READ FAILED] session_id={} transfer_id={} side=B operation=Read::read filename={} offset={} requested_bytes={} actual_bytes=0 file_size={} error_kind={:?} os_error={:?} error_message={}",
                        session_id,
                        transfer_id,
                        filename,
                        bytes_sent,
                        buffer.len(),
                        file_size,
                        error.kind(),
                        error.raw_os_error(),
                        error
                    ),
                );
                let _ = writer.send(make_file_error_packet(transfer_id, &message));
                crate::status::clear_session_file_response(transfer_id);
                return Err(format!("Transfer {transfer_id}: {message}"));
            }
        };
        hasher.update(&buffer[..bytes_read]);
        let mut packet = vec![21u8];
        packet.extend_from_slice(&transfer_id.to_be_bytes());
        packet.extend_from_slice(&chunk_idx.to_be_bytes());
        packet.extend_from_slice(&(bytes_read as u32).to_be_bytes());
        packet.extend_from_slice(&buffer[..bytes_read]);
        if writer.send(packet).is_err() {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!("Transfer {transfer_id}: chunk send failed"));
        }
        bytes_sent += bytes_read as u64;
        chunk_idx += 1;
        if chunk_idx == 1 || chunk_idx % 32 == 0 {
            crate::session_debug::log(
                session_id,
                &format!(
                    "[FILE TRACE] session_id={} transfer_id={} side=B direction=B_TO_A stage=CHUNK_SENT chunk_index={} chunks_sent={} cumulative_bytes={}",
                    session_id, transfer_id, chunk_idx - 1, chunk_idx, bytes_sent
                ),
            );
        }
        thread::sleep(Duration::from_millis(10));
    }

    if bytes_sent != file_size {
        let message = format!(
            "Source file changed while sending (expected {file_size} bytes, read {bytes_sent})"
        );
        let _ = writer.send(make_file_error_packet(transfer_id, &message));
        crate::status::clear_session_file_response(transfer_id);
        return Err(format!("Transfer {transfer_id}: {message}"));
    }

    let hash = hasher.finalize();
    let mut end_packet = vec![22u8];
    end_packet.extend_from_slice(&transfer_id.to_be_bytes());
    end_packet.extend_from_slice(&file_size.to_be_bytes());
    end_packet.extend_from_slice(&hash);
    crate::session_debug::log(
        session_id,
        &format!(
            "[FILE TRACE] session_id={} transfer_id={} side=B direction=B_TO_A stage=END_SENT chunks={} total_bytes={} sha256={}",
            session_id,
            transfer_id,
            chunk_idx,
            bytes_sent,
            hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        ),
    );
    if writer.send(end_packet).is_err() {
        crate::status::clear_session_file_response(transfer_id);
        return Err(format!(
            "Transfer {transfer_id}: completion packet send failed"
        ));
    }

    let hash_hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    match response.recv_timeout(Duration::from_secs(60)) {
        Ok(27) => println!(
            "[FILE TX END] direction=B->A transfer_id={} final_sha256={} success=true",
            transfer_id, hash_hex
        ),
        Ok(other) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: unexpected completion response {other}"
            ));
        }
        Err(error) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: no completion acknowledgement: {error}"
            ));
        }
    }
    crate::status::clear_session_file_response(transfer_id);
    Ok(())
}

fn make_file_error_packet(transfer_id: u64, message: &str) -> Vec<u8> {
    let message_bytes = message.as_bytes();
    let message_bytes = &message_bytes[..message_bytes.len().min(u16::MAX as usize)];
    let mut packet = Vec::with_capacity(13 + message_bytes.len());
    packet.push(24);
    packet.extend_from_slice(&transfer_id.to_be_bytes());
    packet.extend_from_slice(&[0, 0]);
    packet.extend_from_slice(&(message_bytes.len() as u16).to_be_bytes());
    packet.extend_from_slice(message_bytes);
    packet
}

// ============================================================
// AGENT LOOP
// ============================================================

fn run_agent_loop(relay_addr: String, config: identity::device_id::AgentConfig) {
    let host_pin = config.get_effective_pin();
    let backend_url = if !config.backend_url.is_empty() {
        config.backend_url.clone()
    } else {
        identity::device_id::DEFAULT_BACKEND_URL.to_string()
    };
    stage_log(
        &config.system_id,
        "AGENT_STARTUP",
        &format!(
            "system_id={} device_uuid={} relay_addr={} backend_url={} host_pin_configured={}",
            config.system_id,
            config.device_uuid,
            relay_addr,
            backend_url,
            !host_pin.trim().is_empty()
        ),
    );
    crate::session_debug::log(
        &config.system_id,
        &format!("B_START system_id={}", config.system_id),
    );
    crate::session_debug::log(
        &config.system_id,
        &format!("B_RELAY_CONFIG endpoint={}", relay_addr),
    );
    // Use persistent device UUID as the machine identifier
    // Use the config system_id (deterministic from UUID) as the initial system_id hint
    let mut backend = registration::backend_client::BackendClient::new(
        &backend_url,
        &config.device_uuid,
        &config.system_id,
    );

    agent_log!("[DISCOVERY] Registering with backend: {}", backend_url);
    agent_log!("[AGENT] Connecting to relay");
    
    let system_id = if let Some(assigned_id) = backend.register() {
        agent_log!("[DISCOVERY] Registered successfully with Web Dashboard / Device Registry!");
        agent_log!("[DISCOVERY] Backend assigned System ID: {}", assigned_id);
        agent_log!("[AGENT] Local device ID = {}", assigned_id);
        assigned_id
    } else {
        agent_log!("[DISCOVERY] Running in standalone relay mode.");
        config.system_id.clone()
    };
    if system_id.len() != 9 || !system_id.bytes().all(|byte| byte.is_ascii_digit()) {
        eprintln!(
            "[AUTH][ERROR] reason=protocol_error stage=device_registration device_id_length={}",
            system_id.len()
        );
        return;
    }
    println!("[AUTH] host_pin_configured={}", !host_pin.is_empty());
    crate::session_debug::log(
        &system_id,
        &format!("B_SYSTEM_ID system_id={}", system_id),
    );
    crate::session_debug::log(
        &system_id,
        &format!("ROLE role=CONTROLLED video_direction=B->A system_id={}", system_id),
    );

    let id_str = {
        let clean: String = system_id.chars().filter(|c| c.is_ascii_digit()).collect();
        if clean.len() == 9 {
            format!("{} {} {}", &clean[0..3], &clean[3..6], &clean[6..9])
        } else {
            clean.clone()
        }
    };

    agent_log!("[AGENT] Registering device = {}", id_str);
    agent_log!("[IDENTITY] Starting Host Desktop Agent...");
    agent_log!("[IDENTITY] Device ID: {}", system_id);
    agent_log!("[IDENTITY] Device UUID:  {}", config.device_uuid);
    agent_log!("[IDENTITY] System ID:    {}", system_id);
    agent_log!("[IDENTITY] Display ID:   {}", id_str);

    let global_status = Arc::new(Mutex::new(crate::status::AgentStatus::new(&system_id, "Windows Device")));
    backend.start_heartbeat_thread(global_status.clone());

    let mut reconnect_attempt = 0u32;
    let mut authorization_required = false;
    let mut intentional_reconnect = false;
    let is_aws_relay = !relay_addr.starts_with("192.168.") && !relay_addr.starts_with("10.") && !relay_addr.starts_with("127.") && !relay_addr.starts_with("localhost") && !relay_addr.starts_with("172.");

    loop {
        if authorization_required {
            crate::status::set_reconnect_state(
                "Disconnected",
                reconnect_attempt,
                MAX_RECONNECT_ATTEMPTS,
            );
            crate::session_debug::log(
                &system_id,
                &format!(
                    "[RECONNECT TRACE] timestamp_ms={} stage=WAITING_FOR_MANUAL_REAUTH attempt={}/{}",
                    crate::status::now_ms(), reconnect_attempt, MAX_RECONNECT_ATTEMPTS
                ),
            );
            while !crate::status::take_manual_reconnect_request() {
                thread::sleep(Duration::from_millis(250));
            }
            crate::session_debug::log(
                &system_id,
                &format!(
                    "[RECONNECT TRACE] timestamp_ms={} stage=MANUAL_REAUTH_REQUESTED",
                    crate::status::now_ms()
                ),
            );
            authorization_required = false;
            reconnect_attempt = 0;
            crate::status::set_reconnect_state(
                "Reconnecting",
                reconnect_attempt,
                MAX_RECONNECT_ATTEMPTS,
            );
        }

        let reconnect_was_requested = intentional_reconnect;
        if intentional_reconnect {
            intentional_reconnect = false;
        }
        crate::status::set_reconnect_state(
            "Reconnecting",
            reconnect_attempt.saturating_add(1),
            0,
        );
        crate::session_debug::log(
            &system_id,
            &format!(
                "[RECONNECT TRACE] timestamp_ms={} stage=ATTEMPT_STARTED attempt={} retry_limit=unbounded intentional={}",
                crate::status::now_ms(),
                reconnect_attempt.saturating_add(1),
                reconnect_was_requested
            ),
        );
        stage_log(
            &system_id,
            "RECONNECT_STARTED",
            &format!(
                "attempt={} max_attempts={} intentional={}",
                reconnect_attempt.saturating_add(1),
                MAX_RECONNECT_ATTEMPTS,
                reconnect_was_requested
            ),
        );

        agent_log!("[RELAY] Connecting to relay {}...", relay_addr);
        stage_log(
            &system_id,
            "RELAY_CONNECT_STARTED",
            &format!("relay_addr={} system_id={}", relay_addr, system_id),
        );
        crate::session_debug::lifecycle(
            &system_id,
            "OUTER_LOOP_RECONNECT_STARTED",
            "run_agent_loop",
            "original_relay_socket",
            None,
            None,
            reconnect_was_requested,
            "starting TcpStream::connect",
        );
        crate::session_debug::lifecycle(
            &system_id,
            "RECONNECT_STARTED",
            "run_agent_loop",
            "original_relay_socket",
            None,
            None,
            reconnect_was_requested,
            "starting TcpStream::connect",
        );
        crate::session_debug::log(&system_id, "B_TCP_CONNECT_START");

        let stream = match TcpStream::connect(&relay_addr) {
            Ok(s) => {
                stage_log(
                    &system_id,
                    "RELAY_CONNECTED",
                    &format!("relay_addr={} system_id={}", relay_addr, system_id),
                );
                crate::session_debug::lifecycle(
                    &system_id,
                    "NEW_RELAY_CONNECTION",
                    "run_agent_loop",
                    "original_relay_socket",
                    None,
                    None,
                    reconnect_was_requested,
                    "TcpStream::connect succeeded",
                );
                crate::session_debug::log(&system_id, "B_TCP_CONNECT_SUCCESS");
                let _ = s.set_nodelay(true);
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                s
            }
            Err(e) => {
                stage_log(
                    &system_id,
                    "RELAY_DISCONNECTED",
                    &format!("reason=tcp_connect_failed error={:?}", e),
                );
                crate::session_debug::log(
                    &system_id,
                    &format!("B_TCP_CONNECT_FAILED error={}", e),
                );
                crate::session_debug::log(&system_id, "B_DISCONNECT");
                crate::session_debug::log(
                    &system_id,
                    "STATE_CHANGE old=CONNECTING new=DISCONNECTED",
                );
                eprintln!("[RELAY] Connection failed: {:?}", e);
                println!("[SESSION STATE] OFFLINE");
                println!("[RELAY][DISCONNECT] reason=Connection refused or network unreachable");
                println!("[RELAY][DISCONNECT] system_id={}", system_id);
                println!("[RELAY][DISCONNECT] socket_error={:?}", e);
                println!("[RELAY][DISCONNECT] remote_closed=false");
                next_reconnect_attempt(&mut reconnect_attempt);
                record_reconnect_failure(
                    &system_id,
                    reconnect_attempt,
                    "tcp_connect",
                    &format!("error_kind={:?}", e.kind()),
                );
                continue;
            }
        };
        let mut stream = LifecycleTcpStream::new(
            stream,
            system_id.clone(),
            "run_agent_loop",
            "original_relay_socket",
            "RELAY_STREAM_DROP",
            None,
            None,
            reconnect_was_requested,
        );

        println!("[RELAY] TCP connection established");
        println!("[RELAY] Sending registration for System ID: {}", system_id);
        crate::session_debug::log(
            &system_id,
            &format!("B_REGISTER_START system_id={}", system_id),
        );

        // Registration (Type 1 + System ID as relay session key)
        let mut register_pkt = Vec::with_capacity(1 + system_id.len());
        register_pkt.push(1u8);
        register_pkt.extend_from_slice(system_id.as_bytes());

        if let Err(error) = stream.write_all(&register_pkt) {
            crate::session_debug::log(&system_id, "B_REGISTER_SEND_FAILED");
            eprintln!("[RELAY] Failed to send registration packet");
            println!("[SESSION STATE] OFFLINE");
            next_reconnect_attempt(&mut reconnect_attempt);
            record_reconnect_failure(
                &system_id,
                reconnect_attempt,
                "registration_write",
                &format!("error_kind={:?}", error.kind()),
            );
            continue;
        }
        crate::session_debug::log(
            &system_id,
            &format!("B_REGISTER_SENT system_id={}", system_id),
        );

        let mut ack = [0u8; 1];
        if let Err(error) = stream.read_exact(&mut ack) {
            crate::session_debug::log(&system_id, "B_REGISTER_ACK_READ_FAILED");
            eprintln!("[RELAY] Registration failed or unacknowledged by relay");
            println!("[SESSION STATE] OFFLINE");
            next_reconnect_attempt(&mut reconnect_attempt);
            record_reconnect_failure(
                &system_id,
                reconnect_attempt,
                "registration_ack_read",
                &format!("error_kind={:?}", error.kind()),
            );
            continue;
        }
        crate::session_debug::log(
            &system_id,
            &format!("B_REGISTER_ACK ack={}", ack[0]),
        );
        if ack[0] != 1 {
            eprintln!("[RELAY] Registration failed or unacknowledged by relay");
            println!("[SESSION STATE] OFFLINE");
            reconnect_attempt = MAX_RECONNECT_ATTEMPTS;
            authorization_required = true;
            crate::status::set_disconnect_reason_if_unknown("authorization_rejected");
            crate::status::set_relay_state(false);
            crate::status::set_reconnect_state(
                "Disconnected",
                MAX_RECONNECT_ATTEMPTS,
                MAX_RECONNECT_ATTEMPTS,
            );
            crate::session_debug::log(
                &system_id,
                &format!(
                    "[RECONNECT TRACE] timestamp_ms={} stage=ATTEMPT_FAILED source=registration_rejected reason=authorization_rejected",
                    crate::status::now_ms()
                ),
            );
            crate::session_debug::log(
                &system_id,
                &format!(
                    "[RECONNECT TRACE] timestamp_ms={} stage=RETRY_CANCELLED reason=authorization_rejected manual_retry_required=true",
                    crate::status::now_ms()
                ),
            );
            continue;
        }

        println!("[RELAY] Registration ACK received");
        stage_log(
            &system_id,
            "DEVICE_REGISTERED",
            &format!("device_id={} ack={}", system_id, ack[0]),
        );
        crate::status::set_relay_state(true);
        crate::status::set_reconnect_state("Healthy", 0, MAX_RECONNECT_ATTEMPTS);
        reconnect_attempt = 0;
        crate::session_debug::log(
            &system_id,
            "STATE_CHANGE old=CONNECTING new=CONNECTED",
        );
        agent_log!("[AGENT] Registration acknowledged");
        agent_log!("[AGENT] Registration sent");
        agent_log!("[AGENT] ACTUAL DEVICE ID = {}", id_str);

        println!("[SESSION STATE] ONLINE");

        let idle_write_stream = match stream.try_clone() {
            Ok(s) => Arc::new(Mutex::new(s)),
            Err(e) => {
                eprintln!("[Agent] Failed to clone stream for writer: {:?}", e);
                next_reconnect_attempt(&mut reconnect_attempt);
                record_reconnect_failure(
                    &system_id,
                    reconnect_attempt,
                    "relay_stream_clone",
                    &format!("error_kind={:?}", e.kind()),
                );
                continue;
            }
        };

        let is_running_conn = Arc::new(AtomicBool::new(true));
        let is_in_session = Arc::new(AtomicBool::new(false));
        stream.is_in_session = Some(Arc::clone(&is_in_session));

        // Idle Type 14 probes measure this agent's TCP path to the relay.
        let hb_write = Arc::clone(&idle_write_stream);
        let hb_running = Arc::clone(&is_running_conn);
        let hb_in_session = Arc::clone(&is_in_session);
        let heartbeat_identity = system_id.clone();
        let idle_heartbeat = Arc::new(Mutex::new(crate::status::HeartbeatTracker::default()));
        let idle_heartbeat_sender = Arc::clone(&idle_heartbeat);

        let heartbeat_handle = thread::spawn(move || {
            let mut previous_id = 0u64;
            let mut next_ping = Instant::now();
            while hb_running.load(Ordering::SeqCst) {
                if !hb_in_session.load(Ordering::SeqCst) {
                    let now = Instant::now();
                    let timed_out = idle_heartbeat_sender
                        .lock()
                        .ok()
                        .and_then(|mut tracker| tracker.timeout(now, HEARTBEAT_TIMEOUT));
                    if let Some(probe_id) = timed_out {
                        crate::session_debug::log(
                            &heartbeat_identity,
                            &format!(
                                "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=TIMEOUT reason=response_timeout",
                                crate::status::now_ms(), probe_id
                            ),
                        );
                        log_health_transition(
                            &heartbeat_identity,
                            "ENDPOINT_TO_RELAY",
                            crate::status::record_heartbeat_missed(false, "response_timeout"),
                        );
                    }

                    if now >= next_ping {
                        let probe_id = next_heartbeat_id(&mut previous_id);
                        let scheduled = idle_heartbeat_sender
                            .lock()
                            .map(|mut tracker| tracker.schedule(probe_id))
                            .unwrap_or(false);
                        if scheduled {
                            crate::session_debug::log(
                                &heartbeat_identity,
                                &format!(
                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=SCHEDULED type=14 interval_ms={}",
                                    crate::status::now_ms(), probe_id, HEARTBEAT_INTERVAL.as_millis()
                                ),
                            );
                            let mut ping_pkt = Vec::with_capacity(9);
                            ping_pkt.push(14u8);
                            ping_pkt.extend_from_slice(&probe_id.to_be_bytes());

                            let mut writer = match hb_write.lock() {
                                Ok(writer) => writer,
                                Err(_) => {
                                    if let Ok(mut tracker) = idle_heartbeat_sender.lock() {
                                        tracker.cancel(probe_id);
                                    }
                                    continue;
                                }
                            };
                            if hb_in_session.load(Ordering::SeqCst) {
                                if let Ok(mut tracker) = idle_heartbeat_sender.lock() {
                                    tracker.cancel(probe_id);
                                }
                                crate::session_debug::log(
                                    &heartbeat_identity,
                                    &format!(
                                        "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=CANCELLED reason=session_started",
                                        crate::status::now_ms(), probe_id
                                    ),
                                );
                                continue;
                            }
                            let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
                            let write_result = writer.write_all(&ping_pkt);
                            drop(writer);
                            match write_result {
                                Ok(()) => {
                                    if let Ok(mut tracker) = idle_heartbeat_sender.lock() {
                                        tracker.mark_sent(probe_id, Instant::now());
                                    }
                                    stage_log(
                                        &heartbeat_identity,
                                        "HEARTBEAT_SENT",
                                        &format!("type=14 probe_id={} packet_bytes=9", probe_id),
                                    );
                                    crate::session_debug::log(
                                        &heartbeat_identity,
                                        &format!(
                                            "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=SENT type=14 packet_bytes=9",
                                            crate::status::now_ms(), probe_id
                                        ),
                                    );
                                }
                                Err(error) => {
                                    if let Ok(mut tracker) = idle_heartbeat_sender.lock() {
                                        tracker.cancel(probe_id);
                                    }
                                    crate::session_debug::log(
                                        &heartbeat_identity,
                                        &format!(
                                            "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=WRITE_ERROR error_kind={:?}",
                                            crate::status::now_ms(), probe_id, error.kind()
                                        ),
                                    );
                                    log_health_transition(
                                        &heartbeat_identity,
                                        "ENDPOINT_TO_RELAY",
                                        crate::status::record_heartbeat_missed(false, "socket_write_error"),
                                    );
                                    crate::session_debug::lifecycle(
                                        &heartbeat_identity,
                                        "SOCKET_WRITE_ERROR",
                                        "relay_heartbeat",
                                        "cloned_idle_heartbeat_stream",
                                        None,
                                        Some(hb_in_session.as_ref()),
                                        false,
                                        &format!("Type 14 write_all failed: {:?}", error.kind()),
                                    );
                                }
                            }
                            next_ping = Instant::now() + HEARTBEAT_INTERVAL;
                        }
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
        });

        'viewer_loop: loop {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut type_buf = [0u8; 1];

            match stream.read_exact(&mut type_buf) {
                Ok(_) => {
                    stage_log(
                        &system_id,
                        "INCOMING_MESSAGE_RECEIVED",
                        &format!("message_type={}", type_buf[0]),
                    );
                    match type_buf[0] {
                        // Type 14: Heartbeat ACK from relay
                        14 => {
                            let mut time_buf = [0u8; 8];
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            match stream.read_exact(&mut time_buf) {
                                Ok(()) => {
                                    let probe_id = u64::from_be_bytes(time_buf);
                                    let received_at = Instant::now();
                                    let outcome = idle_heartbeat
                                        .lock()
                                        .map(|mut tracker| tracker.receive(probe_id, received_at))
                                        .unwrap_or(crate::status::HeartbeatReceive::Stale);
                                    match outcome {
                                        crate::status::HeartbeatReceive::Matched { rtt_ms } => {
                                            stage_log(
                                                &system_id,
                                                "HEARTBEAT_ACKNOWLEDGED",
                                                &format!("probe_id={} rtt_ms={}", probe_id, rtt_ms),
                                            );
                                            crate::session_debug::log(
                                                &system_id,
                                                &format!(
                                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=RESPONSE_RECEIVED packet_type=14",
                                                    crate::status::now_ms(), probe_id
                                                ),
                                            );
                                            crate::session_debug::log(
                                                &system_id,
                                                &format!(
                                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=RTT_CALCULATED rtt_ms={}",
                                                    crate::status::now_ms(), probe_id, rtt_ms
                                                ),
                                            );
                                            log_health_transition(
                                                &system_id,
                                                "ENDPOINT_TO_RELAY",
                                                crate::status::record_heartbeat_success(false, rtt_ms),
                                            );
                                            println!("[HEARTBEAT] endpoint_to_relay_rtt_ms={}", rtt_ms);
                                        }
                                        crate::status::HeartbeatReceive::Stale => {
                                            crate::session_debug::log(
                                                &system_id,
                                                &format!(
                                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=STALE_OR_UNMATCHED_RESPONSE",
                                                    crate::status::now_ms(), probe_id
                                                ),
                                            );
                                        }
                                        crate::status::HeartbeatReceive::PeerRequest => {
                                            crate::session_debug::log(
                                                &system_id,
                                                &format!(
                                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=UNEXPECTED_PEER_PROBE",
                                                    crate::status::now_ms(), probe_id
                                                ),
                                            );
                                        }
                                    }
                                }
                                Err(error) => {
                                    if error.kind() == std::io::ErrorKind::UnexpectedEof {
                                        crate::session_debug::lifecycle(
                                            &system_id,
                                            "SOCKET_EOF",
                                            "run_agent_loop",
                                            "original_relay_socket",
                                            None,
                                            Some(is_in_session.as_ref()),
                                            reconnect_was_requested,
                                            &format!("Type 14 ACK read_exact failed: {:?}", error),
                                        );
                                    }
                                    crate::session_debug::lifecycle(
                                        &system_id,
                                        "SOCKET_READ_ERROR",
                                        "run_agent_loop",
                                        "original_relay_socket",
                                        None,
                                        Some(is_in_session.as_ref()),
                                        reconnect_was_requested,
                                        &format!("Type 14 ACK read_exact failed: {:?}", error),
                                    );
                                }
                            }
                        }

                        // Type 3: Incoming session request / Authentication
                        3 => {
                            stage_log(
                                &system_id,
                                "CONNECTION_REQUEST_RECEIVED",
                                &format!("message_type=3 device_id={}", system_id),
                            );
                            native_auth_trace(
                                &system_id,
                                "SESSION_REQUEST_RECEIVED",
                                "relay_type_3_received",
                            );
                            crate::session_debug::lifecycle(
                                &system_id,
                                "AUTH_REQUEST_RECEIVED",
                                "run_agent_loop",
                                "original_relay_socket",
                                None,
                                Some(is_in_session.as_ref()),
                                reconnect_was_requested,
                                "received Type 3 authentication request",
                            );
                            crate::session_debug::lifecycle(
                                &system_id,
                                "TYPE3_RECEIVED",
                                "run_agent_loop",
                                "original_relay_socket",
                                None,
                                Some(is_in_session.as_ref()),
                                intentional_reconnect,
                                "received type byte 3",
                            );
                            crate::session_debug::log(
                                &system_id,
                                &format!(
                                    "B_SESSION_REQUEST_RECEIVED type=3 system_id={}",
                                    system_id
                                ),
                            );
                            let mut auth_hash = [0u8; 32];
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                            if let Err(error) = stream.read_exact(&mut auth_hash) {
                                if error.kind() == std::io::ErrorKind::UnexpectedEof {
                                    crate::session_debug::lifecycle(
                                        &system_id,
                                        "SOCKET_EOF",
                                        "run_agent_loop",
                                        "original_relay_socket",
                                        None,
                                        Some(is_in_session.as_ref()),
                                        reconnect_was_requested,
                                        &format!("Type 3 authentication hash read_exact failed: {:?}", error),
                                    );
                                }
                                crate::session_debug::lifecycle(
                                    &system_id,
                                    "SOCKET_READ_ERROR",
                                    "run_agent_loop",
                                    "original_relay_socket",
                                    None,
                                    Some(is_in_session.as_ref()),
                                    reconnect_was_requested,
                                    &format!("Type 3 authentication hash read_exact failed: {:?}", error),
                                );
                                native_auth_trace(
                                    &system_id,
                                    AUTH_RELAY_DISCONNECTED,
                                    "type3_auth_hash_read_failed",
                                );
                                eprintln!("[Agent] Failed to read auth hash from relay.");
                                break 'viewer_loop;
                            }
                            let is_zero_hash = is_all_zero_auth_hash(&auth_hash);
                            let target_len = system_id.len();
                            stage_log(
                                &system_id,
                                "SESSION_FIELDS_VALIDATED",
                                &format!(
                                    "target_device_id_len={} expected_payload_len=32 actual_payload_len=32 hash_empty={}",
                                    target_len,
                                    is_zero_hash
                                ),
                            );
                            stage_log(
                                &system_id,
                                "AUTHENTICATION_STARTED",
                                &format!(
                                    "device_id={} host_approval_check=true supplied_digest_len=32",
                                    system_id
                                ),
                            );
                            native_auth_trace(
                                &system_id,
                                "AUTHENTICATION_DATA_RECEIVED",
                                "received_32_auth_bytes; host_approval_check; contents_not_logged",
                            );

                            let (is_approved, approved_token, approved_requester) = if is_zero_hash {
                                (false, None, None)
                            } else {
                                match crate::status::consume_approved_session(&auth_hash) {
                                    Some((tok, req)) => (true, Some(tok), Some(req)),
                                    None => (false, None, None),
                                }
                            };

                            if !is_approved {
                                stage_log(
                                    &system_id,
                                    "AUTHENTICATION_FAILED",
                                    &format!("device_id={} reason=HOST_APPROVAL_MISSING_OR_EXPIRED response_byte=0x00", system_id),
                                );
                                stage_log(
                                    &system_id,
                                    "SESSION_REJECTED",
                                    &format!("device_id={} response_byte=0 reason=HOST_APPROVAL_MISSING_OR_EXPIRED", system_id),
                                );
                                native_auth_trace(
                                    &system_id,
                                    "AUTHENTICATION_REJECTED",
                                    "HOST_APPROVAL_MISSING_OR_EXPIRED",
                                );
                                crate::session_debug::log(
                                    &system_id,
                                    &format!(
                                        "[AUTH] result=rejected reason=HOST_APPROVAL_MISSING_OR_EXPIRED response_byte=0x00 device_id={}",
                                        system_id
                                    ),
                                );
                                native_auth_trace(
                                    &system_id,
                                    AUTH_SESSION_REJECTED,
                                    "writing_auth_failure_byte_0x00",
                                );
                                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                                if let Err(error) = stream.write_all(&[0u8]) {
                                    native_auth_trace(
                                        &system_id,
                                        AUTH_RELAY_DISCONNECTED,
                                        "approval_response_write_failed",
                                    );
                                    eprintln!(
                                        "[AUTH][ERROR] reason=approval_response_write_failed error_kind={:?}",
                                        error.kind()
                                    );
                                    break 'viewer_loop;
                                }
                                continue 'viewer_loop;
                            }

                            stage_log(
                                &system_id,
                                "AUTHENTICATION_SUCCEEDED",
                                &format!("device_id={} response_byte=0x01", system_id),
                            );
                            native_auth_trace(
                                &system_id,
                                "AUTHENTICATION_ACCEPTED",
                                "host_approved_session_matched",
                            );
                            crate::session_debug::log(
                                &system_id,
                                &format!(
                                    "[AUTH] result=accepted response_byte=0x01 device_id={}",
                                    system_id
                                ),
                            );

                            println!("[Host] Received connection request for Target ID: {}", id_str);
                            println!("[SESSION] ACCEPT received");
                            println!("[SESSION] Starting remote session");
                            println!("[STREAM STATE] STARTING");
                            crate::session_debug::log(
                                &system_id,
                                "B_STREAM_STATE state=STARTING",
                            );
                            println!("[STREAM] Starting screen capture");
                            println!("[STREAM] Starting H.264 encoder");

                            is_in_session.store(true, Ordering::SeqCst);
                            let retired_idle_probe_id = idle_heartbeat
                                .lock()
                                .ok()
                                .and_then(|mut tracker| tracker.cancel_pending());
                            if let Some(probe_id) = retired_idle_probe_id {
                                crate::session_debug::log(
                                    &system_id,
                                    &format!(
                                        "[PING TRACE] timestamp_ms={} trace_id={} channel=ENDPOINT_TO_RELAY stage=CANCELLED reason=session_handoff",
                                        crate::status::now_ms(), probe_id
                                    ),
                                );
                            }
                            if let Ok(writer_guard) = idle_write_stream.lock() {
                                drop(writer_guard);
                            }

                            // The approval byte is a one-byte ACK in the type-3 auth flow and must be
                            // written on the active control socket, not via the idle heartbeat writer.
                            // This keeps the host relay state machine aligned with the relay's idle-loop
                            // heartbeat handling and avoids a stray '1' being treated as an unexpected idle byte.
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                            if let Err(error) = stream.write_all(&[1u8]) {
                                native_auth_trace(
                                    &system_id,
                                    "APPROVAL_SEND_FAILED",
                                    &format!("approval_ack_write_failed: {:?}", error),
                                );
                                crate::session_debug::lifecycle(
                                    &system_id,
                                    "SOCKET_WRITE_ERROR",
                                    "run_agent_loop",
                                    "original_relay_socket",
                                    None,
                                    Some(is_in_session.as_ref()),
                                    reconnect_was_requested,
                                    &format!("approval ACK write_all failed: {:?}", error),
                                );
                                eprintln!("[Agent] Failed to send approval ACK to relay.");
                                break 'viewer_loop;
                            }
                            stage_log(
                                &system_id,
                                "SESSION_ACCEPTED",
                                &format!("device_id={} response_byte=1 outcome=accepted", system_id),
                            );
                            stage_log(
                                &system_id,
                                "STREAM_INITIALIZATION_STARTED",
                                &format!("device_id={} target_fps={}", system_id, TARGET_FPS),
                            );
                            native_auth_trace(&system_id, "APPROVAL_SENT", "approval_ack_1_written");
                            crate::session_debug::log(
                                &system_id,
                                "B_SESSION_APPROVAL_SENT ack=1",
                            );
                            crate::session_debug::lifecycle(
                                &system_id,
                                "APPROVAL_SENT",
                                "run_agent_loop",
                                "original_relay_socket",
                                None,
                                Some(is_in_session.as_ref()),
                                intentional_reconnect,
                                "approval ACK byte 1 written",
                            );

                            let active_request_token = approved_token.clone();
                            if let Some(ref req_id) = approved_requester {
                                println!("[Host] Approved session for requester {}", req_id);
                            }
                            println!("[Host] Approval response sent successfully (APPROVED)");
                            backend.log_session_start_with_token(&system_id, active_request_token.as_deref(), Some("desktop"));
                            println!("[Agent] Session APPROVED! Starting live video...");
                            // ====================================================
                            // CONNECTION STATE & STREAMING PIPELINE
                            // ====================================================
                            let session_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(e) => {
                                    stage_log(
                                        &system_id,
                                        "STREAM_INITIALIZATION_FAILED",
                                        &format!("device_id={} error=session_stream_clone_failed: {:?}", system_id, e),
                                    );
                                    eprintln!("[Agent] Failed to clone stream for session thread: {}", e);
                                    break 'viewer_loop;
                                }
                            };
                            let backend_clone = backend.clone();
                            let system_id_clone = system_id.clone();
                            let is_in_session_clone = Arc::clone(&is_in_session);
                            crate::session_debug::lifecycle(
                                &system_id,
                                "SESSION_PIPELINE_SOCKET_CLONED",
                                "run_agent_loop",
                                "original_relay_socket",
                                None,
                                Some(is_in_session.as_ref()),
                                intentional_reconnect,
                                "session worker TcpStream::try_clone succeeded",
                            );
                            crate::session_debug::lifecycle(
                                &system_id,
                                "SESSION_PIPELINE_STARTED",
                                "run_agent_loop",
                                "cloned_session_stream",
                                None,
                                Some(is_in_session.as_ref()),
                                intentional_reconnect,
                                "spawning session worker",
                            );
                            native_auth_trace(
                                &system_id,
                                "SESSION_WORKER_STARTED",
                                "session_worker_spawned_after_approval",
                            );
                            
                            thread::spawn(move || {
                                let panic_identity = system_id_clone.clone();
                                let panic_in_session = Arc::clone(&is_in_session_clone);
                                let panic_connected = Arc::new(AtomicBool::new(true));
                                let worker_connected = Arc::clone(&panic_connected);
                                let worker_result = std::panic::catch_unwind(
                                    std::panic::AssertUnwindSafe(|| {
                                let stream = LifecycleTcpStream::new(
                                    session_stream,
                                    system_id_clone.clone(),
                                    "session_pipeline",
                                    "session_worker_stream",
                                    "SESSION_STREAM_DROP",
                                    Some(Arc::clone(&worker_connected)),
                                    Some(Arc::clone(&is_in_session_clone)),
                                    false,
                                );
                                let system_id = system_id_clone;
                                let backend = backend_clone;
                                let is_in_session = is_in_session_clone;

                                let is_connected = worker_connected;
                                let is_conn_read = Arc::clone(&is_connected);
                                let is_conn_write = Arc::clone(&is_connected);
                                let is_conn_capture = Arc::clone(&is_connected);
                                let is_conn_clip = Arc::clone(&is_connected);
                                let is_conn_audio = Arc::clone(&is_connected);
                                let is_conn_ping = Arc::clone(&is_connected);
                                
                                if let Err(e) = stream.set_nodelay(true) {
                                    eprintln!("[Agent] Warning: Could not set TCP_NODELAY: {}", e);
                                }
                                let heartbeat_shutdown_stream = match stream.try_clone() {
                                    Ok(stream) => Some(stream),
                                    Err(error) => {
                                        crate::session_debug::log(
                                            &system_id,
                                            &format!(
                                                "[PING TRACE] timestamp_ms={} channel=SESSION_END_TO_END stage=SHUTDOWN_CLONE_FAILED error_kind={:?}",
                                                crate::status::now_ms(), error.kind()
                                            ),
                                        );
                                        None
                                    }
                                };

                                // TCP INPUT
                                let read_stream = match stream.try_clone() {
                                    Ok(s) => {
                                        crate::session_debug::lifecycle(
                                            &system_id,
                                            "SESSION_PIPELINE_SOCKET_CLONED",
                                            "session_pipeline",
                                            "session_worker_stream",
                                            Some(is_connected.as_ref()),
                                            Some(is_in_session.as_ref()),
                                            false,
                                            "created input reader TcpStream clone",
                                        );
                                        s
                                    }
                                    Err(e) => {
                                        crate::session_debug::lifecycle(
                                            &system_id,
                                            "SOCKET_READ_ERROR",
                                            "session_pipeline",
                                            "session_worker_stream",
                                            Some(is_connected.as_ref()),
                                            Some(is_in_session.as_ref()),
                                            false,
                                            &format!("TcpStream::try_clone failed: {:?}", e),
                                        );
                                        stage_log(
                                            &system_id,
                                            "STREAM_INITIALIZATION_FAILED",
                                            &format!("device_id={} error=reader_socket_clone_failed: {:?}", system_id, e),
                                        );
                                        mark_session_disconnected(
                                            &system_id,
                                            "session_pipeline",
                                            "session_worker_stream",
                                            is_connected.as_ref(),
                                            is_in_session.as_ref(),
                                            false,
                                            &format!("reader socket clone failed: {:?}", e),
                                        );
                                        return;
                                    }
                            };
                            let _ = read_stream.set_read_timeout(None);
                            let mut read_stream = SessionReadStream {
                                inner: read_stream,
                                identity: system_id.clone(),
                                is_connected: Arc::clone(&is_connected),
                                is_in_session: Arc::clone(&is_in_session),
                                intentional_reconnect: false,
                            };

                            // Bounded Send Buffer: 64KB ensures the OS doesn't hide seconds of latency
                            #[cfg(windows)]
                            {
                                use std::os::windows::io::AsRawSocket;
                                let sock = stream.as_raw_socket();
                                let sndbuf: i32 = 65536;
                                unsafe {
                                    windows_sys::Win32::Networking::WinSock::setsockopt(
                                        sock as usize,
                                        windows_sys::Win32::Networking::WinSock::SOL_SOCKET,
                                        windows_sys::Win32::Networking::WinSock::SO_SNDBUF,
                                        &sndbuf as *const i32 as *const _,
                                        4,
                                    );
                                }
                            }

                            let video_slot = Arc::new(Mutex::new(std::collections::VecDeque::<Vec<u8>>::with_capacity(3)));
                            let video_slot_writer = Arc::clone(&video_slot);
                            let video_slot_capture = Arc::clone(&video_slot);
                            let video_recovery_needed = Arc::new(AtomicBool::new(false));
                            let video_recovery_capture = Arc::clone(&video_recovery_needed);
                            
                            let (out_tx, out_rx) = sync_channel::<Vec<u8>>(4);
                            let write_stream = out_tx.clone();
                            let write_tcp = match stream.try_clone() {
                                Ok(s) => {
                                    crate::session_debug::lifecycle(
                                        &system_id,
                                        "SESSION_PIPELINE_SOCKET_CLONED",
                                        "session_pipeline",
                                        "session_worker_stream",
                                        Some(is_connected.as_ref()),
                                        Some(is_in_session.as_ref()),
                                        false,
                                        "created packet writer TcpStream clone",
                                    );
                                    s
                                }
                                Err(e) => {
                                    crate::session_debug::lifecycle(
                                        &system_id,
                                        "SOCKET_WRITE_ERROR",
                                        "session_pipeline",
                                        "session_worker_stream",
                                        Some(is_connected.as_ref()),
                                        Some(is_in_session.as_ref()),
                                        false,
                                        &format!("TcpStream::try_clone failed: {:?}", e),
                                    );
                                    stage_log(
                                        &system_id,
                                        "STREAM_INITIALIZATION_FAILED",
                                        &format!("device_id={} error=writer_socket_clone_failed: {:?}", system_id, e),
                                    );
                                    mark_session_disconnected(
                                        &system_id,
                                        "session_pipeline",
                                        "session_worker_stream",
                                        is_connected.as_ref(),
                                        is_in_session.as_ref(),
                                        false,
                                        &format!("writer socket clone failed: {:?}", e),
                                    );
                                    return;
                                }
                            };
                            let mut write_tcp = LifecycleTcpStream::new(
                                write_tcp,
                                system_id.clone(),
                                "session_writer",
                                "cloned_session_writer_stream",
                                "SESSION_STREAM_DROP",
                                Some(Arc::clone(&is_connected)),
                                Some(Arc::clone(&is_in_session)),
                                false,
                            );
                            let _ = write_tcp.set_write_timeout(None);

                            let write_connected = Arc::clone(&is_connected);

                            println!("[STREAM STATE] ACTIVE");
                            println!("[VIDEO STATE] ACTIVE");
                            stage_log(
                                &system_id,
                                "STREAM_STARTED",
                                &format!("device_id={} state=ACTIVE", system_id),
                            );
                            crate::session_debug::log(
                                &system_id,
                                "B_STREAM_STATE state=ACTIVE",
                            );
                            crate::session_debug::lifecycle(
                                &system_id,
                                "SESSION_STREAM_STARTED",
                                "session_pipeline",
                                "cloned_session_stream",
                                Some(is_connected.as_ref()),
                                Some(is_in_session.as_ref()),
                                false,
                                "session stream entered ACTIVE state",
                            );
                            let diagnostic_system_id = system_id.clone();
                            let diagnostic_connected = Arc::clone(&is_connected);
                            let diagnostic_in_session = Arc::clone(&is_in_session);
                            let mut session_heartbeat_tracker = crate::status::HeartbeatTracker::default();
                            if let Some(probe_id) = retired_idle_probe_id {
                                session_heartbeat_tracker.remember_stale(probe_id);
                            }
                            let session_heartbeat = Arc::new(Mutex::new(session_heartbeat_tracker));
                            let session_heartbeat_writer = Arc::clone(&session_heartbeat);
                            let session_heartbeat_input = Arc::clone(&session_heartbeat);
                            let session_heartbeat_ping = Arc::clone(&session_heartbeat);

                            let writer_handle = thread::spawn(move || {
                                crate::session_debug::lifecycle(
                                    &diagnostic_system_id,
                                    "SESSION_WRITE_STARTED",
                                    "session_writer",
                                    "cloned_session_writer_stream",
                                    Some(diagnostic_connected.as_ref()),
                                    Some(diagnostic_in_session.as_ref()),
                                    false,
                                    "writer thread entered packet loop",
                                );
                                // Maximum number of bytes written per OS send() call for
                                // TYPE 13 / TYPE 15 video packets.  Keeping each write ≤ the
                                // socket SO_SNDBUF (64 KB) prevents write_all() from blocking
                                // for seconds while the kernel drains a 300-500 KB IDR frame
                                // through a congested WAN link — which was the confirmed root
                                // cause of the relay's ~4 s session-bridge timer firing.
                                //
                                // The byte stream emitted to the relay is UNCHANGED; only the
                                // number of underlying OS write() calls changes.  The receiver
                                // sees:  [TYPE13 header (21 B)] [H264 payload, in order]
                                // exactly as before.
                                //
                                // TYPE 13 header layout (21 bytes, from packet construction):
                                //   [0]      type byte (13)
                                //   [1..5]   width  u32 BE
                                //   [5..9]   height u32 BE
                                //   [9..13]  payload_len u32 BE
                                //   [13..21] captured_at_ms u64 BE
                                //   [21..]   H264 payload
                                const TYPE13_HEADER_LEN: usize = 21; // verified from construction above
                                const MAX_VIDEO_WRITE_CHUNK: usize = 32 * 1024; // 32 KB

                                // Counters captured by the closure below.
                                let mut video_trace_count = 0u64;
                                let mut diagnostic_video_count = 0u64;
                                let mut outbound_chat_message_id = 0u64;
                                let mut outbound_audio_packet_count = 0u64;

                                let mut write_packet = |packet: Vec<u8>| -> bool {
                                    if packet.first() == Some(&13u8) || packet.first() == Some(&15u8) {
                                        video_trace_count += 1;
                                        let is_log_frame = video_trace_count == 1 || video_trace_count % 60 == 0;

                                        if is_log_frame {
                                            let ts = if packet.len() > TYPE13_HEADER_LEN {
                                                u64::from_be_bytes(
                                                    packet[13..21].try_into().unwrap_or([0; 8])
                                                )
                                            } else { 0 };
                                            let now = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis() as u64;
                                            if ts > 0 && now > ts {
                                                println!(
                                                    "[VIDEO TRACE] frames={} send_timestamp={} ageMs={}",
                                                    video_trace_count, now, now - ts
                                                );
                                            }
                                            println!("[RELAY VIDEO SIZE]\ntype={}\nframe={}\nbytes={}", packet.first().copied().unwrap_or(0), video_trace_count, packet.len());
                                        }

                                        // --- Chunked write for large video packets ---
                                        //
                                        // Write the fixed header (21 bytes) first, then the
                                        // H264 payload in MAX_VIDEO_WRITE_CHUNK slices.
                                        // This keeps each OS write call ≤ SO_SNDBUF so
                                        // write_all never blocks for more than a single
                                        // buffer-drain cycle (a few ms), instead of
                                        // stalling 2.5 s on a 400 KB IDR.
                                        //
                                        // The resulting byte stream is byte-for-byte identical
                                        // to a single write_all(&packet).

                                        let header_bytes = packet.len().min(TYPE13_HEADER_LEN);
                                        let payload_bytes = packet.len().saturating_sub(TYPE13_HEADER_LEN);
                                        let chunk_count = (payload_bytes + MAX_VIDEO_WRITE_CHUNK - 1)
                                            .saturating_div(MAX_VIDEO_WRITE_CHUNK)
                                            .max(1);

                                        let write_start = std::time::Instant::now();

                                        // 1. Write the header
                                        if let Err(e) = write_tcp.write_all(&packet[..header_bytes]) {
                                            crate::session_debug::lifecycle(
                                                &diagnostic_system_id,
                                                "SOCKET_WRITE_ERROR",
                                                "session_writer",
                                                "cloned_session_writer_stream",
                                                Some(diagnostic_connected.as_ref()),
                                                Some(diagnostic_in_session.as_ref()),
                                                false,
                                                &format!(
                                                    "write_all failed (TYPE13 header): {:?}; type={} bytes={}",
                                                    e,
                                                    packet.first().copied().unwrap_or(255),
                                                    packet.len()
                                                ),
                                            );
                                            crate::session_debug::lifecycle(
                                                &diagnostic_system_id,
                                                "SESSION_END_REQUESTED",
                                                "session_writer",
                                                "cloned_session_writer_stream",
                                                Some(diagnostic_connected.as_ref()),
                                                Some(diagnostic_in_session.as_ref()),
                                                false,
                                                "session packet write failed",
                                            );
                                            eprintln!(
                                                "[Writer] TCP video header write failed: type={} bytes={} error={:?}; ending session",
                                                packet.first().copied().unwrap_or(255),
                                                packet.len(),
                                                e
                                            );
                                            println!("[VIDEO STATE] WRITE_FAILED");
                                            crate::session_debug::log(
                                                &diagnostic_system_id,
                                                &format!("B_VIDEO_WRITE_FAILED error={}", e),
                                            );
                                            mark_session_disconnected(
                                                &diagnostic_system_id,
                                                "session_writer",
                                                "cloned_session_writer_stream",
                                                diagnostic_connected.as_ref(),
                                                diagnostic_in_session.as_ref(),
                                                false,
                                                &format!("write_all failed (video header): {:?}", e),
                                            );
                                            return false;
                                        }

                                        // 2. Write the H264 payload in chunks
                                        if payload_bytes > 0 {
                                            for chunk in packet[TYPE13_HEADER_LEN..].chunks(MAX_VIDEO_WRITE_CHUNK) {
                                                if let Err(e) = write_tcp.write_all(chunk) {
                                                    crate::session_debug::lifecycle(
                                                        &diagnostic_system_id,
                                                        "SOCKET_WRITE_ERROR",
                                                        "session_writer",
                                                        "cloned_session_writer_stream",
                                                        Some(diagnostic_connected.as_ref()),
                                                        Some(diagnostic_in_session.as_ref()),
                                                        false,
                                                        &format!(
                                                            "write_all failed (TYPE13 payload chunk): {:?}; type={} bytes={}",
                                                            e,
                                                            packet.first().copied().unwrap_or(255),
                                                            packet.len()
                                                        ),
                                                    );
                                                    crate::session_debug::lifecycle(
                                                        &diagnostic_system_id,
                                                        "SESSION_END_REQUESTED",
                                                        "session_writer",
                                                        "cloned_session_writer_stream",
                                                        Some(diagnostic_connected.as_ref()),
                                                        Some(diagnostic_in_session.as_ref()),
                                                        false,
                                                        "session packet write failed",
                                                    );
                                                    eprintln!(
                                                        "[Writer] TCP packet write failed: type={} bytes={} error={:?}; ending session to preserve stream framing",
                                                        packet.first().copied().unwrap_or(255),
                                                        packet.len(),
                                                        e
                                                    );
                                                    println!("[VIDEO STATE] WRITE_FAILED");
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!("B_VIDEO_WRITE_FAILED error={}", e),
                                                    );
                                                    mark_session_disconnected(
                                                        &diagnostic_system_id,
                                                        "session_writer",
                                                        "cloned_session_writer_stream",
                                                        diagnostic_connected.as_ref(),
                                                        diagnostic_in_session.as_ref(),
                                                        false,
                                                        &format!("write_all failed (video payload chunk): {:?}", e),
                                                    );
                                                    return false;
                                                }
                                            }
                                        }

                                        let elapsed_ms = write_start.elapsed().as_millis();

                                        // Log first TYPE 13 and every 60th
                                        if packet.first() == Some(&13u8) {
                                            diagnostic_video_count += 1;
                                            if diagnostic_video_count == 1 {
                                                println!(
                                                    "[VIDEO WRITE] first TYPE13 total_bytes={} payload_bytes={} chunks={} write_ms={}",
                                                    packet.len(),
                                                    payload_bytes,
                                                    chunk_count,
                                                    elapsed_ms
                                                );
                                                crate::session_debug::log(
                                                    &diagnostic_system_id,
                                                    "B_FIRST_TYPE13_SENT",
                                                );
                                            }
                                            if diagnostic_video_count == 1 || diagnostic_video_count % 60 == 0 {
                                                if diagnostic_video_count > 1 {
                                                    println!(
                                                        "[VIDEO WRITE] TYPE13 count={} total_bytes={} payload_bytes={} chunks={} write_ms={}",
                                                        diagnostic_video_count,
                                                        packet.len(),
                                                        payload_bytes,
                                                        chunk_count,
                                                        elapsed_ms
                                                    );
                                                }
                                                crate::session_debug::log(
                                                    &diagnostic_system_id,
                                                    &format!(
                                                        "B_TYPE13_SENT count={}",
                                                        diagnostic_video_count
                                                    ),
                                                );
                                            }
                                        }

                                        true
                                    } else {
                                        // Non-video packets (control, audio, clipboard, file, ping):
                                        // use the original single write_all — they are small and
                                        // latency-sensitive; chunking adds no benefit.
                                        let packet_type = packet.first().copied().unwrap_or(255);
                                        let direction_byte = packet.get(1).copied().unwrap_or(255);
                                        let audio_trace_count = if packet_type == 17 {
                                            outbound_audio_packet_count =
                                                outbound_audio_packet_count.saturating_add(1);
                                            (outbound_audio_packet_count == 1
                                                || outbound_audio_packet_count % 100 == 0)
                                                .then_some(outbound_audio_packet_count)
                                        } else {
                                            None
                                        };
                                        let payload_bytes = if packet_type == 16 && packet.len() >= 4 {
                                            u16::from_be_bytes([packet[2], packet[3]]) as usize
                                        } else {
                                            0
                                        };
                                        let chat_message_id = if packet_type == 16 {
                                            outbound_chat_message_id = outbound_chat_message_id.saturating_add(1);
                                            Some(format!("CHAT-B-TO-A-{outbound_chat_message_id:08}"))
                                        } else {
                                            None
                                        };
                                        let packet_trace_id = matches!(packet_type, 16 | 20..=27)
                                            .then(|| crate::session_debug::next_packet_trace_id(packet_type));
                                        let heartbeat_id = if packet_type == 14 && packet.len() == 9 {
                                            Some(u64::from_be_bytes(packet[1..9].try_into().unwrap()))
                                        } else {
                                            None
                                        };
                                        let transfer_id = if (20..=27).contains(&packet_type) && packet.len() >= 9 {
                                            u64::from_be_bytes(packet[1..9].try_into().unwrap()).to_string()
                                        } else {
                                            "n/a".to_string()
                                        };
                                        let write_started = Instant::now();
                                        if let Some(count) = audio_trace_count {
                                            eprintln!(
                                                "[AUDIO TX] stage=socket_write_begin packet={} direction=B_TO_A session_id=not_carried_by_protocol local_device_id={} packet_bytes={}",
                                                count,
                                                diagnostic_system_id,
                                                packet.len()
                                            );
                                        }
                                        if let Some(probe_id) = heartbeat_id {
                                            crate::session_debug::log(
                                                &diagnostic_system_id,
                                                &format!(
                                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=SOCKET_WRITE_BEGIN type=14 direction=B_TO_A packet_bytes=9",
                                                    crate::status::now_ms(), probe_id
                                                ),
                                            );
                                            if let Ok(mut tracker) = session_heartbeat_writer.lock() {
                                                tracker.mark_sent(probe_id, write_started);
                                            }
                                        }
                                        if let Some(trace_id) = packet_trace_id.as_deref() {
                                            crate::session_debug::log(
                                                &diagnostic_system_id,
                                                &format!(
                                                    "[{} TRACE] trace_id={} side=B direction=B_TO_A stage=SOCKET_WRITE_BEGIN type={} transfer_id={} length={}",
                                                    if packet_type == 16 { "CHAT" } else { "FILE" },
                                                    trace_id,
                                                    packet_type,
                                                    transfer_id,
                                                    packet.len()
                                                ),
                                            );
                                        }
                                        if packet_type == 16 || packet_type == 99 {
                                            crate::session_debug::log(
                                                &diagnostic_system_id,
                                                &format!(
                                                    "[TYPE{} TRACE] component=HOST direction=SEND source=session_writer direction_byte={} payload_bytes={} packet_bytes={} message_id={}",
                                                    packet_type,
                                                    direction_byte,
                                                    payload_bytes,
                                                    packet.len(),
                                                    chat_message_id.as_deref().unwrap_or("n/a")
                                                ),
                                            );
                                        }
                                        if let Some(message_id) = chat_message_id.as_deref() {
                                            crate::session_debug::log(
                                                &diagnostic_system_id,
                                                &format!(
                                                    "[CHAT TRACE] message_id={} session_id={} side=B direction=B_TO_A stage=SOCKET_WRITE_BEGIN type=16 payload_bytes={} packet_bytes={}",
                                                    message_id, diagnostic_system_id, payload_bytes, packet.len()
                                                ),
                                            );
                                        }
                                        match write_tcp.write_all(&packet) {
                                            Ok(_) => {
                                                if let Some(count) = audio_trace_count {
                                                    eprintln!(
                                                        "[AUDIO TX] stage=socket_write_complete packet={} direction=B_TO_A session_id=not_carried_by_protocol local_device_id={} packet_bytes={} write_ms={}",
                                                        count,
                                                        diagnostic_system_id,
                                                        packet.len(),
                                                        write_started.elapsed().as_millis()
                                                    );
                                                }
                                                if let Some(message_id) = chat_message_id.as_deref() {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[CHAT TRACE] message_id={} session_id={} side=B direction=B_TO_A stage=SOCKET_WRITE_COMPLETE type=16 payload_bytes={} packet_bytes={} write_ms={}",
                                                            message_id,
                                                            diagnostic_system_id,
                                                            payload_bytes,
                                                            packet.len(),
                                                            write_started.elapsed().as_millis()
                                                        ),
                                                    );
                                                }
                                                if let Some(probe_id) = heartbeat_id {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=SOCKET_WRITE_COMPLETE type=14 direction=B_TO_A write_ms={}",
                                                            crate::status::now_ms(),
                                                            probe_id,
                                                            write_started.elapsed().as_millis()
                                                        ),
                                                    );
                                                }
                                                if let Some(trace_id) = packet_trace_id.as_deref() {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[{} TRACE] trace_id={} side=B direction=B_TO_A stage=SOCKET_WRITE_COMPLETE type={} transfer_id={} bytes_written={}",
                                                            if packet_type == 16 { "CHAT" } else { "FILE" },
                                                            trace_id,
                                                            packet_type,
                                                            transfer_id,
                                                            packet.len()
                                                        ),
                                                    );
                                                }
                                                if packet_type == 16 || packet_type == 99 {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[TYPE{} TRACE] component=HOST direction=SEND_COMPLETE source=session_writer direction_byte={} payload_bytes={} packet_bytes={}",
                                                            packet_type,
                                                            direction_byte,
                                                            payload_bytes,
                                                            packet.len()
                                                        ),
                                                    );
                                                }
                                                if packet_type == 99 {
                                                    mark_session_disconnected(
                                                        &diagnostic_system_id,
                                                        "session_writer",
                                                        "cloned_session_writer_stream",
                                                        diagnostic_connected.as_ref(),
                                                        diagnostic_in_session.as_ref(),
                                                        false,
                                                        "host sent session disconnect Type 99",
                                                    );
                                                    return false;
                                                }
                                                true
                                            }
                                            Err(e) => {
                                                if packet_type == 17 {
                                                    eprintln!(
                                                        "[AUDIO TX] stage=socket_write_failed direction=B_TO_A session_id=not_carried_by_protocol local_device_id={} packet_bytes={} error_kind={:?}",
                                                        diagnostic_system_id,
                                                        packet.len(),
                                                        e.kind()
                                                    );
                                                }
                                                if let Some(message_id) = chat_message_id.as_deref() {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[CHAT TRACE] message_id={} session_id={} side=B direction=B_TO_A stage=SOCKET_WRITE_ERROR type=16 payload_bytes={} packet_bytes={} error_kind={:?}",
                                                            message_id,
                                                            diagnostic_system_id,
                                                            payload_bytes,
                                                            packet.len(),
                                                            e.kind()
                                                        ),
                                                    );
                                                }
                                                if let Some(probe_id) = heartbeat_id {
                                                    if let Ok(mut tracker) = session_heartbeat_writer.lock() {
                                                        tracker.cancel(probe_id);
                                                    }
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=SOCKET_WRITE_ERROR error_kind={:?}",
                                                            crate::status::now_ms(), probe_id, e.kind()
                                                        ),
                                                    );
                                                }
                                                if let Some(trace_id) = packet_trace_id.as_deref() {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[{} TRACE] trace_id={} side=B direction=B_TO_A stage=SOCKET_WRITE_ERROR type={} transfer_id={} length={} os_error={:?}",
                                                            if packet_type == 16 { "CHAT" } else { "FILE" },
                                                            trace_id,
                                                            packet_type,
                                                            transfer_id,
                                                            packet.len(),
                                                            e
                                                        ),
                                                    );
                                                }
                                                if packet_type == 16 || packet_type == 99 {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "[TYPE{} TRACE] component=HOST direction=SEND_FAILED source=session_writer direction_byte={} payload_bytes={} packet_bytes={} reason={:?}",
                                                            packet_type,
                                                            direction_byte,
                                                            payload_bytes,
                                                            packet.len(),
                                                            e
                                                        ),
                                                    );
                                                }
                                                crate::session_debug::lifecycle(
                                                    &diagnostic_system_id,
                                                    "SOCKET_WRITE_ERROR",
                                                    "session_writer",
                                                    "cloned_session_writer_stream",
                                                    Some(diagnostic_connected.as_ref()),
                                                    Some(diagnostic_in_session.as_ref()),
                                                    false,
                                                    &format!(
                                                        "write_all failed: {:?}; type={} bytes={}",
                                                        e,
                                                        packet.first().copied().unwrap_or(255),
                                                        packet.len()
                                                    ),
                                                );
                                                crate::session_debug::lifecycle(
                                                    &diagnostic_system_id,
                                                    "SESSION_END_REQUESTED",
                                                    "session_writer",
                                                    "cloned_session_writer_stream",
                                                    Some(diagnostic_connected.as_ref()),
                                                    Some(diagnostic_in_session.as_ref()),
                                                    false,
                                                    "session packet write failed",
                                                );
                                                eprintln!(
                                                    "[Writer] TCP packet write failed: type={} bytes={} error={:?}; ending session to preserve stream framing",
                                                    packet.first().copied().unwrap_or(255),
                                                    packet.len(),
                                                    e
                                                );
                                                println!("[VIDEO STATE] WRITE_FAILED");
                                                crate::session_debug::log(
                                                    &diagnostic_system_id,
                                                    &format!("B_VIDEO_WRITE_FAILED error={}", e),
                                                );
                                                mark_session_disconnected(
                                                    &diagnostic_system_id,
                                                    "session_writer",
                                                    "cloned_session_writer_stream",
                                                    diagnostic_connected.as_ref(),
                                                    diagnostic_in_session.as_ref(),
                                                    false,
                                                    &format!("write_all failed: {:?}", e),
                                                );
                                                false
                                            }
                                        }
                                    }
                                };



                                let mut sent_video_count = 0u64;
                                let mut last_video_tx_log = Instant::now();

                                while write_connected.load(Ordering::Acquire) {
                                    let mut sent_control = false;
                                    let mut error = false;

                                    // Keep control traffic responsive without letting a sustained
                                    // file transfer starve the live video queue.
                                    if let Ok(packet) = out_rx.try_recv() {
                                        sent_control = true;
                                        if !write_packet(packet) {
                                            error = true;
                                        }
                                    }
                                    if error { break; }

                                    // Then send ONE video packet if available
                                    let video_packet = video_slot_writer
                                        .lock()
                                        .ok()
                                        .and_then(|mut q| q.pop_front());
                                    
                                    if let Some(packet) = video_packet {
                                        sent_video_count += 1;
                                        if sent_video_count == 1 || last_video_tx_log.elapsed() >= Duration::from_secs(2) {
                                            println!(
                                                "[VIDEO TX] packet_count={} bytes={}",
                                                sent_video_count, packet.len()
                                            );
                                            last_video_tx_log = Instant::now();
                                        }
                                        if !write_packet(packet) {
                                            break;
                                        }
                                    } else if !sent_control {
                                        // Wait a bit to avoid busy loop if both queues are empty
                                        match out_rx.recv_timeout(Duration::from_millis(5)) {
                                            Ok(packet) => {
                                                if !write_packet(packet) {
                                                    break;
                                                }
                                            }
                                            Err(_) => {}
                                        }
                                    }
                                }
                                crate::session_debug::lifecycle(
                                    &diagnostic_system_id,
                                    "SESSION_WRITE_RETURNED",
                                    "session_writer",
                                    "cloned_session_writer_stream",
                                    Some(diagnostic_connected.as_ref()),
                                    Some(diagnostic_in_session.as_ref()),
                                    false,
                                    "writer loop exited",
                                );
                                crate::session_debug::lifecycle(
                                    &diagnostic_system_id,
                                    "THREAD_EXIT",
                                    "session_writer",
                                    "cloned_session_writer_stream",
                                    Some(diagnostic_connected.as_ref()),
                                    Some(diagnostic_in_session.as_ref()),
                                    false,
                                    "writer thread returning",
                                );
                            });

                            let write_stream_clip = write_stream.clone();
                            let write_stream_audio = write_stream.clone();
                            let write_stream_ping = write_stream.clone();
                            let write_stream_file = write_stream.clone();
                            let file_session_id = system_id.clone();

                            let is_conn_file = Arc::clone(&is_connected);
                            thread::spawn(move || {
                                let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\temp".to_string());
                                let trigger_file = std::path::Path::new(&local_app_data).join("DeskStream").join("send_file.txt");
                                while is_conn_file.load(Ordering::SeqCst) {
                                    if trigger_file.exists() {
                                        if let Ok(file_path_str) = std::fs::read_to_string(&trigger_file) {
                                            let _ = std::fs::remove_file(&trigger_file);
                                            for selected_path in file_path_str.lines().map(str::trim).filter(|path| !path.is_empty()) {
                                                let selected_path = PathBuf::from(selected_path);
                                                let transfers = if selected_path.is_dir() {
                                                    collect_transfer_files(&selected_path)
                                                        .map_err(|error| error.to_string())
                                                } else if selected_path.is_file() {
                                                    let name = selected_path
                                                        .file_name()
                                                        .map(|name| name.to_string_lossy().into_owned())
                                                        .ok_or_else(|| "Selected path has no file name".to_string());
                                                    name.map(|name| vec![(selected_path.clone(), name)])
                                                } else {
                                                    Err("Selected path is not a regular file or folder".to_string())
                                                };
                                                match transfers {
                                                    Ok(transfers) if transfers.is_empty() => {
                                                        eprintln!("[FILE TX ERROR] Selected folder contains no regular files");
                                                    }
                                                    Ok(transfers) => {
                                                        for (source_path, relative_name) in transfers {
                                                            if let Err(error) = send_session_file(
                                                                &write_stream_file,
                                                                &file_session_id,
                                                                &source_path,
                                                                &relative_name,
                                                            ) {
                                                                eprintln!("[FILE TX ERROR] {error}");
                                                            }
                                                        }
                                                    }
                                                    Err(error) => {
                                                        eprintln!("[FILE TX ERROR] Could not enumerate selected transfer source: {error}");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                }
                            });

                            let current_rtt_ms = Arc::new(AtomicU64::new(10));
                            let current_rtt_in = Arc::clone(&current_rtt_ms);

                            let active_screen_idx = Arc::new(AtomicUsize::new(0));
                            let active_idx_input = Arc::clone(&active_screen_idx);
                            let active_idx_capture = Arc::clone(&active_screen_idx);

                            let shared_frame = Arc::new((Mutex::new(None::<FrameData>), Condvar::new()));
                            let shared_frame_cap = Arc::clone(&shared_frame);

                            // FIXED: shared clipboard state + remote-apply flag to avoid echo loops.
                            let last_clipboard_text = Arc::new(Mutex::new(String::new()));
                            let remote_clipboard_apply = Arc::new(AtomicBool::new(false));
                            let last_clip_recv = Arc::clone(&last_clipboard_text);
                            let last_clip_send = Arc::clone(&last_clipboard_text);
                            let remote_clip_recv = Arc::clone(&remote_clipboard_apply);
                            let remote_clip_send = Arc::clone(&remote_clipboard_apply);
                            if let Err(error) = crate::status::clear_session_peer_system_id() {
                                eprintln!("[SESSION UI] Failed to clear stale peer identity: {}", error);
                            }
                            let session_ui_ready =
                                match crate::status::set_session_writer(Some(write_stream.clone())) {
                                    Ok(()) => true,
                                    Err(error) => {
                                        eprintln!("[SESSION UI] Failed to initialize session bridge: {}", error);
                                        false
                                    }
                                };
                            // Publish CONNECTED only after the real session writer is ready for
                            // chat, microphone, reverse-control, and disconnect commands.
                            if session_ui_ready {
                                crate::status::set_session_active(true);
                            }
                            let write_stream_input = write_stream.clone();
                            let input_identity = system_id.clone();
                            let input_connected = Arc::clone(&is_connected);
                            let input_in_session = Arc::clone(&is_in_session);
                            let input_heartbeat = session_heartbeat_input;

                            // INPUT THREAD
                            let input_handle = thread::spawn(move || {
                                crate::session_debug::lifecycle(
                                    &input_identity,
                                    "SESSION_READ_STARTED",
                                    "session_input_reader",
                                    "cloned_session_reader_stream",
                                    Some(input_connected.as_ref()),
                                    Some(input_in_session.as_ref()),
                                    false,
                                    "reader thread entered packet loop",
                                );
                                let (_audio_output_stream, audio_playback_queue, audio_output_rate) =
                                    setup_session_audio_playback();
                                // FIXED: thread-safe arboard clipboard handle for remote apply.
                                let mut clip = Clipboard::new().ok();
                                let mut current_file: Option<File> = None;
                                let mut current_file_path: Option<PathBuf> = None;
                                let mut current_transfer_id: Option<u64> = None;
                                let mut current_file_chunk_index = 0u32;
                                let mut current_filename = String::new();
                                let mut total_file_size: u64 = 0;
                                let mut received_bytes: u64 = 0;
                                let mut file_hasher = Sha256::new();
                                let mut inbound_chat_message_id = 0u64;
                                let mut inbound_audio_packet_count = 0u64;
                                // FIXED: keyboard modifier latch (Ctrl/Shift/Alt/Win).
                                let mut mod_ctrl = false;
                                let mut mod_shift = false;
                                let mut mod_alt = false;
                                let mut mod_win = false;
                                let user_profile = env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
                                let drop_dir = PathBuf::from(user_profile).join("Downloads").join("DeskStream");
                                let canonical_drop_dir = fs::create_dir_all(&drop_dir)
                                    .and_then(|_| drop_dir.canonicalize());
                                if let Err(error) = &canonical_drop_dir {
                                    eprintln!("[FILE RX] Failed to prepare download folder: {}", error);
                                }
                                let send_file_error = |transfer_id: u64, message: &str| {
                                    let message = message.as_bytes();
                                    let mut packet = vec![24u8];
                                    packet.extend_from_slice(&transfer_id.to_be_bytes());
                                    packet.extend_from_slice(&[0, 0]);
                                    packet.extend_from_slice(&(message.len() as u16).to_be_bytes());
                                    packet.extend_from_slice(message);
                                    if let Err(error) = write_stream_input.send(packet) {
                                        eprintln!("[FILE RX ERROR] Failed to send transfer error: {}", error);
                                    }
                                };

                                while is_conn_read.load(Ordering::SeqCst) {
                                    let mut pkt_type_buf = [0u8; 1];
                                    if read_stream.read_exact(&mut pkt_type_buf).is_err() {
                                        mark_session_disconnected(
                                            &input_identity,
                                            "session_input_reader",
                                            "cloned_session_reader_stream",
                                            input_connected.as_ref(),
                                            input_in_session.as_ref(),
                                            false,
                                            "failed reading packet type",
                                        );
                                        break;
                                    }
                                    let packet_trace_id = if matches!(pkt_type_buf[0], 16 | 20..=27) {
                                        let trace_id = crate::session_debug::next_packet_trace_id(pkt_type_buf[0]);
                                        crate::session_debug::log(
                                            &input_identity,
                                            &format!(
                                                "[{} TRACE] trace_id={} side=B direction=A_TO_B stage=SOCKET_READ_BEGIN type={}",
                                                if pkt_type_buf[0] == 16 { "CHAT" } else { "FILE" },
                                                trace_id,
                                                pkt_type_buf[0]
                                            ),
                                        );
                                        Some(trace_id)
                                    } else {
                                        None
                                    };

                                    match pkt_type_buf[0] {
                                        0..=10 => {
                                            let mut data = [0u8; 8];
                                            if read_stream.read_exact(&mut data).is_err() {
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    "failed reading control packet payload",
                                                );
                                                break;
                                            }
                                            crate::session_debug::lifecycle(
                                                &input_identity,
                                                "CONTROL_RECEIVED",
                                                "session_input_reader",
                                                "cloned_session_reader_stream",
                                                Some(input_connected.as_ref()),
                                                Some(input_in_session.as_ref()),
                                                false,
                                                &format!("control_type={} payload_bytes=8", pkt_type_buf[0]),
                                            );
                                            attach_thread_to_input_desktop();
                                            let event_type = pkt_type_buf[0];
                                            
                                            if event_type < 10 && !crate::status::session_perm_control_input() {
                                                continue;
                                            }

                                            if event_type == 10 {
                                                active_idx_input.store(data[0] as usize, Ordering::SeqCst);
                                                continue;
                                            }
                                            if event_type == 9 {
                                                let scroll_y = i16::from_be_bytes(data[2..4].try_into().unwrap());
                                                send_native_mouse_wheel(scroll_y);
                                                continue;
                                            }
                                            // FIXED: track modifiers and inject key events without hot-loop println.
                                            if event_type == 5 || event_type == 6 {
                                                let key_code = u32::from_be_bytes(data[0..4].try_into().unwrap());
                                                let is_up = event_type == 6;
                                                match key_code {
                                                    16 | 160 | 161 => mod_shift = !is_up,
                                                    17 | 162 | 163 => mod_ctrl = !is_up,
                                                    18 | 164 | 165 => mod_alt = !is_up,
                                                    91 | 92 => mod_win = !is_up,
                                                    _ => {}
                                                }
                                                let _ = (mod_ctrl, mod_shift, mod_alt, mod_win);
                                                send_native_key(key_code, is_up);
                                                continue;
                                            }

                                            let norm_x = u16::from_be_bytes(data[0..2].try_into().unwrap());
                                            let norm_y = u16::from_be_bytes(data[2..4].try_into().unwrap());

                                            #[cfg(windows)]
                                            let (sw, sh) = unsafe {
                                                (
                                                    windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) as f32,
                                                    windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) as f32,
                                                )
                                            };
                                            #[cfg(not(windows))]
                                            let (sw, sh) = (1920.0f32, 1080.0f32);

                                            let target_x = ((norm_x as f32 / 65535.0) * (sw - 1.0)).round() as i32;
                                            let target_y = ((norm_y as f32 / 65535.0) * (sh - 1.0)).round() as i32;

                                            // FIXED: mouse inject without hot-loop println noise.
                                            #[cfg(windows)]
                                            {
                                                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                                                    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
                                                    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
                                                };

                                                match event_type {
                                                    0 => set_native_cursor_pos(target_x, target_y),
                                                    1 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_LEFTDOWN);
                                                    }
                                                    2 => send_native_mouse_click(MOUSEEVENTF_LEFTUP),
                                                    3 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_RIGHTDOWN);
                                                    }
                                                    4 => send_native_mouse_click(MOUSEEVENTF_RIGHTUP),
                                                    7 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_MIDDLEDOWN);
                                                    }
                                                    8 => send_native_mouse_click(MOUSEEVENTF_MIDDLEUP),
                                                    _ => {}
                                                }
                                            }
                                        }
                                        11 => {
                                            let mut payload = [0u8; 4];
                                            if read_stream.read_exact(&mut payload).is_err() {
                                                break;
                                            }
                                            let is_mobile = u32::from_be_bytes(payload) == 1;
                                            println!("[SESSION] Viewer connected (is_mobile={})", is_mobile);
                                        }

                                        12 => {
                                            let mut len_buf = [0u8; 4];
                                            if let Err(error) = read_stream.read_exact(&mut len_buf) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    12,
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let len = u32::from_be_bytes(len_buf) as usize;
                                            if len > MAX_CLIPBOARD_PACKET_BYTES {
                                                if len <= MAX_RECOVERABLE_PACKET_DISCARD_BYTES {
                                                    if let Err(error) = discard_packet_bytes(&mut read_stream, len) {
                                                        mark_session_packet_read_failed(
                                                            &input_identity,
                                                            12,
                                                            "oversized_payload",
                                                            &error,
                                                            input_connected.as_ref(),
                                                            input_in_session.as_ref(),
                                                        );
                                                        break;
                                                    }
                                                    crate::session_debug::log(
                                                        &input_identity,
                                                        &format!(
                                                            "[CLIPBOARD RX TRACE] packet_type=12 payload_bytes={} max_payload_bytes={} result=discarded reason=oversized",
                                                            len, MAX_CLIPBOARD_PACKET_BYTES
                                                        ),
                                                    );
                                                    continue;
                                                }
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    &format!(
                                                        "TYPE12 clipboard length {} exceeds the safe discard limit {}",
                                                        len, MAX_RECOVERABLE_PACKET_DISCARD_BYTES
                                                    ),
                                                );
                                                break;
                                            }
                                            let mut text_buf = vec![0u8; len];
                                            if let Err(error) = read_stream.read_exact(&mut text_buf) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    12,
                                                    "payload",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            // FIXED: only apply remote clipboard when different; mark remote-apply flag.
                                            if let Ok(text) = String::from_utf8(text_buf) {
                                                if text.len() > MAX_CLIPBOARD_PACKET_BYTES {
                                                    continue;
                                                }
                                                let mut should_apply = false;
                                                if let Ok(mut guard) = last_clip_recv.lock() {
                                                    if *guard != text {
                                                        *guard = text.clone();
                                                        should_apply = true;
                                                    }
                                                }
                                                if should_apply {
                                                    remote_clip_recv.store(true, Ordering::SeqCst);
                                                    if let Some(ref mut c) = clip {
                                                        let already_matches = c
                                                            .get_text()
                                                            .map(|current| current == text)
                                                            .unwrap_or(false);
                                                        if !already_matches {
                                                            if let Err(error) = c.set_text(&text) {
                                                                eprintln!(
                                                                    "[CLIPBOARD RX] Failed to apply remote text: {error}"
                                                                );
                                                            }
                                                        }
                                                    } else {
                                                        eprintln!(
                                                            "[CLIPBOARD RX] Clipboard handle unavailable; remote text was not applied"
                                                        );
                                                    }
                                                    remote_clip_recv.store(false, Ordering::SeqCst);
                                                }
                                            }
                                        }

                                        14 => {
                                            let mut time_buf = [0u8; 8];
                                            if let Err(error) = read_stream.read_exact(&mut time_buf) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    14,
                                                    "heartbeat_timestamp",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let probe_id = u64::from_be_bytes(time_buf);
                                            let received_at = Instant::now();
                                            let outcome = input_heartbeat
                                                .lock()
                                                .map(|mut tracker| tracker.receive(probe_id, received_at))
                                                .unwrap_or(crate::status::HeartbeatReceive::Stale);
                                            match outcome {
                                                crate::status::HeartbeatReceive::Matched { rtt_ms } => {
                                                    current_rtt_in.store(rtt_ms, Ordering::SeqCst);
                                                    crate::session_debug::log(
                                                        &input_identity,
                                                        &format!(
                                                            "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=RESPONSE_RECEIVED type=14 direction=A_TO_B",
                                                            crate::status::now_ms(), probe_id
                                                        ),
                                                    );
                                                    crate::session_debug::log(
                                                        &input_identity,
                                                        &format!(
                                                            "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=RTT_CALCULATED rtt_ms={}",
                                                            crate::status::now_ms(), probe_id, rtt_ms
                                                        ),
                                                    );
                                                    log_health_transition(
                                                        &input_identity,
                                                        "SESSION_END_TO_END",
                                                        crate::status::record_heartbeat_success(true, rtt_ms),
                                                    );
                                                }
                                                crate::status::HeartbeatReceive::Stale => {
                                                    crate::session_debug::log(
                                                        &input_identity,
                                                        &format!(
                                                            "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=STALE_RESPONSE",
                                                            crate::status::now_ms(), probe_id
                                                        ),
                                                    );
                                                }
                                                crate::status::HeartbeatReceive::PeerRequest => {
                                                    let mut echo = Vec::with_capacity(9);
                                                    echo.push(14);
                                                    echo.extend_from_slice(&time_buf);
                                                    if write_stream_input.send(echo).is_ok() {
                                                        crate::session_debug::log(
                                                            &input_identity,
                                                            &format!(
                                                                "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=ECHO_QUEUED type=14 direction=A_TO_B packet_bytes=9",
                                                                crate::status::now_ms(), probe_id
                                                            ),
                                                        );
                                                    } else {
                                                        crate::session_debug::log(
                                                            &input_identity,
                                                            &format!(
                                                                "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=ECHO_QUEUE_ERROR",
                                                                crate::status::now_ms(), probe_id
                                                            ),
                                                        );
                                                    }
                                                }
                                            }
                                        }

                                        16 => {
                                            let mut meta = [0u8; 3];
                                            if let Err(error) = read_stream.read_exact(&mut meta) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    16,
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let payload_size = u16::from_be_bytes([meta[1], meta[2]]) as usize;
                                            let msg_bytes = match read_packet_payload(&mut read_stream, payload_size) {
                                                Ok(payload) => payload,
                                                Err(error) => {
                                                    mark_session_packet_read_failed(
                                                        &input_identity,
                                                        16,
                                                        "payload",
                                                        &error,
                                                        input_connected.as_ref(),
                                                        input_in_session.as_ref(),
                                                    );
                                                    break;
                                                }
                                            };
                                            inbound_chat_message_id = inbound_chat_message_id.saturating_add(1);
                                            let message_id = format!("CHAT-A-TO-B-{inbound_chat_message_id:08}");
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[CHAT TRACE] trace_id={} message_id={} session_id={} side=B direction=A_TO_B stage=PARSER_ACCEPTED type=16 component=HOST direction_byte={} payload_bytes={} packet_bytes={} result=framing_valid",
                                                    packet_trace_id.as_deref().unwrap_or("missing"),
                                                    message_id,
                                                    input_identity,
                                                    meta[0],
                                                    payload_size,
                                                    4 + payload_size
                                                ),
                                            );
                                            if meta[0] != 0 {
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[TYPE16 TRACE] component=HOST direction=RECV_REJECTED source=session_input_reader direction_byte={} expected_direction_byte=0 payload_bytes={} reason=unexpected_direction",
                                                        meta[0], payload_size
                                                    ),
                                                );
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[CHAT TRACE] message_id={} session_id={} side=B direction=A_TO_B stage=RECEIVE_REJECTED type=16 reason=unexpected_direction",
                                                        message_id, input_identity
                                                    ),
                                                );
                                                continue;
                                            }
                                            if payload_size == 0 {
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    "[TYPE16 TRACE] component=HOST direction=RECV_REJECTED source=session_input_reader payload_bytes=0 reason=empty_message",
                                                );
                                                continue;
                                            }
                                            println!("[B CHAT] session_id={} message_id={} receive type=16 direction={} payload_size={} total_size={}", input_identity, message_id, meta[0], payload_size, 4 + payload_size);
                                            match String::from_utf8(msg_bytes) {
                                                Ok(text) => {
                                                    match crate::status::push_session_chat(text, false) {
                                                        Ok(message) => {
                                                            crate::session_debug::log(
                                                                &input_identity,
                                                                &format!(
                                                                    "[CHAT TRACE] message_id={} session_id={} side=B direction=A_TO_B stage=UI_PUBLISHED type=16 native_message_id={} payload_bytes={} result=success",
                                                                    message_id, input_identity, message.id, payload_size
                                                                ),
                                                            );
                                                        }
                                                        Err(error) => {
                                                            crate::session_debug::log(
                                                                &input_identity,
                                                                &format!(
                                                                    "[CHAT TRACE] message_id={} session_id={} side=B direction=A_TO_B stage=UI_PUBLISH_FAILED type=16 payload_bytes={} error={}",
                                                                    message_id, input_identity, payload_size, error
                                                                ),
                                                            );
                                                            eprintln!("[CHAT UI] Failed to retain received message: {}", error);
                                                        }
                                                    }
                                                }
                                                Err(error) => {
                                                    crate::session_debug::log(
                                                        &input_identity,
                                                        &format!(
                                                            "[CHAT TRACE] message_id={} session_id={} side=B direction=A_TO_B stage=PARSE_FAILED type=16 payload_bytes={} valid_up_to={} error_len={:?}",
                                                            message_id,
                                                            input_identity,
                                                            payload_size,
                                                            error.utf8_error().valid_up_to(),
                                                            error.utf8_error().error_len()
                                                        ),
                                                    );
                                                    eprintln!(
                                                        "[CHAT RX UTF8 ERROR] type=16 payload_len={} valid_up_to={} error_len={:?}",
                                                        payload_size,
                                                        error.utf8_error().valid_up_to(),
                                                        error.utf8_error().error_len()
                                                    );
                                                }
                                            }
                                        }

                                        18 => {
                                            let mut state = [0u8; 1];
                                            if read_stream.read_exact(&mut state).is_err() {
                                                break;
                                            }
                                            if state[0] > 1 {
                                                eprintln!("[CHAT TYPING RX] Invalid state byte {}", state[0]);
                                                continue;
                                            }
                                            crate::status::set_session_remote_typing(state[0] == 1);
                                        }

                                        30 => {
                                            let accepted = crate::status::set_session_reverse_request_pending();
                                            let peer_system_id = match crate::status::session_peer_system_id() {
                                                Ok(Some(peer_id)) => peer_id,
                                                Ok(None) => "unknown".to_string(),
                                                Err(error) => {
                                                    eprintln!("[REVERSE] Could not read peer identity for TYPE 30: {error}");
                                                    "unavailable".to_string()
                                                }
                                            };
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[REVERSE TRACE] role=HOST direction=RECV message_type=30 request_id=not_carried_by_wire active_session_id=unavailable active_session_host_id={} peer_system_id={} result={}",
                                                    input_identity,
                                                    peer_system_id,
                                                    if accepted { "pending" } else { "duplicate_ignored" }
                                                ),
                                            );
                                            println!(
                                                "[REVERSE] Received TYPE 30 request during active session; pending={accepted}."
                                            );
                                        }

                                        31 => {
                                            let mut decision = [0u8; 1];
                                            if read_stream.read_exact(&mut decision).is_err() {
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    "failed reading Type 31 decision",
                                                );
                                                break;
                                            }
                                            if decision[0] > 1 {
                                                eprintln!("[REVERSE] Invalid TYPE 31 decision={}", decision[0]);
                                                continue;
                                            }
                                            let request_id = match crate::status::session_reverse_request_id() {
                                                Ok(Some(request_id)) => request_id,
                                                Ok(None) => "none".to_string(),
                                                Err(error) => {
                                                    eprintln!("[REVERSE] Could not read pending request ID: {error}");
                                                    "unavailable".to_string()
                                                }
                                            };
                                            let peer_system_id = match crate::status::session_peer_system_id() {
                                                Ok(Some(peer_id)) => peer_id,
                                                Ok(None) => "unknown".to_string(),
                                                Err(error) => {
                                                    eprintln!("[REVERSE] Could not read peer identity for TYPE 31: {error}");
                                                    "unavailable".to_string()
                                                }
                                            };
                                            if crate::status::set_session_reverse_decision(decision[0] == 1) {
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[REVERSE TRACE] role=HOST direction=RECV message_type=31 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} decision={} result=accepted_for_pending_request",
                                                        request_id, input_identity, peer_system_id, decision[0]
                                                    ),
                                                );
                                                println!("[REVERSE] Received TYPE 31 decision={} for the outstanding request; active roles are unchanged.", decision[0]);
                                            } else {
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[REVERSE TRACE] role=HOST direction=RECV message_type=31 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} decision={} result=unsolicited_or_duplicate_ignored",
                                                        request_id, input_identity, peer_system_id, decision[0]
                                                    ),
                                                );
                                                eprintln!("[REVERSE] Ignored unsolicited or duplicate TYPE 31 decision={}.", decision[0]);
                                            }
                                        }

                                        32 => {
                                            let mut peer_id = [0u8; 9];
                                            if read_stream.read_exact(&mut peer_id).is_err() {
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    "failed reading Type 32 peer identity",
                                                );
                                                break;
                                            }
                                            match String::from_utf8(peer_id.to_vec()) {
                                                Ok(peer_id) => {
                                                    if let Err(error) = crate::status::set_session_peer_system_id(peer_id.clone()) {
                                                        eprintln!("[REVERSE] Ignoring invalid peer identity: {}", error);
                                                    } else {
                                                        println!("[REVERSE] Registered active session peer system_id={}", peer_id);
                                                    }
                                                }
                                                Err(_) => {
                                                    eprintln!("[REVERSE] Ignoring non-UTF8 peer identity.");
                                                }
                                            }
                                        }

                                        17 => {
                                            let mut header = [0u8; 10];
                                            if read_stream.read_exact(&mut header).is_err() {
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    "failed reading Type 17 header",
                                                );
                                                break;
                                            }
                                            let payload_len = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
                                            let sample_rate = u32::from_be_bytes(header[4..8].try_into().unwrap());
                                            let channels = u16::from_be_bytes(header[8..10].try_into().unwrap());
                                            if payload_len == 0 || payload_len > 10 * 1024 * 1024 {
                                                eprintln!("[AUDIO RX] Invalid TYPE 17 payload length={}", payload_len);
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    &format!("invalid Type 17 payload length {}", payload_len),
                                                );
                                                break;
                                            }
                                            let mut payload = vec![0u8; payload_len];
                                            if read_stream.read_exact(&mut payload).is_err() {
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    "failed reading Type 17 payload",
                                                );
                                                break;
                                            }
                                            if let Some(samples) = decode_audio_packet(
                                                &payload,
                                                sample_rate,
                                                channels,
                                                audio_output_rate,
                                            ) {
                                                if let Ok(mut queue) = audio_playback_queue.lock() {
                                                    let max_samples = (audio_output_rate as f32 * 0.3) as usize; // 300ms max latency
                                                    if queue.len().saturating_add(samples.len()) > max_samples {
                                                        eprintln!(
                                                            "[AUDIO RX] stage=playback_resync reason=queue_limit queue_samples={} incoming_samples={} output_rate={}",
                                                            queue.len(),
                                                            samples.len(),
                                                            audio_output_rate
                                                        );
                                                        queue.clear();
                                                    }
                                                    queue.extend(samples);
                                                    inbound_audio_packet_count =
                                                        inbound_audio_packet_count.saturating_add(1);
                                                    if inbound_audio_packet_count == 1
                                                        || inbound_audio_packet_count % 100 == 0
                                                    {
                                                        eprintln!(
                                                            "[AUDIO RX] stage=queued packets={} source_rate={} source_channels={} payload_bytes={} queue_samples={} output_rate={}",
                                                            inbound_audio_packet_count,
                                                            sample_rate,
                                                            channels,
                                                            payload_len,
                                                            queue.len(),
                                                            audio_output_rate
                                                        );
                                                    }
                                                }
                                            } else {
                                                eprintln!(
                                                    "[AUDIO RX] Invalid TYPE 17 frame rate={} channels={} bytes={}",
                                                    sample_rate,
                                                    channels,
                                                    payload_len
                                                );
                                            }
                                        }

                                        20 => {
                                            let mut hdr = [0u8; 18];
                                            if let Err(error) = read_stream.read_exact(&mut hdr) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    20,
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            let offered_size = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
                                            let name_len = u16::from_be_bytes([hdr[16], hdr[17]]) as usize;
                                            if name_len > 4096 {
                                                if let Err(error) = discard_packet_bytes(&mut read_stream, name_len) {
                                                    mark_session_packet_read_failed(
                                                        &input_identity,
                                                        20,
                                                        "oversized_filename",
                                                        &error,
                                                        input_connected.as_ref(),
                                                        input_in_session.as_ref(),
                                                    );
                                                    break;
                                                }
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[FILE RX TRACE] direction=A->B packet_type=20 transfer_id={} filename_bytes={} result=rejected reason=filename_too_large",
                                                        transfer_id, name_len
                                                    ),
                                                );
                                                let mut reject = vec![25u8];
                                                reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                if let Err(error) = write_stream_input.send(reject) {
                                                    eprintln!("[FILE RX OFFER] Failed to reject oversized filename: {}", error);
                                                }
                                                continue;
                                            }
                                            let mut name_buf = vec![0u8; name_len];
                                            
                                            if !crate::status::session_perm_file_transfer() {
                                                if let Err(error) = read_stream.read_exact(&mut name_buf) {
                                                    mark_session_packet_read_failed(
                                                        &input_identity,
                                                        20,
                                                        "filename",
                                                        &error,
                                                        input_connected.as_ref(),
                                                        input_in_session.as_ref(),
                                                    );
                                                    break;
                                                }
                                                eprintln!("[FILE RX OFFER] Rejected because File Transfer permission is OFF");
                                                let mut reject = vec![25u8];
                                                reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                let _ = write_stream_input.send(reject);
                                                continue;
                                            }
                                            
                                            if let Err(error) = read_stream.read_exact(&mut name_buf) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    20,
                                                    "filename",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let received_name = match String::from_utf8(name_buf) {
                                                Ok(name) => name,
                                                Err(error) => {
                                                    eprintln!("[FILE RX OFFER] Rejected invalid UTF-8 file path: {}", error);
                                                    let mut reject = vec![25u8];
                                                    reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                    let _ = write_stream_input.send(reject);
                                                    continue;
                                                }
                                            };
                                            current_filename = match validate_relative_transfer_path(&received_name) {
                                                Ok(path) => path.to_string_lossy().replace('\\', "/"),
                                                Err(error) => {
                                                    eprintln!("[FILE RX OFFER] Rejected unsafe file path: {}", error);
                                                    let mut reject = vec![25u8];
                                                    reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                    let _ = write_stream_input.send(reject);
                                                    continue;
                                                }
                                            };
                                            if canonical_drop_dir.is_err() {
                                                let mut reject = vec![25u8];
                                                reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                let _ = write_stream_input.send(reject);
                                                continue;
                                            }
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[FILE TRACE] trace_id={} side=B direction=A_TO_B stage=PARSER_ACCEPTED type=20 transfer_id={} filename_bytes={} file_bytes={}",
                                                    packet_trace_id.as_deref().unwrap_or("missing"),
                                                    transfer_id,
                                                    name_len,
                                                    offered_size
                                                ),
                                            );
                                            // FIXED: Type 20 offer → UI acceptance state (25 reject / 26 accept).
                                            current_file = None;
                                            current_file_path = None;
                                            current_transfer_id = Some(transfer_id);
                                            current_file_chunk_index = 0;
                                            total_file_size = offered_size;
                                            received_bytes = 0;
                                            file_hasher = Sha256::new();
                                            if let Err(error) = crate::status::push_session_file_offer(crate::status::SessionFileOffer {
                                                transfer_id,
                                                filename: current_filename.clone(),
                                                size: total_file_size,
                                            }) {
                                                eprintln!("[FILE RX OFFER] Failed to publish offer: {}", error);
                                                let mut reject = vec![25u8];
                                                reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                let _ = write_stream_input.send(reject);
                                                current_transfer_id = None;
                                                continue;
                                            }
                                            crate::status::update_session_file_progress(crate::status::SessionFileProgress {
                                                transfer_id,
                                                filename: current_filename.clone(),
                                                total_bytes: total_file_size,
                                                received_bytes: 0,
                                                chunk_index: 0,
                                                running_sha256_hex: String::new(),
                                                accepted: false,
                                            });
                                            agent_log!(
                                                "[FILE RX OFFER] transfer_id={} filename={} total_bytes={} awaiting_accept=true",
                                                transfer_id, current_filename, total_file_size
                                            );
                                        }

                                        21 => {
                                            let mut hdr = [0u8; 16];
                                            if let Err(error) = read_stream.read_exact(&mut hdr) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    21,
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            let chunk_idx = u32::from_be_bytes(hdr[8..12].try_into().unwrap());
                                            let chunk_len = u32::from_be_bytes(hdr[12..16].try_into().unwrap()) as usize;
                                            if chunk_len > 10 * 1024 * 1024 {
                                                if chunk_len <= MAX_RECOVERABLE_PACKET_DISCARD_BYTES {
                                                    if let Err(error) = discard_packet_bytes(&mut read_stream, chunk_len) {
                                                        mark_session_packet_read_failed(
                                                            &input_identity,
                                                            21,
                                                            "oversized_chunk_payload",
                                                            &error,
                                                            input_connected.as_ref(),
                                                            input_in_session.as_ref(),
                                                        );
                                                        break;
                                                    }
                                                    crate::session_debug::log(
                                                        &input_identity,
                                                        &format!(
                                                            "[FILE RX TRACE] direction=A->B packet_type=21 transfer_id={} chunk_bytes={} max_chunk_bytes=10485760 result=transfer_aborted reason=oversized_chunk",
                                                            transfer_id, chunk_len
                                                        ),
                                                    );
                                                    if current_transfer_id == Some(transfer_id) {
                                                        current_file = None;
                                                        if let Some(path) = current_file_path.take() {
                                                            let _ = fs::remove_file(path);
                                                        }
                                                        current_transfer_id = None;
                                                        received_bytes = 0;
                                                        crate::status::finish_session_file(transfer_id);
                                                    }
                                                    send_file_error(transfer_id, "File chunk exceeds the maximum supported size");
                                                    continue;
                                                }
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[FILE RX TRACE] direction=A->B packet_type=21 transfer_id={} chunk_bytes={} max_recoverable_discard_bytes={} result=session_end reason=unrecoverable_stream_framing",
                                                        transfer_id, chunk_len, MAX_RECOVERABLE_PACKET_DISCARD_BYTES
                                                    ),
                                                );
                                                mark_session_disconnected(
                                                    &input_identity,
                                                    "session_input_reader",
                                                    "cloned_session_reader_stream",
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                    false,
                                                    &format!(
                                                        "TYPE21 chunk length {} exceeds the {}-byte safe discard limit",
                                                        chunk_len, MAX_RECOVERABLE_PACKET_DISCARD_BYTES
                                                    ),
                                                );
                                                break;
                                            }
                                            let mut chunk_buf = vec![0u8; chunk_len];
                                            if let Err(error) = read_stream.read_exact(&mut chunk_buf) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    21,
                                                    "chunk_payload",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[FILE TRACE] trace_id={} side=B direction=A_TO_B stage=PARSER_ACCEPTED type=21 transfer_id={} chunk_index={} payload_bytes={} packet_bytes={}",
                                                    packet_trace_id.as_deref().unwrap_or("missing"),
                                                    transfer_id,
                                                    chunk_idx,
                                                    chunk_len,
                                                    17 + chunk_len
                                                ),
                                            );
                                            if chunk_len == 0 {
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[FILE RX TRACE] direction=A->B packet_type=21 transfer_id={} chunk_index={} result=transfer_aborted reason=empty_chunk",
                                                        transfer_id, chunk_idx
                                                    ),
                                                );
                                                if current_transfer_id == Some(transfer_id) {
                                                    current_file = None;
                                                    if let Some(path) = current_file_path.take() {
                                                        let _ = fs::remove_file(path);
                                                    }
                                                    current_transfer_id = None;
                                                    received_bytes = 0;
                                                    crate::status::finish_session_file(transfer_id);
                                                }
                                                send_file_error(transfer_id, "File chunk must not be empty");
                                                continue;
                                            }
                                            
                                            // FIXED: Type 21 chunks require prior Type 26 acceptance; progress + SHA every 100 chunks.
                                            let accepted = match crate::status::session_file_is_accepted(transfer_id) {
                                                Ok(accepted) => accepted,
                                                Err(error) => {
                                                    eprintln!("[FILE RX] Failed to read acceptance state: {}", error);
                                                    false
                                                }
                                            };
                                            let chunk_is_valid = current_transfer_id == Some(transfer_id)
                                                && accepted
                                                && chunk_idx == current_file_chunk_index
                                                && received_bytes.saturating_add(chunk_len as u64) <= total_file_size;
                                            if chunk_is_valid {
                                                if current_file.is_none() {
                                                    let destination = canonical_drop_dir.as_ref()
                                                        .map_err(|error| std::io::Error::new(
                                                            std::io::ErrorKind::Other,
                                                            error.to_string(),
                                                        ))
                                                        .and_then(|root| {
                                                            create_session_transfer_file(
                                                                &drop_dir,
                                                                root,
                                                                &current_filename,
                                                            )
                                                        });
                                                    match destination {
                                                        Ok((file, path)) => {
                                                            current_file = Some(file);
                                                            current_file_path = Some(path);
                                                        }
                                                        Err(error) => {
                                                            eprintln!("[FILE RX START] transfer_id={} create_failed={:?}", transfer_id, error);
                                                            send_file_error(transfer_id, "Unable to create destination file");
                                                            crate::status::finish_session_file(transfer_id);
                                                            current_transfer_id = None;
                                                            continue;
                                                        }
                                                    }
                                                }
                                                if let Some(ref mut file) = current_file {
                                                    if let Err(error) = file.write_all(&chunk_buf) {
                                                        send_file_error(transfer_id, &format!("Disk write failed: {}", error));
                                                        current_file = None;
                                                        if let Some(path) = current_file_path.take() {
                                                            let _ = fs::remove_file(path);
                                                        }
                                                        crate::status::finish_session_file(transfer_id);
                                                        current_transfer_id = None;
                                                        received_bytes = 0;
                                                        continue;
                                                    }
                                                    file_hasher.update(&chunk_buf);
                                                    received_bytes += chunk_len as u64;
                                                    current_file_chunk_index = current_file_chunk_index.saturating_add(1);

                                                    // FIXED: progress via status; publish running SHA-256 every 100 chunks.
                                                    let publish_hash = current_file_chunk_index % 100 == 0;
                                                    let running_hex = if publish_hash {
                                                        let checkpoint = file_hasher.clone().finalize();
                                                        checkpoint.iter().map(|b| format!("{:02x}", b)).collect::<String>()
                                                    } else {
                                                        String::new()
                                                    };
                                                    crate::status::update_session_file_progress(crate::status::SessionFileProgress {
                                                        transfer_id,
                                                        filename: current_filename.clone(),
                                                        total_bytes: total_file_size,
                                                        received_bytes,
                                                        chunk_index: current_file_chunk_index,
                                                        running_sha256_hex: running_hex,
                                                        accepted: true,
                                                    });
                                                }
                                            } else if current_transfer_id == Some(transfer_id) && !accepted {
                                                // FIXED: chunks before UI acceptance (Type 26) — ignore, keep offer pending.
                                                continue;
                                            } else if current_transfer_id == Some(transfer_id) {
                                                let reason = if chunk_idx != current_file_chunk_index {
                                                    format!("Unexpected chunk index {}; expected {}", chunk_idx, current_file_chunk_index)
                                                } else {
                                                    "Chunk exceeded offered file size".to_string()
                                                };
                                                send_file_error(transfer_id, &reason);
                                                current_file = None;
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                                crate::status::finish_session_file(transfer_id);
                                                current_transfer_id = None;
                                                received_bytes = 0;
                                            }
                                        }

                                        22 => {
                                            let mut hdr = [0u8; 48];
                                            if let Err(error) = read_stream.read_exact(&mut hdr) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    22,
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            let final_size = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[FILE TRACE] trace_id={} side=B direction=A_TO_B stage=PARSER_ACCEPTED type=22 transfer_id={} packet_bytes=49 file_bytes={}",
                                                    packet_trace_id.as_deref().unwrap_or("missing"),
                                                    transfer_id,
                                                    final_size
                                                ),
                                            );
                                            let hash_hex: String = hdr[16..48].iter().map(|b| format!("{:02x}", b)).collect();
                                            if current_transfer_id != Some(transfer_id) {
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[FILE RX TRACE] direction=A->B packet_type=22 transfer_id={} active_transfer_id={:?} result=rejected reason=unexpected_transfer",
                                                        transfer_id, current_transfer_id
                                                    ),
                                                );
                                                send_file_error(transfer_id, "Unexpected file completion packet");
                                                continue;
                                            }
                                            let actual_hash = std::mem::replace(&mut file_hasher, Sha256::new()).finalize();
                                            let accepted = match crate::status::session_file_is_accepted(transfer_id) {
                                                Ok(accepted) => accepted,
                                                Err(error) => {
                                                    eprintln!("[FILE RX] Failed to read acceptance state: {}", error);
                                                    false
                                                }
                                            };
                                            if should_create_empty_transfer_file(
                                                current_transfer_id == Some(transfer_id),
                                                accepted,
                                                final_size,
                                                current_file.is_some(),
                                            ) {
                                                if let Ok(root) = &canonical_drop_dir {
                                                    match create_session_transfer_file(
                                                        &drop_dir,
                                                        root,
                                                        &current_filename,
                                                    ) {
                                                        Ok((file, path)) => {
                                                            current_file = Some(file);
                                                            current_file_path = Some(path);
                                                        }
                                                        Err(error) => {
                                                            eprintln!("[FILE RX END] Failed to create empty file: {}", error);
                                                        }
                                                    }
                                                }
                                            }
                                            // FIXED: Type 22 end — final SHA-256 verify + Type 27 ack / Type 24 error.
                                            let transfer_matches = current_transfer_id == Some(transfer_id) && accepted;
                                            let size_matches = total_file_size == final_size && received_bytes == final_size;
                                            let hash_matches = actual_hash.as_slice() == &hdr[16..48];
                                            let flush_ok = current_file
                                                .take()
                                                .map(|mut file| file.flush().is_ok())
                                                .unwrap_or(false);
                                            crate::status::update_session_file_progress(crate::status::SessionFileProgress {
                                                transfer_id,
                                                filename: current_filename.clone(),
                                                total_bytes: total_file_size,
                                                received_bytes,
                                                chunk_index: current_file_chunk_index,
                                                running_sha256_hex: hash_hex.clone(),
                                                accepted: transfer_matches,
                                            });
                                            let valid = transfer_matches && size_matches && hash_matches;
                                            if valid && flush_ok {
                                                let mut ack = vec![27u8];
                                                ack.extend_from_slice(&transfer_id.to_be_bytes());
                                                if let Err(error) = write_stream_input.send(ack) {
                                                    eprintln!("[FILE RX COMPLETE_ACK] transfer_id={} err={}", transfer_id, error);
                                                }
                                            } else {
                                                send_file_error(transfer_id, "File size or SHA-256 verification failed");
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                            }
                                            crate::status::finish_session_file(transfer_id);
                                            current_transfer_id = None;
                                            current_file_path = None;
                                        }

                                        23 => {
                                            let mut hdr = [0u8; 8];
                                            if let Err(error) = read_stream.read_exact(&mut hdr) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    23,
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let transfer_id = u64::from_be_bytes(hdr);
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[FILE TRACE] trace_id={} side=B direction=A_TO_B stage=PARSER_ACCEPTED type=23 transfer_id={} packet_bytes=9",
                                                    packet_trace_id.as_deref().unwrap_or("missing"),
                                                    transfer_id
                                                ),
                                            );
                                            crate::status::resolve_session_file_response(transfer_id, 23);
                                            if current_transfer_id == Some(transfer_id) {
                                                current_file = None;
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                                current_transfer_id = None;
                                                received_bytes = 0;
                                            }
                                            crate::status::finish_session_file(transfer_id);
                                            println!("[File Transfer] Cancelled.");
                                        }

                                        24 => {
                                            let mut hdr = [0u8; 12];
                                            if let Err(error) = read_stream.read_exact(&mut hdr) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    24,
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            let msg_len = u16::from_be_bytes([hdr[10], hdr[11]]) as usize;
                                            if msg_len > 4096 {
                                                if let Err(error) = discard_packet_bytes(&mut read_stream, msg_len) {
                                                    mark_session_packet_read_failed(
                                                        &input_identity,
                                                        24,
                                                        "oversized_error_message",
                                                        &error,
                                                        input_connected.as_ref(),
                                                        input_in_session.as_ref(),
                                                    );
                                                    break;
                                                }
                                                crate::session_debug::log(
                                                    &input_identity,
                                                    &format!(
                                                        "[FILE RX TRACE] direction=A->B packet_type=24 transfer_id={} message_bytes={} result=truncated reason=error_message_too_large",
                                                        transfer_id, msg_len
                                                    ),
                                                );
                                                crate::status::resolve_session_file_response(transfer_id, 24);
                                                if current_transfer_id == Some(transfer_id) {
                                                    current_file = None;
                                                    if let Some(path) = current_file_path.take() {
                                                        let _ = fs::remove_file(path);
                                                    }
                                                    current_transfer_id = None;
                                                    received_bytes = 0;
                                                }
                                                crate::status::finish_session_file(transfer_id);
                                                continue;
                                            }
                                            let mut msg_buf = vec![0u8; msg_len];
                                            if let Err(error) = read_stream.read_exact(&mut msg_buf) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    24,
                                                    "error_message",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[FILE TRACE] trace_id={} side=B direction=A_TO_B stage=PARSER_ACCEPTED type=24 transfer_id={} message_bytes={} packet_bytes={}",
                                                    packet_trace_id.as_deref().unwrap_or("missing"),
                                                    transfer_id,
                                                    msg_len,
                                                    13 + msg_len
                                                ),
                                            );
                                            crate::status::resolve_session_file_response(transfer_id, 24);
                                            if current_transfer_id == Some(transfer_id) {
                                                current_file = None;
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                                current_transfer_id = None;
                                                received_bytes = 0;
                                            }
                                            crate::status::finish_session_file(transfer_id);
                                            if let Ok(msg) = String::from_utf8(msg_buf) {
                                                println!("[FILE RX ERROR] direction=A->B transfer_id={} success=false reason=\"{}\"", transfer_id, msg);
                                            }
                                        }

                                        25 | 26 | 27 => {
                                            let mut hdr = [0u8; 8];
                                            if let Err(error) = read_stream.read_exact(&mut hdr) {
                                                mark_session_packet_read_failed(
                                                    &input_identity,
                                                    pkt_type_buf[0],
                                                    "header",
                                                    &error,
                                                    input_connected.as_ref(),
                                                    input_in_session.as_ref(),
                                                );
                                                break;
                                            }
                                            let transfer_id = u64::from_be_bytes(hdr);
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[FILE TRACE] trace_id={} side=B direction=A_TO_B stage=PARSER_ACCEPTED type={} transfer_id={} packet_bytes=9",
                                                    packet_trace_id.as_deref().unwrap_or("missing"),
                                                    pkt_type_buf[0],
                                                    transfer_id
                                                ),
                                            );
                                            let matched_sender = crate::status::resolve_session_file_response(transfer_id, pkt_type_buf[0]);
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=B receiver=A stage=RESPONSE_ROUTED packet_type={} matched_waiter={} result={}",
                                                    input_identity,
                                                    transfer_id,
                                                    pkt_type_buf[0],
                                                    matched_sender,
                                                    if matched_sender { "DELIVERED" } else { "STALE_OR_UNMATCHED" }
                                                ),
                                            );
                                            println!("[FILE TX RESPONSE] session_id={} transfer_id={} type={} matched_waiter={}", input_identity, transfer_id, pkt_type_buf[0], matched_sender);
                                        }

                                        99 => {
                                            println!("[SESSION DISCONNECT] Received TYPE 99 from relay; terminating active session.");
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[TYPE99 TRACE] component=HOST direction=RECV device_id={} session_id={} source=session_input_reader reason=packet_byte_99",
                                                    input_identity, input_identity
                                                ),
                                            );
                                            crate::session_debug::lifecycle(
                                                &input_identity,
                                                "TYPE99_RECEIVED",
                                                "session_input_reader",
                                                "cloned_session_reader_stream",
                                                Some(input_connected.as_ref()),
                                                Some(input_in_session.as_ref()),
                                                false,
                                                "received session disconnect type 99",
                                            );
                                            mark_session_disconnected(
                                                &input_identity,
                                                "session_input_reader",
                                                "cloned_session_reader_stream",
                                                input_connected.as_ref(),
                                                input_in_session.as_ref(),
                                                false,
                                                "received Type 99 from relay",
                                            );
                                            crate::session_debug::log(
                                                &input_identity,
                                                &format!(
                                                    "[TYPE99 TRACE] component=HOST direction=RECV_COMPLETE device_id={} session_id={} source=session_input_reader reason=session_disconnected",
                                                    input_identity, input_identity
                                                ),
                                            );
                                            break;
                                        }

                                        _ => {}
                                    }
                                }
                                crate::session_debug::lifecycle(
                                    &input_identity,
                                    "SESSION_READ_RETURNED",
                                    "session_input_reader",
                                    "cloned_session_reader_stream",
                                    Some(input_connected.as_ref()),
                                    Some(input_in_session.as_ref()),
                                    false,
                                    "reader loop exited",
                                );
                                crate::session_debug::lifecycle(
                                    &input_identity,
                                    "THREAD_EXIT",
                                    "session_input_reader",
                                    "cloned_session_reader_stream",
                                    Some(input_connected.as_ref()),
                                    Some(input_in_session.as_ref()),
                                    false,
                                    "reader thread returning",
                                );
                                drop(current_file.take());
                                if let Some(path) = current_file_path.take() {
                                    let _ = fs::remove_file(path);
                                }
                                if let Some(transfer_id) = current_transfer_id.take() {
                                    crate::status::finish_session_file(transfer_id);
                                }
                            });

                            let ping_identity = system_id.clone();
                            let ping_handle = thread::spawn(move || {
                                let mut previous_id = 0u64;
                                let mut next_ping = Instant::now();
                                while is_conn_ping.load(Ordering::SeqCst) {
                                    let now = Instant::now();
                                    let timed_out = session_heartbeat_ping
                                        .lock()
                                        .ok()
                                        .and_then(|mut tracker| tracker.timeout(now, HEARTBEAT_TIMEOUT));
                                    if let Some(probe_id) = timed_out {
                                        crate::session_debug::log(
                                            &ping_identity,
                                            &format!(
                                                "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=TIMEOUT reason=response_timeout",
                                                crate::status::now_ms(), probe_id
                                            ),
                                        );
                                        log_health_transition(
                                            &ping_identity,
                                            "SESSION_END_TO_END",
                                            crate::status::record_heartbeat_missed(true, "response_timeout"),
                                        );
                                        let missed_count = crate::status::heartbeat_missed_count(true).unwrap_or(0);
                                        if missed_count >= crate::status::HEARTBEAT_MISS_LIMIT {
                                            if crate::status::set_disconnect_reason_if_unknown("heartbeat_timeout") {
                                                crate::session_debug::log(
                                                    &ping_identity,
                                                    &format!(
                                                        "[DISCONNECT TRACE] timestamp_ms={} stage=FIRST_FAILURE source=session_heartbeat reason=heartbeat_timeout missed_count={}",
                                                        crate::status::now_ms(), missed_count
                                                    ),
                                                );
                                            }
                                            crate::session_debug::log(
                                                &ping_identity,
                                                &format!(
                                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=SESSION_SOCKET_SHUTDOWN reason=heartbeat_timeout missed_count={}",
                                                    crate::status::now_ms(), probe_id, missed_count
                                                ),
                                            );
                                            is_conn_ping.store(false, Ordering::Release);
                                            if let Some(shutdown_stream) = heartbeat_shutdown_stream.as_ref() {
                                                let _ = shutdown_stream.shutdown(std::net::Shutdown::Both);
                                            }
                                            break;
                                        }
                                    }
                                    if now >= next_ping {
                                        let probe_id = next_heartbeat_id(&mut previous_id);
                                        let scheduled = session_heartbeat_ping
                                            .lock()
                                            .map(|mut tracker| tracker.schedule(probe_id))
                                            .unwrap_or(false);
                                        if scheduled {
                                            let mut ping_pkt = Vec::with_capacity(9);
                                            ping_pkt.push(14u8);
                                            ping_pkt.extend_from_slice(&probe_id.to_be_bytes());
                                            crate::session_debug::log(
                                                &ping_identity,
                                                &format!(
                                                    "[PING TRACE] timestamp_ms={} trace_id={} channel=SESSION_END_TO_END stage=SCHEDULED type=14 interval_ms={}",
                                                    crate::status::now_ms(), probe_id, HEARTBEAT_INTERVAL.as_millis()
                                                ),
                                            );
                                            if write_stream_ping.send(ping_pkt).is_err() {
                                                if let Ok(mut tracker) = session_heartbeat_ping.lock() {
                                                    tracker.cancel(probe_id);
                                                }
                                                break;
                                            }
                                            next_ping = Instant::now() + HEARTBEAT_INTERVAL;
                                        }
                                    }
                                    thread::sleep(Duration::from_millis(100));
                                }
                            });

                            // FIXED: local clipboard watcher skips remote-apply echoes and enforces size limit.
                            let _clip_identity = system_id.clone();
                            let clip_handle = thread::spawn(move || {
                                let mut clip = Clipboard::new().ok();
                                while is_conn_clip.load(Ordering::SeqCst) {
                                    if remote_clip_send.load(Ordering::SeqCst) {
                                        thread::sleep(Duration::from_millis(50));
                                        continue;
                                    }
                                    if let Some(ref mut c) = clip {
                                        if let Ok(text) = c.get_text() {
                                            if text.len() > MAX_CLIPBOARD_PACKET_BYTES {
                                                thread::sleep(Duration::from_millis(1000));
                                                continue;
                                            }
                                            let mut is_new = false;
                                            if let Ok(mut guard) = last_clip_send.lock() {
                                                if *guard != text && !text.is_empty() {
                                                    *guard = text.clone();
                                                    is_new = true;
                                                }
                                            }
                                            if is_new {
                                                let bytes = text.into_bytes();
                                                let mut packet = Vec::with_capacity(5 + bytes.len());
                                                packet.push(12u8);
                                                packet.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                                                packet.extend_from_slice(&bytes);
                                                if write_stream_clip.send(packet).is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    thread::sleep(Duration::from_millis(1000));
                                }
                            });

                            let _audio = match start_audio_capture(write_stream_audio, is_conn_audio) {
                                Ok(stream) => {
                                    crate::status::set_session_audio_capture_available(true);
                                    println!("[Audio] Microphone capture is available for this session.");
                                    Some(stream)
                                }
                                Err(error) => {
                                    crate::status::set_session_audio_capture_available(false);
                                    eprintln!("[Audio] Microphone capture is unavailable for this session: {error}");
                                    None
                                }
                            };

                            let capture_handle = thread::spawn(move || {
                                let mut screens = Screen::all().unwrap_or_default();
                                let mut last_idx = active_idx_capture.load(Ordering::SeqCst);

                                println!("[Capture] {} monitor(s) detected.", screens.len());

                                let mut next_frame_time = Instant::now() + Duration::from_micros(FRAME_INTERVAL_MICROS);
                                let mut captured_frames = 0u64;
                                let mut last_capture_log = Instant::now();

                                while is_conn_capture.load(Ordering::SeqCst) {
                                    if !crate::status::session_perm_view_screen() {
                                        thread::sleep(Duration::from_millis(50));
                                        continue;
                                    }

                                    let current_idx = active_idx_capture.load(Ordering::SeqCst);

                                    if current_idx != last_idx || screens.is_empty() {
                                        screens = Screen::all().unwrap_or_default();
                                        last_idx = current_idx;
                                    }

                                    if screens.is_empty() {
                                        thread::sleep(Duration::from_micros(FRAME_INTERVAL_MICROS));
                                        continue;
                                    }

                                    let mut captured_frame = None;

                                    if !screens.is_empty() {
                                        let screen = match screens.get(current_idx) {
                                            Some(s) => s,
                                            None => &screens[0],
                                        };
                                        if let Ok(img) = screen.capture() {
                                            let source_width = img.width() as usize;
                                            let source_height = img.height() as usize;
                                            let raw = img.into_raw();
                                            let expected = source_width.saturating_mul(source_height).saturating_mul(4);
                                            if raw.len() >= expected {
                                                captured_frame = Some(FrameData {
                                                    width: source_width,
                                                    height: source_height,
                                                    raw_pixels: raw,
                                                    captured_at_ms: current_time_millis(),
                                                });
                                            }
                                        }
                                    }

                                    if captured_frame.is_none() {
                                        captured_frame = capture_screen_gdi();
                                    }

                                    if let Some(frame) = captured_frame {
                                        captured_frames += 1;
                                        if last_capture_log.elapsed() >= Duration::from_secs(1) {
                                            println!(
                                                "[VIDEO CAPTURE] frames={} captured_at_ms={} source={}x{}",
                                                captured_frames,
                                                frame.captured_at_ms,
                                                frame.width,
                                                frame.height
                                            );
                                            captured_frames = 0;
                                            last_capture_log = Instant::now();
                                        }
                                        let (lock, cvar) = &*shared_frame_cap;
                                        if let Ok(mut shared) = lock.lock() {
                                            *shared = Some(frame);
                                            cvar.notify_one();
                                        }
                                    } else {
                                        screens = Screen::all().unwrap_or_default();
                                        thread::sleep(Duration::from_millis(16));
                                    }

                                    let now = Instant::now();
                                    if now < next_frame_time {
                                        thread::sleep(next_frame_time - now);
                                    }
                                    
                                    // Advance monotonic timer and avoid accumulating extreme debt
                                    next_frame_time += Duration::from_micros(FRAME_INTERVAL_MICROS);
                                    let current_time = Instant::now();
                                    if next_frame_time < current_time {
                                        next_frame_time = current_time;
                                    }
                                }
                            });

                            let mut frame_number: u64 = 0;
                            let mut hw_encoder: Option<HardwareH264Encoder> = None;
                            let mut current_enc_width: u32 = 0;
                            let mut current_enc_height: u32 = 0;
                            let mut encode_error_count = 0u64;
                            // FIXED: do not re-init every frame when hw_encoder stays None after a fatal failure.
                            let mut encoder_init_failed = false;

                            // ── PRE-WARM ENCODER ─────────────────────────────────────────────
                            // The encoder was previously initialized lazily on the first captured
                            // frame.  On a 4K display this incurred a 3-8 second GPU/MF setup
                            // delay during which the relay received no TYPE 13 video packets.
                            // The relay's internal session-idle timer fired (≈3-4 s) and sent
                            // TYPE 99, terminating every session before video could ever begin.
                            //
                            // Fix: initialize the encoder immediately at session start using the
                            // current screen resolution.  When the first captured frame arrives it
                            // will pass the `hw_encoder.is_none()` check as false, skipping
                            // re-initialization and encoding the frame without any GPU ramp-up
                            // penalty.  If dimensions differ from the pre-warmed values the
                            // existing reinit path handles it normally.
                            // ─────────────────────────────────────────────────────────────────
                            crate::session_debug::lifecycle(
                                &system_id,
                                "ENCODER_PREWARM_START",
                                "session_pipeline",
                                "cloned_session_stream",
                                Some(is_connected.as_ref()),
                                Some(is_in_session.as_ref()),
                                false,
                                "pre-warming H264 encoder before first captured frame",
                            );
                            #[cfg(windows)]
                            let (prewarm_w, prewarm_h) = unsafe {
                                (
                                    windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                                        windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
                                    ) as u32,
                                    windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                                        windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
                                    ) as u32,
                                )
                            };
                            #[cfg(not(windows))]
                            let (prewarm_w, prewarm_h) = (1920u32, 1080u32);
                            if prewarm_w > 0 && prewarm_h > 0 {
                                let dynamic_bitrate =
                                    ((prewarm_w as usize).saturating_mul(prewarm_h as usize)
                                        as f64
                                        * 16.0) as u32;
                                let target_bitrate = if is_aws_relay {
                                    dynamic_bitrate.clamp(1_500_000, 3_000_000)
                                } else {
                                    dynamic_bitrate.clamp(20_000_000, 75_000_000)
                                };
                                let (target_w, target_h) = if is_aws_relay {
                                    if prewarm_w > 1920 || prewarm_h > 1080 {
                                        let ratio_w = 1920.0 / prewarm_w as f32;
                                        let ratio_h = 1080.0 / prewarm_h as f32;
                                        let ratio = ratio_w.min(ratio_h);
                                        let mut w = (prewarm_w as f32 * ratio) as u32;
                                        let mut h = (prewarm_h as f32 * ratio) as u32;
                                        w -= w % 2;
                                        h -= h % 2;
                                        (w, h)
                                    } else {
                                        (prewarm_w, prewarm_h)
                                    }
                                } else {
                                    (prewarm_w, prewarm_h)
                                };

                                match HardwareH264Encoder::new(
                                    target_w,
                                    target_h,
                                    TARGET_FPS,
                                    target_bitrate,
                                ) {
                                    Ok(enc) => {
                                        hw_encoder = Some(enc);
                                        current_enc_width = target_w;
                                        current_enc_height = target_h;
                                        println!(
                                            "[H264 HW] Encoder pre-warmed {}x{} — ready for first frame",
                                            prewarm_w, prewarm_h
                                        );
                                        crate::session_debug::lifecycle(
                                            &system_id,
                                            "ENCODER_PREWARM_OK",
                                            "session_pipeline",
                                            "cloned_session_stream",
                                            Some(is_connected.as_ref()),
                                            Some(is_in_session.as_ref()),
                                            false,
                                            &format!(
                                                "encoder ready {}x{} before first captured frame",
                                                prewarm_w, prewarm_h
                                            ),
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[H264 HW] Pre-warm failed (lazy init on first frame): {}",
                                            e
                                        );
                                        crate::session_debug::lifecycle(
                                            &system_id,
                                            "ENCODER_PREWARM_FAILED",
                                            "session_pipeline",
                                            "cloned_session_stream",
                                            Some(is_connected.as_ref()),
                                            Some(is_in_session.as_ref()),
                                            false,
                                            &format!("pre-warm error: {}", e),
                                        );
                                    }
                                }
                            }

                            let mut last_encode_log = Instant::now();
                            let mut total_encoded_frames = 0u64;
                            let mut total_encoded_bytes = 0u64;

                            while is_conn_write.load(Ordering::SeqCst) {
                                let frame_opt = {
                                    let (lock, cvar) = &*shared_frame;
                                    let mut shared = match lock.lock() {
                                        Ok(g) => g,
                                        Err(_) => break,
                                    };

                                    if shared.is_none() {
                                        let result = cvar.wait_timeout(shared, Duration::from_millis(50));
                                        match result {
                                            Ok((guard, _)) => {
                                                shared = guard;
                                            }
                                            Err(_) => continue,
                                        }
                                    }
                                    shared.take()
                                };

                                let frame = match frame_opt {
                                    Some(f) => f,
                                    None => continue,
                                };
                                
                                let recovery_req = video_recovery_capture.load(Ordering::Acquire);

                                let src_width = frame.width;
                                let src_height = frame.height;
                                if src_width == 0 || src_height == 0 {
                                    continue;
                                }

                                let (target_w, target_h) = if is_aws_relay {
                                    if src_width > 1920 || src_height > 1080 {
                                        let ratio_w = 1920.0 / src_width as f32;
                                        let ratio_h = 1080.0 / src_height as f32;
                                        let ratio = ratio_w.min(ratio_h);
                                        let mut w = (src_width as f32 * ratio) as u32;
                                        let mut h = (src_height as f32 * ratio) as u32;
                                        w -= w % 2;
                                        h -= h % 2;
                                        (w, h)
                                    } else {
                                        (src_width as u32, src_height as u32)
                                    }
                                } else {
                                    (src_width as u32, src_height as u32)
                                };

                                // FIXED: only (re)init when missing once, or when resolution actually changes.
                                let resolution_changed =
                                    current_enc_width != target_w || current_enc_height != target_h;
                                if (hw_encoder.is_none() && !encoder_init_failed) || resolution_changed {
                                    let dynamic_bitrate = ((src_width * src_height) as f64 * 16.0) as u32;
                                    let target_bitrate = if is_aws_relay {
                                        dynamic_bitrate.clamp(1_500_000, 3_000_000)
                                    } else {
                                        dynamic_bitrate.clamp(20_000_000, 75_000_000)
                                    };

                                    match HardwareH264Encoder::new(target_w, target_h, TARGET_FPS, target_bitrate) {
                                        Ok(enc) => {
                                            hw_encoder = Some(enc);
                                            current_enc_width = target_w;
                                            current_enc_height = target_h;
                                            encoder_init_failed = false;
                                        }
                                        Err(e) => {
                                            // FIXED: remember the attempted dimensions and stop
                                            // retrying the same failed encoder setup every frame.
                                            hw_encoder = None;
                                            current_enc_width = target_w;
                                            current_enc_height = target_h;
                                            encoder_init_failed = true;
                                            eprintln!("[H264 HW][FATAL] Hardware encoder initialization failed: {}", e);
                                            continue;
                                        }
                                    }
                                }

                                let encoder = match hw_encoder.as_mut() {
                                    Some(enc) => enc,
                                    None => continue,
                                };
                                let is_keyframe_request = frame_number == 0
                                    || !encoder.has_produced_keyframe
                                    || (frame_number % (TARGET_FPS as u64) == 0)
                                    || recovery_req;

                                let h264_bytes = match encoder.encode_rgba(
                                    &frame.raw_pixels,
                                    src_width,
                                    src_height,
                                    is_keyframe_request,
                                ) {
                                    Ok(bytes) => bytes,
                                    Err(error) => {
                                        encode_error_count += 1;
                                        if encode_error_count == 1 || encode_error_count % 60 == 0 {
                                            eprintln!(
                                                "[VIDEO ENCODE ERROR] count={} error={}",
                                                encode_error_count, error
                                            );
                                        }
                                        continue;
                                    }
                                };

                                if h264_bytes.is_empty() {
                                    continue;
                                }

                                frame_number += 1;
                                total_encoded_frames += 1;
                                total_encoded_bytes += h264_bytes.len() as u64;
                                let (_nal_types, _has_sps, _has_pps, has_idr) =
                                    encoder::mf_encoder::inspect_nals(&h264_bytes);

                                if has_idr || last_encode_log.elapsed() >= Duration::from_secs(2) {
                                    println!(
                                        "[VIDEO ENCODE] frame={} is_idr={} bytes={} total_frames={} total_bytes={} enc_dim={}x{}",
                                        frame_number, has_idr, h264_bytes.len(), total_encoded_frames, total_encoded_bytes, current_enc_width, current_enc_height
                                    );
                                    last_encode_log = Instant::now();
                                }

                                // FIXED: TYPE 13 layout unchanged — type + width + height + size + pts + payload.
                                let packet_size = 21 + h264_bytes.len();
                                let mut packet = Vec::with_capacity(packet_size);
                                packet.push(13u8);
                                packet.extend_from_slice(&current_enc_width.to_be_bytes());
                                packet.extend_from_slice(&current_enc_height.to_be_bytes());
                                packet.extend_from_slice(&(h264_bytes.len() as u32).to_be_bytes());
                                packet.extend_from_slice(&frame.captured_at_ms.to_be_bytes());
                                packet.extend_from_slice(&h264_bytes);

                                if has_idr {
                                    video_recovery_capture.store(false, Ordering::Release);
                                } else if recovery_req {
                                    // Still waiting for the requested IDR — do not enqueue P-frames.
                                    continue;
                                }

                                let mut queue = match video_slot_capture.lock() {
                                    Ok(q) => q,
                                    Err(_) => {
                                        mark_session_disconnected(
                                            &system_id,
                                            "session_capture_pipeline",
                                            "cloned_session_writer_stream",
                                            is_connected.as_ref(),
                                            is_in_session.as_ref(),
                                            false,
                                            "video packet queue mutex poisoned",
                                        );
                                        break;
                                    }
                                };

                                // FIXED: keep at most three frames; clear only for a requested
                                // keyframe when full, and request recovery only on full/no-IDR.
                                if queue.len() >= 3 {
                                    if is_keyframe_request {
                                        queue.clear();
                                    } else {
                                        let _ = queue.pop_front();
                                        if !has_idr {
                                            video_recovery_capture.store(true, Ordering::Release);
                                        }
                                    }
                                }

                                queue.push_back(packet);
                            }

                            // SHUTDOWN OF SESSION
                            crate::session_debug::lifecycle(
                                &system_id,
                                "SESSION_CLEANUP",
                                "session_pipeline",
                                "cloned_session_stream",
                                Some(is_connected.as_ref()),
                                Some(is_in_session.as_ref()),
                                false,
                                "session worker entered ordered cleanup",
                            );
                            if is_connected.swap(false, Ordering::SeqCst) {
                                crate::session_debug::lifecycle(
                                    &system_id,
                                    "IS_CONNECTED_FALSE",
                                    "session_pipeline",
                                    "cloned_session_stream",
                                    Some(is_connected.as_ref()),
                                    Some(is_in_session.as_ref()),
                                    false,
                                    "session worker cleanup",
                                );
                            }
                            crate::session_debug::lifecycle(
                                &system_id,
                                "SOCKET_SHUTDOWN",
                                "session_pipeline",
                                "cloned_session_stream",
                                Some(is_connected.as_ref()),
                                Some(is_in_session.as_ref()),
                                false,
                                "no explicit TcpStream::shutdown call; worker-owned handles are released during cleanup",
                            );
                            drop(write_stream);

                            let _ = input_handle.join();
                            let _ = ping_handle.join();
                            let _ = clip_handle.join();
                            let _ = capture_handle.join();
                            let _ = writer_handle.join();

                            backend.log_session_end_with_token(&system_id, active_request_token.as_deref(), 0.0);
                            println!("[STREAM STATE] STOPPED");
                            crate::session_debug::log(
                                &system_id,
                                "B_STREAM_STATE state=STOPPED",
                            );
                            stage_log(
                                &system_id,
                                "SESSION_DISCONNECTED",
                                &format!("device_id={} outcome=session_ended", system_id),
                            );

                            println!("[Agent] Session ended. Preparing for next request...");
                            is_in_session.store(false, Ordering::SeqCst);
                            crate::session_debug::lifecycle(
                                &system_id,
                                "IS_IN_SESSION_FALSE",
                                "session_pipeline",
                                "cloned_session_stream",
                                Some(is_connected.as_ref()),
                                Some(is_in_session.as_ref()),
                                false,
                                "normal session cleanup completed",
                            );
                            // Session ended — update the health endpoint so B's overlay hides.
                            crate::status::set_session_active(false);
                            if let Err(error) = crate::status::set_session_writer(None) {
                                eprintln!("[SESSION UI] Failed to clear session bridge: {}", error);
                            }
                            if let Err(error) = crate::status::clear_session_peer_system_id() {
                                eprintln!("[SESSION UI] Failed to clear session peer identity: {}", error);
                            }
                            thread::sleep(Duration::from_millis(500));
                            crate::session_debug::lifecycle(
                                &system_id,
                                "SESSION_PIPELINE_EXIT",
                                "session_pipeline",
                                "cloned_session_stream",
                                Some(is_connected.as_ref()),
                                Some(is_in_session.as_ref()),
                                false,
                                "normal worker return",
                            );
                            crate::session_debug::lifecycle(
                                &system_id,
                                "THREAD_EXIT",
                                "session_pipeline",
                                "session_worker_stream",
                                Some(is_connected.as_ref()),
                                Some(is_in_session.as_ref()),
                                false,
                                "session worker returning",
                            );
                                    }),
                                );
                                if let Err(payload) = worker_result {
                                    let panic_message = payload
                                        .downcast_ref::<String>()
                                        .map(String::as_str)
                                        .or_else(|| payload.downcast_ref::<&str>().copied())
                                        .unwrap_or("non-string panic payload");
                                    crate::session_debug::lifecycle(
                                        &panic_identity,
                                        "SESSION_END_REQUESTED",
                                        "session_pipeline",
                                        "cloned_session_stream",
                                        Some(panic_connected.as_ref()),
                                        Some(panic_in_session.as_ref()),
                                        false,
                                        &format!("session worker panicked: {}", panic_message),
                                    );
                                    crate::session_debug::lifecycle(
                                        &panic_identity,
                                        "THREAD_PANIC",
                                        "session_pipeline",
                                        "session_worker_stream",
                                        Some(panic_connected.as_ref()),
                                        Some(panic_in_session.as_ref()),
                                        false,
                                        &format!("panic payload: {}", panic_message),
                                    );
                                    crate::session_debug::lifecycle(
                                        &panic_identity,
                                        "SESSION_PIPELINE_EXIT",
                                        "session_pipeline",
                                        "cloned_session_stream",
                                        Some(panic_connected.as_ref()),
                                        Some(panic_in_session.as_ref()),
                                        false,
                                        &format!("panic payload: {}", panic_message),
                                    );
                                    std::panic::resume_unwind(payload);
                                } else if panic_in_session.load(Ordering::SeqCst) {
                                    crate::session_debug::lifecycle(
                                        &panic_identity,
                                        "SESSION_PIPELINE_EXIT",
                                        "session_pipeline",
                                        "cloned_session_stream",
                                        Some(panic_connected.as_ref()),
                                        Some(panic_in_session.as_ref()),
                                        false,
                                        "worker returned before clearing is_in_session",
                                    );
                                } else {
                                    crate::session_debug::lifecycle(
                                        &panic_identity,
                                        "THREAD_EXIT",
                                        "session_pipeline",
                                        "session_worker_stream",
                                        Some(panic_connected.as_ref()),
                                        Some(panic_in_session.as_ref()),
                                        false,
                                        "session worker thread returning",
                                    );
                                }
                            }); // END OF THREAD

                            if is_in_session.load(Ordering::SeqCst) {
                                crate::session_debug::lifecycle(
                                    &system_id,
                                    "OUTER_LOOP_RECONNECT_BLOCKED_BY_ACTIVE_SESSION",
                                    "run_agent_loop",
                                    "original_relay_socket",
                                    None,
                                    Some(is_in_session.as_ref()),
                                    reconnect_was_requested,
                                    "waiting for session worker cleanup",
                                );
                            }
                            while is_in_session.load(Ordering::SeqCst) {
                                thread::sleep(Duration::from_millis(200));
                            }

                            crate::session_debug::lifecycle(
                                &system_id,
                                "OUTER_LOOP_RECONNECT_REQUESTED",
                                "run_agent_loop",
                                "original_relay_socket",
                                None,
                                Some(is_in_session.as_ref()),
                                true,
                                "session worker completed; intentional reconnect requested",
                            );
                            intentional_reconnect = true;
                            break 'viewer_loop;
                        }

                        // Type 99: Session disconnect signal
                        99 => {
                            println!("[Agent] Idle state confirmed.");
                            crate::session_debug::log(
                                &system_id,
                                &format!(
                                    "[TYPE99 TRACE] component=HOST direction=RECV device_id={} session_id={} source=idle_relay_reader reason=packet_byte_99",
                                    system_id, system_id
                                ),
                            );
                            crate::session_debug::log(
                                &system_id,
                                &format!(
                                    "[TYPE99 TRACE] component=HOST direction=RECV_COMPLETE device_id={} session_id={} source=idle_relay_reader reason=idle_state_confirmed",
                                    system_id, system_id
                                ),
                            );
                        }

                        _ => {}
                    }
                }

                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                        continue 'viewer_loop;
                    }
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        crate::session_debug::lifecycle(
                            &system_id,
                            "SOCKET_EOF",
                            "run_agent_loop",
                            "original_relay_socket",
                            None,
                            Some(is_in_session.as_ref()),
                            reconnect_was_requested,
                            &format!("relay type read_exact returned EOF: {:?}", e),
                        );
                    }
                    crate::session_debug::lifecycle(
                        &system_id,
                        "SOCKET_READ_ERROR",
                        "run_agent_loop",
                        "original_relay_socket",
                        None,
                        Some(is_in_session.as_ref()),
                        reconnect_was_requested,
                        &format!("relay type read_exact failed: {:?}", e),
                    );
                    println!("[RELAY][DISCONNECT] reason=Socket read error or closed by relay: {:?}", e);
                    println!("[RELAY][DISCONNECT] system_id={}", system_id);
                    println!("[RELAY][DISCONNECT] socket_error={:?}", e);
                    println!("[RELAY][DISCONNECT] remote_closed=true");
                    stage_log(
                        &system_id,
                        "RELAY_DISCONNECTED",
                        &format!("device_id={} reason=socket_closed error={:?}", system_id, e),
                    );
                    crate::session_debug::log(
                        &system_id,
                        &format!("B_DISCONNECT error={}", e),
                    );
                    crate::session_debug::log(
                        &system_id,
                        "STATE_CHANGE old=CONNECTED new=DISCONNECTED",
                    );
                    println!("[SESSION STATE] OFFLINE");
                    break 'viewer_loop;
                }
            }
        }

        is_running_conn.store(false, Ordering::SeqCst);
        let _ = heartbeat_handle.join();

        if intentional_reconnect {
            intentional_reconnect = false;
            reconnect_attempt = 0;
            crate::status::set_reconnect_state("Reconnecting", 0, MAX_RECONNECT_ATTEMPTS);
            println!("[Agent] Pairing complete. Immediately replenishing listener socket...");
            continue;
        }

        next_reconnect_attempt(&mut reconnect_attempt);
        record_reconnect_failure(
            &system_id,
            reconnect_attempt,
            "relay_socket_closed",
            "unexpected_socket_loss",
        );
    }
}

// ============================================================
// MAIN
// ============================================================

#[cfg(test)]
mod session_feature_tests {
    use super::{
        authenticate_viewer_pin, collect_transfer_files,
        decode_audio_packet, discard_packet_bytes, is_all_zero_auth_hash,
        make_file_error_packet, next_reconnect_attempt, read_next_file_chunk,
        read_packet_payload, reconnect_delay, send_session_file,
        should_create_empty_transfer_file, verify_viewer_pin, viewer_pin_sha256,
        ViewerAuthResult, AUTH_PIN_INVALID, AUTH_PIN_MISSING, AUTH_PROTOCOL_MISMATCH,
    };
    use std::io::{self, Read};
    use std::sync::mpsc::{sync_channel, TryRecvError};
    use std::time::Duration;
    use sha2::Digest;

    #[test]
    fn host_approval_workflow_authenticates_valid_token_and_rejects_unapproved_or_replayed() {
        use crate::status::{clear_approved_sessions, record_approved_session, check_and_consume_approved_session};
        clear_approved_sessions();

        let token = "approved-token-12345";
        let valid_hash: [u8; 32] = sha2::Sha256::digest(token.as_bytes()).into();
        let unapproved_hash: [u8; 32] = sha2::Sha256::digest(b"unapproved-token").into();
        let zero_hash = [0u8; 32];

        // 1. Before approval: rejected
        assert!(!check_and_consume_approved_session(&valid_hash));
        assert!(!check_and_consume_approved_session(&zero_hash));

        // 2. Host approves request
        record_approved_session(token, "requester-device-999")
            .expect("approved session should be recorded");

        // 3. Unapproved and zero hashes rejected
        assert!(!check_and_consume_approved_session(&unapproved_hash));
        assert!(!check_and_consume_approved_session(&zero_hash));

        // 4. Approved token accepted
        assert!(check_and_consume_approved_session(&valid_hash));

        // 5. Replay attempt rejected (single-use)
        assert!(!check_and_consume_approved_session(&valid_hash));
    }

    #[test]
    fn viewer_authentication_requires_a_configured_matching_pin() {
        let supplied_hash: [u8; 32] = sha2::Sha256::digest(b"correct-pin").into();
        let invalid_hash: [u8; 32] = sha2::Sha256::digest(b"wrong-pin").into();

        assert!(verify_viewer_pin("correct-pin", &supplied_hash));
        assert!(!verify_viewer_pin("wrong-pin", &supplied_hash));
        assert!(!verify_viewer_pin("correct-pin", &invalid_hash));
        assert!(!verify_viewer_pin("", &supplied_hash));
        assert!(!verify_viewer_pin("   ", &supplied_hash));
    }

    #[test]
    fn missing_host_pin_rejects_zero_and_empty_string_digests() {
        let zeros = [0u8; 32];
        let empty_digest = viewer_pin_sha256("");
        assert!(!is_all_zero_auth_hash(&empty_digest));
        assert_eq!(
            authenticate_viewer_pin("", &zeros),
            ViewerAuthResult::Rejected(AUTH_PIN_MISSING)
        );
        assert_eq!(
            authenticate_viewer_pin("", &empty_digest),
            ViewerAuthResult::Rejected(AUTH_PIN_MISSING)
        );
        assert_eq!(
            authenticate_viewer_pin("", &viewer_pin_sha256("not-empty")),
            ViewerAuthResult::Rejected(AUTH_PIN_MISSING)
        );
        assert_eq!(
            authenticate_viewer_pin("   ", &zeros),
            ViewerAuthResult::Rejected(AUTH_PIN_MISSING)
        );
    }

    #[test]
    fn configured_pin_rejects_zero_hash_and_wrong_digest() {
        let zeros = [0u8; 32];
        assert_eq!(
            authenticate_viewer_pin("secret", &zeros),
            ViewerAuthResult::Rejected(AUTH_PROTOCOL_MISMATCH)
        );
        assert_eq!(
            authenticate_viewer_pin("secret", &viewer_pin_sha256("other")),
            ViewerAuthResult::Rejected(AUTH_PIN_INVALID)
        );
        assert_eq!(
            authenticate_viewer_pin("secret", &viewer_pin_sha256("secret")),
            ViewerAuthResult::Accepted
        );
    }

    #[test]
    fn invalid_session_tokens_and_digests_are_rejected() {
        let pin = "987654";
        let valid_hash = viewer_pin_sha256(pin);
        
        // Single bit flip
        let mut corrupted = valid_hash;
        corrupted[15] ^= 0x01;
        assert_eq!(
            authenticate_viewer_pin(pin, &corrupted),
            ViewerAuthResult::Rejected(AUTH_PIN_INVALID)
        );

        // All zeros
        assert_eq!(
            authenticate_viewer_pin(pin, &[0u8; 32]),
            ViewerAuthResult::Rejected(AUTH_PROTOCOL_MISMATCH)
        );

        // Empty PIN on host rejects any token
        assert_eq!(
            authenticate_viewer_pin("", &valid_hash),
            ViewerAuthResult::Rejected(AUTH_PIN_MISSING)
        );
    }

    #[test]
    fn default_host_pin_authenticates_valid_hash_and_rejects_invalid_hash() {
        use crate::identity::device_id::DEFAULT_PIN;
        let valid_hash = viewer_pin_sha256(DEFAULT_PIN);
        let wrong_hash = viewer_pin_sha256("wrong-pin");
        let zero_hash = [0u8; 32];

        // Valid PIN digest is accepted
        assert_eq!(
            authenticate_viewer_pin(DEFAULT_PIN, &valid_hash),
            ViewerAuthResult::Accepted
        );

        // Wrong PIN digest is rejected
        assert_eq!(
            authenticate_viewer_pin(DEFAULT_PIN, &wrong_hash),
            ViewerAuthResult::Rejected(AUTH_PIN_INVALID)
        );

        // All-zero digest is rejected as protocol mismatch
        assert_eq!(
            authenticate_viewer_pin(DEFAULT_PIN, &zero_hash),
            ViewerAuthResult::Rejected(AUTH_PROTOCOL_MISMATCH)
        );
    }

    #[test]
    fn incorrect_target_device_id_format_is_detected() {
        let is_valid_target_id = |id: &str| -> bool {
            let clean: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
            clean.len() == 9 && id.chars().all(|c| c.is_ascii_digit() || c == ' ')
        };

        assert!(is_valid_target_id("123456789"));
        assert!(is_valid_target_id("123 456 789"));
        assert!(!is_valid_target_id("12345678"));
        assert!(!is_valid_target_id("1234567890"));
        assert!(!is_valid_target_id("12345678a"));
        assert!(!is_valid_target_id(""));
    }

    #[test]
    fn transient_reconnect_attempts_continue_past_the_old_limit() {
        let mut attempt = 0;
        for expected in 1..=100 {
            assert_eq!(next_reconnect_attempt(&mut attempt), expected);
        }
        let delay = reconnect_delay(attempt);
        assert!((Duration::from_secs(8)..=Duration::from_secs(10)).contains(&delay));
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "test read failure"))
        }
    }

    struct ChunkedReader<R> {
        inner: R,
        max_read: usize,
    }

    impl<R: Read> Read for ChunkedReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let limit = buffer.len().min(self.max_read);
            self.inner.read(&mut buffer[..limit])
        }
    }

    #[test]
    fn type_17_audio_packet_decodes_interleaved_samples() {
        let mut payload = Vec::new();
        for sample in [0.25f32, 0.25, -0.5, -0.5] {
            payload.extend_from_slice(&sample.to_le_bytes());
        }
        assert_eq!(
            decode_audio_packet(&payload, 48_000, 2, 48_000).unwrap(),
            vec![0.25, -0.5]
        );
        assert!(decode_audio_packet(&payload[..3], 48_000, 2, 48_000).is_none());
    }

    #[test]
    fn file_sender_distinguishes_eof_from_read_failure() {
        let mut eof_reader = io::Cursor::new(Vec::<u8>::new());
        assert_eq!(read_next_file_chunk(&mut eof_reader, &mut [0u8; 8]).unwrap(), None);

        let mut failing_reader = FailingReader;
        assert_eq!(
            read_next_file_chunk(&mut failing_reader, &mut [0u8; 8])
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn file_read_error_uses_existing_type_24_frame() {
        let packet = make_file_error_packet(0x0102_0304_0506_0708, "read failed");
        assert_eq!(packet[0], 24);
        assert_eq!(&packet[1..9], &0x0102_0304_0506_0708u64.to_be_bytes());
        assert_eq!(&packet[9..11], &[0, 0]);
        assert_eq!(u16::from_be_bytes([packet[11], packet[12]]), 11);
        assert_eq!(&packet[13..], b"read failed");
    }

    #[test]
    fn native_sender_waits_for_acceptance_and_completion_ack() {
        for contents in [Vec::new(), b"small transfer".to_vec(), vec![0x5a; 700_000]] {
            let source = std::env::temp_dir().join(format!(
                "DeskStream-send-test-{}-{}.bin",
                std::process::id(),
                rand::random::<u64>()
            ));
            std::fs::write(&source, &contents).unwrap();
            let (writer, receiver) = sync_channel(8);
            let send_source = source.clone();
            let sender = std::thread::spawn(move || {
                send_session_file(&writer, "test-session", &send_source, "test.bin")
            });

            let offer = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(offer[0], 20);
            assert_eq!(u64::from_be_bytes(offer[9..17].try_into().unwrap()), contents.len() as u64);
            let transfer_id = u64::from_be_bytes(offer[1..9].try_into().unwrap());
            assert_eq!(u16::from_be_bytes([offer[17], offer[18]]) as usize, 8);
            assert_eq!(&offer[19..], b"test.bin");
            assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));

            crate::status::resolve_session_file_response(transfer_id, 26);
            let mut received = Vec::new();
            let mut expected_chunk_index = 0u32;
            loop {
                let packet = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
                assert_eq!(u64::from_be_bytes(packet[1..9].try_into().unwrap()), transfer_id);
                match packet[0] {
                    21 => {
                        let chunk_index = u32::from_be_bytes(packet[9..13].try_into().unwrap());
                        let chunk_len = u32::from_be_bytes(packet[13..17].try_into().unwrap()) as usize;
                        assert_eq!(chunk_index, expected_chunk_index);
                        assert_eq!(packet.len(), 17 + chunk_len);
                        received.extend_from_slice(&packet[17..]);
                        expected_chunk_index += 1;
                    }
                    22 => {
                        assert_eq!(packet.len(), 49);
                        assert_eq!(u64::from_be_bytes(packet[9..17].try_into().unwrap()), contents.len() as u64);
                        let digest = sha2::Sha256::digest(&received);
                        assert_eq!(&packet[17..49], digest.as_slice());
                        assert_eq!(received, contents);
                        assert!(!sender.is_finished());
                        crate::status::resolve_session_file_response(transfer_id, 27);
                        break;
                    }
                    packet_type => panic!("unexpected sender packet type {packet_type}"),
                }
            }

            sender.join().unwrap().unwrap();
            std::fs::remove_file(source).unwrap();
        }
    }

    #[test]
    fn native_sender_decisions_are_routed_by_transfer_id() {
        let first_source = std::env::temp_dir().join(format!(
            "DeskStream-route-test-{}-{}.bin",
            std::process::id(),
            rand::random::<u64>()
        ));
        let second_source = std::env::temp_dir().join(format!(
            "DeskStream-route-test-{}-{}.bin",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::write(&first_source, b"accepted payload").unwrap();
        std::fs::write(&second_source, b"rejected payload").unwrap();
        let (writer, receiver) = sync_channel(8);
        let first_writer = writer.clone();
        let first_path = first_source.clone();
        let first_sender = std::thread::spawn(move || {
            send_session_file(&first_writer, "test-session", &first_path, "first.bin")
        });
        let second_path = second_source.clone();
        let second_sender = std::thread::spawn(move || {
            send_session_file(&writer, "test-session", &second_path, "second.bin")
        });

        let offers = [
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
            receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        ];
        fn offered_filename(offer: &[u8]) -> &[u8] {
            let name_len = u16::from_be_bytes(offer[17..19].try_into().unwrap()) as usize;
            &offer[19..19 + name_len]
        }
        let (first_offer, second_offer) = if offered_filename(&offers[0]) == b"first.bin" {
            (&offers[0], &offers[1])
        } else {
            (&offers[1], &offers[0])
        };
        assert_eq!(offered_filename(first_offer), b"first.bin");
        assert_eq!(offered_filename(second_offer), b"second.bin");
        assert_eq!(first_offer[0], 20);
        assert_eq!(second_offer[0], 20);
        let first_id = u64::from_be_bytes(first_offer[1..9].try_into().unwrap());
        let second_id = u64::from_be_bytes(second_offer[1..9].try_into().unwrap());
        assert_ne!(first_id, second_id);
        assert!(!crate::status::resolve_session_file_response(u64::MAX, 26));
        assert!(crate::status::resolve_session_file_response(first_id, 26));
        assert!(crate::status::resolve_session_file_response(second_id, 25));

        let mut delivered = Vec::new();
        loop {
            let packet = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            let transfer_id = u64::from_be_bytes(packet[1..9].try_into().unwrap());
            assert_eq!(transfer_id, first_id);
            match packet[0] {
                21 => delivered.extend_from_slice(&packet[17..]),
                22 => {
                    assert_eq!(delivered, b"accepted payload");
                    assert!(crate::status::resolve_session_file_response(first_id, 27));
                    break;
                }
                packet_type => panic!("unexpected packet after approval routing: {packet_type}"),
            }
        }

        first_sender.join().unwrap().unwrap();
        assert!(second_sender.join().unwrap().unwrap_err().contains("was rejected"));
        std::fs::remove_file(first_source).unwrap();
        std::fs::remove_file(second_source).unwrap();
    }

    #[test]
    fn folder_transfer_prefixes_root_once_and_preserves_nested_paths() {
        let root = std::env::temp_dir().join(format!(
            "DeskStream-folder-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("top.txt"), b"top").unwrap();
        std::fs::write(nested.join("child.txt"), b"child").unwrap();

        let result = collect_transfer_files(&root);
        let cleanup = std::fs::remove_dir_all(&root);
        let files = result.unwrap();
        cleanup.unwrap();
        let root_name = root.file_name().unwrap().to_string_lossy();
        let mut relative_paths = files
            .iter()
            .map(|(_, relative)| relative.as_str())
            .collect::<Vec<_>>();
        relative_paths.sort_unstable();
        assert_eq!(
            relative_paths,
            vec![
                format!("{root_name}/nested/child.txt"),
                format!("{root_name}/top.txt")
            ]
        );
        assert!(files.iter().all(|(path, _)| path.is_absolute()));
    }

    #[test]
    fn accepted_empty_file_completion_requires_an_empty_destination_file() {
        assert!(should_create_empty_transfer_file(true, true, 0, false));
        assert!(!should_create_empty_transfer_file(false, true, 0, false));
        assert!(!should_create_empty_transfer_file(true, false, 0, false));
        assert!(!should_create_empty_transfer_file(true, true, 1, false));
        assert!(!should_create_empty_transfer_file(true, true, 0, true));
    }

    #[test]
    fn type16_payload_reads_handle_partial_reads_and_adjacent_packets() {
        let mut reader = ChunkedReader {
            inner: io::Cursor::new(b"firstsecond".to_vec()),
            max_read: 2,
        };
        assert_eq!(read_packet_payload(&mut reader, 5).unwrap(), b"first");
        assert_eq!(read_packet_payload(&mut reader, 6).unwrap(), b"second");
    }

    #[test]
    fn discarding_rejected_packet_payload_preserves_next_packet() {
        let mut reader = io::Cursor::new(b"oversized-payloadNEXT".to_vec());
        discard_packet_bytes(&mut reader, b"oversized-payload".len()).unwrap();
        let mut next_packet = [0u8; 4];
        reader.read_exact(&mut next_packet).unwrap();
        assert_eq!(&next_packet, b"NEXT");
    }

    #[test]
    fn discarding_truncated_packet_payload_reports_eof() {
        let mut reader = io::Cursor::new(b"short".to_vec());
        assert_eq!(
            discard_packet_bytes(&mut reader, 10).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}

fn main() {
    // ---------------------------------------------------------
    // PHASE 8: ENTERPRISE BOOT LOGGER & PHASE 4: PROVISIONING
    // ---------------------------------------------------------
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\temp".to_string());
    let deskstream_dir = std::path::Path::new(&local_app_data).join("DeskStream");
    let _ = std::fs::create_dir_all(&deskstream_dir);
    
    // Create the boot logger in the %TEMP% directory as requested (or a subfolder there)
    let temp_dir = std::env::var("TEMP").unwrap_or_else(|_| "C:\\temp".to_string());
    let boot_log_path = std::path::Path::new(&temp_dir).join("DeskStream-boot.log");

    // Clear old boot log or keep appending if we prefer
    let mut boot_log = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&boot_log_path)
        .expect("Failed to open boot log");

    use std::io::Write;
    let now = || -> String {
        // Very basic ISO-like format using SystemTime (for standard Rust without chrono)
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        format!("[{}]", secs)
    };

    let _ = writeln!(boot_log, "{} DeskStream starting", now());
    let _ = writeln!(boot_log, "{} Version: {}", now(), env!("CARGO_PKG_VERSION"));
    
    if let Ok(exe_path) = std::env::current_exe() {
        let _ = writeln!(boot_log, "{} Executable: {}", now(), exe_path.display());
        if let Some(parent) = exe_path.parent() {
            let _ = std::env::set_current_dir(parent);
        }
    }
    
    let _ = writeln!(boot_log, "{} Operating System: Windows", now());
    let _ = writeln!(boot_log, "{} Architecture: x64", now());
    let _ = writeln!(boot_log, "{} User data: {}", now(), deskstream_dir.display());

    // Single-instance enforcement via port 49182
    let listener = match std::net::TcpListener::bind("127.0.0.1:49182") {
        Ok(listener) => listener,
        Err(_) => {
            // Another instance is already running — bring its window to the foreground
            #[cfg(windows)]
            unsafe {
                let title: Vec<u16> = "DeskStream Agent\0".encode_utf16().collect();
                let main_hwnd = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                    std::ptr::null(),
                    title.as_ptr(),
                );
                if main_hwnd != 0 {
                    windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                        main_hwnd,
                        windows_sys::Win32::UI::WindowsAndMessaging::SW_RESTORE,
                    );
                    windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(main_hwnd);
                }
            }
            agent_log!("[Agent] Another DeskStream instance is running — brought to foreground.");
            std::process::exit(0);
        }
    };

    set_process_dpi_aware();

    let args: Vec<String> = env::args().collect();

    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "34.207.168.24:9001".to_string()
    };

    // FIRST-LAUNCH CONFIG PROVISIONING
    let config_path = deskstream_dir.join("config.json");
    if !config_path.exists() {
        let default_config = r#"{
    "version": 1,
    "initialized": true
}"#;
        let _ = std::fs::write(&config_path, default_config);
    }
    let _ = writeln!(boot_log, "{} Configuration initialized", now());

    let keys_path = deskstream_dir.join("keys.json");
    if !keys_path.exists() {
        let _ = std::fs::write(&keys_path, "{}");
    }
    let env_path = deskstream_dir.join(".env");
    if !env_path.exists() {
        let _ = std::fs::write(&env_path, "DESKSTREAM_SIGNING_ENABLED=false\n");
    }
    let _ = writeln!(boot_log, "{} Environment initialized", now());

    // Load persistent identity
    let config = identity::device_id::AgentConfig::load_or_create("", &relay_addr);
    let relay_addr = config.relay_addr.clone();

    // Initialise shared log buffer (bounded, 500 lines)
    let _log_buf: Arc<Mutex<std::collections::VecDeque<String>>> =
        Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(500)));

    // Initialise LIVE status global
    let initial_status = crate::status::AgentStatus::new(&config.system_id, &config.name);
    let _ = crate::status::LIVE.set(Arc::new(Mutex::new(initial_status)));

    let current_exe_path = env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();

    agent_log!("========================================");
    agent_log!("       REMOTE DESKTOP AGENT STARTING");
    agent_log!("  EXECUTABLE:    {}", current_exe_path);
    agent_log!("========================================");

    // Health server on port 49182 — keep existing IPC
    let sys_id_for_health = config.system_id.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut buf = [0; 1024];
                if let Ok(n) = stream.read(&mut buf) {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let is_options = request.starts_with("OPTIONS");
                    
                    let mut is_discover = false;
                    let mut target_id = String::new();
                    if let Some(path_start) = request.find("GET /") {
                        let path_part = &request[path_start + 4..];
                        if let Some(space_idx) = path_part.find(' ') {
                            let path = &path_part[..space_idx];
                            if path.starts_with("/discover?target=") {
                                is_discover = true;
                                target_id = path.trim_start_matches("/discover?target=").to_string();
                            }
                        }
                    }

                    let response = if is_options {
                        // CORS preflight response
                        "HTTP/1.1 204 No Content\r\n\
                         Access-Control-Allow-Origin: *\r\n\
                         Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                         Access-Control-Allow-Headers: Content-Type\r\n\
                         Access-Control-Allow-Private-Network: true\r\n\
                         Access-Control-Max-Age: 86400\r\n\
                         Connection: close\r\n\
                         \r\n".to_string()
                    } else if is_discover {
                        if let Some((ip, port)) = crate::network::discovery::discover_target_agent(&target_id) {
                            format!(
                                "HTTP/1.1 200 OK\r\n\
                                 Content-Type: application/json\r\n\
                                 Access-Control-Allow-Origin: *\r\n\
                                 Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                                 Access-Control-Allow-Headers: Content-Type\r\n\
                                 Access-Control-Allow-Private-Network: true\r\n\
                                 Connection: close\r\n\
                                 \r\n\
                                 {{\"status\": \"found\", \"ip\": \"{}\", \"port\": {}}}",
                                ip, port
                            )
                        } else {
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: application/json\r\n\
                             Access-Control-Allow-Origin: *\r\n\
                             Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                             Access-Control-Allow-Headers: Content-Type\r\n\
                             Access-Control-Allow-Private-Network: true\r\n\
                             Connection: close\r\n\
                             \r\n\
                             {\"status\": \"not_found\"}".to_string()
                        }
                    } else {
                        // Normal GET response with JSON body
                        format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: application/json\r\n\
                             Access-Control-Allow-Origin: *\r\n\
                             Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                             Access-Control-Allow-Headers: Content-Type\r\n\
                             Access-Control-Allow-Private-Network: true\r\n\
                             Connection: close\r\n\
                             \r\n\
                             {{\"running\": true, \"system_id\": \"{}\", \"status\": \"online\"}}",
                            sys_id_for_health
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                }
            }
        }
    });

    agent_log!("========================================");
    agent_log!("       REMOTE DESKTOP AGENT (60 FPS)");
    agent_log!("  BUILD VERSION: 1.1.3 (Production Desktop Agent)");
    agent_log!("  EXECUTABLE:    {}", current_exe_path);
    agent_log!("========================================");
    agent_log!("  Device UUID: {}", config.device_uuid);
    agent_log!("  System ID:   {}", config.formatted_id());
    agent_log!("  Relay: {}", relay_addr);
    agent_log!("========================================");

    crate::network::discovery::start_discovery_listener(config.system_id.clone());

    // Spawn the complete agent engine on a background thread
    // The WebView UI runs on the main thread (required by Win32/WebView2).
    let agent_config = config.clone();
    let agent_relay_addr = relay_addr.clone();
    thread::spawn(move || {
        run_agent_loop(agent_relay_addr, agent_config);
    });

    // ============================================================
    // WEBVIEW2 GUI — runs on main thread
    // Shows the exact DeskStream website design inside a native window.
    // Agent engine runs independently on background threads.
    // ============================================================
    let quit_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Start system tray icon
    tray::start_tray(quit_signal.clone());

    // Start embedded local HTTP server for the UI and API Proxy
    let _ = writeln!(boot_log, "{} Backend starting", now());
    let local_port = local_server::start_local_server(config.system_id.clone(), quit_signal.clone());
    let local_url = format!("http://127.0.0.1:{}/dashboard.html", local_port);
    agent_log!("[BOOT] Started local embedded UI server on {}", local_url);

    // Health Poll Loop
    let mut healthy = false;
    for attempt in 1..=60 { // 30 seconds max
        let _ = writeln!(boot_log, "{} Health check attempt {}", now(), attempt);
        if let Ok(resp) = reqwest::blocking::get(format!("http://127.0.0.1:{}/health", local_port)) {
            if resp.status().is_success() {
                healthy = true;
                let _ = writeln!(boot_log, "{} Backend healthy", now());
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    if healthy {
        let _ = writeln!(boot_log, "{} Main window created", now());
        webview_app::run_webview(quit_signal, local_url);
    } else {
        let _ = writeln!(boot_log, "{} Fatal startup error: Backend unhealthy after 30 seconds", now());
        let error_html = format!(
            "data:text/html;charset=utf-8,<html><head><title>Startup Error</title><style>
            body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; padding: 40px; background: #0f172a; color: #f8fafc; }}
            h2 {{ font-weight: 600; color: #f1f5f9; }}
            .container {{ max-width: 600px; margin: 40px auto; background: #1e293b; padding: 30px; border-radius: 12px; box-shadow: 0 10px 15px -3px rgba(0,0,0,0.1); border: 1px solid #334155; }}
            pre {{ background: #0f172a; padding: 15px; border-radius: 8px; overflow-x: auto; font-size: 13px; color: #cbd5e1; border: 1px solid #334155; }}
            .btn {{ display: inline-block; background: #3b82f6; color: white; padding: 8px 16px; border-radius: 6px; text-decoration: none; font-weight: 500; margin-right: 12px; font-size: 14px; }}
            .btn-outline {{ background: transparent; border: 1px solid #475569; color: #e2e8f0; }}
            </style></head><body>
            <div class='container'>
            <h2>DeskStream</h2>
            <p>Unable to start the local service.</p>
            <p><strong>Error:</strong> Local backend health check timed out.</p>
            <p><strong>Diagnostic log:</strong></p>
            <pre>{}</pre>
            <div style='margin-top: 24px;'>
                <a href='#' onclick='window.location.reload()' class='btn'>Retry</a>
                <a href='#' class='btn btn-outline'>Exit</a>
            </div>
            </div></body></html>",
            boot_log_path.to_string_lossy().replace("\\", "/")
        );
        webview_app::run_webview(quit_signal, error_html);
    }
}
