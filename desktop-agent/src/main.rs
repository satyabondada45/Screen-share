// Hardware H264 Video Pipeline (120 FPS Ultra-Low Latency)
pub mod registration {
    pub mod backend_client;
}
pub mod encoder;
pub mod identity;

use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use enigo::{Axis, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use encoder::HardwareH264Encoder;
use screenshots::Screen;
use sha2::{Digest, Sha256};

use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ============================================================
// VIDEO SETTINGS (120 FPS Ultra-Low Latency)
// ============================================================

const TARGET_FPS: u32 = 120;
const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;
const TARGET_BITRATE: u32 = 8_000_000;
const FRAME_INTERVAL_MICROS: u64 = 8333; // 120 FPS ~ 8.333 ms per frame

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

#[cfg(windows)]
fn send_native_mouse_click(flags: u32) {
    unsafe {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::mouse_event(flags, 0, 0, 0, 0);
    }
}

#[cfg(windows)]
fn send_native_mouse_wheel(scroll_y: i16) {
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_WHEEL;
        windows_sys::Win32::UI::Input::KeyboardAndMouse::mouse_event(MOUSEEVENTF_WHEEL, 0, 0, scroll_y as i32, 0);
    }
}

#[cfg(windows)]
fn send_native_key(vk: u32, is_up: bool) {
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
        };
        let mut flags = if is_up { KEYEVENTF_KEYUP } else { 0 };
        // Extended keys on Windows: arrows (37-40), PageUp/Dn (33-34), End (35), Home (36), Insert (45), Delete (46), Windows key (91-92)
        if matches!(vk, 33..=46 | 91..=93 | 144 | 145) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        keybd_event(vk as u8, 0, flags, 0);
    }
}

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
// AUTOSTART
// ============================================================

#[cfg(windows)]
fn enable_autostart(app_name: &str) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegSetValueExW, HKEY_CURRENT_USER, REG_SZ,
    };

    let current_exe = env::current_exe().map_err(|e| e.to_string())?;

    let target_path = if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let bin_dir = std::path::Path::new(&local_app_data).join("DeskStream").join("bin");
        let _ = fs::create_dir_all(&bin_dir);
        let target_exe = bin_dir.join("desktop-agent.exe");
        if current_exe != target_exe {
            let _ = fs::copy(&current_exe, &target_exe);
        }
        target_exe
    } else {
        current_exe
    };

    let exe_path_str = target_path.to_str().ok_or("Invalid path")?;
    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
        .encode_utf16()
        .collect();

    let name_utf16: Vec<u16> = format!("{}\0", app_name).encode_utf16().collect();
    let val_utf16: Vec<u16> = format!("\"{}\"\0", exe_path_str).encode_utf16().collect();

    unsafe {
        let mut key = 0;
        if RegCreateKeyW(HKEY_CURRENT_USER, subkey.as_ptr(), &mut key) != 0 {
            return Err("Failed to open registry key".to_string());
        }
        let res = RegSetValueExW(
            key,
            name_utf16.as_ptr(),
            0,
            REG_SZ,
            val_utf16.as_ptr() as *const u8,
            (val_utf16.len() * 2) as u32,
        );
        RegCloseKey(key);
        if res == 0 {
            Ok(exe_path_str.to_string())
        } else {
            Err(format!("RegSetValueExW failed with code {}", res))
        }
    }
}

#[cfg(not(windows))]
fn enable_autostart(_app_name: &str) -> Result<String, String> {
    Ok("non-windows".to_string())
}

// ============================================================
// CONNECTION DIALOG
// ============================================================



// ============================================================
// SHA256
// ============================================================

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

// ============================================================
// KEY MAPPING
// ============================================================

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

        16 => Some(Key::Shift),
        17 => Some(Key::Control),
        18 => Some(Key::Alt),

        37 => Some(Key::LeftArrow),
        38 => Some(Key::UpArrow),
        39 => Some(Key::RightArrow),
        40 => Some(Key::DownArrow),

        46 => Some(Key::Delete),

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
        })
    }
}

// ============================================================
// AUDIO
// ============================================================

