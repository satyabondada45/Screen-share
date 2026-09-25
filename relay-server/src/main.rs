use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tungstenite::Message;

enum ViewerSession {
    WebSocket(Arc<Mutex<tungstenite::WebSocket<TcpStream>>>),
    Tcp(TcpStream),
}

macro_rules! relay_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            use std::io::Write;
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
        relay_log!($($arg)*);
    };
}

struct ViewerSessionRequest {
    auth_hash: [u8; 32],
    response_tx: Sender<bool>,
    session: ViewerSession,
}

type ClientMap = Arc<Mutex<HashMap<String, (u64, Sender<ViewerSessionRequest>)>>>;

const RELAY_ADDR: &str = "0.0.0.0:9001";

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
            Ok(0) => {
                return false;
            }
            Ok(n) => {
                offset += n;
            }
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
            Ok(n) => {
                offset += n;
            }
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

fn read_exact_logged(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    name: &str,
) -> bool {
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

fn host_to_viewer_tcp(
    mut host: TcpStream,
    mut viewer: TcpStream,
    host_control: TcpStream,
    viewer_control: TcpStream,
) {
    println!("[Relay] HOST -> VIEWER forwarding started");
    let _ = host.set_nodelay(true);
    let _ = viewer.set_nodelay(true);

    let mut buffer = [0u8; 128 * 1024];
    loop {
        match host.read(&mut buffer) {
            Ok(0) => {
                println!("[Relay] Host -> Viewer TCP connection closed.");
                let _ = viewer.shutdown(Shutdown::Both);
                let _ = host_control.shutdown(Shutdown::Both);
                let _ = viewer_control.shutdown(Shutdown::Both);
                break;
            }
            Ok(n) => {
                if !send_all(&mut viewer, &buffer[..n]) {
                    let _ = host.shutdown(Shutdown::Both);
                    let _ = viewer.shutdown(Shutdown::Both);
                    let _ = host_control.shutdown(Shutdown::Both);
                    let _ = viewer_control.shutdown(Shutdown::Both);
                    break;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!("[Relay] HOST -> VIEWER read error: {:?}", e);
                let _ = viewer.shutdown(Shutdown::Both);
                let _ = host_control.shutdown(Shutdown::Both);
                let _ = viewer_control.shutdown(Shutdown::Both);
                break;
            }
        }
    }
}

fn viewer_to_host_tcp(
    mut viewer: TcpStream,
    mut host: TcpStream,
    host_control: TcpStream,
    viewer_control: TcpStream,
) {
    println!("[Relay] VIEWER -> HOST forwarding started");
    let _ = viewer.set_nodelay(true);
    let _ = host.set_nodelay(true);

    let mut buffer = [0u8; 64 * 1024];
    loop {
        match viewer.read(&mut buffer) {
            Ok(0) => {
                println!("[Relay] Viewer -> Host TCP connection closed.");
                let _ = host.shutdown(Shutdown::Both);
                let _ = viewer.shutdown(Shutdown::Both);
                let _ = host_control.shutdown(Shutdown::Both);
                let _ = viewer_control.shutdown(Shutdown::Both);
                break;
            }
            Ok(n) => {
                if !send_all(&mut host, &buffer[..n]) {
                    let _ = viewer.shutdown(Shutdown::Both);
                    let _ = host.shutdown(Shutdown::Both);
                    let _ = host_control.shutdown(Shutdown::Both);
                    let _ = viewer_control.shutdown(Shutdown::Both);
                    break;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!("[Relay] VIEWER -> HOST read error: {:?}", e);
                let _ = host.shutdown(Shutdown::Both);
                let _ = viewer.shutdown(Shutdown::Both);
                let _ = host_control.shutdown(Shutdown::Both);
                let _ = viewer_control.shutdown(Shutdown::Both);
                break;
            }
        }
    }
}

// ============================================================
// HOST SESSION RUNNER (WEBSOCKET BRIDGE)
// ============================================================

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
    let session_id_for_thread = session_id.to_string();

    // Dedicated WS-writer channels: Priority queueing to ensure video can apply backpressure
    // WITHOUT starving control packets or file-transfer packets.
    let (ws_tx_ctrl, ws_rx_ctrl) = std::sync::mpsc::channel::<Vec<u8>>(); // Unbounded for control/file
    let (ws_tx_vid, ws_rx_vid) = std::sync::mpsc::sync_channel::<Vec<u8>>(2); // Bounded (size 2) for video

    let ws_tx_ctrl_fwd = ws_tx_ctrl.clone();
    let ws_tx_vid_fwd = ws_tx_vid.clone();

    let ws_arc_writer = Arc::clone(&ws_arc);
    let ws_arc_closer = Arc::clone(&ws_arc);
    let is_active_writer = Arc::clone(&is_active);
    let session_id_writer = session_id.to_string();

    // WS writer thread
    let ws_writer_handle = thread::spawn(move || {
        // Guarantee that the Viewer receives the Approval packet (Type 1) AND Stream Active (Type 2) FIRST
        if let Ok(mut ws) = ws_arc_writer.lock() {
            let _ = ws.send(Message::Binary(vec![1u8]));
            let _ = ws.send(Message::Binary(vec![2u8]));
        }

        let mut idle_ms = 0;
        while is_active_writer.load(Ordering::SeqCst) {
            let mut sent_control = false;
            while let Ok(msg_bytes) = ws_rx_ctrl.try_recv() {
                let mut lock = match ws_arc_writer.lock() {
                    Ok(l) => l,
                    Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                };
                let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                if let Err(e) = lock.send(Message::Binary(msg_bytes)) {
                    eprintln!("[WS CLOSE] component=ws_writer reason=ctrl_send_failed error={:?} device={}", e, session_id_writer);
                    is_active_writer.store(false, Ordering::SeqCst);
                    break;
                }
                sent_control = true;
                idle_ms = 0;
            }
            if !is_active_writer.load(Ordering::SeqCst) { break; }
            if sent_control { continue; }

            let mut got_msg = false;
            match ws_rx_ctrl.recv_timeout(Duration::from_millis(10)) {
                Ok(msg_bytes) => {
                    let mut lock = match ws_arc_writer.lock() {
                        Ok(l) => l,
                        Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                    };
                    let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = lock.send(Message::Binary(msg_bytes)) {
                        eprintln!("[WS CLOSE] component=ws_writer reason=ctrl_send_failed error={:?} device={}", e, session_id_writer);
                        is_active_writer.store(false, Ordering::SeqCst);
                        break;
                    }
                    got_msg = true;
                    idle_ms = 0;
                }
                Err(_) => {}
            }
            if !is_active_writer.load(Ordering::SeqCst) { break; }
            if got_msg { continue; }

            match ws_rx_vid.recv_timeout(Duration::from_millis(10)) {
                Ok(msg_bytes) => {
                    let mut lock = match ws_arc_writer.lock() {
                        Ok(l) => l,
                        Err(_) => { is_active_writer.store(false, Ordering::SeqCst); break; }
                    };
                    let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = lock.send(Message::Binary(msg_bytes)) {
                        eprintln!("[WS CLOSE] component=ws_writer reason=vid_send_failed error={:?} device={}", e, session_id_writer);
                        is_active_writer.store(false, Ordering::SeqCst);
                        break;
                    }
                    got_msg = true;
                    idle_ms = 0;
                }
                Err(_) => {}
            }
            if !is_active_writer.load(Ordering::SeqCst) { break; }
            if got_msg { continue; }

            idle_ms += 20;
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
        println!("[WS LIFECYCLE] host_to_ws thread started for device={}", session_id_for_thread);
        while is_active_reader.load(Ordering::SeqCst) {
            let _ = host_reader.set_read_timeout(Some(Duration::from_millis(200)));
            let mut type_buf = [0u8; 1];
            if let Err(e) = host_reader.read_exact(&mut type_buf) {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!("[WS CLOSE] component=host_to_ws reason=host_read_failed error={:?} device={}", e, session_id_for_thread);
                break;
            }
            let pkt = type_buf[0];

            match pkt {
                13 | 15 => {
                    let mut header = [0u8; 20];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = host_reader.read_exact(&mut header) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=video_header_failed error={:?} device={}", e, session_id_for_thread);
                        break;
                    }
                    let width  = u32::from_be_bytes(header[0..4].try_into().unwrap());
                    let height = u32::from_be_bytes(header[4..8].try_into().unwrap());
                    let psize  = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
                    if width == 0 || width > 7680 || height == 0 || height > 4320 || psize > 50 * 1024 * 1024 {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=invalid_video_dims w={} h={} p={} device={}", width, height, psize, session_id_for_thread);
                        break;
                    }
                    let mut payload = vec![0u8; psize];
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=video_payload_failed error={:?} device={}", e, session_id_for_thread);
                        break;
                    }
                    println!("[VIDEO RELAY RX]");
                    println!("type = 13");
                    println!("bytes = {}", 1 + 20 + psize);
                    
                    let mut msg = Vec::with_capacity(1 + 20 + psize);
                    msg.push(pkt);
                    msg.extend_from_slice(&header);
                    msg.extend_from_slice(&payload);
                    let msg_len = msg.len();
                    if ws_tx_vid_fwd.send(msg).is_err() {
                        eprintln!("[WS ERROR] component=host_to_ws reason=vid_channel_closed device={}", session_id_for_thread);
                        break;
                    } else {
                        println!("[VIDEO RELAY TX]");
                        println!("type = 13");
                        println!("bytes = {}", msg_len);
                    }
                }
                17 => {
                    let mut header = [0u8; 10];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = host_reader.read_exact(&mut header) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=audio_header_failed error={:?} device={}", e, session_id_for_thread);
                        break;
                    }
                    let psize = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
                    if psize > 10 * 1024 * 1024 { break; }
                    let mut payload = vec![0u8; psize];
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=audio_payload_failed error={:?} device={}", e, session_id_for_thread);
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
                        break;
                    }
                    println!("[HEARTBEAT] device_id={}", session_id_for_thread);
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
                        break;
                    }
                    let psize = u32::from_be_bytes(hdr) as usize;
                    if psize > 50 * 1024 * 1024 { break; }
                    let mut payload = vec![0u8; psize];
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=clipboard_payload_failed error={:?} device={}", e, session_id_for_thread);
                        break;
                    }
                    let mut msg = Vec::with_capacity(5 + psize);
                    msg.push(12u8);
                    msg.extend_from_slice(&hdr);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
                }
                16 => {
                    let mut hdr = [0u8; 2];
                    let _ = host_reader.set_read_timeout(Some(Duration::from_secs(5)));
                    if let Err(e) = host_reader.read_exact(&mut hdr) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=chat_header_failed error={:?} device={}", e, session_id_for_thread);
                        break;
                    }
                    let psize = u16::from_be_bytes(hdr) as usize;
                    let mut payload = vec![0u8; psize];
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=chat_payload_failed error={:?} device={}", e, session_id_for_thread);
                        break;
                    }
                    let mut msg = Vec::with_capacity(3 + psize);
                    msg.push(16u8);
                    msg.extend_from_slice(&hdr);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_ctrl_fwd.send(msg);
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
                99 => {
                    println!("[WS CLOSE] component=host_to_ws reason=host_sent_99 device={}", session_id_for_thread);
                    break;
                }
                _ => {
                    // Unknown type: log and continue (1 byte already consumed, stream in sync)
                    eprintln!("[WS ERROR] component=host_to_ws reason=unknown_type type={} device={}", pkt, session_id_for_thread);
                }
            }
        }
        println!("[WS CLOSE] component=host_to_ws reason=thread_exit device={}", session_id_for_thread);

        // Determine WHO actually ended the session BEFORE we clear the flag.
        // - is_active still true here  => the loop exited on a HOST-side read error / type 99,
        //   i.e. the agent really disconnected. Report it honestly as "Host disconnected" (1011).
        // - is_active already false     => the viewer (browser) side ended the session (ws_to_host
        //   broke first). Do NOT masquerade this as a host disconnect. The agent TCP stays alive
        //   and is re-registered by handle_host; a viewer-side decode error can therefore never
        //   cause the relay to kill the host-agent connection.
        let host_ended_session = is_active_reader.load(Ordering::SeqCst);
        is_active_reader.store(false, Ordering::SeqCst);

        if let Ok(mut ws) = ws_arc_closer.lock() {
            if host_ended_session {
                eprintln!("[WS CLOSE] device={} -> AGENT/HOST side disconnected (genuine host read error).", session_id_for_thread);
                let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Error,
                    reason: "Host disconnected".into(),
                }));
            } else {
                println!("[WS CLOSE] device={} -> VIEWER side ended session. Host agent connection is preserved (not a host disconnect).", session_id_for_thread);
                let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Normal,
                    reason: "Viewer disconnected".into(),
                }));
            }
            let _ = ws.get_ref().shutdown(Shutdown::Both);
        }
    });

    // WebSocket -> Host (control input loop)
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
                if !data.is_empty() {
                    let type_name = match data[0] {
                        0 => "MOUSE_MOVE", 1 | 3 | 7 => "MOUSE_DOWN", 2 | 4 | 8 => "MOUSE_UP",
                        5 => "KEY_DOWN", 6 => "KEY_UP", 9 => "MOUSE_WHEEL", _ => "CONTROL",
                    };
                    println!("[CONTROL RX] type={} bytes={} device={}", data[0], data.len(), session_id);
                    println!("[RELAY CONTROL RX] {}", type_name);
                    println!("[CONTROL DEBUG][RELAY RX]\ntype={}\nlength={}", type_name, data.len());
                }
                // CRITICAL: control write failure is NON-FATAL
                if !send_all(&mut host_writer, &data) {
                    eprintln!("[WS ERROR] component=ws_to_host reason=control_write_failed \
                        type={} device={}", data.first().copied().unwrap_or(255), session_id);
                    // Non-fatal: video stream continues
                } else {
                    if !data.is_empty() {
                        let type_name = match data[0] {
                            0 => "MOUSE_MOVE", 1 | 3 | 7 => "MOUSE_DOWN", 2 | 4 | 8 => "MOUSE_UP",
                            5 => "KEY_DOWN", 6 => "KEY_UP", 9 => "MOUSE_WHEEL", _ => "CONTROL",
                        };
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
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
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
    drop(ws_tx_ctrl); // signal writer thread to exit
    drop(ws_tx_vid);
    println!("[REGISTRY] Viewer disconnected for Device ID: {}", session_id);
    let _ = host_writer.write_all(&[99u8]);
    let _ = host_to_ws_handle.join();
    let _ = ws_writer_handle.join();
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
    // Simple manual parse for backend_url field (avoid serde_json dependency)
    let key = "\"backend_url\":";
    if let Some(pos) = contents.find(key) {
        let after = &contents[pos + key.len()..];
        let start = after.find('"')? + 1;
        let end = after[start..].find('"')? + start;
        return Some(after[start..end].to_string());
    }
    None
}

fn resolve_backend_config() -> (String, String, String) {
    let backend_url = env::var("SCREENSHARE_BACKEND_URL")
        .unwrap_or_else(|_| {
            read_server_config()
                .unwrap_or_else(|| "http://127.0.0.1/Screen%20Share/backend/api".to_string())
        });
    let backend_host = env::var("SCREENSHARE_BACKEND_HOST")
        .unwrap_or_else(|_| {
            let host = backend_url
                .strip_prefix("http://")
                .or_else(|| backend_url.strip_prefix("https://"))
                .unwrap_or(&backend_url)
                .split('/')
                .next()
                .unwrap_or("127.0.0.1");
            host.to_string()
        });
    let backend_port = env::var("SCREENSHARE_BACKEND_PORT")
        .unwrap_or_else(|_| "80".to_string());
    (backend_url, backend_host, backend_port)
}

fn update_backend_presence(session_id: &str, _is_register: bool) {
    // The relay ONLY sends heartbeats. Registration is exclusively performed by the
    // agent's BackendClient (which uses machine_identifier for idempotent upserts).
    // This prevents the relay from creating new/duplicate device rows with the
    // relay-level session_id, which would diverge from the persisted system_id.
    let sid = session_id.to_string();

    // Backend URL is configurable via environment variable.
    // Defaults to localhost only for local development.
    // In production, falls back to /Program Files/Screen Share/server-config.json
    let (backend_url, backend_host, backend_port) = resolve_backend_config();

    thread::spawn(move || {
        println!("[SYNC] RELAY REGISTERED SYSTEM ID = {}", sid);
        let _path = "/devices/heartbeat.php";

        let json_payload = format!("{{\"system_id\":\"{}\"}}", sid);

        // Try the configurable backend first, then fall back to raw TCP to the backend host
        let urls = vec![
            format!("{}/devices/heartbeat.php", backend_url.trim_end_matches('/')),
        ];

        for url in &urls {
            if let Ok(mut stream) = TcpStream::connect(format!("{}:{}", backend_host, backend_port)) {
                let req = format!("POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", url, backend_host, json_payload.len(), json_payload);
                let _ = stream.write_all(req.as_bytes());
                let mut response = String::new();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                let _ = stream.read_to_string(&mut response);
                return;
            }
        }
        eprintln!("[PROXY] Failed to connect to backend at {}:{} for device heartbeat.", backend_host, backend_port);
    });
}


// ============================================================
// HOST CONNECTION & LIFECYCLE (PERSISTENT AGENT THREAD)
// ============================================================

fn handle_host(
    mut stream: TcpStream,
    hosts: ClientMap,
) {
    let conn_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
    let mut type_byte = [0u8; 1];

    if !read_exact_logged(
        &mut stream,
        &mut type_byte,
        "Host packet type",
    ) {
        return;
    }

    let packet_type = type_byte[0];

    if packet_type != 1 {
        eprintln!(
            "[Relay] Invalid host registration packet type: {}",
            packet_type
        );
        return;
    }

    // System IDs are strictly 9 digits long.
    // DO NOT use peek() because TCP may fragment the payload, leading to truncated IDs.
    let id_len = 9;

    let mut actual_id_buf = vec![0u8; id_len];
    if !read_exact_logged(&mut stream, &mut actual_id_buf, "Host session ID") {
        return;
    }

    let session_id = match std::str::from_utf8(&actual_id_buf) {
        Ok(id) => id.trim().to_string(),
        Err(_) => {
            eprintln!("[Relay] Invalid host session ID.");
            return;
        }
    };

    println!("[DEBUG TRACE] [RELAY AGENT REGISTRATION] calculated id_len = {}. parsed session_id = '{}'", id_len, session_id);

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

    // PROXY REGISTRATION TO BACKEND
    update_backend_presence(&session_id, true);
    println!("[REGISTRY] Device ID: {}", session_id);
    println!("[REGISTRY] Connection ID: {}", peer);
    println!("[REGISTRY] Status: ONLINE");
    println!("[RELAY] Connection remains active");

    let (session_tx, session_rx): (Sender<ViewerSessionRequest>, Receiver<ViewerSessionRequest>) = channel();

    // Register active agent sender in hosts map
    if let Ok(mut map) = hosts.lock() {
        map.insert(session_id.clone(), (conn_id, session_tx));
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
            println!("[PAIR] Target socket: HOST ({})", host_peer);
            println!("[PAIR] Message type: AUTH_REQUEST (3)");
            println!("[PAIR] Message size: 33 bytes");

            let mut auth_request = Vec::with_capacity(33);
            auth_request.push(3u8);
            auth_request.extend_from_slice(&req.auth_hash);

            println!("[PAIR] Sending AUTH_REQUEST to HOST");
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
                        println!("[PAIR] HOST authentication response byte=0x01");
                        break true;
                    }
                    0x00 => {
                        println!("[PAIR] HOST authentication response byte=0x00");
                        println!("[APPROVAL] Host rejected: {}", session_id);
                        break false;
                    }
                    0x0E => {
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
                    }
                    other => {
                        eprintln!(
                            "[PAIR][ERROR] Unexpected byte 0x{:02X} while waiting for HOST authentication response",
                            other
                        );
                        break false;
                    }
                }
            };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
            if !approved {
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

                    let host_r = match stream.try_clone() { Ok(s) => s, Err(_) => break };
                    let host_w = match stream.try_clone() { Ok(s) => s, Err(_) => break };
                    let host_c1 = match stream.try_clone() { Ok(s) => s, Err(_) => break };
                    let host_c2 = match stream.try_clone() { Ok(s) => s, Err(_) => break };

                    let viewer_r = match viewer_tcp.try_clone() { Ok(s) => s, Err(_) => break };
                    let viewer_w = match viewer_tcp.try_clone() { Ok(s) => s, Err(_) => break };
                    let viewer_c1 = match viewer_tcp.try_clone() { Ok(s) => s, Err(_) => break };
                    let viewer_c2 = match viewer_tcp.try_clone() { Ok(s) => s, Err(_) => break };

                    let t1 = thread::spawn(move || host_to_viewer_tcp(host_r, viewer_w, host_c1, viewer_c1));
                    let t2 = thread::spawn(move || viewer_to_host_tcp(viewer_r, host_w, host_c2, viewer_c2));
                    let _ = t1.join();
                    let _ = t2.join();
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
                    // Type 14: Heartbeat / Keepalive from host
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

                            // PROXY HEARTBEAT TO BACKEND
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
                    // Normal idle wait timeout, loop back to check viewer requests and heartbeats
                    continue;
                }
                // Actual socket disconnect / error
                println!("[RELAY] Agent {} disconnected", session_id);
                println!("[RELAY] Disconnect reason: {:?}", e);
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
            }
        }
    }
    println!("[RELAY][AGENT] Agent connection state: OFFLINE for device={}", session_id);
}

