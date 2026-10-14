use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_FILE_BYTES: u64 = 16 * 1024 * 1024;
static LOG_LOCK: Mutex<()> = Mutex::new(());
static PACKET_TRACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn next_packet_trace_id(packet_type: u8) -> String {
    let prefix = match packet_type {
        16 | 18 => "CHAT",
        20..=27 => "FILE",
        _ => "PACKET",
    };
    let sequence = PACKET_TRACE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    format!("{prefix}-{}-{sequence:08}", std::process::id())
}

pub fn trace_id_for_session(target_system_id: &str) -> String {
    // Both endpoints can derive this correlation key without changing the relay protocol.
    let mut hash = 0x811c9dc5u32;
    for byte in format!("deskstream-session:{target_system_id}").bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{hash:08X}")
}

pub fn log(identity: &str, event: &str) {
    let trace_id = trace_id_for_session(identity);
    log_with_trace(identity, &trace_id, "NATIVE_AGENT", event);
}

pub fn log_with_trace(identity: &str, trace_id: &str, component: &str, event: &str) {
    let safe_identity = sanitize(identity);
    let safe_component = sanitize(component);
    let safe_event = sanitize(event);
    let trace_id = if trace_id.len() == 8 && trace_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        trace_id.to_ascii_uppercase()
    } else {
        trace_id_for_session(&safe_identity)
    };
    let timestamp_ms = now_ms();
    let current_thread = std::thread::current();
    let thread_name = sanitize(current_thread.name().unwrap_or("unnamed"));
    let thread_id = format!("{:?}", current_thread.id());
    let first_token = safe_event.split_whitespace().next().unwrap_or("UNKNOWN");
    let event_name = first_token
        .strip_prefix("event=")
        .unwrap_or(first_token)
        .trim_matches(|character| character == '[' || character == ']');
    let component = safe_event
        .split_whitespace()
        .find_map(|field| field.strip_prefix("component="))
        .unwrap_or(&safe_component);
    let direction = safe_event
        .split_whitespace()
        .find_map(|field| field.strip_prefix("direction="))
        .unwrap_or("EVENT");
    let line = format!(
        "time_ms={timestamp_ms} timestamp_ms={timestamp_ms} pid={} thread_name={} thread_id={} SESSION_TRACE_ID={trace_id} component={} direction={} event={} identity={safe_identity} {safe_event}\n",
        std::process::id(),
        thread_name,
        thread_id,
        sanitize(component),
        sanitize(direction),
        sanitize(event_name)
    );
    append_line(&line);
}

pub fn lifecycle(
    identity: &str,
    event: &str,
    function: &str,
    socket: &str,
    is_connected: Option<&std::sync::atomic::AtomicBool>,
    is_in_session: Option<&std::sync::atomic::AtomicBool>,
    intentional_reconnect: bool,
    detail: &str,
) {
    use std::sync::atomic::Ordering;

    let connected = is_connected
        .map(|value| value.load(Ordering::SeqCst).to_string())
        .unwrap_or_else(|| "n/a".to_string());
    let in_session = is_in_session
        .map(|value| value.load(Ordering::SeqCst).to_string())
        .unwrap_or_else(|| "n/a".to_string());
    log(
        identity,
        &format!(
            "event={} function={} socket={} is_connected={} is_in_session={} intentional_reconnect={} detail={}",
            sanitize(event),
            sanitize(function),
            sanitize(socket),
            connected,
            in_session,
            intentional_reconnect,
            sanitize(detail)
        ),
    );
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(2048)
        .collect()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn log_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| {
            path.join("DeskStream")
                .join("logs")
                .join("session-debug.log")
        })
}

fn append_line(line: &str) {
    let Some(path) = log_path() else {
        eprintln!("[SESSION DEBUG] LOCALAPPDATA is not set; diagnostic event was not persisted");
        return;
    };
    let Ok(_guard) = LOG_LOCK.lock() else {
        eprintln!("[SESSION DEBUG] Diagnostic log lock was poisoned");
        return;
    };
    let Some(log_dir) = path.parent() else {
        eprintln!("[SESSION DEBUG] Diagnostic log path has no parent directory");
        return;
    };
    if let Err(error) = std::fs::create_dir_all(log_dir) {
        eprintln!("[SESSION DEBUG] Failed to create diagnostic log directory: {error}");
        return;
    }

    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.len().saturating_add(line.len() as u64) > MAX_LOG_FILE_BYTES {
            let archive_name = format!("session-debug-{}-{}.log", now_ms(), std::process::id());
            let archive_path = log_dir.join(archive_name);
            match std::fs::rename(&path, archive_path) {
                Ok(()) => prune_archives(log_dir),
                Err(error) => {
                    eprintln!("[SESSION DEBUG] Failed to rotate diagnostic log: {error}");
                    return;
                }
            }
        }
    }

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(line.as_bytes()) {
                eprintln!("[SESSION DEBUG] Failed to append diagnostic event: {error}");
            }
        }
        Err(error) => eprintln!("[SESSION DEBUG] Failed to open diagnostic log: {error}"),
    }
}

