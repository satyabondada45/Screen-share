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
        "A_AUTH_PAGE_INITIALIZED",
        "A_AUTH_PACKET_NOT_SENT",
        "A_AUTH_PACKET_SEND_START",
        "A_AUTH_PACKET_SENT",
        "A_AUTH_IDENTITY_PACKET_SENT",
        "A_AUTH_WEBSOCKET_OPEN",
        "A_AUTH_PACKET_RECEIVED",
        "A_AUTH_ACCEPTED",
        "A_AUTH_CONNECTED_STATE",
        "A_AUTH_FAILURE",
        "A_AUTH_TIMEOUT",
        "A_AUTH_MESSAGE_HANDLER_ERROR",
        "A_AUTH_WEBSOCKET_ERROR",
        "A_AUTH_WEBSOCKET_CLOSED",
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
        "A_TYPE99_SEND",
        "A_TYPE99_SEND_COMPLETE",
        "A_TYPE99_RECEIVE",
        "A_TYPE99_RECEIVE_COMPLETE",
        "A_TYPE99_CLOSE_REQUEST",
        "A_TYPE99_CLOSE_REQUEST_COMPLETE",
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
        "component",
        "role",
        "video_direction",
        "target_system_id",
        "device_id",
        "session_id",
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
        "direction",
        "source",
        "reason",
        "timestamp",
        "connection_id",
        "socket_current",
        "target_id_present",
        "target_id_length",
        "token_present",
        "packet_length",
        "timeout_ms",
        "authenticated",
        "was_clean",
        "page",
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

    let proxy_main = proxy.clone();
    let _webview = builder
        .with_devtools(false)
        .with_ipc_handler(move |msg| {
            let body = msg.into_body();
            if !handle_session_debug_ipc(&body) {
                let _ = proxy_main.send_event(body);
            }
        })
        .with_initialization_script(r#"
            window.__deskstreamDesktop = true;
            window.ipc = {
                postMessage: function (message) {
                    try {
                        if (window.chrome && window.chrome.webview) {
                            window.chrome.webview.postMessage(String(message));
                            return;
                        }
                    } catch (error) {
                        console.error('[DeskStream IPC]', error);
                    }
                }
            };
        "#)
        .build(&window)
        .expect("Failed to create WebView2 — is Microsoft Edge WebView2 Runtime installed?");

    let mut overlay_window: Option<tao::window::Window> = None;
    let mut overlay_webview: Option<wry::WebView> = None;

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Poll;

        // Check quit signal from tray
        if quit.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        match event {
            Event::UserEvent(req) => {
                match req.trim_matches('"') {
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
                        let selection = if req.trim_matches('"') == "send_folder" {
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
                    "get_maximize_state" => {
                        let is_max = window.is_maximized();
                        let script = format!("if(window.updateMaximizeIcon) window.updateMaximizeIcon({});", is_max);
                        let _ = _webview.evaluate_script(&script);
                    }
                    "drag_window" => { let _ = window.drag_window(); },
                    "open_overlay" => {
                        if overlay_webview.is_none() {
                            let url = format!("{}?overlay=1", local_url);
                            let overlay_win = WindowBuilder::new()
                                .with_title("DeskStream Overlay")
                                .with_maximized(true)
                                .with_resizable(false)
                                .with_decorations(false)
                                .with_transparent(true)
                                .with_always_on_top(true)
                                .build(target)
                                .expect("Failed to build overlay window");
                            // Position is managed by JS
                                
                            #[cfg(target_os = "windows")]
                            let builder = WebViewBuilder::new()
                                .with_url(&url)
                                .with_transparent(true)
                                .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI --autoplay-policy=no-user-gesture-required");
                                
                            #[cfg(not(target_os = "windows"))]
                            let builder = WebViewBuilder::new().with_url(&url).with_transparent(true);
                            
                            let proxy_clone = proxy.clone();
                            let overlay_wv = builder
                                .with_ipc_handler(move |msg| {
                                    let _ = proxy_clone.send_event(msg.into_body());
                                })
                                .build(&overlay_win)
                                .expect("Failed to build overlay webview");
                                
                            overlay_window = Some(overlay_win);
                            overlay_webview = Some(overlay_wv);
                        }
                    },
                    "close_overlay" => {
                        overlay_webview = None;
                        overlay_window = None;
                    },
                    "drag_overlay" => {
                        if let Some(win) = &overlay_window {
                            let _ = win.drag_window();
                        }
                    },
                    "toggle_b_chat" => {
                        let _ = _webview.evaluate_script("if(typeof toggleBSessionChat === 'function') toggleBSessionChat();");
                    },
                    "toggle_b_mic" => {
                        if let Some(overlay) = overlay_webview.as_ref() {
                            if let Err(error) = overlay.evaluate_script(
                                "if(typeof toggleBSessionMic === 'function') toggleBSessionMic();",
                            ) {
                                eprintln!("[SESSION UI] Failed to toggle overlay microphone: {error}");
                            }
                        } else {
                            if let Err(error) = _webview.evaluate_script(
                                "if(typeof toggleBSessionMic === 'function') toggleBSessionMic();",
                            ) {
                                eprintln!("[SESSION UI] Failed to toggle microphone: {error}");
                            }
                        }
                    },
                    "request_b_reverse" => {
                        let _ = _webview.evaluate_script("if(typeof requestBSessionReverse === 'function') requestBSessionReverse();");
                    },
                    "disconnect_b" => {
                        let _ = _webview.evaluate_script("if(typeof disconnectBSession === 'function') disconnectBSession();");
                    },
                    _ => {}
                }
            }
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested, .. } => {
                if window_id == window.id() {
                    quit.store(true, Ordering::Relaxed); *control_flow = ControlFlow::Exit;
                }
            }
            Event::WindowEvent { window_id, event: WindowEvent::Resized(_), .. } => {
                if window_id == window.id() {
                    let is_max = window.is_maximized();
                    let script = format!("if(window.updateMaximizeIcon) window.updateMaximizeIcon({});", is_max);
                    let _ = _webview.evaluate_script(&script);
                }
            }
            _ => {}
        }
    });
}

