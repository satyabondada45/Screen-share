use std::env;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const CHAT_TYPING_PACKET_TYPE: u8 = 18;
pub const CHAT_TYPING_STOP: u8 = 0;
pub const CHAT_TYPING_START: u8 = 1;
pub const HEARTBEAT_MISS_LIMIT: u32 = 3;

#[derive(Clone, Debug)]
pub struct HeartbeatHealth {
    pub state: String,
    pub rtt_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    pub missed_count: u32,
    pub last_failure_reason: Option<String>,
}

impl Default for HeartbeatHealth {
    fn default() -> Self {
        Self {
            state: "Disconnected".to_string(),
            rtt_ms: None,
            last_success_ms: None,
            missed_count: 0,
            last_failure_reason: None,
        }
    }
}

impl HeartbeatHealth {
    fn success(&mut self, rtt_ms: u64, now_ms: u64) -> Option<(String, String)> {
        let old_state = self.state.clone();
        self.rtt_ms = Some(rtt_ms);
        self.last_success_ms = Some(now_ms);
        self.missed_count = 0;
        self.last_failure_reason = None;
        self.state = "Healthy".to_string();
        (old_state != self.state).then(|| (old_state, self.state.clone()))
    }

    fn missed(&mut self, reason: &str) -> Option<(String, String)> {
        let old_state = self.state.clone();
        self.missed_count = self.missed_count.saturating_add(1);
        self.last_failure_reason = Some(reason.to_string());
        self.state = if self.missed_count >= HEARTBEAT_MISS_LIMIT {
            "Lost".to_string()
        } else {
            "Degraded".to_string()
        };
        (old_state != self.state).then(|| (old_state, self.state.clone()))
    }

