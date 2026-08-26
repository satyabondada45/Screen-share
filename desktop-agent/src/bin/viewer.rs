use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use sha2::{Digest, Sha256};

use std::collections::VecDeque;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/* ============================================================
   LIMITS
   ============================================================ */

const MAX_AUDIO_PACKET: usize = 1024 * 1024;
const MAX_CLIPBOARD_SIZE: usize = 16 * 1024 * 1024;
const MAX_CHAT_SIZE: usize = u16::MAX as usize;
const MAX_FILE_NAME_SIZE: usize = 65_535;

const MAX_FRAME_PIXELS: usize = 100_000_000;

const AUDIO_QUEUE_LIMIT: usize = 96_000;

const NETWORK_READ_TIMEOUT: Duration = Duration::from_millis(250);

/* ============================================================
   DPI
   ============================================================ */

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

/* ============================================================
   TIME
   ============================================================ */

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/* ============================================================
   HASH
   ============================================================ */

fn compute_sha256(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

/* ============================================================
   BYTE READERS
   ============================================================ */

fn read_u16_be(buf: &[u8]) -> Option<u16> {
    if buf.len() < 2 {
        return None;
    }
    Some(u16::from_be_bytes([buf[0], buf[1]]))
}

fn read_u32_be(buf: &[u8]) -> Option<u32> {
    if buf.len() < 4 {
        return None;
    }
    Some(u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]))
}

fn read_u64_be(buf: &[u8]) -> Option<u64> {
    if buf.len() < 8 {
        return None;
    }
    Some(u64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ]))
}

/* ============================================================
   INTERRUPTIBLE TCP READ
   ============================================================ */

