// Fixed Video Pipeline (H264 -> TYPE 15)
pub mod registration {
    pub mod backend_client;
}

use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use enigo::{Axis, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use openh264::encoder::Encoder;
use openh264::formats::YUVSource;
use rand::Rng;
use screenshots::Screen;
use sha2::{Digest, Sha256};

use image::imageops::FilterType;
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
// VIDEO SETTINGS
// ============================================================

const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;
const FRAME_INTERVAL_MS: u64 = 33;

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

// ============================================================
// AUTOSTART
// ============================================================

#[cfg(windows)]
fn enable_autostart(app_name: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegSetValueExW, HKEY_CURRENT_USER, REG_SZ,
    };

    let exe_path = env::current_exe().map_err(|e| e.to_string())?;
    let exe_path_str = exe_path.to_str().ok_or("Invalid path")?;

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
            Ok(())
        } else {
            Err(format!("RegSetValueExW failed with code {}", res))
        }
    }
}

#[cfg(not(windows))]
fn enable_autostart(_app_name: &str) -> Result<(), String> {
    Ok(())
}

// ============================================================
// CONNECTION DIALOG
// ============================================================

#[cfg(windows)]
fn prompt_connection_dialog(id_str: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    };

    let title: Vec<u16> = "Remote Desktop Request\0".encode_utf16().collect();
    let text: Vec<u16> = format!(
        "Incoming remote control request for ID: {}\n\n\
         Do you want to ALLOW this session?",
        id_str
    )
    .encode_utf16()
    .collect();

    unsafe {
        let result = MessageBoxW(
            0,
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
        );
        result == IDYES
    }
}

#[cfg(not(windows))]
fn prompt_connection_dialog(_id_str: &str) -> bool {
    false
}

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
// PREPARE FRAME (FIXED - supports RGB or RGBA, forces remote to 3 bytes/pixel)
// ============================================================

fn prepare_frame(raw_pixels: Vec<u8>, width: usize, height: usize, is_remote: bool) -> Option<FrameData> {
    if raw_pixels.is_empty() || width == 0 || height == 0 {
        return None;
    }

    let expected = width.checked_mul(height)?.checked_mul(4)?;

    if raw_pixels.len() < expected {
        eprintln!(
            "[Capture] Invalid pixel buffer: got {}, expected {}",
            raw_pixels.len(),
            expected
        );
        return None;
    }

    // Prepare result frame (resize if needed)
    let frame_data = if width as u32 <= MAX_WIDTH && height as u32 <= MAX_HEIGHT {
        let w = (width as u32 + 1) & !1;
        let h = (height as u32 + 1) & !1;
        if w != width as u32 || h != height as u32 {
            if let Some(img) =
                image::RgbaImage::from_raw(width as u32, height as u32, raw_pixels[..expected].to_vec())
            {
                let resized = image::imageops::resize(&img, w, h, FilterType::Triangle);
                FrameData {
                    width: w as usize,
                    height: h as usize,
                    raw_pixels: resized.into_raw(),
                }
            } else {
                FrameData {
                    width,
                    height,
                    raw_pixels: raw_pixels[..expected].to_vec(),
                }
            }
        } else {
            FrameData {
                width,
                height,
                raw_pixels: raw_pixels[..expected].to_vec(),
            }
        }
    } else {
        // Resize preserving aspect ratio.
        let scale_x = MAX_WIDTH as f32 / width as f32;
        let scale_y = MAX_HEIGHT as f32 / height as f32;
        let scale = scale_x.min(scale_y);

        let new_width = ((width as f32) * scale).round().max(2.0) as u32;
        let new_height = ((height as f32) * scale).round().max(2.0) as u32;
        let new_width = (new_width + 1) & !1;
        let new_height = (new_height + 1) & !1;

        let image = image::RgbaImage::from_raw(
            width as u32,
            height as u32,
            raw_pixels[..expected].to_vec(),
        )?;

        let resized = image::imageops::resize(&image, new_width, new_height, FilterType::Triangle);

        FrameData {
            width: new_width as usize,
            height: new_height as usize,
            raw_pixels: resized.into_raw(),
        }
    };

    // ========================================================
    // [VIDEO DEBUG] STAGE 2 — prepare_frame()
    // ========================================================
    static PREPARE_COUNTER: AtomicU64 = AtomicU64::new(0);
    let p_idx = PREPARE_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    if p_idx <= 3 || p_idx % 30 == 0 {
        let mut non_black = 0usize;
        let mut min_r = 255u8;
        let mut max_r = 0u8;
        let mut min_g = 255u8;
        let mut max_g = 0u8;
        let mut min_b = 255u8;
        let mut max_b = 0u8;
        let mut sum = 0u64;

        for chunk in frame_data.raw_pixels.chunks_exact(4) {
            let r = chunk[0];
            let g = chunk[1];
            let b = chunk[2];
            if r > 0 || g > 0 || b > 0 {
                non_black += 1;
            }
            min_r = min_r.min(r);
            max_r = max_r.max(r);
            min_g = min_g.min(g);
            max_g = max_g.max(g);
            min_b = min_b.min(b);
            max_b = max_b.max(b);
            sum += (r as u64) + (g as u64) + (b as u64);
        }
        let total_pixels = frame_data.width * frame_data.height;
        let avg = if total_pixels > 0 { (sum as f64) / ((total_pixels * 3) as f64) } else { 0.0 };

        println!("[VIDEO DEBUG][PREPARED FRAME #{}]\nwidth={}\nheight={}\nraw_bytes={}", p_idx, frame_data.width, frame_data.height, frame_data.raw_pixels.len());
        println!("[VIDEO DEBUG][PREPARED STATS #{}]\nmin=R:{} G:{} B:{}\nmax=R:{} G:{} B:{}\naverage={:.2}\nnon_black={}\ntotal_pixels={}", p_idx, min_r, min_g, min_b, max_r, max_g, max_b, avg, non_black, total_pixels);
        if non_black == 0 {
            println!("[VIDEO DEBUG][ERROR] PREPARED FRAME #{} IS COMPLETELY BLACK", p_idx);
        } else {
            println!("[VIDEO DEBUG][OK] PREPARED FRAME #{} CONTAINS VISIBLE PIXELS", p_idx);
        }
    }

    Some(frame_data)
}

