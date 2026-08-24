use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use lz4_flex::decompress_size_prepended;
use minifb::{Key, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use std::collections::VecDeque;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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

#[inline(always)]
fn pixel_to_u32(chunk: &[u8]) -> u32 {
    if chunk.len() < 3 { return 0; }
    let r = chunk[0] as u32;
    let g = chunk[1] as u32;
    let b = chunk[2] as u32;
    (r << 16) | (g << 8) | b
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

        println!("\n[File Transfer] Streaming '{}' ({:.2} MB)...", filename, file_size as f64 / (1024.0 * 1024.0));

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
            print!("\r[File Transfer] Uploading: {:.1}%", pct);
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_millis(1));
        }

        println!("\n[File Transfer] Upload complete!");
    });
}

struct RenderFrame {
    width: usize,
    height: usize,
    buffer: Vec<u32>,
}

fn main() {
    set_process_dpi_aware();

    let args: Vec<String> = env::args().collect();
    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "127.0.0.1:9001".to_string()
    };

    print!("Enter Remote Desktop ID (e.g. 482-910): ");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let target_id = input.trim().replace('-', "");

    if target_id.len() != 6 {
        eprintln!("Invalid 6-digit ID format.");
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

    let mut connect_pkt = vec![2u8];
    connect_pkt.extend_from_slice(target_id.as_bytes());
    stream.write_all(&connect_pkt).unwrap();

    println!("[Viewer] Request sent. Waiting for remote host to ACCEPT...");

    let mut ack = [0u8; 1];
    if stream.read_exact(&mut ack).is_err() {
        eprintln!("[Viewer] Connection error.");
        return;
    }

    match ack[0] {
        1 => println!("[Viewer] Connection APPROVED by host! Streaming active..."),
        2 => {
            eprintln!("[Viewer] Connection REJECTED by the remote host.");
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

    let mut read_stream = stream.try_clone().unwrap();
    let mut write_stream = stream;

    let last_clipboard = Arc::new(Mutex::new(String::new()));
    let last_clip_recv = Arc::clone(&last_clipboard);
    let last_clip_send = Arc::clone(&last_clipboard);

    let (_audio_stream, audio_queue) = setup_audio_playback();
    let (frame_tx, frame_rx): (SyncSender<RenderFrame>, Receiver<RenderFrame>) = sync_channel(2);
    let (out_tx, out_rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) = sync_channel(2048);

    let out_tx_clip = out_tx.clone();
    let out_tx_file = out_tx.clone();
    let out_tx_pong = out_tx.clone();

    // 1. Dedicated Outbound Network Thread
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

    // 2. Dedicated Inbound Network Receiver Thread (Heap-Safe)
    thread::spawn(move || {
        let mut clip = Clipboard::new().ok();
        let mut current_pixel_buffer: Vec<u32> = Vec::new();
        let mut current_width = 0;
        let mut current_height = 0;

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

                    if size == 0 || size > 60_000_000 { continue; }

                    let mut compressed_payload = vec![0u8; size];
                    if read_stream.read_exact(&mut compressed_payload).is_err() { break; }

                    let raw_bytes = match decompress_size_prepended(&compressed_payload) {
                        Ok(bytes) => bytes,
                        Err(_) => continue,
                    };

                    current_width = width;
                    current_height = height;
                    let total_pixels = current_width * current_height;
                    current_pixel_buffer.resize(total_pixels, 0);

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

                        if let Ok(raw_tile) = decompress_size_prepended(&tile_payload) {
                            let start_x = col * TILE_SIZE;
                            let start_y = row * TILE_SIZE;
                            let actual_w = (current_width.saturating_sub(start_x)).min(TILE_SIZE);
                            let actual_h = (current_height.saturating_sub(start_y)).min(TILE_SIZE);

                            for y in 0..actual_h {
                                for x in 0..actual_w {
                                    let src_idx = (y * actual_w + x) * 4;
                                    if src_idx + 4 <= raw_tile.len() {
                                        let pixel = pixel_to_u32(&raw_tile[src_idx..src_idx + 4]);
                                        let dst_idx = (start_y + y) * current_width + (start_x + x);
                                        if dst_idx < current_pixel_buffer.len() {
                                            current_pixel_buffer[dst_idx] = pixel;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let _ = frame_tx.try_send(RenderFrame {
                        width: current_width,
                        height: current_height,
                        buffer: current_pixel_buffer.clone(),
                    });
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
                // Memory-Safe Audio Buffer Parsing (No Unaligned Pointer Casts)
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

                    let mut pong_pkt = Vec::with_capacity(9);
                    pong_pkt.push(15u8);
                    pong_pkt.extend_from_slice(&time_buf);
                    let _ = out_tx_pong.send(pong_pkt);
                }
                _ => {}
            }
        }
    });

    // 3. Outbound Clipboard Thread
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

    // 4. UI Rendering Loop
    let mut window: Option<Window> = None;
    let mut current_frame: Option<RenderFrame> = None;
    let mut prev_left = false;
    let mut prev_right = false;
    let mut last_norm_x: Option<u16> = None;
    let mut last_norm_y: Option<u16> = None;

    while is_connected.load(Ordering::SeqCst) {
        while let Ok(frame) = frame_rx.try_recv() {
            current_frame = Some(frame);
        }

        if let Some(ref frame) = current_frame {
            let (w, h) = (frame.width, frame.height);

            if w == 0 || h == 0 || frame.buffer.len() != (w * h) {
                thread::sleep(Duration::from_millis(5));
                continue;
            }

            let win = window.get_or_insert_with(|| {
                let mut opts = WindowOptions::default();
                opts.scale_mode = ScaleMode::AspectRatioStretch;
                opts.resize = true;
                let mut created_win = Window::new("Screen Share - AnyDesk Rust Client (Press F5 to Send File)", w, h, opts)
                    .unwrap_or_else(|e| panic!("{}", e));
                created_win.set_target_fps(60);
                created_win
            });

            if !win.is_open() || win.is_key_down(Key::Escape) {
                break;
            }

            let _ = win.update_with_buffer(&frame.buffer, w, h);

            // Keyboard Events
            for key in win.get_keys_pressed(minifb::KeyRepeat::No) {
                match key {
                    Key::F1 => {
                        let _ = out_tx.send(vec![7u8, 0, 0, 0, 0, 0, 0, 0, 0]);
                    }
                    Key::F2 => {
                        let _ = out_tx.send(vec![7u8, 1, 0, 0, 0, 0, 0, 0, 0]);
                    }
                    Key::F3 => {
                        let _ = out_tx.send(vec![7u8, 2, 0, 0, 0, 0, 0, 0, 0]);
                    }
                    Key::F5 => {
                        print!("\nEnter file path to send: ");
                        let _ = io::stdout().flush();
                        let mut file_path = String::new();
                        if io::stdin().read_line(&mut file_path).is_ok() {
                            let clean_path = file_path.trim().trim_matches('"').to_string();
                            if !clean_path.is_empty() {
                                send_file_async(clean_path, out_tx_file.clone());
                            }
                        }
                    }
                    _ => {
                        let mut p = vec![5u8];
                        let key_code = key as u32;
                        p.extend_from_slice(&key_code.to_be_bytes());
                        p.extend_from_slice(&[0u8; 4]);
                        let _ = out_tx.send(p);
                    }
                }
            }

            for key in win.get_keys_released() {
                if key != Key::F5 && key != Key::F1 && key != Key::F2 && key != Key::F3 {
                    let mut pkt = vec![6u8];
                    let key_code = key as u32;
                    pkt.extend_from_slice(&key_code.to_be_bytes());
                    pkt.extend_from_slice(&[0u8; 4]);
                    let _ = out_tx.send(pkt);
                }
            }

            // Scroll Wheel
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

            // Absolute Hardware Mouse Coordinate Mapping
            if win.is_active() {
                if let Some((mx, my)) = win.get_mouse_pos(MouseMode::Discard) {
                    if mx >= 4.0 && my >= 4.0 && mx <= (w as f32 - 4.0) && my <= (h as f32 - 4.0) {
                        let norm_x_f32 = ((mx - 4.0) / (w as f32 - 8.0).max(1.0)).clamp(0.0, 1.0);
                        let norm_y_f32 = ((my - 4.0) / (h as f32 - 8.0).max(1.0)).clamp(0.0, 1.0);

                        let norm_x = (norm_x_f32 * 65535.0) as u16;
                        let norm_y = (norm_y_f32 * 65535.0) as u16;

                        let should_move = match (last_norm_x, last_norm_y) {
                            (Some(lx), Some(ly)) => {
                                (norm_x as i32 - lx as i32).abs() >= 15 || (norm_y as i32 - ly as i32).abs() >= 15
                            }
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

                        // Left Click
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

                        // Right Click
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
            thread::sleep(Duration::from_millis(10));
        }
    }

    is_connected.store(false, Ordering::SeqCst);
    let _ = clip_thread.join();
}