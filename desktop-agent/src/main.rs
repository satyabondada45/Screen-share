#![windows_subsystem = "windows"]

// Hardware H264 Video Pipeline (120 FPS Ultra-Low Latency)
pub mod registration {
    pub mod backend_client;
}
pub mod encoder;
pub mod identity;
pub mod network;
pub mod relay;
pub mod status;
pub mod tray;
pub mod ui;
pub mod webview_app;
pub mod local_server;
// TEMPORARY SESSION DIAGNOSTICS: remove with session_debug.rs after the A/B trace.
pub mod session_debug;


use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use enigo::{Axis, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use encoder::HardwareH264Encoder;
use screenshots::Screen;
use sha2::{Digest, Sha256};

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ============================================================
// VIDEO SETTINGS (120 FPS Ultra-Low Latency)
// ============================================================

const TARGET_FPS: u32 = 60;
const TARGET_BITRATE: u32 = 8_000_000;
const FRAME_INTERVAL_MICROS: u64 = 16667; // 60 FPS ~ 16.667 ms per frame

// ============================================================
// DPI
// ============================================================

#[cfg(windows)]
fn set_process_dpi_aware() {
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

#[cfg(not(windows))]
fn set_process_dpi_aware() {}

// ============================================================
// NATIVE MOUSE
// ============================================================

#[cfg(windows)]
fn set_native_cursor_pos(x: i32, y: i32) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
}

#[cfg(windows)]
fn send_native_mouse_click(flags: u32) {
    unsafe {
        windows_sys::Win32::UI::Input::KeyboardAndMouse::mouse_event(flags, 0, 0, 0, 0);
    }
}

#[cfg(windows)]
fn send_native_mouse_wheel(scroll_y: i16) {
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_WHEEL;
        windows_sys::Win32::UI::Input::KeyboardAndMouse::mouse_event(MOUSEEVENTF_WHEEL, 0, 0, scroll_y as i32, 0);
    }
}

#[cfg(windows)]
fn send_native_key(vk: u32, is_up: bool) {
    unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
        };
        let mut flags = if is_up { KEYEVENTF_KEYUP } else { 0 };
        // Extended keys on Windows: arrows (37-40), PageUp/Dn (33-34), End (35), Home (36), Insert (45), Delete (46), Windows key (91-92)
        if matches!(vk, 33..=46 | 91..=93 | 144 | 145) {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        keybd_event(vk as u8, 0, flags, 0);
    }
}

#[cfg(windows)]
fn attach_thread_to_input_desktop() {
    unsafe {
        let hdesktop = windows_sys::Win32::System::StationsAndDesktops::OpenInputDesktop(
            0,
            0,
            0x10000000u32, // GENERIC_ALL
        );
        if hdesktop != 0 {
            windows_sys::Win32::System::StationsAndDesktops::SetThreadDesktop(hdesktop);
            windows_sys::Win32::System::StationsAndDesktops::CloseDesktop(hdesktop);
        }
    }
}

#[cfg(not(windows))]
fn attach_thread_to_input_desktop() {}


// ============================================================
// HIDE CONSOLE & LOGGING
// ============================================================

#[cfg(windows)]
fn hide_console_window() {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        let window = GetConsoleWindow();
        if window != 0 {
            ShowWindow(window, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn hide_console_window() {}

macro_rules! agent_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            use std::io::Write;
            let _ = writeln!(std::io::stdout(), "{}", msg);
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("C:\\Users\\Public\\deskstream_agent.log") {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let _ = writeln!(file, "[{}] {}", now, msg);
            }
        }
    };
}

macro_rules! println {
    ($($arg:tt)*) => {
        agent_log!($($arg)*)
    };
}

macro_rules! eprintln {
    ($($arg:tt)*) => {
        agent_log!($($arg)*);
    };
}

// ============================================================
// CONNECTION DIALOG
// ============================================================



// ============================================================
// SHA256
// ============================================================

fn compute_sha256(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

// ============================================================
// TIME
// ============================================================

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================
// KEY MAPPING
// ============================================================

fn map_key_code(code: u32) -> Option<Key> {
    match code {
        65 => Some(Key::Unicode('a')),
        66 => Some(Key::Unicode('b')),
        67 => Some(Key::Unicode('c')),
        68 => Some(Key::Unicode('d')),
        69 => Some(Key::Unicode('e')),
        70 => Some(Key::Unicode('f')),
        71 => Some(Key::Unicode('g')),
        72 => Some(Key::Unicode('h')),
        73 => Some(Key::Unicode('i')),
        74 => Some(Key::Unicode('j')),
        75 => Some(Key::Unicode('k')),
        76 => Some(Key::Unicode('l')),
        77 => Some(Key::Unicode('m')),
        78 => Some(Key::Unicode('n')),
        79 => Some(Key::Unicode('o')),
        80 => Some(Key::Unicode('p')),
        81 => Some(Key::Unicode('q')),
        82 => Some(Key::Unicode('r')),
        83 => Some(Key::Unicode('s')),
        84 => Some(Key::Unicode('t')),
        85 => Some(Key::Unicode('u')),
        86 => Some(Key::Unicode('v')),
        87 => Some(Key::Unicode('w')),
        88 => Some(Key::Unicode('x')),
        89 => Some(Key::Unicode('y')),
        90 => Some(Key::Unicode('z')),

        48 => Some(Key::Unicode('0')),
        49 => Some(Key::Unicode('1')),
        50 => Some(Key::Unicode('2')),
        51 => Some(Key::Unicode('3')),
        52 => Some(Key::Unicode('4')),
        53 => Some(Key::Unicode('5')),
        54 => Some(Key::Unicode('6')),
        55 => Some(Key::Unicode('7')),
        56 => Some(Key::Unicode('8')),
        57 => Some(Key::Unicode('9')),

        32 => Some(Key::Space),
        13 => Some(Key::Return),
        8 => Some(Key::Backspace),
        9 => Some(Key::Tab),

        16 => Some(Key::Shift),
        17 => Some(Key::Control),
        18 => Some(Key::Alt),

        37 => Some(Key::LeftArrow),
        38 => Some(Key::UpArrow),
        39 => Some(Key::RightArrow),
        40 => Some(Key::DownArrow),

        46 => Some(Key::Delete),

        _ => None,
    }
}

// ============================================================
// FRAME DATA
// ============================================================

struct FrameData {
    width: usize,
    height: usize,
    raw_pixels: Vec<u8>,
    captured_at_ms: u64,
}

#[cfg(windows)]
fn capture_screen_gdi() -> Option<FrameData> {
    unsafe {
        let hdesktop = windows_sys::Win32::System::StationsAndDesktops::OpenInputDesktop(
            0,
            0,
            0x10000000u32, // GENERIC_ALL
        );
        if hdesktop != 0 {
            windows_sys::Win32::System::StationsAndDesktops::SetThreadDesktop(hdesktop);
            windows_sys::Win32::System::StationsAndDesktops::CloseDesktop(hdesktop);
        }

        let width = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) as usize;
        let height = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) as usize;
        if width == 0 || height == 0 {
            return None;
        }

        let hdc_screen = windows_sys::Win32::Graphics::Gdi::GetDC(0 as _);
        if hdc_screen == 0 {
            return None;
        }
        let hdc_mem = windows_sys::Win32::Graphics::Gdi::CreateCompatibleDC(hdc_screen);
        if hdc_mem == 0 {
            windows_sys::Win32::Graphics::Gdi::ReleaseDC(0 as _, hdc_screen);
            return None;
        }

        let mut bmi: windows_sys::Win32::Graphics::Gdi::BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32); // Top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = windows_sys::Win32::Graphics::Gdi::BI_RGB;

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm = windows_sys::Win32::Graphics::Gdi::CreateDIBSection(
            hdc_screen,
            &bmi,
            windows_sys::Win32::Graphics::Gdi::DIB_RGB_COLORS,
            &mut bits_ptr,
            0 as _,
            0,
        );

        if hbm == 0 || bits_ptr.is_null() {
            windows_sys::Win32::Graphics::Gdi::DeleteDC(hdc_mem);
            windows_sys::Win32::Graphics::Gdi::ReleaseDC(0 as _, hdc_screen);
            return None;
        }

        let old_obj = windows_sys::Win32::Graphics::Gdi::SelectObject(hdc_mem, hbm as _);
        let blt_res = windows_sys::Win32::Graphics::Gdi::BitBlt(
            hdc_mem,
            0,
            0,
            width as i32,
            height as i32,
            hdc_screen,
            0,
            0,
            windows_sys::Win32::Graphics::Gdi::SRCCOPY | windows_sys::Win32::Graphics::Gdi::CAPTUREBLT,
        );

        let buf_size = width * height * 4;
        let mut buffer = vec![0u8; buf_size];

        if blt_res != 0 {
            let src_slice = std::slice::from_raw_parts(bits_ptr as *const u8, buf_size);
            // Convert BGRA to RGBA
            for (dst_px, src_px) in buffer.chunks_exact_mut(4).zip(src_slice.chunks_exact(4)) {
                dst_px[0] = src_px[2]; // R
                dst_px[1] = src_px[1]; // G
                dst_px[2] = src_px[0]; // B
                dst_px[3] = 255;       // A
            }
        }

        windows_sys::Win32::Graphics::Gdi::SelectObject(hdc_mem, old_obj);
        windows_sys::Win32::Graphics::Gdi::DeleteObject(hbm as _);
        windows_sys::Win32::Graphics::Gdi::DeleteDC(hdc_mem);
        windows_sys::Win32::Graphics::Gdi::ReleaseDC(0 as _, hdc_screen);

        if blt_res == 0 {
            return None;
        }

        Some(FrameData {
            width,
            height,
            raw_pixels: buffer,
            captured_at_ms: current_time_millis(),
        })
    }
}

// ============================================================
// AUDIO
// ============================================================

fn start_audio_capture(
    write_stream: std::sync::mpsc::SyncSender<Vec<u8>>,
    is_running: Arc<AtomicBool>,
) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_input_device()?;
    let config = device.default_input_config().ok()?;
    let stream_config: cpal::StreamConfig = config.clone().into();
    let sample_rate = stream_config.sample_rate.0;
    let channels = stream_config.channels;

    println!(
        "[Audio] Microphone active: {} Hz, {} channels",
        sample_rate, channels
    );

    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &_| {
                if !is_running.load(Ordering::SeqCst) {
                    return;
                }
                if !crate::status::session_audio_enabled() {
                    return;
                }

                let byte_len = data.len() * std::mem::size_of::<f32>();
                if byte_len == 0 {
                    return;
                }

                let mut packet = Vec::with_capacity(11 + byte_len);
                packet.push(17u8);
                packet.extend_from_slice(&(byte_len as u32).to_be_bytes());
                packet.extend_from_slice(&(sample_rate as u32).to_be_bytes());
                packet.extend_from_slice(&(channels as u16).to_be_bytes());
                for sample in data {
                    packet.extend_from_slice(&sample.to_le_bytes());
                }

                let _ = write_stream.try_send(packet);
            },
            |_error| {},
            None,
        )
        .ok()?;

    stream.play().ok()?;
    Some(stream)
}
fn setup_session_audio_playback() -> (Option<cpal::Stream>, Arc<Mutex<VecDeque<f32>>>, u32) {
    let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let queue_for_callback = Arc::clone(&queue);
    let device = match cpal::default_host().default_output_device() {
        Some(device) => device,
        None => {
            eprintln!("[Audio] No output device available for session playback.");
            return (None, queue, 0);
        }
    };
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[Audio] Failed to read output configuration: {}", error);
            return (None, queue, 0);
        }
    };
    let stream_config: cpal::StreamConfig = config.clone().into();
    let output_rate = stream_config.sample_rate.0;
    let output_channels = stream_config.channels.max(1) as usize;
    let stream = match device.build_output_stream(
        &stream_config,
        move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
            if let Ok(mut samples) = queue_for_callback.lock() {
                for frame in output.chunks_mut(output_channels) {
                    let sample = samples.pop_front().unwrap_or(0.0);
                    for channel in frame {
                        *channel = sample;
                    }
                }
            } else {
                output.fill(0.0);
            }
        },
        |error| {
            eprintln!("[Audio] Output stream failed: {}", error);
        },
        None,
    ) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("[Audio] Failed to create output stream: {}", error);
            return (None, queue, 0);
        }
    };
    if let Err(error) = stream.play() {
        eprintln!("[Audio] Failed to start output stream: {}", error);
        return (None, queue, 0);
    }
    (Some(stream), queue, output_rate)
}