// ============================================================
// YUV CONVERTER (For H.264)
// ============================================================

struct MyYuv {
    width: usize,
    height: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl YUVSource for MyYuv {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }
}

fn rgba_to_yuv420(rgba: &[u8], width: usize, height: usize) -> MyYuv {
    let y_size = width * height;
    let uv_size = (width / 2) * (height / 2);
    let mut y = vec![0u8; y_size];
    let mut u = vec![0u8; uv_size];
    let mut v = vec![0u8; uv_size];

    for j in 0..height {
        for i in 0..width {
            let px = (j * width + i) * 4;
            if px + 2 >= rgba.len() {
                continue;
            }
            let r = rgba[px] as f32;
            let g = rgba[px + 1] as f32;
            let b = rgba[px + 2] as f32;

            let yy = (0.257 * r + 0.504 * g + 0.098 * b + 16.0) as u8;
            y[j * width + i] = yy;

            if j % 2 == 0 && i % 2 == 0 {
                let uu = (-0.148 * r - 0.291 * g + 0.439 * b + 128.0) as u8;
                let vv = (0.439 * r - 0.368 * g - 0.071 * b + 128.0) as u8;
                let uv_idx = (j / 2) * (width / 2) + (i / 2);
                if uv_idx < uv_size {
                    u[uv_idx] = uu;
                    v[uv_idx] = vv;
                }
            }
        }
    }

    // ========================================================
    // [VIDEO DEBUG] STAGE 3 — RGBA -> YUV420
    // ========================================================
    static YUV_COUNTER: AtomicU64 = AtomicU64::new(0);
    let y_idx = YUV_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    if y_idx <= 3 || y_idx % 30 == 0 {
        let (y_min, y_max, y_avg) = calc_plane_stats(&y);
        let (u_min, u_max, u_avg) = calc_plane_stats(&u);
        let (v_min, v_max, v_avg) = calc_plane_stats(&v);

        println!("[VIDEO DEBUG][YUV #{}]\nwidth={}\nheight={}\nY_size={}\nU_size={}\nV_size={}\nY_stride={}\nU_stride={}\nV_stride={}",
            y_idx, width, height, y.len(), u.len(), v.len(), width, width / 2, width / 2);
        println!("[VIDEO DEBUG][YUV STATS #{}]\nY={:.1}/{:.1}/{:.2}\nU={:.1}/{:.1}/{:.2}\nV={:.1}/{:.1}/{:.2}",
            y_idx, y_min, y_max, y_avg, u_min, u_max, u_avg, v_min, v_max, v_avg);

        if y_min == y_max && y_avg <= 16.5 {
            println!("[VIDEO DEBUG][ERROR] YUV INPUT #{} IS EFFECTIVELY BLACK", y_idx);
        } else {
            println!("[VIDEO DEBUG][OK] YUV #{} CONTAINS IMAGE DATA", y_idx);
        }
    }

    MyYuv { width, height, y, u, v }
}

