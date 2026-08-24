pub mod registration {
    pub mod backend_client;
}

use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rand::Rng;
use screenshots::Screen;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use enigo::{Axis, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use lz4_flex::compress_prepend_size;

const TILE_SIZE: usize = 64;

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

#[cfg(not(windows))]
fn set_native_cursor_pos(_x: i32, _y: i32) {}

#[cfg(not(windows))]
fn send_native_mouse_click(_flags: u32) {}

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

fn hash_slice(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

fn extract_tile(src: &[u8], frame_w: usize, frame_h: usize, col: usize, row: usize) -> Vec<u8> {
    let start_x = col * TILE_SIZE;
    let start_y = row * TILE_SIZE;
    let actual_w = (frame_w.saturating_sub(start_x)).min(TILE_SIZE);
    let actual_h = (frame_h.saturating_sub(start_y)).min(TILE_SIZE);

    let mut tile_buf = Vec::with_capacity(actual_w * actual_h * 4);
    for y in 0..actual_h {
        let offset = ((start_y + y) * frame_w + start_x) * 4;
        let line = &src[offset..offset + (actual_w * 4)];
        tile_buf.extend_from_slice(line);
    }
    tile_buf
}

fn start_audio_capture(
    write_stream: Arc<Mutex<TcpStream>>, 
    is_running: Arc<AtomicBool>
) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = device.default_output_config().ok()?;
    let stream_config: cpal::StreamConfig = config.into();

    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &_| {
            if !is_running.load(Ordering::SeqCst) { return; }
            let byte_len = data.len() * 4;
            let mut packet = Vec::with_capacity(5 + byte_len);
            packet.push(13u8);
            packet.extend_from_slice(&(byte_len as u32).to_be_bytes());

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
    ).ok()?;

    stream.play().ok()?;
    Some(stream)
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

    println!("========================================");
    println!("  YOUR REMOTE DESKTOP ID: {}", id_str);
    println!("========================================");

    // Backend Registration & Heartbeat
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

    println!("[Agent] Connecting to relay {}...", relay_addr);

    let mut stream = match TcpStream::connect(&relay_addr) {
        Ok(s) => {
            let _ = s.set_nodelay(true);
            s
        }
        Err(e) => {
            eprintln!("[Agent] Failed to connect to relay server: {:?}", e);
            return;
        }
    };

    let mut register_pkt = vec![1u8];
    register_pkt.extend_from_slice(session_id.to_string().as_bytes());
    stream.write_all(&register_pkt).unwrap();

    println!("[Agent] Waiting for remote viewer connection...");

    let mut req_signal = [0u8; 1];
    if stream.read_exact(&mut req_signal).is_err() || req_signal[0] != 3 {
        eprintln!("[Agent] Relay connection dropped.");
        return;
    }

    println!("\n========================================");
    println!("  INCOMING REMOTE CONTROL REQUEST!      ");
    println!("========================================");
    print!("Accept connection from remote viewer? (y/n): ");
    let _ = io::stdout().flush();

    let mut choice = String::new();
    let _ = io::stdin().read_line(&mut choice);

    if choice.trim().eq_ignore_ascii_case("y") {
        let _ = stream.write_all(&[1u8]);
        backend.log_session_start(&session_id.to_string());
        println!("[Agent] Access APPROVED! Live streaming started...\n");
    } else {
        let _ = stream.write_all(&[0u8]);
        println!("[Agent] Access REJECTED. Session terminated.");
        return;
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
    let target_interval_ms = Arc::new(AtomicU64::new(16));
    let current_rtt_in = Arc::clone(&current_rtt_ms);
    let target_interval_cap = Arc::clone(&target_interval_ms);

    let active_screen_idx = Arc::new(AtomicUsize::new(0));
    let active_idx_input = Arc::clone(&active_screen_idx);
    let active_idx_capture = Arc::clone(&active_screen_idx);

    let (tx, rx) = sync_channel::<FrameData>(1);
    let last_clipboard_text = Arc::new(Mutex::new(String::new()));
    let last_clip_recv = Arc::clone(&last_clipboard_text);
    let last_clip_send = Arc::clone(&last_clipboard_text);

    // 1. Thread: Input, Clipboard & File Stream Receiver
    let input_handle = thread::spawn(move || {
        let mut enigo = Enigo::new(&Settings::default()).unwrap();
        let mut clip = Clipboard::new().ok();

        let mut current_file: Option<File> = None;
        let mut current_filename = String::new();
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
                    if read_stream.read_exact(&mut data).is_err() { break; }
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
                            MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
                        };

                        match event_type {
                            0 => {
                                set_native_cursor_pos(target_x, target_y);
                            }
                            1 => {
                                set_native_cursor_pos(target_x, target_y);
                                send_native_mouse_click(MOUSEEVENTF_LEFTDOWN);
                            }
                            2 => {
                                send_native_mouse_click(MOUSEEVENTF_LEFTUP);
                            }
                            3 => {
                                set_native_cursor_pos(target_x, target_y);
                                send_native_mouse_click(MOUSEEVENTF_RIGHTDOWN);
                            }
                            4 => {
                                send_native_mouse_click(MOUSEEVENTF_RIGHTUP);
                            }
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
                    if read_stream.read_exact(&mut len_buf).is_err() { break; }
                    let len = u32::from_be_bytes(len_buf) as usize;

                    let mut text_buf = vec![0u8; len];
                    if read_stream.read_exact(&mut text_buf).is_err() { break; }

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
                    if read_stream.read_exact(&mut time_buf).is_err() { break; }
                    let sent_time = u64::from_be_bytes(time_buf);
                    let now = current_time_millis();
                    if now >= sent_time {
                        let rtt = now - sent_time;
                        current_rtt_in.store(rtt, Ordering::SeqCst);
                    }
                }
                20 => {
                    let mut meta_hdr = [0u8; 10];
                    if read_stream.read_exact(&mut meta_hdr).is_err() { break; }

                    let name_len = u16::from_be_bytes(meta_hdr[0..2].try_into().unwrap()) as usize;
                    total_file_size = u64::from_be_bytes(meta_hdr[2..10].try_into().unwrap());

                    let mut name_buf = vec![0u8; name_len];
                    if read_stream.read_exact(&mut name_buf).is_err() { break; }

                    current_filename = String::from_utf8_lossy(&name_buf).to_string();
                    let target_path = drop_dir.join(&current_filename);

                    match File::create(&target_path) {
                        Ok(f) => {
                            current_file = Some(f);
                            received_bytes = 0;
                            println!("\n[File Transfer] Receiving '{}' ({:.2} MB)...", current_filename, total_file_size as f64 / (1024.0 * 1024.0));
                        }
                        Err(e) => {
                            eprintln!("[File Transfer] Create error: {:?}", e);
                            current_file = None;
                        }
                    }
                }
                21 => {
                    let mut chunk_len_buf = [0u8; 4];
                    if read_stream.read_exact(&mut chunk_len_buf).is_err() { break; }
                    let chunk_len = u32::from_be_bytes(chunk_len_buf) as usize;

                    let mut chunk_buf = vec![0u8; chunk_len];
                    if read_stream.read_exact(&mut chunk_buf).is_err() { break; }

                    if let Some(ref mut file) = current_file {
                        let _ = file.write_all(&chunk_buf);
                        received_bytes += chunk_len as u64;

                        let pct = (received_bytes as f64 / total_file_size.max(1) as f64) * 100.0;
                        print!("\r[File Transfer] Progress: {:.1}% ({}/{} bytes)", pct, received_bytes, total_file_size);
                        let _ = io::stdout().flush();

                        if received_bytes >= total_file_size {
                            let _ = file.flush();
                            current_file = None;
                            println!("\n[File Transfer] Done! Saved to RemoteDrop/{}", current_filename);
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // 2. Thread: Latency Ping Probe (500ms Interval)
    let ping_handle = thread::spawn(move || {
        while is_conn_ping.load(Ordering::SeqCst) {
            let mut ping_pkt = Vec::with_capacity(9);
            ping_pkt.push(14u8);
            ping_pkt.extend_from_slice(&current_time_millis().to_be_bytes());

            if let Ok(mut s) = write_stream_ping.lock() {
                if s.write_all(&ping_pkt).is_err() { break; }
            }
            thread::sleep(Duration::from_millis(500));
        }
    });

    // 3. Thread: Outbound Clipboard Polling
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
                            if s.write_all(&packet).is_err() { break; }
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(300));
        }
    });

    // 4. Audio Loopback
    let _audio = start_audio_capture(write_stream_audio, is_conn_audio);

    // 5. Screen Capture Engine (Adaptive FPS)
    let capture_handle = thread::spawn(move || {
        while is_conn_capture.load(Ordering::SeqCst) {
            let start = Instant::now();

            let rtt = current_rtt_ms.load(Ordering::SeqCst);
            let frame_delay_ms = if rtt < 35 {
                16
            } else if rtt < 80 {
                33
            } else {
                66
            };
            target_interval_cap.store(frame_delay_ms, Ordering::SeqCst);

            let current_idx = active_idx_capture.load(Ordering::SeqCst);
            let screens = Screen::all().unwrap_or_default();
            if screens.is_empty() {
                thread::sleep(Duration::from_millis(50));
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
                let _ = tx.try_send(frame);
            }

            let target_interval = Duration::from_millis(frame_delay_ms);
            let elapsed = start.elapsed();
            if elapsed < target_interval {
                thread::sleep(target_interval - elapsed);
            }
        }
    });

    // 6. Dirty Tile Network Encoder & Transmission Loop
    let mut previous_tile_hashes: Vec<u64> = Vec::new();
    let mut frame_count: u64 = 0;
    let mut last_dim = (0usize, 0usize);
    let mut network_batch_buffer = Vec::with_capacity(1024 * 1024);

    while is_conn_write.load(Ordering::SeqCst) {
        if let Ok(frame) = rx.recv_timeout(Duration::from_millis(50)) {
            let (w, h) = (frame.width, frame.height);
            let cols = (w + TILE_SIZE - 1) / TILE_SIZE;
            let rows = (h + TILE_SIZE - 1) / TILE_SIZE;
            let total_tiles = cols * rows;

            frame_count += 1;
            let force_keyframe = frame_count % 150 == 0 || last_dim != (w, h) || previous_tile_hashes.len() != total_tiles;

            if force_keyframe {
                last_dim = (w, h);
                let compressed = compress_prepend_size(&frame.raw_pixels);

                network_batch_buffer.clear();
                network_batch_buffer.push(1u8);
                network_batch_buffer.extend_from_slice(&(w as u32).to_be_bytes());
                network_batch_buffer.extend_from_slice(&(h as u32).to_be_bytes());
                network_batch_buffer.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
                network_batch_buffer.extend_from_slice(&compressed);

                if let Ok(mut s) = write_stream_frames.lock() {
                    if s.write_all(&network_batch_buffer).is_err() {
                        is_conn_write.store(false, Ordering::SeqCst);
                        break;
                    }
                }

                previous_tile_hashes = vec![0u64; total_tiles];
                for r in 0..rows {
                    for c in 0..cols {
                        let tile_bytes = extract_tile(&frame.raw_pixels, w, h, c, r);
                        previous_tile_hashes[r * cols + c] = hash_slice(&tile_bytes);
                    }
                }
            } else {
                let mut dirty_tiles = Vec::new();
                let mut current_hashes = vec![0u64; total_tiles];

                for r in 0..rows {
                    for c in 0..cols {
                        let tile_bytes = extract_tile(&frame.raw_pixels, w, h, c, r);
                        let h_val = hash_slice(&tile_bytes);
                        current_hashes[r * cols + c] = h_val;

                        if h_val != previous_tile_hashes[r * cols + c] {
                            let compressed_tile = compress_prepend_size(&tile_bytes);
                            dirty_tiles.push((c as u16, r as u16, compressed_tile));
                        }
                    }
                }

                if dirty_tiles.is_empty() {
                    if let Ok(mut s) = write_stream_frames.lock() {
                        if s.write_all(&[0u8]).is_err() {
                            is_conn_write.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                } else if dirty_tiles.len() > (total_tiles * 4) / 10 {
                    let compressed = compress_prepend_size(&frame.raw_pixels);
                    network_batch_buffer.clear();
                    network_batch_buffer.push(1u8);
                    network_batch_buffer.extend_from_slice(&(w as u32).to_be_bytes());
                    network_batch_buffer.extend_from_slice(&(h as u32).to_be_bytes());
                    network_batch_buffer.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
                    network_batch_buffer.extend_from_slice(&compressed);

                    if let Ok(mut s) = write_stream_frames.lock() {
                        if s.write_all(&network_batch_buffer).is_err() {
                            is_conn_write.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                    previous_tile_hashes = current_hashes;
                } else {
                    network_batch_buffer.clear();
                    network_batch_buffer.push(2u8);
                    network_batch_buffer.extend_from_slice(&(dirty_tiles.len() as u16).to_be_bytes());

                    for (tx_c, ty_r, comp_data) in dirty_tiles {
                        network_batch_buffer.extend_from_slice(&tx_c.to_be_bytes());
                        network_batch_buffer.extend_from_slice(&ty_r.to_be_bytes());
                        network_batch_buffer.extend_from_slice(&(comp_data.len() as u32).to_be_bytes());
                        network_batch_buffer.extend_from_slice(&comp_data);
                    }

                    if let Ok(mut s) = write_stream_frames.lock() {
                        if s.write_all(&network_batch_buffer).is_err() {
                            is_conn_write.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                    previous_tile_hashes = current_hashes;
                }
            }
        }
    }

    let _ = input_handle.join();
    let _ = ping_handle.join();
    let _ = clip_handle.join();
    let _ = capture_handle.join();
}