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
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::{Window, WindowBuilder},
};
#[cfg(target_os = "windows")]
use tao::platform::windows::WindowExtWindows;
use wry::WebViewBuilder;
#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;

const IPC_INITIALIZATION_SCRIPT: &str = r#"
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
"#;

const OVERLAY_INITIALIZATION_SCRIPT: &str = r#"
    window.__deskstreamOverlayWindow = true;
    (function prepareTransparentOverlayDocument() {
        if (new URLSearchParams(window.location.search).get('overlay') !== '1') return;

        const applyOverlayStyles = () => {
            if (document.documentElement) {
                document.documentElement.classList.add('deskstream-overlay');
                document.documentElement.style.setProperty('background', 'transparent', 'important');
                document.documentElement.style.setProperty('background-color', 'transparent', 'important');
            }
            if (document.head && !document.getElementById('deskstream-overlay-transparent-root')) {
                const style = document.createElement('style');
                style.id = 'deskstream-overlay-transparent-root';
                style.textContent = `
                    html, body, #bSessionUiMount, #bSessionUiRoot, #bSessionOverlay {
                        background: transparent !important;
                        background-color: transparent !important;
                        background-image: none !important;
                        border: 0 !important;
                        box-shadow: none !important;
                    }
                    html, body {
                        margin: 0 !important;
                    }
                `;
                document.head.appendChild(style);
            }
            if (document.body) {
                document.body.style.setProperty('background', 'transparent', 'important');
                document.body.style.setProperty('background-color', 'transparent', 'important');
                document.body.style.setProperty('margin', '0', 'important');
            }
            if (!document.head || !document.body) requestAnimationFrame(applyOverlayStyles);
        };
        applyOverlayStyles();
    })();
"#;

fn resize_overlay(window: &Window, layout: &str) {
    let (width, height) = match layout {
        // menu: wide enough for the 204px menu + 8px gap + 48px button = 260; round up to 268
        // height: 5 items × 38px + separators + padding + button margin = ~300
        "menu"  => (268_u32, 300_u32),
        "chat"  => (360_u32, 460_u32),
        "panel" => (380_u32, 460_u32),
        // idle: just the 40px button + 4px margin each side
        _       => (48_u32, 48_u32),
    };
    let previous_position = window.outer_position().ok();
    let previous_size = window.outer_size();
    let target_size = LogicalSize::new(width, height).to_physical::<u32>(window.scale_factor());
    window.set_inner_size(LogicalSize::new(width, height));

    if let Some(position) = previous_position {
        let (x, y) = right_aligned_overlay_position(
            (position.x, position.y),
            previous_size.width,
            target_size.width,
        );
        let requested = PhysicalPosition::new(x, y);
        #[cfg(target_os = "windows")]
        set_overlay_position_clamped_to_size(
            window,
            requested,
            PhysicalSize::new(target_size.width, target_size.height),
        );
        #[cfg(not(target_os = "windows"))]
        window.set_outer_position(requested);
    }
}

fn right_aligned_overlay_position(
    previous_position: (i32, i32),
    previous_width: u32,
    target_width: u32,
) -> (i32, i32) {
    let right = previous_position
        .0
        .saturating_add(previous_width.min(i32::MAX as u32) as i32);
    (
        right.saturating_sub(target_width.min(i32::MAX as u32) as i32),
        previous_position.1,
    )
}

fn clamp_overlay_position(
    requested: (i32, i32),
    size: (u32, u32),
    work_area: (i32, i32, i32, i32),
) -> (i32, i32) {
    let max_x = work_area.2.saturating_sub(size.0.min(i32::MAX as u32) as i32);
    let max_y = work_area.3.saturating_sub(size.1.min(i32::MAX as u32) as i32);
    (
        requested.0.clamp(work_area.0, max_x.max(work_area.0)),
        requested.1.clamp(work_area.1, max_y.max(work_area.1)),
    )
}

#[cfg(target_os = "windows")]
fn set_overlay_position_clamped(window: &Window, requested: PhysicalPosition<i32>) -> bool {
    set_overlay_position_clamped_to_size(window, requested, window.outer_size())
}