/// Parse a Windows ICO file and return a tao Icon for its 32x32 entry.
#[cfg(target_os = "windows")]
fn load_icon_from_ico(ico_bytes: &[u8]) -> Option<tao::window::Icon> {
    if ico_bytes.len() < 6 {
        return None;
    }
    let reserved = u16::from_le_bytes([ico_bytes[0], ico_bytes[1]]);
    let image_type = u16::from_le_bytes([ico_bytes[2], ico_bytes[3]]);
    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]) as usize;
    if reserved != 0 || image_type != 1 || count == 0 || ico_bytes.len() < 6 + count * 16 {
        return None;
    }

    let mut entries = Vec::with_capacity(count);

    for i in 0..count {
        let base = 6 + i * 16;
        let width = if ico_bytes[base] == 0 {
            256
        } else {
            ico_bytes[base] as u32
        };
        let height = if ico_bytes[base + 1] == 0 {
            256
        } else {
            ico_bytes[base + 1] as u32
        };
        let img_size = u32::from_le_bytes([
            ico_bytes[base + 8],
            ico_bytes[base + 9],
            ico_bytes[base + 10],
            ico_bytes[base + 11],
        ]) as usize;
        let img_offset = u32::from_le_bytes([
            ico_bytes[base + 12],
            ico_bytes[base + 13],
            ico_bytes[base + 14],
            ico_bytes[base + 15],
        ]) as usize;
        if width > 0 && height > 0 && img_size > 0 {
            entries.push((width, height, img_offset, img_size));
        }
    }

    entries.sort_by_key(|(width, height, _, _)| {
        (
            u32::from(*width != 32 || *height != 32),
            std::cmp::Reverse(*width * *height),
        )
    });

    for (width, height, offset, size) in entries {
        let Some(end) = offset.checked_add(size) else {
            continue;
        };
        let Some(img_data) = ico_bytes.get(offset..end) else {
            continue;
        };
        if let Some(icon) = decode_ico_image(img_data, width, height) {
            return Some(icon);
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn decode_ico_image(
    img_data: &[u8],
    directory_width: u32,
    directory_height: u32,
) -> Option<tao::window::Icon> {
    if img_data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        let img = image::load_from_memory(img_data).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        if w != directory_width || h != directory_height {
            return None;
        }
        let pixels = rgba.into_raw();
        return tao::window::Icon::from_rgba(pixels, w, h).ok();
    }

    if img_data.len() < 40 {
        return None;
    }

    let header_size = u32::from_le_bytes(img_data[0..4].try_into().ok()?) as usize;
    let bitmap_width = i32::from_le_bytes(img_data[4..8].try_into().ok()?);
    let doubled_height = i32::from_le_bytes(img_data[8..12].try_into().ok()?);
    let planes = u16::from_le_bytes(img_data[12..14].try_into().ok()?);
    let bit_count = u16::from_le_bytes(img_data[14..16].try_into().ok()?);
    let compression = u32::from_le_bytes(img_data[16..20].try_into().ok()?);

    if header_size < 40
        || header_size > img_data.len()
        || bitmap_width <= 0
        || doubled_height <= 0
        || doubled_height % 2 != 0
        || bitmap_width as u32 != directory_width
        || (doubled_height / 2) as u32 != directory_height
        || planes != 1
        || bit_count != 32
        || compression != 0
    {
        return None;
    }

    let width = directory_width as usize;
    let height = directory_height as usize;
    let pixel_bytes = width.checked_mul(height)?.checked_mul(4)?;
    let pixels_end = header_size.checked_add(pixel_bytes)?;
    let dib_pixels = img_data.get(header_size..pixels_end)?;
    let mask_stride = width.checked_add(31)?.checked_div(32)?.checked_mul(4)?;
    let mask_bytes = mask_stride.checked_mul(height)?;
    let and_mask = img_data.get(pixels_end..pixels_end.checked_add(mask_bytes)?);
    let has_alpha = dib_pixels.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut rgba = vec![0; pixel_bytes];

    for y in 0..height {
        let source_row = height - 1 - y;
        for x in 0..width {
            let source = (source_row * width + x) * 4;
            let target = (y * width + x) * 4;
            let mut alpha = if has_alpha {
                dib_pixels[source + 3]
            } else {
                255
            };

            if let Some(mask) = and_mask {
                let mask_index = source_row * mask_stride + x / 8;
                if mask.get(mask_index).is_some_and(|byte| byte & (0x80 >> (x % 8)) != 0) {
                    alpha = 0;
                }
            }

            rgba[target] = dib_pixels[source + 2];
            rgba[target + 1] = dib_pixels[source + 1];
            rgba[target + 2] = dib_pixels[source];
            rgba[target + 3] = alpha;
        }
    }

    tao::window::Icon::from_rgba(rgba, directory_width, directory_height).ok()
}

#[cfg(all(test, target_os = "windows"))]
mod icon_tests {
    use super::load_icon_from_ico;

    #[test]
    fn bundled_dib_ico_loads_as_a_window_icon() {
        assert!(load_icon_from_ico(include_bytes!("../assets/icon.ico")).is_some());
    }

    #[test]
    fn malformed_ico_is_rejected() {
        assert!(load_icon_from_ico(&[0, 0, 1, 0, 1, 0]).is_none());
    }
}