    fn lost(&mut self, reason: &str) -> Option<(String, String)> {
        let old_state = self.state.clone();
        self.missed_count = self.missed_count.max(HEARTBEAT_MISS_LIMIT);
        self.last_failure_reason = Some(reason.to_string());
        self.state = "Lost".to_string();
        (old_state != self.state).then(|| (old_state, self.state.clone()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatReceive {
    Matched { rtt_ms: u64 },
    Stale,
    PeerRequest,
}

struct PendingHeartbeat {
    id: u64,
    sent_at: Option<Instant>,
}

#[derive(Default)]
pub struct HeartbeatTracker {
    pending: Option<PendingHeartbeat>,
    recent_ids: VecDeque<u64>,
}

impl HeartbeatTracker {
    pub fn schedule(&mut self, id: u64) -> bool {
        if self.pending.is_some() || self.recent_ids.contains(&id) {
            return false;
        }
        self.pending = Some(PendingHeartbeat { id, sent_at: None });
        true
    }

    pub fn mark_sent(&mut self, id: u64, sent_at: Instant) -> bool {
        match self.pending.as_mut() {
            Some(pending) if pending.id == id => {
                pending.sent_at = Some(sent_at);
                true
            }
            _ => false,
        }
    }

    pub fn receive(&mut self, id: u64, received_at: Instant) -> HeartbeatReceive {
        if self.pending.as_ref().is_some_and(|pending| pending.id == id) {
            let pending = self.pending.take().expect("matched pending heartbeat");
            let Some(sent_at) = pending.sent_at else {
                self.remember(id);
                return HeartbeatReceive::Stale;
            };
            self.remember(id);
            return HeartbeatReceive::Matched {
                rtt_ms: received_at.duration_since(sent_at).as_millis().min(u64::MAX as u128) as u64,
            };
        }
        if self.recent_ids.contains(&id) {
            HeartbeatReceive::Stale
        } else {
            HeartbeatReceive::PeerRequest
        }
    }

    pub fn timeout(&mut self, now: Instant, timeout: Duration) -> Option<u64> {
        let pending = self.pending.as_ref()?;
        let sent_at = pending.sent_at?;
        if now.duration_since(sent_at) < timeout {
            return None;
        }
        let id = self.pending.take()?.id;
        self.remember(id);
        Some(id)
    }

    pub fn cancel(&mut self, id: u64) {
        if self.pending.as_ref().is_some_and(|pending| pending.id == id) {
            self.pending = None;
            self.remember(id);
        }
    }

    pub fn cancel_pending(&mut self) -> Option<u64> {
        let id = self.pending.take()?.id;
        self.remember(id);
        Some(id)
    }

    pub fn remember_stale(&mut self, id: u64) {
        self.remember(id);
    }

    fn remember(&mut self, id: u64) {
        self.recent_ids.push_back(id);
        while self.recent_ids.len() > 16 {
            self.recent_ids.pop_front();
        }
    }
}

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
    pub relay_health: HeartbeatHealth,
    pub session_health: HeartbeatHealth,
    pub reconnect_state: String,
    pub reconnect_attempt: u32,
    pub reconnect_max_attempts: u32,
    pub disconnect_reason: String,
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

pub fn set_session_audio_capture_available(available: bool) {
    SESSION_AUDIO_CAPTURE_AVAILABLE.store(available, Ordering::Release);
}

pub fn session_audio_capture_available() -> bool {
    SESSION_AUDIO_CAPTURE_AVAILABLE.load(Ordering::Acquire)
}

pub fn set_session_perm_view_screen(allowed: bool) {
    SESSION_PERM_VIEW_SCREEN.store(allowed, Ordering::Release);
}
pub fn session_perm_view_screen() -> bool {
    SESSION_PERM_VIEW_SCREEN.load(Ordering::Acquire)
}

pub fn set_session_perm_control_input(allowed: bool) {
    SESSION_PERM_CONTROL_INPUT.store(allowed, Ordering::Release);
}
pub fn session_perm_control_input() -> bool {
    SESSION_PERM_CONTROL_INPUT.load(Ordering::Acquire)
}

pub fn set_session_perm_file_transfer(allowed: bool) {
    SESSION_PERM_FILE_TRANSFER.store(allowed, Ordering::Release);
}
pub fn session_perm_file_transfer() -> bool {
    SESSION_PERM_FILE_TRANSFER.load(Ordering::Acquire)
}

pub fn set_session_reverse_request_pending() -> bool {
    SESSION_REVERSE_REQUEST
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

pub fn take_session_reverse_request() -> bool {
    SESSION_REVERSE_REQUEST.load(Ordering::Acquire)
}

pub fn clear_session_reverse_request_pending() {
    SESSION_REVERSE_REQUEST.store(false, Ordering::Release);
}

pub fn begin_session_reverse_request_with_id(request_id: &str) -> Result<bool, String> {
    if request_id.is_empty()
        || request_id.len() > 64
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("reverse request ID must be 1-64 safe ASCII characters".to_string());
    }
    let mut current_id = SESSION_REVERSE_REQUEST_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "reverse request ID lock poisoned".to_string())?;
    if SESSION_REVERSE_REQUEST_OUTSTANDING.load(Ordering::Acquire) {
        return Ok(false);
    }
    *current_id = Some(request_id.to_string());
    if SESSION_REVERSE_REQUEST_OUTSTANDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        *current_id = None;
        return Ok(false);
    }
    Ok(true)
}

pub fn begin_session_reverse_request() -> bool {
    SESSION_REVERSE_REQUEST_OUTSTANDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

pub fn cancel_session_reverse_request() {
    SESSION_REVERSE_DECISION.store(0, Ordering::Release);
    SESSION_REVERSE_REQUEST_OUTSTANDING.store(false, Ordering::Release);
    if let Ok(mut request_id) = SESSION_REVERSE_REQUEST_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *request_id = None;
    }
}

pub fn cancel_session_reverse_request_with_id(request_id: &str) -> Result<bool, String> {
    let mut current_id = SESSION_REVERSE_REQUEST_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "reverse request ID lock poisoned".to_string())?;
    if current_id.as_deref() != Some(request_id) {
        return Ok(false);
    }
    SESSION_REVERSE_DECISION.store(0, Ordering::Release);
    SESSION_REVERSE_REQUEST_OUTSTANDING.store(false, Ordering::Release);
    *current_id = None;
    Ok(true)
}

pub fn acknowledge_session_reverse_decision_with_id(request_id: &str) -> Result<bool, String> {
    let mut current_id = SESSION_REVERSE_REQUEST_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "reverse request ID lock poisoned".to_string())?;
    if current_id.as_deref() != Some(request_id)
        || SESSION_REVERSE_DECISION.load(Ordering::Acquire) == 0
    {
        return Ok(false);
    }
    SESSION_REVERSE_DECISION.store(0, Ordering::Release);
    SESSION_REVERSE_REQUEST_OUTSTANDING.store(false, Ordering::Release);
    *current_id = None;
    Ok(true)
}

