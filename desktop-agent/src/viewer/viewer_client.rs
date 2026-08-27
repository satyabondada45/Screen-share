use minifb::{Key, KeyRepeat, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const MAX_FRAME_PIXELS: usize = 100_000_000;
const NETWORK_READ_TIMEOUT: Duration = Duration::from_millis(250);

fn compute_sha256(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

fn read_exact_interruptible(stream: &mut TcpStream, buffer: &mut [u8], connected: &AtomicBool) -> io::Result<()> {
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

#[derive(Clone)]
struct RenderFrame {
    width: usize,
    height: usize,
    buffer: Vec<u32>,
}

fn decode_h264_to_rgb32(h264_data: &[u8], decoder: &mut Decoder) -> Option<RenderFrame> {
    let yuv = match decoder.decode(h264_data) {
        Ok(Some(f)) => f,
        Ok(None) => return None,
        Err(_) => return None,
    };

    let (width, height) = yuv.dimensions();
    if width == 0 || height == 0 { return None; }
    let pixel_count = width * height;
    if pixel_count > MAX_FRAME_PIXELS { return None; }

    let mut rgb_raw = vec![0u8; pixel_count * 3];
    yuv.write_rgb8(&mut rgb_raw);

    let mut buffer = Vec::with_capacity(pixel_count);
    for chunk in rgb_raw.chunks_exact(3) {
        let r = chunk[0] as u32;
        let g = chunk[1] as u32;
        let b = chunk[2] as u32;
        buffer.push((r << 16) | (g << 8) | b);
    }

    Some(RenderFrame { width, height, buffer })
}

fn make_input_packet(action: u8, x: u16, y: u16) -> Vec<u8> {
    let mut packet = Vec::with_capacity(5);
    packet.push(action);
    packet.extend_from_slice(&x.to_be_bytes());
    packet.extend_from_slice(&y.to_be_bytes());
    packet
}

fn make_key_packet(action: u8, code: u16) -> Vec<u8> {
    let mut packet = Vec::with_capacity(3);
    packet.push(action);
    packet.extend_from_slice(&code.to_be_bytes());
    packet
}

fn key_to_agent_code(key: Key) -> Option<u16> {
    match key {
        Key::A => Some(0x0041), Key::B => Some(0x0042), Key::C => Some(0x0043),
        Key::D => Some(0x0044), Key::E => Some(0x0045), Key::F => Some(0x0046),
        Key::G => Some(0x0047), Key::H => Some(0x0048), Key::I => Some(0x0049),
        Key::J => Some(0x004A), Key::K => Some(0x004B), Key::L => Some(0x004C),
        Key::M => Some(0x004D), Key::N => Some(0x004E), Key::O => Some(0x004F),
        Key::P => Some(0x0050), Key::Q => Some(0x0051), Key::R => Some(0x0052),
        Key::S => Some(0x0053), Key::T => Some(0x0054), Key::U => Some(0x0055),
        Key::V => Some(0x0056), Key::W => Some(0x0057), Key::X => Some(0x0058),
        Key::Y => Some(0x0059), Key::Z => Some(0x005A),
        Key::Key0 => Some(0x0030), Key::Key1 => Some(0x0031), Key::Key2 => Some(0x0032),
        Key::Key3 => Some(0x0033), Key::Key4 => Some(0x0034), Key::Key5 => Some(0x0035),
        Key::Key6 => Some(0x0036), Key::Key7 => Some(0x0037), Key::Key8 => Some(0x0038),
        Key::Key9 => Some(0x0039),
        Key::Enter => Some(0x000D), Key::Space => Some(0x0020), Key::Backspace => Some(0x0008),
        Key::Tab => Some(0x0009), Key::LeftShift | Key::RightShift => Some(0x0010),
        Key::LeftCtrl | Key::RightCtrl => Some(0x0011), Key::LeftAlt | Key::RightAlt => Some(0x0012),
        Key::Left => Some(0x0025), Key::Up => Some(0x0026), Key::Right => Some(0x0027), Key::Down => Some(0x0028),
        _ => None,
    }
}

pub fn start_remote_viewer(relay_addr: &str, target_system_id: &str, pin: &str) {
    let clean_id = target_system_id.replace(" ", "").trim().to_string();
    let relay = relay_addr.to_string();
    let pin_str = pin.to_string();

    thread::spawn(move || {
        println!("[Viewer] Connecting to relay: {} for Host ID: {}", relay, clean_id);

        let mut stream = match TcpStream::connect(&relay) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                let _ = s.set_read_timeout(Some(NETWORK_READ_TIMEOUT));
                s
            }
            Err(e) => {
                eprintln!("[Viewer] Connection to relay failed: {:?}", e);
                return;
            }
        };

        let auth_hash = compute_sha256(&pin_str);
        let id_bytes = clean_id.as_bytes();
        let mut handshake = Vec::with_capacity(1 + id_bytes.len() + 32);
        handshake.push(2u8);
        handshake.extend_from_slice(id_bytes);
        handshake.extend_from_slice(&auth_hash);

        if stream.write_all(&handshake).is_err() {
            eprintln!("[Viewer] Handshake send failed.");
            return;
        }

        let mut status = [0u8; 1];
        if stream.read_exact(&mut status).is_err() || status[0] != 2 {
            eprintln!("[Viewer] Connection rejected or host offline (status: {:?})", status);
            return;
        }

        println!("[Viewer] Connected and Authenticated successfully!");

        let connected = Arc::new(AtomicBool::new(true));
        let (out_tx, out_rx) = sync_channel::<Vec<u8>>(128);
        let shared_frame: Arc<Mutex<Option<RenderFrame>>> = Arc::new(Mutex::new(None));

        let mut write_stream = stream.try_clone().unwrap();
        let is_conn_out = Arc::clone(&connected);
        let outbound_thread = thread::spawn(move || {
            while is_conn_out.load(Ordering::SeqCst) {
                match out_rx.recv() {
                    Ok(packet) => {
                        if write_stream.write_all(&packet).is_err() {
                            is_conn_out.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut read_stream = stream.try_clone().unwrap();
        let is_conn_in = Arc::clone(&connected);
        let sf_clone = Arc::clone(&shared_frame);
        let inbound_thread = thread::spawn(move || {
            let mut decoder = match Decoder::new() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[Viewer] OpenH264 decoder init failed: {:?}", e);
                    is_conn_in.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let mut header = [0u8; 13];
            while is_conn_in.load(Ordering::SeqCst) {
                if read_exact_interruptible(&mut read_stream, &mut header, &is_conn_in).is_err() {
                    break;
                }

                let ptype = header[0];
                let payload_len = u32::from_be_bytes(header[9..13].try_into().unwrap()) as usize;

                if payload_len > 50 * 1024 * 1024 {
                    break;
                }

                let mut payload = vec![0u8; payload_len];
                if read_exact_interruptible(&mut read_stream, &mut payload, &is_conn_in).is_err() {
                    break;
                }

                if ptype == 13 || ptype == 15 {
                    if let Some(frame) = decode_h264_to_rgb32(&payload, &mut decoder) {
                        if let Ok(mut lock) = sf_clone.lock() {
                            *lock = Some(frame);
                        }
                    }
                }
            }
            is_conn_in.store(false, Ordering::SeqCst);
        });

        // Minifb Render Window for Remote Desktop
        let mut window: Option<Window> = None;
        let mut last_w = 0;
        let mut last_h = 0;
        let mut prev_left = false;
        let mut prev_right = false;
        let out_input = out_tx.clone();

        while connected.load(Ordering::SeqCst) {
            let frame = match shared_frame.lock() {
                Ok(mut g) => g.take(),
                Err(_) => break,
            };

            if let Some(f) = frame {
                let w = f.width;
                let h = f.height;
                if window.is_none() || last_w != w || last_h != h {
                    let mut opts = WindowOptions::default();
                    opts.resize = true;
                    opts.scale_mode = ScaleMode::AspectRatioStretch;
                    let win_title = format!("Remote Desktop — Session [{}]", clean_id);
                    let mut created = match Window::new(&win_title, w.min(1600), h.min(900), opts) {
                        Ok(win) => win,
                        Err(_) => break,
                    };
                    created.limit_update_rate(Some(Duration::from_micros(8_333))); // 120 FPS
                    window = Some(created);
                    last_w = w;
                    last_h = h;
                }

                if let Some(ref mut win) = window {
                    if !win.is_open() || win.is_key_down(Key::Escape) {
                        connected.store(false, Ordering::SeqCst);
                        break;
                    }

                    let _ = win.update_with_buffer(&f.buffer, w, h);

                    // Mouse Input Handling
                    if win.is_active() {
                        if let Some((mx, my)) = win.get_mouse_pos(MouseMode::Clamp) {
                            let (win_w, win_h) = win.get_size();
                            let nx = ((mx / win_w.max(1) as f32).clamp(0.0, 1.0) * 65535.0) as u16;
                            let ny = ((my / win_h.max(1) as f32).clamp(0.0, 1.0) * 65535.0) as u16;

                            let _ = out_input.send(make_input_packet(0, nx, ny));

                            let left = win.get_mouse_down(MouseButton::Left);
                            if left && !prev_left { let _ = out_input.send(make_input_packet(1, nx, ny)); }
                            if !left && prev_left { let _ = out_input.send(make_input_packet(2, nx, ny)); }
                            prev_left = left;

                            let right = win.get_mouse_down(MouseButton::Right);
                            if right && !prev_right { let _ = out_input.send(make_input_packet(3, nx, ny)); }
                            if !right && prev_right { let _ = out_input.send(make_input_packet(4, nx, ny)); }
                            prev_right = right;
                        }
                    }

                    // Keyboard Input Handling
                    for key in win.get_keys_pressed(KeyRepeat::No) {
                        if let Some(code) = key_to_agent_code(key) {
                            let _ = out_input.send(make_key_packet(5, code));
                        }
                    }
                    for key in win.get_keys_released() {
                        if let Some(code) = key_to_agent_code(key) {
                            let _ = out_input.send(make_key_packet(6, code));
                        }
                    }
                }
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }

        connected.store(false, Ordering::SeqCst);
        let _ = outbound_thread.join();
        let _ = inbound_thread.join();
        println!("[Viewer] Remote session ended.");
    });
}
