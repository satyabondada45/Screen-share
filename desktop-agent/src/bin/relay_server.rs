use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

struct AgentSession {
    viewer_tx: Sender<TcpStream>,
}

type AgentRegistry = Arc<Mutex<HashMap<String, AgentSession>>>;

fn pipe_stream(mut reader: TcpStream, mut writer: TcpStream) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if writer.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = writer.shutdown(std::net::Shutdown::Both);
}

fn main() {
    let server = TcpListener::bind("0.0.0.0:9001").expect("Failed to bind port 9001");
    println!("========================================");
    println!("  RELAY SERVER READY: 0.0.0.0:9001");
    println!("========================================");

    let registry: AgentRegistry = Arc::new(Mutex::new(HashMap::new()));

    for stream in server.incoming() {
        let mut stream = match stream {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                s
            }
            Err(_) => continue,
        };

        let reg = Arc::clone(&registry);

        thread::spawn(move || {
            let mut handshake = [0u8; 7];
            if stream.read_exact(&mut handshake).is_err() {
                return;
            }

            let action = handshake[0];
            let peer_id = match std::str::from_utf8(&handshake[1..7]) {
                Ok(s) => s.to_string(),
                Err(_) => return,
            };

            if action == 1 {
                // Agent Registration
                println!("[Relay] Agent registered Session ID: {}", peer_id);
                let (viewer_tx, viewer_rx): (Sender<TcpStream>, Receiver<TcpStream>) = channel();

                {
                    let mut map = reg.lock().unwrap();
                    map.insert(peer_id.clone(), AgentSession { viewer_tx });
                }

                if let Ok(mut viewer_stream) = viewer_rx.recv() {
                    // Send Connection Request Flag (Type 3) to Agent
                    if stream.write_all(&[3u8]).is_err() {
                        let _ = viewer_stream.write_all(&[0u8]);
                        return;
                    }

                    // Read Agent Decision: 1 = Accept, 0 = Reject
                    let mut decision = [0u8; 1];
                    if stream.read_exact(&mut decision).is_err() || decision[0] != 1 {
                        println!("[Relay] Session {} rejected by host.", peer_id);
                        let _ = viewer_stream.write_all(&[2u8]);
                        return;
                    }

                    // Acknowledge connection to Viewer (1 = Approved)
                    let _ = viewer_stream.write_all(&[1u8]);
                    println!("[Relay] Session {} approved! Bridging streams...", peer_id);

                    let v_read = match viewer_stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let v_write = viewer_stream;

                    let a_read = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let a_write = stream;

                    let h1 = thread::spawn(move || pipe_stream(a_read, v_write));
                    let h2 = thread::spawn(move || pipe_stream(v_read, a_write));

                    let _ = h1.join();
                    let _ = h2.join();
                    println!("[Relay] Session {} closed.", peer_id);
                }

                let mut map = reg.lock().unwrap();
                map.remove(&peer_id);
            } else if action == 2 {
                // Viewer Route Request
                println!("[Relay] Viewer connecting to ID: {}", peer_id);
                let target_tx = {
                    let map = reg.lock().unwrap();
                    map.get(&peer_id).map(|s| s.viewer_tx.clone())
                };

                if let Some(tx) = target_tx {
                    let _ = tx.send(stream);
                } else {
                    let _ = stream.write_all(&[0u8]);
                    println!("[Relay] Session {} not online.", peer_id);
                }
            }
        });
    }
}