fn decode_audio_packet(
    payload: &[u8],
    sample_rate: u32,
    channels: u16,
    output_rate: u32,
) -> Option<Vec<f32>> {
    if payload.is_empty()
        || payload.len() % 4 != 0
        || sample_rate == 0
        || sample_rate > 192_000
        || channels == 0
        || channels > 32
        || output_rate == 0
    {
        return None;
    }
    let channel_count = channels as usize;
    let samples = payload
        .chunks_exact(4)
        .map(|bytes| {
            let sample = f32::from_le_bytes(bytes.try_into().ok()?);
            Some(if sample.is_finite() { sample } else { 0.0 })
        })
        .collect::<Option<Vec<_>>>()?;
    if samples.len() % channel_count != 0 {
        return None;
    }

    let mono = samples
        .chunks_exact(channel_count)
        .map(|frame| {
            frame.iter().copied().sum::<f32>() / channel_count as f32
        })
        .collect::<Vec<_>>();
    let output_len = ((mono.len() as u64 * output_rate as u64) / sample_rate as u64) as usize;
    if output_len == 0 {
        return Some(Vec::new());
    }
    Some(
        (0..output_len)
            .map(|index| {
                let source_index = index * sample_rate as usize / output_rate as usize;
                mono[source_index.min(mono.len() - 1)]
            })
            .collect(),
    )
}

fn read_next_file_chunk<R: Read>(reader: &mut R, buffer: &mut [u8]) -> std::io::Result<Option<usize>> {
    match reader.read(buffer)? {
        0 => Ok(None),
        bytes_read => Ok(Some(bytes_read)),
    }
}

fn validate_relative_transfer_path(path: &str) -> Result<PathBuf, String> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.contains('\0')
        || normalized.len() > 4096
    {
        return Err("Transfer path must be a safe relative path".to_string());
    }

    let mut relative = PathBuf::new();
    for part in normalized.split('/') {
        let upper = part.to_ascii_uppercase();
        let base = upper.split('.').next().unwrap_or_default();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.len() > 240
            || part.ends_with('.')
            || part.ends_with(' ')
            || part
                .chars()
                .any(|ch| ch.is_control() || "<>\"|?*".contains(ch))
            || matches!(base, "CON" | "PRN" | "AUX" | "NUL")
            || (base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && matches!(base.as_bytes()[3], b'1'..=b'9'))
        {
            return Err("Transfer path contains an unsafe component".to_string());
        }
        relative.push(part);
    }
    Ok(relative)
}

fn create_session_transfer_file(
    drop_dir: &Path,
    canonical_drop_dir: &Path,
    relative_filename: &str,
) -> std::io::Result<(File, PathBuf)> {
    let relative_path = validate_relative_transfer_path(relative_filename)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let mut destination = drop_dir.join(&relative_path);
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid destination path")
    })?;
    fs::create_dir_all(parent)?;
    let canonical_parent = parent.canonicalize()?;
    if !canonical_parent.starts_with(canonical_drop_dir) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Destination escapes the DeskStream download folder",
        ));
    }
    
    let file_stem = relative_path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let extension = relative_path.extension().map(|e| e.to_string_lossy().into_owned());
    let mut counter = 1;
    while destination.exists() {
        let new_name = if let Some(ext) = &extension {
            format!("{} ({}).{}", file_stem, counter, ext)
        } else {
            format!("{} ({})", file_stem, counter)
        };
        destination = destination.with_file_name(new_name);
        counter += 1;
    }

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    Ok((file, destination))
}


fn collect_transfer_files(root: &Path) -> std::io::Result<Vec<(PathBuf, String)>> {
    let root_folder_name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".to_string());

    fn visit(
        root: &Path,
        root_folder_name: &str,
        current: &Path,
        files: &mut Vec<(PathBuf, String)>,
    ) -> std::io::Result<()> {
        let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Folder transfer does not follow symbolic links: {}",
                        path.display()
                    ),
                ));
            }
            if metadata.is_dir() {
                visit(root, root_folder_name, &path, files)?;
            } else if metadata.is_file() {
                let relative = path.strip_prefix(root).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?;
                let relative_str = relative.to_string_lossy().replace('\\', "/");
                let relative_prefixed = format!("{}/{}", root_folder_name, relative_str);
                validate_relative_transfer_path(&relative_prefixed)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                files.push((path, relative_prefixed));
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Folder transfer encountered a non-regular file: {}",
                        path.display()
                    ),
                ));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, &root_folder_name, root, &mut files)?;
    let folder_name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Folder".to_string());
    for (_, relative) in &mut files {
        *relative = format!("{folder_name}/{relative}");
        validate_relative_transfer_path(relative)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    }
    Ok(files)
}

fn send_session_file(
    writer: &SyncSender<Vec<u8>>,
    source_path: &Path,
    relative_filename: &str,
) -> Result<(), String> {
    let filename = validate_relative_transfer_path(relative_filename)?;
    let filename = filename.to_string_lossy().replace('\\', "/");
    let name_bytes = filename.as_bytes();
    if name_bytes.len() > 4096 {
        return Err("Transfer path is too long".to_string());
    }

    let mut file =
        File::open(source_path).map_err(|error| format!("Failed to open source file: {error}"))?;
    let file_size = file
        .metadata()
        .map_err(|error| format!("Failed to read source file metadata: {error}"))?
        .len();
    let transfer_id: u64 = rand::random();
    let mut offer = vec![20u8];
    offer.extend_from_slice(&transfer_id.to_be_bytes());
    offer.extend_from_slice(&file_size.to_be_bytes());
    offer.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    offer.extend_from_slice(name_bytes);

    let response = crate::status::register_session_file_response(transfer_id);
    if writer.send(offer).is_err() {
        crate::status::clear_session_file_response(transfer_id);
        return Err(format!("Transfer {transfer_id}: offer send failed"));
    }
    println!(
        "[FILE TX START] direction=B->A transfer_id={} filename={} total_bytes={}",
        transfer_id, filename, file_size
    );

    match response.recv_timeout(Duration::from_secs(60)) {
        Ok(26) => {}
        Ok(25) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!("Transfer {transfer_id} was rejected"));
        }
        Ok(other) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: unexpected offer response {other}"
            ));
        }
        Err(error) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: no acceptance response: {error}"
            ));
        }
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    let mut chunk_idx = 0u32;
    let mut bytes_sent = 0u64;
    loop {
        let bytes_read = match read_next_file_chunk(&mut file, &mut buffer) {
            Ok(None) => break,
            Ok(Some(bytes_read)) => bytes_read,
            Err(error) => {
                let message = format!("Failed to read source file: {error}");
                let _ = writer.send(make_file_error_packet(transfer_id, &message));
                crate::status::clear_session_file_response(transfer_id);
                return Err(format!("Transfer {transfer_id}: {message}"));
            }
        };
        hasher.update(&buffer[..bytes_read]);
        let mut packet = vec![21u8];
        packet.extend_from_slice(&transfer_id.to_be_bytes());
        packet.extend_from_slice(&chunk_idx.to_be_bytes());
        packet.extend_from_slice(&(bytes_read as u32).to_be_bytes());
        packet.extend_from_slice(&buffer[..bytes_read]);
        if writer.send(packet).is_err() {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!("Transfer {transfer_id}: chunk send failed"));
        }
        bytes_sent += bytes_read as u64;
        chunk_idx += 1;
        thread::sleep(Duration::from_millis(10));
    }

    if bytes_sent != file_size {
        let message = format!(
            "Source file changed while sending (expected {file_size} bytes, read {bytes_sent})"
        );
        let _ = writer.send(make_file_error_packet(transfer_id, &message));
        crate::status::clear_session_file_response(transfer_id);
        return Err(format!("Transfer {transfer_id}: {message}"));
    }

    let hash = hasher.finalize();
    let mut end_packet = vec![22u8];
    end_packet.extend_from_slice(&transfer_id.to_be_bytes());
    end_packet.extend_from_slice(&file_size.to_be_bytes());
    end_packet.extend_from_slice(&hash);
    if writer.send(end_packet).is_err() {
        crate::status::clear_session_file_response(transfer_id);
        return Err(format!(
            "Transfer {transfer_id}: completion packet send failed"
        ));
    }

    let hash_hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    match response.recv_timeout(Duration::from_secs(60)) {
        Ok(27) => println!(
            "[FILE TX END] direction=B->A transfer_id={} final_sha256={} success=true",
            transfer_id, hash_hex
        ),
        Ok(other) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: unexpected completion response {other}"
            ));
        }
        Err(error) => {
            crate::status::clear_session_file_response(transfer_id);
            return Err(format!(
                "Transfer {transfer_id}: no completion acknowledgement: {error}"
            ));
        }
    }
    crate::status::clear_session_file_response(transfer_id);
    Ok(())
}

fn make_file_error_packet(transfer_id: u64, message: &str) -> Vec<u8> {
    let message_bytes = message.as_bytes();
    let message_bytes = &message_bytes[..message_bytes.len().min(u16::MAX as usize)];
    let mut packet = Vec::with_capacity(13 + message_bytes.len());
    packet.push(24);
    packet.extend_from_slice(&transfer_id.to_be_bytes());
    packet.extend_from_slice(&[0, 0]);
    packet.extend_from_slice(&(message_bytes.len() as u16).to_be_bytes());
    packet.extend_from_slice(message_bytes);
    packet
}

// ============================================================
// AGENT LOOP
// ============================================================