pub fn session_reverse_request_id() -> Result<Option<String>, String> {
    SESSION_REVERSE_REQUEST_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|request_id| request_id.clone())
        .map_err(|_| "reverse request ID lock poisoned".to_string())
}

pub fn set_session_reverse_decision(accepted: bool) -> bool {
    if !SESSION_REVERSE_REQUEST_OUTSTANDING.load(Ordering::Acquire) {
        return false;
    }
    let decision = if accepted { 1 } else { 2 };
    if SESSION_REVERSE_DECISION
        .compare_exchange(0, decision, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    if !SESSION_REVERSE_REQUEST_OUTSTANDING.load(Ordering::Acquire) {
        let _ = SESSION_REVERSE_DECISION.compare_exchange(
            decision,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        return false;
    }
    true
}

pub fn take_session_reverse_decision() -> u8 {
    let decision = SESSION_REVERSE_DECISION.swap(0, Ordering::AcqRel);
    if decision != 0 {
        SESSION_REVERSE_REQUEST_OUTSTANDING.store(false, Ordering::Release);
    }
    decision
}

pub fn take_session_reverse_decision_with_peer() -> Result<(u8, Option<String>), String> {
    let mut peer = SESSION_PEER_SYSTEM_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "session peer ID lock poisoned".to_string())?;
    let decision = SESSION_REVERSE_DECISION.swap(0, Ordering::AcqRel);
    if decision != 0 {
        SESSION_REVERSE_REQUEST_OUTSTANDING.store(false, Ordering::Release);
    }
    let peer_system_id = peer.clone();
    if decision != 0 {
        *peer = None;
    }
    Ok((decision, peer_system_id))
}

pub fn peek_session_reverse_decision_with_peer(
) -> Result<(u8, Option<String>, Option<String>), String> {
    let peer_system_id = SESSION_PEER_SYSTEM_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "session peer ID lock poisoned".to_string())?
        .clone();
    let request_id = SESSION_REVERSE_REQUEST_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "reverse request ID lock poisoned".to_string())?
        .clone();
    Ok((
        SESSION_REVERSE_DECISION.load(Ordering::Acquire),
        peer_system_id,
        request_id,
    ))
}

pub fn set_session_peer_system_id(system_id: String) -> Result<(), String> {
    if system_id.len() != 9 || !system_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("peer system ID must contain exactly 9 digits".to_string());
    }
    *SESSION_PEER_SYSTEM_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "session peer ID lock poisoned".to_string())? = Some(system_id);
    Ok(())
}

pub fn session_peer_system_id() -> Result<Option<String>, String> {
    SESSION_PEER_SYSTEM_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|peer| peer.clone())
        .map_err(|_| "session peer ID lock poisoned".to_string())
}

pub fn clear_session_peer_system_id() -> Result<(), String> {
    let mut peer = SESSION_PEER_SYSTEM_ID
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "session peer ID lock poisoned".to_string())?;
    if SESSION_REVERSE_DECISION.load(Ordering::Acquire) == 0 {
        *peer = None;
    }
    Ok(())
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
            relay_health: HeartbeatHealth::default(),
            session_health: HeartbeatHealth::default(),
            reconnect_state: "Disconnected".to_string(),
            reconnect_attempt: 0,
            reconnect_max_attempts: 0,
            disconnect_reason: "unknown_termination".to_string(),
        }
    }
}

