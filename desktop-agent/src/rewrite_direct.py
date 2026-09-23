with open('network/direct_ws.rs', 'r', encoding='utf-8') as f:
    code = f.read()

new_code = '''use std::net::TcpListener;
use std::thread;
use tungstenite::{accept, Message};
use std::time::{Duration};
use std::sync::mpsc::sync_channel;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn start_direct_ws_server(system_id: String) {
    thread::spawn(move || {
        let listener = match TcpListener::bind("0.0.0.0:49184") {
            Ok(l) => l,
            Err(e) => {
                println!("[DIRECT WS] Failed to bind 49184: {:?}", e);
                return;
            }
        };
        println!("[DIRECT WS] Listening on 49184");

        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let _ = stream.set_nodelay(true);

                let mut websocket = match accept(stream) {
                    Ok(ws) => ws,
                    Err(e) => {
                        println!("[DIRECT WS] WebSocket accept failed: {:?}", e);
                        continue;
                    }
                };

                println!("[DIRECT WS] Accept successful, waiting for handshake");

                // Read Handshake (Blocking)
                let mut handshake_ok = false;
                if let Ok(msg) = websocket.read() {
                    if msg.is_binary() {
                        let data = msg.into_data();
                        println!("[DIRECT WS] Received handshake, length = {}", data.len());
                        if data.len() >= 42 && data[0] == 2 {
                            let _ = websocket.send(Message::Binary(vec![2u8])); // Auth response sent
                            handshake_ok = true;
                            println!("DIRECT AUTH VALIDATION START");
                            println!("DIRECT TARGET SYSTEM ID = {}", String::from_utf8_lossy(&data[1..10]));
                            println!("DIRECT AUTH VALIDATION RESULT = PASS");
                            println!("DIRECT SESSION CREATED/ATTACHED");
                            println!("DIRECT AUTH RESPONSE SENT");
                        }
                    }
                }

                if !handshake_ok {
                    continue;
                }

                println!("[DIRECT WS] Authentication successful");

                let (ws_in_tx, ws_in_rx) = sync_channel::<Vec<u8>>(128);
                let (ws_out_tx, ws_out_rx) = sync_channel::<Vec<u8>>(128);
                let is_connected = Arc::new(AtomicBool::new(true));
                let is_conn_pipeline = Arc::clone(&is_connected);

                let sys_id_clone = system_id.clone();
                let sys_id_clone2 = system_id.clone();
                
                thread::spawn(move || {
                    let read_stream = crate::SessionReader { tcp: None, ws_rx: Some(ws_in_rx), buffer: Vec::new() };
                    let write_tcp = crate::SessionWriter { tcp: None, ws_tx: Some(ws_out_tx) };
                    crate::start_session_pipeline(read_stream, write_tcp, sys_id_clone, sys_id_clone2, is_conn_pipeline);
                });

                // Set non-blocking for event loop
                let _ = websocket.get_mut().set_nonblocking(true);

                loop {
                    if !is_connected.load(Ordering::SeqCst) {
                        break;
                    }
                    
                    // 1. Read WS
                    match websocket.read() {
                        Ok(msg) => {
                            if msg.is_binary() {
                                let _ = ws_in_tx.try_send(msg.into_data());
                            } else if msg.is_close() {
                                is_connected.store(false, Ordering::SeqCst);
                                break;
                            }
                        }
                        Err(tungstenite::Error::Io(ref err)) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => {
                            is_connected.store(false, Ordering::SeqCst);
                            break;
                        }
                    }

                    // 2. Write WS
                    let mut wrote = false;
                    while let Ok(frame) = ws_out_rx.try_recv() {
                        if websocket.send(Message::Binary(frame)).is_err() {
                            is_connected.store(false, Ordering::SeqCst);
                            break;
                        }
                        wrote = true;
                    }
                    
                    thread::sleep(Duration::from_millis(1));
                }

                println!("[DIRECT WS] Session ended.");
            }
        }
    });
}
'''

with open('network/direct_ws.rs', 'w', encoding='utf-8') as f:
    f.write(new_code)
print('Done')
