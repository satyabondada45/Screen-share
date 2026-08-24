use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use tungstenite::accept;
use tungstenite::Message;

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

fn read_exact_bytes(stream: &mut TcpStream, buf: &mut [u8]) -> bool {
    stream.read_exact(buf).is_ok()
}

fn handle_websocket_client(mut websocket: tungstenite::WebSocket<TcpStream>, hosts: ClientMap) {
    let msg = match websocket.read() {
        Ok(m) => m,
        Err(_) => return,
    };

    let data = msg.into_data();
    if data.len() < 7 {
        return;
    }

    let conn_type = data[0];
    let session_id = match String::from_utf8(data[1..7].to_vec()) {
        Ok(id) => id,
        Err(_) => return,
    };

    if conn_type == 2 {
        let auth_hash = if data.len() >= 39 {
            &data[7..39]
        } else {
            &[0u8; 32]
        };

        let host_stream = {
            let mut map = hosts.lock().unwrap();
            map.remove(&session_id)
        };

        if let Some(mut host) = host_stream {
            let mut req_pkt = Vec::with_capacity(33);
            req_pkt.push(3u8);
            req_pkt.extend_from_slice(auth_hash);

            if host.write_all(&req_pkt).is_err() {
                let _ = websocket.send(Message::Binary(vec![3u8]));
                return;
            }

            let mut host_resp = [0u8; 1];
            if host.read_exact(&mut host_resp).is_err() || host_resp[0] != 1 {
                let _ = websocket.send(Message::Binary(vec![2u8]));
                return;
            }

            let _ = websocket.send(Message::Binary(vec![1u8])); // Approved

            let mut host_read = host.try_clone().unwrap();
            let mut host_write = host;
            websocket.get_mut().set_read_timeout(Some(std::time::Duration::from_millis(10))).unwrap();
            let ws_arc = Arc::new(Mutex::new(websocket));
            let ws_write = Arc::clone(&ws_arc);
            let ws_read = Arc::clone(&ws_arc);

            // Framed Host -> WebSocket bridge
            thread::spawn(move || {
                loop {
                    let mut type_buf = [0u8; 1];
                    if !read_exact_bytes(&mut host_read, &mut type_buf) { break; }
                    let pkt_type = type_buf[0];

                    match pkt_type {
                        0 => {
                            if let Ok(mut ws) = ws_write.lock() {
                                if ws.send(Message::Binary(vec![0u8])).is_err() { break; }
                            }
                        }
                        1 => {
                            let mut hdr = [0u8; 28];
                            if !read_exact_bytes(&mut host_read, &mut hdr) { break; }
                            let payload_size = u32::from_be_bytes(hdr[24..28].try_into().unwrap()) as usize;

                            let mut payload = vec![0u8; payload_size];
                            if !read_exact_bytes(&mut host_read, &mut payload) { break; }

                            let mut full_pkt = Vec::with_capacity(1 + 28 + payload_size);
                            full_pkt.push(1u8);
                            full_pkt.extend_from_slice(&hdr);
                            full_pkt.extend_from_slice(&payload);

                            if let Ok(mut ws) = ws_write.lock() {
                                if ws.send(Message::Binary(full_pkt)).is_err() { break; }
                            }
                        }
                        12 => {
                            let mut len_buf = [0u8; 4];
                            if !read_exact_bytes(&mut host_read, &mut len_buf) { break; }
                            let len = u32::from_be_bytes(len_buf) as usize;
                            let mut clip_data = vec![0u8; len];
                            if !read_exact_bytes(&mut host_read, &mut clip_data) { break; }
                        }
                        13 => {
                            let mut len_buf = [0u8; 4];
                            if !read_exact_bytes(&mut host_read, &mut len_buf) { break; }
                            let len = u32::from_be_bytes(len_buf) as usize;
                            let mut audio_data = vec![0u8; len];
                            if !read_exact_bytes(&mut host_read, &mut audio_data) { break; }
                        }
                        14 => {
                            let mut time_buf = [0u8; 8];
                            if !read_exact_bytes(&mut host_read, &mut time_buf) { break; }
                        }
                        16 => {
                            let mut meta = [0u8; 3];
                            if !read_exact_bytes(&mut host_read, &mut meta) { break; }
                            let len = u16::from_be_bytes([meta[1], meta[2]]) as usize;
                            let mut msg_data = vec![0u8; len];
                            if !read_exact_bytes(&mut host_read, &mut msg_data) { break; }
                        }
                        _ => {
                            break;
                        }
                    }
                }
            });

            // WebSocket -> Host (Input Injection & File Transfers)
            thread::spawn(move || loop {
                let msg = {
                    let mut ws = match ws_read.lock() {
                        Ok(guard) => guard,
                        Err(_) => break,
                    };
                    match ws.read() {
                        Ok(m) => Ok(m),
                        Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                            drop(ws);
                            thread::sleep(std::time::Duration::from_millis(5));
                            continue;
                        }
                        Err(e) => Err(e),
                    }
                };

                match msg {
                    Ok(Message::Binary(bin)) => {
                        if host_write.write_all(&bin).is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            });
        } else {
            let _ = websocket.send(Message::Binary(vec![3u8]));
        }
    }
}

fn handle_connection(mut stream: TcpStream, hosts: ClientMap) {
    let mut peek_buf = [0u8; 3];
    if stream.peek(&mut peek_buf).is_err() {
        return;
    }

    if &peek_buf == b"GET" {
        if let Ok(ws) = accept(stream) {
            handle_websocket_client(ws, hosts);
        }
        return;
    }

    let mut init_buf = [0u8; 7];
    if stream.read_exact(&mut init_buf).is_err() {
        return;
    }

    let conn_type = init_buf[0];
    let session_id = match String::from_utf8(init_buf[1..7].to_vec()) {
        Ok(id) => id,
        Err(_) => return,
    };

    match conn_type {
        1 => {
            println!("[Relay] Host registered for session ID: {}", session_id);
            if let Ok(mut map) = hosts.lock() {
                map.insert(session_id.clone(), stream);
            }
        }
        2 => {
            let mut auth_hash = [0u8; 32];
            if stream.read_exact(&mut auth_hash).is_err() {
                return;
            }

            let host_stream = {
                let mut map = hosts.lock().unwrap();
                map.remove(&session_id)
            };

            if let Some(mut host) = host_stream {
                let mut req_pkt = Vec::with_capacity(33);
                req_pkt.push(3u8);
                req_pkt.extend_from_slice(&auth_hash);

                if host.write_all(&req_pkt).is_err() {
                    let _ = stream.write_all(&[3u8]);
                    return;
                }

                let mut host_resp = [0u8; 1];
                if host.read_exact(&mut host_resp).is_err() {
                    let _ = stream.write_all(&[3u8]);
                    return;
                }

                if host_resp[0] == 1 {
                    let _ = stream.write_all(&[1u8]);
                    let host_read = host.try_clone().unwrap();
                    let host_write = host;
                    let viewer_read = stream.try_clone().unwrap();
                    let viewer_write = stream;

                    thread::spawn(move || proxy_stream(host_read, viewer_write));
                    thread::spawn(move || proxy_stream(viewer_read, host_write));
                } else {
                    let _ = stream.write_all(&[2u8]);
                }
            } else {
                let _ = stream.write_all(&[3u8]);
            }
        }
        _ => {}
    }
}

fn main() {
    let listener = TcpListener::bind("0.0.0.0:9001").expect("Failed to bind relay server to port 9001");
    println!("========================================");
    println!("  Framed Relay Server Active on Port 9001");
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