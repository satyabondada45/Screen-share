use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use lz4_flex::decompress_size_prepended;
use minifb::{Key, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn compute_sha256(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

#[inline(always)]
fn pixel_to_u32(chunk: &[u8]) -> u32 {
    if chunk.len() < 4 {
        if chunk.len() == 3 {
            return ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        }
        return 0;
    }
    ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32)
}

fn draw_hud_char(buffer: &mut [u32], buf_w: usize, buf_h: usize, x: usize, y: usize, c: char, color: u32) {
    let bitmap: [u8; 7] = match c.to_ascii_uppercase() {
        '0' => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x1F, 0x01, 0x01, 0x1F, 0x10, 0x10, 0x1F],
        '3' => [0x1F, 0x01, 0x01, 0x1F, 0x01, 0x01, 0x1F],
        '4' => [0x11, 0x11, 0x11, 0x1F, 0x01, 0x01, 0x01],
        '5' => [0x1F, 0x10, 0x10, 0x1F, 0x01, 0x01, 0x1F],
        '6' => [0x1F, 0x10, 0x10, 0x1F, 0x11, 0x11, 0x1F],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x1F, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x1F],
        '9' => [0x1F, 0x11, 0x11, 0x1F, 0x01, 0x01, 0x1F],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0F, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0F],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x11, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        ':' => [0x00, 0x04, 0x00, 0x00, 0x04, 0x00, 0x00],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    };

    for (row, byte) in bitmap.iter().enumerate() {
        for col in 0..5 {
            if (byte >> (4 - col)) & 1 == 1 {
                let px = x + col;
                let py = y + row;
                if px < buf_w && py < buf_h {
                    buffer[py * buf_w + px] = color;
                }
            }
        }
    }
}

fn render_hud_overlay(buffer: &mut [u32], w: usize, h: usize, rtt_ms: u64, fps: u32) {
    if w < 240 || h < 40 { return; }

    let hud_w = 200;
    let hud_h = 24;
    let hud_x = w.saturating_sub(hud_w + 16);
    let hud_y = 12;

    for y in hud_y..(hud_y + hud_h) {
        for x in hud_x..(hud_x + hud_w) {
            if x < w && y < h {
                buffer[y * w + x] = 0x0F172A;
            }
        }
    }

    let text = format!("FPS:{}  RTT:{}MS", fps, rtt_ms);
    let mut cursor_x = hud_x + 12;
    let text_color = if rtt_ms < 50 { 0x22C55E } else { 0xEAB308 };

    for ch in text.chars() {
        draw_hud_char(buffer, w, h, cursor_x, hud_y + 8, ch, text_color);
        cursor_x += 8;
    }
}

fn render_chat_overlay(buffer: &mut [u32], w: usize, h: usize, messages: &[String]) {
    if w < 320 || h < 200 { return; }

    let chat_w = 280;
    let chat_h = 160;
    let chat_x = 16;
    let chat_y = h.saturating_sub(chat_h + 16);

    // Draw dark transluscent background
    for y in chat_y..(chat_y + chat_h) {
        for x in chat_x..(chat_x + chat_w) {
            if x < w && y < h {
                buffer[y * w + x] = 0x0B0F19;
            }
        }
    }

    // Draw Title
    let title = "LIVE CHAT (F6 TO SEND)";
    let mut tx = chat_x + 10;
    for ch in title.chars() {
        draw_hud_char(buffer, w, h, tx, chat_y + 10, ch, 0x38BDF8);
        tx += 8;
    }

    // Render recent messages
    let mut line_y = chat_y + 32;
    for msg in messages.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
        let mut mx = chat_x + 10;
        for ch in msg.chars().take(30) {
            draw_hud_char(buffer, w, h, mx, line_y, ch, 0xF8FAFC);
            mx += 8;
        }
        line_y += 18;
    }
}