pub static LIVE: OnceLock<Arc<Mutex<AgentStatus>>> = OnceLock::new();
static SESSION_WRITER: OnceLock<Mutex<Option<SyncSender<Vec<u8>>>>> = OnceLock::new();
static SESSION_CHAT_HISTORY: OnceLock<Mutex<SessionChatHistory>> = OnceLock::new();
static SESSION_REMOTE_TYPING: AtomicBool = AtomicBool::new(false);
static SESSION_AUDIO_ENABLED: AtomicBool = AtomicBool::new(false);
static SESSION_AUDIO_CAPTURE_AVAILABLE: AtomicBool = AtomicBool::new(false);
static SESSION_PERM_VIEW_SCREEN: AtomicBool = AtomicBool::new(true);
static SESSION_PERM_CONTROL_INPUT: AtomicBool = AtomicBool::new(true);
static SESSION_PERM_FILE_TRANSFER: AtomicBool = AtomicBool::new(true);
static SESSION_REVERSE_REQUEST: AtomicBool = AtomicBool::new(false);
static SESSION_REVERSE_REQUEST_OUTSTANDING: AtomicBool = AtomicBool::new(false);
static SESSION_REVERSE_DECISION: AtomicU8 = AtomicU8::new(0);
static SESSION_REVERSE_REQUEST_ID: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static SESSION_PEER_SYSTEM_ID: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static SESSION_FILE_OFFERS: OnceLock<Mutex<HashMap<u64, SessionFileOffer>>> = OnceLock::new();
static SESSION_ACCEPTED_FILES: OnceLock<Mutex<HashMap<u64, bool>>> = OnceLock::new();
static SESSION_FILE_RESPONSES: OnceLock<Mutex<HashMap<u64, mpsc::Sender<u8>>>> = OnceLock::new();
static MANUAL_RECONNECT_REQUESTED: AtomicBool = AtomicBool::new(false);

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

// FIXED: progress reporting for inbound file transfers (UI/status polling).
#[derive(Clone, Default)]
pub struct SessionFileProgress {
    pub transfer_id: u64,
    pub filename: String,
    pub total_bytes: u64,
    pub received_bytes: u64,
    pub chunk_index: u32,
    pub running_sha256_hex: String,
    pub accepted: bool,
}

static SESSION_FILE_PROGRESS: OnceLock<Mutex<HashMap<u64, SessionFileProgress>>> = OnceLock::new();

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
    SESSION_AUDIO_CAPTURE_AVAILABLE.store(false, Ordering::Release);
    SESSION_REVERSE_REQUEST.store(false, Ordering::Release);
    if writer.is_some() {
        *SESSION_REVERSE_REQUEST_ID
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "reverse request ID lock poisoned".to_string())? = None;
        SESSION_REVERSE_REQUEST_OUTSTANDING.store(false, Ordering::Release);
        SESSION_REVERSE_DECISION.store(0, Ordering::Release);
        *SESSION_PEER_SYSTEM_ID
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "session peer ID lock poisoned".to_string())? = None;
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
        if SESSION_REVERSE_DECISION.load(Ordering::Acquire) == 0 {
            *SESSION_REVERSE_REQUEST_ID
                .get_or_init(|| Mutex::new(None))
                .lock()
                .map_err(|_| "reverse request ID lock poisoned".to_string())? = None;
            SESSION_REVERSE_REQUEST_OUTSTANDING.store(false, Ordering::Release);
            *SESSION_PEER_SYSTEM_ID
                .get_or_init(|| Mutex::new(None))
                .lock()
                .map_err(|_| "session peer ID lock poisoned".to_string())? = None;
        }
        SESSION_PERM_VIEW_SCREEN.store(true, Ordering::Release);
        SESSION_PERM_CONTROL_INPUT.store(true, Ordering::Release);
        SESSION_PERM_FILE_TRANSFER.store(true, Ordering::Release);
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
        SESSION_FILE_PROGRESS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| "file progress lock poisoned".to_string())?
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
    // FIXED: clear progress when transfer ends.
    if let Ok(mut progress) = SESSION_FILE_PROGRESS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        progress.remove(&transfer_id);
    }
}

// FIXED: publish / query inbound file transfer progress for UI.
pub fn update_session_file_progress(progress: SessionFileProgress) {
    if let Ok(mut map) = SESSION_FILE_PROGRESS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        map.insert(progress.transfer_id, progress);
    }
}

pub fn session_file_progress() -> Result<Vec<SessionFileProgress>, String> {
    SESSION_FILE_PROGRESS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map(|map| map.values().cloned().collect())
        .map_err(|_| "file progress lock poisoned".to_string())
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

pub fn resolve_session_file_response(transfer_id: u64, response: u8) -> bool {
    if let Ok(mut responses) = SESSION_FILE_RESPONSES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        let sender = responses.get(&transfer_id).cloned();
        if response == 23 || response == 24 || response == 25 || response == 27 {
            responses.remove(&transfer_id);
        }
        if let Some(sender) = sender {
            return sender.send(response).is_ok();
        }
    }
    false
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
                if g.reconnect_state == "Healthy" {
                    g.relay_health.state = "Healthy".to_string();
                }
            } else {
                g.status = "offline".to_string();
                g.relay_health.state = if g.reconnect_state == "Disconnected" {
                    "Disconnected".to_string()
                } else {
                    "Reconnecting".to_string()
                };
            }
        }
    }
}

