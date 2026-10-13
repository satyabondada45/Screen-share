// src/webview_app.rs
// DeskStream — Unified Application UI.
//
// Loads the live DeskStream website inside a native Windows WebView2 window.
// The agent engine runs on background threads spawned from main.rs.

use image;
use rfd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Window, WindowBuilder},
};
use wry::WebViewBuilder;
#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;

fn center_window_on_primary_monitor(window: &Window) {
    let Some(monitor) = window.current_monitor().or_else(|| window.primary_monitor()) else {
        return;
    };

    let origin = monitor.position();
    let monitor_size = monitor.size();
    let window_size = window.outer_size();
    let x = origin.x + (monitor_size.width as i32 - window_size.width as i32) / 2;
    let y = origin.y + (monitor_size.height as i32 - window_size.height as i32) / 2;
    window.set_outer_position(PhysicalPosition::new(x, y));
}

/// Runs the WebView application on the main thread.
/// `quit` is shared with the tray controller and closes the application when set.
pub fn run_webview(quit: Arc<AtomicBool>, local_url: String) {
    let event_loop = EventLoopBuilder::<String>::with_user_event().build();
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
        .with_additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI --autoplay-policy=no-user-gesture-required",
        );

    #[cfg(not(target_os = "windows"))]
    let builder = WebViewBuilder::new().with_url(&local_url);

    let proxy_main = proxy.clone();
    let webview = builder
        .with_devtools(false)
        .with_ipc_handler(move |message| {
            if let Err(error) = proxy_main.send_event(message.into_body()) {
                eprintln!("[WEBVIEW IPC] Failed to forward message to the main thread: {error}");
            }
        })
        .with_initialization_script(
            r#"
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
            "#,
        )
        .build(&window)
        .expect("Failed to create WebView2 — is Microsoft Edge WebView2 Runtime installed?");

    let mut overlay_window: Option<Window> = None;
    let mut overlay_webview: Option<wry::WebView> = None;

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Poll;

        if quit.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        match event {
            Event::UserEvent(request) => match request.trim_matches('"') {
                "minimize" => window.set_minimized(true),
                "maximize" => {
                    if !window.is_maximized() {
                        window.set_maximized(true);
                    }
                }
                "toggle_maximize" => {
                    window.set_maximized(!window.is_maximized());
                }
                "send_file" | "send_folder" => {
                    let selection = if request.trim_matches('"') == "send_folder" {
                        rfd::FileDialog::new().pick_folder().map(|path| vec![path])
                    } else {
                        rfd::FileDialog::new().pick_files()
                    };

                    if let Some(paths) = selection {
                        let local_app_data = std::env::var("LOCALAPPDATA")
                            .unwrap_or_else(|_| "C:\\temp".to_string());
                        let app_dir =
                            std::path::Path::new(&local_app_data).join("DeskStream");

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
                                eprintln!("[FILE TX] Failed to queue selected paths: {error}");
                            }
                        }
                    }
                }
                "close" => {
                    quit.store(true, Ordering::Relaxed);
                    *control_flow = ControlFlow::Exit;
                }
                "get_maximize_state" => {
                    let script = format!(
                        "if(window.updateMaximizeIcon) window.updateMaximizeIcon({});",
                        window.is_maximized()
                    );
                    if let Err(error) = webview.evaluate_script(&script) {
                        eprintln!("[SESSION UI] Failed to update maximize state: {error}");
                    }
                }
                "drag_window" => {
                    if let Err(error) = window.drag_window() {
                        eprintln!("[SESSION UI] Failed to drag window: {error}");
                    }
                }
                "open_overlay" => {
                    if overlay_webview.is_none() {
                        let overlay_base = if local_url.contains("/dashboard.html") {
                            local_url.replace("/dashboard.html", "/remote_session.html")
                        } else {
                            local_url.clone()
                        };
                        let url = if overlay_base.contains('?') {
                            format!("{}&overlay=1", overlay_base)
                        } else {
                            format!("{}?overlay=1", overlay_base)
                        };
                        let overlay_win = match WindowBuilder::new()
                            .with_title("DeskStream Overlay")
                            .with_inner_size(LogicalSize::new(360_u32, 90_u32))
                            .with_resizable(false)
                            .with_decorations(false)
                            .with_transparent(true)
                            .with_always_on_top(true)
                            .build(target)
                        {
                            Ok(window) => window,
                            Err(error) => {
                                eprintln!("[SESSION UI] Failed to create overlay window: {error}");
                                return;
                            }
                        };

                        if let Some(monitor) = overlay_win.current_monitor().or_else(|| overlay_win.primary_monitor()) {
                            let origin = monitor.position();
                            let monitor_size = monitor.size();
                            let window_size = overlay_win.outer_size();
                            let x = origin.x + (monitor_size.width as i32 - window_size.width as i32 - 24).max(0);
                            let y = origin.y + 24;
                            overlay_win.set_outer_position(PhysicalPosition::new(x, y));
                        }

                        #[cfg(target_os = "windows")]
                        let overlay_builder = WebViewBuilder::new()
                            .with_url(&url)
                            .with_transparent(true)
                            .with_additional_browser_args(
                                "--disable-features=msWebOOUI,msPdfOOUI --autoplay-policy=no-user-gesture-required",
                            );

                        #[cfg(not(target_os = "windows"))]
                        let overlay_builder =
                            WebViewBuilder::new().with_url(&url).with_transparent(true);

                        let proxy_overlay = proxy.clone();
                        let overlay = match overlay_builder
                            .with_devtools(false)
                            .with_ipc_handler(move |message| {
                                if let Err(error) =
                                    proxy_overlay.send_event(message.into_body())
                                {
                                    eprintln!(
                                        "[WEBVIEW IPC] Failed to forward overlay message: {error}"
                                    );
                                }
                            })
                            .build(&overlay_win)
                        {
                            Ok(overlay) => overlay,
                            Err(error) => {
                                eprintln!("[SESSION UI] Failed to create overlay WebView: {error}");
                                return;
                            }
                        };

                        overlay_window = Some(overlay_win);
                        overlay_webview = Some(overlay);
                    }
                }
                "close_overlay" => {
                    overlay_webview = None;
                    overlay_window = None;
                }
                "drag_overlay" => {
                    if let Some(overlay) = &overlay_window {
                        if let Err(error) = overlay.drag_window() {
                            eprintln!("[SESSION UI] Failed to drag overlay: {error}");
                        }
                    }
                }
                "toggle_b_chat" => {
                    let _ = webview.evaluate_script(
                        "if(typeof toggleBSessionChat === 'function') toggleBSessionChat();",
                    );
                }
                "toggle_b_mic" => {
                    let target_webview = overlay_webview.as_ref().unwrap_or(&webview);
                    if let Err(error) = target_webview.evaluate_script(
                        "if(typeof toggleBSessionMic === 'function') toggleBSessionMic();",
                    ) {
                        eprintln!("[SESSION UI] Failed to toggle microphone: {error}");
                    }
                }
                "request_b_reverse" => {
                    let _ = webview.evaluate_script(
                        "if(typeof requestBSessionReverse === 'function') requestBSessionReverse();",
                    );
                }
                "disconnect_b" => {
                    overlay_webview = None;
                    overlay_window = None;
                    let _ = webview.evaluate_script(
                        "console.info('[DISCONNECT CALLER TRACE] source=END_SESSION_BUTTON timestamp=' + new Date().toISOString() + ' mode=B'); if(typeof disconnectBSession === 'function') disconnectBSession(null, 'END_SESSION_BUTTON');",
                    );
                }
                _ => {}
            },
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if window_id == window.id() {
                    overlay_webview = None;
                    overlay_window = None;
                    quit.store(true, Ordering::Relaxed);
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::Resized(_),
                ..
            } => {
                if window_id == window.id() {
                    let script = format!(
                        "if(window.updateMaximizeIcon) window.updateMaximizeIcon({});",
                        window.is_maximized()
                    );
                    if let Err(error) = webview.evaluate_script(&script) {
                        eprintln!("[SESSION UI] Failed to update maximize state: {error}");
                    }
                }
            }
            _ => {}
        }
    });
}

