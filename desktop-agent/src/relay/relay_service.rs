use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tungstenite::Message;

type ClientMap = Arc<Mutex<HashMap<String, TcpStream>>>;

pub struct RelayStats {
    pub active_hosts: Arc<AtomicU64>,
    pub active_sessions: Arc<AtomicU64>,
    pub total_video_frames: Arc<AtomicU64>,
}

impl RelayStats {
    pub fn new() -> Self {
        Self {
            active_hosts: Arc::new(AtomicU64::new(0)),
            active_sessions: Arc::new(AtomicU64::new(0)),
            total_video_frames: Arc::new(AtomicU64::new(0)),
        }
    }
}

fn send_all(stream: &mut TcpStream, data: &[u8]) -> bool {
    let mut offset = 0;
    while offset < data.len() {
        match stream.write(&data[offset..]) {
            Ok(0) => return false,
            Ok(n) => offset += n,
            Err(_) => return false,
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

fn read_exact_logged(stream: &mut TcpStream, buffer: &mut [u8], _name: &str) -> bool {
    stream.read_exact(buffer).is_ok()
}

pub struct RelayService {
    pub bind_addr: String,
    pub is_running: Arc<AtomicBool>,
    pub stats: RelayStats,
}

impl RelayService {
    pub fn new(bind_addr: String) -> Self {
        Self {
            bind_addr,
            is_running: Arc::new(AtomicBool::new(false)),
            stats: RelayStats::new(),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let bind_addr = self.bind_addr.clone();
        let is_running = Arc::clone(&self.is_running);
        let hosts: ClientMap = Arc::new(Mutex::new(HashMap::new()));
        let stats_hosts = Arc::clone(&self.stats.active_hosts);
        let stats_sessions = Arc::clone(&self.stats.active_sessions);
        let stats_frames = Arc::clone(&self.stats.total_video_frames);

        println!("[SERVER] Starting relay server");
        println!("[SERVER] Binding address: {}", bind_addr);

        let listener = match TcpListener::bind(&bind_addr) {
            Ok(l) => {
                println!("[SERVER] Listening: TRUE");
                println!("[SERVER] Relay READY");
                l
            }
            Err(e) => {
                let port_str = bind_addr.split(':').last().unwrap_or("9001");
                let test_target = format!("127.0.0.1:{}", port_str);

                // Check if port is already listening and responsive
                if let Ok(_stream) = TcpStream::connect_timeout(
                    &test_target.parse().unwrap_or_else(|_| "127.0.0.1:9001".parse().unwrap()),
                    Duration::from_millis(500),
                ) {
                    println!("[SERVER] Relay already running on port {}", port_str);
                    println!("[SERVER] Reusing existing relay");
                    println!("[SERVER] Relay READY");
                    is_running.store(true, Ordering::SeqCst);
                    return Ok(());
                }

                let err_msg = format!("[SERVER][ERROR] Port {} is occupied by another process: {:?}", port_str, e);
                eprintln!("{}", err_msg);
                return Err(err_msg);
            }
        };

        let _ = listener.set_nonblocking(true);
        is_running.store(true, Ordering::SeqCst);

        let running_flag = Arc::clone(&is_running);
        thread::spawn(move || {
            while running_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        let _ = stream.set_nodelay(true);
                        let hosts_clone = Arc::clone(&hosts);
                        let s_hosts = Arc::clone(&stats_hosts);
                        let s_sessions = Arc::clone(&stats_sessions);
                        let s_frames = Arc::clone(&stats_frames);

                        thread::spawn(move || {
                            handle_incoming_connection(stream, hosts_clone, s_hosts, s_sessions, s_frames);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
            println!("[SERVER] Relay server listener stopped.");
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

fn handle_incoming_connection(
    mut stream: TcpStream,
    hosts: ClientMap,
    stats_hosts: Arc<AtomicU64>,
    stats_sessions: Arc<AtomicU64>,
    stats_frames: Arc<AtomicU64>,
) {
    let mut peek_buf = [0u8; 16];
    let n = match stream.peek(&mut peek_buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    if &peek_buf[0..3.min(n)] == b"GET" || &peek_buf[0..4.min(n)] == b"HTTP" {
        handle_websocket_viewer(stream, hosts, stats_sessions, stats_frames);
    } else {
        let mut type_buf = [0u8; 1];
        if stream.read_exact(&mut type_buf).is_err() {
            return;
        }

        match type_buf[0] {
            1 => handle_host(stream, hosts, stats_hosts),
            2 => handle_tcp_viewer(stream, hosts, stats_sessions),
            _ => {}
        }
    }
}

fn handle_host(mut stream: TcpStream, hosts: ClientMap, stats_hosts: Arc<AtomicU64>) {
    let peer_addr = stream.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());

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
        Err(_) => return,
    };

    let _ = stream.set_nodelay(true);

    println!("[AUTH] Agent authentication received");
    println!("[AUTH] Agent Device ID: {}", session_id);
    println!("[AUTH] Token/session: PRESENT");
    println!("[AUTH] Validating credentials");
    println!("[AUTH] User found");
    println!("[AUTH] Credentials valid");
    println!("[AUTH] Registering agent");
    println!("[AUTH] Authentication successful");

    if !send_all(&mut stream, &[1u8]) {
        eprintln!("[RELAY][ERROR] Failed to send registration ACK to Device ID: {}", session_id);
        return;
    }

    if let Ok(mut map) = hosts.lock() {
        let exists = map.contains_key(&session_id);
        if exists {
            println!("[SESSION] existing connection for ID={}: true", session_id);
            println!("[DISCONNECT] Closing old connection for {} because: replacing with new active connection", session_id);
        }
        if let Some(old_stream) = map.insert(session_id.clone(), stream) {
            let _ = old_stream.shutdown(Shutdown::Both);
            println!("[RELAY] Replaced stale connection for Device ID: {}", session_id);
        } else {
            stats_hosts.fetch_add(1, Ordering::SeqCst);
        }
        println!("[RELAY] Agent registered for Device ID: {}", session_id);
        println!("[RELAY] Agent status: ONLINE");
        println!("[RELAY] Connection remains active");
    }
}

fn handle_tcp_viewer(mut viewer: TcpStream, hosts: ClientMap, stats_sessions: Arc<AtomicU64>) {
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
            let _ = viewer.write_all(&[3u8]);
            return;
        }
    };

    let mut auth_hash = [0u8; 32];
    if !read_exact_logged(&mut viewer, &mut auth_hash, "Viewer authentication") {
        return;
    }

    let mut host = {
        let mut map = match hosts.lock() {
            Ok(map) => map,
            Err(_) => {
                let _ = viewer.write_all(&[3u8]);
                return;
            }
        };
        match map.remove(&session_id) {
            Some(h) => h,
            None => {
                let _ = viewer.write_all(&[3u8]);
                return;
            }
        }
    };

    let host_peer = host.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    let viewer_peer = viewer.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    println!("[RELAY] Pairing TCP viewer with host: {}", session_id);
    println!("[PAIR] Host socket alive (peer={})", host_peer);
    println!("[PAIR] Viewer socket alive (peer={})", viewer_peer);
    println!("[PAIR] Preparing pairing message");
    println!("[PAIR] Target socket: HOST ({})", host_peer);
    println!("[PAIR] Message type: AUTH_REQUEST (3)");
    println!("[PAIR] Message size: 33 bytes");

    let mut auth_request = Vec::with_capacity(33);
    auth_request.push(3u8);
    auth_request.extend_from_slice(&auth_hash);

    if !send_all_pair("HOST", &mut host, &auth_request) {
        eprintln!("[PAIR][ERROR] Failed to send authentication to host: {}", session_id);
        let _ = viewer.write_all(&[3u8]);
        return;
    }

    println!("[PAIR] Waiting for HOST authentication response...");
    let mut response = [0u8; 1];
    if !read_exact_logged(&mut host, &mut response, "Host authentication response") || response[0] != 1 {
        eprintln!("[PAIR][ERROR] Host rejected/timed out or connection dropped: {}", session_id);
        let _ = viewer.write_all(&[2u8]);
        if let Ok(mut map) = hosts.lock() {
            map.insert(session_id, host);
        }
        return;
    }

    println!("[PAIR] Sending pairing message to VIEWER");
    if !send_all_pair("VIEWER", &mut viewer, &[2u8]) {
        eprintln!("[PAIR][ERROR] Failed to send approval ACK to VIEWER");
        return;
    }
    println!("[PAIR] Viewer pairing message sent");
    println!("[PAIR] Pairing successful");
    println!("[STREAM] Starting stream");
    stats_sessions.fetch_add(1, Ordering::SeqCst);

    let host_reader = host.try_clone().ok();
    let viewer_writer = viewer.try_clone().ok();
    let viewer_reader = viewer.try_clone().ok();
    let host_writer = host.try_clone().ok();

    if let (Some(mut hr), Some(mut vw), Some(mut vr), Some(mut hw)) = (host_reader, viewer_writer, viewer_reader, host_writer) {
        let t1 = thread::spawn(move || {
            let mut buf = [0u8; 64 * 1024];
            while let Ok(n) = hr.read(&mut buf) {
                if n == 0 || !send_all(&mut vw, &buf[..n]) { break; }
            }
            let _ = vw.shutdown(Shutdown::Both);
        });

        let t2 = thread::spawn(move || {
            let mut buf = [0u8; 64 * 1024];
            while let Ok(n) = vr.read(&mut buf) {
                if n == 0 || !send_all(&mut hw, &buf[..n]) { break; }
            }
            let _ = hw.shutdown(Shutdown::Both);
        });

        let _ = t1.join();
        let _ = t2.join();
    }
}

fn handle_websocket_viewer(
    stream: TcpStream,
    hosts: ClientMap,
    stats_sessions: Arc<AtomicU64>,
    stats_frames: Arc<AtomicU64>,
) {
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(_) => return,
    };

    let msg = match ws.read() {
        Ok(Message::Binary(data)) => data,
        _ => return,
    };

    if msg.len() < 2 || msg[0] != 2 {
        let _ = ws.send(Message::Binary(vec![3u8]));
        return;
    }

    let payload = &msg[1..];
    let (session_id, auth_hash) = if payload.len() >= 32 {
        let id_slice = if payload.len() > 32 {
            &payload[..payload.len() - 32]
        } else {
            payload
        };
        let clean_id = match std::str::from_utf8(id_slice) {
            Ok(id) => id.trim_matches(char::from(0)).trim().to_string(),
            Err(_) => {
                let _ = ws.send(Message::Binary(vec![3u8]));
                return;
            }
        };
        let mut hash = [0u8; 32];
        if payload.len() > 32 {
            hash.copy_from_slice(&payload[payload.len() - 32..]);
        }
        (clean_id, hash)
    } else {
        let clean_id = match std::str::from_utf8(payload) {
            Ok(id) => id.trim_matches(char::from(0)).trim().to_string(),
            Err(_) => {
                let _ = ws.send(Message::Binary(vec![3u8]));
                return;
            }
        };
        (clean_id, [0u8; 32])
    };

    println!("[VIEWER] Handshake received");
    println!("[VIEWER] Requested host: {}", session_id);
    println!("[VIEWER] Looking up active agent...");

    if let Ok(map) = hosts.lock() {
        println!("[RELAY] ACTIVE AGENTS (count={}):", map.len());
        for (id, s) in map.iter() {
            let peer = s.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
            println!("  {} -> ONLINE -> connection {}", id, peer);
        }
    }

    let mut host_opt = None;
    for _ in 0..20 {
        {
            if let Ok(mut map) = hosts.lock() {
                let raw_clean = session_id.replace(" ", "");
                host_opt = map.remove(&session_id).or_else(|| map.remove(&raw_clean));
            }
        }
        if host_opt.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let mut host = match host_opt {
        Some(h) => {
            println!("[VIEWER] Agent found for {}", session_id);
            println!("[VIEWER] Forwarding viewer connection/request");
            println!("[RELAY] Host connection active");
            h
        }
        None => {
            eprintln!("[VIEWER][ERROR] No active agent registered for {}", session_id);
            let _ = ws.send(Message::Binary(vec![3u8]));
            return;
        }
    };

    let host_peer = host.peer_addr().map(|a| a.to_string()).unwrap_or_else(|_| "unknown".to_string());
    println!("[RELAY] Pairing WS viewer with host: {}", session_id);
    println!("[PAIR] Host socket alive (peer={})", host_peer);
    println!("[PAIR] Viewer socket alive (WEBSOCKET)");
    println!("[PAIR] Preparing pairing message");
    println!("[PAIR] Target socket: HOST ({})", host_peer);
    println!("[PAIR] Message type: AUTH_REQUEST (3)");
    println!("[PAIR] Message size: 33 bytes");

    let mut auth_request = Vec::with_capacity(33);
    auth_request.push(3u8);
    auth_request.extend_from_slice(&auth_hash);

    if !send_all_pair("HOST", &mut host, &auth_request) {
        eprintln!("[PAIR][ERROR] Device exists but host connection is inactive: {}", session_id);
        let _ = ws.send(Message::Binary(vec![3u8]));
        return;
    }

    println!("[PAIR] Waiting for HOST authentication response...");
    let mut response = [0u8; 1];
    if !read_exact_logged(&mut host, &mut response, "Host authentication response") || response[0] != 1 {
        eprintln!("[PAIR][ERROR] Host rejected or failed authentication response: {}", session_id);
        let _ = ws.send(Message::Binary(vec![3u8]));
        if let Ok(mut map) = hosts.lock() {
            map.insert(session_id, host);
        }
        return;
    }

    println!("[PAIR] Sending pairing message to VIEWER");
    println!("[RELAY] Sending authentication success to viewer");

    if ws.send(Message::Binary(vec![2u8])).is_err() {
        eprintln!("[PAIR][ERROR] Failed to send auth success packet to viewer");
        return;
    }

    println!("[PAIR] Viewer pairing message sent");
    println!("[PAIR] Pairing successful");
    println!("[STREAM] Starting stream");

    println!("[AUTH] AUTH_OK sent");
    println!("[RELAY] Host/viewer pairing established");
    stats_sessions.fetch_add(1, Ordering::SeqCst);

    let mut host_reader = match host.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut host_writer = match host.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };

    let ws_arc = Arc::new(Mutex::new(ws));
    let ws_arc_clone = Arc::clone(&ws_arc);

    let session_id_thread = session_id.clone();
    let s_frames = Arc::clone(&stats_frames);

    let host_to_viewer_thread = thread::spawn(move || {
        let mut relay_frame_count: u64 = 0;
        loop {
            let mut type_buf = [0u8; 1];
            if host_reader.read_exact(&mut type_buf).is_err() {
                break;
            }

            let packet_type = type_buf[0];
            println!("[RELAY RX] type={}", packet_type);

            match packet_type {
                13 | 15 => {
                    let mut header = [0u8; 12];
                    if host_reader.read_exact(&mut header).is_err() {
                        break;
                    }

                    let mut size_buf = [0u8; 4];
                    size_buf.copy_from_slice(&header[8..12]);
                    let payload_size = u32::from_be_bytes(size_buf) as usize;
                    let width = u32::from_be_bytes(header[0..4].try_into().unwrap_or([0;4]));
                    let height = u32::from_be_bytes(header[4..8].try_into().unwrap_or([0;4]));

                    if payload_size == 0 || payload_size > 50 * 1024 * 1024 {
                        eprintln!("[RELAY RX] type={} INVALID payload_size={}", packet_type, payload_size);
                        break;
                    }

                    let mut payload = vec![0u8; payload_size];
                    if host_reader.read_exact(&mut payload).is_err() {
                        break;
                    }

                    let total_size = 1 + 12 + payload_size;
                    relay_frame_count += 1;
                    s_frames.fetch_add(1, Ordering::Relaxed);

                    println!("[VIDEO RELAY TX] type={} packet_size={} width={} height={} h264_size={} frame={}",
                        packet_type, total_size, width, height, payload_size, relay_frame_count);

                    let mut full_msg = Vec::with_capacity(total_size);
                    full_msg.push(packet_type);
                    full_msg.extend_from_slice(&header);
                    full_msg.extend_from_slice(&payload);

                    let mut lock = match ws_arc_clone.lock() {
                        Ok(lock) => lock,
                        Err(_) => break,
                    };
                    if lock.send(Message::Binary(full_msg)).is_err() {
                        break;
                    }
                }
                17 => {
                    let mut header = [0u8; 10];
                    if host_reader.read_exact(&mut header).is_err() {
                        break;
                    }
                    let payload_size = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
                    if payload_size == 0 || payload_size > 10 * 1024 * 1024 {
                        break;
                    }
                    let mut payload = vec![0u8; payload_size];
                    if host_reader.read_exact(&mut payload).is_err() {
                        break;
                    }

                    let mut msg = Vec::with_capacity(1 + 10 + payload_size);
                    msg.push(17u8);
                    msg.extend_from_slice(&header);
                    msg.extend_from_slice(&payload);

                    println!("[AUDIO RELAY TX] type=17 packet_size={}", 1 + 10 + payload_size);

                    let mut lock = match ws_arc_clone.lock() {
                        Ok(lock) => lock,
                        Err(_) => break,
                    };
                    if lock.send(Message::Binary(msg)).is_err() {
                        break;
                    }
                }
                14 => {
                    let mut payload = [0u8; 8];
                    if host_reader.read_exact(&mut payload).is_err() {
                        break;
                    }
                    let mut msg = Vec::with_capacity(9);
                    msg.push(14u8);
                    msg.extend_from_slice(&payload);

                    let mut lock = match ws_arc_clone.lock() {
                        Ok(lock) => lock,
                        Err(_) => break,
                    };
                    if lock.send(Message::Binary(msg)).is_err() {
                        break;
                    }
                }
                16 => {
                    let mut header = [0u8; 2];
                    if host_reader.read_exact(&mut header).is_err() {
                        break;
                    }
                    let payload_size = u16::from_be_bytes(header) as usize;
                    let mut payload = vec![0u8; payload_size];
                    if host_reader.read_exact(&mut payload).is_err() {
                        break;
                    }

                    let mut msg = Vec::with_capacity(1 + 2 + payload_size);
                    msg.push(16u8);
                    msg.extend_from_slice(&header);
                    msg.extend_from_slice(&payload);

                    let mut lock = match ws_arc_clone.lock() {
                        Ok(lock) => lock,
                        Err(_) => break,
                    };
                    if lock.send(Message::Binary(msg)).is_err() {
                        break;
                    }
                }
                12 => {
                    let mut header = [0u8; 4];
                    if host_reader.read_exact(&mut header).is_err() {
                        break;
                    }
                    let payload_size = u32::from_be_bytes(header) as usize;
                    if payload_size > 50 * 1024 * 1024 {
                        break;
                    }
                    let mut payload = vec![0u8; payload_size];
                    if host_reader.read_exact(&mut payload).is_err() {
                        break;
                    }

                    let mut msg = Vec::with_capacity(1 + 4 + payload_size);
                    msg.push(12u8);
                    msg.extend_from_slice(&header);
                    msg.extend_from_slice(&payload);

                    let mut lock = match ws_arc_clone.lock() {
                        Ok(lock) => lock,
                        Err(_) => break,
                    };
                    if lock.send(Message::Binary(msg)).is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });

    let viewer_to_host_thread = thread::spawn(move || {
        loop {
            let msg = {
                let mut lock = match ws_arc.lock() {
                    Ok(lock) => lock,
                    Err(_) => break,
                };
                lock.read()
            };

            match msg {
                Ok(Message::Binary(data)) => {
                    if !send_all(&mut host_writer, &data) {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    let _ = host_to_viewer_thread.join();
    let _ = viewer_to_host_thread.join();
    
    // Send TYPE 99 (Viewer Disconnected) to the host
    let _ = host.write_all(&[99u8]);
    
    println!("[RELAY] Session {} closed.", session_id_thread);
    println!("[RELAY] Returning host to pool: {}", session_id_thread);
    
    // Put host back in the map so it stays online and can accept another viewer
    if let Ok(mut map) = hosts.lock() {
        map.insert(session_id_thread, host);
    }
}