pub fn record_heartbeat_success(session: bool, rtt_ms: u64) -> Option<(String, String)> {
    let status = LIVE.get()?;
    let mut status = status.lock().ok()?;
    let now = now_ms();
    status.last_heartbeat_ms = now;
    if session {
        status.session_health.success(rtt_ms, now)
    } else {
        status.relay_health.success(rtt_ms, now)
    }
}

pub fn record_heartbeat_missed(session: bool, reason: &str) -> Option<(String, String)> {
    let status = LIVE.get()?;
    let mut status = status.lock().ok()?;
    if session {
        status.session_health.missed(reason)
    } else {
        status.relay_health.missed(reason)
    }
}

pub fn mark_heartbeat_lost(session: bool, reason: &str) -> Option<(String, String)> {
    let status = LIVE.get()?;
    let mut status = status.lock().ok()?;
    if session {
        status.session_health.lost(reason)
    } else {
        status.relay_health.lost(reason)
    }
}

pub fn heartbeat_missed_count(session: bool) -> Option<u32> {
    let status = LIVE.get()?;
    let status = status.lock().ok()?;
    Some(if session {
        status.session_health.missed_count
    } else {
        status.relay_health.missed_count
    })
}

pub fn set_reconnect_state(state: &str, attempt: u32, max_attempts: u32) {
    if let Some(status) = LIVE.get() {
        if let Ok(mut status) = status.lock() {
            status.reconnect_state = state.to_string();
            status.reconnect_attempt = attempt;
            status.reconnect_max_attempts = max_attempts;
            if !status.in_session {
                status.relay_health.state = match state {
                    "Healthy" => "Healthy",
                    "Reconnecting" => "Reconnecting",
                    "Disconnected" => "Disconnected",
                    _ => status.relay_health.state.as_str(),
                }
                .to_string();
            }
        }
    }
}

pub fn set_disconnect_reason(reason: &str) {
    if let Some(status) = LIVE.get() {
        if let Ok(mut status) = status.lock() {
            status.disconnect_reason = reason.to_string();
            if reason.starts_with("explicit_") {
                status.session_health.state = "Disconnected".to_string();
            }
        }
    }
}

pub fn set_disconnect_reason_if_unknown(reason: &str) -> bool {
    if let Some(status) = LIVE.get() {
        if let Ok(mut status) = status.lock() {
            if status.disconnect_reason == "unknown_termination" {
                status.disconnect_reason = reason.to_string();
                return true;
            }
        }
    }
    false
}

pub fn request_manual_reconnect() -> bool {
    let Some(status) = LIVE.get() else {
        return false;
    };
    let Ok(status) = status.lock() else {
        return false;
    };
    if status.reconnect_state != "Disconnected" {
        return false;
    }
    MANUAL_RECONNECT_REQUESTED.store(true, Ordering::Release);
    true
}

pub fn take_manual_reconnect_request() -> bool {
    MANUAL_RECONNECT_REQUESTED.swap(false, Ordering::AcqRel)
}

#[derive(Clone, Debug)]
pub struct ApprovedSession {
    pub token_hash: [u8; 32],
    pub token: String,
    pub requester_id: String,
    pub expires_at: Instant,
    pub consumed: Arc<AtomicBool>,
}

static APPROVED_SESSIONS: OnceLock<Mutex<Vec<ApprovedSession>>> = OnceLock::new();

fn get_approved_sessions() -> &'static Mutex<Vec<ApprovedSession>> {
    APPROVED_SESSIONS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn record_approved_session(token: &str, requester_id: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let token = token.trim();
    if token.is_empty() {
        return Err("approval token is empty".to_string());
    }
    let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let now = Instant::now();
    let expires_at = now + Duration::from_secs(60);

    let mut list = get_approved_sessions()
        .lock()
        .map_err(|_| "approved session lock poisoned".to_string())?;
    list.retain(|session| {
        session.expires_at > now
            && !session.consumed.load(Ordering::SeqCst)
            && session.token_hash != hash
    });
    list.push(ApprovedSession {
        token_hash: hash,
        token: token.to_string(),
        requester_id: requester_id.to_string(),
        expires_at,
        consumed: Arc::new(AtomicBool::new(false)),
    });
    Ok(())
}