#[cfg(target_os = "windows")]
fn set_overlay_position_clamped_to_size(
    window: &Window,
    requested: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> bool {
    use windows_sys::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
    };

    let monitor_point = POINT {
        x: requested.x.saturating_add((size.width / 2).min(i32::MAX as u32) as i32),
        y: requested.y.saturating_add((size.height / 2).min(i32::MAX as u32) as i32),
    };
    let monitor = unsafe { MonitorFromPoint(monitor_point, MONITOR_DEFAULTTONEAREST) };
    if monitor == 0 {
        eprintln!("[SESSION UI] Could not find a monitor for overlay positioning");
        return false;
    }

    let mut monitor_info: MONITORINFO = unsafe { std::mem::zeroed() };
    monitor_info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut monitor_info) } == 0 {
        eprintln!("[SESSION UI] Could not read monitor work area for overlay positioning");
        return false;
    }

    let work = monitor_info.rcWork;
    let (x, y) = clamp_overlay_position(
        (requested.x, requested.y),
        (size.width, size.height),
        (work.left, work.top, work.right, work.bottom),
    );
    let target = PhysicalPosition::new(x, y);
    match window.outer_position() {
        Ok(current) if current == target => {}
        Ok(_) => window.set_outer_position(target),
        Err(error) => {
            eprintln!("[SESSION UI] Could not read current overlay position: {error}");
            return false;
        }
    }
    true
}

#[cfg(target_os = "windows")]
fn set_overlay_input_region(window: &Window, regions: &str) {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
    };

    let scale = window.scale_factor();
    let combined_region = unsafe { CreateRectRgn(0, 0, 0, 0) };
    if combined_region == 0 {
        eprintln!("[SESSION UI] Failed to allocate overlay input region");
        return;
    }

    let mut region_count = 0;
    for rectangle in regions.split(';') {
        let values = rectangle
            .split(',')
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(values) = values else {
            continue;
        };
        if values.len() != 4 || values.iter().any(|value| !value.is_finite()) {
            continue;
        }

        let left = (values[0] * scale).floor() as i32;
        let top = (values[1] * scale).floor() as i32;
        let right = ((values[0] + values[2]) * scale).ceil() as i32;
        let bottom = ((values[1] + values[3]) * scale).ceil() as i32;
        if right <= left || bottom <= top {
            continue;
        }

        let rectangle_region = unsafe { CreateRectRgn(left, top, right, bottom) };
        if rectangle_region == 0 {
            eprintln!("[SESSION UI] Failed to allocate overlay control region");
            continue;
        }
        let combine_result =
            unsafe { CombineRgn(combined_region, combined_region, rectangle_region, RGN_OR) };
        unsafe { DeleteObject(rectangle_region) };
        if combine_result == 0 {
            unsafe { DeleteObject(combined_region) };
            eprintln!("[SESSION UI] Failed to combine overlay input regions");
            return;
        }
        region_count += 1;
    }

    if region_count == 0 {
        unsafe { DeleteObject(combined_region) };
        return;
    }

    let result = unsafe { SetWindowRgn(window.hwnd(), combined_region, 1) };
    if result == 0 {
        unsafe { DeleteObject(combined_region) };
        eprintln!("[SESSION UI] Failed to apply overlay input region");
    }
}

fn destroy_overlay(window: &mut Option<Window>, webview: &mut Option<wry::WebView>) {
    drop(webview.take());
    if let Some(window) = window.take() {
        let _ = window.set_visible(false);
        drop(window);
    }
}

#[allow(dead_code)]
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

fn controller_dashboard_url(local_url: &str) -> String {
    let url_without_fragment = local_url.split_once('#').map_or(local_url, |(url, _)| url);
    let (url_without_query, query) = url_without_fragment
        .split_once('?')
        .map_or((url_without_fragment, None), |(url, query)| (url, Some(query)));
    let path_start = url_without_query.find("://").map_or(0, |scheme_end| {
        url_without_query[scheme_end + 3..]
            .find('/')
            .map_or(url_without_query.len(), |path_start| scheme_end + 3 + path_start)
    });
    let path = &url_without_query[path_start..];
    let filename = path.rsplit('/').next().unwrap_or_default();
    let path_is_directory = path.is_empty()
        || path.ends_with('/')
        || !filename.contains('.');
    let dashboard_base = if path.is_empty() || (!path.ends_with('/') && !filename.contains('.')) {
        format!("{url_without_query}/")
    } else if path.ends_with('/') {
        url_without_query.to_string()
    } else {
        let directory_end = url_without_query
            .rfind('/')
            .filter(|index| *index >= path_start)
            .unwrap_or(path_start);
        url_without_query[..=directory_end].to_string()
    };
    let mut dashboard_url = format!("{dashboard_base}dashboard.html");
    if filename == "dashboard.html" || path_is_directory {
        if let Some(query) = query {
            dashboard_url.push('?');
            dashboard_url.push_str(query);
        }
    }
    dashboard_url
}

