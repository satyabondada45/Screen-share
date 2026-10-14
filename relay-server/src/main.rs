use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::Url;
use sha2::{Digest, Sha256};
use tungstenite::Message;

enum ViewerSession {
    WebSocket(Arc<Mutex<tungstenite::WebSocket<TcpStream>>>),
    Tcp(TcpStream),
}

macro_rules! relay_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            let _ = writeln!(std::io::stdout(), "{}", msg);
            let log_dir = if cfg!(windows) {
                std::env::var("PROGRAMDATA")
                    .map(|p| format!("{}\\Screen Share\\logs", p))
                    .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string())
            } else {
                "/var/log/deskstream".to_string()
            };
            let _ = std::fs::create_dir_all(&log_dir);
            let log_path = if cfg!(windows) {
                format!("{}\\relay.log", log_dir)
            } else {
                format!("{}/relay.log", log_dir)
            };
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let _ = writeln!(file, "[{}] {}", now, msg);
            }
        }
    };
}

macro_rules! println {
    ($($arg:tt)*) => {
        relay_log!($($arg)*);
    };
}

macro_rules! eprintln {
    ($($arg:tt)*) => {
        relay_log!($($arg)*)
    };
}

fn trace_type99(
    direction: &str,
    device_id: &str,
    session_id: &str,
    source: &str,
    reason: &str,
) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    println!(
        "[TYPE99 TRACE] component=RELAY direction={} device_id={} session_id={} source={} reason={} timestamp_ms={}",
        direction, device_id, session_id, source, reason, timestamp_ms
    );
}

fn trace_packet(
    direction: &str,
    device_id: &str,
    source: &str,
    destination: &str,
    packet_type: u8,
    packet_len: usize,
) {
    if !matches!(packet_type, 1 | 2 | 3 | 13 | 14 | 32 | 99) {
        return;
    }
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let thread_id = format!("{:?}", thread::current().id());
    println!(
        "[TRACE99][RELAY][{}] timestamp={} device={} session={} source={} destination={} type={} length={} thread={}",
        direction, timestamp_ms, device_id, device_id, source, destination, packet_type, packet_len, thread_id
    );
}

struct ViewerSessionRequest {
    auth_hash: [u8; 32],
    response_tx: Sender<bool>,
    session: ViewerSession,
}

type ClientMap = Arc<Mutex<HashMap<String, (u64, Sender<ViewerSessionRequest>)>>>;

const RELAY_ADDR: &str = "0.0.0.0:9001";
const CHAT_TYPING_PACKET_TYPE: u8 = 18;
const REVERSE_REQUEST_PACKET_TYPE: u8 = 30;
const REVERSE_ACK_PACKET_TYPE: u8 = 31;
const DEVICE_ID_LENGTH: usize = 9;
const VIEWER_AUTH_HASH_LENGTH: usize = 32;
const MAX_BACKEND_RESPONSE_BYTES: u64 = 4096;

type PresenceQueue = (std::sync::mpsc::SyncSender<String>, Arc<Mutex<HashMap<String, ()>>>);
static BACKEND_PRESENCE_QUEUE: OnceLock<PresenceQueue> = OnceLock::new();

fn valid_device_id(device_id: &str) -> bool {
    device_id.len() == DEVICE_ID_LENGTH && device_id.bytes().all(|byte| byte.is_ascii_digit())
}

fn verify_viewer_pin(pin: &str, supplied_hash: &[u8; VIEWER_AUTH_HASH_LENGTH]) -> bool {
    if pin.is_empty() {
        return false;
    }
    let expected_hash = Sha256::digest(pin.as_bytes());
    expected_hash
        .iter()
        .zip(supplied_hash)
        .fold(0u8, |difference, (expected, supplied)| {
            difference | (expected ^ supplied)
        })
        == 0
}

fn read_viewer_handshake<R: Read>(reader: &mut R) -> std::io::Result<(String, [u8; VIEWER_AUTH_HASH_LENGTH])> {
    let mut id_bytes = [0u8; DEVICE_ID_LENGTH];
    reader.read_exact(&mut id_bytes)?;
    if !id_bytes.iter().all(u8::is_ascii_digit) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "viewer device ID must contain exactly nine ASCII digits",
        ));
    }

    let session_id = std::str::from_utf8(&id_bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
        .to_string();
    let mut auth_hash = [0u8; VIEWER_AUTH_HASH_LENGTH];
    reader.read_exact(&mut auth_hash)?;
    Ok((session_id, auth_hash))
}

fn parse_websocket_viewer_handshake(
    packet: &[u8],
) -> std::io::Result<(String, [u8; VIEWER_AUTH_HASH_LENGTH])> {
    let expected_len = 1 + DEVICE_ID_LENGTH + VIEWER_AUTH_HASH_LENGTH;
    if packet.len() != expected_len || packet[0] != 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "viewer packet must contain type, nine-digit device ID, and 32-byte authentication hash",
        ));
    }

    let id_bytes = &packet[1..1 + DEVICE_ID_LENGTH];
    if !id_bytes.iter().all(u8::is_ascii_digit) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "viewer device ID must contain exactly nine ASCII digits",
        ));
    }

    let session_id = std::str::from_utf8(id_bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
        .to_string();
    let mut auth_hash = [0u8; VIEWER_AUTH_HASH_LENGTH];
    auth_hash.copy_from_slice(&packet[1 + DEVICE_ID_LENGTH..]);
    Ok((session_id, auth_hash))
}

fn chat_typing_packet(state: u8) -> Option<Vec<u8>> {
    (state <= 1).then(|| vec![CHAT_TYPING_PACKET_TYPE, state])
}

fn reverse_ack_packet(state: u8) -> Option<Vec<u8>> {
    (state <= 1).then(|| vec![REVERSE_ACK_PACKET_TYPE, state])
}

fn reverse_request_packet() -> Vec<u8> {
    vec![REVERSE_REQUEST_PACKET_TYPE]
}

fn initial_packet_type(bytes_peeked: usize, peek: &[u8]) -> Option<u8> {
    if bytes_peeked == 0 {
        return None;
    }
    peek.first().copied()
}

fn is_http_get_prefix(bytes_peeked: usize, peek: &[u8]) -> bool {
    if bytes_peeked == 0 {
        return false;
    }
    let prefix_len = bytes_peeked.min(4);
    b"GET ".starts_with(&peek[..prefix_len])
}

fn is_health_check(stream: &TcpStream) -> std::io::Result<bool> {
    const HEALTH_PREFIX: &[u8] = b"GET /health";
    let mut peek = [0u8; HEALTH_PREFIX.len() + 1];

    for _ in 0..50 {
        let bytes_peeked = stream.peek(&mut peek)?;
        if bytes_peeked == 0 {
            return Ok(false);
        }
        let prefix_length = bytes_peeked.min(HEALTH_PREFIX.len());
        if peek[..prefix_length] != HEALTH_PREFIX[..prefix_length] {
            return Ok(false);
        }
        if bytes_peeked > HEALTH_PREFIX.len() {
            return Ok(matches!(peek[HEALTH_PREFIX.len()], b' ' | b'?' | b'/'));
        }
        thread::sleep(Duration::from_millis(2));
    }

    Ok(false)
}

// ============================================================
// AUTOSTART & BACKGROUND PERSISTENCE
// ============================================================

#[cfg(windows)]
fn enable_autostart(app_name: &str) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegSetValueExW, HKEY_CURRENT_USER, REG_SZ,
    };

    let target_path = env::current_exe().map_err(|e| e.to_string())?;

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
// SEND ALL
// ============================================================

fn send_all(stream: &mut TcpStream, data: &[u8]) -> bool {
    let mut offset = 0;

    while offset < data.len() {
        match stream.write(&data[offset..]) {
            Ok(0) => return false,
            Ok(n) => offset += n,
            Err(e) => {
                eprintln!("[Relay] Write error: {:?}", e);
                return false;
            }
        }
    }

    true
}

fn send_all_pair(target_name: &str, stream: &mut TcpStream, data: &[u8]) -> bool {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    println!("[PAIR] Writing to {} socket (peer={})", target_name, peer);
    let mut offset = 0;

    while offset < data.len() {
        match stream.write(&data[offset..]) {
            Ok(0) => {
                eprintln!("[PAIR][ERROR] Write failed: target={} peer={} error=ZeroBytesWritten", target_name, peer);
                return false;
            }
            Ok(n) => offset += n,
            Err(e) => {
                eprintln!("[PAIR][ERROR] Write failed: target={} peer={} error={:?}", target_name, peer, e);
                return false;
            }
        }
    }

    println!("[PAIR] Pairing message sent successfully to {} ({})", target_name, peer);
    true
}

// ============================================================
// READ EXACT WITH LOGGING
// ============================================================

fn read_exact_logged(stream: &mut TcpStream, buffer: &mut [u8], name: &str) -> bool {
    match stream.read_exact(buffer) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("[Relay] {} read failed: {:?}", name, e);
            false
        }
    }
}

// ============================================================
// TCP FORWARDING: HOST <-> VIEWER
// ============================================================

struct TcpBridgeState {
    active: AtomicBool,
    viewer_disconnected: AtomicBool,
    host_failed: AtomicBool,
}

struct TcpBridgeWorkerGuard(Arc<TcpBridgeState>);

impl Drop for TcpBridgeWorkerGuard {
    fn drop(&mut self) {
        self.0.active.store(false, Ordering::SeqCst);
    }
}

struct WebSocketBridgeWorkerGuard(Arc<AtomicBool>);

impl Drop for WebSocketBridgeWorkerGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn mark_viewer_disconnected(state: &TcpBridgeState, viewer: &TcpStream) {
    if state.active.swap(false, Ordering::SeqCst) {
        state.viewer_disconnected.store(true, Ordering::SeqCst);
    }
    let _ = viewer.shutdown(Shutdown::Both);
}

fn host_to_viewer_tcp(
    mut host: TcpStream,
    mut viewer: TcpStream,
    state: Arc<TcpBridgeState>,
) {
    println!("[Relay] HOST -> VIEWER forwarding started");
    let _ = host.set_nodelay(true);
    let _ = viewer.set_nodelay(true);
    let _ = host.set_read_timeout(Some(Duration::from_millis(200)));

    let mut buffer = [0u8; 128 * 1024];
    while state.active.load(Ordering::SeqCst) {
        match host.read(&mut buffer) {
            Ok(0) => {
                println!("[Relay] Host -> Viewer TCP connection closed.");
                state.host_failed.store(true, Ordering::SeqCst);
                state.active.store(false, Ordering::SeqCst);
                let _ = viewer.shutdown(Shutdown::Both);
                break;
            }
            Ok(n) => {
                if !send_all(&mut viewer, &buffer[..n]) {
                    mark_viewer_disconnected(&state, &viewer);
                    break;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!("[Relay] HOST -> VIEWER read error: {:?}", e);
                state.host_failed.store(true, Ordering::SeqCst);
                state.active.store(false, Ordering::SeqCst);
                let _ = viewer.shutdown(Shutdown::Both);
                break;
            }
        }
    }
}

fn viewer_to_host_tcp(
    mut viewer: TcpStream,
    mut host: TcpStream,
    state: Arc<TcpBridgeState>,
) {
    println!("[Relay] VIEWER -> HOST forwarding started");
    let _ = viewer.set_nodelay(true);
    let _ = host.set_nodelay(true);
    let _ = viewer.set_read_timeout(Some(Duration::from_millis(200)));

    let mut buffer = [0u8; 64 * 1024];
    while state.active.load(Ordering::SeqCst) {
        match viewer.read(&mut buffer) {
            Ok(0) => {
                println!("[Relay] Viewer -> Host TCP connection closed.");
                mark_viewer_disconnected(&state, &viewer);
                break;
            }
            Ok(n) => {
                if !send_all(&mut host, &buffer[..n]) {
                    state.host_failed.store(true, Ordering::SeqCst);
                    state.active.store(false, Ordering::SeqCst);
                    let _ = viewer.shutdown(Shutdown::Both);
                    break;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!("[Relay] VIEWER -> HOST read error: {:?}", e);
                mark_viewer_disconnected(&state, &viewer);
                break;
            }
        }
    }

    if state.viewer_disconnected.load(Ordering::SeqCst)
        && !state.host_failed.load(Ordering::SeqCst)
    {
        if let Err(error) = host.write_all(&[99u8]) {
            eprintln!("[Relay] Viewer disconnect notification to host failed: {:?}", error);
        }
    }
}

fn bridge_tcp_session(host: &TcpStream, viewer: TcpStream) -> bool {
    let host_reader = match host.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[Relay] Failed to clone host read socket: {:?}", error);
            let _ = viewer.shutdown(Shutdown::Both);
            return true;
        }
    };
    let mut host_writer = match host.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[Relay] Failed to clone host write socket: {:?}", error);
            let _ = viewer.shutdown(Shutdown::Both);
            return true;
        }
    };
    let viewer_reader = match viewer.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[Relay] Failed to clone viewer read socket: {:?}", error);
            let _ = host_writer.write_all(&[99u8]);
            let _ = viewer.shutdown(Shutdown::Both);
            return false;
        }
    };
    let viewer_writer = match viewer.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[Relay] Failed to clone viewer write socket: {:?}", error);
            let _ = host_writer.write_all(&[99u8]);
            let _ = viewer.shutdown(Shutdown::Both);
            return false;
        }
    };

    let state = Arc::new(TcpBridgeState {
        active: AtomicBool::new(true),
        viewer_disconnected: AtomicBool::new(false),
        host_failed: AtomicBool::new(false),
    });
    let host_to_viewer_state = Arc::clone(&state);
    let viewer_to_host_state = Arc::clone(&state);
    let host_to_viewer_guard_state = Arc::clone(&state);
    let viewer_to_host_guard_state = Arc::clone(&state);

    let host_to_viewer = thread::spawn(move || {
        let _active_guard = TcpBridgeWorkerGuard(host_to_viewer_guard_state);
        host_to_viewer_tcp(host_reader, viewer_writer, host_to_viewer_state);
    });
    let viewer_to_host = thread::spawn(move || {
        let _active_guard = TcpBridgeWorkerGuard(viewer_to_host_guard_state);
        viewer_to_host_tcp(viewer_reader, host_writer, viewer_to_host_state);
    });

    let host_to_viewer_panicked = host_to_viewer.join().is_err();
    let viewer_to_host_panicked = viewer_to_host.join().is_err();
    let host_failed = state.host_failed.load(Ordering::SeqCst)
        || host_to_viewer_panicked
        || viewer_to_host_panicked;
    if host_to_viewer_panicked || viewer_to_host_panicked {
        let _ = host.shutdown(Shutdown::Both);
    }
    host_failed
}