pub fn revoke_approved_session(token: &str) -> Result<bool, String> {
    use sha2::{Digest, Sha256};
    let token = token.trim();
    if token.is_empty() {
        return Ok(false);
    }
    let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let mut list = get_approved_sessions()
        .lock()
        .map_err(|_| "approved session lock poisoned".to_string())?;
    let previous_len = list.len();
    list.retain(|session| session.token_hash != hash);
    Ok(list.len() != previous_len)
}

pub fn consume_approved_session(supplied_hash: &[u8; 32]) -> Option<(String, String)> {
    if supplied_hash.iter().all(|&b| b == 0) {
        return None;
    }

    let now = Instant::now();
    if let Ok(mut list) = get_approved_sessions().lock() {
        list.retain(|s| s.expires_at > now);

        for session in list.iter() {
            if session.expires_at > now {
                let diff = session
                    .token_hash
                    .iter()
                    .zip(supplied_hash.iter())
                    .fold(0u8, |acc, (a, b)| acc | (a ^ b));

                if diff == 0 {
                    if session
                        .consumed
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        return Some((session.token.clone(), session.requester_id.clone()));
                    }
                }
            }
        }
    }
    None
}

pub fn check_and_consume_approved_session(supplied_hash: &[u8; 32]) -> bool {
    consume_approved_session(supplied_hash).is_some()
}

