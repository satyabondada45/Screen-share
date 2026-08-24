use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

type ClientMap = Arc<Mutex<HashMap<String, TcpStream>>>;

fn proxy_stream(mut reader: TcpStream, mut writer: TcpStream) {
    let mut buffer = [0u8; 65536];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if writer.write_all(&buffer[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut stream: TcpStream, hosts: ClientMap) {
    let mut init_buf = [0u8; 7]; // 1B Type + 6B ID
    if stream.read_exact(&mut init_buf).is_err() {
        return;
    }

    let conn_type = init_buf[0];
    let session_id = match String::from_utf8(init_buf[1..7].to_vec()) {
        Ok(id) => id,
        Err(_) => return,
    };

    match conn_type {
        // 1: Host Agent Registration
        1 => {
            println!("[Relay] Host registered for session ID: {}", session_id);
            if let Ok(mut map) = hosts.lock() {
                map.insert(session_id.clone(), stream);
            }
        }
        // 2: Viewer Connection Request
        2 => {
            let mut auth_hash = [0u8; 32];
            if stream.read_exact(&mut auth_hash).is_err() {
                return;
            }

            println!("[Relay] Viewer connecting to host: {}", session_id);
            let host_stream = {
                let mut map = hosts.lock().unwrap();
                map.remove(&session_id)
            };

            if let Some(mut host) = host_stream {
                // Forward viewer connect request + auth hash to host
                let mut req_pkt = Vec::with_capacity(33);
                req_pkt.push(3u8);
                req_pkt.extend_from_slice(&auth_hash);

                if host.write_all(&req_pkt).is_err() {
                    let _ = stream.write_all(&[3u8]); // Offline
                    return;
                }

                // Read authorization response from host
                let mut host_resp = [0u8; 1];
                if host.read_exact(&mut host_resp).is_err() {
                    let _ = stream.write_all(&[3u8]);
                    return;
                }

                if host_resp[0] == 1 {
                    let _ = stream.write_all(&[1u8]); // Approved

                    let host_read = host.try_clone().unwrap();
                    let host_write = host;
                    let viewer_read = stream.try_clone().unwrap();
                    let viewer_write = stream;

                    // Host -> Viewer (Screen, Audio, Clipboard, Chat, Ping)
                    thread::spawn(move || {
                        proxy_stream(host_read, viewer_write);
                    });

                    // Viewer -> Host (Input, Scroll, File, Chat, Pong)
                    thread::spawn(move || {
                        proxy_stream(viewer_read, host_write);
                    });

                    println!("[Relay] Live session bridge active for ID: {}", session_id);
                } else {
                    let _ = stream.write_all(&[2u8]); // Rejected / Invalid PIN
                }
            } else {
                let _ = stream.write_all(&[3u8]); // Host not found
            }
        }
        _ => {}
    }
}

fn main() {
    let listener = TcpListener::bind("0.0.0.0:9001").expect("Failed to bind relay server to port 9001");
    println!("========================================");
    println!("  Screen Share TCP Relay Server Active");
    println!("  Listening on port 9001");
    println!("========================================");

    let hosts: ClientMap = Arc::new(Mutex::new(HashMap::new()));

    for stream in listener.incoming() {
        if let Ok(s) = stream {
            let _ = s.set_nodelay(true);
            let hosts_clone = Arc::clone(&hosts);
            thread::spawn(move || {
                handle_connection(s, hosts_clone);
            });
        }
    }
}