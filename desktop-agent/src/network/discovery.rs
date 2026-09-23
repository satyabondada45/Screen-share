use std::net::UdpSocket;
use std::thread;
use std::time::Duration;

pub fn start_discovery_listener(system_id: String) {
    thread::spawn(move || {
        let socket = match UdpSocket::bind("0.0.0.0:49183") {
            Ok(s) => s,
            Err(e) => {
                println!("[DISCOVERY] Failed to bind UDP 49183: {:?}", e);
                return;
            }
        };

        println!("[DISCOVERY] Listening for broadcast on UDP 49183");

        let mut buf = [0u8; 1024];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((amt, src)) => {
                    let msg = String::from_utf8_lossy(&buf[..amt]);
                    if msg.starts_with("DS_DISCOVER:") {
                        let target_id = msg.trim_start_matches("DS_DISCOVER:").trim();
                        if target_id == system_id {
                            println!("[DISCOVERY] Received broadcast for our ID from {}", src);
                            let response = format!("DS_FOUND:{}", system_id);
                            let _ = socket.send_to(response.as_bytes(), src);
                        }
                    }
                }
                Err(e) => {
                    println!("[DISCOVERY] Error receiving UDP: {:?}", e);
                }
            }
        }
    });
}

pub fn discover_target_agent(target_id: &str) -> Option<(String, u16)> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_broadcast(true).ok()?;
    socket.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    
    let msg = format!("DS_DISCOVER:{}", target_id);
    // Broadcast to the subnet
    socket.send_to(msg.as_bytes(), "255.255.255.255:49183").ok()?;

    let mut buf = [0u8; 1024];
    if let Ok((amt, src)) = socket.recv_from(&mut buf) {
        let response = String::from_utf8_lossy(&buf[..amt]);
        if response.starts_with("DS_FOUND:") {
            let id = response.trim_start_matches("DS_FOUND:").trim();
            if id == target_id {
                return Some((src.ip().to_string(), 49184));
            }
        }
    }
    None
}