pub fn clear_approved_sessions() {
    if let Ok(mut list) = get_approved_sessions().lock() {
        list.clear();
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
            if active {
                g.session_health = HeartbeatHealth {
                    state: "Healthy".to_string(),
                    ..HeartbeatHealth::default()
                };
                g.disconnect_reason = "unknown_termination".to_string();
            } else if g.session_health.state != "Lost" {
                g.session_health.state = "Disconnected".to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        acknowledge_session_reverse_decision_with_id, begin_session_reverse_request,
        begin_session_reverse_request_with_id, cancel_session_reverse_request,
        cancel_session_reverse_request_with_id,
        check_and_consume_approved_session, clear_approved_sessions,
        clear_session_peer_system_id, record_approved_session, revoke_approved_session,
        peek_session_reverse_decision_with_peer, session_peer_system_id,
        set_session_peer_system_id, set_session_reverse_decision,
        take_session_reverse_decision, take_session_reverse_decision_with_peer,
        session_audio_enabled, session_perm_control_input, session_perm_file_transfer,
        session_perm_view_screen, set_session_audio_enabled, set_session_perm_control_input,
        set_session_perm_file_transfer, set_session_perm_view_screen, set_session_writer,
        HeartbeatHealth, HeartbeatReceive, HeartbeatTracker, SessionChatHistory,
        CHAT_TYPING_PACKET_TYPE, CHAT_TYPING_START, CHAT_TYPING_STOP, HEARTBEAT_MISS_LIMIT,
    };
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    #[test]
    fn heartbeat_success_resets_misses_and_records_round_trip() {
        let mut health = HeartbeatHealth::default();
        health.missed("timeout");
        assert_eq!(health.state, "Degraded");
        assert_eq!(health.missed_count, 1);

        health.success(42, 1_700_000_000_000);
        assert_eq!(health.state, "Healthy");
        assert_eq!(health.rtt_ms, Some(42));
        assert_eq!(health.last_success_ms, Some(1_700_000_000_000));
        assert_eq!(health.missed_count, 0);
        assert_eq!(health.last_failure_reason, None);
    }

    #[test]
    fn heartbeat_becomes_lost_after_three_misses() {
        let mut health = HeartbeatHealth::default();
        for _ in 0..HEARTBEAT_MISS_LIMIT {
            health.missed("response_timeout");
        }
        assert_eq!(health.state, "Lost");
        assert_eq!(health.missed_count, HEARTBEAT_MISS_LIMIT);
        assert_eq!(health.last_failure_reason.as_deref(), Some("response_timeout"));
    }

    #[test]
    fn heartbeat_tracker_matches_echo_and_suppresses_late_responses() {
        let sent_at = Instant::now() - Duration::from_millis(42);
        let mut tracker = HeartbeatTracker::default();
        assert!(tracker.schedule(1234));
        assert!(tracker.mark_sent(1234, sent_at));
        assert!(matches!(
            tracker.receive(1234, Instant::now()),
            HeartbeatReceive::Matched { rtt_ms } if rtt_ms >= 42
        ));
        assert_eq!(tracker.receive(1234, Instant::now()), HeartbeatReceive::Stale);
    }

    #[test]
    fn heartbeat_tracker_times_out_once_and_recognizes_peer_requests() {
        let sent_at = Instant::now() - Duration::from_secs(9);
        let mut tracker = HeartbeatTracker::default();
        assert!(tracker.schedule(5678));
        assert!(tracker.mark_sent(5678, sent_at));
        assert_eq!(tracker.timeout(Instant::now(), Duration::from_secs(8)), Some(5678));
        assert_eq!(tracker.timeout(Instant::now(), Duration::from_secs(8)), None);
        assert_eq!(tracker.receive(5678, Instant::now()), HeartbeatReceive::Stale);
        assert_eq!(tracker.receive(9999, Instant::now()), HeartbeatReceive::PeerRequest);
    }

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

    #[test]
    fn reverse_decisions_are_bound_to_one_outstanding_request() {
        cancel_session_reverse_request();
        assert!(!set_session_reverse_decision(true));
        assert!(begin_session_reverse_request());
        assert!(!begin_session_reverse_request());
        assert!(set_session_reverse_decision(true));
        assert!(!set_session_reverse_decision(false));
        assert_eq!(take_session_reverse_decision(), 1);
        assert!(!set_session_reverse_decision(true));

        assert!(begin_session_reverse_request());
        assert!(set_session_reverse_decision(false));
        assert_eq!(take_session_reverse_decision(), 2);
        cancel_session_reverse_request();
    }

    #[test]
    fn reverse_decision_is_retained_until_matching_request_acknowledges_it() {
        cancel_session_reverse_request();
        set_session_peer_system_id("123456789".to_string()).unwrap();

        assert!(begin_session_reverse_request_with_id("reverse-001").unwrap());
        assert!(!begin_session_reverse_request_with_id("reverse-002").unwrap());
        assert!(!acknowledge_session_reverse_decision_with_id("reverse-002").unwrap());
        assert!(set_session_reverse_decision(true));
        assert!(!acknowledge_session_reverse_decision_with_id("reverse-002").unwrap());
        assert_eq!(
            peek_session_reverse_decision_with_peer().unwrap(),
            (
                1,
                Some("123456789".to_string()),
                Some("reverse-001".to_string())
            )
        );

        assert!(acknowledge_session_reverse_decision_with_id("reverse-001").unwrap());
        assert_eq!(
            peek_session_reverse_decision_with_peer().unwrap(),
            (0, Some("123456789".to_string()), None)
        );
        assert!(!set_session_reverse_decision(false));
        clear_session_peer_system_id().unwrap();
        cancel_session_reverse_request();
    }

    #[test]
    fn cancelling_reverse_request_requires_matching_request_id() {
        cancel_session_reverse_request();

        assert!(begin_session_reverse_request_with_id("reverse-cancel-001").unwrap());
        assert!(!cancel_session_reverse_request_with_id("reverse-cancel-002").unwrap());
        assert_eq!(
            super::session_reverse_request_id().unwrap().as_deref(),
            Some("reverse-cancel-001")
        );
        assert!(cancel_session_reverse_request_with_id("reverse-cancel-001").unwrap());
        assert!(!set_session_reverse_decision(true));
        assert_eq!(super::session_reverse_request_id().unwrap(), None);
    }

    #[test]
    fn cancelling_reverse_request_preserves_active_peer_identity() {
        set_session_peer_system_id("123456789".to_string()).unwrap();
        assert!(begin_session_reverse_request());

        cancel_session_reverse_request();

        assert!(!set_session_reverse_decision(true));
        assert_eq!(session_peer_system_id().unwrap().as_deref(), Some("123456789"));
        clear_session_peer_system_id().expect("peer identity should clear");
    }

    #[test]
    fn approved_reverse_decision_and_peer_survive_session_teardown() {
        cancel_session_reverse_request();
        let (writer, _reader) = sync_channel(1);
        set_session_writer(Some(writer)).expect("session writer should initialize");
        set_session_peer_system_id("123456789".to_string()).unwrap();
        assert!(begin_session_reverse_request());
        assert!(set_session_reverse_decision(true));

        set_session_writer(None).expect("session writer should clear");

        assert_eq!(
            take_session_reverse_decision_with_peer().expect("decision and peer should be consumed"),
            (1, Some("123456789".to_string()))
        );
        assert_eq!(session_peer_system_id().unwrap(), None);
    }

    #[test]
    fn pending_reverse_decision_keeps_local_request_id_across_teardown() {
        cancel_session_reverse_request();
        let (writer, _reader) = sync_channel(1);
        set_session_writer(Some(writer)).expect("session writer should initialize");
        set_session_peer_system_id("123456789".to_string()).unwrap();
        assert!(begin_session_reverse_request_with_id("reverse-teardown-001").unwrap());
        assert!(set_session_reverse_decision(true));

        set_session_writer(None).expect("session writer should clear");

        assert_eq!(
            peek_session_reverse_decision_with_peer().unwrap(),
            (
                1,
                Some("123456789".to_string()),
                Some("reverse-teardown-001".to_string())
            )
        );
        assert!(acknowledge_session_reverse_decision_with_id("reverse-teardown-001").unwrap());
        assert_eq!(
            peek_session_reverse_decision_with_peer().unwrap(),
            (0, Some("123456789".to_string()), None)
        );

        clear_session_peer_system_id().unwrap();
        cancel_session_reverse_request();
    }

    #[test]
    fn approved_reverse_peer_survives_peer_cleanup_until_decision_is_consumed() {
        cancel_session_reverse_request();
        set_session_peer_system_id("123456789".to_string()).unwrap();
        assert!(begin_session_reverse_request());
        assert!(set_session_reverse_decision(true));

        clear_session_peer_system_id().expect("session cleanup should retain approved peer");

        assert_eq!(
            take_session_reverse_decision_with_peer().expect("decision and peer should be consumed"),
            (1, Some("123456789".to_string()))
        );
        assert_eq!(session_peer_system_id().unwrap(), None);
    }

    #[test]
    fn selected_session_permissions_survive_session_writer_initialization() {
        set_session_perm_view_screen(false);
        set_session_perm_control_input(false);
        set_session_perm_file_transfer(false);

        let (writer, _reader) = sync_channel(1);
        set_session_writer(Some(writer)).expect("session writer should initialize");

        assert!(!session_perm_view_screen());
        assert!(!session_perm_control_input());
        assert!(!session_perm_file_transfer());

        set_session_writer(None).expect("session writer should clear");
        assert!(session_perm_view_screen());
        assert!(session_perm_control_input());
        assert!(session_perm_file_transfer());
    }

    #[test]
    fn approved_session_workflow_accepts_and_prevents_replay() {
        use sha2::{Digest, Sha256};
        clear_approved_sessions();
        let token = "test-token-abcdef123456";
        let valid_hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        assert!(!revoke_approved_session(token).unwrap());
        assert!(!check_and_consume_approved_session(&valid_hash));
        let invalid_hash: [u8; 32] = Sha256::digest(b"wrong-token").into();
        let zero_hash = [0u8; 32];

        // 1. Unapproved token rejected
        assert!(!check_and_consume_approved_session(&valid_hash));
        assert!(!check_and_consume_approved_session(&zero_hash));

        // Revoking a prepared approval prevents the relay handshake from consuming it.
        record_approved_session(token, "987654321").unwrap();
        assert!(revoke_approved_session(token).unwrap());
        assert!(!check_and_consume_approved_session(&valid_hash));

        // 2. Record approval
        record_approved_session(token, "987654321").unwrap();
        record_approved_session(token, "987654321").unwrap();

        // 3. Invalid and zero hashes still rejected
        assert!(!check_and_consume_approved_session(&invalid_hash));
        assert!(!check_and_consume_approved_session(&zero_hash));

        // 4. Valid hash accepted on first attempt
        assert!(check_and_consume_approved_session(&valid_hash));

        // 5. Replay attempt rejected (single-use consumption)
        assert!(!check_and_consume_approved_session(&valid_hash));
        assert!(revoke_approved_session(token).unwrap());
    }
}