// ============================================================
// HOST SESSION RUNNER (WEBSOCKET BRIDGE)
// ============================================================

const CONTROL_BYTES_PER_TURN: usize = 256 * 1024;
const CONTROL_PACKETS_PER_TURN: usize = 64;

fn take_control_burst(receiver: &std::sync::mpsc::Receiver<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut bytes_sent = 0;
    while packets.len() < CONTROL_PACKETS_PER_TURN
        && (bytes_sent < CONTROL_BYTES_PER_TURN || packets.is_empty())
    {
        match receiver.try_recv() {
            Ok(packet) => {
                bytes_sent += packet.len();
                packets.push(packet);
            }
            Err(_) => break,
        }
    }
    packets
}

fn read_host_video_packet<R: Read>(reader: &mut R, packet_type: u8) -> std::io::Result<Vec<u8>> {
    let mut header = [0u8; 20];
    reader.read_exact(&mut header)?;
    let width = u32::from_be_bytes(header[0..4].try_into().unwrap());
    let height = u32::from_be_bytes(header[4..8].try_into().unwrap());
    let payload_size = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    if width == 0 || width > 7680 || height == 0 || height > 4320 || payload_size > 50 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid video dimensions or payload size: {width}x{height}, {payload_size} bytes"),
        ));
    }
    let mut packet = Vec::with_capacity(1 + header.len() + payload_size);
    packet.push(packet_type);
    packet.extend_from_slice(&header);
    let payload_start = packet.len();
    packet.resize(payload_start + payload_size, 0);
    reader.read_exact(&mut packet[payload_start..])?;
    Ok(packet)
}

fn read_host_chat_packet<R: Read>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let mut header = [0u8; 3];
    reader.read_exact(&mut header)?;
    let payload_size = u16::from_be_bytes([header[1], header[2]]) as usize;
    let mut packet = Vec::with_capacity(1 + header.len() + payload_size);
    packet.push(16);
    packet.extend_from_slice(&header);
    let payload_start = packet.len();
    packet.resize(payload_start + payload_size, 0);
    reader.read_exact(&mut packet[payload_start..])?;
    Ok(packet)
}

fn notify_host_viewer_disconnect(
    session_id: &str,
    host_writer: &mut TcpStream,
    host_ended_session: bool,
) {
    if host_ended_session {
        trace_type99(
            "SKIPPED",
            session_id,
            session_id,
            "bridge_cleanup",
            "host_ended_session_no_viewer_disconnect_echo",
        );
        return;
    }

    if let Err(error) = host_writer.write_all(&[99u8]) {
        trace_type99(
            "SEND_FAILED",
            session_id,
            session_id,
            "bridge_cleanup",
            &format!("viewer_disconnect_type_99_write_failed:{error}"),
        );
    } else {
        trace_type99(
            "SEND_COMPLETE",
            session_id,
            session_id,
            "bridge_cleanup",
            "viewer_disconnect_type_99_written_to_host",
        );
    }
}