fn prune_archives(log_dir: &Path) {
    let entries = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("[SESSION DEBUG] Failed to list rotated diagnostic logs: {error}");
            return;
        }
    };
    let mut archives = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("[SESSION DEBUG] Failed to read a rotated diagnostic log entry: {error}");
                continue;
            }
        };
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("session-debug-") {
            continue;
        }
        let is_file = match entry.file_type() {
            Ok(kind) => kind.is_file(),
            Err(error) => {
                eprintln!("[SESSION DEBUG] Failed to inspect a rotated diagnostic log: {error}");
                continue;
            }
        };
        if !is_file {
            continue;
        }
        let modified = match entry.metadata().and_then(|metadata| metadata.modified()) {
            Ok(modified) => modified,
            Err(error) => {
                eprintln!(
                    "[SESSION DEBUG] Failed to read rotated diagnostic log metadata: {error}"
                );
                continue;
            }
        };
        archives.push((modified, entry.path()));
    }
    archives.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, archive) in archives.into_iter().skip(5) {
        if let Err(error) = std::fs::remove_file(archive) {
            eprintln!("[SESSION DEBUG] Failed to prune rotated diagnostic log: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct TestLocalAppData {
        previous: Option<OsString>,
        path: PathBuf,
    }

    impl TestLocalAppData {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "DeskStream-session-debug-{}-{}",
                std::process::id(),
                now_ms()
            ));
            std::fs::create_dir(&path).expect("create isolated diagnostic test directory");
            let previous = std::env::var_os("LOCALAPPDATA");
            std::env::set_var("LOCALAPPDATA", &path);
            Self { previous, path }
        }
    }

    impl Drop for TestLocalAppData {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                std::env::set_var("LOCALAPPDATA", previous);
            } else {
                std::env::remove_var("LOCALAPPDATA");
            }
            if let Err(error) = std::fs::remove_dir_all(&self.path) {
                eprintln!("[SESSION DEBUG TEST] Failed to clean test directory: {error}");
            }
        }
    }

    #[test]
    fn trace_id_is_stable_and_shared_format() {
        assert_eq!(trace_id_for_session("123456789"), "CA4078E8");
        assert_eq!(trace_id_for_session("123456789").len(), 8);
    }

    #[test]
    fn persisted_lines_include_trace_metadata_and_rotation_is_bounded() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("lock test environment");
        let _test_environment = TestLocalAppData::new();
        let identity = "123456789";
        log_with_trace(
            identity,
            &trace_id_for_session(identity),
            "WEBVIEW",
            "[TYPE99 TRACE] component=HOST direction=RECV source=test",
        );
        let log_dir = log_path()
            .expect("test log path")
            .parent()
            .expect("log parent")
            .to_path_buf();
        let active_log = log_dir.join("session-debug.log");
        let line = std::fs::read_to_string(&active_log).expect("read persisted diagnostic line");
        assert!(line.contains("time_ms="));
        assert!(line.contains("SESSION_TRACE_ID=CA4078E8"));
        assert!(line.contains("component=HOST direction=RECV event=TYPE99"));

        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&active_log)
            .expect("open active diagnostic log");
        file.set_len(MAX_LOG_FILE_BYTES)
            .expect("prepare size-limited rotation");
        drop(file);
        for index in 0..6 {
            std::fs::write(
                log_dir.join(format!("session-debug-test-{index}.log")),
                b"old",
            )
            .expect("create prior archive");
        }
        let next_line =
            "time_ms=1 SESSION_TRACE_ID=CA4078E8 component=TEST direction=EVENT event=ROTATE\n";
        append_line(next_line);

        let entries = std::fs::read_dir(&log_dir).expect("list rotated logs");
        let mut archive_count = 0;
        for entry in entries {
            let entry = entry.expect("read rotated log entry");
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("session-debug-")
                && entry.file_type().expect("inspect archive").is_file()
            {
                archive_count += 1;
                assert!(entry.metadata().expect("archive metadata").len() <= MAX_LOG_FILE_BYTES + 1024 * 1024);
            }
        }
        assert_eq!(archive_count, 5);
        let active_str = std::fs::read_to_string(&active_log).expect("read new active log");
        assert!(active_str.contains(next_line.trim_end()));
    }
}