fn calc_plane_stats(plane: &[u8]) -> (f64, f64, f64) {
    if plane.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut min_val = 255u8;
    let mut max_val = 0u8;
    let mut sum = 0u64;
    for &b in plane {
        min_val = min_val.min(b);
        max_val = max_val.max(b);
        sum += b as u64;
    }
    (min_val as f64, max_val as f64, (sum as f64) / (plane.len() as f64))
}

// ============================================================
// AGENT LOOP
// ============================================================

fn run_agent_loop(relay_addr: String, session_id: u32, id_str: String) {
    let backend = registration::backend_client::BackendClient::new(
        "http://127.0.0.1/Screen%20Share/backend/api",
        &session_id.to_string(),
    );

    if backend.register() {
        println!("[Backend Sync] Registered successfully with PHP Web Dashboard!");
    } else {
        println!("[Backend Sync] Running standalone mode.");
    }

    backend.start_heartbeat_thread();

    loop {
        println!("[Agent] Connecting to relay {}...", relay_addr);

        let mut stream = match TcpStream::connect(&relay_addr) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                s
            }
            Err(e) => {
                eprintln!("[Agent] Relay error: {:?}", e);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        // Registration
        let mut register_pkt = Vec::with_capacity(7);
        register_pkt.push(1u8);
        register_pkt.extend_from_slice(session_id.to_string().as_bytes());

        if stream.write_all(&register_pkt).is_err() {
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        println!("[Agent] Registered on relay. Waiting for incoming session request...");

        // Authentication request
        let mut req_signal = [0u8; 33];
        if stream.read_exact(&mut req_signal[0..1]).is_err() {
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        if req_signal[0] != 3 {
            eprintln!("[Agent] Unexpected relay packet: {}", req_signal[0]);
            continue;
        }
        if stream.read_exact(&mut req_signal[1..33]).is_err() {
            continue;
        }

        let incoming_hash = &req_signal[1..33];
        let unattended_pin = env::var("AGENT_PIN").unwrap_or_default();
        let mut authorized = false;

        println!("[Host] Received connection request");
        println!("[Host] Target ID: {}", id_str);

        if !unattended_pin.is_empty() {
            let local_hash = compute_sha256(&unattended_pin);
            if local_hash == incoming_hash {
                println!("[Agent] Unattended access PIN verified!");
                authorized = true;
            }
        }

        if !authorized {
            authorized = prompt_connection_dialog(&id_str);
        }

        println!("[Host] Sending approval/rejection");

        if !authorized {
            let _ = stream.write_all(&[2u8]);
            println!("[Host] Approval response sent successfully (REJECTED)");
            println!("[Agent] Session REJECTED.");
            continue;
        }

        let _ = stream.write_all(&[1u8]);
        println!("[Host] Approval response sent successfully (APPROVED)");
        backend.log_session_start(&session_id.to_string());
        println!("[Agent] Session APPROVED! Starting live video...");

        // ====================================================
        // CONNECTION STATE
        // ====================================================

        let is_connected = Arc::new(AtomicBool::new(true));
        let is_conn_read = Arc::clone(&is_connected);
        let is_conn_write = Arc::clone(&is_connected);
        let is_conn_capture = Arc::clone(&is_connected);
        let is_conn_clip = Arc::clone(&is_connected);
        let is_conn_audio = Arc::clone(&is_connected);
        let is_conn_ping = Arc::clone(&is_connected);

        // ====================================================
        // TCP INPUT
        // ====================================================

        let mut read_stream = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => {
                is_connected.store(false, Ordering::Release);
                continue;
            }
        };

        // ====================================================
        // OUTPUT QUEUE
        // ====================================================

        let (out_tx, out_rx) = sync_channel::<Vec<u8>>(128);
        let write_stream = out_tx.clone();
        let mut write_tcp = match stream.try_clone() {
            Ok(s) => s,
            Err(_) => {
                is_connected.store(false, Ordering::Release);
                continue;
            }
        };

        let write_connected = Arc::clone(&is_connected);

        let writer_handle = thread::spawn(move || {
            while write_connected.load(Ordering::Acquire) {
                match out_rx.recv() {
                    Ok(packet) => {
                        let pkt_type = packet.first().copied().unwrap_or(0);
                        if pkt_type == 15 {
                            println!("[VIDEO DEBUG][WRITER]\nreceived_type=15\npacket_size={}", packet.len());
                            if packet.len() >= 13 {
                                let width = u32::from_be_bytes(packet[1..5].try_into().unwrap());
                                let height = u32::from_be_bytes(packet[5..9].try_into().unwrap());
                                let declared_h264_size = u32::from_be_bytes(packet[9..13].try_into().unwrap()) as usize;
                                let actual_h264_size = packet.len() - 13;
                                let valid = declared_h264_size == actual_h264_size;
                                println!("[VIDEO DEBUG][WRITER PARSE]\nwidth={}\nheight={}\ndeclared_h264_size={}\nactual_h264_size={}\nvalid={}",
                                    width, height, declared_h264_size, actual_h264_size, valid);

                                let mut hasher = Sha256::new();
                                hasher.update(&packet[13..]);
                                let hash = format!("{:x}", hasher.finalize());
                                println!("[VIDEO DEBUG][WRITER HASH]\nsha256={}", hash);
                            }
                        }

                        match write_tcp.write_all(&packet) {
                            Ok(_) => {}
                            Err(e) => {
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    || e.kind() == std::io::ErrorKind::TimedOut
                                {
                                    continue;
                                }
                                eprintln!("[Writer] TCP error: {:?}", e);
                                write_connected.store(false, Ordering::Release);
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // ====================================================
        // STREAM CLONES
        // ====================================================

        let write_stream_clip = write_stream.clone();
        let write_stream_frames = write_stream.clone();
        let write_stream_audio = write_stream.clone();
        let write_stream_ping = write_stream.clone();

        // ====================================================
        // RTT
        // ====================================================

        let current_rtt_ms = Arc::new(AtomicU64::new(10));
        let current_rtt_in = Arc::clone(&current_rtt_ms);

        // ====================================================
        // ACTIVE SCREEN
        // ====================================================

        let active_screen_idx = Arc::new(AtomicUsize::new(0));
        let active_idx_input = Arc::clone(&active_screen_idx);
        let active_idx_capture = Arc::clone(&active_screen_idx);

        // ====================================================
        // LATEST FRAME BUFFER
        // ====================================================

        let shared_frame = Arc::new((Mutex::new(None::<FrameData>), Condvar::new()));
        let shared_frame_cap = Arc::clone(&shared_frame);

        // ====================================================
        // CLIPBOARD
        // ====================================================

        let last_clipboard_text = Arc::new(Mutex::new(String::new()));
        let last_clip_recv = Arc::clone(&last_clipboard_text);
        let last_clip_send = Arc::clone(&last_clipboard_text);

        // ====================================================
        // INPUT THREAD
        // ====================================================

        let input_handle = thread::spawn(move || {
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[Input] Enigo initialization failed: {:?}", e);
                    return;
                }
            };

            let mut clip = Clipboard::new().ok();
            let mut current_file: Option<File> = None;
            let mut total_file_size: u64 = 0;
            let mut received_bytes: u64 = 0;
            let drop_dir = PathBuf::from("RemoteDrop");
            let _ = fs::create_dir_all(&drop_dir);

            while is_conn_read.load(Ordering::SeqCst) {
                let mut type_buf = [0u8; 1];
                if read_stream.read_exact(&mut type_buf).is_err() {
                    is_conn_read.store(false, Ordering::SeqCst);
                    break;
                }

                match type_buf[0] {
                    // Mouse / Keyboard
                    0..=8 => {
                        let mut data = [0u8; 8];
                        if read_stream.read_exact(&mut data).is_err() {
                            is_conn_read.store(false, Ordering::SeqCst);
                            break;
                        }

                        let event_type = type_buf[0];

                        // Monitor selection.
                        if event_type == 7 {
                            active_idx_input.store(data[0] as usize, Ordering::SeqCst);
                            continue;
                        }

                        // Scroll.
                        if event_type == 8 {
                            let scroll_y = i16::from_be_bytes(data[2..4].try_into().unwrap());
                            let steps = (scroll_y / 120) as i32;
                            if steps != 0 {
                                let _ = enigo.scroll(steps, Axis::Vertical);
                            }
                            continue;
                        }

                        let norm_x = u16::from_be_bytes(data[0..2].try_into().unwrap());
                        let norm_y = u16::from_be_bytes(data[2..4].try_into().unwrap());

                        let current_idx = active_idx_input.load(Ordering::SeqCst);
                        let screens = Screen::all().unwrap_or_default();
                        let screen_ref = screens.get(current_idx).or_else(|| screens.first());

                        let (sx, sy, sw, sh) = if let Some(s) = screen_ref {
                            (
                                s.display_info.x,
                                s.display_info.y,
                                s.display_info.width as f32,
                                s.display_info.height as f32,
                            )
                        } else {
                            (0, 0, 1920.0, 1080.0)
                        };

                        let target_x =
                            sx + ((norm_x as f32 / 65535.0) * (sw - 1.0)).round() as i32;
                        let target_y =
                            sy + ((norm_y as f32 / 65535.0) * (sh - 1.0)).round() as i32;

                        #[cfg(windows)]
                        {
                            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                                MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN,
                                MOUSEEVENTF_RIGHTUP,
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
                                _ => {}
                            }
                        }

                        match event_type {
                            5 => {
                                let key_code = u32::from_be_bytes(data[0..4].try_into().unwrap());
                                if let Some(k) = map_key_code(key_code) {
                                    let _ = enigo.key(k, Direction::Press);
                                }
                            }
                            6 => {
                                let key_code = u32::from_be_bytes(data[0..4].try_into().unwrap());
                                if let Some(k) = map_key_code(key_code) {
                                    let _ = enigo.key(k, Direction::Release);
                                }
                            }
                            _ => {}
                        }
                    }

                    // Clipboard
                    12 => {
                        let mut len_buf = [0u8; 4];
                        if read_stream.read_exact(&mut len_buf).is_err() {
                            break;
                        }
                        let len = u32::from_be_bytes(len_buf) as usize;
                        if len > 10 * 1024 * 1024 {
                            eprintln!("[Clipboard] Payload too large.");
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

                    // RTT echo (type 14)
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

                    // Chat
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

                    // File start
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

                    // File chunk
                    21 => {
                        let mut chunk_len_buf = [0u8; 4];
                        if read_stream.read_exact(&mut chunk_len_buf).is_err() {
                            break;
                        }
                        let chunk_len = u32::from_be_bytes(chunk_len_buf) as usize;
                        if chunk_len > 100 * 1024 * 1024 {
                            eprintln!("[File] Chunk too large.");
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

                    123 => {
                        // Protocol boundary fix: stray JSON/text data
                        eprintln!("[Input] Ignored stray JSON/text data starting with '{{'.");
                    }

                    _ => {
                        eprintln!("[Input] Unknown packet type: {}", type_buf[0]);
                    }
                }
            }
        });

        // ====================================================
        // PING
        // ====================================================

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

        // ====================================================
        // CLIPBOARD
        // ====================================================

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

        // ====================================================
        // AUDIO
        // ====================================================

        let _audio = start_audio_capture(write_stream_audio, is_conn_audio);

        // ====================================================
        // SCREEN CAPTURE THREAD (UPDATED - passes is_remote flag)
        // ====================================================

        let capture_handle = thread::spawn(move || {
            let mut screens = Screen::all().unwrap_or_default();
            let mut last_idx = active_idx_capture.load(Ordering::SeqCst);

            println!("[Capture] {} monitor(s) detected.", screens.len());

            while is_conn_capture.load(Ordering::SeqCst) {
                let start = Instant::now();

                let current_idx = active_idx_capture.load(Ordering::SeqCst);

                if current_idx != last_idx || screens.is_empty() {
                    screens = Screen::all().unwrap_or_default();
                    last_idx = current_idx;
                    println!("[Capture] Refreshed monitor list: {} monitor(s)", screens.len());
                }

                if screens.is_empty() {
                    eprintln!("[Capture] No screens detected.");
                    thread::sleep(Duration::from_millis(FRAME_INTERVAL_MS));
                    continue;
                }

                let screen = match screens.get(current_idx) {
                    Some(s) => s,
                    None => {
                        eprintln!(
                            "[Capture] Invalid monitor index {}, using monitor 0.",
                            current_idx
                        );
                        &screens[0]
                    }
                };

                match screen.capture() {
                    Ok(img) => {
                        let source_width = img.width() as usize;
                        let source_height = img.height() as usize;
                        let raw = img.into_raw();

                        // ========================================================
                        // [VIDEO DEBUG] STAGE 1 — SCREEN CAPTURE
                        // ========================================================
                        static CAP_COUNTER: AtomicU64 = AtomicU64::new(0);
                        let cap_idx = CAP_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
                        if cap_idx <= 3 || cap_idx % 30 == 0 {
                            let expected_bytes = source_width * source_height * 4;
                            println!("[VIDEO DEBUG][CAPTURE #{}]\nwidth={}\nheight={}\nraw_bytes={}\nexpected_bytes={}",
                                cap_idx, source_width, source_height, raw.len(), expected_bytes);

                            if raw.len() >= 4 {
                                let p0 = &raw[0..4];
                                let p1 = if raw.len() >= 8 { &raw[4..8] } else { p0 };
                                let center_idx = (source_height / 2 * source_width + source_width / 2) * 4;
                                let p_center = if center_idx + 4 <= raw.len() { &raw[center_idx..center_idx+4] } else { p0 };
                                let last_idx = raw.len().saturating_sub(4);
                                let p_last = &raw[last_idx..last_idx+4];

                                println!("[VIDEO DEBUG][CAPTURE PIXELS #{}]\npixel0={},{},{},{}\npixel1={},{},{},{}\ncenter={},{},{},{}\nlast={},{},{},{}",
                                    cap_idx,
                                    p0[0], p0[1], p0[2], p0[3],
                                    p1[0], p1[1], p1[2], p1[3],
                                    p_center[0], p_center[1], p_center[2], p_center[3],
                                    p_last[0], p_last[1], p_last[2], p_last[3]
                                );

                                let mut min_r = 255u8;
                                let mut max_r = 0u8;
                                let mut min_g = 255u8;
                                let mut max_g = 0u8;
                                let mut min_b = 255u8;
                                let mut max_b = 0u8;
                                let mut sum = 0u64;
                                let mut non_black = 0usize;

                                for chunk in raw.chunks_exact(4) {
                                    let r = chunk[0];
                                    let g = chunk[1];
                                    let b = chunk[2];
                                    if r > 0 || g > 0 || b > 0 {
                                        non_black += 1;
                                    }
                                    min_r = min_r.min(r);
                                    max_r = max_r.max(r);
                                    min_g = min_g.min(g);
                                    max_g = max_g.max(g);
                                    min_b = min_b.min(b);
                                    max_b = max_b.max(b);
                                    sum += (r as u64) + (g as u64) + (b as u64);
                                }
                                let total_pixels = source_width * source_height;
                                let avg = if total_pixels > 0 { (sum as f64) / ((total_pixels * 3) as f64) } else { 0.0 };

                                println!("[VIDEO DEBUG][CAPTURE STATS #{}]\nmin=R:{} G:{} B:{}\nmax=R:{} G:{} B:{}\naverage={:.2}\nnon_black={}\ntotal_pixels={}",
                                    cap_idx, min_r, min_g, min_b, max_r, max_g, max_b, avg, non_black, total_pixels);
                            }
                        }

                        let expected = source_width
                            .checked_mul(source_height)
                            .and_then(|v| v.checked_mul(4))
                            .unwrap_or(0);

                        if raw.len() < expected {
                            eprintln!(
                                "[Capture] Invalid raw buffer: {} < {}",
                                raw.len(),
                                expected
                            );
                            continue;
                        }

                        let is_remote = false; // backend = 4 bytes, remote = 3 bytes (FIXED)
                        if let Some(frame) = prepare_frame(raw, source_width, source_height, is_remote) {
                            let (lock, cvar) = &*shared_frame_cap;
                            let mut shared = match lock.lock() {
                                Ok(g) => g,
                                Err(_) => {
                                    eprintln!("[Capture] Frame mutex poisoned.");
                                    continue;
                                }
                            };
                            *shared = Some(frame);
                            cvar.notify_one();
                        }
                    }
                    Err(e) => {
                        eprintln!("[Capture] screen.capture() FAILED: {:?}", e);
                    }
                }

                let elapsed = start.elapsed();
                let target = Duration::from_millis(FRAME_INTERVAL_MS);
                if elapsed < target {
                    thread::sleep(target - elapsed);
                }
            }

            println!("[Capture] Capture thread stopped.");
        });

        // ====================================================
        // VIDEO ENCODING + STREAMING
        // ====================================================

        let mut frame_number: u64 = 0;
        let mut h264_encoder: Option<openh264::encoder::Encoder> = None;
        let mut enc_width = 0;
        let mut enc_height = 0;

        println!(
            "[Video] Streaming H.264 at max {}x{}",
            MAX_WIDTH, MAX_HEIGHT
        );

        while is_conn_write.load(Ordering::SeqCst) {
            let frame_opt = {
                let (lock, cvar) = &*shared_frame;
                let mut shared = match lock.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        eprintln!("[Video] Frame mutex poisoned.");
                        break;
                    }
                };

                if shared.is_none() {
                    let result = cvar.wait_timeout(shared, Duration::from_millis(200));
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

            let width = frame.width;
            let height = frame.height;

            if width == 0 || height == 0 {
                continue;
            }

            // H.264 ENCODING
            if width % 2 != 0 || height % 2 != 0 {
                eprintln!("[H264] Skipping odd dimensions {}x{}", width, height);
                continue;
            }

            if h264_encoder.is_none() || enc_width != width || enc_height != height {
                let config = openh264::encoder::EncoderConfig::new()
                    .bitrate(openh264::encoder::BitRate::from_bps(2_000_000))
                    .max_frame_rate(openh264::encoder::FrameRate::from_hz(30.0))
                    .rate_control_mode(openh264::encoder::RateControlMode::Quality)
                    .usage_type(openh264::encoder::UsageType::ScreenContentRealTime)
                    .intra_frame_period(openh264::encoder::IntraFramePeriod::from_num_frames(30))
                    .skip_frames(false);

                match Encoder::with_api_config(openh264::OpenH264API::from_source(), config) {
                    Ok(enc) => {
                        h264_encoder = Some(enc);
                        enc_width = width;
                        enc_height = height;
                        println!("[H264] Initialized encoder {}x{}", width, height);
                    }
                    Err(e) => {
                        eprintln!("[H264] Failed to initialize encoder: {:?}", e);
                        continue;
                    }
                }
            }

            let encoder = h264_encoder.as_mut().unwrap();
            let yuv = rgba_to_yuv420(&frame.raw_pixels, width, height);

            frame_number += 1;
            let is_debug_frame = frame_number <= 3 || frame_number == 10 || frame_number == 30 || frame_number == 60 || frame_number % 30 == 0;

            // ========================================================
            // [VIDEO DEBUG] STAGE 4 — H264 ENCODER INPUT
            // ========================================================
            if is_debug_frame {
                println!("[VIDEO DEBUG][H264 INPUT #{}]\nwidth={}\nheight={}\nY_bytes={}\nU_bytes={}\nV_bytes={}",
                    frame_number, width, height, yuv.y.len(), yuv.u.len(), yuv.v.len());
            }

            let bitstream = match encoder.encode(&yuv) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("[H264] Encode error: {:?}", e);
                    continue;
                }
            };

            let h264_bytes = bitstream.to_vec();

            // ========================================================
            // [VIDEO DEBUG] STAGE 4 — H264 ENCODER OUTPUT & NAL SCAN
            // ========================================================
            if is_debug_frame {
                println!("[VIDEO DEBUG][H264 OUTPUT #{}]\nencoded_bytes={}", frame_number, h264_bytes.len());
                if h264_bytes.is_empty() {
                    println!("[VIDEO DEBUG][WARNING] H264 ENCODER RETURNED ZERO BYTES");
                } else {
                    let hex_prefix: Vec<String> = h264_bytes.iter().take(32).map(|b| format!("{:02X}", b)).collect();
                    println!("[VIDEO DEBUG][H264 HEADER #{}]\n{}", frame_number, hex_prefix.join(" "));

                    // Scan NAL units
                    let mut start_codes_found = 0;
                    let mut nal_types = Vec::new();
                    let mut has_sps = false;
                    let mut has_pps = false;
                    let mut has_idr = false;
                    let mut has_sei = false;

                    let mut i = 0;
                    while i + 2 < h264_bytes.len() {
                        let mut sc_len = 0;
                        if i + 3 < h264_bytes.len() && h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 0 && h264_bytes[i+3] == 1 {
                            sc_len = 4;
                        } else if h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 1 {
                            sc_len = 3;
                        }
                        if sc_len > 0 {
                            start_codes_found += 1;
                            if i + sc_len < h264_bytes.len() {
                                let n_type = h264_bytes[i + sc_len] & 0x1F;
                                nal_types.push(n_type);
                                if n_type == 7 { has_sps = true; }
                                if n_type == 8 { has_pps = true; }
                                if n_type == 5 { has_idr = true; }
                                if n_type == 6 { has_sei = true; }
                            }
                            i += sc_len;
                        } else {
                            i += 1;
                        }
                    }
                    println!("[VIDEO DEBUG][H264 NAL #{}]\nstart_codes_found={}\nNAL_types={:?}", frame_number, start_codes_found, nal_types);
                    println!("[VIDEO DEBUG][H264 NAL #{}]\nSPS={}\nPPS={}\nIDR={}\nSEI={}", frame_number, has_sps, has_pps, has_idr, has_sei);
                }
            }

            if h264_bytes.is_empty() {
                // OpenH264 may buffer frames; skip if no output.
                continue;
            }

            // Protocol:
            // [1 byte TYPE] = 15 (H.264 VIDEO)
            // [4 byte WIDTH]
            // [4 byte HEIGHT]
            // [4 byte DATA_SIZE]
            // [H264 DATA...]
            // Header = 13 bytes (type + 3*4)
            let packet_size = 13 + h264_bytes.len();
            let mut packet = Vec::with_capacity(packet_size);

            packet.push(15u8); // TYPE 15 = H.264 VIDEO
            packet.extend_from_slice(&(width as u32).to_be_bytes());
            packet.extend_from_slice(&(height as u32).to_be_bytes());
            packet.extend_from_slice(&(h264_bytes.len() as u32).to_be_bytes());
            packet.extend_from_slice(&h264_bytes);

            // ========================================================
            // [VIDEO DEBUG] STAGE 5 — TYPE 15 PACKET
            // ========================================================
            if is_debug_frame {
                println!("[VIDEO DEBUG][TX #{}]\ntype=15\nwidth={}\nheight={}\nh264_size={}\ntotal_packet_size={}",
                    frame_number, width, height, h264_bytes.len(), packet_size);
                let pkt_hex_prefix: Vec<String> = packet.iter().take(32).map(|b| format!("{:02X}", b)).collect();
                println!("[VIDEO DEBUG][TX HEADER #{}]\n{}", frame_number, pkt_hex_prefix.join(" "));

                let mut hasher = Sha256::new();
                hasher.update(&h264_bytes);
                let h264_hash = format!("{:x}", hasher.finalize());
                println!("[VIDEO DEBUG][TX HASH #{}]\nsha256={}", frame_number, h264_hash);
            }

            if write_stream_frames.send(packet).is_err() {
                eprintln!("[Video] Writer disconnected.");
                is_conn_write.store(false, Ordering::SeqCst);
                break;
            }
        }

        // ====================================================
        // SHUTDOWN
        // ====================================================

        is_connected.store(false, Ordering::SeqCst);
        drop(write_stream);

        let _ = input_handle.join();
        let _ = ping_handle.join();
        let _ = clip_handle.join();
        let _ = capture_handle.join();
        let _ = writer_handle.join();

        backend.log_session_end(&session_id.to_string(), 0.0);

        println!("[Agent] Session ended. Reconnecting...");
        thread::sleep(Duration::from_millis(1000));
    }
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    set_process_dpi_aware();

    let args: Vec<String> = env::args().collect();

    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "192.168.29.229:9001".to_string()
    };

    let session_id: u32 = rand::thread_rng().gen_range(100_000..999_999);
    let session_string = session_id.to_string();
    let id_str = format!(
        "{}-{}",
        &session_string[0..3],
        &session_string[3..6]
    );

    let _ = enable_autostart("ScreenShareAgent");

    println!("========================================");
    println!("       REMOTE DESKTOP AGENT");
    println!("========================================");
    println!("  YOUR REMOTE DESKTOP ID: {}", id_str);
    println!("  Relay: {}", relay_addr);
    println!("  Video: Maximum 1920x1080");
    println!("  Codec: H.264");
    println!("  H.264 Encoder: OpenH264");
    println!("========================================");

    run_agent_loop(relay_addr, session_id, id_str);
}