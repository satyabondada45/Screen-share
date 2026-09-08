use std::env;
use std::sync::{Arc, Mutex, OnceLock};

/// Live, process-wide status published by the agent and consumed by the
/// Screen Share GUI through the local health server (127.0.0.1:49182).
///
/// This is the ONLY IPC channel between the GUI and the agent. The GUI never
/// opens a relay/backend connection of its own; it only reads this status.
#[derive(Clone, Default)]
pub struct AgentStatus {
    pub system_id: String,
    pub status: String, // "online" | "connecting" | "offline" | "error"
    pub backend_connected: bool,
    pub relay_connected: bool,
    pub last_heartbeat_ms: u64,
    pub device_name: String,
    pub version: String,
    pub environment: String,
}

impl AgentStatus {
    pub fn new(system_id: &str, device_name: &str) -> Self {
        let environment = env::var("SCREENSHARE_ENV")
            .unwrap_or_else(|_| "Production".to_string());
        AgentStatus {
            system_id: system_id.to_string(),
            status: "connecting".to_string(),
            backend_connected: false,
            relay_connected: false,
            last_heartbeat_ms: 0,
            device_name: device_name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            environment,
        }
    }
}

pub static LIVE: OnceLock<Arc<Mutex<AgentStatus>>> = OnceLock::new();

/// Returns the shared status handle. Panics only if called before initialization
/// (which happens in `main` before any thread is spawned).
pub fn live() -> Arc<Mutex<AgentStatus>> {
    LIVE.get().expect("AgentStatus not initialized").clone()
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Used by the relay connection loop to flip relay connectivity and overall
/// online/offline status without touching the video pipeline.
pub fn set_relay_state(connected: bool) {
    if let Some(s) = LIVE.get() {
        if let Ok(mut g) = s.lock() {
            g.relay_connected = connected;
            if connected {
                g.status = "online".to_string();
                g.last_heartbeat_ms = now_ms();
            } else {
                g.status = "offline".to_string();
            }
        }
    }
}

/// Called whenever a heartbeat packet is sent/received so the GUI can show the
/// "Last heartbeat" relative time.
pub fn touch_heartbeat() {
    if let Some(s) = LIVE.get() {
        if let Ok(mut g) = s.lock() {
            g.last_heartbeat_ms = now_ms();
        }
    }
}