// ============================================================
// NORMAL TCP VIEWER CONNECTION
// ============================================================

fn handle_viewer(
    mut viewer: TcpStream,
    hosts: ClientMap,
) {
    let mut type_byte = [0u8; 1];

    if !read_exact_logged(
        &mut viewer,
        &mut type_byte,
        "Viewer packet type",
    ) {
        return;
    }

    println!("[Relay] Viewer connected");

    if type_byte[0] != 2 {
        eprintln!(
            "[Relay] Invalid viewer packet type: {}",
            type_byte[0]
        );
        let _ = viewer.write_all(&[3u8]);
        return;
    }

    let mut peek_buf = [0u8; 64];
    let n = match viewer.peek(&mut peek_buf) {
        Ok(n) if n >= 32 => n,
        _ => return,
    };

    let id_len = if n > 32 { n - 32 } else { 9 };
    let mut id_buf = vec![0u8; id_len];
    if !read_exact_logged(&mut viewer, &mut id_buf, "Viewer session ID") {
        return;
    }

    let session_id = match std::str::from_utf8(&id_buf) {
        Ok(id) => id.trim().to_string(),
        Err(_) => {
            eprintln!("[Relay] Invalid viewer session ID.");
            let _ = viewer.write_all(&[3u8]);
            return;
        }
    };

    let mut auth_hash = [0u8; 32];
    if !read_exact_logged(
        &mut viewer,
        &mut auth_hash,
        "Viewer authentication",
    ) {
        return;
    }

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
            println!("[RELAY][ROUTE] Requested System ID {} not currently connected", session_id);
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
        println!("[Relay] TCP viewer pairing rejected for {}", session_id);
    }
}

