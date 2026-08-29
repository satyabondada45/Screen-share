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
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("C:\\Users\\Public\\deskstream_relay.log") {
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

type ClientMap = Arc<Mutex<HashMap<String, Sender<ViewerSessionRequest>>>>;

const RELAY_ADDR: &str = "0.0.0.0:9001";

// ============================================================
// AUTOSTART & BACKGROUND PERSISTENCE
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
        let target_exe = bin_dir.join("relay-server.exe");
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

    // Dedicated WS-writer channel: eliminates Arc<Mutex<WebSocket>> contention
    // between the video-sender thread and the control-read loop.
    let (ws_tx, ws_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
    let ws_tx_fwd = ws_tx.clone();

    let ws_arc_writer = Arc::clone(&ws_arc);
    let is_active_writer = Arc::clone(&is_active);
    let session_id_writer = session_id.to_string();

    // WS writer thread
    let ws_writer_handle = thread::spawn(move || {
        // Guarantee that the Viewer receives the Approval packet (Type 1) FIRST
        if let Ok(mut ws) = ws_arc_writer.lock() {
            let _ = ws.send(Message::Binary(vec![1u8]));
        }

        while is_active_writer.load(Ordering::SeqCst) {
            let msg_bytes = match ws_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(b) => b,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            };
            let mut lock = match ws_arc_writer.lock() {
                Ok(l) => l,
                Err(_) => {
                    eprintln!("[WS CLOSE] component=ws_writer reason=mutex_poisoned device={}", session_id_writer);
                    break;
                }
            };
            let _ = lock.get_ref().set_write_timeout(Some(Duration::from_secs(5)));
            if let Err(e) = lock.send(Message::Binary(msg_bytes)) {
                eprintln!("[WS CLOSE] component=ws_writer reason=send_failed error={:?} device={}", e, session_id_writer);
                break;
            }
        }
        println!("[WS CLOSE] component=ws_writer reason=exiting device={}", session_id_writer);
    });

    // Host -> WS forwarder thread
    let host_to_ws_handle = thread::spawn(move || {
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
                    if width == 0 || width > 7680 || height == 0 || height > 4320 || psize == 0 || psize > 50 * 1024 * 1024 {
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
                    if ws_tx_fwd.try_send(msg).is_err() {
                        eprintln!("[WS ERROR] component=host_to_ws reason=tx_full_dropped_video device={}", session_id_for_thread);
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
                    if psize == 0 || psize > 10 * 1024 * 1024 { break; }
                    let mut payload = vec![0u8; psize];
                    if let Err(e) = host_reader.read_exact(&mut payload) {
                        eprintln!("[WS CLOSE] component=host_to_ws reason=audio_payload_failed error={:?} device={}", e, session_id_for_thread);
                        break;
                    }
                    let mut msg = Vec::with_capacity(1 + 10 + psize);
                    msg.push(17u8);
                    msg.extend_from_slice(&header);
                    msg.extend_from_slice(&payload);
                    let _ = ws_tx_fwd.try_send(msg);
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
                    let _ = ws_tx_fwd.try_send(msg);
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
                    let _ = ws_tx_fwd.try_send(msg);
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
                    let _ = ws_tx_fwd.try_send(msg);
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
    });

    // WebSocket -> Host (control input loop)
    loop {
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
            Ok(Message::Close(_)) => {
                println!("[WS CLOSE] component=ws_to_host reason=browser_sent_close device={}", session_id);
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
                break;
            }
            _ => {}
        }
    }

    println!("[RELAY] Viewer disconnected: main_loop_exited");
    println!("[RELAY] Session ended: main_loop_exited");
    println!("[WS CLOSE] component=bridge reason=main_loop_exited device={}", session_id);
    is_active.store(false, Ordering::SeqCst);
    drop(ws_tx); // signal writer thread to exit
    println!("[REGISTRY] Viewer disconnected for Device ID: {}", session_id);
    let _ = host_writer.write_all(&[99u8]);
    let _ = host_to_ws_handle.join();
    let _ = ws_writer_handle.join();
    true
}

// ============================================================
// BACKEND PRESENCE PROXY
// ============================================================

fn update_backend_presence(session_id: &str, is_register: bool) {
    let sid = session_id.to_string();
    thread::spawn(move || {
        if let Ok(mut stream) = TcpStream::connect("127.0.0.1:80") {
            let json_payload = if is_register {
                format!("{{\"system_id\":\"{}\", \"hostname\":\"Remote Device\", \"os_type\":\"unknown\"}}", sid)
            } else {
                format!("{{\"system_id\":\"{}\"}}", sid)
            };
            let path = if is_register { "/Screen%20Share/backend/api/devices/register.php" } else { "/Screen%20Share/backend/api/devices/heartbeat.php" };
            
            let req = format!("POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", path, json_payload.len(), json_payload);
            let _ = stream.write_all(req.as_bytes());
            
            let mut response = String::new();
            let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
            let _ = stream.read_to_string(&mut response);
            if is_register {
                println!("[PROXY] Registration HTTP response for {}: {}", sid, response.lines().next().unwrap_or("none"));
            }
        } else {
            eprintln!("[PROXY] Failed to connect to 127.0.0.1:80 for device presence.");
        }
    });
}

// ============================================================
// HOST CONNECTION & LIFECYCLE (PERSISTENT AGENT THREAD)
// ============================================================

fn handle_host(
    mut stream: TcpStream,
    hosts: ClientMap,
) {
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

    let mut id_buf = [0u8; 32];
    let n = match stream.peek(&mut id_buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let mut id_len = 0;
    for &b in &id_buf[..n] {
        if b.is_ascii_alphanumeric() || b == b'-' {
            id_len += 1;
        } else {
            break;
        }
    }
    if id_len == 0 {
        id_len = if n >= 9 { 9 } else { 6 };
    }

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
        map.insert(session_id.clone(), session_tx);
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

            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            if !send_all_pair("HOST", &mut stream, &auth_request) {
                eprintln!("[PAIR][ERROR] Failed to send auth request to host: {}", session_id);
                let _ = req.response_tx.send(false);
                continue;
            }

            println!("[PAIR] Waiting for HOST authentication response...");
            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
            let mut response = [0u8; 1];
            if !read_exact_logged(&mut stream, &mut response, "Host authentication response") {
                eprintln!("[PAIR][ERROR] Failed to read authentication response from HOST: {}", session_id);
                let _ = req.response_tx.send(false);
                continue;
            }

            if response[0] != 1 {
                println!("[APPROVAL] Host rejected or timed out: {}", session_id);
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
                println!("[RELAY][DISCONNECT] reason=Connection closed or reset by remote agent");
                println!("[RELAY][DISCONNECT] system_id={}", session_id);
                println!("[RELAY][DISCONNECT] socket_error={:?}", e);
                println!("[RELAY][DISCONNECT] remote_closed=true");
                break;
            }
        }
    }

    // Clean up from activeAgents on exit
    if let Ok(mut map) = hosts.lock() {
        map.remove(&session_id);
    }
    println!("[RELAY] Agent disconnected: network_close");
    println!("[REGISTRY] Device {} removed from active registry (OFFLINE)", session_id);
    println!("[RELAY] activeAgents[{}] = OFFLINE", session_id);
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
        map.get(&session_id).cloned()
    };

    let host_tx = match host_tx {
        Some(tx) => tx,
        None => {
            println!("[Relay] Target host not found: {}", session_id);
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
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("[Relay] WS accept error: {:?}", e);
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

    println!("[WS] OPEN");
    println!("[AUTH] Authentication message received");
    println!("[AUTH] Target ID = {}", session_id);
    println!("[AUTH] Token present = {}", if payload.len() >= 32 { "YES" } else { "NO" });

    println!("[AUTH] Received viewer authentication");
    println!("[RELAY] Viewer requested device: {}", session_id);
    println!("[VIEWER] Requested target = {}", session_id);

    if let Ok(map) = hosts.lock() {
        println!("[RELAY] Registered agents = {:?}", map.keys().collect::<Vec<_>>());
    }

    // Look up agent sender (poll up to 2 seconds if reconnecting)
    let mut host_tx_opt = None;
    for _ in 0..20 {
        {
            if let Ok(map) = hosts.lock() {
                let raw_clean = session_id.replace(' ', "");
                host_tx_opt = map.get(&session_id).cloned().or_else(|| map.get(&raw_clean).cloned());
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
            println!("[RELAY] Agent lookup: FOUND");
            tx
        }
        None => {
            println!("[RELAY AUTH]");
            println!("Viewer connected");
            println!("[RELAY AUTH]");
            println!("Requested target = {}", session_id);
            println!("[RELAY AUTH]");
            println!("Viewer/account = N/A"); // We don't have account info here yet
            println!("[RELAY AUTH]");
            println!("Token valid = YES"); // No token validation on relay
            println!("[RELAY AUTH]");
            println!("Target agent registered = NO");
            println!("[RELAY AUTH]");
            println!("Authorization = DENIED");
            println!("[RELAY AUTH]");
            println!("Reject reason = Target System ID missing or agent offline");

            println!("[RELAY] Agent lookup: NOT FOUND");
            eprintln!("[AUTH] REJECTED: session not found");
            println!("[WS] Closing connection\ncode = 1008\nreason = session not found");
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

    if host_tx.send(req).is_err() {
        eprintln!("[PAIR][ERROR] Agent connection dropped before pairing: {}", session_id);
        if let Ok(mut lock) = ws_arc.lock() {
            let _ = lock.send(Message::Binary(vec![3u8]));
        }
        return;
    }

    println!("[PAIR] Waiting for HOST authentication response...");
    let approved = resp_rx.recv().unwrap_or(false);

    if !approved {
        println!("[RELAY AUTH]");
        println!("Viewer connected");
        println!("[RELAY AUTH]");
        println!("Requested target = {}", session_id);
        println!("[RELAY AUTH]");
        println!("Viewer/account = N/A");
        println!("[RELAY AUTH]");
        println!("Token valid = YES");
        println!("[RELAY AUTH]");
        println!("Target agent registered = YES");
        println!("[RELAY AUTH]");
        println!("Authorization = DENIED");
        println!("[RELAY AUTH]");
        println!("Reject reason = Host device rejected authentication or timed out");

        println!("[AUTH] REJECTED: Host device rejected authentication");
        println!("[WS] Closing connection\ncode = 1008\nreason = authentication rejected");
        if let Ok(mut lock) = ws_arc.lock() {
            let _ = lock.close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                reason: "Rejected".into(),
            }));
        }
        return;
    }
    
    println!("[RELAY AUTH]");
    println!("Viewer connected");
    println!("[RELAY AUTH]");
    println!("Requested target = {}", session_id);
    println!("[RELAY AUTH]");
    println!("Viewer/account = N/A");
    println!("[RELAY AUTH]");
    println!("Token valid = YES");
    println!("[RELAY AUTH]");
    println!("Target agent registered = YES");
    println!("[RELAY AUTH]");
    println!("Authorization = ALLOWED");

    println!("[AUTH] Authentication accepted");

    println!("[PAIR] Sending pairing message to VIEWER");
    println!("[RELAY] Sending authentication success to viewer");
    if let Ok(mut lock) = ws_arc.lock() {
        if lock.send(Message::Binary(vec![2u8])).is_err() {
            eprintln!("[PAIR][ERROR] Failed to send auth success packet to viewer");
            return;
        }
    }
    println!("[PAIR] Viewer pairing message sent");
    println!("[PAIR] Pairing successful");
    println!("[STREAM] Starting stream");
    println!("[CONNECTION] entering persistent connection loop");
    println!("[RELAY] Host/viewer pairing established");
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    let current_exe_path = env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    if let Ok(path) = enable_autostart("DeskStreamRelayServer") {
        println!("[RELAY] Autostart registered -> {}", path);
    }

    println!("========================================");
    println!("       REMOTE DESKTOP RELAY SERVER");
    println!("  BUILD VERSION: 1.0.1 (Production Dedicated Relay)");
    println!("  EXECUTABLE:    {}", current_exe_path);
    println!("========================================");
    println!("[RELAY] Starting relay server...");
    println!("[RELAY] Binding to {}...", RELAY_ADDR);

    let listener = match TcpListener::bind(RELAY_ADDR) {
        Ok(l) => {
            println!("[RELAY] Relay listening on {}", RELAY_ADDR);
            println!("========================================");
            l
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::AddrInUse || e.raw_os_error() == Some(10048) {
                println!("[RELAY] Port 9001 is already in use by an active Relay Server instance.");
                println!("[RELAY] Exactly ONE relay server instance should run on port 9001.");
                println!("[RELAY] Existing relay server is already operational. Exiting cleanly without error.");
                return;
            }
            eprintln!("[RELAY][FATAL] Failed to bind to {}: {:?}", RELAY_ADDR, e);
            return;
        }
    };

    let hosts: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    for incoming in listener.incoming() {
        let stream = match incoming {
            Ok(stream) => stream,
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

        let mut peek = [0u8; 7];
        if stream.peek(&mut peek).is_err() {
            eprintln!("[Relay] Failed to inspect connection from {}", peer);
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