fn read_exact_interruptible(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    connected: &AtomicBool,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        if !connected.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "connection closed"));
        }
        match stream.read(&mut buffer[offset..]) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed")),
            Ok(n) => offset += n,
            Err(ref e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/* ============================================================
   OUTBOUND PACKET
   ============================================================ */

fn send_packet(tx: &SyncSender<Vec<u8>>, packet: Vec<u8>) -> bool {
    tx.send(packet).is_ok()
}

/* ============================================================
   FRAME
   ============================================================ */

#[derive(Clone)]
struct RenderFrame {
    width: usize,
    height: usize,
    buffer: Vec<u32>,
}

/* ============================================================
   H.264 DECODER WRAPPER
   ============================================================ */

fn decode_h264_to_rgb32(h264_data: &[u8], decoder: &mut Decoder) -> Option<RenderFrame> {
    // Decode to YUV
    let yuv = match decoder.decode(h264_data) {
        Ok(Some(f)) => f,
        Ok(None) => return None,
        Err(e) => {
            eprintln!("[H264] Decode error: {:?}", e);
            return None;
        }
    };

    let (width, height) = yuv.dimensions();
    if width == 0 || height == 0 || width * height > MAX_FRAME_PIXELS {
        eprintln!("[H264] Invalid dimensions: {}x{}", width, height);
        return None;
    }

    let y_plane = yuv.y();
    let u_plane = yuv.u();
    let v_plane = yuv.v();

    let mut buffer = Vec::with_capacity(width * height);

    // YUV420 to RGB32 (standard conversion)
    for j in 0..height {
        for i in 0..width {
            let y_idx = j * width + i;
            let uv_idx = (j / 2) * (width / 2) + (i / 2);
            let y = y_plane[y_idx] as f32;
            let u = u_plane[uv_idx] as f32 - 128.0;
            let v = v_plane[uv_idx] as f32 - 128.0;

            let r = (y + 1.402 * v).clamp(0.0, 255.0) as u8;
            let g = (y - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8;
            let b = (y + 1.772 * u).clamp(0.0, 255.0) as u8;

            buffer.push(((r as u32) << 16) | ((g as u32) << 8) | (b as u32));
        }
    }

    Some(RenderFrame { width, height, buffer })
}

/* ============================================================
   HUD
   ============================================================ */

fn draw_char(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    c: char,
    color: u32,
) {
    let bitmap: [u8; 7] = match c.to_ascii_uppercase() {
        '0' => [31, 17, 17, 17, 17, 17, 31],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [31, 1, 1, 31, 16, 16, 31],
        '3' => [31, 1, 1, 31, 1, 1, 31],
        '4' => [17, 17, 17, 31, 1, 1, 1],
        '5' => [31, 16, 16, 31, 1, 1, 31],
        '6' => [31, 16, 16, 31, 17, 17, 31],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [31, 17, 17, 31, 17, 17, 31],
        '9' => [31, 17, 17, 31, 1, 1, 31],
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [15, 16, 16, 16, 16, 16, 15],
        'D' => [28, 18, 17, 17, 17, 18, 28],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [15, 16, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [1, 1, 1, 1, 17, 17, 14],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 17, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 27, 17],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        ':' => [0, 4, 0, 0, 4, 0, 0],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [0, 0, 0, 0, 0, 0, 0],
    };

    for row in 0..7 {
        for col in 0..5 {
            if ((bitmap[row] >> (4 - col)) & 1) != 0 {
                let px = x + col;
                let py = y + row;
                if px < width && py < height {
                    buffer[py * width + px] = color;
                }
            }
        }
    }
}

fn draw_text(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    text: &str,
    color: u32,
) {
    let mut cursor = x;
    for c in text.chars() {
        draw_char(buffer, width, height, cursor, y, c, color);
        cursor += 8;
    }
}

fn render_status(buffer: &mut [u32], width: usize, height: usize, fps: u32, rtt: u64) {
    if width < 250 || height < 40 {
        return;
    }
    let box_width = 210usize;
    let box_height = 28usize;
    let x = width.saturating_sub(box_width + 12);
    let y = 12usize;
    for py in y..y + box_height {
        if py >= height {
            break;
        }
        for px in x..x + box_width {
            if px < width {
                buffer[py * width + px] = 0x101827;
            }
        }
    }
    let text = format!("FPS:{} RTT:{}MS", fps, rtt);
    draw_text(buffer, width, height, x + 10, y + 9, &text, 0xFFFFFF);
}

/* ============================================================
   AUDIO
   ============================================================ */

fn setup_audio_playback() -> (Option<cpal::Stream>, Arc<Mutex<VecDeque<f32>>>) {
    let queue = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(AUDIO_QUEUE_LIMIT)));
    let queue_clone = Arc::clone(&queue);
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => {
            eprintln!("[Audio] No output device.");
            return (None, queue);
        }
    };
    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[Audio] Config error: {:?}", e);
            return (None, queue);
        }
    };
    let stream_config: cpal::StreamConfig = config.clone().into();
    let output_channels = stream_config.channels as usize;
    let stream = match device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            if let Ok(mut q) = queue_clone.lock() {
                for frame in data.chunks_mut(output_channels.max(1)) {
                    let sample = q.pop_front().unwrap_or(0.0);
                    for output in frame.iter_mut() {
                        *output = sample;
                    }
                }
            } else {
                for sample in data.iter_mut() {
                    *sample = 0.0;
                }
            }
        },
        move |err| {
            eprintln!("[Audio] Stream error: {:?}", err);
        },
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Audio] Stream creation failed: {:?}", e);
            return (None, queue);
        }
    };
    if let Err(e) = stream.play() {
        eprintln!("[Audio] Play error: {:?}", e);
        return (None, queue);
    }
    println!(
        "[Audio] Output initialized: {} channels @ {} Hz",
        stream_config.channels, stream_config.sample_rate.0
    );
    (Some(stream), queue)
}

/* ============================================================
   KEYBOARD
   ============================================================ */