fn start_audio_capture(
    write_stream: std::sync::mpsc::SyncSender<Vec<u8>>,
    is_running: Arc<AtomicBool>,
) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_input_device()?;
    let config = device.default_input_config().ok()?;
    let stream_config: cpal::StreamConfig = config.clone().into();
    let sample_rate = stream_config.sample_rate.0;
    let channels = stream_config.channels;

    println!(
        "[Audio] Microphone active: {} Hz, {} channels",
        sample_rate, channels
    );

    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &_| {
                if !is_running.load(Ordering::SeqCst) {
                    return;
                }

                let byte_len = data.len() * std::mem::size_of::<f32>();

                let mut packet = Vec::with_capacity(11 + byte_len);
                packet.push(17u8);
                packet.extend_from_slice(&(byte_len as u32).to_be_bytes());
                packet.extend_from_slice(&(sample_rate as u32).to_be_bytes());
                packet.extend_from_slice(&(channels as u16).to_be_bytes());

                let slice = unsafe {
                    std::slice::from_raw_parts(data.as_ptr() as *const u8, byte_len)
                };
                packet.extend_from_slice(slice);

                let _ = write_stream.try_send(packet);
            },
            |_error| {},
            None,
        )
        .ok()?;

    stream.play().ok()?;
    Some(stream)
}

// ============================================================
// AGENT LOOP
// ============================================================

