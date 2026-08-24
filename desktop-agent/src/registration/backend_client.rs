use std::collections::HashMap;
use std::env;
use std::thread;
use std::time::Duration;

pub struct BackendClient {
    base_url: String,
    device_id: String,
}

impl BackendClient {
    pub fn new(base_url: &str, device_id: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            device_id: device_id.to_string(),
        }
    }

    pub fn register(&self) -> bool {
        let url = format!("{}/devices/register.php", self.base_url);
        let hostname = env::var("COMPUTERNAME")
            .or_else(|_| env::var("HOSTNAME"))
            .unwrap_or_else(|_| "RustDevice".to_string());

        let mut payload = HashMap::new();
        payload.insert("device_id", self.device_id.clone());
        payload.insert("os_info", env::consts::OS.to_string());
        payload.insert("hostname", hostname);

        let client = reqwest::blocking::Client::new();
        match client.post(&url).json(&payload).send() {
            Ok(res) => res.status().is_success(),
            Err(e) => {
                eprintln!("[Backend Sync] Registration failed: {:?}", e);
                false
            }
        }
    }

    pub fn start_heartbeat_thread(&self) {
        let url = format!("{}/devices/heartbeat.php", self.base_url);
        let dev_id = self.device_id.clone();

        thread::spawn(move || {
            let client = reqwest::blocking::Client::new();
            let mut payload = HashMap::new();
            payload.insert("device_id", dev_id);

            loop {
                let _ = client.post(&url).json(&payload).send();
                thread::sleep(Duration::from_secs(30));
            }
        });
    }

    pub fn log_session_start(&self, session_key: &str) {
        let url = format!("{}/sessions/start.php", self.base_url);
        let mut payload = HashMap::new();
        payload.insert("device_id", self.device_id.clone());
        payload.insert("session_key", session_key.to_string());

        let client = reqwest::blocking::Client::new();
        let _ = client.post(&url).json(&payload).send();
    }
}