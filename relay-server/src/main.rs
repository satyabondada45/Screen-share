use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tungstenite::Message;

type ClientMap = Arc<Mutex<HashMap<String, TcpStream>>>;

const RELAY_ADDR: &str = "0.0.0.0:9001";

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
            eprintln!(
                "[Relay] {} read failed: {:?}",
                name,
                e
            );

            false
        }
    }
}

// ============================================================
// HOST -> VIEWER
//
// IMPORTANT:
//
// After authentication, TCP traffic is NOT parsed.
//
// Everything is copied exactly as received.
//
// Protocol:
//
// [TYPE][WIDTH][HEIGHT][JPEG_SIZE][JPEG]
//
// TCP does not preserve application packet boundaries.
// Therefore we simply proxy the bytes.
// ============================================================

fn host_to_viewer(
    mut host: TcpStream,
    mut viewer: TcpStream,
    host_control: TcpStream,
    viewer_control: TcpStream,
) {
    println!(
        "[Relay] HOST -> VIEWER forwarding started"
    );

    let _ = host.set_nodelay(true);
    let _ = viewer.set_nodelay(true);

    let mut buffer = [0u8; 128 * 1024];

    let mut total_received: u64 = 0;
    let mut total_sent: u64 = 0;
    let mut last_reported_mb: u64 = 0;

    loop {
        match host.read(&mut buffer) {
            Ok(0) => {
                println!(
                    "[Relay] Host -> Viewer connection closed."
                );

                println!(
                    "[Relay Video] Final host bytes received: {}",
                    total_received
                );

                println!(
                    "[Relay Video] Final viewer bytes sent: {}",
                    total_sent
                );

                let _ = viewer.shutdown(Shutdown::Both);
                let _ = host_control.shutdown(Shutdown::Both);
                let _ = viewer_control.shutdown(Shutdown::Both);

                break;
            }

            Ok(n) => {
                total_received += n as u64;

                println!(
                    "[Relay Video] HOST READ: {} bytes | total={}",
                    n,
                    total_received
                );

                /*
                    IMPORTANT:

                    Do NOT parse this buffer.

                    TCP may contain:
                    - part of a packet
                    - one complete packet
                    - multiple packets
                */

                if !send_all(
                    &mut viewer,
                    &buffer[..n],
                ) {
                    println!(
                        "[Relay] Failed forwarding data to viewer."
                    );

                    let _ = host.shutdown(Shutdown::Both);
                    let _ = viewer.shutdown(Shutdown::Both);
                    let _ = host_control.shutdown(Shutdown::Both);
                    let _ = viewer_control.shutdown(Shutdown::Both);

                    break;
                }

                total_sent += n as u64;

                println!(
                    "[Relay Video] FORWARDED: {} bytes | total={}",
                    n,
                    total_sent
                );

                let current_mb =
                    total_received / (1024 * 1024);

                if current_mb >= last_reported_mb + 10 {
                    last_reported_mb = current_mb;

                    println!(
                        "[Relay Video] Host received: {} MB",
                        current_mb
                    );

                    println!(
                        "[Relay Video] Viewer sent: {} MB",
                        total_sent / (1024 * 1024)
                    );
                }
            }

            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!(
                    "[Relay] HOST -> VIEWER read error: {:?}",
                    e
                );

                let _ = viewer.shutdown(Shutdown::Both);
                let _ = host_control.shutdown(Shutdown::Both);
                let _ = viewer_control.shutdown(Shutdown::Both);

                break;
            }
        }
    }
}

// ============================================================
// VIEWER -> HOST
//
// Everything is copied unchanged.
//
// Examples:
//
// 0  = mouse move
// 1  = left mouse down
// 2  = left mouse up
// 3  = right mouse down
// 4  = right mouse up
// 5  = keyboard down
// 6  = keyboard up
// 8  = scroll
// 12 = clipboard
// 16 = chat
// 20/21 = file transfer
// ============================================================