fn run_websocket_bridge(
    session_id: &str,
    stream: &mut TcpStream,
    ws_arc: Arc<Mutex<tungstenite::WebSocket<TcpStream>>>,
) -> bool {
    let mut host_reader = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut host_writer = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let is_active = Arc::new(AtomicBool::new(true));
    let is_active_reader = Arc::clone(&is_active);
    let host_ended_session = Arc::new(AtomicBool::new(false));
    let host_ended_session_reader = Arc::clone(&host_ended_session);
    let session_id_for_thread = session_id.to_string();

    // Dedicated WS-writer channels
    let (ws_tx_ctrl, ws_rx_ctrl) = std::sync::mpsc::channel::<Vec<u8>>();
    let (ws_tx_vid, ws_rx_vid) = std::sync::mpsc::sync_channel::<Vec<u8>>(2);

    let ws_tx_ctrl_fwd = ws_tx_ctrl.clone();
    let ws_tx_vid_fwd = ws_tx_vid.clone();

    let ws_arc_writer = Arc::clone(&ws_arc);
    let ws_arc_closer = Arc::clone(&ws_arc);
    let is_active_writer = Arc::clone(&is_active);
    let is_active_writer_guard = Arc::clone(&is_active);
    let session_id_writer = session_id.to_string();

    // WS writer thread (guarantees Approval + Active first)
    let ws_writer_handle = thread::spawn(move || {
        let _active_guard = WebSocketBridgeWorkerGuard(is_active_writer_guard);
        if let Ok(mut ws) = ws_arc_writer.lock() {
            trace_packet("RELAY->VIEWER", &session_id_writer, "RELAY", "VIEWER", 1, 1);
            let _ = ws.send(Message::Binary(vec![1u8]));
            trace_packet("RELAY->VIEWER", &session_id_writer, "RELAY", "VIEWER", 2, 1);
            let _ = ws.send(Message::Binary(vec![2u8]));
        }

        let mut idle_ms = 0;
        let mut forwarded_video_count = 0u64;
        let mut forwarded_heartbeat_count = 0u64;
        while is_active_writer.load(Ordering::SeqCst) {
            let mut did_work = false;

            for msg_bytes in take_control_burst(&ws_rx_ctrl) {
                if let Some(&packet_type) = msg_bytes.first() {
                    trace_packet("RELAY->VIEWER", &session_id_writer, "RELAY", "VIEWER", packet_type, msg_bytes.len());
                }
                let is_type99 = msg_bytes.first() == Some(&99);
                if is_type99 {
                    trace_type99("SEND", &session_id_writer, &session_id_writer, "ws_writer_control_queue", "queued_binary_type_99");
                }
                let mut lock = match ws_arc_writer.lock() {
                    Ok(lock) => lock,
                    Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                };
                let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                if let Err(error) = lock.send(Message::Binary(msg_bytes)) {
                    eprintln!("[WS CLOSE] component=ws_writer reason=ctrl_send_failed error={:?} device={}", error, session_id_writer);
                    is_active_writer.store(false, Ordering::SeqCst);
                    break;
                }
                if is_type99 {
                    trace_type99("SEND_COMPLETE", &session_id_writer, &session_id_writer, "ws_writer_control_queue", "binary_type_99_sent_to_viewer");
                }
                did_work = true;
                idle_ms = 0;
            }
            if !is_active_writer.load(Ordering::SeqCst) { break; }

            match ws_rx_vid.try_recv() {
                Ok(msg_bytes) => {
                    if let Some(&packet_type) = msg_bytes.first() {
                        forwarded_video_count += u64::from(packet_type == 13 || packet_type == 15);
                        if forwarded_video_count == 1 || forwarded_video_count % 60 == 0 || packet_type == 99 {
                            trace_packet("RELAY->VIEWER", &session_id_writer, "RELAY", "VIEWER", packet_type, msg_bytes.len());
                        }
                    }
                    let is_type99 = msg_bytes.first() == Some(&99);
                    if is_type99 {
                        trace_type99("SEND", &session_id_writer, &session_id_writer, "ws_writer_video_queue", "queued_binary_type_99");
                    }
                    let mut lock = match ws_arc_writer.lock() {
                        Ok(lock) => lock,
                        Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                    };
                    let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                    if let Err(error) = lock.send(Message::Binary(msg_bytes)) {
                        eprintln!("[WS CLOSE] component=ws_writer reason=vid_send_failed error={:?} device={}", error, session_id_writer);
                        is_active_writer.store(false, Ordering::SeqCst);
                        break;
                    }
                    if is_type99 {
                        trace_type99("SEND_COMPLETE", &session_id_writer, &session_id_writer, "ws_writer_video_queue", "binary_type_99_sent_to_viewer");
                    }
                    did_work = true;
                    idle_ms = 0;
                }
                Err(_) => {}
            }
            if !is_active_writer.load(Ordering::SeqCst) { break; }

            if !did_work {
                match ws_rx_ctrl.recv_timeout(Duration::from_millis(10)) {
                    Ok(msg_bytes) => {
                        if let Some(&packet_type) = msg_bytes.first() {
                            if packet_type == 14 {
                                forwarded_heartbeat_count += 1;
                            }
                            if packet_type != 14 || forwarded_heartbeat_count == 1 || forwarded_heartbeat_count % 50 == 0 {
                                trace_packet("RELAY->VIEWER", &session_id_writer, "RELAY", "VIEWER", packet_type, msg_bytes.len());
                            }
                        }
                        let is_type99 = msg_bytes.first() == Some(&99);
                        if is_type99 {
                            trace_type99("SEND", &session_id_writer, &session_id_writer, "ws_writer_control_recv_timeout", "queued_binary_type_99");
                        }
                        let mut lock = match ws_arc_writer.lock() {
                            Ok(lock) => lock,
                            Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                        };
                        let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                        if let Err(error) = lock.send(Message::Binary(msg_bytes)) {
                            eprintln!("[WS CLOSE] component=ws_writer reason=ctrl_send_failed error={:?} device={}", error, session_id_writer);
                            is_active_writer.store(false, Ordering::SeqCst);
                            break;
                        }
                        if is_type99 {
                            trace_type99("SEND_COMPLETE", &session_id_writer, &session_id_writer, "ws_writer_control_recv_timeout", "binary_type_99_sent_to_viewer");
                        }
                        did_work = true;
                        idle_ms = 0;
                    }
                    Err(_) => {}
                }
                if !is_active_writer.load(Ordering::SeqCst) { break; }
                if !did_work {
                    if let Ok(msg_bytes) = ws_rx_vid.try_recv() {
                        if let Some(&packet_type) = msg_bytes.first() {
                            forwarded_video_count += u64::from(packet_type == 13 || packet_type == 15);
                            if forwarded_video_count == 1 || forwarded_video_count % 60 == 0 || packet_type == 99 {
                                trace_packet("RELAY->VIEWER", &session_id_writer, "RELAY", "VIEWER", packet_type, msg_bytes.len());
                            }
                        }
                        let is_type99 = msg_bytes.first() == Some(&99);
                        if is_type99 {
                            trace_type99("SEND", &session_id_writer, &session_id_writer, "ws_writer_video_fallback", "queued_binary_type_99");
                        }
                        let mut lock = match ws_arc_writer.lock() {
                            Ok(lock) => lock,
                            Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                        };
                        let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                        if let Err(error) = lock.send(Message::Binary(msg_bytes)) {
                            eprintln!("[WS CLOSE] component=ws_writer reason=vid_send_failed error={:?} device={}", error, session_id_writer);
                            is_active_writer.store(false, Ordering::SeqCst);
                            break;
                        }
                        if is_type99 {
                            trace_type99("SEND_COMPLETE", &session_id_writer, &session_id_writer, "ws_writer_video_fallback", "binary_type_99_sent_to_viewer");
                        }
                        did_work = true;
                        idle_ms = 0;
                    }
                }
            }
            if did_work { continue; }

            idle_ms += 10;
            if idle_ms >= 100 {
                let mut lock = match ws_arc_writer.lock() {
                    Ok(l) => l,
                    Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                };
                let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                if lock.send(Message::Ping(vec![])).is_err() {
                    is_active_writer.store(false, Ordering::SeqCst);
                    break;
                }
                idle_ms = 0;
            }
        }
        println!("[WS CLOSE] component=ws_writer reason=exiting device={}", session_id_writer);
    });

    // Host -> WS forwarder thread
    let host_to_ws_handle = thread::spawn(move || {
        let _active_guard = WebSocketBridgeWorkerGuard(Arc::clone(&is_active_reader));
        println!("[WS LIFECYCLE] host_to_ws thread started for device={}", session_id_for_thread);
        let mut video_frame_count = 0u64;
        let mut heartbeat_count = 0u64;
        while is_active_reader.load(Ordering::SeqCst) {
            let _ = host_reader.set_read_timeout(Some(Duration::from_millis(200)));
            let mut type_buf = [0u8; 1];
            if let Err(e) = host_reader.read_exact(&mut type_buf) {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!("[WS CLOSE] component=host_to_ws reason=host_read_failed error={:?} device={}", e, session_id_for_thread);
                host_ended_session_reader.store(true, Ordering::SeqCst);
                break;
            }
            let pkt = type_buf[0];

            match pkt {
                13 | 15 => {
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    let msg = match read_host_video_packet(&mut host_reader, pkt) {
                        Ok(msg) => msg,
                        Err(e) => {
                            eprintln!("[WS CLOSE] component=host_to_ws reason=video_packet_read_failed error={:?} device={}", e, session_id_for_thread);
                            host_ended_session_reader.store(true, Ordering::SeqCst);
                            break;
                        }
                    };
                    video_frame_count += 1;
                    if video_frame_count == 1 || video_frame_count % 60 == 0 {
                        trace_packet("HOST->RELAY", &session_id_for_thread, "HOST", "RELAY", pkt, msg.len());
                    }
                    if video_frame_count == 1 || video_frame_count % 120 == 0 {
                        println!("[VIDEO RELAY RX] type={} bytes={} frames={}", pkt, msg.len(), video_frame_count);
                    }
                    let msg_len = msg.len();
                    if ws_tx_vid_fwd.send(msg).is_err() {
                        eprintln!("[WS ERROR] component=host_to_ws reason=vid_channel_closed device={}", session_id_for_thread);
                        break;
                    } else if video_frame_count == 1 || video_frame_count % 120 == 0 {
                        println!("[VIDEO RELAY TX] type={} bytes={} frames={}", pkt, msg_len, video_frame_count);
                    }
                }
                17 => {
                    let mut header = [0u8; 10];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = host_reader.read_exact(&mut header) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=audio_header_failed error={:?} device={}", e, session_id_for_thread);
                        host_ended_session_reader.store(true, Ordering::SeqCst);
                        break;
                    }
                    let psize = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
                    if psize > 10 * 1024 * 1024 { break; }
                    let mut payload = vec![0u8; psize];
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=audio_payload_failed error={:?} device={}", e, session_id_for_thread);
                        host_ended_session_reader.store(true, Ordering::SeqCst);
                        break;
                    }
                    let mut msg = Vec::with_capacity(1 + 10 + psize);
                    msg.push(17u8);
                    msg.extend_from_slice(&header);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                14 => {
                    let mut payload = [0u8; 8];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=heartbeat_failed error={:?} device={}", e, session_id_for_thread);
                        host_ended_session_reader.store(true, Ordering::SeqCst);
                        break;
                    }
                    println!("[HEARTBEAT] device_id={}", session_id_for_thread);
                    heartbeat_count += 1;
                    if heartbeat_count == 1 || heartbeat_count % 50 == 0 {
                        trace_packet("HOST->RELAY", &session_id_for_thread, "HOST", "RELAY", pkt, 9);
                    }
                    let mut msg = Vec::with_capacity(9);
                    msg.push(14u8);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                12 => {
                    let mut hdr = [0u8; 4];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = host_reader.read_exact(&mut hdr) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=clipboard_header_failed error={:?} device={}", e, session_id_for_thread);
                        host_ended_session_reader.store(true, Ordering::SeqCst);
                        break;
                    }
                    let psize = u32::from_be_bytes(hdr) as usize;
                    if psize > 50 * 1024 * 1024 { break; }
                    let mut payload = vec![0u8; psize];
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=clipboard_payload_failed error={:?} device={}", e, session_id_for_thread);
                        host_ended_session_reader.store(true, Ordering::SeqCst);
                        break;
                    }
                    let mut msg = Vec::with_capacity(5 + psize);
                    msg.push(12u8);
                    msg.extend_from_slice(&hdr);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                16 => {
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    let msg = match read_host_chat_packet(&mut host_reader) {
                        Ok(msg) => msg,
                        Err(error) => {
                            eprintln!("[WS CLOSE] component=host_to_ws reason=chat_packet_read_failed error={:?} device={}", error, session_id_for_thread);
                            host_ended_session_reader.store(true, Ordering::SeqCst);
                            break;
                        }
                    };
                    let psize = msg.len() - 4;
                    let packet_hex = msg.iter().map(|byte| format!("{:02x}", byte)).collect::<Vec<_>>().join(" ");
                    if ws_tx_ctrl_fwd.send(msg).is_err() {
                        eprintln!("[WS ERROR] component=host_to_ws reason=chat_channel_closed device={}", session_id_for_thread);
                        break;
                    }
                    println!("[CHAT RELAY TRACE] direction=B->A type=16 payload_bytes={} packet_bytes={} bytes_hex=\"{}\"",
                        psize, 4 + psize, packet_hex);
                }
                CHAT_TYPING_PACKET_TYPE => {
                    let mut state = [0u8; 1];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(error) = host_reader.read_exact(&mut state) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=typing_state_failed error={:?} device={}", error, session_id_for_thread);
                        host_ended_session_reader.store(true, Ordering::SeqCst);
                        break;
                    }
                    let Some(message) = chat_typing_packet(state[0]) else {
                        eprintln!("[WS ERROR] component=host_to_ws reason=invalid_typing_state state={} device={}", state[0], session_id_for_thread);
                        continue;
                    };
                    if ws_tx_ctrl_fwd.send(message).is_err() {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=typing_channel_closed device={}", session_id_for_thread);
                        break;
                    }
                }
                20 => {
                    let mut hdr = [0u8; 18];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(_) = host_reader.read_exact(&mut hdr) { break; }
                    let name_len = u16::from_be_bytes([hdr[16], hdr[17]]) as usize;
                    if name_len > 4096 { break; }
                    let mut payload = vec![0u8; name_len];
                    if let Err(_) = host_reader.read_exact(&mut payload) { break; }
                    let mut msg = Vec::with_capacity(1 + 18 + name_len);
                    msg.push(20u8);
                    msg.extend_from_slice(&hdr);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                21 => {
                    let mut hdr = [0u8; 16];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(_) = host_reader.read_exact(&mut hdr) { break; }
                    let payload_len = u32::from_be_bytes(hdr[12..16].try_into().unwrap()) as usize;
                    if payload_len > 10 * 1024 * 1024 { break; }
                    let mut payload = vec![0u8; payload_len];
                    if let Err(_) = host_reader.read_exact(&mut payload) { break; }
                    let mut msg = Vec::with_capacity(1 + 16 + payload_len);
                    msg.push(21u8);
                    msg.extend_from_slice(&hdr);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                22 => {
                    let mut hdr = [0u8; 48];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(_) = host_reader.read_exact(&mut hdr) { break; }
                    let mut msg = Vec::with_capacity(1 + 48);
                    msg.push(22u8);
                    msg.extend_from_slice(&hdr);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                23 => {
                    let mut hdr = [0u8; 8];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(_) = host_reader.read_exact(&mut hdr) { break; }
                    let mut msg = Vec::with_capacity(1 + 8);
                    msg.push(23u8);
                    msg.extend_from_slice(&hdr);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                24 => {
                    let mut hdr = [0u8; 12];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(_) = host_reader.read_exact(&mut hdr) { break; }
                    let msg_len = u16::from_be_bytes([hdr[10], hdr[11]]) as usize;
                    if msg_len > 4096 { break; }
                    let mut payload = vec![0u8; msg_len];
                    if let Err(_) = host_reader.read_exact(&mut payload) { break; }
                    let mut msg = Vec::with_capacity(1 + 12 + msg_len);
                    msg.push(24u8);
                    msg.extend_from_slice(&hdr);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                25 | 26 | 27 => {
                    let mut hdr = [0u8; 8];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(_) = host_reader.read_exact(&mut hdr) { break; }
                    let mut msg = Vec::with_capacity(1 + 8);
                    msg.push(pkt);
                    msg.extend_from_slice(&hdr);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                REVERSE_REQUEST_PACKET_TYPE => {
                    if ws_tx_ctrl_fwd.send(reverse_request_packet()).is_err() {
                        eprintln!("[WS ERROR] component=host_to_ws reason=reverse_request_channel_closed device={}", session_id_for_thread);
                        break;
                    }
                }
                REVERSE_ACK_PACKET_TYPE => {
                    let mut state = [0u8; 1];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(error) = host_reader.read_exact(&mut state) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=reverse_ack_read_failed error={:?} device={}", error, session_id_for_thread);
                        break;
                    }
                    let Some(message) = reverse_ack_packet(state[0]) else {
                        eprintln!("[WS ERROR] component=host_to_ws reason=invalid_reverse_ack state={} device={}", state[0], session_id_for_thread);
                        continue;
                    };
                    if ws_tx_ctrl_fwd.send(message).is_err() {
                        eprintln!("[WS ERROR] component=host_to_ws reason=reverse_ack_channel_closed device={}", session_id_for_thread);
                        break;
                    }
                }
                99 => {
                    trace_type99("RECV", &session_id_for_thread, &session_id_for_thread, "host_to_ws", "host_tcp_type_99_consumed_not_forwarded_to_viewer");
                    trace_packet("HOST->RELAY", &session_id_for_thread, "HOST", "RELAY", pkt, 1);
                    trace_type99("RECV_COMPLETE", &session_id_for_thread, &session_id_for_thread, "host_to_ws", "type_99_consumed_without_viewer_forward");
                    println!("[WS CLOSE] component=host_to_ws reason=host_sent_99 device={}", session_id_for_thread);
                    host_ended_session_reader.store(true, Ordering::SeqCst);
                    break;
                }
                _ => {
                    eprintln!("[WS ERROR] component=host_to_ws reason=unknown_type type={} device={}", pkt, session_id_for_thread);
                }
            }
        }
        println!("[WS CLOSE] component=host_to_ws reason=thread_exit device={}", session_id_for_thread);

        let host_ended_session = host_ended_session_reader.load(Ordering::SeqCst);
        is_active_reader.store(false, Ordering::SeqCst);

        if let Ok(mut ws) = ws_arc_closer.lock() {
            if host_ended_session {
                eprintln!("[WS CLOSE] device={} -> AGENT/HOST side disconnected (genuine host read error).", session_id_for_thread);
                let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Error,
                    reason: "Host disconnected".into(),
                }));
            } else {
                println!("[WS CLOSE] device={} -> VIEWER side ended session. Host agent connection is preserved.", session_id_for_thread);
                let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Normal,
                    reason: "Viewer disconnected".into(),
                }));
            }
            let _ = ws.get_ref().shutdown(Shutdown::Both);
        }
    });

    // WebSocket -> Host (control input loop)
    let mut mouse_move_count = 0u64;
    loop {
        if !is_active.load(Ordering::SeqCst) {
            println!("[WS CLOSE] component=ws_to_host reason=active_flag_false device={}", session_id);
            break;
        }

        let msg_res = {
            let mut lock = match ws_arc.lock() {
                Ok(l) => l,
                Err(_) => {
                    eprintln!("[WS CLOSE] component=ws_to_host reason=mutex_poisoned device={}", session_id);
                    break;
                }
            };
            let _ = lock.get_ref().set_read_timeout(Some(Duration::from_millis(10)));
            lock.read()
        };

        match msg_res {
            Ok(Message::Binary(data)) => {
                let is_type99 = data.first() == Some(&99);
                if let Some(&packet_type) = data.first() {
                    trace_packet("VIEWER->RELAY", session_id, "VIEWER", "RELAY", packet_type, data.len());
                }
                if is_type99 {
                    trace_packet("RELAY->HOST", session_id, "RELAY", "HOST", 99, data.len());
                    trace_type99("RECV", session_id, session_id, "viewer_websocket_binary", "binary_type_99");
                }
                let (type_name, log_control_packet) = if let Some(&packet_type) = data.first() {
                    let type_name = match packet_type {
                        0 => "MOUSE_MOVE", 1 | 3 | 7 => "MOUSE_DOWN", 2 | 4 | 8 => "MOUSE_UP",
                        5 => "KEY_DOWN", 6 => "KEY_UP", 9 => "MOUSE_WHEEL", _ => "CONTROL",
                    };
                    let log_packet = if packet_type == 0 {
                        mouse_move_count += 1;
                        mouse_move_count == 1 || mouse_move_count % 120 == 0
                    } else {
                        true
                    };
                    if log_packet {
                        println!("[CONTROL RX] type={} bytes={} device={}", packet_type, data.len(), session_id);
                        println!("[RELAY CONTROL RX] {}", type_name);
                        println!("[CONTROL DEBUG][RELAY RX]\ntype={}\nlength={}", type_name, data.len());
                    }
                    (type_name, log_packet)
                } else {
                    ("CONTROL", false)
                };
                if !send_all(&mut host_writer, &data) {
                    eprintln!("[WS ERROR] component=ws_to_host reason=control_write_failed type={} device={}",
                        data.first().copied().unwrap_or(255), session_id);
                    host_ended_session.store(true, Ordering::SeqCst);
                    let _ = host_writer.shutdown(Shutdown::Both);
                    break;
                } else {
                    if is_type99 {
                        trace_type99("SEND", session_id, session_id, "ws_to_host", "viewer_binary_forwarded_to_host");
                    }
                    if log_control_packet {
                        println!("[CONTROL DEBUG][RELAY -> HOST]\ntype={}\nlength={}", type_name, data.len());
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let mut lock = match ws_arc.lock() {
                    Ok(l) => l,
                    Err(_) => { eprintln!("[WS CLOSE] component=ws_to_host reason=ping_mutex_poisoned device={}", session_id); break; }
                };
                if let Err(e) = lock.send(Message::Pong(data)) {
                    eprintln!("[WS CLOSE] component=ws_to_host reason=pong_failed error={:?} device={}", e, session_id);
                    break;
                }
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(frame)) => {
                println!("[WS CLOSE] component=ws_to_host reason=browser_sent_close device={}", session_id);
                println!("[RELAY][CLOSE] connection_type=viewer device={} reason=browser_close error={:?} peer_connected=true", session_id, frame);
                break;
            }
            Err(tungstenite::error::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(e) => {
                println!("[WS CLOSE] component=ws_to_host reason=ws_read_error error={:?} device={}", e, session_id);
                println!("[RELAY][ERROR] WS read error: {:?}", e);
                println!("[RELAY][CLOSE] connection_type=viewer device={} reason=ws_read_error error={:?} peer_connected=true", session_id, e);
                break;
            }
            _ => {}
        }
    }

    println!("[RELAY] Viewer disconnected: main_loop_exited");
    println!("[RELAY] Session ended: main_loop_exited");
    println!("[WS CLOSE] component=bridge reason=main_loop_exited device={}", session_id);
    is_active.store(false, Ordering::SeqCst);

    let host_to_ws_panicked = host_to_ws_handle.join().is_err();
    notify_host_viewer_disconnect(
        session_id,
        &mut host_writer,
        host_ended_session.load(Ordering::SeqCst),
    );

    drop(ws_tx_ctrl);
    drop(ws_tx_vid);

    let ws_writer_panicked = ws_writer_handle.join().is_err();
    if host_to_ws_panicked || ws_writer_panicked {
        eprintln!(
            "[WS ERROR] component=bridge reason=worker_panicked host_to_ws_panicked={} ws_writer_panicked={} device={}",
            host_to_ws_panicked, ws_writer_panicked, session_id
        );
    }
    true
}

// ============================================================
// BACKEND PRESENCE PROXY
// ============================================================

fn read_server_config() -> Option<String> {
    let path = if cfg!(windows) {
        let pf = env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        std::path::Path::new(&pf).join("Screen Share").join("server-config.json")
    } else {
        std::path::PathBuf::from("/etc/deskstream/server-config.json")
    };
    let contents = std::fs::read_to_string(&path).ok()?;
    let key = "\"backend_url\":";
    if let Some(pos) = contents.find(key) {
        let after = &contents[pos + key.len()..];
        let start = after.find('"')? + 1;
        let end = after[start..].find('"')? + start;
        return Some(after[start..end].to_string());
    }
    None
}

fn resolve_backend_url(
    backend_url: &str,
    backend_host: Option<&str>,
    backend_port: Option<&str>,
) -> Result<Url, String> {
    let mut url = Url::parse(backend_url).map_err(|_| "invalid backend URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("backend URL must be an HTTP(S) URL without embedded credentials, query, or fragment".to_string());
    }
    if let Some(host) = backend_host.filter(|host| !host.is_empty()) {
        url.set_host(Some(host))
            .map_err(|_| "invalid SCREENSHARE_BACKEND_HOST".to_string())?;
    }
    if let Some(port) = backend_port.filter(|port| !port.is_empty()) {
        let port = port
            .parse::<u16>()
            .map_err(|_| "invalid SCREENSHARE_BACKEND_PORT".to_string())?;
        if port == 0 {
            return Err("invalid SCREENSHARE_BACKEND_PORT".to_string());
        }
        url.set_port(Some(port))
            .map_err(|_| "invalid SCREENSHARE_BACKEND_PORT".to_string())?;
    }
    let normalized_path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&normalized_path);
    Ok(url)
}

fn configured_backend_url() -> Result<Url, String> {
    let backend_url = env::var("SCREENSHARE_BACKEND_URL")
        .ok()
        .or_else(read_server_config)
        .unwrap_or_else(|| "http://127.0.0.1/Screen%20Share/backend/api".to_string());
    resolve_backend_url(
        &backend_url,
        env::var("SCREENSHARE_BACKEND_HOST").ok().as_deref(),
        env::var("SCREENSHARE_BACKEND_PORT").ok().as_deref(),
    )
}

fn heartbeat_endpoint(base_url: &Url) -> Result<Url, String> {
    base_url
        .join("devices/heartbeat.php")
        .map_err(|_| "invalid heartbeat endpoint URL".to_string())
}

fn validate_presence_response(
    status: reqwest::StatusCode,
    body: serde_json::Value,
) -> Result<(), String> {
    if !status.is_success() {
        return Err(format!("backend returned HTTP {status}"));
    }
    if body.get("status").and_then(|value| value.as_str()) != Some("success") {
        return Err("backend reported heartbeat failure".to_string());
    }
    if body.get("updated").and_then(|value| value.as_bool()) != Some(true) {
        return Err("backend did not update a matching device record".to_string());
    }
    Ok(())
}

fn sanitized_backend_response_body(body: &str) -> String {
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_str::<serde_json::Value>(body)
    else {
        return "non_json_or_unrecognized_body".to_string();
    };

    let status = match fields.get("status").and_then(serde_json::Value::as_str) {
        Some("success") => "success",
        Some("error") => "error",
        _ => "unknown",
    };
    let message = match fields
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
    {
        Some(message) if message.eq_ignore_ascii_case("unauthorized") => "unauthorized",
        Some(message) if message.eq_ignore_ascii_case("forbidden") => "forbidden",
        Some(message) if message.starts_with("Method not allowed") => "method_not_allowed",
        Some(message) if message.starts_with("Missing system_id") => "missing_device_identifier",
        Some(message) if message.eq_ignore_ascii_case("Heartbeat received.") => "heartbeat_received",
        Some(_) => "unrecognized_message_omitted",
        None => "not_provided",
    };

    format!(r#"{{"status":"{status}","message":"{message}"}}"#)
}

fn send_backend_presence(
    client: &Client,
    base_url: &Url,
    session_id: &str,
) -> Result<(), String> {
    let endpoint = heartbeat_endpoint(base_url)?;
    let response = client
        .post(endpoint.clone())
        .json(&serde_json::json!({ "system_id": session_id }))
        .send()
        .map_err(|error| format!("endpoint={} method=POST request failed: {error}", endpoint.path()))?;
    let status = response.status();
    let mut body_bytes = Vec::new();
    response
        .take(MAX_BACKEND_RESPONSE_BYTES)
        .read_to_end(&mut body_bytes)
        .map_err(|_| format!("endpoint={} HTTP {status} response body unavailable", endpoint.path()))?;
    let body = String::from_utf8_lossy(&body_bytes);
    if !status.is_success() {
        return Err(format!(
            "endpoint={} method=POST HTTP {} sanitized_body={}",
            endpoint.path(),
            status,
            sanitized_backend_response_body(&body)
        ));
    }
    let body = serde_json::from_str::<serde_json::Value>(&body).map_err(|_| {
        format!(
            "endpoint={} HTTP {status} sanitized_body=malformed_json_omitted",
            endpoint.path()
        )
    })?;
    validate_presence_response(status, body)
}

fn start_backend_presence_worker() {
    let client = match Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            eprintln!("[PROXY][ERROR] stage=client_init error={}", error);
            return;
        }
    };
    let base_url = match configured_backend_url() {
        Ok(url) => url,
        Err(error) => {
            eprintln!("[PROXY][ERROR] stage=config error={}", error);
            return;
        }
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel::<String>(128);
    let pending = Arc::new(Mutex::new(HashMap::<String, ()>::new()));
    if BACKEND_PRESENCE_QUEUE
        .set((sender, Arc::clone(&pending)))
        .is_err()
    {
        return;
    }

    thread::spawn(move || {
        let mut success_counts = HashMap::<String, u64>::new();

        while let Ok(session_id) = receiver.recv() {
            let mut result = Err("request was not attempted".to_string());
            for attempt in 1..=3 {
                result = send_backend_presence(&client, &base_url, &session_id);
                if result.is_ok() {
                    break;
                }
                if attempt < 3 {
                    thread::sleep(Duration::from_millis(500 * attempt as u64));
                }
            }
            if let Ok(mut entries) = pending.lock() {
                entries.remove(&session_id);
            }
            match result {
                Ok(()) => {
                    let count = success_counts.entry(session_id.clone()).or_default();
                    *count += 1;
                    if *count == 1 || *count % 50 == 0 {
                        println!(
                            "[PROXY] stage=heartbeat_sync device_id={} result=updated count={}",
                            session_id, count
                        );
                    }
                }
                Err(error) => eprintln!(
                    "[PROXY][ERROR] stage=heartbeat_sync device_id={} attempts=3 error={}",
                    session_id, error
                ),
            }
        }
    });
}

fn update_backend_presence(session_id: &str, _is_register: bool) {
    let Some((sender, pending)) = BACKEND_PRESENCE_QUEUE.get() else {
        eprintln!(
            "[PROXY][ERROR] stage=enqueue device_id={} reason=worker_not_started",
            session_id
        );
        return;
    };
    let mut entries = match pending.lock() {
        Ok(entries) => entries,
        Err(_) => {
            eprintln!(
                "[PROXY][ERROR] stage=enqueue device_id={} reason=pending_lock_poisoned",
                session_id
            );
            return;
        }
    };
    if entries.contains_key(session_id) {
        return;
    }
    entries.insert(session_id.to_string(), ());
    if let Err(error) = sender.try_send(session_id.to_string()) {
        entries.remove(session_id);
        eprintln!(
            "[PROXY][ERROR] stage=enqueue device_id={} reason=queue_full_or_closed error={}",
            session_id, error
        );
    }
}

// ============================================================
// HOST CONNECTION & LIFECYCLE (PERSISTENT AGENT THREAD)
// ============================================================

fn handle_host(mut stream: TcpStream, hosts: ClientMap) {
    let conn_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
    let mut type_byte = [0u8; 1];

    if !read_exact_logged(&mut stream, &mut type_byte, "Host packet type") {
        return;
    }

    let packet_type = type_byte[0];

    if packet_type != 1 {
        eprintln!("[Relay] Invalid host registration packet type: {}", packet_type);
        return;
    }

    let id_len = 9;
    let mut actual_id_buf = vec![0u8; id_len];
    if !read_exact_logged(&mut stream, &mut actual_id_buf, "Host session ID") {
        return;
    }

    let session_id = match std::str::from_utf8(&actual_id_buf) {
        Ok(id) if valid_device_id(id) => id.to_string(),
        Err(_) => {
            eprintln!("[Relay] Invalid host session ID.");
            return;
        }
        _ => {
            eprintln!("[Relay] Invalid host session ID format.");
            return;
        }
    };

    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    let _ = stream.set_nodelay(true);

    println!("[RELAY] Client connected: {}", peer);
    println!("[RELAY] Registration received: {}", session_id);

    if !send_all(&mut stream, &[1u8]) {
        eprintln!("[RELAY][ERROR] Failed to send registration ACK to Device ID: {}", session_id);
        return;
    }

    println!("[RELAY] Registration ACK sent: {}", session_id);
    println!("[RELAY] Agent connected");
    println!("[RELAY] Registered agent: {}", session_id);
    println!("[RELAY] activeAgents[{}] = ONLINE", session_id);
    println!("[REGISTRY] Register device");

    update_backend_presence(&session_id, true);
    println!("[REGISTRY] Device ID: {}", session_id);
    println!("[REGISTRY] Connection ID: {}", peer);
    println!("[REGISTRY] Status: ONLINE");
    println!("[RELAY] Connection remains active");

    let (session_tx, session_rx): (Sender<ViewerSessionRequest>, Receiver<ViewerSessionRequest>) = channel();

    if let Ok(mut map) = hosts.lock() {
        let replaced = map.insert(session_id.clone(), (conn_id, session_tx));
        if let Some((previous_conn_id, _)) = replaced {
            println!(
                "[TRACE99][REGISTRY] timestamp={} device={} old_connection_id={} new_connection_id={} event=HOST_REGISTRATION_REPLACED",
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
                session_id, previous_conn_id, conn_id
            );
        } else {
            println!(
                "[TRACE99][REGISTRY] timestamp={} device={} connection_id={} event=HOST_REGISTERED",
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
                session_id, conn_id
            );
        }
        println!("[RELAY] Agent registered: {}", session_id);
        println!("[RELAY] Active agents: {}", map.len());
    } else {
        eprintln!("[AUTH][ERROR] Failed to lock host registry.");
        return;
    }

    // ============================================================
    // PERSISTENT HOST CONNECTION & HEARTBEAT LOOP
    // ============================================================

    let mut idle_buf = [0u8; 1];
    let host_peer = peer.clone();

    loop {
        // 1. Check for incoming viewer session request
        if let Ok(req) = session_rx.try_recv() {
            println!("[ROUTING] Forwarding request to agent: {}", session_id);
            println!("[PAIR] Preparing pairing message");
            let mut auth_request = Vec::with_capacity(33);
            auth_request.push(3u8);
            auth_request.extend_from_slice(&req.auth_hash);

            println!("[PAIR] Sending AUTH_REQUEST to HOST");
            trace_packet("RELAY->HOST", &session_id, "RELAY", "HOST", 3, auth_request.len());
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            if !send_all_pair("HOST", &mut stream, &auth_request) {
                eprintln!("[PAIR][ERROR] Failed to send authentication request: {}", session_id);
                let _ = req.response_tx.send(false);
                continue;
            }

            println!("[PAIR] Waiting for HOST authentication response");
            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
            let approved = loop {
                let mut response = [0u8; 1];
                if let Err(e) = stream.read_exact(&mut response) {
                    eprintln!("[PAIR][ERROR] Authentication response read failed: {}", e);
                    break false;
                }

                match response[0] {
                    0x01 => {
                        trace_packet("HOST->RELAY", &session_id, "HOST", "RELAY", 1, 1);
                        println!("[PAIR] HOST authentication response byte=0x01");
                        break true;
                    }
                    0x00 => {
                        trace_packet("HOST->RELAY", &session_id, "HOST", "RELAY", 0, 1);
                        println!("[PAIR] HOST authentication response byte=0x00");
                        println!("[APPROVAL] Host rejected: {}", session_id);
                        break false;
                    }
                    0x0E => {
                        trace_packet("HOST->RELAY", &session_id, "HOST", "RELAY", 14, 9);
                        println!("[PAIR] Heartbeat received while waiting for authentication response");
                        let mut heartbeat = [0u8; 8];
                        if let Err(e) = stream.read_exact(&mut heartbeat) {
                            eprintln!("[PAIR][ERROR] Authentication response read failed: {}", e);
                            break false;
                        }
                        let mut ack = [0u8; 9];
                        ack[0] = 0x0E;
                        ack[1..].copy_from_slice(&heartbeat);
                        if let Err(e) = stream.write_all(&ack) {
                            eprintln!("[PAIR][ERROR] Failed to acknowledge heartbeat during authentication: {}", e);
                            break false;
                        }
                        trace_packet("RELAY->HOST", &session_id, "RELAY", "HOST", 14, ack.len());
                    }
                    other => {
                        eprintln!("[PAIR][ERROR] Unexpected byte 0x{:02X} while waiting for HOST authentication response", other);
                        break false;
                    }
                }
            };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
            if !approved {
                match req.session {
                    ViewerSession::Tcp(mut viewer) => {
                        let _ = send_all_pair("VIEWER", &mut viewer, &[3u8]);
                        let _ = viewer.shutdown(Shutdown::Both);
                    }
                    ViewerSession::WebSocket(ws_arc) => {
                        if let Ok(mut ws) = ws_arc.lock() {
                            let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                                code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                                reason: "Rejected".into(),
                            }));
                        }
                    }
                }
                let _ = req.response_tx.send(false);
                continue;
            }

            println!("[APPROVAL] Target {} accepted", session_id);
            println!("[APPROVAL] Request sent to {}", session_id);
            println!("[RELAY] Session paired");
            println!("[APPROVAL] Target {} accepted", session_id);
            let _ = req.response_tx.send(true);

            match req.session {
                ViewerSession::WebSocket(ws_arc) => {
                    run_websocket_bridge(&session_id, &mut stream, ws_arc);
                }
                ViewerSession::Tcp(mut viewer_tcp) => {
                    println!("[PAIR] Starting TCP bridge for {}", session_id);
                    let _ = send_all_pair("VIEWER", &mut viewer_tcp, &[2u8]);

                    if bridge_tcp_session(&stream, viewer_tcp) {
                        break;
                    }
                }
            }

            println!("[REGISTRY] Re-registering host {} in active registry (viewer session ended)", session_id);
            println!("[RELAY] activeAgents[{}] = ONLINE", session_id);
            println!("[REGISTRY] Device ID: {}", session_id);
            println!("[REGISTRY] Status: ONLINE");
            println!("[RELAY] Connection remains active");
            continue;
        }

        // 2. Non-blocking read with 200ms timeout for Heartbeats and Keepalive
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        match stream.read_exact(&mut idle_buf) {
            Ok(_) => {
                match idle_buf[0] {
                    14 => {
                        let mut time_buf = [0u8; 8];
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        if stream.read_exact(&mut time_buf).is_ok() {
                            println!("[HEARTBEAT] device_id={}", session_id);
                            println!("[RELAY] Agent {} heartbeat", session_id);
                            let mut ack = Vec::with_capacity(9);
                            ack.push(14u8);
                            ack.extend_from_slice(&time_buf);
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                            let _ = stream.write_all(&ack);

                            update_backend_presence(&session_id, false);
                        }
                    }
                    other => {
                        eprintln!("[RELAY] Received unexpected idle byte: {} from agent {}", other, session_id);
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    println!("[TRACE99][SOCKET] HOST EOF timestamp={} device={} connection_id={} error={:?}",
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
                        session_id, conn_id, e);
                } else {
                    eprintln!("[TRACE99][SOCKET] HOST ERROR timestamp={} device={} connection_id={} error={:?}",
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
                        session_id, conn_id, e);
                }
                println!("[RELAY] Agent {} disconnected", session_id);
                println!("[RELAY][CLOSE] connection_type=agent device={} reason=remote_closed error={:?} peer_connected=false", session_id, e);
                break;
            }
        }
    }

    // Clean up from activeAgents on exit
    if let Ok(mut map) = hosts.lock() {
        if let Some((existing_id, _)) = map.get(&session_id) {
            if *existing_id == conn_id {
                map.remove(&session_id);
                println!(
                    "[TRACE99][REGISTRY] timestamp={} device={} connection_id={} event=HOST_UNREGISTERED",
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
                    session_id, conn_id
                );
            } else {
                println!(
                    "[TRACE99][REGISTRY] timestamp={} device={} exiting_connection_id={} current_connection_id={} event=STALE_HOST_EXIT_PRESERVED",
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),
                    session_id, conn_id, existing_id
                );
            }
        }
    }
    println!("[RELAY][AGENT] Agent connection state: OFFLINE for device={}", session_id);
}

