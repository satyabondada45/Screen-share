use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::status;
use crate::status::AgentStatus;

pub struct BackendClient {
    base_url: String,
    machine_identifier: String,
    system_id: String,
}

impl BackendClient {
    pub fn new(base_url: &str, machine_identifier: &str, system_id: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            machine_identifier: machine_identifier.to_string(),
            system_id: system_id.to_string(),
        }
    }

    /// Registers the host agent machine with the PHP MySQL backend.
    /// Returns the permanent 9-digit System ID assigned by the database.
    pub fn register(&mut self) -> Option<String> {
        let hostname = env::var("COMPUTERNAME")
            .or_else(|_| env::var("HOSTNAME"))
            .unwrap_or_else(|_| "Desktop-PC".to_string());

        let mut payload = HashMap::new();
        payload.insert("machine_identifier", self.machine_identifier.clone());
        payload.insert("hostname", hostname);
        payload.insert("os_info", env::consts::OS.to_string());
        if !self.system_id.is_empty() {
            payload.insert("system_id", self.system_id.clone());
        }

        println!("[System Registration]");
        println!("  Device UUID: {}", self.machine_identifier);
        println!("  System ID:   {}", if self.system_id.is_empty() { "Pending..." } else { &self.system_id });

        let client = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[System Registration][ERROR] Failed to create HTTP client: {:?}", e);
                return None;
            }
        };

        let url = format!("{}/devices/register.php", self.base_url.trim_end_matches('/'));
        println!("  Attempting Backend URL: {}", url);

        match client.post(&url).json(&payload).send() {
            Ok(res) => {
                let status = res.status();
                if status.is_success() {
                    if let Ok(json) = res.json::<serde_json::Value>() {
                        if let Some(sys) = json.get("system") {
                            if let Some(sid) = sys.get("system_id").and_then(|v| v.as_str()) {
                                self.system_id = sid.to_string();
                                println!("[System Registration] SUCCESS -> Assigned System ID: {}", sid);
                                return Some(sid.to_string());
                            }
                        }
                        eprintln!("[System Registration][ERROR] Response missing 'system.system_id' field");
                    }
                } else {
                    eprintln!("[System Registration][ERROR] HTTP {}", status);
                }
            }
            Err(e) => {
                eprintln!("[System Registration][ERROR] Failed for {}: {:?}", url, e);
            }
        }

        None
    }

    /// Spawns a background heartbeat thread that pings MySQL every 3 seconds.
    /// Publishes the backend connection state into the shared live status so the
    /// Screen Share GUI can display it.
    pub fn start_heartbeat_thread(&self, status: Arc<Mutex<AgentStatus>>) {
        let base_url = self.base_url.clone();
        let machine_id = self.machine_identifier.clone();
        let sys_id = self.system_id.clone();

        thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());

            let mut payload = HashMap::new();
            payload.insert("machine_identifier", machine_id.clone());
            payload.insert("system_id", sys_id.clone());

            let url = format!("{}/devices/heartbeat.php", base_url.trim_end_matches('/'));

            let mut count: u64 = 0;
            loop {
                match client.post(&url).json(&payload).send() {
                    Ok(res) if res.status().is_success() => {
                        if let Ok(mut g) = status.lock() {
                            g.backend_connected = true;
                        }
                        count += 1;
                        if count == 1 {
                            println!("[HEARTBEAT] Active for device_id: {}, machine: {} -> HTTP {}", sys_id, machine_id, res.status());
                            println!("[HEARTBEAT] device_id={}", sys_id);
                        }
                        if count % 10 == 0 {
                            println!("[HEARTBEAT] device_id={} (ping #{})", sys_id, count);
                        }
                        status::touch_heartbeat();
                    }
                    Err(e) => {
                        if count < 3 {
                            eprintln!("[HEARTBEAT][ERROR] Failed for {}: {:?}", url, e);
                        }
                        if let Ok(mut g) = status.lock() {
                            g.backend_connected = false;
                        }
                    }
                    _ => {}
                }
                thread::sleep(Duration::from_secs(3));
            }
        });
    }

    /// Logs the start of an active remote session in the database.
    pub fn log_session_start(&self, session_key: &str) {
        let url = format!("{}/sessions/start.php", self.base_url);
        let mut payload = HashMap::new();
        payload.insert("device_id", self.system_id.clone());
        payload.insert("session_key", session_key.to_string());

        let client = reqwest::blocking::Client::new();
        let _ = client.post(&url).json(&payload).send();
    }

    /// Records session conclusion upon disconnection.
    pub fn log_session_end(&self, session_key: &str, bandwidth_mb: f64) {
        let url = format!("{}/sessions/end.php", self.base_url);
        let mut payload = HashMap::new();
        payload.insert("session_key", session_key.to_string());
        payload.insert("bandwidth_used_mb", bandwidth_mb.to_string());

        let client = reqwest::blocking::Client::new();
        let _ = client.post(&url).json(&payload).send();
    }
}