// TEMPORARY SESSION DIAGNOSTICS: remove this module and its call sites after the A/B trace.
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn log(identity: &str, event: &str) {
    write_log(identity, event);
}

pub fn lifecycle(
    identity: &str,
    event: &str,
    function: &str,
    socket: &str,
    is_connected: Option<&AtomicBool>,
    is_in_session: Option<&AtomicBool>,
    intentional_reconnect: bool,
    detail: &str,
) {
    let connected = is_connected
        .map(|value| value.load(Ordering::SeqCst).to_string())
        .unwrap_or_else(|| "n/a".to_string());
    let in_session = is_in_session
        .map(|value| value.load(Ordering::SeqCst).to_string())
        .unwrap_or_else(|| "n/a".to_string());
    write_log(
        identity,
        &format!(
            "event={} function={} socket={} is_connected={} is_in_session={} intentional_reconnect={} detail={}",
            event,
            function,
            socket,
            connected,
            in_session,
            intentional_reconnect,
            detail
        ),
    );
}

fn write_log(identity: &str, event: &str) {
    let local_app_data = match std::env::var_os("LOCALAPPDATA") {
        Some(path) => PathBuf::from(path),
        None => {
            eprintln!("[SESSION DEBUG] LOCALAPPDATA is not set; diagnostic event was not persisted");
            return;
        }
    };
    let log_dir = local_app_data.join("DeskStream").join("logs");
    if let Err(error) = std::fs::create_dir_all(&log_dir) {
        eprintln!("[SESSION DEBUG] Failed to create diagnostic log directory: {error}");
        return;
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let current_thread = std::thread::current();
    let thread_name = current_thread.name().unwrap_or("unnamed");
    let thread_id = format!("{:?}", current_thread.id());
    let identity = identity.replace(['\r', '\n'], "_");
    let event = event.replace(['\r', '\n'], "_");
    let path = log_dir.join("session-debug.log");
    match std::fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            if let Err(error) = writeln!(
                file,
                "timestamp_ms={} pid={} thread_name={} thread_id={} identity={} {}",
                timestamp_ms,
                std::process::id(),
                thread_name,
                thread_id,
                identity,
                event
            ) {
                eprintln!("[SESSION DEBUG] Failed to append diagnostic event: {error}");
            }
        }
        Err(error) => eprintln!("[SESSION DEBUG] Failed to open diagnostic log: {error}"),
    }
}