// ============================================================
// WEBSOCKET VIEWER
// ============================================================

fn handle_websocket_viewer(
    stream: TcpStream,
    hosts: ClientMap,
) {
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
        Ok(Message::Binary(data)) => {
            println!("[RELAY DEBUG] viewer authentication packet received");
            data
        }
        Ok(other) => {
            eprintln!("[Relay] Unexpected WS handshake message: {:?}", other);
            return;
        }
        Err(e) => {
            eprintln!("[Relay] WS handshake read error: {:?}", e);
            return;
        }
    };

    if msg.len() < 2 || msg[0] != 2 {
        eprintln!("[Relay] Invalid WS viewer packet");
        let _ = ws.send(Message::Binary(vec![3u8]));
        return;
    }

    let payload = &msg[1..];
    let (session_id, auth_hash) = if payload.len() >= 32 {
        let id_len = payload.len() - 32;
        let id_slice = &payload[..id_len];
        let clean_id = match std::str::from_utf8(id_slice) {
            Ok(id) => id.trim_matches(char::from(0)).trim().to_string(),
            Err(_) => {
                eprintln!("[AUTH] REJECTED: Invalid WS session ID format");
                println!("[WS] Closing connection\ncode = 1008\nreason = Invalid WS session ID");
                let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                    reason: "Invalid ID".into(),
                }));
                return;
            }
        };
        let mut hash = [0u8; 32];
        if payload.len() >= 32 {
            hash.copy_from_slice(&payload[payload.len() - 32..]);
        }
        (clean_id, hash)
    } else {
        let clean_id = match std::str::from_utf8(payload) {
            Ok(id) => id.trim_matches(char::from(0)).trim().to_string(),
            Err(_) => {
                eprintln!("[AUTH] REJECTED: Invalid WS session ID");
                println!("[WS] Closing connection\ncode = 1008\nreason = Invalid WS session ID");
                let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                    reason: "Invalid ID".into(),
                }));
                return;
            }
        };
        (clean_id, [0u8; 32])
    };

    println!("[DEBUG TRACE] [RELAY VIEWER HANDSHAKE] Payload length: {}, calculated session_id = '{}'", payload.len(), session_id);
    println!("[WS] OPEN");
    println!("[AUTH] Authentication message received");
    println!("[AUTH] Target ID = {}", session_id);
    println!("[AUTH] Token present = {}", if payload.len() >= 32 { "YES" } else { "NO" });

    println!("[AUTH] Received viewer authentication");
    println!("[RELAY][WS] Viewer requested device = {}", session_id);
    println!("[RELAY][LOOKUP] device = {}", session_id);
    println!("[SYNC] VIEWER REQUESTED SYSTEM ID = {}", session_id);

    // Look up agent sender (poll up to 2 seconds if reconnecting)
    let mut host_tx_opt = None;
    for _ in 0..20 {
        {
            if let Ok(map) = hosts.lock() {
                let raw_clean = session_id.replace(' ', "");
                host_tx_opt = map.get(&session_id).map(|(_, tx)| tx.clone()).or_else(|| map.get(&raw_clean).map(|(_, tx)| tx.clone()));
            }
        }
        if host_tx_opt.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let target_found = host_tx_opt.is_some();
    println!("[ROUTING] Target connection found: {}", target_found);

    let host_tx = match host_tx_opt {
        Some(tx) => {
            println!("[RELAY][LOOKUP] FOUND");
            tx
        }
        None => {
            println!("[RELAY][LOOKUP] NOT FOUND");
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
        eprintln!("[PAIR][ERROR] Agent connection dropped before pairing: {}", session_id);
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

    // ============================================================
    // CRITICAL: Must call run_websocket_bridge() to keep the
    // WebSocket alive for the full session. NOT calling it caused
    // the function to return immediately, dropping the TcpStream
    // and producing a browser-side 1006 Abnormal Closure.
    // The [2u8] "session active" packet is sent INSIDE the bridge
    // writer thread as its first action, so we do NOT send it here.
    // ============================================================

    println!("[RELAY][AUTH] Viewer authentication accepted for device={}", session_id);
    println!("[RELAY][PAIR] Viewer paired with agent device={}", session_id);
    println!("[RELAY][STREAM] Entering persistent WebSocket bridge for device={}", session_id);

    // The ws_arc Mutex<WebSocket> and the host TcpStream both move into run_websocket_bridge.
    // That function blocks until the session ends (viewer disconnects or agent disconnects).
    // We need the underlying TcpStream from the ws_arc to pair with the host.
    // The ViewerSessionRequest already transported ws_arc to handle_host via the channel,
    // and handle_host called req.response_tx.send(true), so at this point ws_arc is ours.
    //
    // We must obtain the underlying TcpStream from inside the WebSocket to give it to
    // run_websocket_bridge as the host-facing channel. But ws_arc already holds the viewer's
    // WebSocket — and run_websocket_bridge takes `stream` (the HOST tcp stream) + `ws_arc`.
    //
    // The host stream is NOT available here — it lives in handle_host. This is why the design
    // routes the entire ViewerSession to handle_host via the channel, and handle_host calls
    // run_websocket_bridge from its side with its own `stream` reference.
    //
    // The response_tx.send(true) already unblocked handle_host, which then called
    // run_websocket_bridge() directly (line 715). The bridge is now running in handle_host's
    // thread. We must NOT return here — we must BLOCK until the bridge finishes, otherwise
    // this thread will drop ws_arc (the viewer socket) while the bridge is still using it.
    //
    // Solution: wait on a signal from handle_host that the bridge is done.
    // We do this by recving on a second "done" channel sent back via resp_tx.
    // But the current design has no done-channel. The simplest correct fix:
    // Block here until ws_arc's underlying socket is closed (bridge dropped it).
    // We detect closure by trying to read from the Arc<Mutex<WebSocket>>.

    // Block this thread while the bridge (running in handle_host's thread) is alive.
    // The bridge holds a clone of ws_arc. When it finishes it drops ws_arc clone.
    // We detect completion by waiting until the Arc strong_count drops to 1 (only us hold it).
    // This avoids any changes to the bridge or channel protocol.
    loop {
        // If only this thread holds ws_arc, the bridge has exited and released its clone.
        if Arc::strong_count(&ws_arc) <= 1 {
            println!("[RELAY][CLOSE] Bridge exited for device={} viewer_thread_releasing", session_id);
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    println!("[RELAY][CLOSE] Viewer connection lifecycle complete for device={}", session_id);
    println!("[PAIR] Pairing successful");
    println!("[STREAM] Session ended");
    println!("[RELAY] Host/viewer session complete for device={}", session_id);
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
    
    // Read port from environment (Render/Railway use PORT, we also check RELAY_PORT)
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
                std::thread::sleep(std::time::Duration::from_millis(100));
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
        let mut peek = [0u8; 14];
        if stream.peek(&mut peek).is_err() {
            eprintln!("[Relay] Failed to inspect connection from {}", peer);
            continue;
        }

        if &peek[0..11] == b"GET /health" {
            println!("[Relay] Health check from {}", peer);
            let mut stream = stream;
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
            let _ = stream.shutdown(Shutdown::Both);
            continue;
        }

        let connection_type = peek[0];
        let hosts_clone = Arc::clone(&hosts);

        match connection_type {
            // HOST REGISTRATION (Type 1)
            1 => {
                println!("[Relay] Connection identified as HOST");
                thread::spawn(move || {
                    handle_host(stream, hosts_clone);
                });
            }

            // NORMAL TCP VIEWER (Type 2)
            2 => {
                println!("[Relay] Connection identified as TCP VIEWER");
                thread::spawn(move || {
                    handle_viewer(stream, hosts_clone);
                });
            }

            // WEBSOCKET VIEWER (HTTP GET)
            71 => {
                println!("[Relay] Connection identified as WEBSOCKET");
                thread::spawn(move || {
                    handle_websocket_viewer(stream, hosts_clone);
                });
            }

            _ => {
                eprintln!("[Relay] Unknown initial connection type: {}", connection_type);
                let stream = stream;
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }
}