fn viewer_to_host(
    mut viewer: TcpStream,
    mut host: TcpStream,
    host_control: TcpStream,
    viewer_control: TcpStream,
) {
    println!(
        "[Relay] VIEWER -> HOST forwarding started"
    );

    let _ = viewer.set_nodelay(true);
    let _ = host.set_nodelay(true);

    let mut buffer = [0u8; 64 * 1024];

    let mut total_received: u64 = 0;
    let mut total_sent: u64 = 0;

    loop {
        match viewer.read(&mut buffer) {
            Ok(0) => {
                println!(
                    "[Relay] Viewer -> Host connection closed."
                );

                let _ = host.shutdown(Shutdown::Both);
                let _ = viewer.shutdown(Shutdown::Both);
                let _ = host_control.shutdown(Shutdown::Both);
                let _ = viewer_control.shutdown(Shutdown::Both);

                break;
            }

            Ok(n) => {
                total_received += n as u64;

                println!(
                    "[Relay Input] VIEWER READ: {} bytes | total={}",
                    n,
                    total_received
                );

                if !send_all(
                    &mut host,
                    &buffer[..n],
                ) {
                    println!(
                        "[Relay] Failed forwarding data to host."
                    );

                    let _ = viewer.shutdown(Shutdown::Both);
                    let _ = host.shutdown(Shutdown::Both);
                    let _ = host_control.shutdown(Shutdown::Both);
                    let _ = viewer_control.shutdown(Shutdown::Both);

                    break;
                }

                total_sent += n as u64;

                println!(
                    "[Relay Input] FORWARDED TO HOST: {} bytes | total={}",
                    n,
                    total_sent
                );
            }

            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    continue;
                }
                eprintln!(
                    "[Relay] VIEWER -> HOST read error: {:?}",
                    e
                );

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
// HOST CONNECTION
// ============================================================
//
// Host registration:
//
// 1 byte  = 1
// 6 bytes = session ID
//
// Total = 7 bytes
// ============================================================

fn handle_host(
    mut stream: TcpStream,
    hosts: ClientMap,
) {
    let mut init = [0u8; 7];

    if !read_exact_logged(
        &mut stream,
        &mut init,
        "Host registration",
    ) {
        return;
    }

    let packet_type = init[0];

    if packet_type != 1 {
        eprintln!(
            "[Relay] Invalid host registration packet type: {}",
            packet_type
        );

        return;
    }

    let session_id = match std::str::from_utf8(
        &init[1..7],
    ) {
        Ok(id) => id.to_string(),

        Err(_) => {
            eprintln!(
                "[Relay] Invalid host session ID."
            );

            return;
        }
    };

    let _ = stream.set_nodelay(true);

    println!(
        "[Relay] Host registered: {}",
        session_id
    );

    /*
        Replace any stale host with this connection.
    */

    if let Ok(mut map) = hosts.lock() {
        map.insert(
            session_id.clone(),
            stream,
        );
    } else {
        eprintln!(
            "[Relay] Failed to lock host registry."
        );

        return;
    }

    println!(
        "[Relay] Host {} waiting for viewer...",
        session_id
    );
}

// ============================================================
// NORMAL TCP VIEWER CONNECTION
// ============================================================
//
// Viewer:
//
// 1 byte  = 2
// 6 bytes = session ID
// 32 bytes = SHA256 PIN
//
// Total handshake = 39 bytes
// ============================================================

fn handle_viewer(
    mut viewer: TcpStream,
    hosts: ClientMap,
) {
    let mut header = [0u8; 7];

    if !read_exact_logged(
        &mut viewer,
        &mut header,
        "Viewer handshake",
    ) {
        return;
    }

    if header[0] != 2 {
        eprintln!(
            "[Relay] Invalid viewer packet type: {}",
            header[0]
        );

        let _ = viewer.write_all(&[3u8]);

        return;
    }

    let session_id = match std::str::from_utf8(
        &header[1..7],
    ) {
        Ok(id) => id.to_string(),

        Err(_) => {
            eprintln!(
                "[Relay] Invalid viewer session ID."
            );

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

    println!(
        "[Relay] Viewer requesting host {}",
        session_id
    );

    // --------------------------------------------------------
    // Take ownership of host
    // --------------------------------------------------------

    let host = {
        let mut map = match hosts.lock() {
            Ok(map) => map,

            Err(_) => {
                let _ = viewer.write_all(&[3u8]);
                return;
            }
        };

        map.remove(&session_id)
    };

    let mut host = match host {
        Some(host) => host,

        None => {
            println!(
                "[Relay] Host {} not found.",
                session_id
            );

            let _ = viewer.write_all(&[3u8]);

            return;
        }
    };

    let _ = host.set_nodelay(true);
    let _ = viewer.set_nodelay(true);

    // --------------------------------------------------------
    // Send authentication request to host
    //
    // Host receives:
    //
    // [3][32-byte SHA256]
    // --------------------------------------------------------

    let mut auth_request =
        Vec::with_capacity(33);

    auth_request.push(3u8);

    auth_request.extend_from_slice(
        &auth_hash,
    );

    if !send_all(
        &mut host,
        &auth_request,
    ) {
        eprintln!(
            "[Relay] Failed to send authentication to host."
        );

        let _ = viewer.write_all(&[3u8]);

        return;
    }

    // --------------------------------------------------------
    // Host authentication response
    //
    // 1 = approved
    // 2 = rejected
    // --------------------------------------------------------

    let mut response = [0u8; 1];

    if !read_exact_logged(
        &mut host,
        &mut response,
        "Host authentication response",
    ) {
        let _ = viewer.write_all(&[3u8]);
        return;
    }

    if response[0] != 1 {
        println!(
            "[Relay] Host rejected viewer {}",
            session_id
        );

        let _ = viewer.write_all(&[2u8]);

        // Put host back into registry.
        if let Ok(mut map) = hosts.lock() {
            map.insert(
                session_id.clone(),
                host,
            );
        }

        return;
    }

    // --------------------------------------------------------
    // Authentication successful
    // --------------------------------------------------------

    println!(
        "[Relay] Session approved: {}",
        session_id
    );

    println!(
        "[Relay] TCP session fully established: {}",
        session_id
    );

    // Tell viewer connection is approved.
    if !send_all(
        &mut viewer,
        &[1u8],
    ) {
        return;
    }

    // --------------------------------------------------------
    // Clone streams
    // --------------------------------------------------------

    let host_reader =
        match host.try_clone() {
            Ok(s) => s,

            Err(e) => {
                eprintln!(
                    "[Relay] Failed to clone host reader: {:?}",
                    e
                );

                return;
            }
        };

    let viewer_writer =
        match viewer.try_clone() {
            Ok(s) => s,

            Err(e) => {
                eprintln!(
                    "[Relay] Failed to clone viewer writer: {:?}",
                    e
                );

                return;
            }
        };

    let viewer_reader =
        match viewer.try_clone() {
            Ok(s) => s,

            Err(e) => {
                eprintln!(
                    "[Relay] Failed to clone viewer reader: {:?}",
                    e
                );

                return;
            }
        };

    let host_writer =
        match host.try_clone() {
            Ok(s) => s,

            Err(e) => {
                eprintln!(
                    "[Relay] Failed to clone host writer: {:?}",
                    e
                );

                return;
            }
        };

    // Additional control clones used to shut down
    // the opposite direction when one side closes.

    let host_control_for_host_thread =
        match host.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

    let viewer_control_for_host_thread =
        match viewer.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

    let host_control_for_viewer_thread =
        match host.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

    let viewer_control_for_viewer_thread =
        match viewer.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };

    // --------------------------------------------------------
    // HOST -> VIEWER
    // --------------------------------------------------------

    let host_to_viewer_thread =
        thread::spawn(move || {
            host_to_viewer(
                host_reader,
                viewer_writer,
                host_control_for_host_thread,
                viewer_control_for_host_thread,
            );
        });

    // --------------------------------------------------------
    // VIEWER -> HOST
    // --------------------------------------------------------

    let viewer_to_host_thread =
        thread::spawn(move || {
            viewer_to_host(
                viewer_reader,
                host_writer,
                host_control_for_viewer_thread,
                viewer_control_for_viewer_thread,
            );
        });

    // --------------------------------------------------------
    // Wait for forwarding threads
    // --------------------------------------------------------

    let _ = host_to_viewer_thread.join();
    let _ = viewer_to_host_thread.join();

    let _ = host.shutdown(Shutdown::Both);
    let _ = viewer.shutdown(Shutdown::Both);

    println!(
        "[Relay] Session {} closed.",
        session_id
    );
}

// ============================================================
// WEBSOCKET VIEWER
// ============================================================
//
// WebSocket viewers cannot simply receive arbitrary TCP chunks
// because WebSocket itself requires message boundaries.
//
// Therefore the relay parses the application protocol here
// and creates one WebSocket binary message per application
// packet.
//
// Supported host -> viewer packets:
//
// 1  = video
// 13 = some data packet
// 16 = chat
// 12 = clipboard
// ============================================================

fn handle_websocket_viewer(
    stream: TcpStream,
    hosts: ClientMap,
) {
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,

        Err(e) => {
            eprintln!(
                "[Relay] WS accept error: {:?}",
                e
            );

            return;
        }
    };

    // --------------------------------------------------------
    // First WebSocket message = viewer handshake
    // --------------------------------------------------------

    let msg = match ws.read() {
        Ok(Message::Binary(data)) => data,

        Ok(other) => {
            eprintln!(
                "[Relay] Unexpected WS handshake message: {:?}",
                other
            );

            return;
        }

        Err(e) => {
            eprintln!(
                "[Relay] WS handshake read error: {:?}",
                e
            );

            return;
        }
    };

    /*
        Expected:

        1 byte  = 2
        6 bytes = session ID
        32 bytes = SHA256 PIN

        Total = 39
    */

    if msg.len() < 39 || msg[0] != 2 {
        eprintln!(
            "[Relay] Invalid WS viewer packet"
        );

        let _ = ws.send(
            Message::Binary(vec![3u8])
        );

        return;
    }

    let session_id =
        match std::str::from_utf8(
            &msg[1..7],
        ) {
            Ok(id) => id.to_string(),

            Err(_) => {
                eprintln!(
                    "[Relay] Invalid WS session ID."
                );

                return;
            }
        };

    let mut auth_hash = [0u8; 32];

    auth_hash.copy_from_slice(
        &msg[7..39]
    );

    println!(
        "[Relay] WS Viewer requesting host {}",
        session_id
    );

    // --------------------------------------------------------
    // Find host
    // --------------------------------------------------------

    let host_opt = {
        let mut map = match hosts.lock() {
            Ok(map) => map,

            Err(_) => {
                return;
            }
        };

        map.remove(&session_id)
    };

    let mut host = match host_opt {
        Some(host) => host,

        None => {
            println!(
                "[Relay] Host {} not found.",
                session_id
            );

            let _ = ws.send(
                Message::Binary(vec![3u8])
            );

            return;
        }
    };

    let _ = host.set_nodelay(true);

    // --------------------------------------------------------
    // Send authentication request to host
    // --------------------------------------------------------

    let mut auth_request =
        Vec::with_capacity(33);

    auth_request.push(3u8);

    auth_request.extend_from_slice(
        &auth_hash,
    );

    if !send_all(
        &mut host,
        &auth_request,
    ) {
        let _ = ws.send(
            Message::Binary(vec![3u8])
        );

        return;
    }

    // --------------------------------------------------------
    // Read host authentication response
    // --------------------------------------------------------

    let mut response = [0u8; 1];

    if !read_exact_logged(
        &mut host,
        &mut response,
        "Host authentication response",
    ) {
        let _ = ws.send(
            Message::Binary(vec![3u8])
        );

        return;
    }

    if response[0] != 1 {
        println!(
            "[Relay] Host rejected WS viewer {}",
            session_id
        );

        let _ = ws.send(
            Message::Binary(vec![2u8])
        );

        if let Ok(mut map) = hosts.lock() {
            map.insert(
                session_id.clone(),
                host,
            );
        }

        return;
    }

    // --------------------------------------------------------
    // Approved
    // --------------------------------------------------------

    println!(
        "[Relay] WS Session approved: {}",
        session_id
    );

    if ws.send(
        Message::Binary(vec![1u8])
    ).is_err() {
        return;
    }

    // --------------------------------------------------------
    // Clone host
    // --------------------------------------------------------

    let mut host_reader =
        match host.try_clone() {
            Ok(s) => s,

            Err(e) => {
                eprintln!(
                    "[Relay] Failed to clone WS host reader: {:?}",
                    e
                );

                return;
            }
        };

    let mut host_writer =
        match host.try_clone() {
            Ok(s) => s,

            Err(e) => {
                eprintln!(
                    "[Relay] Failed to clone WS host writer: {:?}",
                    e
                );

                return;
            }
        };

    // --------------------------------------------------------
    // Configure WebSocket TCP stream
    // --------------------------------------------------------

    let ws_stream = ws.get_mut();

    let _ = ws_stream.set_read_timeout(
        Some(Duration::from_millis(50))
    );

    let _ = ws_stream.set_nodelay(true);

    // --------------------------------------------------------
    // Put WebSocket in Arc/Mutex
    // --------------------------------------------------------

    let ws_arc =
        Arc::new(Mutex::new(ws));

    let ws_arc_clone =
        Arc::clone(&ws_arc);

    // --------------------------------------------------------
    // HOST -> WEBSOCKET VIEWER
    // --------------------------------------------------------

    let _host_to_viewer_thread =
        thread::spawn(move || {
            loop {
                // ------------------------------------------------
                // Read packet type
                // ------------------------------------------------

                let mut type_buf = [0u8; 1];

                if host_reader
                    .read_exact(&mut type_buf)
                    .is_err()
                {
                    break;
                }

                let packet_type =
                    type_buf[0];

                match packet_type {
                    // ====================================================
                    // VIDEO
                    //
                    // Type:
                    //
                    // 1 byte  = type
                    // 12 bytes = video header
                    // N bytes = JPEG
                    //
                    // JPEG size is bytes 8..12 of the 12-byte header.
                    // ====================================================

                    1 => {
                        let mut header =
                            [0u8; 12];

                        if host_reader
                            .read_exact(&mut header)
                            .is_err()
                        {
                            break;
                        }

                        let mut size_buf =
                            [0u8; 4];

                        size_buf.copy_from_slice(
                            &header[8..12]
                        );

                        let payload_size =
                            u32::from_be_bytes(
                                size_buf
                            ) as usize;

                        println!(
                            "[Relay Video] Packet type: 1"
                        );

                        println!(
                            "[Relay Video] Header size: 12"
                        );

                        println!(
                            "[Relay Video] Payload size: {}",
                            payload_size
                        );

                        // Basic sanity check.
                        if payload_size >
                            50 * 1024 * 1024
                        {
                            eprintln!(
                                "[Relay Video] PARSE ERROR: Invalid payload size: {}",
                                payload_size
                            );

                            break;
                        }

                        let mut payload =
                            vec![0u8; payload_size];

                        if let Err(e) = host_reader.read_exact(&mut payload) {
                            eprintln!("[Relay Video] PARSE ERROR: failed to read payload: {:?}", e);
                            break;
                        }

                        let total_size = 1 + 12 + payload_size;
                        println!("[Relay Video] Total packet size: {}", total_size);

                        let mut msg =
                            Vec::with_capacity(
                                total_size
                            );

                        msg.push(1u8);
                        msg.extend_from_slice(&header);
                        msg.extend_from_slice(&payload);
                        
                        println!("[Video Debug] First 32 bytes: {:02X?}", &msg[..std::cmp::min(msg.len(), 32)]);
                        println!("[Relay Video] Sending WebSocket binary frame: {} bytes", msg.len());

                        let mut lock =
                            match ws_arc_clone.lock() {
                                Ok(lock) => lock,

                                Err(_) => {
                                    break;
                                }
                            };

                        if lock
                            .send(
                                Message::Binary(msg)
                            )
                            .is_err()
                        {
                            break;
                        }
                    }

                    // ====================================================
                    // TYPE 13
                    //
                    // 1 byte type
                    // 10 byte header
                    // payload size = first 4 bytes
                    // ====================================================

                    13 => {
                        let mut header =
                            [0u8; 10];

                        if host_reader
                            .read_exact(&mut header)
                            .is_err()
                        {
                            break;
                        }

                        let mut size_buf =
                            [0u8; 4];

                        size_buf.copy_from_slice(
                            &header[0..4]
                        );

                        let payload_size =
                            u32::from_be_bytes(
                                size_buf
                            ) as usize;

                        if payload_size >
                            50 * 1024 * 1024
                        {
                            eprintln!(
                                "[Relay] Invalid type 13 payload size: {}",
                                payload_size
                            );

                            break;
                        }

                        let mut payload =
                            vec![0u8; payload_size];

                        if host_reader
                            .read_exact(&mut payload)
                            .is_err()
                        {
                            break;
                        }

                        let mut msg =
                            Vec::with_capacity(
                                1 +
                                10 +
                                payload_size
                            );

                        msg.push(13u8);

                        msg.extend_from_slice(
                            &header
                        );

                        msg.extend_from_slice(
                            &payload
                        );

                        let mut lock =
                            match ws_arc_clone.lock() {
                                Ok(lock) => lock,
                                Err(_) => break,
                            };

                        if lock
                            .send(
                                Message::Binary(msg)
                            )
                            .is_err()
                        {
                            break;
                        }
                    }

                    // ====================================================
                    // TYPE 16
                    //
                    // 1 byte type
                    // 2 byte payload size
                    // payload
                    // ====================================================

                    16 => {
                        let mut header =
                            [0u8; 2];

                        if host_reader
                            .read_exact(&mut header)
                            .is_err()
                        {
                            break;
                        }

                        let payload_size =
                            u16::from_be_bytes(
                                header
                            ) as usize;

                        let mut payload =
                            vec![0u8; payload_size];

                        if host_reader
                            .read_exact(&mut payload)
                            .is_err()
                        {
                            break;
                        }

                        let mut msg =
                            Vec::with_capacity(
                                1 +
                                2 +
                                payload_size
                            );

                        msg.push(16u8);

                        msg.extend_from_slice(
                            &header
                        );

                        msg.extend_from_slice(
                            &payload
                        );

                        let mut lock =
                            match ws_arc_clone.lock() {
                                Ok(lock) => lock,
                                Err(_) => break,
                            };

                        if lock
                            .send(
                                Message::Binary(msg)
                            )
                            .is_err()
                        {
                            break;
                        }
                    }

                    // ====================================================
                    // TYPE 12 - CLIPBOARD
                    //
                    // 1 byte type
                    // 4 byte payload size
                    // payload
                    // ====================================================

                    12 => {
                        let mut header =
                            [0u8; 4];

                        if host_reader
                            .read_exact(&mut header)
                            .is_err()
                        {
                            break;
                        }

                        let payload_size =
                            u32::from_be_bytes(
                                header
                            ) as usize;

                        if payload_size >
                            50 * 1024 * 1024
                        {
                            eprintln!(
                                "[Relay] Invalid clipboard payload size: {}",
                                payload_size
                            );

                            break;
                        }

                        let mut payload =
                            vec![0u8; payload_size];

                        if host_reader
                            .read_exact(&mut payload)
                            .is_err()
                        {
                            break;
                        }

                        let mut msg =
                            Vec::with_capacity(
                                1 +
                                4 +
                                payload_size
                            );

                        msg.push(12u8);

                        msg.extend_from_slice(
                            &header
                        );

                        msg.extend_from_slice(
                            &payload
                        );

                        let mut lock =
                            match ws_arc_clone.lock() {
                                Ok(lock) => lock,
                                Err(_) => break,
                            };

                        if lock
                            .send(
                                Message::Binary(msg)
                            )
                            .is_err()
                        {
                            break;
                        }
                    }

                    // ====================================================
                    // UNKNOWN
                    // ====================================================

                    _ => {
                        eprintln!(
                            "[Relay] Unknown packet type from host to viewer: {}",
                            packet_type
                        );

                        break;
                    }
                }
            }

            println!(
                "[Relay] WS HOST -> VIEWER thread closed."
            );
        });

    // --------------------------------------------------------
    // WEBSOCKET VIEWER -> HOST
    // --------------------------------------------------------

    loop {
        let msg_res = {
            let mut lock =
                match ws_arc.lock() {
                    Ok(lock) => lock,

                    Err(_) => break,
                };

            lock.read()
        };

        match msg_res {
            // ----------------------------------------------------
            // Binary message
            // ----------------------------------------------------

            Ok(Message::Binary(data)) => {
                if !send_all(
                    &mut host_writer,
                    &data,
                ) {
                    eprintln!(
                        "[Relay] Failed forwarding WS data to host."
                    );

                    break;
                }
            }

            // ----------------------------------------------------
            // Ping
            // ----------------------------------------------------

            Ok(Message::Ping(data)) => {
                let mut lock =
                    match ws_arc.lock() {
                        Ok(lock) => lock,
                        Err(_) => break,
                    };

                if lock
                    .send(Message::Pong(data))
                    .is_err()
                {
                    break;
                }
            }

            // ----------------------------------------------------
            // Pong
            // ----------------------------------------------------

            Ok(Message::Pong(_)) => {}

            // ----------------------------------------------------
            // Close
            // ----------------------------------------------------

            Ok(Message::Close(_)) => {
                println!(
                    "[Relay] WS viewer sent close."
                );

                break;
            }

            // ----------------------------------------------------
            // Text is not expected
            // ----------------------------------------------------

            Ok(Message::Text(_)) => {
                eprintln!(
                    "[Relay] Unexpected WS text message."
                );
            }

            // ----------------------------------------------------
            // Timeout / WouldBlock
            // ----------------------------------------------------

            Err(
                tungstenite::error::Error::Io(ref e)
            )
                if e.kind()
                    == std::io::ErrorKind::WouldBlock
                    || e.kind()
                        == std::io::ErrorKind::TimedOut =>
            {
                thread::sleep(
                    Duration::from_millis(1000)
                );

                continue;
            }

            // ----------------------------------------------------
            // Other errors
            // ----------------------------------------------------

            Err(e) => {
                eprintln!(
                    "[Relay] WS read error: {:?}",
                    e
                );

                break;
            }

            _ => {}
        }
    }

    // --------------------------------------------------------
    // Close host when WebSocket closes
    // --------------------------------------------------------

    let _ = host_writer.shutdown(
        Shutdown::Both
    );

    let _ = host.shutdown(
        Shutdown::Both
    );

    println!(
        "[Relay] WS Session {} closed.",
        session_id
    );
}

// ============================================================
// MAIN
// ============================================================

fn main() {
    println!("========================================");
    println!("       REMOTE DESKTOP RELAY SERVER");
    println!("========================================");

    println!(
        "  Listening on {}",
        RELAY_ADDR
    );

    println!("========================================");

    // --------------------------------------------------------
    // Bind TCP listener
    // --------------------------------------------------------

    let listener =
        TcpListener::bind(RELAY_ADDR)
            .expect(
                "Failed to bind relay server to port 9001"
            );

    // --------------------------------------------------------
    // Host registry
    // --------------------------------------------------------

    let hosts: ClientMap =
        Arc::new(
            Mutex::new(
                HashMap::new()
            )
        );

    // --------------------------------------------------------
    // Accept connections
    // --------------------------------------------------------

    for incoming in listener.incoming() {
        let stream = match incoming {
            Ok(stream) => stream,

            Err(e) => {
                eprintln!(
                    "[Relay] Incoming connection error: {:?}",
                    e
                );

                continue;
            }
        };

        let peer =
            stream.peer_addr()
                .map(|addr| addr.to_string())
                .unwrap_or_else(
                    |_| "unknown".to_string()
                );

        println!(
            "[Relay] New connection from {}",
            peer
        );

        let _ =
            stream.set_nodelay(true);

        // ----------------------------------------------------
        // Peek first byte without consuming it.
        // ----------------------------------------------------

        let mut peek =
            [0u8; 7];

        if stream
            .peek(&mut peek)
            .is_err()
        {
            eprintln!(
                "[Relay] Failed to inspect connection from {}",
                peer
            );

            continue;
        }

        let connection_type =
            peek[0];

        let hosts_clone =
            Arc::clone(&hosts);

        // ----------------------------------------------------
        // HOST
        //
        // 1 + 6 byte ID
        // ----------------------------------------------------

        match connection_type {
            1 => {
                println!(
                    "[Relay] Connection identified as HOST"
                );

                thread::spawn(
                    move || {
                        handle_host(
                            stream,
                            hosts_clone,
                        );
                    }
                );
            }

            // ------------------------------------------------
            // NORMAL TCP VIEWER
            //
            // 2 + 6 byte ID + 32 byte hash
            // ------------------------------------------------

            2 => {
                println!(
                    "[Relay] Connection identified as TCP VIEWER"
                );

                thread::spawn(
                    move || {
                        handle_viewer(
                            stream,
                            hosts_clone,
                        );
                    }
                );
            }

            // ------------------------------------------------
            // WEBSOCKET VIEWER
            //
            // First byte of HTTP request:
            //
            // G = GET
            // ASCII 71
            // ------------------------------------------------

            71 => {
                println!(
                    "[Relay] Connection identified as WEBSOCKET"
                );

                thread::spawn(
                    move || {
                        handle_websocket_viewer(
                            stream,
                            hosts_clone,
                        );
                    }
                );
            }

            // ------------------------------------------------
            // UNKNOWN
            // ------------------------------------------------

            _ => {
                eprintln!(
                    "[Relay] Unknown initial connection type: {}",
                    connection_type
                );

                let mut stream =
                    stream;

                let _ =
                    stream.shutdown(
                        Shutdown::Both
                    );
            }
        }
    }
}