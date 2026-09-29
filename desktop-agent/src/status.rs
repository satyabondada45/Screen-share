use std::env;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, SyncSender};
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
    /// True while a real TCP relay session is active (B is streaming to A).
    /// Set by the agent loop when the Type-3 auth completes successfully.
    pub in_session: bool,
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
            in_session: false,
        }
    }
}

pub static LIVE: OnceLock<Arc<Mutex<AgentStatus>>> = OnceLock::new();
static SESSION_WRITER: OnceLock<Mutex<Option<SyncSender<Vec<u8>>>>> = OnceLock::new();
static SESSION_CHAT_MESSAGES: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static SESSION_FILE_OFFERS: OnceLock<Mutex<HashMap<u64, SessionFileOffer>>> = OnceLock::new();
static SESSION_ACCEPTED_FILES: OnceLock<Mutex<HashMap<u64, bool>>> = OnceLock::new();
static SESSION_FILE_RESPONSES: OnceLock<Mutex<HashMap<u64, mpsc::Sender<u8>>>> = OnceLock::new();

#[derive(Clone)]
pub struct SessionFileOffer {
    pub transfer_id: u64,
    pub filename: String,
    pub size: u64,
}

fn session_writer() -> &'static Mutex<Option<SyncSender<Vec<u8>>>> {
    SESSION_WRITER.get_or_init(|| Mutex::new(None))
}

pub fn set_session_writer(writer: Option<SyncSender<Vec<u8>>>) -> Result<(), String> {
    *session_writer()
        .lock()
        .map_err(|_| "session writer lock poisoned".to_string())? = writer.clone();
    if writer.is_some() {
        SESSION_CHAT_MESSAGES
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock()
            .map_err(|_| "chat queue lock poisoned".to_string())?
            .clear();
        SESSION_FILE_OFFERS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "file offer lock poisoned".to_string())?
            .clear();
        SESSION_ACCEPTED_FILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "accepted file lock poisoned".to_string())?
            .clear();
    } else {
        let pending_responses = SESSION_FILE_RESPONSES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "file response lock poisoned".to_string())?
            .drain()
            .map(|(_, sender)| sender)
            .collect::<Vec<_>>();
        for sender in pending_responses {
            let _ = sender.send(23);
        }
        SESSION_CHAT_MESSAGES
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock()
            .map_err(|_| "chat queue lock poisoned".to_string())?
            .clear();
        SESSION_FILE_OFFERS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "file offer lock poisoned".to_string())?
            .clear();
        SESSION_ACCEPTED_FILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "accepted file lock poisoned".to_string())?
            .clear();
    }
    Ok(())
}

pub fn send_session_packet(packet: Vec<u8>) -> Result<(), String> {
    let writer = session_writer()
        .lock()
        .map_err(|_| "session writer lock poisoned".to_string())?
        .clone()
        .ok_or_else(|| "no active session".to_string())?;
    writer.send(packet).map_err(|_| "session writer disconnected".to_string())
}

pub fn push_session_chat(message: String) -> Result<(), String> {
    let mut messages = SESSION_CHAT_MESSAGES
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .map_err(|_| "chat queue lock poisoned".to_string())?;
    if messages.len() == 500 {
        messages.pop_front();
    }
    messages.push_back(message);
    Ok(())
}

pub fn take_session_chat() -> Result<Vec<String>, String> {
    SESSION_CHAT_MESSAGES
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .map(|mut messages| messages.drain(..).collect())
        .map_err(|_| "chat queue lock poisoned".to_string())
}

pub fn push_session_file_offer(offer: SessionFileOffer) -> Result<(), String> {
    let mut offers = SESSION_FILE_OFFERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "file offer lock poisoned".to_string())?;
    offers.insert(offer.transfer_id, offer);
    Ok(())
}

pub fn session_file_offers() -> Result<Vec<SessionFileOffer>, String> {
    SESSION_FILE_OFFERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map(|offers| offers.values().cloned().collect())
        .map_err(|_| "file offer lock poisoned".to_string())
}

pub fn decide_session_file(transfer_id: u64, accept: bool) -> Result<(), String> {
    let offer = SESSION_FILE_OFFERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "file offer lock poisoned".to_string())?
        .remove(&transfer_id)
        .ok_or_else(|| "file offer is no longer pending".to_string())?;

    if accept {
        SESSION_ACCEPTED_FILES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "accepted file lock poisoned".to_string())?
            .insert(transfer_id, true);
    }

    let mut packet = Vec::with_capacity(9);
    packet.push(if accept { 26 } else { 25 });
    packet.extend_from_slice(&transfer_id.to_be_bytes());
    if let Err(error) = send_session_packet(packet) {
        if accept {
            finish_session_file(transfer_id);
        }
        push_session_file_offer(offer);
        return Err(error);
    }
    Ok(())
}

pub fn session_file_is_accepted(transfer_id: u64) -> Result<bool, String> {
    SESSION_ACCEPTED_FILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map(|accepted| accepted.contains_key(&transfer_id))
        .map_err(|_| "accepted file lock poisoned".to_string())
}

pub fn finish_session_file(transfer_id: u64) {
    if let Ok(mut accepted) = SESSION_ACCEPTED_FILES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        accepted.remove(&transfer_id);
    }
}

pub fn register_session_file_response(transfer_id: u64) -> Receiver<u8> {
    let (tx, rx) = mpsc::channel();
    if let Ok(mut responses) = SESSION_FILE_RESPONSES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        responses.insert(transfer_id, tx);
    }
    rx
}

pub fn clear_session_file_response(transfer_id: u64) {
    if let Ok(mut responses) = SESSION_FILE_RESPONSES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        responses.remove(&transfer_id);
    }
}

pub fn resolve_session_file_response(transfer_id: u64, response: u8) {
    if let Ok(mut responses) = SESSION_FILE_RESPONSES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        let sender = responses.get(&transfer_id).cloned();
        if response == 23 || response == 24 || response == 25 || response == 27 {
            responses.remove(&transfer_id);
        }
        if let Some(sender) = sender {
            let _ = sender.send(response);
        }
    }
}

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

/// Called by the agent loop when a real session becomes active (B starts streaming).
pub fn set_session_active(active: bool) {
    if let Some(s) = LIVE.get() {
        if let Ok(mut g) = s.lock() {
            g.in_session = active;
        }
    }
}