fn key_to_agent_code(key: Key) -> Option<u32> {
    match key {
        Key::A => Some(65),
        Key::B => Some(66),
        Key::C => Some(67),
        Key::D => Some(68),
        Key::E => Some(69),
        Key::F => Some(70),
        Key::G => Some(71),
        Key::H => Some(72),
        Key::I => Some(73),
        Key::J => Some(74),
        Key::K => Some(75),
        Key::L => Some(76),
        Key::M => Some(77),
        Key::N => Some(78),
        Key::O => Some(79),
        Key::P => Some(80),
        Key::Q => Some(81),
        Key::R => Some(82),
        Key::S => Some(83),
        Key::T => Some(84),
        Key::U => Some(85),
        Key::V => Some(86),
        Key::W => Some(87),
        Key::X => Some(88),
        Key::Y => Some(89),
        Key::Z => Some(90),
        Key::Key0 => Some(48),
        Key::Key1 => Some(49),
        Key::Key2 => Some(50),
        Key::Key3 => Some(51),
        Key::Key4 => Some(52),
        Key::Key5 => Some(53),
        Key::Key6 => Some(54),
        Key::Key7 => Some(55),
        Key::Key8 => Some(56),
        Key::Key9 => Some(57),
        Key::Space => Some(32),
        Key::Enter => Some(13),
        Key::Backspace => Some(8),
        Key::Tab => Some(9),
        Key::LeftShift | Key::RightShift => Some(16),
        Key::LeftCtrl | Key::RightCtrl => Some(17),
        Key::LeftAlt | Key::RightAlt => Some(18),
        Key::Left => Some(37),
        Key::Up => Some(38),
        Key::Right => Some(39),
        Key::Down => Some(40),
        Key::Delete => Some(46),
        Key::Escape => Some(27),
        _ => None,
    }
}

/* ============================================================
   GENERIC INPUT PACKET
   ============================================================ */

fn make_input_packet(packet_type: u8, a: u16, b: u16) -> Vec<u8> {
    let mut packet = Vec::with_capacity(9);
    packet.push(packet_type);
    packet.extend_from_slice(&a.to_be_bytes());
    packet.extend_from_slice(&b.to_be_bytes());
    packet.extend_from_slice(&[0u8; 4]);
    packet
}

fn make_key_packet(packet_type: u8, code: u32) -> Vec<u8> {
    let mut packet = Vec::with_capacity(9);
    packet.push(packet_type);
    packet.extend_from_slice(&code.to_be_bytes());
    packet.extend_from_slice(&[0u8; 4]);
    packet
}

/* ============================================================
   FILE TRANSFER
   ============================================================ */

fn send_file_async(path_string: String, tx: SyncSender<Vec<u8>>) {
    thread::spawn(move || {
        let path = Path::new(&path_string);
        if !path.exists() || !path.is_file() {
            eprintln!("[File] Invalid path: {}", path_string);
            return;
        }
        let filename = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => return,
        };
        if filename.as_bytes().len() > MAX_FILE_NAME_SIZE {
            eprintln!("[File] Filename too large.");
            return;
        }
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[File] Open failed: {:?}", e);
                return;
            }
        };
        let size = match file.metadata() {
            Ok(m) => m.len(),
            Err(e) => {
                eprintln!("[File] Metadata failed: {:?}", e);
                return;
            }
        };
        let name = filename.as_bytes();
        let mut meta = Vec::with_capacity(11 + name.len());
        meta.push(20);
        meta.extend_from_slice(&(name.len() as u16).to_be_bytes());
        meta.extend_from_slice(&size.to_be_bytes());
        meta.extend_from_slice(name);
        if !send_packet(&tx, meta) {
            return;
        }
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("[File] Read error: {:?}", e);
                    return;
                }
            };
            let mut packet = Vec::with_capacity(5 + n);
            packet.push(21);
            packet.extend_from_slice(&(n as u32).to_be_bytes());
            packet.extend_from_slice(&buffer[..n]);
            if !send_packet(&tx, packet) {
                return;
            }
        }
        println!("[File] Transfer complete.");
    });
}

/* ============================================================
   MAIN
   ============================================================ */

