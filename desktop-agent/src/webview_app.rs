// src/webview_app.rs
// DeskStream — Unified Application UI.
//
// Loads the live Hostinger DeskStream website inside a native Windows WebView2 window.
// The full agent engine (screen capture, H.264, relay, remote control, file transfer)
// runs on background threads spawned from main.rs.
// This is ONE application: the Controller UI and Agent engine are in the same binary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// TEMPORARY SESSION DIAGNOSTICS: accept only fixed event names and non-secret metadata.
fn handle_session_debug_ipc(message: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(message) else {
        return false;
    };
    if value.get("type").and_then(serde_json::Value::as_str) != Some("session_debug") {
        return false;
    }

    let event = match value.get("event").and_then(serde_json::Value::as_str) {
        Some(event) => event,
        None => return true,
    };
    const ALLOWED_EVENTS: &[&str] = &[
        "A_START",
        "ROLE",
        "A_WS_CONFIG",
        "A_TARGET_ID",
        "A_WS_CONNECT_START",
        "A_WS_CONNECT_SUCCESS",
        "A_WS_CONNECT_FAILED",
        "A_TYPE2_SENT",
        "A_SESSION_APPROVED",
        "A_STREAM_STATE",
        "A_TYPE13_RECEIVED",
        "A_FIRST_TYPE13_RECEIVED",
        "A_FIRST_FRAME_DECODED",
        "A_FIRST_FRAME_DISPLAYED",
        "A_DISCONNECT",
        "STATE_CHANGE",
        "A_CHAT_SEND_START",
        "A_CHAT_SEND_TEXT_CREATED",
        "A_CHAT_WS_SEND_SUCCESS",
        "A_CHAT_WS_SEND_FAILED",
        "A_CHAT_SEND_FAILED",
        "A_CHAT_LOCAL_APPEND_START",
        "A_CHAT_LOCAL_APPEND_SUCCESS",
        "A_CHAT_LOCAL_APPEND_FAILED",
        "A_CHAT_RENDER_MESSAGE",
    ];
    if !ALLOWED_EVENTS.contains(&event) {
        return true;
    }

    let field = |name: &str| -> String {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(|value| {
                value
                    .chars()
                    .filter(|character| !character.is_control())
                    .take(256)
                    .collect::<String>()
            })
            .unwrap_or_default()
    };
    let system_id = field("system_id");
    let identity = if system_id.len() == 9 && system_id.chars().all(|ch| ch.is_ascii_digit()) {
        system_id
    } else {
        "unknown".to_string()
    };
    let mut details = Vec::new();
    for name in [
        "role",
        "video_direction",
        "target_system_id",
        "endpoint",
        "state",
        "old",
        "new",
        "count",
        "result",
        "code",
        "message_id",
        "sender",
        "message_length",
        "packet_type",
        "state_count_before",
        "state_count_after",
        "dom_count_before",
        "dom_count_after",
        "dom_count",
        "panel_open",
        "rendered",
        "error",
    ] {
        let value = field(name);
        if !value.is_empty() {
            details.push(format!("{name}={value}"));
        }
    }
    crate::session_debug::log(&identity, &format!("{} {}", event, details.join(" ")));
    true
}

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;
#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;

/// The bundled desktop application UI.
const DESKSTREAM_URL: &str = "deskstream://localhost/dashboard.html";