/// Parses a Windows ICO file and returns an icon for the best available entry.
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
    for index in 0..count {
        let base = 6 + index * 16;
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
        let image_size = u32::from_le_bytes([
            ico_bytes[base + 8],
            ico_bytes[base + 9],
            ico_bytes[base + 10],
            ico_bytes[base + 11],
        ]) as usize;
        let image_offset = u32::from_le_bytes([
            ico_bytes[base + 12],
            ico_bytes[base + 13],
            ico_bytes[base + 14],
            ico_bytes[base + 15],
        ]) as usize;

        if width > 0 && height > 0 && image_size > 0 {
            entries.push((width, height, image_offset, image_size));
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
        let Some(image_data) = ico_bytes.get(offset..end) else {
            continue;
        };
        if let Some(icon) = decode_ico_image(image_data, width, height) {
            return Some(icon);
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn decode_ico_image(
    image_data: &[u8],
    directory_width: u32,
    directory_height: u32,
) -> Option<tao::window::Icon> {
    if image_data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        let decoded = image::load_from_memory(image_data).ok()?;
        let rgba = decoded.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());
        if width != directory_width || height != directory_height {
            return None;
        }
        return tao::window::Icon::from_rgba(rgba.into_raw(), width, height).ok();
    }

    if image_data.len() < 40 {
        return None;
    }

    let header_size = u32::from_le_bytes(image_data[0..4].try_into().ok()?) as usize;
    let bitmap_width = i32::from_le_bytes(image_data[4..8].try_into().ok()?);
    let doubled_height = i32::from_le_bytes(image_data[8..12].try_into().ok()?);
    let planes = u16::from_le_bytes(image_data[12..14].try_into().ok()?);
    let bit_count = u16::from_le_bytes(image_data[14..16].try_into().ok()?);
    let compression = u32::from_le_bytes(image_data[16..20].try_into().ok()?);

    if header_size < 40
        || header_size > image_data.len()
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
    let dib_pixels = image_data.get(header_size..pixels_end)?;
    let mask_stride = width.checked_add(31)?.checked_div(32)?.checked_mul(4)?;
    let mask_bytes = mask_stride.checked_mul(height)?;
    let mask_end = pixels_end.checked_add(mask_bytes)?;
    let and_mask = image_data.get(pixels_end..mask_end);
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
                if mask
                    .get(mask_index)
                    .is_some_and(|byte| byte & (0x80 >> (x % 8)) != 0)
                {
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
