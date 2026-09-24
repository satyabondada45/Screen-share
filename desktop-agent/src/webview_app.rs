// src/webview_app.rs
// DeskStream — Unified Application UI.
//
// Loads the live Hostinger DeskStream website inside a native Windows WebView2 window.
// The full agent engine (screen capture, H.264, relay, remote control, file transfer)
// runs on background threads spawned from main.rs.
// This is ONE application: the Controller UI and Agent engine are in the same binary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

/// The bundled desktop application UI.
const DESKSTREAM_URL: &str = "deskstream://localhost/dashboard.html";

/// Run the WebView2 application on the main thread.
/// `quit` — shared flag; when set true (e.g. from tray Exit), the window closes.
pub fn run_webview(quit: Arc<AtomicBool>) {
    let event_loop: EventLoop<()> = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title("DeskStream")
        .with_inner_size(LogicalSize::new(1280_u32, 800_u32))
        .with_min_inner_size(LogicalSize::new(900_u32, 600_u32))
        .with_resizable(true)
        .with_decorations(true)
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

    // Centre the window on the primary monitor
    if let Some(monitor) = window.primary_monitor() {
        let monitor_size = monitor.size();
        let window_size = window.outer_size();
        let x = ((monitor_size.width as i32) - (window_size.width as i32)) / 2;
        let y = ((monitor_size.height as i32) - (window_size.height as i32)) / 2;
        window.set_outer_position(PhysicalPosition::new(x.max(0), y.max(0)));
    }

    // Build the WebView2 pointing directly at the live Hostinger DeskStream site.
    // All authentication, session management, and data are handled server-side on Hostinger.
    // The embedded agent engine health endpoint is available at localhost:49182.
    let _webview = WebViewBuilder::new()
        .with_custom_protocol("deskstream".into(), move |_id, _request| {
            let path = _request.uri().path();
            let (content, content_type) = match path {
                "/dashboard.html" | "/" | "" => (include_bytes!("../assets/dashboard.html").to_vec(), "text/html"),
                "/session.html" => (include_bytes!("../assets/session.html").to_vec(), "text/html"),
                "/icon.ico" => (include_bytes!("../assets/icon.ico").to_vec(), "image/x-icon"),
                "/icon.png" => (include_bytes!("../assets/icon.png").to_vec(), "image/png"),
                _ => (vec![], "text/plain"),
            };
            wry::http::Response::builder()
                .header(wry::http::header::CONTENT_TYPE, content_type)
                .body(std::borrow::Cow::Owned(content))
                .unwrap()
        })
        .with_url(DESKSTREAM_URL)
        .with_devtools(false)
        .with_initialization_script(r#"
            // Expose the embedded agent health endpoint to the Hostinger frontend.
            // The Hostinger JS code can poll this to show "This Computer: Online".
            window.__agentHealth = 'http://localhost:49182';
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
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // Signal all threads including tray to stop
                quit.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
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