fn run_agent_loop(relay_addr: String, config: identity::device_id::AgentConfig) {
    let host_ip = relay_addr.split(':').next().unwrap_or("127.0.0.1");
    let backend_url = format!("http://{}/Screen%20Share/backend/api", host_ip);

    // Use persistent device UUID as the machine identifier
    // Use the config system_id (deterministic from UUID) as the initial system_id hint
    let mut backend = registration::backend_client::BackendClient::new(
        &backend_url,
        &config.device_uuid,
        &config.system_id,
    );

    println!("[DISCOVERY] Registering with backend: {}", backend_url);
    let system_id = if let Some(assigned_id) = backend.register() {
        println!("[DISCOVERY] Registered successfully with Web Dashboard / Device Registry!");
        println!("[DISCOVERY] Backend assigned System ID: {}", assigned_id);
        assigned_id
    } else {
        println!("[DISCOVERY] Running in standalone relay mode.");
        config.system_id.clone()
    };

    let id_str = {
        let clean: String = system_id.chars().filter(|c| c.is_ascii_digit()).collect();
        if clean.len() == 9 {
            format!("{} {} {}", &clean[0..3], &clean[3..6], &clean[6..9])
        } else {
            clean.clone()
        }
    };

    println!("[IDENTITY] Device ID: {}", system_id);
    println!("[IDENTITY] Device UUID:  {}", config.device_uuid);
    println!("[IDENTITY] System ID:    {}", system_id);
    println!("[IDENTITY] Display ID:   {}", id_str);

    backend.start_heartbeat_thread();

    let mut backoff_secs = 1;

    loop {
        println!("[RELAY] Connecting to relay {}...", relay_addr);

        let mut stream = match TcpStream::connect(&relay_addr) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                s
            }
            Err(e) => {
                eprintln!("[RELAY] Connection failed: {:?}", e);
                println!("[SESSION STATE] OFFLINE");
                println!("[RELAY][DISCONNECT] reason=Connection refused or network unreachable");
                println!("[RELAY][DISCONNECT] system_id={}", system_id);
                println!("[RELAY][DISCONNECT] socket_error={:?}", e);
                println!("[RELAY][DISCONNECT] remote_closed=false");
                thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
                continue;
            }
        };

        println!("[RELAY] TCP connection established");
        println!("[RELAY] Sending registration for System ID: {}", system_id);

        // Registration (Type 1 + System ID as relay session key)
        let mut register_pkt = Vec::with_capacity(1 + system_id.len());
        register_pkt.push(1u8);
        register_pkt.extend_from_slice(system_id.as_bytes());

        if stream.write_all(&register_pkt).is_err() {
            eprintln!("[RELAY] Failed to send registration packet");
            println!("[SESSION STATE] OFFLINE");
            thread::sleep(Duration::from_secs(backoff_secs));
            backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
            continue;
        }

        let mut ack = [0u8; 1];
        if stream.read_exact(&mut ack).is_err() || ack[0] != 1 {
            eprintln!("[RELAY] Registration failed or unacknowledged by relay");
            println!("[SESSION STATE] OFFLINE");
            thread::sleep(Duration::from_secs(backoff_secs));
            backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
            continue;
        }

        println!("[RELAY] Registration ACK received");
        println!("[SESSION STATE] ONLINE");
        backoff_secs = 1;

        let idle_write_stream = match stream.try_clone() {
            Ok(s) => Arc::new(Mutex::new(s)),
            Err(e) => {
                eprintln!("[Agent] Failed to clone stream for writer: {:?}", e);
                continue;
            }
        };

        let is_running_conn = Arc::new(AtomicBool::new(true));
        let is_in_session = Arc::new(AtomicBool::new(false));

        // Heartbeat thread: Sends Type 14 to relay every 5s during idle state
        let hb_write = Arc::clone(&idle_write_stream);
        let hb_running = Arc::clone(&is_running_conn);
        let hb_in_session = Arc::clone(&is_in_session);

        let heartbeat_handle = thread::spawn(move || {
            while hb_running.load(Ordering::SeqCst) {
                if !hb_in_session.load(Ordering::SeqCst) {
                    let mut ping_pkt = Vec::with_capacity(9);
                    ping_pkt.push(14u8);
                    ping_pkt.extend_from_slice(&current_time_millis().to_be_bytes());

                    if let Ok(mut writer) = hb_write.lock() {
                        let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
                        let _ = writer.write_all(&ping_pkt);
                    }
                }
                thread::sleep(Duration::from_secs(5));
            }
        });

        'viewer_loop: loop {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut type_buf = [0u8; 1];

            match stream.read_exact(&mut type_buf) {
                Ok(_) => {
                    match type_buf[0] {
                        // Type 14: Heartbeat ACK from relay
                        14 => {
                            let mut time_buf = [0u8; 8];
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            if stream.read_exact(&mut time_buf).is_ok() {
                                println!("[HEARTBEAT] -> {}", system_id);
                                println!("[HEARTBEAT] <- ACK");
                            }
                        }

                        // Type 3: Incoming session request / Authentication
                        3 => {
                            let mut auth_hash = [0u8; 32];
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                            if stream.read_exact(&mut auth_hash).is_err() {
                                eprintln!("[Agent] Failed to read auth hash from relay.");
                                break 'viewer_loop;
                            }

                            println!("[Host] Received connection request for Target ID: {}", id_str);
                            println!("[SESSION] ACCEPT received");
                            println!("[SESSION] Starting remote session");
                            println!("[STREAM STATE] STARTING");
                            println!("[STREAM] Starting screen capture");
                            println!("[STREAM] Starting H.264 encoder");

                            is_in_session.store(true, Ordering::SeqCst);

                            // Send approval response [1u8]
                            if let Ok(mut writer) = idle_write_stream.lock() {
                                let _ = writer.set_write_timeout(Some(Duration::from_secs(5)));
                                let _ = writer.write_all(&[1u8]);
                            }

                            println!("[Host] Approval response sent successfully (APPROVED)");
                            backend.log_session_start(&system_id);
                            println!("[Agent] Session APPROVED! Starting live video...");

                            // ====================================================
                            // CONNECTION STATE & STREAMING PIPELINE
                            // ====================================================

                            let is_connected = Arc::new(AtomicBool::new(true));
                            let is_conn_read = Arc::clone(&is_connected);
                            let is_conn_write = Arc::clone(&is_connected);
                            let is_conn_capture = Arc::clone(&is_connected);
                            let is_conn_clip = Arc::clone(&is_connected);
                            let is_conn_audio = Arc::clone(&is_connected);
                            let is_conn_ping = Arc::clone(&is_connected);

                            // TCP INPUT
                            let mut read_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = read_stream.set_read_timeout(None);

                            // OUTPUT QUEUE
                            let (out_tx, out_rx) = sync_channel::<Vec<u8>>(128);
                            let write_stream = out_tx.clone();
                            let mut write_tcp = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = write_tcp.set_write_timeout(None);

                            let write_connected = Arc::clone(&is_connected);

                            println!("[STREAM STATE] ACTIVE");
                            println!("[VIDEO STATE] ACTIVE");

                            let writer_handle = thread::spawn(move || {
                                while write_connected.load(Ordering::Acquire) {
                                    match out_rx.recv_timeout(Duration::from_millis(100)) {
                                        Ok(packet) => {
                                            match write_tcp.write_all(&packet) {
                                                Ok(_) => {}
                                                Err(e) => {
                                                    if e.kind() == std::io::ErrorKind::WouldBlock
                                                        || e.kind() == std::io::ErrorKind::TimedOut
                                                    {
                                                        continue;
                                                    }
                                                    eprintln!("[Writer] TCP error: {:?}", e);
                                                    println!("[VIDEO STATE] WRITE_FAILED");
                                                    write_connected.store(false, Ordering::Release);
                                                    break;
                                                }
                                            }
                                        }
                                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                                        Err(_) => break,
                                    }
                                }
                            });

                            let write_stream_clip = write_stream.clone();
                            let write_stream_frames = write_stream.clone();
                            let write_stream_audio = write_stream.clone();
                            let write_stream_ping = write_stream.clone();

                            let current_rtt_ms = Arc::new(AtomicU64::new(10));
                            let current_rtt_in = Arc::clone(&current_rtt_ms);

                            let active_screen_idx = Arc::new(AtomicUsize::new(0));
                            let active_idx_input = Arc::clone(&active_screen_idx);
                            let active_idx_capture = Arc::clone(&active_screen_idx);

                            let shared_frame = Arc::new((Mutex::new(None::<FrameData>), Condvar::new()));
                            let shared_frame_cap = Arc::clone(&shared_frame);

                            let last_clipboard_text = Arc::new(Mutex::new(String::new()));
                            let last_clip_recv = Arc::clone(&last_clipboard_text);
                            let last_clip_send = Arc::clone(&last_clipboard_text);

                            // INPUT THREAD
                            let input_handle = thread::spawn(move || {
                                println!("[INPUT THREAD] Started native input processing loop");
                                let mut clip = Clipboard::new().ok();
                                let mut current_file: Option<File> = None;
                                let mut total_file_size: u64 = 0;
                                let mut received_bytes: u64 = 0;
                                let drop_dir = PathBuf::from("RemoteDrop");
                                let _ = fs::create_dir_all(&drop_dir);

                                while is_conn_read.load(Ordering::SeqCst) {
                                    let mut pkt_type_buf = [0u8; 1];
                                    if read_stream.read_exact(&mut pkt_type_buf).is_err() {
                                        is_conn_read.store(false, Ordering::SeqCst);
                                        break;
                                    }

                                    match pkt_type_buf[0] {
                                        0..=10 => {
                                            let mut data = [0u8; 8];
                                            if read_stream.read_exact(&mut data).is_err() {
                                                is_conn_read.store(false, Ordering::SeqCst);
                                                break;
                                            }
                                            attach_thread_to_input_desktop();
                                            let event_type = pkt_type_buf[0];
                                            let type_name = match event_type {
                                                0 => "MOUSE_MOVE",
                                                1 | 3 | 7 => "MOUSE_DOWN",
                                                2 | 4 | 8 => "MOUSE_UP",
                                                5 => "KEY_DOWN",
                                                6 => "KEY_UP",
                                                9 => "MOUSE_WHEEL",
                                                _ => "CONTROL",
                                            };
                                            println!("[CONTROL DEBUG][HOST RX]\ntype={}\nlength=9", type_name);
                                            println!("[AGENT CONTROL RX]\ntype={}\nbytes=9", event_type);
                                            println!("[AGENT CONTROL RX] {}", type_name);

                                            if event_type == 10 {
                                                active_idx_input.store(data[0] as usize, Ordering::SeqCst);
                                                continue;
                                            }
                                            if event_type == 9 {
                                                let scroll_y = i16::from_be_bytes(data[2..4].try_into().unwrap());
                                                #[cfg(windows)]
                                                {
                                                    send_native_mouse_wheel(scroll_y);
                                                    println!("[AGENT MOUSE] injected wheel scroll={}", scroll_y);
                                                    println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_WHEEL\nresult=success");
                                                }
                                                continue;
                                            }
                                            if event_type == 5 || event_type == 6 {
                                                let key_code = u32::from_be_bytes(data[0..4].try_into().unwrap());
                                                #[cfg(windows)]
                                                {
                                                    send_native_key(key_code, event_type == 6);
                                                    println!("[INPUT INJECT] keyboard event={} vk={}", if event_type == 5 { "keydown" } else { "keyup" }, key_code);
                                                    println!("[AGENT KEYBOARD]\nevent={}\nkey={}\ncode={}", if event_type == 5 { "keydown" } else { "keyup" }, key_code, key_code);
                                                    println!("[AGENT KEYBOARD] injected");
                                                    println!("[CONTROL DEBUG][INPUT INJECTION]\ntype={}\nresult=success", if event_type == 5 { "KEY_DOWN" } else { "KEY_UP" });
                                                }
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

                                            #[cfg(windows)]
                                            {
                                                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                                                    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
                                                    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
                                                };

                                                match event_type {
                                                    0 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        println!("[INPUT INJECT] mousemove x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=none", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_MOVE\nresult=success");
                                                    }
                                                    1 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_LEFTDOWN);
                                                        println!("[INPUT INJECT] mousedown button=LEFT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=left_down", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_DOWN\nresult=success");
                                                    }
                                                    2 => {
                                                        send_native_mouse_click(MOUSEEVENTF_LEFTUP);
                                                        println!("[INPUT INJECT] mouseup button=LEFT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=left_up", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_UP\nresult=success");
                                                    }
                                                    3 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_RIGHTDOWN);
                                                        println!("[INPUT INJECT] mousedown button=RIGHT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=right_down", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_DOWN\nresult=success");
                                                    }
                                                    4 => {
                                                        send_native_mouse_click(MOUSEEVENTF_RIGHTUP);
                                                        println!("[INPUT INJECT] mouseup button=RIGHT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=right_up", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_UP\nresult=success");
                                                    }
                                                    7 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_MIDDLEDOWN);
                                                        println!("[INPUT INJECT] mousedown button=MIDDLE x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=middle_down", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_DOWN\nresult=success");
                                                    }
                                                    8 => {
                                                        send_native_mouse_click(MOUSEEVENTF_MIDDLEUP);
                                                        println!("[INPUT INJECT] mouseup button=MIDDLE x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=middle_up", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_UP\nresult=success");
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }

                                        12 => {
                                            let mut len_buf = [0u8; 4];
                                            if read_stream.read_exact(&mut len_buf).is_err() {
                                                break;
                                            }
                                            let len = u32::from_be_bytes(len_buf) as usize;
                                            if len > 10 * 1024 * 1024 {
                                                break;
                                            }
                                            let mut text_buf = vec![0u8; len];
                                            if read_stream.read_exact(&mut text_buf).is_err() {
                                                break;
                                            }
                                            if let Ok(text) = String::from_utf8(text_buf) {
                                                if let Ok(mut guard) = last_clip_recv.lock() {
                                                    *guard = text.clone();
                                                }
                                                if let Some(ref mut c) = clip {
                                                    let _ = c.set_text(text);
                                                }
                                            }
                                        }

                                        14 => {
                                            let mut time_buf = [0u8; 8];
                                            if read_stream.read_exact(&mut time_buf).is_err() {
                                                break;
                                            }
                                            let sent_time = u64::from_be_bytes(time_buf);
                                            let now = current_time_millis();
                                            if now >= sent_time {
                                                current_rtt_in.store(now - sent_time, Ordering::SeqCst);
                                            }
                                        }

                                        16 => {
                                            let mut meta = [0u8; 3];
                                            if read_stream.read_exact(&mut meta).is_err() {
                                                break;
                                            }
                                            let len = u16::from_be_bytes([meta[1], meta[2]]) as usize;
                                            let mut msg_bytes = vec![0u8; len];
                                            if read_stream.read_exact(&mut msg_bytes).is_err() {
                                                break;
                                            }
                                            if let Ok(txt) = String::from_utf8(msg_bytes) {
                                                println!("\n[Chat from Remote Viewer]: {}", txt);
                                            }
                                        }

                                        20 => {
                                            let mut meta_hdr = [0u8; 10];
                                            if read_stream.read_exact(&mut meta_hdr).is_err() {
                                                break;
                                            }
                                            let name_len = u16::from_be_bytes(meta_hdr[0..2].try_into().unwrap()) as usize;
                                            total_file_size = u64::from_be_bytes(meta_hdr[2..10].try_into().unwrap());
                                            let mut name_buf = vec![0u8; name_len];
                                            if read_stream.read_exact(&mut name_buf).is_err() {
                                                break;
                                            }
                                            let current_filename = String::from_utf8_lossy(&name_buf).to_string();
                                            let target_path = drop_dir.join(&current_filename);
                                            match File::create(&target_path) {
                                                Ok(f) => {
                                                    current_file = Some(f);
                                                    received_bytes = 0;
                                                }
                                                Err(e) => {
                                                    eprintln!("[File] Failed to create file: {:?}", e);
                                                    current_file = None;
                                                }
                                            }
                                        }

                                        21 => {
                                            let mut chunk_len_buf = [0u8; 4];
                                            if read_stream.read_exact(&mut chunk_len_buf).is_err() {
                                                break;
                                            }
                                            let chunk_len = u32::from_be_bytes(chunk_len_buf) as usize;
                                            if chunk_len > 100 * 1024 * 1024 {
                                                break;
                                            }
                                            let mut chunk_buf = vec![0u8; chunk_len];
                                            if read_stream.read_exact(&mut chunk_buf).is_err() {
                                                break;
                                            }
                                            if let Some(ref mut file) = current_file {
                                                if file.write_all(&chunk_buf).is_err() {
                                                    current_file = None;
                                                    continue;
                                                }
                                                received_bytes += chunk_len as u64;
                                                if received_bytes >= total_file_size {
                                                    let _ = file.flush();
                                                    current_file = None;
                                                }
                                            }
                                        }

                                        99 => {
                                            println!("[Agent] Viewer disconnected command received.");
                                            is_conn_read.store(false, Ordering::SeqCst);
                                            break;
                                        }

                                        _ => {}
                                    }
                                }
                            });

                            let ping_handle = thread::spawn(move || {
                                while is_conn_ping.load(Ordering::SeqCst) {
                                    let mut ping_pkt = Vec::with_capacity(9);
                                    ping_pkt.push(14u8);
                                    ping_pkt.extend_from_slice(&current_time_millis().to_be_bytes());
                                    if write_stream_ping.send(ping_pkt).is_err() {
                                        break;
                                    }
                                    thread::sleep(Duration::from_millis(100));
                                }
                            });

                            let clip_handle = thread::spawn(move || {
                                let mut clip = Clipboard::new().ok();
                                while is_conn_clip.load(Ordering::SeqCst) {
                                    if let Some(ref mut c) = clip {
                                        if let Ok(text) = c.get_text() {
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

                            let _audio = start_audio_capture(write_stream_audio, is_conn_audio);

                            let capture_handle = thread::spawn(move || {
                                let mut screens = Screen::all().unwrap_or_default();
                                let mut last_idx = active_idx_capture.load(Ordering::SeqCst);

                                println!("[Capture] {} monitor(s) detected.", screens.len());

                                while is_conn_capture.load(Ordering::SeqCst) {
                                    let frame_start = Instant::now();
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
                                                });
                                            }
                                        }
                                    }

                                    if captured_frame.is_none() {
                                        captured_frame = capture_screen_gdi();
                                    }

                                    if let Some(frame) = captured_frame {
                                        let (lock, cvar) = &*shared_frame_cap;
                                        if let Ok(mut shared) = lock.lock() {
                                            *shared = Some(frame);
                                            cvar.notify_one();
                                        }
                                    } else {
                                        screens = Screen::all().unwrap_or_default();
                                        thread::sleep(Duration::from_millis(16));
                                    }

                                    let elapsed = frame_start.elapsed();
                                    let target = Duration::from_micros(FRAME_INTERVAL_MICROS);
                                    if elapsed < target {
                                        thread::sleep(target - elapsed);
                                    }
                                }
                            });

                            let mut frame_number: u64 = 0;
                            let mut hw_encoder: Option<HardwareH264Encoder> = None;

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

                                let src_width = frame.width;
                                let src_height = frame.height;
                                if src_width == 0 || src_height == 0 {
                                    continue;
                                }

                                if hw_encoder.is_none() {
                                    match HardwareH264Encoder::new(MAX_WIDTH, MAX_HEIGHT, TARGET_FPS, TARGET_BITRATE) {
                                        Ok(enc) => {
                                            hw_encoder = Some(enc);
                                        }
                                        Err(e) => {
                                            eprintln!("[H264 HW][FATAL] Hardware encoder initialization failed: {}", e);
                                            break;
                                        }
                                    }
                                }

                                let encoder = hw_encoder.as_mut().unwrap();
                                let is_keyframe_request = frame_number == 0 || !encoder.has_produced_keyframe || (frame_number % (TARGET_FPS as u64) == 0);
                                if is_keyframe_request {
                                    println!("[H264] KEYFRAME REQUESTED");
                                }

                                let h264_bytes = match encoder.encode_rgba(&frame.raw_pixels, src_width, src_height, is_keyframe_request) {
                                    Ok(bytes) => bytes,
                                    Err(_) => continue,
                                };

                                if h264_bytes.is_empty() {
                                    continue;
                                }

                                frame_number += 1;
                                let mut nal_types = Vec::new();
                                let mut has_sps = false;
                                let mut has_pps = false;
                                let mut has_idr = false;
                                let mut i = 0;
                                while i + 3 < h264_bytes.len() {
                                    if (h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 1)
                                        || (i + 4 <= h264_bytes.len() && h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 0 && h264_bytes[i+3] == 1)
                                    {
                                        let sc_len = if h264_bytes[i+2] == 1 { 3 } else { 4 };
                                        if i + sc_len < h264_bytes.len() {
                                            let n_type = h264_bytes[i + sc_len] & 0x1F;
                                            nal_types.push(n_type);
                                            if n_type == 7 { has_sps = true; }
                                            else if n_type == 8 { has_pps = true; }
                                            else if n_type == 5 { has_idr = true; }
                                        }
                                        i += sc_len;
                                    } else {
                                        i += 1;
                                    }
                                }

                                if has_idr {
                                    println!("[H264] IDR GENERATED");
                                    println!("[H264] NAL TYPES={:?}", nal_types);
                                    println!("[H264] SENDING KEYFRAME TO VIEWER");
                                }

                                let mut raw_hash = 0u64;
                                for (idx, &b) in frame.raw_pixels.iter().take(4096).enumerate() {
                                    raw_hash = raw_hash.wrapping_add((b as u64).wrapping_mul(idx as u64 + 1));
                                }
                                let pts = (frame_number as u64) * (1_000_000u64 / (TARGET_FPS as u64));

                                println!("[ENCODER]\nframe={}\ninput_bytes={}\ninput_hash={:016x}\noutput_bytes={}\nnal_types={:?}\nsps={}\npps={}\nidr={}\npts={}",
                                    frame_number,
                                    frame.raw_pixels.len(),
                                    raw_hash,
                                    h264_bytes.len(),
                                    nal_types,
                                    has_sps,
                                    has_pps,
                                    has_idr,
                                    pts
                                );

                                let format_str = if h264_bytes.starts_with(&[0, 0, 0, 1]) || h264_bytes.starts_with(&[0, 0, 1]) {
                                    "AnnexB"
                                } else {
                                    "AVC"
                                };
                                println!("[AGENT H264]\nencoder=hardware\nformat={}\nsize={}\nNAL types={:?}", format_str, h264_bytes.len(), nal_types);
                                println!("[VIDEO AGENT] frame captured");
                                println!("[VIDEO AGENT] H264 encoded: {} bytes", h264_bytes.len());
                                println!("[VIDEO AGENT] sending TYPE=13 width={} height={} h264_size={}", MAX_WIDTH, MAX_HEIGHT, h264_bytes.len());
                                println!("[VIDEO AGENT] NAL types: {:?} SPS={} PPS={} IDR={}", nal_types, has_sps, has_pps, has_idr);

                                let packet_size = 13 + h264_bytes.len();
                                let mut packet = Vec::with_capacity(packet_size);
                                packet.push(13u8);
                                packet.extend_from_slice(&(MAX_WIDTH as u32).to_be_bytes());
                                packet.extend_from_slice(&(MAX_HEIGHT as u32).to_be_bytes());
                                packet.extend_from_slice(&(h264_bytes.len() as u32).to_be_bytes());
                                packet.extend_from_slice(&h264_bytes);

                                let is_first_key = frame_number == 1;
                                let send_res = if is_first_key {
                                    write_stream_frames.send(packet).map_err(|_| std::sync::mpsc::TrySendError::Disconnected(Vec::new()))
                                } else {
                                    write_stream_frames.try_send(packet)
                                };

                                if let Err(e) = send_res {
                                    match e {
                                        std::sync::mpsc::TrySendError::Full(_) => {}
                                        std::sync::mpsc::TrySendError::Disconnected(_) => {
                                            is_conn_write.store(false, Ordering::SeqCst);
                                            break;
                                        }
                                    }
                                }
                            }

                            // SHUTDOWN OF SESSION
                            is_connected.store(false, Ordering::SeqCst);
                            drop(write_stream);

                            let _ = input_handle.join();
                            let _ = ping_handle.join();
                            let _ = clip_handle.join();
                            let _ = capture_handle.join();
                            let _ = writer_handle.join();

                            backend.log_session_end(&system_id, 0.0);
                            println!("[STREAM STATE] STOPPED");

                            println!("[Agent] Session ended. Preparing for next request...");
                            is_in_session.store(false, Ordering::SeqCst);
                            thread::sleep(Duration::from_millis(500));
                        }

                        // Type 99: Session disconnect signal
                        99 => {
                            println!("[Agent] Idle state confirmed.");
                        }

                        _ => {}
                    }
                }

                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                        continue 'viewer_loop;
                    }
                    println!("[RELAY][DISCONNECT] reason=Socket read error or closed by relay: {:?}", e);
                    println!("[RELAY][DISCONNECT] system_id={}", system_id);
                    println!("[RELAY][DISCONNECT] socket_error={:?}", e);
                    println!("[RELAY][DISCONNECT] remote_closed=true");
                    println!("[SESSION STATE] OFFLINE");
                    break 'viewer_loop;
                }
            }
        }

        is_running_conn.store(false, Ordering::SeqCst);
        let _ = heartbeat_handle.join();

        println!("[Agent] Relay connection lost. Reconnecting in {}s...", backoff_secs);
        thread::sleep(Duration::from_secs(backoff_secs));
        backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
    }
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    let _single_instance_guard = match std::net::TcpListener::bind("127.0.0.1:49182") {
        Ok(listener) => listener,
        Err(_) => {
            println!("[Agent] Another instance of DeskStream agent is already running. Exiting.");
            std::process::exit(0);
        }
    };

    set_process_dpi_aware();

    let args: Vec<String> = env::args().collect();

    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "192.168.29.229:9001".to_string()
    };

    // Load persistent identity from %LOCALAPPDATA%\DeskStream\agent_config.json
    // This uses the Windows MachineGuid as the permanent device UUID
    let config = identity::device_id::AgentConfig::load_or_create("", &relay_addr);

    let current_exe_path = env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let _ = enable_autostart("ScreenShareAgent");

    println!("========================================");
    println!("       REMOTE DESKTOP AGENT (120 FPS)");
    println!("  BUILD VERSION: 1.0.1 (Production Desktop Agent)");
    println!("  EXECUTABLE:    {}", current_exe_path);
    println!("========================================");
    println!("  Device UUID: {}", config.device_uuid);
    println!("  System ID:   {}", config.formatted_id());
    println!("  Relay: {}", relay_addr);
    println!("  Video: 1920x1080 @ 120 FPS");
    println!("  Codec: Hardware H.264 (Low Latency)");
    println!("========================================");

    run_agent_loop(relay_addr, config);
}