fn run_agent_loop(relay_addr: String, config: identity::device_id::AgentConfig) {
    crate::session_debug::log(
        &config.system_id,
        &format!("B_START system_id={}", config.system_id),
    );
    crate::session_debug::log(
        &config.system_id,
        &format!("B_RELAY_CONFIG endpoint={}", relay_addr),
    );
    let host_ip = relay_addr.split(':').next().unwrap_or("127.0.0.1");
    let backend_url = "https://friendssoftwaresolutions.in/DeskStream/api".to_string(); 
    // Use persistent device UUID as the machine identifier
    // Use the config system_id (deterministic from UUID) as the initial system_id hint
    let mut backend = registration::backend_client::BackendClient::new(
        &backend_url,
        &config.device_uuid,
        &config.system_id,
    );

    agent_log!("[DISCOVERY] Registering with backend: {}", backend_url);
    agent_log!("[AGENT] Connecting to relay");
    
    let system_id = if let Some(assigned_id) = backend.register() {
        agent_log!("[DISCOVERY] Registered successfully with Web Dashboard / Device Registry!");
        agent_log!("[DISCOVERY] Backend assigned System ID: {}", assigned_id);
        agent_log!("[AGENT] Local device ID = {}", assigned_id);
        assigned_id
    } else {
        agent_log!("[DISCOVERY] Running in standalone relay mode.");
        config.system_id.clone()
    };
    crate::session_debug::log(
        &system_id,
        &format!("B_SYSTEM_ID system_id={}", system_id),
    );
    crate::session_debug::log(
        &system_id,
        &format!("ROLE role=CONTROLLED video_direction=B->A system_id={}", system_id),
    );

    let id_str = {
        let clean: String = system_id.chars().filter(|c| c.is_ascii_digit()).collect();
        if clean.len() == 9 {
            format!("{} {} {}", &clean[0..3], &clean[3..6], &clean[6..9])
        } else {
            clean.clone()
        }
    };

    agent_log!("[AGENT] Registering device = {}", id_str);
    agent_log!("[IDENTITY] Starting Host Desktop Agent...");
    agent_log!("[IDENTITY] Device ID: {}", system_id);
    agent_log!("[IDENTITY] Device UUID:  {}", config.device_uuid);
    agent_log!("[IDENTITY] System ID:    {}", system_id);
    agent_log!("[IDENTITY] Display ID:   {}", id_str);

    let global_status = Arc::new(Mutex::new(crate::status::AgentStatus::new(&system_id, "Windows Device")));
    backend.start_heartbeat_thread(global_status.clone());

    let mut backoff_secs = 1;
    let mut intentional_reconnect = false;

    loop {
        if intentional_reconnect {
            intentional_reconnect = false;
        } else {
            // sleep happens at the end of the loop
        }

        agent_log!("[RELAY] Connecting to relay {}...", relay_addr);
        crate::session_debug::log(&system_id, "B_TCP_CONNECT_START");

        let mut stream = match TcpStream::connect(&relay_addr) {
            Ok(s) => {
                crate::session_debug::log(&system_id, "B_TCP_CONNECT_SUCCESS");
                let _ = s.set_nodelay(true);
                let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                s
            }
            Err(e) => {
                crate::session_debug::log(
                    &system_id,
                    &format!("B_TCP_CONNECT_FAILED error={}", e),
                );
                crate::session_debug::log(&system_id, "B_DISCONNECT");
                crate::session_debug::log(
                    &system_id,
                    "STATE_CHANGE old=CONNECTING new=DISCONNECTED",
                );
                eprintln!("[RELAY] Connection failed: {:?}", e);
                println!("[SESSION STATE] OFFLINE");
                println!("[RELAY][DISCONNECT] reason=Connection refused or network unreachable");
                println!("[RELAY][DISCONNECT] system_id={}", system_id);
                println!("[RELAY][DISCONNECT] socket_error={:?}", e);
                println!("[RELAY][DISCONNECT] remote_closed=false");
                thread::sleep(Duration::from_secs(backoff_secs));
                backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
                continue;
            }
        };

        println!("[RELAY] TCP connection established");
        println!("[RELAY] Sending registration for System ID: {}", system_id);
        crate::session_debug::log(
            &system_id,
            &format!("B_REGISTER_START system_id={}", system_id),
        );

        // Registration (Type 1 + System ID as relay session key)
        let mut register_pkt = Vec::with_capacity(1 + system_id.len());
        register_pkt.push(1u8);
        register_pkt.extend_from_slice(system_id.as_bytes());

        if stream.write_all(&register_pkt).is_err() {
            crate::session_debug::log(&system_id, "B_REGISTER_SEND_FAILED");
            eprintln!("[RELAY] Failed to send registration packet");
            println!("[SESSION STATE] OFFLINE");
            thread::sleep(Duration::from_secs(backoff_secs));
            backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
            continue;
        }
        crate::session_debug::log(
            &system_id,
            &format!("B_REGISTER_SENT system_id={}", system_id),
        );

        let mut ack = [0u8; 1];
        if stream.read_exact(&mut ack).is_err() {
            crate::session_debug::log(&system_id, "B_REGISTER_ACK_READ_FAILED");
            eprintln!("[RELAY] Registration failed or unacknowledged by relay");
            println!("[SESSION STATE] OFFLINE");
            thread::sleep(Duration::from_secs(backoff_secs));
            backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
            continue;
        }
        crate::session_debug::log(
            &system_id,
            &format!("B_REGISTER_ACK ack={}", ack[0]),
        );
        if ack[0] != 1 {
            eprintln!("[RELAY] Registration failed or unacknowledged by relay");
            println!("[SESSION STATE] OFFLINE");
            thread::sleep(Duration::from_secs(backoff_secs));
            backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
            continue;
        }

        println!("[RELAY] Registration ACK received");
        crate::session_debug::log(
            &system_id,
            "STATE_CHANGE old=CONNECTING new=CONNECTED",
        );
        agent_log!("[AGENT] Registration acknowledged");
        agent_log!("[AGENT] Registration sent");
        agent_log!("[AGENT] ACTUAL DEVICE ID = {}", id_str);

        println!("[SESSION STATE] ONLINE");
        backoff_secs = 1;

        let idle_write_stream = match stream.try_clone() {
            Ok(s) => Arc::new(Mutex::new(s)),
            Err(e) => {
                eprintln!("[Agent] Failed to clone stream for writer: {:?}", e);
                continue;
            }
        };

        let is_running_conn = Arc::new(AtomicBool::new(true));
        let is_in_session = Arc::new(AtomicBool::new(false));

        // Heartbeat thread: Sends Type 14 to relay every 5s during idle state
        let hb_write = Arc::clone(&idle_write_stream);
        let hb_running = Arc::clone(&is_running_conn);
        let hb_in_session = Arc::clone(&is_in_session);

        let heartbeat_handle = thread::spawn(move || {
            while hb_running.load(Ordering::SeqCst) {
                if !hb_in_session.load(Ordering::SeqCst) {
                    let mut ping_pkt = Vec::with_capacity(9);
                    ping_pkt.push(14u8);
                    ping_pkt.extend_from_slice(&current_time_millis().to_be_bytes());

                    if let Ok(mut writer) = hb_write.lock() {
                        let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
                        let _ = writer.write_all(&ping_pkt);
                    }
                }
                thread::sleep(Duration::from_secs(5));
            }
        });

        'viewer_loop: loop {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut type_buf = [0u8; 1];

            match stream.read_exact(&mut type_buf) {
                Ok(_) => {
                    match type_buf[0] {
                        // Type 14: Heartbeat ACK from relay
                        14 => {
                            let mut time_buf = [0u8; 8];
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            if stream.read_exact(&mut time_buf).is_ok() {
                                println!("[HEARTBEAT] -> {}", system_id);
                                println!("[HEARTBEAT] <- ACK");
                            }
                        }

                        // Type 3: Incoming session request / Authentication
                        3 => {
                            crate::session_debug::log(
                                &system_id,
                                &format!(
                                    "B_SESSION_REQUEST_RECEIVED type=3 system_id={}",
                                    system_id
                                ),
                            );
                            let mut auth_hash = [0u8; 32];
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                            if stream.read_exact(&mut auth_hash).is_err() {
                                eprintln!("[Agent] Failed to read auth hash from relay.");
                                break 'viewer_loop;
                            }

                            println!("[Host] Received connection request for Target ID: {}", id_str);
                            println!("[SESSION] ACCEPT received");
                            println!("[SESSION] Starting remote session");
                            println!("[STREAM STATE] STARTING");
                            crate::session_debug::log(
                                &system_id,
                                "B_STREAM_STATE state=STARTING",
                            );
                            println!("[STREAM] Starting screen capture");
                            println!("[STREAM] Starting H.264 encoder");

                            is_in_session.store(true, Ordering::SeqCst);

                            // The approval byte is a one-byte ACK in the type-3 auth flow and must be
                            // written on the active control socket, not via the idle heartbeat writer.
                            // This keeps the host relay state machine aligned with the relay's idle-loop
                            // heartbeat handling and avoids a stray '1' being treated as an unexpected idle byte.
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                            if stream.write_all(&[1u8]).is_err() {
                                eprintln!("[Agent] Failed to send approval ACK to relay.");
                                break 'viewer_loop;
                            }
                            crate::session_debug::log(
                                &system_id,
                                "B_SESSION_APPROVAL_SENT ack=1",
                            );

                            println!("[Host] Approval response sent successfully (APPROVED)");
                            backend.log_session_start(&system_id);
                            println!("[Agent] Session APPROVED! Starting live video...");
                            // ====================================================
                            // CONNECTION STATE & STREAMING PIPELINE
                            // ====================================================
                            let session_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(e) => {
                                    eprintln!("[Agent] Failed to clone stream for session thread: {}", e);
                                    break 'viewer_loop;
                                }
                            };
                            let backend_clone = backend.clone();
                            let system_id_clone = system_id.clone();
                            let is_in_session_clone = Arc::clone(&is_in_session);
                            
                            thread::spawn(move || {
                                let mut stream = session_stream;
                                let system_id = system_id_clone;
                                let backend = backend_clone;
                                let is_in_session = is_in_session_clone;

                                let is_connected = Arc::new(AtomicBool::new(true));
                                let is_conn_read = Arc::clone(&is_connected);
                                let is_conn_write = Arc::clone(&is_connected);
                                let is_conn_capture = Arc::clone(&is_connected);
                                let is_conn_clip = Arc::clone(&is_connected);
                                let is_conn_audio = Arc::clone(&is_connected);
                                let is_conn_ping = Arc::clone(&is_connected);
                                
                                if let Err(e) = stream.set_nodelay(true) {
                                    eprintln!("[Agent] Warning: Could not set TCP_NODELAY: {}", e);
                                }

                                // TCP INPUT
                                let mut read_stream = match stream.try_clone() {
                                    Ok(s) => s,
                                    Err(_) => {
                                        is_connected.store(false, Ordering::Release);
                                        return;
                                    }
                            };
                            let _ = read_stream.set_read_timeout(None);

                            // Bounded Send Buffer: 64KB ensures the OS doesn't hide seconds of latency
                            #[cfg(windows)]
                            {
                                use std::os::windows::io::AsRawSocket;
                                let sock = stream.as_raw_socket();
                                let sndbuf: i32 = 65536;
                                unsafe {
                                    windows_sys::Win32::Networking::WinSock::setsockopt(
                                        sock as usize,
                                        windows_sys::Win32::Networking::WinSock::SOL_SOCKET,
                                        windows_sys::Win32::Networking::WinSock::SO_SNDBUF,
                                        &sndbuf as *const i32 as *const _,
                                        4,
                                    );
                                }
                            }

                            let video_slot = Arc::new(Mutex::new(std::collections::VecDeque::<Vec<u8>>::with_capacity(3)));
                            let video_slot_writer = Arc::clone(&video_slot);
                            let video_slot_capture = Arc::clone(&video_slot);
                            let video_recovery_needed = Arc::new(AtomicBool::new(false));
                            let video_recovery_capture = Arc::clone(&video_recovery_needed);
                            
                            let (out_tx, out_rx) = sync_channel::<Vec<u8>>(512);
                            let write_stream = out_tx.clone();
                            let mut write_tcp = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    return;
                                }
                            };
                            let _ = write_tcp.set_write_timeout(None);

                            let write_connected = Arc::clone(&is_connected);

                            println!("[STREAM STATE] ACTIVE");
                            println!("[VIDEO STATE] ACTIVE");
                            crate::session_debug::log(
                                &system_id,
                                "B_STREAM_STATE state=ACTIVE",
                            );
                            let diagnostic_system_id = system_id.clone();

                            let writer_handle = thread::spawn(move || {
                                let mut video_trace_count = 0u64;
                                let mut diagnostic_video_count = 0u64;
                                let mut write_packet = |packet: Vec<u8>| -> bool {
                                    if packet.first() == Some(&13u8) || packet.first() == Some(&15u8) {
                                        video_trace_count += 1;
                                        if video_trace_count == 1 || video_trace_count % 60 == 0 {
                                            let ts = u64::from_be_bytes(packet[13..21].try_into().unwrap_or([0; 8]));
                                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                                            if ts > 0 && now > ts {
                                                println!("[VIDEO TRACE] frames={} send_timestamp={} ageMs={}", video_trace_count, now, now - ts);
                                            }
                                        }
                                    }
                                    match write_tcp.write_all(&packet) {
                                        Ok(_) => {
                                            if packet.first() == Some(&13u8) {
                                                diagnostic_video_count += 1;
                                                if diagnostic_video_count == 1 {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        "B_FIRST_TYPE13_SENT",
                                                    );
                                                }
                                                if diagnostic_video_count == 1
                                                    || diagnostic_video_count % 60 == 0
                                                {
                                                    crate::session_debug::log(
                                                        &diagnostic_system_id,
                                                        &format!(
                                                            "B_TYPE13_SENT count={}",
                                                            diagnostic_video_count
                                                        ),
                                                    );
                                                }
                                            }
                                            true
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[Writer] TCP packet write failed: type={} bytes={} error={:?}; ending session to preserve stream framing",
                                                packet.first().copied().unwrap_or(255),
                                                packet.len(),
                                                e
                                            );
                                            println!("[VIDEO STATE] WRITE_FAILED");
                                            crate::session_debug::log(
                                                &diagnostic_system_id,
                                                &format!("B_VIDEO_WRITE_FAILED error={}", e),
                                            );
                                            write_connected.store(false, Ordering::Release);
                                            false
                                        }
                                    }
                                };

                                while write_connected.load(Ordering::Acquire) {
                                    let mut sent_control = false;
                                    let mut error = false;

                                    // Drain all available control/audio packets first (high priority)
                                    while let Ok(packet) = out_rx.try_recv() {
                                        sent_control = true;
                                        if !write_packet(packet) {
                                            error = true;
                                            break;
                                        }
                                    }
                                    if error { break; }

                                    // Then send ONE video packet if available
                                    let video_packet = video_slot_writer
                                        .lock()
                                        .ok()
                                        .and_then(|mut q| q.pop_front());
                                    
                                    if let Some(packet) = video_packet {
                                        if !write_packet(packet) {
                                            break;
                                        }
                                    } else if !sent_control {
                                        // Wait a bit to avoid busy loop if both queues are empty
                                        match out_rx.recv_timeout(Duration::from_millis(5)) {
                                            Ok(packet) => {
                                                if !write_packet(packet) {
                                                    break;
                                                }
                                            }
                                            Err(_) => {}
                                        }
                                    }
                                }
                            });

                            let write_stream_clip = write_stream.clone();
                            let write_stream_audio = write_stream.clone();
                            let write_stream_ping = write_stream.clone();
                            let write_stream_file = write_stream.clone();

                            let is_conn_file = Arc::clone(&is_connected);
                            thread::spawn(move || {
                                let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\temp".to_string());
                                let trigger_file = std::path::Path::new(&local_app_data).join("DeskStream").join("send_file.txt");
                                while is_conn_file.load(Ordering::SeqCst) {
                                    if trigger_file.exists() {
                                        if let Ok(file_path_str) = std::fs::read_to_string(&trigger_file) {
                                            let _ = std::fs::remove_file(&trigger_file);
                                            for selected_path in file_path_str.lines().map(str::trim).filter(|path| !path.is_empty()) {
                                                let selected_path = PathBuf::from(selected_path);
                                                let transfers = if selected_path.is_dir() {
                                                    collect_transfer_files(&selected_path)
                                                        .map_err(|error| error.to_string())
                                                } else if selected_path.is_file() {
                                                    let name = selected_path
                                                        .file_name()
                                                        .map(|name| name.to_string_lossy().into_owned())
                                                        .ok_or_else(|| "Selected path has no file name".to_string());
                                                    name.map(|name| vec![(selected_path.clone(), name)])
                                                } else {
                                                    Err("Selected path is not a regular file or folder".to_string())
                                                };
                                                match transfers {
                                                    Ok(transfers) if transfers.is_empty() => {
                                                        eprintln!("[FILE TX ERROR] Selected folder contains no regular files");
                                                    }
                                                    Ok(transfers) => {
                                                        for (source_path, relative_name) in transfers {
                                                            if let Err(error) = send_session_file(
                                                                &write_stream_file,
                                                                &source_path,
                                                                &relative_name,
                                                            ) {
                                                                eprintln!("[FILE TX ERROR] {error}");
                                                            }
                                                        }
                                                    }
                                                    Err(error) => {
                                                        eprintln!("[FILE TX ERROR] Could not enumerate selected transfer source: {error}");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    std::thread::sleep(std::time::Duration::from_millis(500));
                                }
                            });

                            let current_rtt_ms = Arc::new(AtomicU64::new(10));
                            let current_rtt_in = Arc::clone(&current_rtt_ms);

                            let active_screen_idx = Arc::new(AtomicUsize::new(0));
                            let active_idx_input = Arc::clone(&active_screen_idx);
                            let active_idx_capture = Arc::clone(&active_screen_idx);

                            let shared_frame = Arc::new((Mutex::new(None::<FrameData>), Condvar::new()));
                            let shared_frame_cap = Arc::clone(&shared_frame);

                            let last_clipboard_text = Arc::new(Mutex::new(String::new()));
                            let last_clip_recv = Arc::clone(&last_clipboard_text);
                            let last_clip_send = Arc::clone(&last_clipboard_text);
                            if let Err(error) = crate::status::clear_session_peer_system_id() {
                                eprintln!("[SESSION UI] Failed to clear stale peer identity: {}", error);
                            }
                            let session_ui_ready =
                                match crate::status::set_session_writer(Some(write_stream.clone())) {
                                    Ok(()) => true,
                                    Err(error) => {
                                        eprintln!("[SESSION UI] Failed to initialize session bridge: {}", error);
                                        false
                                    }
                                };
                            // Publish CONNECTED only after the real session writer is ready for
                            // chat, microphone, reverse-control, and disconnect commands.
                            if session_ui_ready {
                                crate::status::set_session_active(true);
                            }
                            let write_stream_input = write_stream.clone();

                            // INPUT THREAD
                            let input_handle = thread::spawn(move || {
                                let (_audio_output_stream, audio_playback_queue, audio_output_rate) =
                                    setup_session_audio_playback();
                                println!("[INPUT THREAD] Started native input processing loop");
                                let mut clip = Clipboard::new().ok();
                                let mut current_file: Option<File> = None;
                                let mut current_file_path: Option<PathBuf> = None;
                                let mut current_transfer_id: Option<u64> = None;
                                let mut current_file_chunk_index = 0u32;
                                let mut current_filename = String::new();
                                let mut total_file_size: u64 = 0;
                                let mut received_bytes: u64 = 0;
                                let mut file_hasher = Sha256::new();
                                let mut chat_packet_diagnostic_logged = false;
                                let user_profile = env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
                                let drop_dir = PathBuf::from(user_profile).join("Downloads").join("DeskStream");
                                let canonical_drop_dir = fs::create_dir_all(&drop_dir)
                                    .and_then(|_| drop_dir.canonicalize());
                                if let Err(error) = &canonical_drop_dir {
                                    eprintln!("[FILE RX] Failed to prepare download folder: {}", error);
                                }
                                let send_file_error = |transfer_id: u64, message: &str| {
                                    let message = message.as_bytes();
                                    let mut packet = vec![24u8];
                                    packet.extend_from_slice(&transfer_id.to_be_bytes());
                                    packet.extend_from_slice(&[0, 0]);
                                    packet.extend_from_slice(&(message.len() as u16).to_be_bytes());
                                    packet.extend_from_slice(message);
                                    if let Err(error) = write_stream_input.send(packet) {
                                        eprintln!("[FILE RX ERROR] Failed to send transfer error: {}", error);
                                    }
                                };

                                while is_conn_read.load(Ordering::SeqCst) {
                                    let mut pkt_type_buf = [0u8; 1];
                                    if read_stream.read_exact(&mut pkt_type_buf).is_err() {
                                        is_conn_read.store(false, Ordering::SeqCst);
                                        break;
                                    }

                                    match pkt_type_buf[0] {
                                        0..=10 => {
                                            let mut data = [0u8; 8];
                                            if read_stream.read_exact(&mut data).is_err() {
                                                is_conn_read.store(false, Ordering::SeqCst);
                                                break;
                                            }
                                            attach_thread_to_input_desktop();
                                            let event_type = pkt_type_buf[0];
                                            
                                            if event_type < 10 && !crate::status::session_perm_control_input() {
                                                continue;
                                            }

                                            let type_name = match event_type {
                                                0 => "MOUSE_MOVE",
                                                1 | 3 | 7 => "MOUSE_DOWN",
                                                2 | 4 | 8 => "MOUSE_UP",
                                                5 => "KEY_DOWN",
                                                6 => "KEY_UP",
                                                9 => "MOUSE_WHEEL",
                                                _ => "CONTROL",
                                            };
                                            println!("[CONTROL DEBUG][HOST RX]\ntype={}\nlength=9", type_name);
                                            println!("[AGENT CONTROL RX]\ntype={}\nbytes=9", event_type);
                                            println!("[AGENT CONTROL RX] {}", type_name);

                                            if event_type == 10 {
                                                active_idx_input.store(data[0] as usize, Ordering::SeqCst);
                                                continue;
                                            }
                                            if event_type == 9 {
                                                let scroll_y = i16::from_be_bytes(data[2..4].try_into().unwrap());
                                                #[cfg(windows)]
                                                {
                                                    send_native_mouse_wheel(scroll_y);
                                                    println!("[AGENT MOUSE] injected wheel scroll={}", scroll_y);
                                                    println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_WHEEL\nresult=success");
                                                }
                                                continue;
                                            }
                                            if event_type == 5 || event_type == 6 {
                                                let key_code = u32::from_be_bytes(data[0..4].try_into().unwrap());
                                                #[cfg(windows)]
                                                {
                                                    send_native_key(key_code, event_type == 6);
                                                    println!("[INPUT INJECT] keyboard event={} vk={}", if event_type == 5 { "keydown" } else { "keyup" }, key_code);
                                                    println!("[AGENT KEYBOARD]\nevent={}\nkey={}\ncode={}", if event_type == 5 { "keydown" } else { "keyup" }, key_code, key_code);
                                                    println!("[AGENT KEYBOARD] injected");
                                                    println!("[CONTROL DEBUG][INPUT INJECTION]\ntype={}\nresult=success", if event_type == 5 { "KEY_DOWN" } else { "KEY_UP" });
                                                }
                                                continue;
                                            }

                                            let norm_x = u16::from_be_bytes(data[0..2].try_into().unwrap());
                                            let norm_y = u16::from_be_bytes(data[2..4].try_into().unwrap());

                                            #[cfg(windows)]
                                            let (sw, sh) = unsafe {
                                                (
                                                    windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) as f32,
                                                    windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) as f32,
                                                )
                                            };
                                            #[cfg(not(windows))]
                                            let (sw, sh) = (1920.0f32, 1080.0f32);

                                            let target_x = ((norm_x as f32 / 65535.0) * (sw - 1.0)).round() as i32;
                                            let target_y = ((norm_y as f32 / 65535.0) * (sh - 1.0)).round() as i32;

                                            #[cfg(windows)]
                                            {
                                                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                                                    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
                                                    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
                                                };

                                                match event_type {
                                                    0 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        println!("[INPUT INJECT] mousemove x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=none", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_MOVE\nresult=success");
                                                    }
                                                    1 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_LEFTDOWN);
                                                        println!("[INPUT INJECT] mousedown button=LEFT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=left_down", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_DOWN\nresult=success");
                                                    }
                                                    2 => {
                                                        send_native_mouse_click(MOUSEEVENTF_LEFTUP);
                                                        println!("[INPUT INJECT] mouseup button=LEFT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=left_up", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_UP\nresult=success");
                                                    }
                                                    3 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_RIGHTDOWN);
                                                        println!("[INPUT INJECT] mousedown button=RIGHT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=right_down", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_DOWN\nresult=success");
                                                    }
                                                    4 => {
                                                        send_native_mouse_click(MOUSEEVENTF_RIGHTUP);
                                                        println!("[INPUT INJECT] mouseup button=RIGHT x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=right_up", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_UP\nresult=success");
                                                    }
                                                    7 => {
                                                        set_native_cursor_pos(target_x, target_y);
                                                        send_native_mouse_click(MOUSEEVENTF_MIDDLEDOWN);
                                                        println!("[INPUT INJECT] mousedown button=MIDDLE x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=middle_down", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_DOWN\nresult=success");
                                                    }
                                                    8 => {
                                                        send_native_mouse_click(MOUSEEVENTF_MIDDLEUP);
                                                        println!("[INPUT INJECT] mouseup button=MIDDLE x={} y={}", target_x, target_y);
                                                        println!("[AGENT MOUSE]\nx={}\ny={}\nbutton=middle_up", target_x, target_y);
                                                        println!("[AGENT MOUSE] injected");
                                                        println!("[CONTROL DEBUG][INPUT INJECTION]\ntype=MOUSE_UP\nresult=success");
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }

                                        12 => {
                                            let mut len_buf = [0u8; 4];
                                            if read_stream.read_exact(&mut len_buf).is_err() {
                                                break;
                                            }
                                            let len = u32::from_be_bytes(len_buf) as usize;
                                            if len > 10 * 1024 * 1024 {
                                                break;
                                            }
                                            let mut text_buf = vec![0u8; len];
                                            if read_stream.read_exact(&mut text_buf).is_err() {
                                                break;
                                            }
                                            if let Ok(text) = String::from_utf8(text_buf) {
                                                if let Ok(mut guard) = last_clip_recv.lock() {
                                                    *guard = text.clone();
                                                }
                                                if let Some(ref mut c) = clip {
                                                    let _ = c.set_text(text);
                                                }
                                            }
                                        }

                                        14 => {
                                            let mut time_buf = [0u8; 8];
                                            if read_stream.read_exact(&mut time_buf).is_err() {
                                                break;
                                            }
                                            let sent_time = u64::from_be_bytes(time_buf);
                                            let now = current_time_millis();
                                            if now >= sent_time {
                                                current_rtt_in.store(now - sent_time, Ordering::SeqCst);
                                            }
                                        }

                                        16 => {
                                            let mut meta = [0u8; 3];
                                            if read_stream.read_exact(&mut meta).is_err() {
                                                break;
                                            }
                                            let len = u16::from_be_bytes([meta[1], meta[2]]) as usize;
                                            let mut msg_bytes = vec![0u8; len];
                                            if read_stream.read_exact(&mut msg_bytes).is_err() {
                                                break;
                                            }
                                            if !chat_packet_diagnostic_logged {
                                                let prefix_hex = msg_bytes.iter().take(64)
                                                    .map(|byte| format!("{:02x}", byte))
                                                    .collect::<Vec<_>>()
                                                    .join(" ");
                                                println!(
                                                    "[CHAT RX TRACE] type=16 declared_payload_len={} consumed_bytes={} payload_start=4 payload_end={} payload_bytes={} payload_prefix_hex=\"{}\"",
                                                    len, 4 + len, 4 + len, msg_bytes.len(), prefix_hex
                                                );
                                                chat_packet_diagnostic_logged = true;
                                            }
                                            match String::from_utf8(msg_bytes) {
                                                Ok(text) => {
                                                    println!("\n[Chat from Remote Viewer]: {}", text);
                                                    if let Err(error) = crate::status::push_session_chat(text, false) {
                                                        eprintln!("[CHAT UI] Failed to retain received message: {}", error);
                                                    }
                                                }
                                                Err(error) => {
                                                    eprintln!(
                                                        "[CHAT RX UTF8 ERROR] type=16 payload_len={} valid_up_to={} error_len={:?}",
                                                        len,
                                                        error.utf8_error().valid_up_to(),
                                                        error.utf8_error().error_len()
                                                    );
                                                }
                                            }
                                        }

                                        18 => {
                                            let mut state = [0u8; 1];
                                            if read_stream.read_exact(&mut state).is_err() {
                                                break;
                                            }
                                            if state[0] > 1 {
                                                eprintln!("[CHAT TYPING RX] Invalid state byte {}", state[0]);
                                                continue;
                                            }
                                            crate::status::set_session_remote_typing(state[0] == 1);
                                        }

                                        30 => {
                                            crate::status::set_session_reverse_request_pending();
                                            println!("[REVERSE] Received TYPE 30 request during active session.");
                                        }

                                        31 => {
                                            let mut decision = [0u8; 1];
                                            if read_stream.read_exact(&mut decision).is_err() {
                                                is_conn_read.store(false, Ordering::SeqCst);
                                                break;
                                            }
                                            if decision[0] > 1 {
                                                eprintln!("[REVERSE] Invalid TYPE 31 decision={}", decision[0]);
                                                continue;
                                            }
                                            crate::status::set_session_reverse_decision(decision[0] == 1);
                                            println!("[REVERSE] Received TYPE 31 decision={}; active roles are unchanged.", decision[0]);
                                        }

                                        32 => {
                                            let mut peer_id = [0u8; 9];
                                            if read_stream.read_exact(&mut peer_id).is_err() {
                                                is_conn_read.store(false, Ordering::SeqCst);
                                                break;
                                            }
                                            match String::from_utf8(peer_id.to_vec()) {
                                                Ok(peer_id) => {
                                                    if let Err(error) = crate::status::set_session_peer_system_id(peer_id.clone()) {
                                                        eprintln!("[REVERSE] Ignoring invalid peer identity: {}", error);
                                                    } else {
                                                        println!("[REVERSE] Registered active session peer system_id={}", peer_id);
                                                    }
                                                }
                                                Err(_) => {
                                                    eprintln!("[REVERSE] Ignoring non-UTF8 peer identity.");
                                                }
                                            }
                                        }

                                        17 => {
                                            let mut header = [0u8; 10];
                                            if read_stream.read_exact(&mut header).is_err() {
                                                is_conn_read.store(false, Ordering::SeqCst);
                                                break;
                                            }
                                            let payload_len = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
                                            let sample_rate = u32::from_be_bytes(header[4..8].try_into().unwrap());
                                            let channels = u16::from_be_bytes(header[8..10].try_into().unwrap());
                                            if payload_len == 0 || payload_len > 10 * 1024 * 1024 {
                                                eprintln!("[AUDIO RX] Invalid TYPE 17 payload length={}", payload_len);
                                                is_conn_read.store(false, Ordering::SeqCst);
                                                break;
                                            }
                                            let mut payload = vec![0u8; payload_len];
                                            if read_stream.read_exact(&mut payload).is_err() {
                                                is_conn_read.store(false, Ordering::SeqCst);
                                                break;
                                            }
                                            if let Some(samples) = decode_audio_packet(
                                                &payload,
                                                sample_rate,
                                                channels,
                                                audio_output_rate,
                                            ) {
                                                if let Ok(mut queue) = audio_playback_queue.lock() {
                                                    let max_samples = (audio_output_rate as f32 * 0.3) as usize; // 300ms max latency
                                                    if queue.len().saturating_add(samples.len()) > max_samples {
                                                        queue.clear();
                                                    }
                                                    queue.extend(samples);
                                                }
                                            } else {
                                                eprintln!(
                                                    "[AUDIO RX] Invalid TYPE 17 frame rate={} channels={} bytes={}",
                                                    sample_rate,
                                                    channels,
                                                    payload_len
                                                );
                                            }
                                        }

                                        20 => {
                                            let mut hdr = [0u8; 18];
                                            if read_stream.read_exact(&mut hdr).is_err() { break; }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            total_file_size = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
                                            let name_len = u16::from_be_bytes([hdr[16], hdr[17]]) as usize;
                                            if name_len > 4096 { break; }
                                            let mut name_buf = vec![0u8; name_len];
                                            
                                            if !crate::status::session_perm_file_transfer() {
                                                if read_stream.read_exact(&mut name_buf).is_err() { break; }
                                                eprintln!("[FILE RX OFFER] Rejected because File Transfer permission is OFF");
                                                let mut reject = vec![25u8];
                                                reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                let _ = write_stream_input.send(reject);
                                                continue;
                                            }
                                            
                                            if read_stream.read_exact(&mut name_buf).is_err() { break; }
                                            let received_name = match String::from_utf8(name_buf) {
                                                Ok(name) => name,
                                                Err(error) => {
                                                    eprintln!("[FILE RX OFFER] Rejected invalid UTF-8 file path: {}", error);
                                                    let mut reject = vec![25u8];
                                                    reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                    let _ = write_stream_input.send(reject);
                                                    continue;
                                                }
                                            };
                                            current_filename = match validate_relative_transfer_path(&received_name) {
                                                Ok(path) => path.to_string_lossy().replace('\\', "/"),
                                                Err(error) => {
                                                    eprintln!("[FILE RX OFFER] Rejected unsafe file path: {}", error);
                                                    let mut reject = vec![25u8];
                                                    reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                    let _ = write_stream_input.send(reject);
                                                    continue;
                                                }
                                            };
                                            if canonical_drop_dir.is_err() {
                                                let mut reject = vec![25u8];
                                                reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                let _ = write_stream_input.send(reject);
                                                continue;
                                            }
                                            current_file = None;
                                            current_file_path = None;
                                            current_transfer_id = Some(transfer_id);
                                            current_file_chunk_index = 0;
                                            received_bytes = 0;
                                            file_hasher = Sha256::new();
                                            if let Err(error) = crate::status::push_session_file_offer(crate::status::SessionFileOffer {
                                                transfer_id,
                                                filename: current_filename.clone(),
                                                size: total_file_size,
                                            }) {
                                                eprintln!("[FILE RX OFFER] Failed to publish offer: {}", error);
                                                let mut reject = vec![25u8];
                                                reject.extend_from_slice(&transfer_id.to_be_bytes());
                                                let _ = write_stream_input.send(reject);
                                            }
                                            println!("[FILE RX OFFER] direction=A->B transfer_id={} filename={} total_bytes={}", transfer_id, current_filename, total_file_size);
                                        }

                                        21 => {
                                            let mut hdr = [0u8; 16];
                                            if read_stream.read_exact(&mut hdr).is_err() { break; }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            let chunk_idx = u32::from_be_bytes(hdr[8..12].try_into().unwrap());
                                            let chunk_len = u32::from_be_bytes(hdr[12..16].try_into().unwrap()) as usize;
                                            if chunk_len > 10 * 1024 * 1024 { break; }
                                            let mut chunk_buf = vec![0u8; chunk_len];
                                            if read_stream.read_exact(&mut chunk_buf).is_err() { break; }
                                            
                                            println!("[FILE RX CHUNK] direction=A->B transfer_id={} chunk_index={} chunk_size={}", transfer_id, chunk_idx, chunk_len);
                                            
                                            let accepted = match crate::status::session_file_is_accepted(transfer_id) {
                                                Ok(accepted) => accepted,
                                                Err(error) => {
                                                    eprintln!("[FILE RX] Failed to read acceptance state: {}", error);
                                                    false
                                                }
                                            };
                                            println!("[FILE RX CHUNK STATE] direction=A->B transfer_id={} received_bytes={} chunk_count={} accepted={}", transfer_id, received_bytes, current_file_chunk_index, accepted);
                                            let chunk_is_valid = current_transfer_id == Some(transfer_id)
                                                && accepted
                                                && chunk_idx == current_file_chunk_index
                                                && received_bytes.saturating_add(chunk_len as u64) <= total_file_size;
                                            if chunk_is_valid {
                                                if current_file.is_none() {
                                                    let destination = canonical_drop_dir.as_ref()
                                                        .map_err(|error| std::io::Error::new(
                                                            std::io::ErrorKind::Other,
                                                            error.to_string(),
                                                        ))
                                                        .and_then(|root| {
                                                            create_session_transfer_file(
                                                                &drop_dir,
                                                                root,
                                                                &current_filename,
                                                            )
                                                        });
                                                    match destination {
                                                        Ok((file, path)) => {
                                                            current_file = Some(file);
                                                            current_file_path = Some(path);
                                                        }
                                                        Err(error) => {
                                                            eprintln!("[FILE RX START] direction=A->B transfer_id={} success=false reason=\"Failed to create file: {:?}\"", transfer_id, error);
                                                            send_file_error(transfer_id, "Unable to create destination file");
                                                            crate::status::finish_session_file(transfer_id);
                                                            current_transfer_id = None;
                                                            continue;
                                                        }
                                                    }
                                                }
                                                if let Some(ref mut file) = current_file {
                                                    if let Err(error) = file.write_all(&chunk_buf) {
                                                        println!("[FILE RX CHUNK] direction=A->B transfer_id={} success=false reason=\"{}\"", transfer_id, error);
                                                        send_file_error(transfer_id, &format!("Disk write failed: {}", error));
                                                        current_file = None;
                                                        if let Some(path) = current_file_path.take() {
                                                            let _ = fs::remove_file(path);
                                                        }
                                                        crate::status::finish_session_file(transfer_id);
                                                        current_transfer_id = None;
                                                        received_bytes = 0;
                                                        continue;
                                                    }
                                                    file_hasher.update(&chunk_buf);
                                                    received_bytes += chunk_len as u64;
                                                    current_file_chunk_index = current_file_chunk_index.saturating_add(1);
                                                }
                                            } else if current_transfer_id == Some(transfer_id) {
                                                let reason = if chunk_idx != current_file_chunk_index {
                                                    format!("Unexpected chunk index {}; expected {}", chunk_idx, current_file_chunk_index)
                                                } else {
                                                    "Chunk exceeded offered file size or transfer was not accepted".to_string()
                                                };
                                                send_file_error(transfer_id, &reason);
                                                current_file = None;
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                                crate::status::finish_session_file(transfer_id);
                                                current_transfer_id = None;
                                                received_bytes = 0;
                                            }
                                        }

                                        22 => {
                                            let mut hdr = [0u8; 48];
                                            if read_stream.read_exact(&mut hdr).is_err() { break; }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            let final_size = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
                                            let hash_hex: String = hdr[16..48].iter().map(|b| format!("{:02x}", b)).collect();
                                            let actual_hash = std::mem::replace(&mut file_hasher, Sha256::new()).finalize();
                                            let accepted = match crate::status::session_file_is_accepted(transfer_id) {
                                                Ok(accepted) => accepted,
                                                Err(error) => {
                                                    eprintln!("[FILE RX] Failed to read acceptance state: {}", error);
                                                    false
                                                }
                                            };
                                            if current_transfer_id.is_none() && final_size == 0 {
                                                current_transfer_id = Some(transfer_id);
                                            }
                                            if current_transfer_id == Some(transfer_id)
                                                && accepted
                                                && final_size == 0
                                                && current_file.is_none()
                                            {
                                                if let Ok(root) = &canonical_drop_dir {
                                                    match create_session_transfer_file(
                                                        &drop_dir,
                                                        root,
                                                        &current_filename,
                                                    ) {
                                                        Ok((file, path)) => {
                                                            current_file = Some(file);
                                                            current_file_path = Some(path);
                                                        }
                                                        Err(error) => {
                                                            eprintln!("[FILE RX END] Failed to create empty file: {}", error);
                                                        }
                                                    }
                                                }
                                            }
                                            let transfer_matches = current_transfer_id == Some(transfer_id) && accepted;
                                            let size_matches = total_file_size == final_size && received_bytes == final_size;
                                            let hash_matches = actual_hash.as_slice() == &hdr[16..48];
                                            let flush_ok = current_file
                                                .take()
                                                .map(|mut file| file.flush().is_ok())
                                                .unwrap_or(false);
                                            println!("[FILE RX VERIFY] direction=A->B transfer_id={} end_received=true file_closed={} expected_bytes={} received_bytes={} final_bytes={} size_match={} sha256_match={} accepted={}", transfer_id, flush_ok, total_file_size, received_bytes, final_size, size_matches, hash_matches, transfer_matches);
                                            let valid = transfer_matches && size_matches && hash_matches;
                                            if valid && flush_ok {
                                                let mut ack = vec![27u8];
                                                ack.extend_from_slice(&transfer_id.to_be_bytes());
                                                if let Err(error) = write_stream_input.send(ack) {
                                                    eprintln!("[FILE RX COMPLETE_ACK] direction=A->B transfer_id={} success=false reason=\"{}\"", transfer_id, error);
                                                } else {
                                                    println!("[FILE RX COMPLETE_ACK] direction=A->B transfer_id={} success=true", transfer_id);
                                                }
                                                println!("[FILE RX END] direction=A->B transfer_id={} final_sha256={} success=true", transfer_id, hash_hex);
                                            } else {
                                                send_file_error(transfer_id, "File size or SHA-256 verification failed");
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                                println!("[FILE RX END] direction=A->B transfer_id={} success=false reason=\"SHA-256 or size mismatch\"", transfer_id);
                                            }
                                            crate::status::finish_session_file(transfer_id);
                                            current_transfer_id = None;
                                            current_file_path = None;
                                        }

                                        23 => {
                                            let mut hdr = [0u8; 8];
                                            if read_stream.read_exact(&mut hdr).is_err() { break; }
                                            let transfer_id = u64::from_be_bytes(hdr);
                                            crate::status::resolve_session_file_response(transfer_id, 23);
                                            current_file = None;
                                            if current_transfer_id == Some(transfer_id) {
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                                current_transfer_id = None;
                                            }
                                            crate::status::finish_session_file(transfer_id);
                                            println!("[File Transfer] Cancelled.");
                                        }

                                        24 => {
                                            let mut hdr = [0u8; 12];
                                            if read_stream.read_exact(&mut hdr).is_err() { break; }
                                            let transfer_id = u64::from_be_bytes(hdr[0..8].try_into().unwrap());
                                            crate::status::resolve_session_file_response(transfer_id, 24);
                                            let msg_len = u16::from_be_bytes([hdr[10], hdr[11]]) as usize;
                                            if msg_len > 4096 { break; }
                                            let mut msg_buf = vec![0u8; msg_len];
                                            if read_stream.read_exact(&mut msg_buf).is_err() { break; }
                                            current_file = None;
                                            if current_transfer_id == Some(transfer_id) {
                                                if let Some(path) = current_file_path.take() {
                                                    let _ = fs::remove_file(path);
                                                }
                                                current_transfer_id = None;
                                            }
                                            crate::status::finish_session_file(transfer_id);
                                            if let Ok(msg) = String::from_utf8(msg_buf) {
                                                println!("[FILE RX ERROR] direction=A->B transfer_id={} success=false reason=\"{}\"", transfer_id, msg);
                                            }
                                        }

                                        25 | 26 | 27 => {
                                            let mut hdr = [0u8; 8];
                                            if read_stream.read_exact(&mut hdr).is_err() { break; }
                                            let transfer_id = u64::from_be_bytes(hdr);
                                            crate::status::resolve_session_file_response(transfer_id, pkt_type_buf[0]);
                                            println!("[FILE TX RESPONSE] transfer_id={} type={}", transfer_id, pkt_type_buf[0]);
                                        }

                                        99 => {
                                            println!("[SESSION DISCONNECT] Received TYPE 99 from relay; terminating active session.");
                                            is_conn_read.store(false, Ordering::SeqCst);
                                            break;
                                        }

                                        _ => {}
                                    }
                                }
                                current_file = None;
                                if let Some(path) = current_file_path.take() {
                                    let _ = fs::remove_file(path);
                                }
                                if let Some(transfer_id) = current_transfer_id.take() {
                                    crate::status::finish_session_file(transfer_id);
                                }
                            });

                            let ping_handle = thread::spawn(move || {
                                while is_conn_ping.load(Ordering::SeqCst) {
                                    let mut ping_pkt = Vec::with_capacity(9);
                                    ping_pkt.push(14u8);
                                    ping_pkt.extend_from_slice(&current_time_millis().to_be_bytes());
                                    if write_stream_ping.send(ping_pkt).is_err() {
                                        break;
                                    }
                                    thread::sleep(Duration::from_millis(100));
                                }
                            });

                            let clip_handle = thread::spawn(move || {
                                let mut clip = Clipboard::new().ok();
                                while is_conn_clip.load(Ordering::SeqCst) {
                                    if let Some(ref mut c) = clip {
                                        if let Ok(text) = c.get_text() {
                                            let mut is_new = false;
                                            if let Ok(mut guard) = last_clip_send.lock() {
                                                if *guard != text && !text.is_empty() {
                                                    *guard = text.clone();
                                                    is_new = true;
                                                }
                                            }
                                            if is_new {
                                                let bytes = text.into_bytes();
                                                let mut packet = Vec::with_capacity(5 + bytes.len());
                                                packet.push(12u8);
                                                packet.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                                                packet.extend_from_slice(&bytes);
                                                if write_stream_clip.send(packet).is_err() {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    thread::sleep(Duration::from_millis(1000));
                                }
                            });

                            let _audio = start_audio_capture(write_stream_audio, is_conn_audio);

                            let capture_handle = thread::spawn(move || {
                                let mut screens = Screen::all().unwrap_or_default();
                                let mut last_idx = active_idx_capture.load(Ordering::SeqCst);

                                println!("[Capture] {} monitor(s) detected.", screens.len());

                                let mut next_frame_time = Instant::now() + Duration::from_micros(FRAME_INTERVAL_MICROS);
                                let mut captured_frames = 0u64;
                                let mut last_capture_log = Instant::now();

                                while is_conn_capture.load(Ordering::SeqCst) {
                                    if !crate::status::session_perm_view_screen() {
                                        thread::sleep(Duration::from_millis(50));
                                        continue;
                                    }

                                    let current_idx = active_idx_capture.load(Ordering::SeqCst);

                                    if current_idx != last_idx || screens.is_empty() {
                                        screens = Screen::all().unwrap_or_default();
                                        last_idx = current_idx;
                                    }

                                    if screens.is_empty() {
                                        thread::sleep(Duration::from_micros(FRAME_INTERVAL_MICROS));
                                        continue;
                                    }

                                    let mut captured_frame = None;

                                    if !screens.is_empty() {
                                        let screen = match screens.get(current_idx) {
                                            Some(s) => s,
                                            None => &screens[0],
                                        };
                                        if let Ok(img) = screen.capture() {
                                            let source_width = img.width() as usize;
                                            let source_height = img.height() as usize;
                                            let raw = img.into_raw();
                                            let expected = source_width.saturating_mul(source_height).saturating_mul(4);
                                            if raw.len() >= expected {
                                                captured_frame = Some(FrameData {
                                                    width: source_width,
                                                    height: source_height,
                                                    raw_pixels: raw,
                                                    captured_at_ms: current_time_millis(),
                                                });
                                            }
                                        }
                                    }

                                    if captured_frame.is_none() {
                                        captured_frame = capture_screen_gdi();
                                    }

                                    if let Some(frame) = captured_frame {
                                        captured_frames += 1;
                                        if last_capture_log.elapsed() >= Duration::from_secs(1) {
                                            println!(
                                                "[VIDEO CAPTURE] frames={} captured_at_ms={} source={}x{}",
                                                captured_frames,
                                                frame.captured_at_ms,
                                                frame.width,
                                                frame.height
                                            );
                                            captured_frames = 0;
                                            last_capture_log = Instant::now();
                                        }
                                        let (lock, cvar) = &*shared_frame_cap;
                                        if let Ok(mut shared) = lock.lock() {
                                            *shared = Some(frame);
                                            cvar.notify_one();
                                        }
                                    } else {
                                        screens = Screen::all().unwrap_or_default();
                                        thread::sleep(Duration::from_millis(16));
                                    }

                                    let now = Instant::now();
                                    if now < next_frame_time {
                                        thread::sleep(next_frame_time - now);
                                    }
                                    
                                    // Advance monotonic timer and avoid accumulating extreme debt
                                    next_frame_time += Duration::from_micros(FRAME_INTERVAL_MICROS);
                                    let current_time = Instant::now();
                                    if next_frame_time < current_time {
                                        next_frame_time = current_time;
                                    }
                                }
                            });

                            let mut frame_number: u64 = 0;
                            let mut hw_encoder: Option<HardwareH264Encoder> = None;
                            let mut current_enc_width: u32 = 0;
                            let mut current_enc_height: u32 = 0;
                            let mut encode_error_count = 0u64;

                            while is_conn_write.load(Ordering::SeqCst) {
                                let frame_opt = {
                                    let (lock, cvar) = &*shared_frame;
                                    let mut shared = match lock.lock() {
                                        Ok(g) => g,
                                        Err(_) => break,
                                    };

                                    if shared.is_none() {
                                        let result = cvar.wait_timeout(shared, Duration::from_millis(50));
                                        match result {
                                            Ok((guard, _)) => {
                                                shared = guard;
                                            }
                                            Err(_) => continue,
                                        }
                                    }
                                    shared.take()
                                };

                                let frame = match frame_opt {
                                    Some(f) => f,
                                    None => continue,
                                };
                                
                                let recovery_req = video_recovery_capture.load(Ordering::Acquire);

                                let src_width = frame.width;
                                let src_height = frame.height;
                                if src_width == 0 || src_height == 0 {
                                    continue;
                                }

                                if hw_encoder.is_none() || current_enc_width != src_width as u32 || current_enc_height != src_height as u32 {
                                    let dynamic_bitrate = ((src_width * src_height) as f64 * 16.0) as u32; 
                                    let target_bitrate = dynamic_bitrate.clamp(20_000_000, 75_000_000); // 20 to 75 Mbps

                                    match HardwareH264Encoder::new(src_width as u32, src_height as u32, TARGET_FPS, target_bitrate) {
                                        Ok(enc) => {
                                            hw_encoder = Some(enc);
                                            current_enc_width = src_width as u32;
                                            current_enc_height = src_height as u32;
                                        }
                                        Err(e) => {
                                            eprintln!("[H264 HW][FATAL] Hardware encoder initialization failed: {}", e);
                                            break;
                                        }
                                    }
                                }

                                let encoder = hw_encoder.as_mut().unwrap();
                                let is_keyframe_request = frame_number == 0 || !encoder.has_produced_keyframe || (frame_number % (TARGET_FPS as u64) == 0) || recovery_req;
                                if is_keyframe_request {
                                    println!("[H264] KEYFRAME REQUESTED");
                                }

                                let h264_bytes = match encoder.encode_rgba(&frame.raw_pixels, src_width, src_height, is_keyframe_request) {
                                    Ok(bytes) => bytes,
                                    Err(error) => {
                                        encode_error_count += 1;
                                        if encode_error_count == 1 || encode_error_count % 60 == 0 {
                                            eprintln!("[VIDEO ENCODE ERROR] count={} error={}", encode_error_count, error);
                                        }
                                        continue;
                                    }
                                };

                                if h264_bytes.is_empty() {
                                    continue;
                                }

                                frame_number += 1;
                                let mut nal_types = Vec::new();
                                let mut has_sps = false;
                                let mut has_pps = false;
                                let mut has_idr = false;

                                let (parsed_nals, sps_found, pps_found, idr_found) = encoder::mf_encoder::inspect_nals(&h264_bytes);
                                nal_types = parsed_nals;
                                has_sps = sps_found;
                                has_pps = pps_found;
                                has_idr = idr_found;
                                let mut i = 0;
                                while i + 3 < h264_bytes.len() {
                                    if (h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 1)
                                        || (i + 4 <= h264_bytes.len() && h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 0 && h264_bytes[i+3] == 1)
                                    {
                                        let sc_len = if h264_bytes[i+2] == 1 { 3 } else { 4 };
                                        if i + sc_len < h264_bytes.len() {
                                            let n_type = h264_bytes[i + sc_len] & 0x1F;
                                            nal_types.push(n_type);
                                            if n_type == 7 { has_sps = true; }
                                            else if n_type == 8 { has_pps = true; }
                                            else if n_type == 5 { has_idr = true; }
                                        }
                                        i += sc_len;
                                    } else {
                                        i += 1;
                                    }
                                }

                                let log_video_frame = frame_number <= 5
                                    || frame_number % (TARGET_FPS as u64) == 0;
                                if log_video_frame {
                                    if has_idr {
                                        println!("[H264] IDR GENERATED");
                                        println!("[H264] NAL TYPES={:?}", nal_types);
                                        println!("[H264] SENDING KEYFRAME TO VIEWER");
                                    }

                                    let mut raw_hash = 0u64;
                                    for (idx, &b) in frame.raw_pixels.iter().take(4096).enumerate() {
                                        raw_hash = raw_hash.wrapping_add((b as u64).wrapping_mul(idx as u64 + 1));
                                    }
                                    let pts = (frame_number as u64) * (1_000_000u64 / (TARGET_FPS as u64));

                                    println!("[ENCODER]\nframe={}\ninput_bytes={}\ninput_hash={:016x}\noutput_bytes={}\nnal_types={:?}\nsps={}\npps={}\nidr={}\npts={}",
                                        frame_number,
                                        frame.raw_pixels.len(),
                                        raw_hash,
                                        h264_bytes.len(),
                                        nal_types,
                                        has_sps,
                                        has_pps,
                                        has_idr,
                                        pts
                                    );
                                    println!("[VIDEO ENCODE]\nframe={}\nbytes={}\nkeyframe={}\nSPS={}\nPPS={}\nIDR={}",
                                        frame_number,
                                        h264_bytes.len(),
                                        if has_idr { "true" } else { "false" },
                                        if has_sps { "true" } else { "false" },
                                        if has_pps { "true" } else { "false" },
                                        if has_idr { "true" } else { "false" }
                                    );

                                    let format_str = if h264_bytes.starts_with(&[0, 0, 0, 1]) || h264_bytes.starts_with(&[0, 0, 1]) {
                                        "AnnexB"
                                    } else {
                                        "AVC"
                                    };
                                    println!("[AGENT H264]\nencoder=hardware\nformat={}\nsize={}\nNAL types={:?}", format_str, h264_bytes.len(), nal_types);
                                    println!("[VIDEO TX]");
                                    println!("capture = YES");
                                    println!("encoded = YES");
                                    println!("width = {}", current_enc_width);
                                    println!("height = {}", current_enc_height);
                                    println!("bytes = {}", h264_bytes.len());
                                    println!("keyframe = {}", if has_idr { "YES" } else { "NO" });
                                }

                                let packet_size = 21 + h264_bytes.len();
                                let mut packet = Vec::with_capacity(packet_size);
                                packet.push(13u8);
                                packet.extend_from_slice(&current_enc_width.to_be_bytes());
                                packet.extend_from_slice(&current_enc_height.to_be_bytes());
                                packet.extend_from_slice(&(h264_bytes.len() as u32).to_be_bytes());
                                packet.extend_from_slice(&frame.captured_at_ms.to_be_bytes());
                                packet.extend_from_slice(&h264_bytes);

                                if log_video_frame {
                                    println!("[VIDEO TX PREP]");
                                    println!("type = 13");
                                    println!("width = {}", current_enc_width);
                                    println!("height = {}", current_enc_height);
                                    println!("payload_size = {}", h264_bytes.len());
                                    println!("keyframe = {}", if has_idr { "true" } else { "false" });
                                    println!("packet_size = {}", packet.len());
                                }

                                if has_idr {
                                    video_recovery_capture.store(false, Ordering::Release);
                                } else if recovery_req {
                                    // Encoder was requested to produce IDR, but didn't produce one yet
                                    continue;
                                }

                                let mut queue = match video_slot_capture.lock() {
                                    Ok(q) => q,
                                    Err(_) => {
                                        println!("[VIDEO TX QUEUE ERROR]\nerror=slot_poisoned");
                                        is_conn_write.store(false, Ordering::SeqCst);
                                        break;
                                    }
                                };
                                
                                if queue.len() >= 3 {
                                    queue.clear();
                                    if !has_idr {
                                        video_recovery_capture.store(true, Ordering::Release);
                                        println!("[VIDEO TX DROP] Low-latency queue reached 3 frames; discarded stale frames and requested IDR");
                                        continue;
                                    }
                                }

                                queue.push_back(packet);
                            }

                            // SHUTDOWN OF SESSION
                            is_connected.store(false, Ordering::SeqCst);
                            drop(write_stream);

                            let _ = input_handle.join();
                            let _ = ping_handle.join();
                            let _ = clip_handle.join();
                            let _ = capture_handle.join();
                            let _ = writer_handle.join();

                            backend.log_session_end(&system_id, 0.0);
                            println!("[STREAM STATE] STOPPED");
                            crate::session_debug::log(
                                &system_id,
                                "B_STREAM_STATE state=STOPPED",
                            );

                            println!("[Agent] Session ended. Preparing for next request...");
                            is_in_session.store(false, Ordering::SeqCst);
                            // Session ended — update the health endpoint so B's overlay hides.
                            crate::status::set_session_active(false);
                            if let Err(error) = crate::status::set_session_writer(None) {
                                eprintln!("[SESSION UI] Failed to clear session bridge: {}", error);
                            }
                            if let Err(error) = crate::status::clear_session_peer_system_id() {
                                eprintln!("[SESSION UI] Failed to clear session peer identity: {}", error);
                            }
                            thread::sleep(Duration::from_millis(500));
                            }); // END OF THREAD

                            intentional_reconnect = true;
                            break 'viewer_loop;
                        }

                        // Type 99: Session disconnect signal
                        99 => {
                            println!("[Agent] Idle state confirmed.");
                        }

                        _ => {}
                    }
                }

                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                        continue 'viewer_loop;
                    }
                    println!("[RELAY][DISCONNECT] reason=Socket read error or closed by relay: {:?}", e);
                    println!("[RELAY][DISCONNECT] system_id={}", system_id);
                    println!("[RELAY][DISCONNECT] socket_error={:?}", e);
                    println!("[RELAY][DISCONNECT] remote_closed=true");
                    crate::session_debug::log(
                        &system_id,
                        &format!("B_DISCONNECT error={}", e),
                    );
                    crate::session_debug::log(
                        &system_id,
                        "STATE_CHANGE old=CONNECTED new=DISCONNECTED",
                    );
                    println!("[SESSION STATE] OFFLINE");
                    break 'viewer_loop;
                }
            }
        }

        is_running_conn.store(false, Ordering::SeqCst);
        let _ = heartbeat_handle.join();

        if intentional_reconnect {
            intentional_reconnect = false;
            backoff_secs = 1;
            println!("[Agent] Pairing complete. Immediately replenishing listener socket...");
            continue;
        }

        println!("[Agent] Relay connection lost. Reconnecting in {}s...", backoff_secs);
        thread::sleep(Duration::from_secs(backoff_secs));
        backoff_secs = match backoff_secs { 1 => 2, 2 => 5, 5 => 10, _ => 10 };
    }
}

// ============================================================
// MAIN
// ============================================================

#[cfg(test)]
mod session_feature_tests {
    use super::{decode_audio_packet, make_file_error_packet, read_next_file_chunk};
    use std::io::{self, Read};

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "test read failure"))
        }
    }

    #[test]
    fn type_17_audio_packet_decodes_interleaved_samples() {
        let mut payload = Vec::new();
        for sample in [0.25f32, 0.25, -0.5, -0.5] {
            payload.extend_from_slice(&sample.to_le_bytes());
        }
        assert_eq!(
            decode_audio_packet(&payload, 48_000, 2, 48_000).unwrap(),
            vec![0.25, -0.5]
        );
        assert!(decode_audio_packet(&payload[..3], 48_000, 2, 48_000).is_none());
    }

    #[test]
    fn file_sender_distinguishes_eof_from_read_failure() {
        let mut eof_reader = io::Cursor::new(Vec::<u8>::new());
        assert_eq!(read_next_file_chunk(&mut eof_reader, &mut [0u8; 8]).unwrap(), None);

        let mut failing_reader = FailingReader;
        assert_eq!(
            read_next_file_chunk(&mut failing_reader, &mut [0u8; 8])
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn file_read_error_uses_existing_type_24_frame() {
        let packet = make_file_error_packet(0x0102_0304_0506_0708, "read failed");
        assert_eq!(packet[0], 24);
        assert_eq!(&packet[1..9], &0x0102_0304_0506_0708u64.to_be_bytes());
        assert_eq!(&packet[9..11], &[0, 0]);
        assert_eq!(u16::from_be_bytes([packet[11], packet[12]]), 11);
        assert_eq!(&packet[13..], b"read failed");
    }
}

fn main() {
    // ---------------------------------------------------------
    // PHASE 8: ENTERPRISE BOOT LOGGER & PHASE 4: PROVISIONING
    // ---------------------------------------------------------
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\temp".to_string());
    let deskstream_dir = std::path::Path::new(&local_app_data).join("DeskStream");
    let _ = std::fs::create_dir_all(&deskstream_dir);
    
    // Create the boot logger in the %TEMP% directory as requested (or a subfolder there)
    let temp_dir = std::env::var("TEMP").unwrap_or_else(|_| "C:\\temp".to_string());
    let boot_log_path = std::path::Path::new(&temp_dir).join("DeskStream-boot.log");

    // Clear old boot log or keep appending if we prefer
    let mut boot_log = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&boot_log_path)
        .expect("Failed to open boot log");

    use std::io::Write;
    let now = || -> String {
        // Very basic ISO-like format using SystemTime (for standard Rust without chrono)
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        format!("[{}]", secs)
    };

    let _ = writeln!(boot_log, "{} DeskStream starting", now());
    let _ = writeln!(boot_log, "{} Version: {}", now(), env!("CARGO_PKG_VERSION"));
    
    if let Ok(exe_path) = std::env::current_exe() {
        let _ = writeln!(boot_log, "{} Executable: {}", now(), exe_path.display());
        if let Some(parent) = exe_path.parent() {
            let _ = std::env::set_current_dir(parent);
        }
    }
    
    let _ = writeln!(boot_log, "{} Operating System: Windows", now());
    let _ = writeln!(boot_log, "{} Architecture: x64", now());
    let _ = writeln!(boot_log, "{} User data: {}", now(), deskstream_dir.display());

    // Single-instance enforcement via port 49182
    let listener = match std::net::TcpListener::bind("127.0.0.1:49182") {
        Ok(listener) => listener,
        Err(_) => {
            // Another instance is already running — bring its window to the foreground
            #[cfg(windows)]
            unsafe {
                let title: Vec<u16> = "DeskStream Agent\0".encode_utf16().collect();
                let main_hwnd = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                    std::ptr::null(),
                    title.as_ptr(),
                );
                if main_hwnd != 0 {
                    windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                        main_hwnd,
                        windows_sys::Win32::UI::WindowsAndMessaging::SW_RESTORE,
                    );
                    windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(main_hwnd);
                }
            }
            agent_log!("[Agent] Another DeskStream instance is running — brought to foreground.");
            std::process::exit(0);
        }
    };

    set_process_dpi_aware();

    let args: Vec<String> = env::args().collect();

    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "34.229.20.54:9001".to_string()
    };

    // FIRST-LAUNCH CONFIG PROVISIONING
    let config_path = deskstream_dir.join("config.json");
    if !config_path.exists() {
        let default_config = r#"{
    "version": 1,
    "initialized": true
}"#;
        let _ = std::fs::write(&config_path, default_config);
    }
    let _ = writeln!(boot_log, "{} Configuration initialized", now());

    let keys_path = deskstream_dir.join("keys.json");
    if !keys_path.exists() {
        let _ = std::fs::write(&keys_path, "{}");
    }
    let env_path = deskstream_dir.join(".env");
    if !env_path.exists() {
        let _ = std::fs::write(&env_path, "DESKSTREAM_SIGNING_ENABLED=false\n");
    }
    let _ = writeln!(boot_log, "{} Environment initialized", now());

    // Load persistent identity
    let config = identity::device_id::AgentConfig::load_or_create("", &relay_addr);
    let relay_addr = config.relay_addr.clone();

    // Initialise shared log buffer (bounded, 500 lines)
    let log_buf: Arc<Mutex<std::collections::VecDeque<String>>> =
        Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(500)));

    // Initialise LIVE status global
    let initial_status = crate::status::AgentStatus::new(&config.system_id, &config.name);
    let _ = crate::status::LIVE.set(Arc::new(Mutex::new(initial_status)));

    let current_exe_path = env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();

    agent_log!("========================================");
    agent_log!("       REMOTE DESKTOP AGENT STARTING");
    agent_log!("  EXECUTABLE:    {}", current_exe_path);
    agent_log!("========================================");

    // Health server on port 49182 — keep existing IPC
    let sys_id_for_health = config.system_id.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut buf = [0; 1024];
                if let Ok(n) = stream.read(&mut buf) {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let is_options = request.starts_with("OPTIONS");
                    
                    let mut is_discover = false;
                    let mut target_id = String::new();
                    if let Some(path_start) = request.find("GET /") {
                        let path_part = &request[path_start + 4..];
                        if let Some(space_idx) = path_part.find(' ') {
                            let path = &path_part[..space_idx];
                            if path.starts_with("/discover?target=") {
                                is_discover = true;
                                target_id = path.trim_start_matches("/discover?target=").to_string();
                            }
                        }
                    }

                    let response = if is_options {
                        // CORS preflight response
                        "HTTP/1.1 204 No Content\r\n\
                         Access-Control-Allow-Origin: *\r\n\
                         Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                         Access-Control-Allow-Headers: Content-Type\r\n\
                         Access-Control-Allow-Private-Network: true\r\n\
                         Access-Control-Max-Age: 86400\r\n\
                         Connection: close\r\n\
                         \r\n".to_string()
                    } else if is_discover {
                        if let Some((ip, port)) = crate::network::discovery::discover_target_agent(&target_id) {
                            format!(
                                "HTTP/1.1 200 OK\r\n\
                                 Content-Type: application/json\r\n\
                                 Access-Control-Allow-Origin: *\r\n\
                                 Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                                 Access-Control-Allow-Headers: Content-Type\r\n\
                                 Access-Control-Allow-Private-Network: true\r\n\
                                 Connection: close\r\n\
                                 \r\n\
                                 {{\"status\": \"found\", \"ip\": \"{}\", \"port\": {}}}",
                                ip, port
                            )
                        } else {
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: application/json\r\n\
                             Access-Control-Allow-Origin: *\r\n\
                             Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                             Access-Control-Allow-Headers: Content-Type\r\n\
                             Access-Control-Allow-Private-Network: true\r\n\
                             Connection: close\r\n\
                             \r\n\
                             {\"status\": \"not_found\"}".to_string()
                        }
                    } else {
                        // Normal GET response with JSON body
                        format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: application/json\r\n\
                             Access-Control-Allow-Origin: *\r\n\
                             Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                             Access-Control-Allow-Headers: Content-Type\r\n\
                             Access-Control-Allow-Private-Network: true\r\n\
                             Connection: close\r\n\
                             \r\n\
                             {{\"running\": true, \"system_id\": \"{}\", \"status\": \"online\"}}",
                            sys_id_for_health
                        )
                    };
                    let _ = stream.write_all(response.as_bytes());
                }
            }
        }
    });

    agent_log!("========================================");
    agent_log!("       REMOTE DESKTOP AGENT (60 FPS)");
    agent_log!("  BUILD VERSION: 1.1.3 (Production Desktop Agent)");
    agent_log!("  EXECUTABLE:    {}", current_exe_path);
    agent_log!("========================================");
    agent_log!("  Device UUID: {}", config.device_uuid);
    agent_log!("  System ID:   {}", config.formatted_id());
    agent_log!("  Relay: {}", relay_addr);
    agent_log!("========================================");

    crate::network::discovery::start_discovery_listener(config.system_id.clone());

    // Spawn the complete agent engine on a background thread
    // The WebView UI runs on the main thread (required by Win32/WebView2).
    let agent_config = config.clone();
    let agent_relay_addr = relay_addr.clone();
    thread::spawn(move || {
        run_agent_loop(agent_relay_addr, agent_config);
    });

    // ============================================================
    // WEBVIEW2 GUI — runs on main thread
    // Shows the exact DeskStream website design inside a native window.
    // Agent engine runs independently on background threads.
    // ============================================================
    let quit_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Start system tray icon
    tray::start_tray(quit_signal.clone());

    // Start embedded local HTTP server for the UI and API Proxy
    let _ = writeln!(boot_log, "{} Backend starting", now());
    let local_port = local_server::start_local_server(config.system_id.clone(), quit_signal.clone());
    let local_url = format!("http://127.0.0.1:{}/dashboard.html", local_port);
    agent_log!("[BOOT] Started local embedded UI server on {}", local_url);

    // Health Poll Loop
    let mut healthy = false;
    for attempt in 1..=60 { // 30 seconds max
        let _ = writeln!(boot_log, "{} Health check attempt {}", now(), attempt);
        if let Ok(resp) = reqwest::blocking::get(format!("http://127.0.0.1:{}/health", local_port)) {
            if resp.status().is_success() {
                healthy = true;
                let _ = writeln!(boot_log, "{} Backend healthy", now());
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    if healthy {
        let _ = writeln!(boot_log, "{} Main window created", now());
        webview_app::run_webview(quit_signal, local_url);
    } else {
        let _ = writeln!(boot_log, "{} Fatal startup error: Backend unhealthy after 30 seconds", now());
        let error_html = format!(
            "data:text/html;charset=utf-8,<html><head><title>Startup Error</title><style>
            body {{ font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; padding: 40px; background: #0f172a; color: #f8fafc; }}
            h2 {{ font-weight: 600; color: #f1f5f9; }}
            .container {{ max-width: 600px; margin: 40px auto; background: #1e293b; padding: 30px; border-radius: 12px; box-shadow: 0 10px 15px -3px rgba(0,0,0,0.1); border: 1px solid #334155; }}
            pre {{ background: #0f172a; padding: 15px; border-radius: 8px; overflow-x: auto; font-size: 13px; color: #cbd5e1; border: 1px solid #334155; }}
            .btn {{ display: inline-block; background: #3b82f6; color: white; padding: 8px 16px; border-radius: 6px; text-decoration: none; font-weight: 500; margin-right: 12px; font-size: 14px; }}
            .btn-outline {{ background: transparent; border: 1px solid #475569; color: #e2e8f0; }}
            </style></head><body>
            <div class='container'>
            <h2>DeskStream</h2>
            <p>Unable to start the local service.</p>
            <p><strong>Error:</strong> Local backend health check timed out.</p>
            <p><strong>Diagnostic log:</strong></p>
            <pre>{}</pre>
            <div style='margin-top: 24px;'>
                <a href='#' onclick='window.location.reload()' class='btn'>Retry</a>
                <a href='#' class='btn btn-outline'>Exit</a>
            </div>
            </div></body></html>",
            boot_log_path.to_string_lossy().replace("\\", "/")
        );
        webview_app::run_webview(quit_signal, error_html);
    }
}
