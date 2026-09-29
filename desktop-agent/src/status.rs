use std::env;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const CHAT_TYPING_PACKET_TYPE: u8 = 18;
pub const CHAT_TYPING_STOP: u8 = 0;
pub const CHAT_TYPING_START: u8 = 1;

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

pub fn set_session_remote_typing(typing: bool) {
    SESSION_REMOTE_TYPING.store(typing, Ordering::Release);
}

pub fn session_remote_typing() -> bool {
    SESSION_REMOTE_TYPING.load(Ordering::Acquire)
}

pub fn set_session_audio_enabled(enabled: bool) {
    SESSION_AUDIO_ENABLED.store(enabled, Ordering::Release);
}

pub fn session_audio_enabled() -> bool {
    SESSION_AUDIO_ENABLED.load(Ordering::Acquire)
}

pub fn set_session_reverse_request_pending() {
    SESSION_REVERSE_REQUEST.store(true, Ordering::Release);
}

pub fn take_session_reverse_request() -> bool {
    SESSION_REVERSE_REQUEST.swap(false, Ordering::AcqRel)
}

pub fn set_session_reverse_decision(accepted: bool) {
    SESSION_REVERSE_DECISION.store(if accepted { 1 } else { 2 }, Ordering::Release);
}

pub fn take_session_reverse_decision() -> u8 {
    SESSION_REVERSE_DECISION.swap(0, Ordering::AcqRel)
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
static SESSION_CHAT_HISTORY: OnceLock<Mutex<SessionChatHistory>> = OnceLock::new();
static SESSION_REMOTE_TYPING: AtomicBool = AtomicBool::new(false);
static SESSION_AUDIO_ENABLED: AtomicBool = AtomicBool::new(false);
static SESSION_REVERSE_REQUEST: AtomicBool = AtomicBool::new(false);
static SESSION_REVERSE_DECISION: AtomicU8 = AtomicU8::new(0);
static SESSION_FILE_OFFERS: OnceLock<Mutex<HashMap<u64, SessionFileOffer>>> = OnceLock::new();
static SESSION_ACCEPTED_FILES: OnceLock<Mutex<HashMap<u64, bool>>> = OnceLock::new();
static SESSION_FILE_RESPONSES: OnceLock<Mutex<HashMap<u64, mpsc::Sender<u8>>>> = OnceLock::new();

#[derive(Clone)]
pub struct SessionChatMessage {
    pub id: u64,
    pub text: String,
    pub from_local: bool,
}

#[derive(Default)]
struct SessionChatHistory {
    next_id: u64,
    messages: VecDeque<SessionChatMessage>,
}

impl SessionChatHistory {
    fn push(&mut self, text: String, from_local: bool) -> Result<SessionChatMessage, String> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| "chat message id exhausted".to_string())?;
        let message = SessionChatMessage {
            id: self.next_id,
            text,
            from_local,
        };
        if self.messages.len() == 500 {
            self.messages.pop_front();
        }
        self.messages.push_back(message.clone());
        Ok(message)
    }

    fn since(&self, after_id: u64) -> Vec<SessionChatMessage> {
        self.messages
            .iter()
            .filter(|message| message.id > after_id)
            .cloned()
            .collect()
    }
}

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
    {
        let mut history = SESSION_CHAT_HISTORY
            .get_or_init(|| Mutex::new(SessionChatHistory::default()))
            .lock()
            .map_err(|_| "chat history lock poisoned".to_string())?;
        history.messages.clear();
        history.next_id = 0;
    }
    SESSION_REMOTE_TYPING.store(false, Ordering::Release);
    SESSION_AUDIO_ENABLED.store(false, Ordering::Release);
    SESSION_REVERSE_REQUEST.store(false, Ordering::Release);
    SESSION_REVERSE_DECISION.store(0, Ordering::Release);
    if writer.is_some() {
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

pub fn push_session_chat(message: String, from_local: bool) -> Result<SessionChatMessage, String> {
    let mut history = SESSION_CHAT_HISTORY
        .get_or_init(|| Mutex::new(SessionChatHistory::default()))
        .lock()
        .map_err(|_| "chat history lock poisoned".to_string())?;
    history.push(message, from_local)
}

pub fn session_chat_since(after_id: u64) -> Result<Vec<SessionChatMessage>, String> {
    SESSION_CHAT_HISTORY
        .get_or_init(|| Mutex::new(SessionChatHistory::default()))
        .lock()
        .map(|history| history.since(after_id))
        .map_err(|_| "chat history lock poisoned".to_string())
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
        if let Err(requeue_error) = push_session_file_offer(offer) {
            return Err(format!(
                "{}; failed to restore pending file offer: {}",
                error, requeue_error
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::{
        session_audio_enabled, set_session_audio_enabled, SessionChatHistory,
        CHAT_TYPING_PACKET_TYPE, CHAT_TYPING_START, CHAT_TYPING_STOP,
    };

    #[test]
    fn session_chat_history_is_replayable_and_deduplicable_by_id() {
        let mut history = SessionChatHistory::default();
        let first = history.push("Hello".to_string(), false).unwrap();
        let second = history.push("Hyderabad 🙂".to_string(), true).unwrap();

        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
        assert_eq!(history.since(0).len(), 2);
        assert_eq!(history.since(first.id)[0].text, "Hyderabad 🙂");
        assert_eq!(history.since(second.id).len(), 0);
        assert!(!first.from_local);
        assert!(second.from_local);
    }

    #[test]
    fn session_chat_history_keeps_only_the_latest_500_messages() {
        let mut history = SessionChatHistory::default();
        for index in 0..501 {
            history.push(index.to_string(), false).unwrap();
        }

        let messages = history.since(0);
        assert_eq!(messages.len(), 500);
        assert_eq!(messages[0].id, 2);
        assert_eq!(messages[499].id, 501);
    }

    #[test]
    fn chat_typing_signal_uses_a_dedicated_two_byte_packet() {
        assert_eq!([CHAT_TYPING_PACKET_TYPE, CHAT_TYPING_START], [18, 1]);
        assert_eq!([CHAT_TYPING_PACKET_TYPE, CHAT_TYPING_STOP], [18, 0]);
    }

    #[test]
    fn session_audio_starts_disabled_and_can_be_toggled() {
        set_session_audio_enabled(false);
        assert!(!session_audio_enabled());
        set_session_audio_enabled(true);
        assert!(session_audio_enabled());
        set_session_audio_enabled(false);
    }
}