fn url_with_overlay_parameter(url: &str) -> String {
    let (url_without_fragment, fragment) = url
        .split_once('#')
        .map_or((url, None), |(url, fragment)| (url, Some(fragment)));
    let separator = if url_without_fragment.contains('?') {
        '&'
    } else {
        '?'
    };
    let mut overlay_url = format!("{url_without_fragment}{separator}overlay=1");
    if let Some(fragment) = fragment {
        overlay_url.push('#');
        overlay_url.push_str(fragment);
    }
    overlay_url
}

fn controller_overlay_url(controller_url: &str, session_token: u64) -> String {
    let base = if let Some(idx) = controller_url.rfind('/') {
        &controller_url[..=idx]
    } else {
        controller_url
    };
    format!("{base}remote_session.html?overlay=1&overlay_session={session_token}")
}

/// Runs the WebView application on the main thread.
/// `quit` is shared with the tray controller and closes the application when set.
pub fn run_webview(quit: Arc<AtomicBool>, local_url: String) {
    let controller_url = controller_dashboard_url(&local_url);
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
        .with_url(&controller_url)
        .with_additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI --autoplay-policy=no-user-gesture-required",
        );

    #[cfg(not(target_os = "windows"))]
    let builder = WebViewBuilder::new().with_url(&controller_url);

    let proxy_main = proxy.clone();
    let webview = builder
        .with_devtools(false)
        .with_ipc_handler(move |message| {
            if let Err(error) = proxy_main.send_event(format!("main:{}", message.into_body())) {
                eprintln!("[WEBVIEW IPC] Failed to forward message to the main thread: {error}");
            }
        })
        .with_initialization_script(IPC_INITIALIZATION_SCRIPT)
        .build(&window)
        .expect("Failed to create WebView2 — is Microsoft Edge WebView2 Runtime installed?");

    let mut overlay_window: Option<Window> = None;
    let mut overlay_webview: Option<wry::WebView> = None;
    let mut active_overlay_token: Option<u64> = None;
    let mut last_overlay_token = 0_u64;
    let mut active_overlay_layout = "idle";
    let local_system_id = crate::status::LIVE
        .get()
        .and_then(|status| status.lock().ok().map(|status| status.system_id.clone()))
        .unwrap_or_else(|| "unavailable".to_string());

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Poll;

        if quit.load(Ordering::Relaxed) {
            *control_flow = ControlFlow::Exit;
            return;
        }

        match event {
            Event::UserEvent(request) => {
                let request = request.trim_matches('"');
                let (source, request_body) =
                    request.split_once(':').unwrap_or(("main", request));
                let (command, argument) = request_body.split_once(':').unwrap_or((request_body, ""));
                let requested_overlay_token = argument.parse::<u64>().ok();
                match command {
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
                    let selection = if command == "send_folder" {
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
                    if source != "main" {
                        eprintln!("[SESSION UI] Ignoring overlay creation request from {source}");
                    } else if requested_overlay_token
                        .is_some_and(|token| token == active_overlay_token.unwrap_or_default())
                        && overlay_webview.is_some()
                        && overlay_window.is_some()
                    {
                        if let Some(overlay_window) = &overlay_window {
                            overlay_window.set_visible(true);
                        }
                    } else if requested_overlay_token
                        .is_some_and(|token| token <= last_overlay_token)
                    {
                        eprintln!("[SESSION UI] Ignoring stale overlay session request");
                    } else if requested_overlay_token.is_none()
                        && overlay_webview.is_some()
                        && overlay_window.is_some()
                    {
                        if let Some(overlay_window) = &overlay_window {
                            overlay_window.set_visible(true);
                        }
                    } else {
                        destroy_overlay(&mut overlay_window, &mut overlay_webview);
                        let token = requested_overlay_token.unwrap_or_else(|| {
                            last_overlay_token.saturating_add(1)
                        });
                        let url = controller_overlay_url(&controller_url, token);
                        let overlay_win = match WindowBuilder::new()
                            .with_title("DeskStream Overlay")
                        .with_inner_size(LogicalSize::new(48_u32, 48_u32))
                            .with_resizable(false)
                            .with_decorations(false)
                            .with_transparent(true)
                            .with_always_on_top(true)
                            .with_focused(false)
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
                            #[cfg(target_os = "windows")]
                            set_overlay_position_clamped(
                                &overlay_win,
                                PhysicalPosition::new(x, y),
                            );
                            #[cfg(not(target_os = "windows"))]
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
                                    proxy_overlay.send_event(format!("overlay:{}", message.into_body()))
                                {
                                    eprintln!(
                                        "[WEBVIEW IPC] Failed to forward overlay message: {error}"
                                    );
                                }
                            })
                            .with_initialization_script(IPC_INITIALIZATION_SCRIPT)
                            .with_initialization_script(OVERLAY_INITIALIZATION_SCRIPT)
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
                        active_overlay_token = Some(token);
                        last_overlay_token = last_overlay_token.max(token);
                        active_overlay_layout = "idle";
                    }
                }
                "close_overlay" => {
                    if let Some(token) = requested_overlay_token {
                        if active_overlay_token == Some(token) {
                            destroy_overlay(&mut overlay_window, &mut overlay_webview);
                            active_overlay_token = None;
                            active_overlay_layout = "idle";
                        } else {
                            eprintln!("[SESSION UI] Ignoring stale overlay close request");
                        }
                    } else {
                        destroy_overlay(&mut overlay_window, &mut overlay_webview);
                        active_overlay_token = None;
                        active_overlay_layout = "idle";
                    }
                }
                "drag_overlay" => {
                    if source == "overlay" {
                        if let Some(overlay) = &overlay_window {
                            if let Err(error) = overlay.drag_window() {
                                eprintln!("[SESSION UI] Failed to drag overlay: {error}");
                            }
                        }
                    }
                }
                "overlay_layout" => {
                    if source == "overlay" {
                        if let Some(overlay) = &overlay_window {
                            if active_overlay_layout != argument {
                                resize_overlay(overlay, argument);
                                active_overlay_layout = match argument {
                                    "menu" => "menu",
                                    "chat" => "chat",
                                    "panel" => "panel",
                                    _ => "idle",
                                };
                                if let Some(overlay) = &overlay_webview {
                                    if let Err(error) = overlay.evaluate_script(
                                        "requestAnimationFrame(() => { if (typeof syncBOverlayLayout === 'function') syncBOverlayLayout(); });",
                                    ) {
                                        eprintln!("[SESSION UI] Failed to refresh overlay bounds after resize: {error}");
                                    }
                                }
                            }
                        }
                    }
                }
                "overlay_region" => {
                    if source == "overlay" {
                        #[cfg(target_os = "windows")]
                        if let Some(overlay) = &overlay_window {
                            set_overlay_input_region(overlay, argument);
                        }
                    }
                }
                "arm_reverse_acceptance" => {
                    if source != "overlay" {
                        eprintln!("[REVERSE] Ignoring auto-accept arm request from {source}");
                    } else {
                        let mut parts = argument.split(':');
                        let peer_id = parts.next().unwrap_or_default();
                        let token = parts.next().and_then(|value| value.parse::<u64>().ok());
                        let request_id = parts.next().unwrap_or_default();
                        let valid_peer_id =
                            peer_id.len() == 9 && peer_id.bytes().all(|byte| byte.is_ascii_digit());
                        let valid_request_id = !request_id.is_empty()
                            && request_id.len() <= 64
                            && request_id.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                            });
                        let active_peer_matches = matches!(
                            crate::status::session_peer_system_id(),
                            Ok(Some(active_peer)) if active_peer == peer_id
                        );
                        let active_session_matches = crate::status::LIVE
                            .get()
                            .and_then(|status| status.lock().ok())
                            .is_some_and(|status| status.in_session);
                        if !valid_peer_id
                            || !valid_request_id
                            || parts.next().is_some()
                            || token != active_overlay_token
                            || !active_peer_matches
                            || !active_session_matches
                        {
                            crate::session_debug::log(
                                &local_system_id,
                                &format!(
                                    "[REVERSE TRACE] role=HOST direction=LOCAL stage=AUTO_ACCEPT_ARM_REJECTED message_type=31 request_id={} active_session_id=unavailable peer_system_id={} result=peer_session_or_overlay_validation_failed",
                                    if valid_request_id { request_id } else { "invalid" },
                                    if valid_peer_id { peer_id } else { "invalid" }
                                ),
                            );
                            if valid_peer_id && valid_request_id {
                                if let Some(overlay) = &overlay_webview {
                                    let script = format!(
                                        "window.postMessage({{ type: 'reverse_autoaccept_arm_result', peer_id: '{peer_id}', request_id: '{request_id}', armed: false }}, '*');"
                                    );
                                    if let Err(error) = overlay.evaluate_script(&script) {
                                        eprintln!("[REVERSE] Could not return validation failure to the session overlay: {error}");
                                    }
                                }
                            }
                            eprintln!("[REVERSE] Refusing to arm incoming approval for a stale or mismatched session");
                        } else {
                            let script = format!(
                                "window.__deskstreamReverseHandoffRequestId = '{request_id}'; window.__deskstreamPendingReverseAcceptance = {{ requesterId: '{peer_id}', requestId: '{request_id}', expiresAt: Date.now() + 60000 }}; if (typeof processPendingReverseAcceptance === 'function') processPendingReverseAcceptance();"
                            );
                            match webview.evaluate_script(&script) {
                                Ok(()) => {
                                    crate::session_debug::log(
                                        &local_system_id,
                                        &format!(
                                            "[REVERSE TRACE] role=HOST direction=LOCAL stage=AUTO_ACCEPT_ARM_DISPATCHED message_type=31 request_id={} active_session_id=unavailable peer_system_id={} result=dashboard_script_submitted",
                                            request_id, peer_id
                                        ),
                                    );
                                }
                                Err(error) => {
                                    crate::session_debug::log(
                                        &local_system_id,
                                        &format!(
                                            "[REVERSE TRACE] role=HOST direction=LOCAL stage=AUTO_ACCEPT_ARM_FAILED message_type=31 request_id={} active_session_id=unavailable peer_system_id={} result=dashboard_script_error reason={}",
                                            request_id, peer_id, error
                                        ),
                                    );
                                    if let Some(overlay) = &overlay_webview {
                                        let script = format!(
                                            "window.postMessage({{ type: 'reverse_autoaccept_arm_result', peer_id: '{peer_id}', request_id: '{request_id}', armed: false }}, '*');"
                                        );
                                        if let Err(overlay_error) = overlay.evaluate_script(&script) {
                                            eprintln!("[REVERSE] Could not return dashboard dispatch failure to the session overlay: {overlay_error}");
                                        }
                                    }
                                    eprintln!("[REVERSE] Failed to arm incoming approval: {error}");
                                }
                            }
                        }
                    }
                }
                "cancel_reverse_acceptance" => {
                    if source != "overlay" {
                        eprintln!("[REVERSE] Ignoring incoming-approval cancellation from {source}");
                    } else {
                        let mut parts = argument.split(':');
                        let peer_id = parts.next().unwrap_or_default();
                        let token = parts.next().and_then(|value| value.parse::<u64>().ok());
                        let request_id = parts.next().unwrap_or_default();
                        let valid_peer_id =
                            peer_id.len() == 9 && peer_id.bytes().all(|byte| byte.is_ascii_digit());
                        let valid_request_id = !request_id.is_empty()
                            && request_id.len() <= 64
                            && request_id.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                            });
                        if !valid_peer_id
                            || !valid_request_id
                            || parts.next().is_some()
                            || token != active_overlay_token
                        {
                            eprintln!("[REVERSE] Ignoring invalid or stale incoming-approval cancellation");
                        } else {
                            let script = format!(
                                "if (window.__deskstreamPendingReverseAcceptance?.requestId === '{request_id}') window.__deskstreamPendingReverseAcceptance = null; if (typeof clearReverseAutoAccept === 'function') clearReverseAutoAccept('{peer_id}', '{request_id}');"
                            );
                            if let Err(error) = webview.evaluate_script(&script) {
                                eprintln!("[REVERSE] Failed to clear pending reverse approval: {error}");
                            }
                        }
                    }
                }
                "complete_reverse_host_handoff" => {
                    if source != "overlay" {
                        eprintln!("[REVERSE] Ignoring host handoff completion from {source}");
                    } else {
                        let mut parts = argument.split(':');
                        let peer_id = parts.next().unwrap_or_default();
                        let token = parts.next().and_then(|value| value.parse::<u64>().ok());
                        let request_id = parts.next().unwrap_or_default();
                        let valid_peer_id =
                            peer_id.len() == 9 && peer_id.bytes().all(|byte| byte.is_ascii_digit());
                        let valid_request_id = !request_id.is_empty()
                            && request_id.len() <= 64
                            && request_id.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                            });
                        let session_still_active = crate::status::LIVE
                            .get()
                            .and_then(|status| status.lock().ok())
                            .is_some_and(|status| status.in_session);
                        if !valid_peer_id
                            || !valid_request_id
                            || parts.next().is_some()
                            || token != active_overlay_token
                            || session_still_active
                        {
                            eprintln!("[REVERSE] Ignoring invalid, stale, or premature reverse host handoff completion");
                        } else {
                            crate::session_debug::log(
                                &local_system_id,
                                &format!(
                                    "[REVERSE TRACE] role=HOST direction=LOCAL stage=OLD_SESSION_CLOSED message_type=31 request_id={} active_session_id=unavailable peer_system_id={} result=waiting_for_reverse_request",
                                    request_id, peer_id
                                ),
                            );
                            destroy_overlay(&mut overlay_window, &mut overlay_webview);
                            active_overlay_token = None;
                            active_overlay_layout = "idle";
                            window.set_minimized(false);
                            window.set_visible(true);
                            if let Err(error) = webview.evaluate_script(
                                "if (typeof showToast === 'function') showToast('Reverse handoff prepared. Waiting for the peer to connect...');",
                            ) {
                                eprintln!("[REVERSE] Could not update dashboard handoff status: {error}");
                            }
                        }
                    }
                }
                "start_reverse_connection" => {
                    eprintln!("[REVERSE] Ignoring obsolete same-direction reconnect command from {source}");
                }
                "reverse_handoff_trace" => {
                    if source != "main" {
                        eprintln!("[REVERSE] Ignoring handoff trace from {source}");
                    } else {
                        let mut parts = argument.split(':');
                        let request_id = parts.next().unwrap_or_default();
                        let stage = parts.next().unwrap_or_default();
                        let peer_id = parts.next().unwrap_or_default();
                        let valid_request_id = !request_id.is_empty()
                            && request_id.len() <= 64
                            && request_id.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                            });
                        let valid_peer_id =
                            peer_id.len() == 9 && peer_id.bytes().all(|byte| byte.is_ascii_digit());
                        if !valid_request_id
                            || !valid_peer_id
                            || parts.next().is_some()
                            || !matches!(
                                stage,
                                "DASHBOARD_DISPATCH"
                                    | "DASHBOARD_NOT_READY"
                                    | "AUTO_ACCEPT_ARMED"
                                    | "AUTO_ACCEPT_ARM_FAILED"
                                    | "AUTO_ACCEPT_CANCELLED"
                                    | "AUTO_ACCEPT_EXPIRED"
                                    | "CONNECTION_STARTED"
                                    | "REQUEST_CREATED"
                                    | "REQUEST_ACCEPTED"
                                    | "REQUEST_FAILED"
                                    | "START_CONNECTION_FAILED"
                                    | "SESSION_UI_OPENED"
                                    | "SESSION_CONNECTED"
                                    | "AUTHORIZATION_FAILED"
                            )
                        {
                            eprintln!("[REVERSE] Ignoring malformed handoff trace");
                        } else {
                            crate::session_debug::log(
                                &local_system_id,
                                &format!(
                                    "[REVERSE TRACE] role=HOST direction=LOCAL stage={} message_type=31 request_id={} active_session_id=not_carried_by_protocol peer_system_id={} result=observed",
                                    stage, request_id, peer_id
                                ),
                            );
                            if matches!(stage, "AUTO_ACCEPT_ARMED" | "AUTO_ACCEPT_ARM_FAILED") {
                                if let Some(overlay) = &overlay_webview {
                                    let armed = stage == "AUTO_ACCEPT_ARMED";
                                    let script = format!(
                                        "window.postMessage({{ type: 'reverse_autoaccept_arm_result', peer_id: '{peer_id}', request_id: '{request_id}', armed: {armed} }}, '*');"
                                    );
                                    if let Err(error) = overlay.evaluate_script(&script) {
                                        eprintln!("[REVERSE] Could not return auto-approval result to the session overlay: {error}");
                                    }
                                }
                            }
                        }
                    }
                }
                "reverse_session_trace" => {
                    if source != "main" {
                        eprintln!("[REVERSE] Ignoring session trace from {source}");
                    } else {
                        let mut parts = argument.split(':');
                        let stage = parts.next().unwrap_or_default();
                        let session_id = parts.next().unwrap_or_default();
                        if stage.is_empty()
                            || !stage
                                .bytes()
                                .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
                            || session_id.len() != 9
                            || !session_id.bytes().all(|byte| byte.is_ascii_digit())
                            || parts.next().is_some()
                            || !matches!(
                                stage,
                                "TYPE30_RECEIVED"
                                    | "TYPE31_ACCEPT_SENT"
                                    | "TYPE31_REJECT_SENT"
                                    | "APPROVAL_POSTED"
                                    | "APPROVAL_ARMED"
                                    | "AUTO_ACCEPT_MATCHED"
                                    | "AUTO_ACCEPT_SUCCEEDED"
                                    | "SIMULTANEOUS_REQUEST_DECLINED"
                            )
                        {
                            eprintln!("[REVERSE] Ignoring malformed session trace");
                        } else {
                            crate::session_debug::log(
                                &local_system_id,
                                &format!(
                                    "[REVERSE TRACE] role=VIEWER direction=LOCAL stage={} message_type={} request_id=not_carried_by_wire active_session_id={} peer_system_id={} result=observed",
                                    stage,
                                    if stage.starts_with("TYPE31") { 31 } else { 30 },
                                    session_id,
                                    session_id
                                ),
                            );
                        }
                    }
                }
                "toggle_b_chat" | "toggle_b_mic" | "request_b_reverse" => {
                    let target_webview = if source == "overlay" {
                        overlay_webview.as_ref()
                    } else {
                        Some(&webview)
                    };
                    let script = match command {
                        "toggle_b_chat" => "if(typeof toggleBSessionChat === 'function') toggleBSessionChat();",
                        "toggle_b_mic" => "if(typeof toggleBSessionMic === 'function') toggleBSessionMic();",
                        "request_b_reverse" => "if(typeof requestBSessionReverse === 'function') requestBSessionReverse();",
                        _ => unreachable!(),
                    };
                    if let Some(target_webview) = target_webview {
                        if let Err(error) = target_webview.evaluate_script(script) {
                            eprintln!("[SESSION UI] Failed to dispatch {command} to {source} WebView: {error}");
                        }
                    } else {
                        eprintln!("[SESSION UI] Cannot dispatch {command}: {source} WebView is unavailable");
                    }
                }
                "disconnect_b" => {
                    let target_webview = if source == "overlay" {
                        overlay_webview.as_ref()
                    } else {
                        Some(&webview)
                    };
                    if let Some(target_webview) = target_webview {
                        if let Err(error) = target_webview.evaluate_script(
                            "if(typeof handleBSessionDisconnectClick === 'function') handleBSessionDisconnectClick(); else if(typeof window.deskStreamEndAnySession === 'function') window.deskStreamEndAnySession('END_SESSION_BUTTON'); else if(typeof disconnectBSession === 'function') disconnectBSession(null, 'END_SESSION_BUTTON');",
                        ) {
                            eprintln!("[SESSION UI] Failed to dispatch disconnect to {source} WebView: {error}");
                        }
                    } else {
                        eprintln!("[SESSION UI] Cannot disconnect: {source} WebView is unavailable");
                    }
                }
                _ => {}
                }
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if overlay_window.as_ref().is_some_and(|overlay| overlay.id() == window_id) {
                    if let Some(overlay) = &overlay_webview {
                        if let Err(error) = overlay.evaluate_script(
                            "if(typeof handleBSessionDisconnectClick === 'function') handleBSessionDisconnectClick(); else if(typeof disconnectBSession === 'function') disconnectBSession(null, 'OVERLAY_WINDOW_CLOSED');",
                        ) {
                            eprintln!("[SESSION UI] Failed to end session after overlay close: {error}");
                        }
                    }
                } else if window_id == window.id() {
                    destroy_overlay(&mut overlay_window, &mut overlay_webview);
                    active_overlay_token = None;
                    active_overlay_layout = "idle";
                    quit.store(true, Ordering::Relaxed);
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::Moved(position),
                ..
            } => {
                if let Some(overlay) = &overlay_window {
                    if overlay.id() == window_id && !set_overlay_position_clamped(overlay, position) {
                        eprintln!("[SESSION UI] Could not clamp overlay during native drag");
                    }
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
                } else if let Some(overlay) = &overlay_window {
                    if overlay.id() == window_id {
                        if let Ok(position) = overlay.outer_position() {
                            if !set_overlay_position_clamped(overlay, position) {
                                eprintln!("[SESSION UI] Could not clamp overlay after resize");
                            }
                        }
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
    let height_valid = doubled_height == directory_height as i32
        || (doubled_height == (directory_height * 2) as i32);

    if header_size < 40
        || header_size > image_data.len()
        || bitmap_width <= 0
        || doubled_height <= 0
        || bitmap_width as u32 != directory_width
        || !height_valid
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

    #[test]
    fn decode_ico_image_accepts_uncompressed_dib() {
        let width = 1u32;
        let height = 1u32;
        let mut image_data = Vec::new();
        image_data.extend_from_slice(&40u32.to_le_bytes());
        image_data.extend_from_slice(&(width as i32).to_le_bytes());
        image_data.extend_from_slice(&((height * 2) as i32).to_le_bytes());
        image_data.extend_from_slice(&1u16.to_le_bytes());
        image_data.extend_from_slice(&32u16.to_le_bytes());
        image_data.extend_from_slice(&0u32.to_le_bytes());
        image_data.extend_from_slice(&0u32.to_le_bytes());
        image_data.extend_from_slice(&0i32.to_le_bytes());
        image_data.extend_from_slice(&0i32.to_le_bytes());
        image_data.extend_from_slice(&0u32.to_le_bytes());
        image_data.extend_from_slice(&0u32.to_le_bytes());
        image_data.extend_from_slice(&[0x12, 0x34, 0x56, 0xFF]);
        image_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        let icon = super::decode_ico_image(&image_data, width, height);
        assert!(icon.is_some());
    }

    #[test]
    fn decode_ico_image_rejects_unsupported_compression_format() {
        let width = 1u32;
        let height = 1u32;
        let mut image_data = Vec::new();
        image_data.extend_from_slice(&40u32.to_le_bytes());
        image_data.extend_from_slice(&(width as i32).to_le_bytes());
        image_data.extend_from_slice(&((height * 2) as i32).to_le_bytes());
        image_data.extend_from_slice(&1u16.to_le_bytes());
        image_data.extend_from_slice(&32u16.to_le_bytes());
        image_data.extend_from_slice(&1u32.to_le_bytes());
        image_data.extend_from_slice(&0u32.to_le_bytes());
        image_data.extend_from_slice(&0i32.to_le_bytes());
        image_data.extend_from_slice(&0i32.to_le_bytes());
        image_data.extend_from_slice(&0u32.to_le_bytes());
        image_data.extend_from_slice(&0u32.to_le_bytes());
        image_data.extend_from_slice(&[0x12, 0x34, 0x56, 0xFF]);
        image_data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        let icon = super::decode_ico_image(&image_data, width, height);
        assert!(icon.is_none());
    }
}

#[cfg(test)]
mod controller_url_tests {
    use super::{
        clamp_overlay_position, controller_dashboard_url, right_aligned_overlay_position,
        url_with_overlay_parameter,
    };

    #[test]
    fn overlay_position_is_clamped_to_the_monitor_work_area() {
        assert_eq!(
            clamp_overlay_position((900, 700), (220, 220), (0, 0, 1000, 800)),
            (780, 580)
        );
        assert_eq!(
            clamp_overlay_position((900, 24), (268, 300), (0, 0, 1000, 800)),
            (732, 24)
        );
        assert_eq!(
            clamp_overlay_position((-1600, -900), (48, 48), (-1600, -900, 0, 0)),
            (-1600, -900)
        );
    }

    #[test]
    fn resizing_overlay_keeps_its_right_edge_anchored() {
        assert_eq!(
            right_aligned_overlay_position((952, 24), 48, 268),
            (732, 24)
        );
    }

    #[test]
    fn direct_session_urls_are_routed_to_the_controller_dashboard() {
        assert_eq!(
            controller_dashboard_url(
                "http://127.0.0.1:49182/remote_session.html?id=123456789&token=secret"
            ),
            "http://127.0.0.1:49182/dashboard.html"
        );
        assert_eq!(
            controller_dashboard_url("http://127.0.0.1:49182/session.html?integrated=1"),
            "http://127.0.0.1:49182/dashboard.html"
        );
        assert_eq!(
            controller_dashboard_url("http://127.0.0.1:49182/controller_session.html"),
            "http://127.0.0.1:49182/dashboard.html"
        );
    }

    #[test]
    fn arbitrary_local_urls_are_routed_to_dashboard_in_the_same_directory() {
        assert_eq!(
            controller_dashboard_url("http://127.0.0.1:49182/auth/index.html"),
            "http://127.0.0.1:49182/auth/dashboard.html"
        );
        assert_eq!(
            controller_dashboard_url("http://127.0.0.1:49182/DeskStream"),
            "http://127.0.0.1:49182/DeskStream/dashboard.html"
        );
    }

    #[test]
    fn dashboard_url_and_its_query_are_preserved() {
        assert_eq!(
            controller_dashboard_url("http://127.0.0.1:49182/dashboard.html?mode=desktop"),
            "http://127.0.0.1:49182/dashboard.html?mode=desktop"
        );
    }

    #[test]
    fn local_server_root_is_expanded_to_the_dashboard_route() {
        assert_eq!(
            controller_dashboard_url("http://127.0.0.1:49182/"),
            "http://127.0.0.1:49182/dashboard.html"
        );
        assert_eq!(
            controller_dashboard_url("http://127.0.0.1:49182"),
            "http://127.0.0.1:49182/dashboard.html"
        );
    }

    #[test]
    fn overlay_parameter_is_added_without_replacing_existing_query_or_fragment() {
        assert_eq!(
            url_with_overlay_parameter("http://127.0.0.1:49182/dashboard.html?mode=desktop#top"),
            "http://127.0.0.1:49182/dashboard.html?mode=desktop&overlay=1#top"
        );
    }
}