// ============================================================
// NORMAL TCP VIEWER CONNECTION
// ============================================================

fn handle_viewer(mut viewer: TcpStream, hosts: ClientMap) {
    let mut type_byte = [0u8; 1];

    if !read_exact_logged(&mut viewer, &mut type_byte, "Viewer packet type") {
        return;
    }

    println!("[Relay] Viewer connected");

    if type_byte[0] != 2 {
        eprintln!("[Relay] Invalid viewer packet type: {}", type_byte[0]);
        let _ = viewer.write_all(&[3u8]);
        return;
    }

    let _ = viewer.set_read_timeout(Some(Duration::from_secs(5)));
    let (session_id, auth_hash) = match read_viewer_handshake(&mut viewer) {
        Ok(handshake) => handshake,
        Err(error) => {
            eprintln!("[AUTH][ERROR] reason=protocol_error stage=viewer_handshake error={}", error);
            let _ = viewer.write_all(&[3u8]);
            return;
        }
    };

    println!("[Relay] Target ID: {}", session_id);

    let host_tx = {
        let map = match hosts.lock() {
            Ok(map) => map,
            Err(_) => {
                let _ = viewer.write_all(&[3u8]);
                return;
            }
        };
        map.get(&session_id).map(|(_, tx)| tx.clone())
    };

    let host_tx = match host_tx {
        Some(tx) => tx,
        None => {
            println!(
                "[RELAY][AUTH] result=rejected reason=host_offline device_id={}",
                session_id
            );
            let _ = viewer.write_all(&[3u8]);
            return;
        }
    };

    let (resp_tx, resp_rx) = channel();
    let req = ViewerSessionRequest {
        auth_hash,
        response_tx: resp_tx,
        session: ViewerSession::Tcp(viewer),
    };

    if host_tx.send(req).is_err() {
        eprintln!("[PAIR][ERROR] Failed to forward request to host: {}", session_id);
        return;
    }

    let approved = resp_rx.recv().unwrap_or(false);
    if !approved {
        println!(
            "[RELAY][AUTH] result=rejected reason=invalid_token_or_auth_timeout device_id={}",
            session_id
        );
    }
}