fn setup_audio_playback() -> (Option<cpal::Stream>, Arc<Mutex<VecDeque<f32>>>) {
    let queue = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(48000 * 2)));
    let queue_clone = Arc::clone(&queue);

    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => return (None, queue),
    };

    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(_) => return (None, queue),
    };
    let stream_config: cpal::StreamConfig = config.into();

    let stream = device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], _: &_| {
            if let Ok(mut q) = queue_clone.lock() {
                for sample in data.iter_mut() {
                    *sample = q.pop_front().unwrap_or(0.0);
                }
            } else {
                for sample in data.iter_mut() {
                    *sample = 0.0;
                }
            }
        },
        |_| {},
        None,
    ).ok();

    if let Some(ref s) = stream {
        let _ = s.play();
    }

    (stream, queue)
}

fn send_file_async(path_str: String, out_tx: SyncSender<Vec<u8>>) {
    thread::spawn(move || {
        let path = Path::new(&path_str);
        if !path.exists() || !path.is_file() {
            println!("\n[File Transfer] Invalid file path: {}", path_str);
            return;
        }

        let filename = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => return,
        };

        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                println!("\n[File Transfer] Open failed: {:?}", e);
                return;
            }
        };

        let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let name_bytes = filename.as_bytes();

        let mut meta_pkt = Vec::with_capacity(11 + name_bytes.len());
        meta_pkt.push(20u8);
        meta_pkt.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
        meta_pkt.extend_from_slice(&file_size.to_be_bytes());
        meta_pkt.extend_from_slice(name_bytes);

        if out_tx.send(meta_pkt).is_err() { return; }

        println!("\n[File Transfer] Uploading '{}' ({:.2} MB)...", filename, file_size as f64 / (1024.0 * 1024.0));

        let mut buffer = [0u8; 65536];
        let mut sent: u64 = 0;

        loop {
            let bytes_read = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };

            let mut chunk_pkt = Vec::with_capacity(5 + bytes_read);
            chunk_pkt.push(21u8);
            chunk_pkt.extend_from_slice(&(bytes_read as u32).to_be_bytes());
            chunk_pkt.extend_from_slice(&buffer[..bytes_read]);

            if out_tx.send(chunk_pkt).is_err() { break; }

            sent += bytes_read as u64;
            let pct = (sent as f64 / file_size.max(1) as f64) * 100.0;
            print!("\r[File Transfer] Progress: {:.1}%", pct);
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_millis(1));
        }

        println!("\n[File Transfer] Upload complete!");
    });
}

#[derive(Clone)]
struct RenderFrame {
    width: usize,
    height: usize,
    buffer: Vec<u32>,
}