/// Run the WebView2 application on the main thread.
/// `quit` — shared flag; when set true (e.g. from tray Exit), the window closes.
pub fn run_webview(quit: Arc<AtomicBool>, local_url: String) {
    let event_loop: tao::event_loop::EventLoop<String> = tao::event_loop::EventLoopBuilder::<String>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("DeskStream")
        .with_inner_size(LogicalSize::new(1280_u32, 800_u32))
        .with_min_inner_size(LogicalSize::new(900_u32, 600_u32))
        .with_resizable(true)
        .with_maximized(true)
        .with_decorations(false)
        .build(&event_loop)
        .expect("Failed to create DeskStream window");
    // Set the window icon from embedded ICO bytes
    #[cfg(target_os = "windows")]
    {
        let icon_bytes = include_bytes!("../assets/icon.ico");
        if let Some(icon) = load_icon_from_ico(icon_bytes) {
            let _ = window.set_window_icon(Some(icon));
        }
    }
    #[cfg(target_os = "windows")]
    let builder = WebViewBuilder::new()
        .with_url(&local_url)
        .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI --autoplay-policy=no-user-gesture-required");

    #[cfg(not(target_os = "windows"))]
    let builder = WebViewBuilder::new()
        .with_url(&local_url);

    let _webview = builder
        .with_devtools(false)
        .with_ipc_handler(move |msg| {
            let body = msg.into_body();
            if !handle_session_debug_ipc(&body) {
                let _ = proxy.send_event(body);
            }
        })
        .with_initialization_script(r#"
            window.__deskstreamDesktop = true;
        "#)
        .build(&window)
        .expect("Failed to create WebView2 — is Microsoft Edge WebView2 Runtime installed?");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        // Check quit signal from tray
        if quit.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        match event {
            Event::UserEvent(req) => {
                match req.as_str() {
                    "minimize" => window.set_minimized(true),
                    "maximize" => {
                        if !window.is_maximized() {
                            window.set_maximized(true);
                        }
                    },
                    "toggle_maximize" => {
                        let is_max = window.is_maximized();
                        window.set_maximized(!is_max);
                    },
                    "send_file" | "send_folder" => {
                        let selection = if req == "send_folder" {
                            rfd::FileDialog::new()
                                .pick_folder()
                                .map(|path| vec![path])
                        } else {
                            rfd::FileDialog::new().pick_files()
                        };
                        if let Some(paths) = selection {
                            let local_app_data = std::env::var("LOCALAPPDATA")
                                .unwrap_or_else(|_| "C:\\temp".to_string());
                            let app_dir = std::path::Path::new(&local_app_data).join("DeskStream");
                            if let Err(error) = std::fs::create_dir_all(&app_dir) {
                                eprintln!("[FILE TX] Failed to create transfer directory: {error}");
                            } else {
                                let selected_paths = paths
                                    .iter()
                                    .map(|path| path.to_string_lossy())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if let Err(error) = std::fs::write(
                                    app_dir.join("send_file.txt"),
                                    selected_paths.as_bytes(),
                                ) {
                                    eprintln!("[FILE TX] Failed to queue selected file: {error}");
                                }
                            }
                        }
                    }
                    "close" => { quit.store(true, Ordering::Relaxed); *control_flow = ControlFlow::Exit; },
                    "drag_window" => { let _ = window.drag_window(); },
                    _ => {}
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                quit.store(true, Ordering::Relaxed); *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

/// Parse a Windows ICO file and return a tao Icon for the 32x32 PNG entry.
#[cfg(target_os = "windows")]
fn load_icon_from_ico(ico_bytes: &[u8]) -> Option<tao::window::Icon> {
    // ICO header: 6 bytes. Directory entries: 16 bytes each.
    if ico_bytes.len() < 6 {
        return None;
    }
    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]) as usize;
    if ico_bytes.len() < 6 + count * 16 {
        return None;
    }

    // Find the 32x32 entry (or largest)
    let mut best_offset = 0usize;
    let mut best_size = 0usize;
    let mut best_dim = 0u8;

    for i in 0..count {
        let base = 6 + i * 16;
        let width = ico_bytes[base];
        let img_size = u32::from_le_bytes([ico_bytes[base + 8], ico_bytes[base + 9], ico_bytes[base + 10], ico_bytes[base + 11]]) as usize;
        let img_offset = u32::from_le_bytes([ico_bytes[base + 12], ico_bytes[base + 13], ico_bytes[base + 14], ico_bytes[base + 15]]) as usize;

        if width == 32 || (best_dim < width) {
            best_offset = img_offset;
            best_size = img_size;
            best_dim = width;
            if width == 32 { break; }
        }
    }

    if best_size == 0 || best_offset + best_size > ico_bytes.len() {
        return None;
    }

    let img_data = &ico_bytes[best_offset..best_offset + best_size];

    // PNG inside ICO: starts with PNG signature
    if img_data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        let img = image::load_from_memory(img_data).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let pixels = rgba.into_raw();
        tao::window::Icon::from_rgba(pixels, w, h).ok()
    } else {
        None
    }
}