// ============================================================
// WEBSOCKET VIEWER
// ============================================================

fn handle_websocket_viewer(stream: TcpStream, hosts: ClientMap) {
    println!("[RELAY][WS] Viewer connected");
    println!("[RELAY DEBUG] TCP/WebSocket connection accepted from {}", stream.peer_addr().unwrap());
    println!("[RELAY DEBUG] HTTP Upgrade request received");

    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => {
            println!("[RELAY DEBUG] WebSocket handshake success");
            ws
        }
        Err(e) => {
            eprintln!("[RELAY][ERROR] WS accept error: {:?}", e);
            println!("[RELAY DEBUG] WebSocket handshake failure: {:?}", e);
            return;
        }
    };

    let msg = match ws.read() {
        Ok(Message::Binary(data)) => data,
        Ok(other) => {
            eprintln!("[Relay] Unexpected WS handshake message: {:?}", other);
            return;
        }
        Err(e) => {
            eprintln!("[Relay] WS handshake read error: {:?}", e);
            return;
        }
    };

    let (session_id, auth_hash) = match parse_websocket_viewer_handshake(&msg) {
        Ok(handshake) => handshake,
        Err(error) => {
            eprintln!("[RELAY][WS][AUTH] result=rejected reason=protocol_error detail={}", error);
            let _ = ws.send(Message::Binary(vec![3u8]));
            return;
        }
    };

    println!("[DEBUG TRACE] [RELAY VIEWER HANDSHAKE] Payload length: {}, calculated session_id = '{}'", msg.len() - 1, session_id);
    println!("[WS] OPEN");
    println!("[AUTH] Authentication message received");
    println!("[AUTH] Target ID = {}", session_id);
    println!("[AUTH] Session request token present in relay handshake = NO");

    println!("[AUTH] Received viewer authentication");
    println!("[RELAY][WS] Viewer requested device = {}", session_id);
    println!("[RELAY][LOOKUP] device = {}", session_id);
    println!("[SYNC] VIEWER REQUESTED SYSTEM ID = {}", session_id);

    let mut host_tx_opt = None;
    for _ in 0..20 {
        {
            if let Ok(map) = hosts.lock() {
                let raw_clean = session_id.replace(' ', "");
                host_tx_opt = map.get(&session_id).map(|(_, tx)| tx.clone())
                    .or_else(|| map.get(&raw_clean).map(|(_, tx)| tx.clone()));
            }
        }
        if host_tx_opt.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let host_tx = match host_tx_opt {
        Some(tx) => tx,
        None => {
            println!("[RELAY][CLOSE] connection_type=viewer device={} reason=agent_not_found error=none peer_connected=false", session_id);
            let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                reason: "Agent not found".into(),
            }));
            return;
        }
    };

    let ws_arc = Arc::new(Mutex::new(ws));
    let (resp_tx, resp_rx) = channel();

    let req = ViewerSessionRequest {
        auth_hash,
        response_tx: resp_tx,
        session: ViewerSession::WebSocket(Arc::clone(&ws_arc)),
    };

    println!("[RELAY][AUTH] Viewer authentication received");

    if host_tx.send(req).is_err() {
        eprintln!("[PAIR][ERROR] Failed to forward request to host: {}", session_id);
        println!("[RELAY][CLOSE] connection_type=viewer device={} reason=agent_dropped error=send_failed peer_connected=false", session_id);
        if let Ok(mut lock) = ws_arc.lock() {
            let _ = lock.close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Error,
                reason: "Agent dropped".into(),
            }));
        }
        return;
    }

    println!("[PAIR] Waiting for HOST authentication response...");
    let approved = resp_rx.recv().unwrap_or(false);

    if !approved {
        println!("[RELAY][AUTH] Authentication rejected");
        println!("[RELAY][CLOSE] connection_type=viewer device={} reason=auth_rejected error=none peer_connected=true", session_id);
        if let Ok(mut lock) = ws_arc.lock() {
            let _ = lock.close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                reason: "Rejected".into(),
            }));
        }
        return;
    }

    println!("[RELAY][AUTH] Authentication accepted");
    println!("[RELAY][PAIR] Viewer paired with agent device={}", session_id);
    println!("[RELAY][STREAM] Entering persistent WebSocket bridge for device={}", session_id);

    println!("[PAIR] Pairing successful");
    println!("[STREAM] Bridge is running in the host session owner for device={}", session_id);
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    println!("========================================");
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("[RELAY] Received SIGINT/SIGTERM, initiating graceful shutdown...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");
    println!("       REMOTE DESKTOP RELAY SERVER");
    println!("  BUILD VERSION: 1.1.1 (Cloud-Ready Relay)");
    println!("========================================");
    start_backend_presence_worker();

    let port = env::var("RELAY_PORT")
        .or_else(|_| env::var("PORT"))
        .unwrap_or_else(|_| "9001".to_string());

    let relay_addr = format!("0.0.0.0:{}", port);

    println!("[RELAY] Starting relay server...");
    println!("[RELAY] Binding to {}...", relay_addr);

    let listener = match TcpListener::bind(&relay_addr) {
        Ok(l) => {
            println!("[RELAY] Relay listening on {}", relay_addr);
            println!("========================================");
            l
        }
        Err(e) => {
            eprintln!("[RELAY][FATAL] Failed to bind to {}: {:?}", relay_addr, e);
            return;
        }
    };

    let hosts: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    let _ = listener.set_nonblocking(true);
    while running.load(Ordering::SeqCst) {
        let (stream, _addr) = match listener.accept() {
            Ok(res) => res,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                eprintln!("[Relay] Incoming connection error: {:?}", e);
                continue;
            }
        };

        let peer = stream.peer_addr().map(|addr| addr.to_string()).unwrap_or_else(|_| "unknown".to_string());
        println!("[RELAY] Client connected: {}", peer);
        println!("[RELAY] Remote address: {}", peer);
        println!("[RELAY] Waiting for authentication...");

        let _ = stream.set_nodelay(true);
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let mut peek = [0u8; 14];
        let mut bytes_peeked = 0;
        let peek_start = Instant::now();
        while peek_start.elapsed() < Duration::from_secs(5) {
            match stream.peek(&mut peek) {
                Ok(0) => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(n) => {
                    bytes_peeked = n;
                    break;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    eprintln!("[Relay] Failed to inspect connection from {}: {:?}", peer, e);
                    let _ = stream.shutdown(Shutdown::Both);
                    break;
                }
            }
        }

        if bytes_peeked == 0 {
            let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            println!("[RELAY][HANDSHAKE] timestamp={} peer={} event=CLIENT_TIMEOUT_BEFORE_INIT", now_ms, peer);
            println!("[Relay] Client closed before sending an initial packet: {}", peer);
            let _ = stream.shutdown(Shutdown::Both);
            continue;
        }

        let connection_type = match initial_packet_type(bytes_peeked, &peek) {
            None => {
                println!("[Relay] Client closed before sending an initial packet: {}", peer);
                let _ = stream.shutdown(Shutdown::Both);
                continue;
            }
            Some(connection_type) => connection_type,
        };

        if is_http_get_prefix(bytes_peeked, &peek) {
            match is_health_check(&stream) {
                Ok(true) => {
                    println!("[RELAY][HEALTH] peer={} result=ok", peer);
                    let mut stream = stream;
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                Ok(false) => {
                    println!(
                        "[RELAY][WS] event=VIEWER_WEBSOCKET_ACCEPTED peer={}",
                        peer
                    );
                    let hosts_clone = Arc::clone(&hosts);
                    thread::spawn(move || handle_websocket_viewer(stream, hosts_clone));
                    continue;
                }
                Err(error) => {
                    eprintln!(
                        "[RELAY][HEALTH] peer={} result=probe_error error={}",
                        peer, error
                    );
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
            }
        }

        let hosts_clone = Arc::clone(&hosts);

        match connection_type {
            99 => {
                let is_local = stream.peer_addr().map(|addr| addr.ip().is_loopback()).unwrap_or(false);
                if is_local {
                    println!("[Relay] Received local shutdown command. Exiting...");
                    running.store(false, Ordering::SeqCst);
                } else {
                    println!("[Relay] Rejected remote shutdown command from {}", peer);
                }
                let _ = stream.shutdown(Shutdown::Both);
            }

            1 => {
                let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                println!("[RELAY][HANDSHAKE] timestamp={} peer={} role=HOST initial_type=1 bytes_peeked={}", now_ms, peer, bytes_peeked);
                println!("[Relay] Connection identified as HOST");
                thread::spawn(move || handle_host(stream, hosts_clone));
            }

            2 => {
                let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                println!("[RELAY][HANDSHAKE] timestamp={} peer={} role=TCP_VIEWER initial_type=2 bytes_peeked={}", now_ms, peer, bytes_peeked);
                println!("[Relay] Connection identified as TCP VIEWER");
                thread::spawn(move || handle_viewer(stream, hosts_clone));
            }

            71 => {
                let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                println!("[RELAY][HANDSHAKE] timestamp={} peer={} role=WEBSOCKET initial_type=71 bytes_peeked={}", now_ms, peer, bytes_peeked);
                println!("[Relay] Connection identified as WEBSOCKET");
                thread::spawn(move || handle_websocket_viewer(stream, hosts_clone));
            }

            _ => {
                let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                eprintln!(
                    "[RELAY][HANDSHAKE] timestamp={} peer={} error=UNKNOWN_CONNECTION_TYPE type={} initial_byte=0x{:02x} bytes_peeked={}",
                    now_ms, peer, connection_type, peek[0], bytes_peeked
                );
                eprintln!("[Relay] Unknown initial connection type: {}", connection_type);
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Read};
    use std::sync::mpsc;

    struct ChunkedReader {
        bytes: io::Cursor<Vec<u8>>,
        max_chunk: usize,
    }

    impl Read for ChunkedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let limit = buffer.len().min(self.max_chunk);
            self.bytes.read(&mut buffer[..limit])
        }
    }

    fn video_packet(payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![13];
        packet.extend_from_slice(&1920u32.to_be_bytes());
        packet.extend_from_slice(&1080u32.to_be_bytes());
        packet.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        packet.extend_from_slice(&123456789u64.to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    fn mixed_stream(video_a: &[u8], chat: &[u8], video_b: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
        let video_a = video_packet(video_a);
        let video_b = video_packet(video_b);
        let mut chat_packet = vec![16, 0];
        chat_packet.extend_from_slice(&(chat.len() as u16).to_be_bytes());
        chat_packet.extend_from_slice(chat);
        let stream = [video_a.clone(), chat_packet.clone(), video_b.clone()].concat();
        (stream, vec![video_a, chat_packet, video_b])
    }

    fn start_mock_backend(
        status: &str,
        body: &str,
        delay: Duration,
    ) -> (Url, thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let base_url = Url::parse(&format!("http://{address}/DeskStream/api/")).unwrap();
        let status = status.to_string();
        let body = body.to_string();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                let header_end = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n");
                if let Some(header_end) = header_end {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length {
                        break;
                    }
                }
            }

            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            request
        });

        (base_url, server)
    }

    fn parse_video_chat_sequence<R: Read>(reader: &mut R) -> io::Result<Vec<Vec<u8>>> {
        let mut packets = Vec::new();
        loop {
            let mut packet_type = [0u8; 1];
            match reader.read_exact(&mut packet_type) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(error) => return Err(error),
            }
            let packet = match packet_type[0] {
                13 | 15 => read_host_video_packet(reader, packet_type[0])?,
                16 => read_host_chat_packet(reader)?,
                unexpected => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unexpected packet type {unexpected}"),
                    ));
                }
            };
            packets.push(packet);
        }
        Ok(packets)
    }

    #[test]
    fn eof_before_initial_packet_is_not_classified_as_type_zero() {
        assert_eq!(initial_packet_type(0, &[0]), None);
    }

    #[test]
    fn actual_zero_initial_byte_remains_an_invalid_packet_type() {
        assert_eq!(initial_packet_type(1, &[0]), Some(0));
    }

    #[test]
    fn device_ids_are_fixed_width_ascii_digits() {
        assert!(valid_device_id("705004256"));
        assert!(!valid_device_id("70500425"));
        assert!(!valid_device_id("7050042560"));
        assert!(!valid_device_id("70500A256"));
    }

    #[test]
    fn viewer_pin_requires_a_nonempty_matching_sha256() {
        let valid_hash: [u8; 32] = Sha256::digest(b"deskstream-pin").into();
        let invalid_hash: [u8; 32] = Sha256::digest(b"other-pin").into();

        assert!(verify_viewer_pin("deskstream-pin", &valid_hash));
        assert!(!verify_viewer_pin("other-pin", &valid_hash));
        assert!(!verify_viewer_pin("", &valid_hash));
        assert!(!verify_viewer_pin("deskstream-pin", &invalid_hash));
    }

    #[test]
    fn backend_url_uses_scheme_defaults_and_optional_host_port_overrides() {
        for backend_url in [
            "https://friendssoftwaresolutions.in/DeskStream/api",
            "https://friendssoftwaresolutions.in/DeskStream/api/",
            "https://friendssoftwaresolutions.in/DeskStream/api///",
        ] {
            let url = resolve_backend_url(backend_url, None, None).unwrap();
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some("friendssoftwaresolutions.in"));
            assert_eq!(url.port_or_known_default(), Some(443));
            assert_eq!(
                heartbeat_endpoint(&url).unwrap().path(),
                "/DeskStream/api/devices/heartbeat.php"
            );
        }

        let overridden =
            resolve_backend_url("http://localhost/api", Some("127.0.0.1"), Some("8080"))
                .unwrap();
        assert_eq!(overridden.host_str(), Some("127.0.0.1"));
        assert_eq!(overridden.port(), Some(8080));
    }

    #[test]
    fn backend_url_rejects_invalid_ports_and_embedded_credentials() {
        assert!(resolve_backend_url("https://example.com/api", None, Some("not-a-port")).is_err());
        assert!(resolve_backend_url("https://user:secret@example.com/api", None, None).is_err());
        assert!(resolve_backend_url("ftp://example.com/api", None, None).is_err());
    }

    #[test]
    fn backend_presence_requires_http_success_and_updated_device() {
        let ok = reqwest::StatusCode::OK;
        assert!(validate_presence_response(
            ok,
            serde_json::json!({"status": "success", "updated": true})
        )
        .is_ok());
        assert!(validate_presence_response(
            ok,
            serde_json::json!({"status": "success", "updated": false})
        )
        .is_err());
        assert!(validate_presence_response(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::Value::Null
        )
        .unwrap_err()
        .contains("HTTP 500"));
        assert!(validate_presence_response(ok, serde_json::json!({"status": "error"})).is_err());
    }

    #[test]
    fn heartbeat_posts_the_device_id_and_accepts_updated_response() {
        let (base_url, server) =
            start_mock_backend("200 OK", r#"{"status":"success","updated":true}"#, Duration::ZERO);
        let client = Client::builder().timeout(Duration::from_secs(2)).build().unwrap();

        send_backend_presence(&client, &base_url, "705004256").unwrap();
        let request = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(request.starts_with(
            "POST /DeskStream/api/devices/heartbeat.php HTTP/1.1\r\n"
        ));
        let body = request.split_once("\r\n\r\n").unwrap().1;
        let payload: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["system_id"], "705004256");
    }

    #[test]
    fn heartbeat_reports_http_and_json_errors() {
        let (base_url, server) = start_mock_backend(
            "503 Service Unavailable",
            r#"{"status":"error"}"#,
            Duration::ZERO,
        );
        let client = Client::builder().timeout(Duration::from_secs(2)).build().unwrap();
        assert!(send_backend_presence(&client, &base_url, "705004256")
            .unwrap_err()
            .contains("HTTP 503"));
        server.join().unwrap();

        let (base_url, server) = start_mock_backend("200 OK", "not-json", Duration::ZERO);
        assert!(send_backend_presence(&client, &base_url, "705004256")
            .unwrap_err()
            .contains("malformed_json_omitted"));
        server.join().unwrap();
    }

    #[test]
    fn heartbeat_timeout_and_unreachable_backend_are_errors() {
        let (base_url, server) = start_mock_backend(
            "200 OK",
            r#"{"status":"success","updated":true}"#,
            Duration::from_millis(250),
        );
        let short_timeout = Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        assert!(send_backend_presence(&short_timeout, &base_url, "705004256")
            .unwrap_err()
            .contains("request failed"));
        server.join().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let unavailable = Url::parse(&format!("http://{address}/DeskStream/api/")).unwrap();
        assert!(send_backend_presence(&short_timeout, &unavailable, "705004256")
            .unwrap_err()
            .contains("request failed"));
    }

    #[test]
    fn tcp_viewer_handshake_reads_fixed_fields_across_fragmented_input() {
        let auth_hash = [0xA5; VIEWER_AUTH_HASH_LENGTH];
        let mut handshake = b"705004256".to_vec();
        handshake.extend_from_slice(&auth_hash);
        handshake.extend_from_slice(b"next-packet");
        let mut reader = ChunkedReader {
            bytes: io::Cursor::new(handshake),
            max_chunk: 1,
        };

        let (session_id, received_hash) = read_viewer_handshake(&mut reader).unwrap();
        assert_eq!(session_id, "705004256");
        assert_eq!(received_hash, auth_hash);
        let mut remaining = Vec::new();
        reader.read_to_end(&mut remaining).unwrap();
        assert_eq!(remaining, b"next-packet");
    }

    #[test]
    fn tcp_viewer_handshake_rejects_incomplete_or_invalid_fields() {
        let mut incomplete = io::Cursor::new(b"705004256short".to_vec());
        assert_eq!(
            read_viewer_handshake(&mut incomplete).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );

        let mut invalid = b"70500A256".to_vec();
        invalid.extend_from_slice(&[0; VIEWER_AUTH_HASH_LENGTH]);
        assert_eq!(
            read_viewer_handshake(&mut io::Cursor::new(invalid))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn websocket_viewer_handshake_requires_exact_protocol_fields() {
        let mut packet = vec![2];
        packet.extend_from_slice(b"705004256");
        packet.extend_from_slice(&[0xA5; VIEWER_AUTH_HASH_LENGTH]);

        let (session_id, auth_hash) = parse_websocket_viewer_handshake(&packet).unwrap();
        assert_eq!(session_id, "705004256");
        assert_eq!(auth_hash, [0xA5; VIEWER_AUTH_HASH_LENGTH]);

        assert!(parse_websocket_viewer_handshake(&packet[1..]).is_err());
        let mut invalid_id = packet.clone();
        invalid_id[4] = b'x';
        assert!(parse_websocket_viewer_handshake(&invalid_id).is_err());
        let mut invalid_type = packet.clone();
        invalid_type[0] = 3;
        assert!(parse_websocket_viewer_handshake(&invalid_type).is_err());
        assert!(parse_websocket_viewer_handshake(&packet[..packet.len() - 1]).is_err());
        let mut trailing = packet;
        trailing.push(0);
        assert!(parse_websocket_viewer_handshake(&trailing).is_err());
    }

    #[test]
    fn browser_websocket_upgrade_is_detected_from_http_get() {
        let request = b"GET / HTTP/1.1\r\nUpgrade: websocket\r\n";
        assert!(is_http_get_prefix(request.len(), request));
        assert!(is_http_get_prefix(1, b"G"));
        assert!(is_http_get_prefix(3, b"GET"));
        assert!(!is_http_get_prefix(4, b"POST"));
        assert!(!is_http_get_prefix(0, b""));
    }

    #[test]
    fn host_initial_packet_type_is_preserved() {
        assert_eq!(initial_packet_type(1, &[1]), Some(1));
    }

    #[test]
    fn video_chat_video_packets_preserve_order_with_coalesced_and_partial_reads() {
        let (stream, expected) = mixed_stream(b"frame-a", "hello from B".as_bytes(), b"frame-b");

        let mut coalesced = io::Cursor::new(stream.clone());
        assert_eq!(parse_video_chat_sequence(&mut coalesced).unwrap(), expected);

        let mut fragmented = ChunkedReader {
            bytes: io::Cursor::new(stream),
            max_chunk: 2,
        };
        assert_eq!(parse_video_chat_sequence(&mut fragmented).unwrap(), expected);
    }

    #[test]
    fn chat_packet_followed_by_video_consumes_exactly_chat_frame() {
        let (stream, expected) = mixed_stream(b"", "chat".as_bytes(), b"next");
        let mut reader = ChunkedReader {
            bytes: io::Cursor::new(stream),
            max_chunk: 1,
        };
        assert_eq!(parse_video_chat_sequence(&mut reader).unwrap(), expected);
    }

    #[test]
    fn control_burst_is_bounded_so_video_can_be_serviced() {
        let (control_tx, control_rx) = mpsc::channel();
        let (video_tx, video_rx) = mpsc::channel();
        control_tx.send(vec![1; CONTROL_BYTES_PER_TURN]).unwrap();
        control_tx.send(vec![2; CONTROL_BYTES_PER_TURN]).unwrap();
        video_tx.send(vec![99]).unwrap();

        let controls = take_control_burst(&control_rx);
        assert_eq!(controls.len(), 1);
        assert_eq!(video_rx.try_recv().unwrap(), vec![99]);
        assert_eq!(control_rx.try_recv().unwrap(), vec![2; CONTROL_BYTES_PER_TURN]);
    }

    #[test]
    fn chat_typing_packets_forward_only_start_or_stop_state() {
        assert_eq!(chat_typing_packet(1), Some(vec![18, 1]));
        assert_eq!(chat_typing_packet(0), Some(vec![18, 0]));
        assert_eq!(chat_typing_packet(2), None);
    }

    #[test]
    fn reverse_ack_accepts_only_explicit_decisions() {
        assert_eq!(reverse_ack_packet(0), Some(vec![31, 0]));
        assert_eq!(reverse_ack_packet(1), Some(vec![31, 1]));
        assert_eq!(reverse_ack_packet(2), None);
    }

    #[test]
    fn reverse_request_keeps_the_existing_one_byte_type_30_frame() {
        assert_eq!(reverse_request_packet(), vec![30]);
    }

    fn mock_backend(
        status: &'static str,
        body: &'static str,
        delay: Duration,
    ) -> (Url, thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let bytes_read = stream.read(&mut buffer).unwrap();
                if bytes_read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..bytes_read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower_line = line.to_ascii_lowercase();
                        lower_line
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            thread::sleep(delay);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            request
        });
        (
            Url::parse(&format!("http://{address}/DeskStream/api/")).unwrap(),
            server,
        )
    }

    fn test_http_client(timeout: Duration) -> Client {
        Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .unwrap()
    }

    #[test]
    fn backend_heartbeat_sends_expected_post_and_accepts_updated_response() {
        let (url, server) = mock_backend(
            "200 OK",
            r#"{"status":"success","updated":true}"#,
            Duration::ZERO,
        );
        let client = test_http_client(Duration::from_secs(2));

        send_backend_presence(&client, &url, "705004256").unwrap();

        let request = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(request.starts_with("POST /DeskStream/api/devices/heartbeat.php HTTP/1.1\r\n"));
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(headers.lines().any(|line| {
            line.split_once(':')
                .map(|(name, value)| {
                    name.eq_ignore_ascii_case("content-type")
                        && value.trim().eq_ignore_ascii_case("application/json")
                })
                .unwrap_or(false)
        }));
        assert!(!headers
            .lines()
            .any(|line| line.to_ascii_lowercase().starts_with("authorization:")));
        let payload: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload, serde_json::json!({"system_id": "705004256"}));
    }

    #[test]
    fn backend_heartbeat_reports_and_redacts_http_403_responses() {
        let (url, server) = mock_backend(
            "403 Forbidden",
            r#"{"status":"error","message":"Forbidden","token":"secret-token","password":"secret-password"}"#,
            Duration::ZERO,
        );
        let error = send_backend_presence(
            &test_http_client(Duration::from_secs(2)),
            &url,
            "705004256",
        )
        .unwrap_err();

        assert!(error.contains("method=POST HTTP 403"));
        assert!(error.contains(r#"sanitized_body={"status":"error","message":"forbidden"}"#));
        assert!(!error.contains("secret-token"));
        assert!(!error.contains("secret-password"));
        let request = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(request.starts_with("POST /DeskStream/api/devices/heartbeat.php HTTP/1.1\r\n"));
    }

    #[test]
    fn backend_heartbeat_reports_http_malformed_and_unupdated_responses() {
        let (url, server) = mock_backend("503 Service Unavailable", "unavailable", Duration::ZERO);
        assert!(send_backend_presence(&test_http_client(Duration::from_secs(2)), &url, "705004256")
            .unwrap_err()
            .contains("HTTP 503"));
        let _ = server.join().unwrap();

        let (url, server) = mock_backend("200 OK", "not-json", Duration::ZERO);
        assert!(send_backend_presence(&test_http_client(Duration::from_secs(2)), &url, "705004256")
            .unwrap_err()
            .contains("malformed_json_omitted"));
        let _ = server.join().unwrap();

        let (url, server) = mock_backend(
            "200 OK",
            r#"{"status":"success","updated":false}"#,
            Duration::ZERO,
        );
        assert!(send_backend_presence(&test_http_client(Duration::from_secs(2)), &url, "705004256")
            .unwrap_err()
            .contains("did not update"));
        let _ = server.join().unwrap();
    }

    #[test]
    fn backend_error_logging_omits_unrecognized_response_details() {
        assert_eq!(
            sanitized_backend_response_body(
                r#"{"status":"error","message":"Forbidden","token":"secret"}"#
            ),
            r#"{"status":"error","message":"forbidden"}"#
        );
        assert_eq!(
            sanitized_backend_response_body(
                r#"{"status":"error","message":"SQL error for device 542157504"}"#
            ),
            r#"{"status":"error","message":"unrecognized_message_omitted"}"#
        );
        assert_eq!(
            sanitized_backend_response_body("<html>Cloudflare request denied</html>"),
            "non_json_or_unrecognized_body"
        );
    }

    #[test]
    fn backend_heartbeat_timeout_is_reported() {
        let (url, server) = mock_backend(
            "200 OK",
            r#"{"status":"success","updated":true}"#,
            Duration::from_millis(200),
        );
        let result = send_backend_presence(&test_http_client(Duration::from_millis(50)), &url, "705004256");
        assert!(result.unwrap_err().contains("request failed"));
        let _ = server.join().unwrap();
    }

    #[test]
    fn backend_heartbeat_unreachable_host_is_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let url = Url::parse(&format!("http://{address}/api/")).unwrap();
        let result = send_backend_presence(
            &test_http_client(Duration::from_secs(1)),
            &url,
            "705004256",
        );
        assert!(result.unwrap_err().contains("request failed"));
    }

    fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (server, client)
    }

    #[test]
    fn unexpected_bridge_worker_exit_clears_owner_visible_active_state() {
        let tcp_state = Arc::new(TcpBridgeState {
            active: AtomicBool::new(true),
            viewer_disconnected: AtomicBool::new(false),
            host_failed: AtomicBool::new(false),
        });
        let tcp_worker_state = Arc::clone(&tcp_state);
        let tcp_worker = thread::spawn(move || {
            let _active_guard = TcpBridgeWorkerGuard(tcp_worker_state);
            panic!("simulated TCP bridge worker panic");
        });
        assert!(tcp_worker.join().is_err());
        assert!(!tcp_state.active.load(Ordering::SeqCst));

        let websocket_active = Arc::new(AtomicBool::new(true));
        let websocket_worker_active = Arc::clone(&websocket_active);
        let websocket_worker = thread::spawn(move || {
            let _active_guard = WebSocketBridgeWorkerGuard(websocket_worker_active);
            panic!("simulated WebSocket bridge worker panic");
        });
        assert!(websocket_worker.join().is_err());
        assert!(!websocket_active.load(Ordering::SeqCst));
    }

    #[test]
    fn repeated_tcp_cleanup_is_idempotent_and_notifies_host_once() {
        let (relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let host_writer = relay_host.try_clone().unwrap();
        let (relay_viewer, viewer_peer) = tcp_pair();
        let viewer_reader = relay_viewer.try_clone().unwrap();
        let state = Arc::new(TcpBridgeState {
            active: AtomicBool::new(true),
            viewer_disconnected: AtomicBool::new(false),
            host_failed: AtomicBool::new(false),
        });
        mark_viewer_disconnected(&state, &relay_viewer);
        mark_viewer_disconnected(&state, &relay_viewer);
        mark_viewer_disconnected(&state, &relay_viewer);

        let forwarder = thread::spawn(move || {
            viewer_to_host_tcp(viewer_reader, host_writer, state.clone());
            state
        });
        let state = forwarder.join().unwrap();
        assert!(!state.active.load(Ordering::SeqCst));
        assert!(state.viewer_disconnected.load(Ordering::SeqCst));
        assert!(!state.host_failed.load(Ordering::SeqCst));

        let mut notification = [0u8; 1];
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        host_peer
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        assert!(matches!(
            host_peer.read(&mut notification),
            Err(ref error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut
        ));
        drop(viewer_peer);
    }

    fn start_websocket_bridge(host: &TcpStream) -> (String, thread::JoinHandle<bool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut host = host.try_clone().unwrap();
        let bridge = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let websocket = tungstenite::accept(stream).unwrap();
            run_websocket_bridge(
                "test-session",
                &mut host,
                Arc::new(Mutex::new(websocket)),
            )
        });
        (format!("ws://{address}"), bridge)
    }

    fn connect_websocket_bridge(
        address: &str,
    ) -> tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>> {
        let (mut websocket, _) = tungstenite::connect(address).unwrap();
        for expected in [vec![1u8], vec![2u8]] {
            match websocket.read().unwrap() {
                Message::Binary(data) => assert_eq!(data, expected),
                message => panic!("unexpected WebSocket bridge startup message: {message:?}"),
            }
        }
        websocket
    }

    #[test]
    fn websocket_viewer_disconnect_notifies_host_once_and_preserves_host_stream() {
        let (relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (address, bridge) = start_websocket_bridge(&relay_host);
        let mut websocket = connect_websocket_bridge(&address);
        websocket.close(None).unwrap();

        let mut notification = [0u8; 1];
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(bridge.join().unwrap());
        host_peer
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        assert!(matches!(
            host_peer.read(&mut notification),
            Err(ref error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut
        ));
        host_peer.write_all(&[42]).unwrap();
    }

    #[test]
    fn websocket_session_can_restart_immediately_after_viewer_disconnect() {
        let (relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let (first_address, first_bridge) = start_websocket_bridge(&relay_host);
        let mut first_viewer = connect_websocket_bridge(&first_address);
        first_viewer.close(None).unwrap();

        let mut notification = [0u8; 1];
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(first_bridge.join().unwrap());

        let (second_address, second_bridge) = start_websocket_bridge(&relay_host);
        let mut second_viewer = connect_websocket_bridge(&second_address);
        let mut host_message = vec![26u8];
        host_message.extend_from_slice(&123u64.to_be_bytes());
        host_peer.write_all(&host_message).unwrap();
        loop {
            match second_viewer.read().unwrap() {
                Message::Binary(message) => {
                    assert_eq!(message, host_message);
                    break;
                }
                Message::Ping(data) => second_viewer.send(Message::Pong(data)).unwrap(),
                Message::Pong(_) => {}
                message => panic!("unexpected WebSocket message: {message:?}"),
            }
        }

        let mut viewer_message = vec![26u8];
        viewer_message.extend_from_slice(&456u64.to_be_bytes());
        second_viewer
            .send(Message::Binary(viewer_message.clone()))
            .unwrap();
        let mut received = vec![0u8; viewer_message.len()];
        host_peer.read_exact(&mut received).unwrap();
        assert_eq!(received, viewer_message);

        second_viewer.close(None).unwrap();
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(second_bridge.join().unwrap());
    }

    #[test]
    fn simultaneous_websocket_viewer_and_host_disconnects_do_not_echo_type99() {
        for _ in 0..20 {
            let (relay_host, mut host_peer) = tcp_pair();
            host_peer
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let (address, bridge) = start_websocket_bridge(&relay_host);
            let mut websocket = connect_websocket_bridge(&address);

            host_peer.write_all(&[99]).unwrap();
            websocket.close(None).unwrap();
            assert!(bridge.join().unwrap());

            host_peer
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            let mut unexpected = [0u8; 1];
            assert!(matches!(
                host_peer.read(&mut unexpected),
                Err(ref error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::TimedOut
            ));
        }
    }

    #[test]
    fn websocket_host_disconnect_does_not_echo_type99() {
        let (relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let (address, bridge) = start_websocket_bridge(&relay_host);
        let _websocket = connect_websocket_bridge(&address);

        host_peer.write_all(&[99]).unwrap();
        assert!(bridge.join().unwrap());
        let mut unexpected = [0u8; 1];
        assert!(matches!(
            host_peer.read(&mut unexpected),
            Err(ref error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut
        ));
        host_peer.write_all(&[42]).unwrap();
    }

    #[test]
    fn websocket_bridge_forwards_file_transfer_packets_in_both_directions() {
        let (relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (address, bridge) = start_websocket_bridge(&relay_host);
        let mut websocket = connect_websocket_bridge(&address);
        let transfer_id = 123456789u64;
        let filename = b"test.bin";

        let mut offer = vec![20];
        offer.extend_from_slice(&transfer_id.to_be_bytes());
        offer.extend_from_slice(&4u64.to_be_bytes());
        offer.extend_from_slice(&(filename.len() as u16).to_be_bytes());
        offer.extend_from_slice(filename);
        websocket.send(Message::Binary(offer.clone())).unwrap();
        let mut received = vec![0; offer.len()];
        host_peer.read_exact(&mut received).unwrap();
        assert_eq!(received, offer);

        let payload = b"data";
        let mut chunk = vec![21];
        chunk.extend_from_slice(&transfer_id.to_be_bytes());
        chunk.extend_from_slice(&0u32.to_be_bytes());
        chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        chunk.extend_from_slice(payload);
        websocket.send(Message::Binary(chunk.clone())).unwrap();
        let mut received = vec![0; chunk.len()];
        host_peer.read_exact(&mut received).unwrap();
        assert_eq!(received, chunk);

        let mut completion = vec![22];
        completion.extend_from_slice(&transfer_id.to_be_bytes());
        completion.extend_from_slice(&4u64.to_be_bytes());
        completion.extend_from_slice(&[0xab; 32]);
        websocket
            .send(Message::Binary(completion.clone()))
            .unwrap();
        let mut received = vec![0; completion.len()];
        host_peer.read_exact(&mut received).unwrap();
        assert_eq!(received, completion);

        let mut approval = vec![26];
        approval.extend_from_slice(&transfer_id.to_be_bytes());
        host_peer.write_all(&approval).unwrap();
        assert_eq!(
            websocket.read().unwrap(),
            Message::Binary(approval.clone())
        );

        let mut completion_ack = vec![27];
        completion_ack.extend_from_slice(&transfer_id.to_be_bytes());
        host_peer.write_all(&completion_ack).unwrap();
        assert_eq!(
            websocket.read().unwrap(),
            Message::Binary(completion_ack)
        );

        let reverse_transfer_id = 987654321u64;
        let mut reverse_offer = vec![20];
        reverse_offer.extend_from_slice(&reverse_transfer_id.to_be_bytes());
        reverse_offer.extend_from_slice(&4u64.to_be_bytes());
        reverse_offer.extend_from_slice(&(filename.len() as u16).to_be_bytes());
        reverse_offer.extend_from_slice(filename);
        host_peer.write_all(&reverse_offer).unwrap();
        assert_eq!(
            websocket.read().unwrap(),
            Message::Binary(reverse_offer.clone())
        );

        let mut reverse_chunk = vec![21];
        reverse_chunk.extend_from_slice(&reverse_transfer_id.to_be_bytes());
        reverse_chunk.extend_from_slice(&0u32.to_be_bytes());
        reverse_chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        reverse_chunk.extend_from_slice(payload);
        host_peer.write_all(&reverse_chunk).unwrap();
        assert_eq!(
            websocket.read().unwrap(),
            Message::Binary(reverse_chunk)
        );

        let mut reverse_completion = vec![22];
        reverse_completion.extend_from_slice(&reverse_transfer_id.to_be_bytes());
        reverse_completion.extend_from_slice(&4u64.to_be_bytes());
        reverse_completion.extend_from_slice(&[0xcd; 32]);
        host_peer.write_all(&reverse_completion).unwrap();
        assert_eq!(
            websocket.read().unwrap(),
            Message::Binary(reverse_completion)
        );

        let mut reverse_approval = vec![26];
        reverse_approval.extend_from_slice(&reverse_transfer_id.to_be_bytes());
        websocket
            .send(Message::Binary(reverse_approval.clone()))
            .unwrap();
        let mut received = vec![0; reverse_approval.len()];
        host_peer.read_exact(&mut received).unwrap();
        assert_eq!(received, reverse_approval);

        let mut reverse_completion_ack = vec![27];
        reverse_completion_ack.extend_from_slice(&reverse_transfer_id.to_be_bytes());
        websocket
            .send(Message::Binary(reverse_completion_ack.clone()))
            .unwrap();
        let mut received = vec![0; reverse_completion_ack.len()];
        host_peer.read_exact(&mut received).unwrap();
        assert_eq!(received, reverse_completion_ack);

        websocket.close(None).unwrap();
        let mut disconnect = [0u8; 1];
        host_peer.read_exact(&mut disconnect).unwrap();
        assert_eq!(disconnect, [99]);
        assert!(bridge.join().unwrap());
    }

    #[test]
    fn host_ended_session_does_not_echo_type99_and_keeps_stream_usable() {
        let (mut relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();

        notify_host_viewer_disconnect("test-session", &mut relay_host, true);
        let mut unexpected = [0u8; 1];
        let read_result = host_peer.read(&mut unexpected);
        assert!(matches!(
            read_result,
            Err(ref error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut
        ));

        host_peer.write_all(&[42u8]).unwrap();
        relay_host.read_exact(&mut unexpected).unwrap();
        assert_eq!(unexpected, [42]);
    }

    #[test]
    fn viewer_ended_session_sends_exactly_one_type99() {
        let (mut relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();

        notify_host_viewer_disconnect("test-session", &mut relay_host, false);
        let mut notification = [0u8; 1];
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        let read_result = host_peer.read(&mut notification);
        assert!(matches!(
            read_result,
            Err(ref error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut
        ));
    }

    #[test]
    fn tcp_viewer_disconnect_notifies_host_once_and_allows_next_session() {
        let (relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let (relay_viewer, viewer_peer) = tcp_pair();
        let first_host = relay_host.try_clone().unwrap();
        let first_session = thread::spawn(move || bridge_tcp_session(&first_host, relay_viewer));
        drop(viewer_peer);

        let mut notification = [0u8; 1];
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(!first_session.join().unwrap());
        host_peer
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut duplicate = [0u8; 1];
        assert!(matches!(
            host_peer.read(&mut duplicate),
            Err(ref error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut
        ));

        host_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (relay_viewer, mut viewer_peer) = tcp_pair();
        let second_host = relay_host.try_clone().unwrap();
        let second_session = thread::spawn(move || bridge_tcp_session(&second_host, relay_viewer));
        let file_chunk = b"file-chunk-during-close";
        viewer_peer.write_all(file_chunk).unwrap();
        drop(viewer_peer);
        let mut received = vec![0u8; file_chunk.len()];
        host_peer.read_exact(&mut received).unwrap();
        assert_eq!(&received, file_chunk);
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(!second_session.join().unwrap());
    }

    #[test]
    fn tcp_host_disconnect_closes_viewer_without_host_reuse() {
        let (relay_host, host_peer) = tcp_pair();
        let (relay_viewer, mut viewer_peer) = tcp_pair();
        viewer_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let session = thread::spawn(move || bridge_tcp_session(&relay_host, relay_viewer));

        drop(host_peer);
        let mut buffer = [0u8; 1];
        let viewer_read = viewer_peer.read(&mut buffer);
        assert!(match viewer_read {
            Ok(0) => true,
            Err(error) => {
                error.kind() == std::io::ErrorKind::ConnectionReset
                    || error.kind() == std::io::ErrorKind::ConnectionAborted
            }
            Ok(_) => false,
        });
        assert!(session.join().unwrap());
    }

    #[test]
    fn host_write_failure_ends_tcp_session_and_closes_viewer() {
        let (relay_host, host_peer) = tcp_pair();
        let (relay_viewer, mut viewer_peer) = tcp_pair();
        viewer_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        viewer_peer
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let session = thread::spawn(move || bridge_tcp_session(&relay_host, relay_viewer));

        host_peer.shutdown(Shutdown::Both).unwrap();
        let payload = vec![7u8; 64 * 1024];
        let _ = viewer_peer.write_all(&payload);
        assert!(session.join().unwrap());
        let mut byte = [0u8; 1];
        assert_eq!(viewer_peer.read(&mut byte).unwrap_or(0), 0);
    }

    #[test]
    fn viewer_write_failure_notifies_host_and_keeps_session_owner_usable() {
        let (relay_host, mut host_peer) = tcp_pair();
        host_peer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        host_peer
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (relay_viewer, viewer_peer) = tcp_pair();
        viewer_peer.shutdown(Shutdown::Read).unwrap();
        let session = thread::spawn(move || bridge_tcp_session(&relay_host, relay_viewer));

        let payload = vec![9u8; 64 * 1024];
        let _ = host_peer.write_all(&payload);
        let mut notification = [0u8; 1];
        host_peer.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(!session.join().unwrap());
    }

    #[test]
    fn simultaneous_tcp_disconnects_terminate_without_panicking() {
        let (relay_host, host_peer) = tcp_pair();
        let (relay_viewer, viewer_peer) = tcp_pair();
        let session = thread::spawn(move || bridge_tcp_session(&relay_host, relay_viewer));

        let host_close = thread::spawn(move || drop(host_peer));
        let viewer_close = thread::spawn(move || drop(viewer_peer));
        host_close.join().unwrap();
        viewer_close.join().unwrap();
        let _host_failed = session.join().unwrap();
    }

    #[test]
    fn closing_one_tcp_session_does_not_interrupt_another_session() {
        let (host_one, mut host_peer_one) = tcp_pair();
        let (viewer_one, mut viewer_peer_one) = tcp_pair();
        host_peer_one
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let session_one = thread::spawn(move || bridge_tcp_session(&host_one, viewer_one));

        let (host_two, mut host_peer_two) = tcp_pair();
        let (viewer_two, mut viewer_peer_two) = tcp_pair();
        host_peer_two
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let session_two = thread::spawn(move || bridge_tcp_session(&host_two, viewer_two));

        viewer_peer_one.write_all(b"session-one").unwrap();
        viewer_peer_two.write_all(b"session-two").unwrap();
        let mut received_one = [0u8; 11];
        let mut received_two = [0u8; 11];
        host_peer_one.read_exact(&mut received_one).unwrap();
        host_peer_two.read_exact(&mut received_two).unwrap();
        assert_eq!(&received_one, b"session-one");
        assert_eq!(&received_two, b"session-two");

        drop(viewer_peer_one);
        let mut notification = [0u8; 1];
        host_peer_one.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(!session_one.join().unwrap());

        viewer_peer_two.write_all(b"still-alive").unwrap();
        let mut received_after_close = [0u8; 11];
        host_peer_two.read_exact(&mut received_after_close).unwrap();
        assert_eq!(&received_after_close, b"still-alive");
        drop(viewer_peer_two);
        host_peer_two.read_exact(&mut notification).unwrap();
        assert_eq!(notification, [99]);
        assert!(!session_two.join().unwrap());
    }
}