fn main() {
    set_process_dpi_aware();

    println!("========================================");
    println!("       REMOTE DESKTOP VIEWER");
    println!("========================================");

    let args: Vec<String> = env::args().collect();
    let relay_addr = env::var("RELAY_ADDR").unwrap_or_else(|_| "192.168.29.229:9001".to_string());

    /* ========================================================
       TARGET ID
       ======================================================== */

    let target_id = if args.len() > 1 {
        args[1].trim().replace("screenshare://", "").replace('-', "").replace('/', "")
    } else {
        print!("Enter Remote Desktop ID: ");
        let _ = io::stdout().flush();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return;
        }
        input.trim().replace('-', "")
    };

    if target_id.len() != 6 || !target_id.chars().all(|c| c.is_ascii_digit()) {
        eprintln!("[Viewer] Invalid ID: {}", target_id);
        return;
    }

    println!("[Viewer] Relay: {}", relay_addr);
    println!("[Viewer] Target: {}", target_id);

    /* ========================================================
       CONNECT
       ======================================================== */

    let mut stream = match TcpStream::connect(&relay_addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Viewer] Connection failed: {:?}", e);
            return;
        }
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));

    let pin = env::var("CONNECT_PIN").unwrap_or_default();
    let auth_hash = compute_sha256(&pin);

    let mut connect_packet = Vec::with_capacity(39);
    connect_packet.push(2);
    connect_packet.extend_from_slice(target_id.as_bytes());
    connect_packet.extend_from_slice(&auth_hash);

    println!("[Handshake] Connecting to relay: {}", relay_addr);
    println!("[Handshake] Target ID: {}", target_id);
    println!("[Handshake] Sending connection request...");

    if let Err(e) = stream.write_all(&connect_packet) {
        eprintln!("[Viewer] Handshake failed: {:?}", e);
        return;
    }

    println!("[Handshake] Waiting for host approval...");

    let mut ack = [0u8; 1];
    if let Err(e) = stream.read_exact(&mut ack) {
        eprintln!("[Viewer] Relay closed connection: {:?}", e);
        return;
    }

    println!("[Handshake] ACK received: {}", ack[0]);
    match ack[0] {
        1 => println!("[Viewer] CONNECTION APPROVED"),
        2 => {
            eprintln!("[Viewer] CONNECTION REJECTED");
            return;
        }
        _ => {
            eprintln!("[Viewer] HOST NOT FOUND");
            return;
        }
    }

    let _ = stream.set_read_timeout(Some(NETWORK_READ_TIMEOUT));

    /* ========================================================
       CONNECTION STATE
       ======================================================== */

    let connected = Arc::new(AtomicBool::new(true));
    let connected_read = Arc::clone(&connected);
    let connected_write = Arc::clone(&connected);
    let connected_clip = Arc::clone(&connected);

    let rtt = Arc::new(AtomicU64::new(0));
    let rtt_read = Arc::clone(&rtt);

    /* ========================================================
       CHAT
       ======================================================== */

    let chat = Arc::new(Mutex::new(Vec::<String>::new()));
    let chat_read = Arc::clone(&chat);

    /* ========================================================
       CLIPBOARD
       ======================================================== */

    let clipboard_state = Arc::new(Mutex::new(String::new()));
    let clipboard_state_in = Arc::clone(&clipboard_state);
    let clipboard_state_out = Arc::clone(&clipboard_state);

    /* ========================================================
       STREAMS
       ======================================================== */

    let mut read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Viewer] Stream clone failed: {:?}", e);
            return;
        }
    };
    let _ = read_stream.set_read_timeout(Some(NETWORK_READ_TIMEOUT));
    let mut write_stream = stream;

    /* ========================================================
       AUDIO
       ======================================================== */

    let (_audio_stream, audio_queue) = setup_audio_playback();

    /* ========================================================
       SHARED FRAME
       ======================================================== */

    let shared_frame: Arc<Mutex<Option<RenderFrame>>> = Arc::new(Mutex::new(None));
    let inbound_shared_frame = Arc::clone(&shared_frame);

    /* ========================================================
       OUTBOUND CHANNEL
       ======================================================== */

    let (out_tx, out_rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) = sync_channel(2048);
    let out_input = out_tx.clone();
    let out_clip = out_tx.clone();
    let out_chat = out_tx.clone();
    let out_file = out_tx.clone();
    let out_pong = out_tx.clone();

    /* ========================================================
       OUTBOUND THREAD
       ======================================================== */

    let outbound_thread = thread::spawn(move || {
        while connected_write.load(Ordering::SeqCst) {
            match out_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(packet) => {
                    if let Err(e) = write_stream.write_all(&packet) {
                        eprintln!("[Network] Send failed: {:?}", e);
                        connected_write.store(false, Ordering::SeqCst);
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = write_stream.shutdown(Shutdown::Both);
    });

    /* ========================================================
       INBOUND THREAD
       ======================================================== */

    let inbound_thread = thread::spawn(move || {
        let mut remote_clipboard = Clipboard::new().ok();
        let mut h264_decoder = Decoder::new().expect("Failed to create H.264 decoder");
        let mut frame_number: u64 = 0;

        while connected_read.load(Ordering::SeqCst) {
            let mut packet_type = [0u8; 1];
            if let Err(e) = read_exact_interruptible(&mut read_stream, &mut packet_type, &connected_read) {
                if connected_read.load(Ordering::SeqCst) {
                    eprintln!("[Network] Receive failed: {:?}", e);
                }
                break;
            }

            match packet_type[0] {
                /* ==================================================
                   H.264 VIDEO (TYPE 15)
                   ================================================== */
                15 => {
                    let mut header = [0u8; 12];
                    if read_exact_interruptible(&mut read_stream, &mut header, &connected_read).is_err() {
                        break;
                    }

                    let width = match read_u32_be(&header[0..4]) {
                        Some(v) => v as usize,
                        None => continue,
                    };
                    let height = match read_u32_be(&header[4..8]) {
                        Some(v) => v as usize,
                        None => continue,
                    };
                    let data_size = match read_u32_be(&header[8..12]) {
                        Some(v) => v as usize,
                        None => continue,
                    };

                    frame_number += 1;
                    if frame_number <= 3 || frame_number % 120 == 0 {
                        println!("[Frame] #{} {}x{} H264={} bytes", frame_number, width, height, data_size);
                    }

                    if width == 0 || height == 0 || data_size == 0 || data_size > 50 * 1024 * 1024 {
                        eprintln!("[Frame] Invalid H.264 header.");
                        continue;
                    }

                    let mut h264_data = vec![0u8; data_size];
                    if read_exact_interruptible(&mut read_stream, &mut h264_data, &connected_read).is_err() {
                        break;
                    }

                    // Decode H.264 to RGB
                    if let Some(frame) = decode_h264_to_rgb32(&h264_data, &mut h264_decoder) {
                        if let Ok(mut guard) = inbound_shared_frame.lock() {
                            *guard = Some(frame);
                        }
                    } else {
                        eprintln!("[Viewer] H.264 decode failed for frame #{}", frame_number);
                    }
                }

                /* ==================================================
                   CLIPBOARD
                   ================================================== */
                12 => {
                    let mut len_buf = [0u8; 4];
                    if read_exact_interruptible(&mut read_stream, &mut len_buf, &connected_read).is_err() {
                        break;
                    }
                    let len = match read_u32_be(&len_buf) {
                        Some(v) => v as usize,
                        None => continue,
                    };
                    if len > MAX_CLIPBOARD_SIZE {
                        eprintln!("[Clipboard] Too large.");
                        continue;
                    }
                    let mut data = vec![0u8; len];
                    if read_exact_interruptible(&mut read_stream, &mut data, &connected_read).is_err() {
                        break;
                    }
                    if let Ok(text) = String::from_utf8(data) {
                        if let Ok(mut state) = clipboard_state_in.lock() {
                            *state = text.clone();
                        }
                        if let Some(ref mut cb) = remote_clipboard {
                            let _ = cb.set_text(text);
                        }
                    }
                }

                /* ==================================================
                   AUDIO (type 17)
                   ================================================== */
                17 => {
                    let mut header = [0u8; 10];
                    if read_exact_interruptible(&mut read_stream, &mut header, &connected_read).is_err() {
                        break;
                    }
                    let byte_len = match read_u32_be(&header[0..4]) {
                        Some(v) => v as usize,
                        None => continue,
                    };
                    let sample_rate = match read_u32_be(&header[4..8]) {
                        Some(v) => v,
                        None => continue,
                    };
                    let channels = match read_u16_be(&header[8..10]) {
                        Some(v) => v as usize,
                        None => continue,
                    };
                    if byte_len == 0 || byte_len > MAX_AUDIO_PACKET || byte_len % 4 != 0 || sample_rate == 0 || channels == 0 {
                        eprintln!("[Audio] Invalid packet.");
                        continue;
                    }
                    let mut data = vec![0u8; byte_len];
                    if read_exact_interruptible(&mut read_stream, &mut data, &connected_read).is_err() {
                        break;
                    }
                    if let Ok(mut queue) = audio_queue.lock() {
                        for chunk in data.chunks_exact(4) {
                            let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                            if sample.is_finite() {
                                queue.push_back(sample);
                            }
                        }
                        while queue.len() > AUDIO_QUEUE_LIMIT {
                            queue.pop_front();
                        }
                    }
                }

                /* ==================================================
                   PING (type 14) – we echo back with same type
                   ================================================== */
                14 => {
                    let mut timestamp = [0u8; 8];
                    if read_exact_interruptible(&mut read_stream, &mut timestamp, &connected_read).is_err() {
                        break;
                    }
                    let sent = match read_u64_be(&timestamp) {
                        Some(v) => v,
                        None => continue,
                    };
                    let now = current_time_millis();
                    if now >= sent {
                        rtt_read.store(now - sent, Ordering::SeqCst);
                    }
                    // Echo back as type 14 (not 15)
                    let mut pong = Vec::with_capacity(9);
                    pong.push(14);
                    pong.extend_from_slice(&timestamp);
                    if out_pong.send(pong).is_err() {
                        break;
                    }
                }

                /* ==================================================
                   CHAT (type 16)
                   ================================================== */
                16 => {
                    let mut meta = [0u8; 3];
                    if read_exact_interruptible(&mut read_stream, &mut meta, &connected_read).is_err() {
                        break;
                    }
                    let sender = meta[0];
                    let len = match read_u16_be(&meta[1..3]) {
                        Some(v) => v as usize,
                        None => continue,
                    };
                    if len > MAX_CHAT_SIZE {
                        continue;
                    }
                    let mut data = vec![0u8; len];
                    if read_exact_interruptible(&mut read_stream, &mut data, &connected_read).is_err() {
                        break;
                    }
                    if let Ok(text) = String::from_utf8(data) {
                        let tag = if sender == 0 { "HOST" } else { "YOU" };
                        if let Ok(mut messages) = chat_read.lock() {
                            messages.push(format!("{}: {}", tag, text));
                            if messages.len() > 100 {
                                let remove = messages.len() - 100;
                                messages.drain(0..remove);
                            }
                        }
                        println!("[Chat] {}: {}", tag, text);
                    }
                }

                other => {
                    eprintln!("[Network] Unknown packet type: {}", other);
                }
            }
        }

        connected_read.store(false, Ordering::SeqCst);
    });

    /* ========================================================
       CLIPBOARD OUTBOUND
       ======================================================== */

    let clipboard_thread = thread::spawn(move || {
        let mut clipboard = Clipboard::new().ok();
        while connected_clip.load(Ordering::SeqCst) {
            if let Some(ref mut cb) = clipboard {
                if let Ok(text) = cb.get_text() {
                    if text.len() <= MAX_CLIPBOARD_SIZE {
                        let mut should_send = false;
                        if let Ok(mut last) = clipboard_state_out.lock() {
                            if *last != text {
                                *last = text.clone();
                                should_send = true;
                            }
                        }
                        if should_send {
                            let bytes = text.as_bytes();
                            let mut packet = Vec::with_capacity(5 + bytes.len());
                            packet.push(12);
                            packet.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                            packet.extend_from_slice(bytes);
                            if out_clip.send(packet).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(300));
        }
    });

    /* ========================================================
       WINDOW
       ======================================================== */

    let mut window: Option<Window> = None;
    let mut last_window_width = 0usize;
    let mut last_window_height = 0usize;
    let mut previous_left = false;
    let mut previous_right = false;
    let mut last_mouse_x: Option<u16> = None;
    let mut last_mouse_y: Option<u16> = None;
    let mut fps_counter = 0u32;
    let mut displayed_fps = 0u32;
    let mut fps_timer = Instant::now();

    /* ========================================================
       RENDER LOOP
       ======================================================== */

    while connected.load(Ordering::SeqCst) {
        let frame = match shared_frame.lock() {
            Ok(mut guard) => match guard.take() {
                Some(frame) => frame,
                None => {
                    drop(guard);
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
            },
            Err(_) => {
                eprintln!("[Viewer] Frame mutex poisoned.");
                connected.store(false, Ordering::SeqCst);
                break;
            }
        };

        let width = frame.width;
        let height = frame.height;
        if width == 0 || height == 0 {
            continue;
        }

        /* ====================================================
           WINDOW CREATION / RESOLUTION CHANGE
           ==================================================== */

        if window.is_none() || last_window_width != width || last_window_height != height {
            println!("[Viewer] Creating display for {}x{}", width, height);
            window = None;
            let window_width = width.min(1600);
            let window_height = height.min(900);
            let mut options = WindowOptions::default();
            options.resize = true;
            options.scale_mode = ScaleMode::AspectRatioStretch;
            let mut created = match Window::new("Remote Desktop Viewer", window_width, window_height, options) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("[Viewer] Window creation failed: {:?}", e);
                    connected.store(false, Ordering::SeqCst);
                    break;
                }
            };
            created.limit_update_rate(Some(Duration::from_micros(8_333))); // 120 FPS
            window = Some(created);
            last_window_width = width;
            last_window_height = height;
        }

        let win = match window.as_mut() {
            Some(w) => w,
            None => continue,
        };

        if !win.is_open() {
            connected.store(false, Ordering::SeqCst);
            break;
        }

        if win.is_key_down(Key::Escape) {
            connected.store(false, Ordering::SeqCst);
            break;
        }

        /* ====================================================
           DISPLAY
           ==================================================== */

        let mut display = frame.buffer.clone();
        fps_counter += 1;
        if fps_timer.elapsed() >= Duration::from_secs(1) {
            displayed_fps = fps_counter;
            fps_counter = 0;
            fps_timer = Instant::now();
        }
        render_status(
            &mut display,
            width,
            height,
            displayed_fps,
            rtt.load(Ordering::SeqCst),
        );

        if let Err(e) = win.update_with_buffer(&display, width, height) {
            eprintln!("[Viewer] Display error: {:?}", e);
            connected.store(false, Ordering::SeqCst);
            break;
        }

        /* ====================================================
           KEYBOARD
           ==================================================== */

        for key in win.get_keys_pressed(KeyRepeat::No) {
            match key {
                Key::F1 => {
                    let packet = make_key_packet(7, 0);
                    let _ = out_input.send(packet);
                }
                Key::F2 => {
                    let packet = make_key_packet(7, 1);
                    let _ = out_input.send(packet);
                }
                Key::F3 => {
                    let packet = make_key_packet(7, 2);
                    let _ = out_input.send(packet);
                }
                Key::F5 => {
                    print!("\nFile path: ");
                    let _ = io::stdout().flush();
                    let mut path = String::new();
                    if io::stdin().read_line(&mut path).is_ok() {
                        let path = path.trim().trim_matches('"').to_string();
                        if !path.is_empty() {
                            send_file_async(path, out_file.clone());
                        }
                    }
                }
                Key::F6 => {
                    print!("\nMessage: ");
                    let _ = io::stdout().flush();
                    let mut message = String::new();
                    if io::stdin().read_line(&mut message).is_ok() {
                        let message = message.trim().to_string();
                        let bytes = message.as_bytes();
                        if !message.is_empty() && bytes.len() <= MAX_CHAT_SIZE {
                            let mut packet = Vec::with_capacity(4 + bytes.len());
                            packet.push(16);
                            packet.push(1);
                            packet.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
                            packet.extend_from_slice(bytes);
                            if out_chat.send(packet).is_ok() {
                                if let Ok(mut messages) = chat.lock() {
                                    messages.push(format!("YOU: {}", message));
                                    if messages.len() > 100 {
                                        let remove = messages.len() - 100;
                                        messages.drain(0..remove);
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {
                    let code = match key_to_agent_code(key) {
                        Some(v) => v,
                        None => continue,
                    };
                    let packet = make_key_packet(5, code);
                    let _ = out_input.send(packet);
                }
            }
        }

        for key in win.get_keys_released() {
            let code = match key_to_agent_code(key) {
                Some(v) => v,
                None => continue,
            };
            let packet = make_key_packet(6, code);
            let _ = out_input.send(packet);
        }

        /* ====================================================
           MOUSE
           ==================================================== */

        if win.is_active() {
            if let Some((mx, my)) = win.get_mouse_pos(MouseMode::Clamp) {
                let (window_width, window_height) = win.get_size();
                let ww = window_width.max(1) as f32;
                let wh = window_height.max(1) as f32;
                let normalized_x = ((mx / ww).clamp(0.0, 1.0) * 65535.0).round() as u16;
                let normalized_y = ((my / wh).clamp(0.0, 1.0) * 65535.0).round() as u16;

                let moved = match (last_mouse_x, last_mouse_y) {
                    (Some(x), Some(y)) => (normalized_x as i32 - x as i32).abs() >= 2
                        || (normalized_y as i32 - y as i32).abs() >= 2,
                    _ => true,
                };

                if moved {
                    let packet = make_input_packet(0, normalized_x, normalized_y);
                    if out_input.send(packet).is_err() {
                        connected.store(false, Ordering::SeqCst);
                        break;
                    }
                    last_mouse_x = Some(normalized_x);
                    last_mouse_y = Some(normalized_y);
                }

                let left = win.get_mouse_down(MouseButton::Left);
                if left && !previous_left {
                    let packet = make_input_packet(1, normalized_x, normalized_y);
                    let _ = out_input.send(packet);
                }
                if !left && previous_left {
                    let packet = make_input_packet(2, normalized_x, normalized_y);
                    let _ = out_input.send(packet);
                }
                previous_left = left;

                let right = win.get_mouse_down(MouseButton::Right);
                if right && !previous_right {
                    let packet = make_input_packet(3, normalized_x, normalized_y);
                    let _ = out_input.send(packet);
                }
                if !right && previous_right {
                    let packet = make_input_packet(4, normalized_x, normalized_y);
                    let _ = out_input.send(packet);
                }
                previous_right = right;
            }

            if let Some((_, sy)) = win.get_scroll_wheel() {
                if sy.abs() > 0.01 {
                    let scroll = (sy * 120.0).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                    let mut packet = Vec::with_capacity(9);
                    packet.push(8);
                    packet.extend_from_slice(&0i16.to_be_bytes());
                    packet.extend_from_slice(&scroll.to_be_bytes());
                    packet.extend_from_slice(&[0u8; 4]);
                    let _ = out_input.send(packet);
                }
            }
        } else {
            if previous_left {
                let packet = make_input_packet(2, 0, 0);
                let _ = out_input.send(packet);
                previous_left = false;
            }
            if previous_right {
                let packet = make_input_packet(4, 0, 0);
                let _ = out_input.send(packet);
                previous_right = false;
            }
        }
    }

    /* ========================================================
       SHUTDOWN
       ======================================================== */

    println!("[Viewer] Closing...");
    connected.store(false, Ordering::SeqCst);
    drop(out_tx);
    thread::sleep(Duration::from_millis(100));
    let _ = clipboard_thread.join();
    let _ = inbound_thread.join();
    let _ = outbound_thread.join();
    println!("[Viewer] Disconnected.");
}