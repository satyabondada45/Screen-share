pub mod registration {
    pub mod backend_client;
}

use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use enigo::{Axis, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use image::codecs::png::PngEncoder;
use image::ImageEncoder;
use rand::Rng;
use screenshots::Screen;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
fn set_process_dpi_aware() {
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

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

#[cfg(windows)]
fn prompt_connection_dialog(id_str: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    };

    let title: Vec<u16> = "Remote Desktop Request\0".encode_utf16().collect();
    let text: Vec<u16> = format!(
        "Incoming remote control request for ID: {}\n\nDo you want to ALLOW this session?",
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

fn compute_sha256(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn map_key_code(code: u32) -> Option<Key> {
    match code {
        65 => Some(Key::Unicode('a')), 66 => Some(Key::Unicode('b')), 67 => Some(Key::Unicode('c')),
        68 => Some(Key::Unicode('d')), 69 => Some(Key::Unicode('e')), 70 => Some(Key::Unicode('f')),
        71 => Some(Key::Unicode('g')), 72 => Some(Key::Unicode('h')), 73 => Some(Key::Unicode('i')),
        74 => Some(Key::Unicode('j')), 75 => Some(Key::Unicode('k')), 76 => Some(Key::Unicode('l')),
        77 => Some(Key::Unicode('m')), 78 => Some(Key::Unicode('n')), 79 => Some(Key::Unicode('o')),
        80 => Some(Key::Unicode('p')), 81 => Some(Key::Unicode('q')), 82 => Some(Key::Unicode('r')),
        83 => Some(Key::Unicode('s')), 84 => Some(Key::Unicode('t')), 85 => Some(Key::Unicode('u')),
        86 => Some(Key::Unicode('v')), 87 => Some(Key::Unicode('w')), 88 => Some(Key::Unicode('x')),
        89 => Some(Key::Unicode('y')), 90 => Some(Key::Unicode('z')),
        48 => Some(Key::Unicode('0')), 49 => Some(Key::Unicode('1')), 50 => Some(Key::Unicode('2')),
        51 => Some(Key::Unicode('3')), 52 => Some(Key::Unicode('4')), 53 => Some(Key::Unicode('5')),
        54 => Some(Key::Unicode('6')), 55 => Some(Key::Unicode('7')), 56 => Some(Key::Unicode('8')),
        57 => Some(Key::Unicode('9')),
        32 => Some(Key::Space), 13 => Some(Key::Return), 8 => Some(Key::Backspace), 9 => Some(Key::Tab),
        16 => Some(Key::Shift), 17 => Some(Key::Control), 18 => Some(Key::Alt),
        37 => Some(Key::LeftArrow), 38 => Some(Key::UpArrow), 39 => Some(Key::RightArrow), 40 => Some(Key::DownArrow),
        46 => Some(Key::Delete),
        _ => None,
    }
}

struct FrameData {
    width: usize,
    height: usize,
    raw_pixels: Vec<u8>,
}

fn start_audio_capture(
    write_stream: Arc<Mutex<TcpStream>>,
    is_running: Arc<AtomicBool>,
) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = device.default_output_config().ok()?;
    let stream_config: cpal::StreamConfig = config.into();

    let sample_rate = stream_config.sample_rate.0;
    let channels = stream_config.channels;

    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &_| {
                if !is_running.load(Ordering::SeqCst) {
                    return;
                }
                let byte_len = data.len() * 4;
                let mut packet = Vec::with_capacity(11 + byte_len);
                packet.push(13u8);
                packet.extend_from_slice(&(byte_len as u32).to_be_bytes());
                packet.extend_from_slice(&(sample_rate as u32).to_be_bytes());
                packet.extend_from_slice(&(channels as u16).to_be_bytes());

                let slice = unsafe {
                    std::slice::from_raw_parts(data.as_ptr() as *const u8, byte_len)
                };
                packet.extend_from_slice(slice);

                if let Ok(mut s) = write_stream.lock() {
                    let _ = s.write_all(&packet);
                }
            },
            |_| {},
            None,
        )
        .ok()?;

    stream.play().ok()?;
    Some(stream)
}

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
                let _ = s.set_write_timeout(Some(Duration::from_millis(150)));
                s
            }
            Err(e) => {
                eprintln!("[Agent] Relay error: {:?}. Retrying in 5s...", e);
                thread::sleep(Duration::from_secs(8));
                continue;
            }
        };

        let mut register_pkt = vec![1u8];
        register_pkt.extend_from_slice(session_id.to_string().as_bytes());
        if stream.write_all(&register_pkt).is_err() {
            thread::sleep(Duration::from_secs(3));
            continue;
        }

        println!("[Agent] Registered on relay. Waiting for incoming session requests...");

        let mut req_signal = [0u8; 33];
        if stream.read_exact(&mut req_signal[0..1]).is_err() || req_signal[0] != 3 {
            thread::sleep(Duration::from_secs(2));
            continue;
        }

        let _ = stream.read_exact(&mut req_signal[1..33]);
        let incoming_hash = &req_signal[1..33];

        let unattended_pin = env::var("AGENT_PIN").unwrap_or_default();
        let mut authorized = false;

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

        if authorized {
            let _ = stream.write_all(&[1u8]);
            backend.log_session_start(&session_id.to_string());
            println!("[Agent] Session APPROVED! Live PNG streaming active...");
        } else {
            let _ = stream.write_all(&[2u8]);
            println!("[Agent] Session REJECTED.");
            continue;
        }

        let is_connected = Arc::new(AtomicBool::new(true));
        let is_conn_read = Arc::clone(&is_connected);
        let is_conn_write = Arc::clone(&is_connected);
        let is_conn_capture = Arc::clone(&is_connected);
        let is_conn_clip = Arc::clone(&is_connected);
        let is_conn_audio = Arc::clone(&is_connected);
        let is_conn_ping = Arc::clone(&is_connected);

        let mut read_stream = stream.try_clone().unwrap();
        let write_stream = Arc::new(Mutex::new(stream));
        let write_stream_clip = Arc::clone(&write_stream);
        let write_stream_frames = Arc::clone(&write_stream);
        let write_stream_audio = Arc::clone(&write_stream);
        let write_stream_ping = Arc::clone(&write_stream);

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

        // Input and Chat Receiver Thread
        let input_handle = thread::spawn(move || {
            let mut enigo = Enigo::new(&Settings::default()).unwrap();
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
                    0..=8 => {
                        let mut data = [0u8; 8];
                        if read_stream.read_exact(&mut data).is_err() {
                            break;
                        }
                        let event_type = type_buf[0];

                        if event_type == 7 {
                            active_idx_input.store(data[0] as usize, Ordering::SeqCst);
                            continue;
                        }
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

                        let target_x = sx + ((norm_x as f32 / 65535.0) * (sw - 1.0)).round() as i32;
                        let target_y = sy + ((norm_y as f32 / 65535.0) * (sh - 1.0)).round() as i32;

                        #[cfg(windows)]
                        {
                            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                                MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
                                MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
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
                    12 => {
                        let mut len_buf = [0u8; 4];
                        if read_stream.read_exact(&mut len_buf).is_err() {
                            break;
                        }
                        let len = u32::from_be_bytes(len_buf) as usize;

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
                    15 => {
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
                            Err(_) => {
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

                        let mut chunk_buf = vec![0u8; chunk_len];
                        if read_stream.read_exact(&mut chunk_buf).is_err() {
                            break;
                        }

                        if let Some(ref mut file) = current_file {
                            let _ = file.write_all(&chunk_buf);
                            received_bytes += chunk_len as u64;

                            if received_bytes >= total_file_size {
                                let _ = file.flush();
                                current_file = None;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        // Keepalive & RTT Ping Worker
        let ping_handle = thread::spawn(move || {
            while is_conn_ping.load(Ordering::SeqCst) {
                let mut ping_pkt = Vec::with_capacity(9);
                ping_pkt.push(14u8);
                ping_pkt.extend_from_slice(&current_time_millis().to_be_bytes());

                if let Ok(mut s) = write_stream_ping.lock() {
                    if s.write_all(&ping_pkt).is_err() {
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(8));
            }
        });

        // Clipboard Synchronization Worker
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

                            if let Ok(mut s) = write_stream_clip.lock() {
                                if s.write_all(&packet).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_millis(8));
            }
        });

        let _audio = start_audio_capture(write_stream_audio, is_conn_audio);

        // Screen Capture Worker
        let capture_handle = thread::spawn(move || {
            let mut screens = Screen::all().unwrap_or_default();
            let mut last_idx = active_idx_capture.load(Ordering::SeqCst);

            while is_conn_capture.load(Ordering::SeqCst) {
                let start = Instant::now();

                let current_idx = active_idx_capture.load(Ordering::SeqCst);
                if current_idx != last_idx || screens.is_empty() {
                    screens = Screen::all().unwrap_or_default();
                    last_idx = current_idx;
                }

                if screens.is_empty() {
                    thread::sleep(Duration::from_millis(8));
                    continue;
                }

                let screen = match screens.get(current_idx) {
                    Some(s) => s,
                    None => &screens[0],
                };

                if let Ok(img) = screen.capture() {
                    let frame = FrameData {
                        width: img.width() as usize,
                        height: img.height() as usize,
                        raw_pixels: img.into_raw(),
                    };
                    let (lock, cvar) = &*shared_frame_cap;
                    let mut shared = lock.lock().unwrap();
                    *shared = Some(frame);
                    cvar.notify_one();
                }

                let target_interval = Duration::from_millis(16);
                let elapsed = start.elapsed();
                if elapsed < target_interval {
                    thread::sleep(target_interval - elapsed);
                }
            }
        });

        let mut network_batch_buffer = Vec::with_capacity(1024 * 1024);

        // PNG Streaming Loop - FIXED
        while is_conn_write.load(Ordering::SeqCst) {
            let frame_opt = {
                let (lock, cvar) = &*shared_frame;
                let mut shared = lock.lock().unwrap();
                if shared.is_none() {
                    let result = cvar.wait_timeout(shared, Duration::from_millis(50)).unwrap();
                    shared = result.0;
                }
                shared.take()
            };

            if let Some(frame) = frame_opt {
                let (w, h) = (frame.width, frame.height);
                let raw_pixels = frame.raw_pixels;

                if raw_pixels.is_empty() {
                    continue;
                }

                let mut png_bytes = Vec::with_capacity(256 * 1024);
                {
                    let encoder = PngEncoder::new(&mut png_bytes);
                    if encoder.write_image(
                        &raw_pixels,
                        w as u32,
                        h as u32,
                        image::ColorType::Rgba8,
                    ).is_err() {
                        continue;
                    }
                }

                network_batch_buffer.clear();
                network_batch_buffer.push(1u8);
                network_batch_buffer.extend_from_slice(&(w as u32).to_be_bytes());
                network_batch_buffer.extend_from_slice(&(h as u32).to_be_bytes());
                network_batch_buffer.extend_from_slice(&(png_bytes.len() as u32).to_be_bytes());
                network_batch_buffer.extend_from_slice(&png_bytes);

                if let Ok(mut s) = write_stream_frames.lock() {
                    if s.write_all(&network_batch_buffer).is_err() {
                        is_conn_write.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }

        let _ = input_handle.join();
        let _ = ping_handle.join();
        let _ = clip_handle.join();
        let _ = capture_handle.join();

        backend.log_session_end(&session_id.to_string(), 0.0);
    }
}

fn main() {
    set_process_dpi_aware();

    let args: Vec<String> = env::args().collect();
    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "127.0.0.1:9001".to_string()
    };

    let session_id: u32 = rand::thread_rng().gen_range(100_000..999_999);
    let id_str = format!("{}-{}", &session_id.to_string()[0..3], &session_id.to_string()[3..6]);

    let _ = enable_autostart("ScreenShareAgent");

    println!("========================================");
    println!("  YOUR REMOTE DESKTOP ID: {}", id_str);
    println!("  Status: Agent Active");
    println!("========================================");

    run_agent_loop(relay_addr, session_id, id_str);
}