fn main() {
    set_process_dpi_aware();

    let args: Vec<String> = env::args().collect();
    let relay_addr = "127.0.0.1:9001".to_string();

    let target_id = if args.len() > 1 {
        let raw = args[1].trim();
        raw.replace("screenshare://", "")
            .replace('/', "")
            .replace('-', "")
    } else {
        print!("Enter Remote Desktop ID (e.g. 482-910): ");
        let _ = io::stdout().flush();
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);
        input.trim().replace('-', "")
    };

    if target_id.len() != 6 {
        eprintln!("Invalid 6-digit ID format: '{}'", target_id);
        return;
    }

    println!("[Viewer] Connecting to relay at {}...", relay_addr);
    let mut stream = match TcpStream::connect(&relay_addr) {
        Ok(s) => {
            let _ = s.set_nodelay(true);
            s
        }
        Err(e) => {
            eprintln!("[Viewer] Failed to connect to relay server: {:?}", e);
            return;
        }
    };

    let pin = env::var("CONNECT_PIN").unwrap_or_default();
    let auth_hash = compute_sha256(&pin);

    let mut connect_pkt = vec![2u8];
    connect_pkt.extend_from_slice(target_id.as_bytes());
    connect_pkt.extend_from_slice(&auth_hash);
    if stream.write_all(&connect_pkt).is_err() {
        eprintln!("[Viewer] Failed to send initialization handshake.");
        return;
    }

    println!("[Viewer] Request sent for host [{}]. Waiting for ACCEPT...", target_id);

    let mut ack = [0u8; 1];
    if stream.read_exact(&mut ack).is_err() {
        eprintln!("[Viewer] Connection dropped by relay.");
        return;
    }

    match ack[0] {
        1 => println!("[Viewer] Connection APPROVED by host! Streaming active..."),
        2 => {
            eprintln!("[Viewer] Connection REJECTED by host or Invalid Security PIN.");
            return;
        }
        _ => {
            eprintln!("[Viewer] Session ID not found or remote host is offline.");
            return;
        }
    }

    let is_connected = Arc::new(AtomicBool::new(true));
    let is_conn_read = Arc::clone(&is_connected);
    let is_conn_write = Arc::clone(&is_connected);
    let is_conn_clip = Arc::clone(&is_connected);

    let live_rtt = Arc::new(AtomicU64::new(12));
    let live_rtt_inbound = Arc::clone(&live_rtt);

    let chat_messages = Arc::new(Mutex::new(Vec::<String>::new()));
    let chat_inbound = Arc::clone(&chat_messages);

    let mut read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Viewer] Stream clone error: {:?}", e);
            return;
        }
    };
    let mut write_stream = stream;

    let last_clipboard = Arc::new(Mutex::new(String::new()));
    let last_clip_recv = Arc::clone(&last_clipboard);
    let last_clip_send = Arc::clone(&last_clipboard);

    let (_audio_stream, audio_queue) = setup_audio_playback();
    let (frame_tx, frame_rx): (SyncSender<RenderFrame>, Receiver<RenderFrame>) = sync_channel(1);
    let (out_tx, out_rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) = sync_channel(2048);

    let out_tx_clip = out_tx.clone();
    let out_tx_file = out_tx.clone();
    let out_tx_pong = out_tx.clone();
    let out_tx_chat = out_tx.clone();

    // Outbound Network Worker
    thread::spawn(move || {
        while is_conn_write.load(Ordering::SeqCst) {
            if let Ok(pkt) = out_rx.recv() {
                if write_stream.write_all(&pkt).is_err() {
                    is_conn_write.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
    });

    // Inbound Network Receiver Thread
    thread::spawn(move || {
        let mut clip = Clipboard::new().ok();
        let mut current_pixel_buffer: Vec<u32> = Vec::new();
        let mut current_width = 0usize;
        let mut current_height = 0usize;

        while is_conn_read.load(Ordering::SeqCst) {
            let mut type_buf = [0u8; 1];
            if read_stream.read_exact(&mut type_buf).is_err() {
                is_conn_read.store(false, Ordering::SeqCst);
                break;
            }

            match type_buf[0] {
                1 => {
                    let mut header_buf = [0u8; 12];
                    if read_stream.read_exact(&mut header_buf).is_err() { break; }

                    let width = u32::from_be_bytes(header_buf[0..4].try_into().unwrap()) as usize;
                    let height = u32::from_be_bytes(header_buf[4..8].try_into().unwrap()) as usize;
                    let size = u32::from_be_bytes(header_buf[8..12].try_into().unwrap()) as usize;

                    if size == 0 || size > 60_000_000 || width == 0 || height == 0 { continue; }

                    let mut compressed_payload = vec![0u8; size];
                    if read_stream.read_exact(&mut compressed_payload).is_err() { break; }

                    let raw_bytes = match decompress_size_prepended(&compressed_payload) {
                        Ok(bytes) => bytes,
                        Err(_) => continue,
                    };

                    current_width = width;
                    current_height = height;
                    let total_pixels = current_width * current_height;

                    if current_pixel_buffer.len() != total_pixels {
                        current_pixel_buffer = vec![0u32; total_pixels];
                    }

                    for (i, chunk) in raw_bytes.chunks_exact(4).enumerate() {
                        if i < total_pixels {
                            current_pixel_buffer[i] = pixel_to_u32(chunk);
                        }
                    }

                    let _ = frame_tx.try_send(RenderFrame {
                        width: current_width,
                        height: current_height,
                        buffer: current_pixel_buffer.clone(),
                    });
                }
                2 => {
                    let mut count_buf = [0u8; 2];
                    if read_stream.read_exact(&mut count_buf).is_err() { break; }
                    let tile_count = u16::from_be_bytes(count_buf) as usize;

                    for _ in 0..tile_count {
                        let mut meta = [0u8; 8];
                        if read_stream.read_exact(&mut meta).is_err() { break; }

                        let col = u16::from_be_bytes(meta[0..2].try_into().unwrap()) as usize;
                        let row = u16::from_be_bytes(meta[2..4].try_into().unwrap()) as usize;
                        let size = u32::from_be_bytes(meta[4..8].try_into().unwrap()) as usize;

                        let mut tile_payload = vec![0u8; size];
                        if read_stream.read_exact(&mut tile_payload).is_err() { break; }

                        if current_width == 0 || current_height == 0 { continue; }

                        if let Ok(raw_tile) = decompress_size_prepended(&tile_payload) {
                            let start_x = col * TILE_SIZE;
                            let start_y = row * TILE_SIZE;

                            if start_x >= current_width || start_y >= current_height { continue; }

                            let actual_w = (current_width - start_x).min(TILE_SIZE);
                            let actual_h = (current_height - start_y).min(TILE_SIZE);

                            for y in 0..actual_h {
                                let target_y = start_y + y;
                                if target_y >= current_height { break; }

                                for x in 0..actual_w {
                                    let target_x = start_x + x;
                                    if target_x >= current_width { break; }

                                    let src_idx = (y * actual_w + x) * 4;
                                    if src_idx + 4 <= raw_tile.len() {
                                        let pixel = pixel_to_u32(&raw_tile[src_idx..src_idx + 4]);
                                        let dst_idx = target_y * current_width + target_x;
                                        if dst_idx < current_pixel_buffer.len() {
                                            current_pixel_buffer[dst_idx] = pixel;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if current_width > 0 && current_height > 0 && current_pixel_buffer.len() == (current_width * current_height) {
                        let _ = frame_tx.try_send(RenderFrame {
                            width: current_width,
                            height: current_height,
                            buffer: current_pixel_buffer.clone(),
                        });
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
                13 => {
                    let mut len_buf = [0u8; 4];
                    if read_stream.read_exact(&mut len_buf).is_err() { break; }
                    let len = u32::from_be_bytes(len_buf) as usize;

                    let mut audio_bytes = vec![0u8; len];
                    if read_stream.read_exact(&mut audio_bytes).is_err() { break; }

                    if let Ok(mut q) = audio_queue.lock() {
                        if q.len() < 48000 {
                            for chunk in audio_bytes.chunks_exact(4) {
                                let sample = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                                q.push_back(sample);
                            }
                        }
                    }
                }
                14 => {
                    let mut time_buf = [0u8; 8];
                    if read_stream.read_exact(&mut time_buf).is_err() { break; }
                    let sent_time = u64::from_be_bytes(time_buf);
                    let now = current_time_millis();
                    if now >= sent_time {
                        live_rtt_inbound.store(now - sent_time, Ordering::SeqCst);
                    }

                    let mut pong_pkt = Vec::with_capacity(9);
                    pong_pkt.push(15u8);
                    pong_pkt.extend_from_slice(&time_buf);
                    let _ = out_tx_pong.send(pong_pkt);
                }
                // Chat Packet Receiver
                16 => {
                    let mut meta = [0u8; 3]; // 1B sender + 2B length
                    if read_stream.read_exact(&mut meta).is_err() { break; }
                    let sender = meta[0];
                    let len = u16::from_be_bytes([meta[1], meta[2]]) as usize;

                    let mut msg_bytes = vec![0u8; len];
                    if read_stream.read_exact(&mut msg_bytes).is_err() { break; }

                    if let Ok(txt) = String::from_utf8(msg_bytes) {
                        let sender_tag = if sender == 0 { "HOST" } else { "YOU" };
                        let formatted = format!("{}: {}", sender_tag, txt);
                        if let Ok(mut list) = chat_inbound.lock() {
                            list.push(formatted);
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // Outbound Clipboard Sync Thread
    let clip_thread = thread::spawn(move || {
        let mut local_clip = Clipboard::new().ok();
        while is_conn_clip.load(Ordering::SeqCst) {
            if let Some(ref mut c) = local_clip {
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
                        let _ = out_tx_clip.send(packet);
                    }
                }
            }
            thread::sleep(Duration::from_millis(300));
        }
    });

    // Window Rendering & Chat Event Loop
    let mut window: Option<Window> = None;
    let mut current_frame: Option<RenderFrame> = None;
    let mut prev_left = false;
    let mut prev_right = false;
    let mut last_norm_x: Option<u16> = None;
    let mut last_norm_y: Option<u16> = None;

    let mut fps_counter: u32 = 0;
    let mut displayed_fps: u32 = 60;
    let mut last_fps_time = Instant::now();

    while is_connected.load(Ordering::SeqCst) {
        if let Ok(frame) = frame_rx.try_recv() {
            current_frame = Some(frame);
        }

        if let Some(ref mut frame) = current_frame {
            let (w, h) = (frame.width, frame.height);

            if w == 0 || h == 0 || frame.buffer.len() != (w * h) {
                thread::sleep(Duration::from_millis(5));
                continue;
            }

            fps_counter += 1;
            if last_fps_time.elapsed() >= Duration::from_secs(1) {
                displayed_fps = fps_counter;
                fps_counter = 0;
                last_fps_time = Instant::now();
            }

            let rtt = live_rtt.load(Ordering::SeqCst);
            render_hud_overlay(&mut frame.buffer, w, h, rtt, displayed_fps);

            if let Ok(messages) = chat_messages.lock() {
                if !messages.is_empty() {
                    render_chat_overlay(&mut frame.buffer, w, h, &messages);
                }
            }

            let win = window.get_or_insert_with(|| {
                let mut opts = WindowOptions::default();
                opts.scale_mode = ScaleMode::AspectRatioStretch;
                opts.resize = true;
                let mut created_win = Window::new("Screen Share Client (F5: Send File | F6: Chat | F1-F3: Monitors)", w, h, opts)
                    .unwrap_or_else(|e| panic!("Failed to initialize Window: {}", e));
                created_win.limit_update_rate(Some(Duration::from_micros(16600)));
                created_win
            });

            if !win.is_open() || win.is_key_down(Key::Escape) {
                break;
            }

            if win.update_with_buffer(&frame.buffer, w, h).is_err() {
                break;
            }

            // Keyboard Handling
            for key in win.get_keys_pressed(minifb::KeyRepeat::No) {
                match key {
                    Key::F1 => { let _ = out_tx.send(vec![7u8, 0, 0, 0, 0, 0, 0, 0, 0]); }
                    Key::F2 => { let _ = out_tx.send(vec![7u8, 1, 0, 0, 0, 0, 0, 0, 0]); }
                    Key::F3 => { let _ = out_tx.send(vec![7u8, 2, 0, 0, 0, 0, 0, 0, 0]); }
                    Key::F5 => {
                        print!("\nEnter file path to transfer: ");
                        let _ = io::stdout().flush();
                        let mut file_path = String::new();
                        if io::stdin().read_line(&mut file_path).is_ok() {
                            let clean_path = file_path.trim().trim_matches('"').to_string();
                            if !clean_path.is_empty() {
                                send_file_async(clean_path, out_tx_file.clone());
                            }
                        }
                    }
                    Key::F6 => {
                        print!("\nEnter message for remote host: ");
                        let _ = io::stdout().flush();
                        let mut chat_input = String::new();
                        if io::stdin().read_line(&mut chat_input).is_ok() {
                            let clean_msg = chat_input.trim().to_string();
                            if !clean_msg.is_empty() {
                                let msg_bytes = clean_msg.as_bytes();
                                let mut packet = Vec::with_capacity(4 + msg_bytes.len());
                                packet.push(16u8);
                                packet.push(1u8); // 1 = Viewer
                                packet.extend_from_slice(&(msg_bytes.len() as u16).to_be_bytes());
                                packet.extend_from_slice(msg_bytes);
                                let _ = out_tx_chat.send(packet);

                                if let Ok(mut list) = chat_messages.lock() {
                                    list.push(format!("YOU: {}", clean_msg));
                                }
                            }
                        }
                    }
                    _ => {
                        let mut p = vec![5u8];
                        p.extend_from_slice(&(key as u32).to_be_bytes());
                        p.extend_from_slice(&[0u8; 4]);
                        let _ = out_tx.send(p);
                    }
                }
            }

            for key in win.get_keys_released() {
                if key != Key::F5 && key != Key::F6 && key != Key::F1 && key != Key::F2 && key != Key::F3 {
                    let mut pkt = vec![6u8];
                    pkt.extend_from_slice(&(key as u32).to_be_bytes());
                    pkt.extend_from_slice(&[0u8; 4]);
                    let _ = out_tx.send(pkt);
                }
            }

            // Mouse Scroll
            if let Some((sx, sy)) = win.get_scroll_wheel() {
                if sx.abs() > 0.0 || sy.abs() > 0.0 {
                    let scroll_x = (sx * 120.0) as i16;
                    let scroll_y = (sy * 120.0) as i16;
                    let mut pkt = vec![8u8];
                    pkt.extend_from_slice(&scroll_x.to_be_bytes());
                    pkt.extend_from_slice(&scroll_y.to_be_bytes());
                    pkt.extend_from_slice(&[0u8; 4]);
                    let _ = out_tx.send(pkt);
                }
            }

            // Mouse Positioning & Clicks
            if win.is_active() {
                if let Some((mx, my)) = win.get_mouse_pos(MouseMode::Discard) {
                    if mx >= 0.0 && my >= 0.0 && mx < w as f32 && my < h as f32 {
                        let norm_x = ((mx / w as f32) * 65535.0).clamp(0.0, 65535.0) as u16;
                        let norm_y = ((my / h as f32) * 65535.0).clamp(0.0, 65535.0) as u16;

                        let should_move = match (last_norm_x, last_norm_y) {
                            (Some(lx), Some(ly)) => (norm_x as i32 - lx as i32).abs() >= 10 || (norm_y as i32 - ly as i32).abs() >= 10,
                            _ => true,
                        };

                        if should_move {
                            let mut move_pkt = vec![0u8];
                            move_pkt.extend_from_slice(&norm_x.to_be_bytes());
                            move_pkt.extend_from_slice(&norm_y.to_be_bytes());
                            move_pkt.extend_from_slice(&[0u8; 4]);
                            let _ = out_tx.send(move_pkt);
                            last_norm_x = Some(norm_x);
                            last_norm_y = Some(norm_y);
                        }

                        let left_down = win.get_mouse_down(MouseButton::Left);
                        if left_down && !prev_left {
                            let mut pkt = vec![1u8];
                            pkt.extend_from_slice(&norm_x.to_be_bytes());
                            pkt.extend_from_slice(&norm_y.to_be_bytes());
                            pkt.extend_from_slice(&[0u8; 4]);
                            let _ = out_tx.send(pkt);
                        } else if !left_down && prev_left {
                            let mut pkt = vec![2u8];
                            pkt.extend_from_slice(&norm_x.to_be_bytes());
                            pkt.extend_from_slice(&norm_y.to_be_bytes());
                            pkt.extend_from_slice(&[0u8; 4]);
                            let _ = out_tx.send(pkt);
                        }
                        prev_left = left_down;

                        let right_down = win.get_mouse_down(MouseButton::Right);
                        if right_down && !prev_right {
                            let mut pkt = vec![3u8];
                            pkt.extend_from_slice(&norm_x.to_be_bytes());
                            pkt.extend_from_slice(&norm_y.to_be_bytes());
                            pkt.extend_from_slice(&[0u8; 4]);
                            let _ = out_tx.send(pkt);
                        } else if !right_down && prev_right {
                            let mut pkt = vec![4u8];
                            pkt.extend_from_slice(&norm_x.to_be_bytes());
                            pkt.extend_from_slice(&norm_y.to_be_bytes());
                            pkt.extend_from_slice(&[0u8; 4]);
                            let _ = out_tx.send(pkt);
                        }
                        prev_right = right_down;
                    }
                }
            } else {
                if prev_left {
                    let _ = out_tx.send(vec![2u8, 0, 0, 0, 0, 0, 0, 0, 0]);
                    prev_left = false;
                }
                if prev_right {
                    let _ = out_tx.send(vec![4u8, 0, 0, 0, 0, 0, 0, 0, 0]);
                    prev_right = false;
                }
            }
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }

    is_connected.store(false, Ordering::SeqCst);
    let _ = clip_thread.join();
}