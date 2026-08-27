use crate::identity::device_id::AgentConfig;
use crate::relay::RelayStats;
use crate::ui::font::{draw_text, text_width};
use arboard::Clipboard;
use minifb::{Key, Window};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const WINDOW_WIDTH: usize = 820;
pub const WINDOW_HEIGHT: usize = 620;

// Color Palette (Dark Theme / Slate & Indigo)
pub const COLOR_BG: u32 = 0x0B0F19;         // Deep Slate Background
pub const COLOR_SURFACE: u32 = 0x151D2C;    // Card Surface
pub const COLOR_SURFACE_ALT: u32 = 0x1E293B;// Header / Secondary Card
pub const COLOR_BORDER: u32 = 0x334155;     // Card Border
pub const COLOR_BORDER_ACTIVE: u32 = 0x3B82F6; // Active Border
pub const COLOR_PRIMARY: u32 = 0x2563EB;    // Indigo / Blue Primary
pub const COLOR_PRIMARY_HOVER: u32 = 0x1D4ED8;
pub const COLOR_SUCCESS: u32 = 0x10B981;    // Emerald Green (Online)
pub const COLOR_WARNING: u32 = 0xF59E0B;    // Amber (Connecting)
pub const COLOR_DANGER: u32 = 0xEF4444;     // Red (Disconnected / Error)
pub const COLOR_TEXT_MAIN: u32 = 0xF8FAFC;  // Pure White / Slate 50
pub const COLOR_TEXT_MUTED: u32 = 0x94A3B8; // Slate 400
pub const COLOR_TEXT_DIM: u32 = 0x64748B;   // Slate 500
pub const COLOR_INPUT_BG: u32 = 0x0F172A;   // Input Background
pub const COLOR_ACCENT_CYAN: u32 = 0x06B6D4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard = 0,
    RelayServer = 1,
    Settings = 2,
    Logs = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Starting,
    Connecting,
    Authenticating,
    Online,
    Disconnected,
}

impl ConnectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionStatus::Starting => "STARTING",
            ConnectionStatus::Connecting => "CONNECTING",
            ConnectionStatus::Authenticating => "AUTHENTICATING",
            ConnectionStatus::Online => "ONLINE",
            ConnectionStatus::Disconnected => "DISCONNECTED",
        }
    }
}

pub struct UiApp {
    pub current_tab: Tab,
    pub input_partner_id: String,
    pub input_partner_pin: String,
    pub input_relay_addr: String,
    pub input_pin: String,
    pub active_field: usize, // 0: None, 1: Partner ID, 2: Partner PIN, 3: Relay Addr, 4: Device PIN
    pub copy_feedback_timer: Option<Instant>,
    pub copy_server_ip_timer: Option<Instant>,
    pub save_feedback_timer: Option<Instant>,
    pub connect_request: Option<(String, String)>,
    pub test_network_requested: bool,
    pub network_test_result: Option<(bool, String)>,
    pub restart_relay_requested: bool,
    pub config_updated: bool,
    pub log_scroll: usize,
    pub gpu_backend_name: String,
    pub lan_ips: Vec<String>,
    last_mouse_down: bool,
}

impl UiApp {
    pub fn new(config: &AgentConfig, gpu_backend: &str, lan_ips: Vec<String>) -> Self {
        Self {
            current_tab: if config.is_server_mode() { Tab::RelayServer } else { Tab::Dashboard },
            input_partner_id: String::new(),
            input_partner_pin: String::new(),
            input_relay_addr: config.relay_addr.clone(),
            input_pin: config.pin.clone(),
            active_field: 0,
            copy_feedback_timer: None,
            copy_server_ip_timer: None,
            save_feedback_timer: None,
            connect_request: None,
            test_network_requested: false,
            network_test_result: None,
            restart_relay_requested: false,
            config_updated: false,
            log_scroll: 0,
            gpu_backend_name: gpu_backend.to_string(),
            lan_ips,
            last_mouse_down: false,
        }
    }

    pub fn handle_input(&mut self, window: &Window, config: &mut AgentConfig) {
        let mouse_down = window.get_mouse_down(minifb::MouseButton::Left);
        let mouse_pos = window.get_mouse_pos(minifb::MouseMode::Pass).unwrap_or((0.0, 0.0));
        let mx = mouse_pos.0 as usize;
        let my = mouse_pos.1 as usize;

        let clicked = mouse_down && !self.last_mouse_down;
        self.last_mouse_down = mouse_down;

        // Handle keyboard characters for active text inputs
        let keys = window.get_keys_pressed(minifb::KeyRepeat::Yes);
        for key in keys {
            match key {
                Key::Backspace => {
                    match self.active_field {
                        1 => { self.input_partner_id.pop(); }
                        2 => { self.input_partner_pin.pop(); }
                        3 => { self.input_relay_addr.pop(); }
                        4 => { self.input_pin.pop(); }
                        _ => {}
                    }
                }
                Key::Tab => {
                    self.active_field = match self.active_field {
                        1 => 2,
                        2 => 1,
                        3 => 4,
                        4 => 3,
                        _ => 1,
                    };
                }
                Key::Escape => {
                    self.active_field = 0;
                }
                Key::Enter => {
                    if self.active_field == 1 || self.active_field == 2 {
                        let target = self.input_partner_id.replace(" ", "").trim().to_string();
                        if !target.is_empty() {
                            self.connect_request = Some((target, self.input_partner_pin.trim().to_string()));
                        }
                    } else if self.active_field == 3 || self.active_field == 4 {
                        config.relay_addr = self.input_relay_addr.trim().to_string();
                        config.pin = self.input_pin.trim().to_string();
                        let _ = config.save("");
                        self.config_updated = true;
                        self.save_feedback_timer = Some(Instant::now());
                        self.active_field = 0;
                    }
                }
                _ => {
                    if let Some(c) = key_to_char(key) {
                        match self.active_field {
                            1 => {
                                if self.input_partner_id.len() < 12 {
                                    self.input_partner_id.push(c);
                                }
                            }
                            2 => {
                                if self.input_partner_pin.len() < 16 {
                                    self.input_partner_pin.push(c);
                                }
                            }
                            3 => {
                                if self.input_relay_addr.len() < 40 {
                                    self.input_relay_addr.push(c);
                                }
                            }
                            4 => {
                                if self.input_pin.len() < 16 {
                                    self.input_pin.push(c);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if clicked {
            // Mode switcher buttons in header
            if is_in_rect(mx, my, 560, 18, 110, 32) {
                if config.app_mode != "client" {
                    config.app_mode = "client".to_string();
                    let _ = config.save("");
                    self.config_updated = true;
                    self.current_tab = Tab::Dashboard;
                }
            } else if is_in_rect(mx, my, 680, 18, 110, 32) {
                if config.app_mode != "server" {
                    config.app_mode = "server".to_string();
                    let _ = config.save("");
                    self.config_updated = true;
                    self.current_tab = Tab::RelayServer;
                }
            }

            // Tab bar
            let tab_y = 70;
            if is_in_rect(mx, my, 30, tab_y, 140, 36) { self.current_tab = Tab::Dashboard; self.active_field = 0; }
            if is_in_rect(mx, my, 180, tab_y, 140, 36) { self.current_tab = Tab::RelayServer; self.active_field = 0; }
            if is_in_rect(mx, my, 330, tab_y, 140, 36) { self.current_tab = Tab::Settings; self.active_field = 0; }
            if is_in_rect(mx, my, 480, tab_y, 140, 36) { self.current_tab = Tab::Logs; self.active_field = 0; }

            // Tab 0: Dashboard clicks
            if self.current_tab == Tab::Dashboard {
                // Copy ID Button
                if is_in_rect(mx, my, 210, 240, 140, 34) {
                    if let Ok(mut clip) = Clipboard::new() {
                        let clean_id = config.system_id.clone();
                        let _ = clip.set_text(clean_id);
                        self.copy_feedback_timer = Some(Instant::now());
                    }
                }

                // If offline banner clicked -> jump to settings
                if is_in_rect(mx, my, 50, 410, 320, 30) {
                    self.current_tab = Tab::Settings;
                    self.active_field = 3;
                }

                // Partner ID Input Field
                if is_in_rect(mx, my, 440, 200, 320, 38) {
                    self.active_field = 1;
                } else if is_in_rect(mx, my, 440, 275, 320, 38) {
                    self.active_field = 2;
                } else if is_in_rect(mx, my, 440, 335, 320, 44) {
                    // Connect Button
                    let target = self.input_partner_id.replace(" ", "").trim().to_string();
                    if !target.is_empty() {
                        self.connect_request = Some((target, self.input_partner_pin.trim().to_string()));
                    }
                } else if !is_in_rect(mx, my, 440, 150, 340, 300) {
                    self.active_field = 0;
                }
            }

            // Tab 1: Relay Server clicks
            if self.current_tab == Tab::RelayServer {
                // Copy Server Address button
                if is_in_rect(mx, my, 600, 255, 140, 32) {
                    let server_ip = self.lan_ips.first().cloned().unwrap_or_else(|| "127.0.0.1".to_string());
                    let full_addr = format!("{}:9001", server_ip);
                    if let Ok(mut clip) = Clipboard::new() {
                        let _ = clip.set_text(full_addr);
                        self.copy_server_ip_timer = Some(Instant::now());
                    }
                }

                if is_in_rect(mx, my, 560, 160, 200, 40) {
                    self.restart_relay_requested = true;
                }
            }

            // Tab 2: Settings clicks
            if self.current_tab == Tab::Settings {
                if is_in_rect(mx, my, 50, 235, 420, 38) {
                    self.active_field = 3;
                } else if is_in_rect(mx, my, 50, 315, 420, 38) {
                    self.active_field = 4;
                } else if is_in_rect(mx, my, 50, 375, 200, 42) {
                    // Save Settings & Connect Now
                    config.relay_addr = self.input_relay_addr.trim().to_string();
                    config.pin = self.input_pin.trim().to_string();
                    let _ = config.save("");
                    self.config_updated = true;
                    self.save_feedback_timer = Some(Instant::now());
                    self.active_field = 0;
                } else if is_in_rect(mx, my, 260, 375, 180, 42) {
                    // Test Network Connection
                    self.test_network_requested = true;
                } else {
                    self.active_field = 0;
                }
            }
        }
    }

    pub fn render(
        &self,
        buffer: &mut [u32],
        config: &AgentConfig,
        conn_status: ConnectionStatus,
        last_conn_error: &str,
        relay_running: bool,
        relay_stats: &RelayStats,
        logs: &Arc<Mutex<VecDeque<String>>>,
    ) {
        // Clear background
        buffer.fill(COLOR_BG);

        let w = WINDOW_WIDTH;
        let h = WINDOW_HEIGHT;

        // Header Background
        fill_rect(buffer, w, h, 0, 0, w, 60, COLOR_SURFACE_ALT);
        fill_rect(buffer, w, h, 0, 59, w, 1, COLOR_BORDER);

        // App Logo & Name
        draw_text(buffer, w, h, 30, 20, "DESKSTREAM", COLOR_TEXT_MAIN, 2);
        draw_text(buffer, w, h, 185, 26, "PRO 120 FPS", COLOR_ACCENT_CYAN, 1);

        // Mode Switcher Pills in Header
        let is_server = config.is_server_mode();
        let client_bg = if !is_server { COLOR_PRIMARY } else { COLOR_SURFACE };
        let server_bg = if is_server { COLOR_PRIMARY } else { COLOR_SURFACE };
        let client_border = if !is_server { COLOR_BORDER_ACTIVE } else { COLOR_BORDER };
        let server_border = if is_server { COLOR_BORDER_ACTIVE } else { COLOR_BORDER };

        fill_rounded_rect(buffer, w, h, 560, 16, 110, 30, 4, client_bg);
        draw_rect_outline(buffer, w, h, 560, 16, 110, 30, client_border);
        draw_text(buffer, w, h, 580, 25, "Client Mode", COLOR_TEXT_MAIN, 1);

        fill_rounded_rect(buffer, w, h, 680, 16, 110, 30, 4, server_bg);
        draw_rect_outline(buffer, w, h, 680, 16, 110, 30, server_border);
        draw_text(buffer, w, h, 700, 25, "Server Mode", COLOR_TEXT_MAIN, 1);

        // Status Badge Pill
        let (status_text, status_color) = if is_server {
            if relay_running {
                ("● SERVER READY (Port 9001)", COLOR_SUCCESS)
            } else {
                ("● SERVER STARTING...", COLOR_WARNING)
            }
        } else {
            match conn_status {
                ConnectionStatus::Starting => ("● STARTING...", 0x3B82F6),
                ConnectionStatus::Connecting => ("● CONNECTING...", 0xF59E0B),
                ConnectionStatus::Authenticating => ("● AUTHENTICATING...", 0xA855F7),
                ConnectionStatus::Online => ("● ONLINE", COLOR_SUCCESS),
                ConnectionStatus::Disconnected => ("● DISCONNECTED", COLOR_DANGER),
            }
        };
        fill_rounded_rect(buffer, w, h, 320, 18, 210, 26, 13, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 320, 18, 210, 26, status_color);
        draw_text(buffer, w, h, 335, 25, status_text, status_color, 1);

        // Tab Bar
        let tab_names = ["Dashboard", "Relay Server", "Settings", "Live Logs"];
        let tab_x_positions = [30, 180, 330, 480];

        for (idx, name) in tab_names.iter().enumerate() {
            let tx = tab_x_positions[idx];
            let is_active = (self.current_tab as usize) == idx;
            let tab_color = if is_active { COLOR_PRIMARY } else { COLOR_SURFACE };
            let border_col = if is_active { COLOR_BORDER_ACTIVE } else { COLOR_BORDER };
            let text_col = if is_active { COLOR_TEXT_MAIN } else { COLOR_TEXT_MUTED };

            fill_rounded_rect(buffer, w, h, tx, 70, 140, 34, 4, tab_color);
            draw_rect_outline(buffer, w, h, tx, 70, 140, 34, border_col);
            let tw = text_width(name, 1);
            let center_x = tx + (140 - tw) / 2;
            draw_text(buffer, w, h, center_x, 82, name, text_col, 1);
        }

        // Render Active Tab Content
        match self.current_tab {
            Tab::Dashboard => self.render_dashboard(buffer, config, conn_status, last_conn_error),
            Tab::RelayServer => self.render_relay_server(buffer, config, relay_running, relay_stats),
            Tab::Settings => self.render_settings(buffer, config),
            Tab::Logs => self.render_logs(buffer, logs),
        }

        // Footer Bar
        fill_rect(buffer, w, h, 0, 585, w, 35, COLOR_SURFACE_ALT);
        fill_rect(buffer, w, h, 0, 584, w, 1, COLOR_BORDER);
        draw_text(buffer, w, h, 30, 595, "DeskStream v2.0 • Ultra-Low Latency Hardware H.264 (120 FPS)", COLOR_TEXT_DIM, 1);
        draw_text(buffer, w, h, 590, 595, &format!("Relay: {}", config.relay_addr), COLOR_TEXT_MUTED, 1);
    }

    fn render_dashboard(&self, buffer: &mut [u32], config: &AgentConfig, conn_status: ConnectionStatus, last_error: &str) {
        let w = WINDOW_WIDTH;
        let h = WINDOW_HEIGHT;

        // Card 1: Your Device / System ID
        fill_rounded_rect(buffer, w, h, 30, 125, 360, 430, 6, COLOR_SURFACE);
        draw_rect_outline(buffer, w, h, 30, 125, 360, 430, COLOR_BORDER);

        draw_text(buffer, w, h, 50, 145, "YOUR SYSTEM ID", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 50, 165, "Share this ID to allow remote access", COLOR_TEXT_DIM, 1);

        // Big System ID Box
        fill_rounded_rect(buffer, w, h, 50, 195, 320, 85, 4, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 50, 195, 320, 85, COLOR_BORDER_ACTIVE);

        let id_display = config.formatted_id();
        draw_text(buffer, w, h, 65, 215, &id_display, COLOR_TEXT_MAIN, 2);

        // Copy Button
        let copy_btn_text = if let Some(t) = self.copy_feedback_timer {
            if t.elapsed().as_secs() < 2 { "Copied!" } else { "Copy ID" }
        } else {
            "Copy ID"
        };
        let copy_btn_col = if copy_btn_text == "Copied!" { COLOR_SUCCESS } else { COLOR_PRIMARY };
        fill_rounded_rect(buffer, w, h, 270, 205, 90, 30, 3, copy_btn_col);
        draw_text(buffer, w, h, 285, 214, copy_btn_text, COLOR_TEXT_MAIN, 1);

        // Device Info Rows
        draw_text(buffer, w, h, 50, 300, "Device Name:", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 160, 300, &config.name, COLOR_TEXT_MAIN, 1);

        draw_text(buffer, w, h, 50, 328, "Security PIN:", COLOR_TEXT_MUTED, 1);
        let pin_str = if config.pin.is_empty() { "(None / Default)" } else { &config.pin };
        draw_text(buffer, w, h, 160, 328, pin_str, COLOR_TEXT_MAIN, 1);

        draw_text(buffer, w, h, 50, 356, "Relay Server:", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 160, 356, &config.relay_addr, COLOR_ACCENT_CYAN, 1);

        draw_text(buffer, w, h, 50, 384, "Status:", COLOR_TEXT_MUTED, 1);
        let (stat_lbl, stat_col) = match conn_status {
            ConnectionStatus::Starting => ("Starting...", 0x3B82F6),
            ConnectionStatus::Connecting => ("Connecting to Server...", 0xF59E0B),
            ConnectionStatus::Authenticating => ("Authenticating with Relay...", 0xA855F7),
            ConnectionStatus::Online => ("Online & Connected", COLOR_SUCCESS),
            ConnectionStatus::Disconnected => ("DISCONNECTED", COLOR_DANGER),
        };
        draw_text(buffer, w, h, 160, 384, stat_lbl, stat_col, 1);

        // Offline / Disconnected Banner with Error
        if conn_status == ConnectionStatus::Disconnected {
            fill_rounded_rect(buffer, w, h, 50, 412, 320, 36, 4, 0x450A0A);
            draw_rect_outline(buffer, w, h, 50, 412, 320, 36, COLOR_DANGER);
            let clean_err = if last_error.is_empty() { "Cannot reach server. Click to configure IP." } else { last_error };
            let display_err = if clean_err.len() > 36 { &clean_err[0..36] } else { clean_err };
            draw_text(buffer, w, h, 60, 418, "DISCONNECTED", COLOR_DANGER, 1);
            draw_text(buffer, w, h, 60, 432, display_err, COLOR_TEXT_MUTED, 1);
        }

        // GPU Acceleration Pill
        fill_rounded_rect(buffer, w, h, 50, 455, 320, 50, 4, COLOR_SURFACE_ALT);
        draw_rect_outline(buffer, w, h, 50, 455, 320, 50, COLOR_BORDER);
        draw_text(buffer, w, h, 65, 465, "Hardware GPU Encoder:", COLOR_ACCENT_CYAN, 1);
        draw_text(buffer, w, h, 65, 485, &format!("{} (Active)", self.gpu_backend_name), COLOR_SUCCESS, 1);

        // Card 2: Connect to Remote Desktop
        fill_rounded_rect(buffer, w, h, 420, 125, 370, 430, 6, COLOR_SURFACE);
        draw_rect_outline(buffer, w, h, 420, 125, 370, 430, COLOR_BORDER);

        draw_text(buffer, w, h, 440, 145, "CONNECT TO REMOTE DESKTOP", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 440, 165, "Enter partner System ID to start remote control", COLOR_TEXT_DIM, 1);

        // Partner ID Input
        draw_text(buffer, w, h, 440, 195, "Partner System ID:", COLOR_TEXT_MAIN, 1);
        let id_border = if self.active_field == 1 { COLOR_BORDER_ACTIVE } else { COLOR_BORDER };
        fill_rounded_rect(buffer, w, h, 440, 215, 330, 40, 4, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 440, 215, 330, 40, id_border);
        let target_txt = if self.input_partner_id.is_empty() {
            if self.active_field == 1 { "" } else { "e.g. 847 293 615" }
        } else {
            &self.input_partner_id
        };
        let target_col = if self.input_partner_id.is_empty() { COLOR_TEXT_DIM } else { COLOR_TEXT_MAIN };
        draw_text(buffer, w, h, 455, 228, target_txt, target_col, 1);

        // Partner PIN Input
        draw_text(buffer, w, h, 440, 275, "Partner PIN (Optional):", COLOR_TEXT_MAIN, 1);
        let pin_border = if self.active_field == 2 { COLOR_BORDER_ACTIVE } else { COLOR_BORDER };
        fill_rounded_rect(buffer, w, h, 440, 295, 330, 40, 4, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 440, 295, 330, 40, pin_border);
        let pin_txt = if self.input_partner_pin.is_empty() {
            if self.active_field == 2 { "" } else { "Enter PIN if required" }
        } else {
            &self.input_partner_pin
        };
        let p_col = if self.input_partner_pin.is_empty() { COLOR_TEXT_DIM } else { COLOR_TEXT_MAIN };
        draw_text(buffer, w, h, 455, 308, pin_txt, p_col, 1);

        // Connect Button
        fill_rounded_rect(buffer, w, h, 440, 355, 330, 48, 4, COLOR_PRIMARY);
        draw_text(buffer, w, h, 520, 370, "CONNECT REMOTE DESKTOP", COLOR_TEXT_MAIN, 1);

        // Features list
        draw_text(buffer, w, h, 440, 430, "• Ultra-Low Latency 120 FPS Video", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 440, 455, "• Seamless Mouse & Keyboard Control", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 440, 480, "• Direct Bi-directional Clipboard Sync", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 440, 505, "• Real-Time Microphone & Audio Sharing", COLOR_TEXT_MUTED, 1);
    }

    fn render_relay_server(&self, buffer: &mut [u32], config: &AgentConfig, running: bool, stats: &RelayStats) {
        let w = WINDOW_WIDTH;
        let h = WINDOW_HEIGHT;

        fill_rounded_rect(buffer, w, h, 30, 125, 760, 430, 6, COLOR_SURFACE);
        draw_rect_outline(buffer, w, h, 30, 125, 760, 430, COLOR_BORDER);

        draw_text(buffer, w, h, 50, 145, "CENTRALIZED RELAY SERVER (HOST MODE)", COLOR_TEXT_MAIN, 2);
        draw_text(buffer, w, h, 50, 175, "Manages incoming client connections, WebSockets, and stream routing.", COLOR_TEXT_MUTED, 1);

        // Server Status Card
        fill_rounded_rect(buffer, w, h, 50, 205, 340, 130, 4, COLOR_SURFACE_ALT);
        draw_rect_outline(buffer, w, h, 50, 205, 340, 130, COLOR_BORDER);
        draw_text(buffer, w, h, 70, 220, "Relay Server Status:", COLOR_TEXT_MUTED, 1);
        let s_text = if running { "ACTIVE & LISTENING" } else { "STOPPED" };
        let s_color = if running { COLOR_SUCCESS } else { COLOR_DANGER };
        draw_text(buffer, w, h, 70, 240, s_text, s_color, 2);
        draw_text(buffer, w, h, 70, 275, &format!("Binding: {}", config.server_bind_addr), COLOR_TEXT_DIM, 1);

        let primary_lan_ip = self.lan_ips.first().cloned().unwrap_or_else(|| "127.0.0.1".to_string());
        draw_text(buffer, w, h, 70, 300, &format!("LAN IP: {}:9001", primary_lan_ip), COLOR_ACCENT_CYAN, 1);

        // Stats Card
        fill_rounded_rect(buffer, w, h, 420, 205, 340, 130, 4, COLOR_SURFACE_ALT);
        draw_rect_outline(buffer, w, h, 420, 205, 340, 130, COLOR_BORDER);
        draw_text(buffer, w, h, 440, 225, "Active Connected Hosts:", COLOR_TEXT_MUTED, 1);
        let hosts_cnt = stats.active_hosts.load(Ordering::Relaxed);
        draw_text(buffer, w, h, 650, 225, &format!("{}", hosts_cnt), COLOR_TEXT_MAIN, 1);

        draw_text(buffer, w, h, 440, 255, "Active Remote Sessions:", COLOR_TEXT_MUTED, 1);
        let sess_cnt = stats.active_sessions.load(Ordering::Relaxed);
        draw_text(buffer, w, h, 650, 255, &format!("{}", sess_cnt), COLOR_TEXT_MAIN, 1);

        draw_text(buffer, w, h, 440, 285, "Routed Video Frames:", COLOR_TEXT_MUTED, 1);
        let frames_cnt = stats.total_video_frames.load(Ordering::Relaxed);
        draw_text(buffer, w, h, 650, 285, &format!("{}", frames_cnt), COLOR_ACCENT_CYAN, 1);

        // Copy Server IP button
        let copy_serv_txt = if let Some(t) = self.copy_server_ip_timer {
            if t.elapsed().as_secs() < 2 { "Address Copied!" } else { "Copy Server Address" }
        } else {
            "Copy Server Address"
        };
        fill_rounded_rect(buffer, w, h, 600, 305, 140, 26, 3, COLOR_PRIMARY);
        draw_text(buffer, w, h, 610, 312, copy_serv_txt, COLOR_TEXT_MAIN, 1);

        // Info box
        fill_rounded_rect(buffer, w, h, 50, 355, 710, 160, 4, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 50, 355, 710, 160, COLOR_BORDER);
        draw_text(buffer, w, h, 70, 375, "HOW TO CONNECT CLIENT LAPTOPS (LAPTOP B):", COLOR_ACCENT_CYAN, 1);
        draw_text(buffer, w, h, 70, 405, &format!("1. Ensure both laptops are on the same Wi-Fi / LAN network."), COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 70, 430, &format!("2. On Laptop B, open DeskStream -> Settings tab."), COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 70, 455, &format!("3. Set Server Address to:  {}:9001  and click Save & Connect.", primary_lan_ip), COLOR_SUCCESS, 1);
        draw_text(buffer, w, h, 70, 480, "4. Laptop B will automatically connect and show ONLINE with its unique System ID.", COLOR_TEXT_MUTED, 1);
    }

    fn render_settings(&self, buffer: &mut [u32], config: &AgentConfig) {
        let w = WINDOW_WIDTH;
        let h = WINDOW_HEIGHT;

        fill_rounded_rect(buffer, w, h, 30, 125, 760, 430, 6, COLOR_SURFACE);
        draw_rect_outline(buffer, w, h, 30, 125, 760, 430, COLOR_BORDER);

        draw_text(buffer, w, h, 50, 145, "APPLICATION CONFIGURATION & NETWORK SETUP", COLOR_TEXT_MAIN, 2);
        draw_text(buffer, w, h, 50, 175, "Configure relay server address, test connection, and set password.", COLOR_TEXT_MUTED, 1);

        // Relay Address Input
        draw_text(buffer, w, h, 50, 215, "Relay Server Address (e.g. 192.168.x.x:9001):", COLOR_TEXT_MAIN, 1);
        let r_border = if self.active_field == 3 { COLOR_BORDER_ACTIVE } else { COLOR_BORDER };
        fill_rounded_rect(buffer, w, h, 50, 235, 420, 38, 4, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 50, 235, 420, 38, r_border);
        draw_text(buffer, w, h, 65, 248, &self.input_relay_addr, COLOR_TEXT_MAIN, 1);

        // Unattended Access PIN
        draw_text(buffer, w, h, 50, 295, "Unattended Access Security PIN (Optional):", COLOR_TEXT_MAIN, 1);
        let p_border = if self.active_field == 4 { COLOR_BORDER_ACTIVE } else { COLOR_BORDER };
        fill_rounded_rect(buffer, w, h, 50, 315, 420, 38, 4, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 50, 315, 420, 38, p_border);
        let pin_disp = if self.input_pin.is_empty() { "(Optional password for direct access)" } else { &self.input_pin };
        let pin_col = if self.input_pin.is_empty() { COLOR_TEXT_DIM } else { COLOR_TEXT_MAIN };
        draw_text(buffer, w, h, 65, 328, pin_disp, pin_col, 1);

        // Save Button
        let save_text = if let Some(t) = self.save_feedback_timer {
            if t.elapsed().as_secs() < 2 { "Saved & Connecting!" } else { "Save & Connect Now" }
        } else {
            "Save & Connect Now"
        };
        let save_bg = if save_text == "Saved & Connecting!" { COLOR_SUCCESS } else { COLOR_PRIMARY };
        fill_rounded_rect(buffer, w, h, 50, 375, 190, 42, 4, save_bg);
        draw_text(buffer, w, h, 65, 390, save_text, COLOR_TEXT_MAIN, 1);

        // Test Network Connection Button
        fill_rounded_rect(buffer, w, h, 260, 375, 210, 42, 4, COLOR_SURFACE_ALT);
        draw_rect_outline(buffer, w, h, 260, 375, 210, 42, COLOR_BORDER_ACTIVE);
        draw_text(buffer, w, h, 275, 390, "Test Network Connection", COLOR_TEXT_MAIN, 1);

        // Test Result Display
        if let Some((success, ref msg)) = self.network_test_result {
            let res_col = if success { COLOR_SUCCESS } else { COLOR_DANGER };
            let res_bg = if success { 0x064E3B } else { 0x7F1D1D };
            fill_rounded_rect(buffer, w, h, 50, 435, 420, 34, 4, res_bg);
            draw_rect_outline(buffer, w, h, 50, 435, 420, 34, res_col);
            let clean_msg = if msg.len() > 50 { &msg[0..50] } else { msg.as_str() };
            draw_text(buffer, w, h, 60, 446, clean_msg, COLOR_TEXT_MAIN, 1);
        }

        // Persistent Identity info
        fill_rounded_rect(buffer, w, h, 500, 215, 260, 220, 4, COLOR_SURFACE_ALT);
        draw_rect_outline(buffer, w, h, 500, 215, 260, 220, COLOR_BORDER);
        draw_text(buffer, w, h, 515, 230, "PERSISTENT IDENTITY", COLOR_ACCENT_CYAN, 1);
        draw_text(buffer, w, h, 515, 255, "Hardware UUID:", COLOR_TEXT_MUTED, 1);
        let short_uuid = if config.device_uuid.len() > 16 { &config.device_uuid[0..16] } else { &config.device_uuid };
        draw_text(buffer, w, h, 515, 275, &format!("{}...", short_uuid), COLOR_TEXT_MAIN, 1);

        draw_text(buffer, w, h, 515, 305, "Assigned System ID:", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 515, 325, &config.formatted_id(), COLOR_SUCCESS, 1);

        draw_text(buffer, w, h, 515, 355, "Application Mode:", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 515, 375, &config.app_mode.to_uppercase(), COLOR_TEXT_MAIN, 1);

        draw_text(buffer, w, h, 515, 400, "Active Port:", COLOR_TEXT_MUTED, 1);
        draw_text(buffer, w, h, 610, 400, "9001 (TCP)", COLOR_TEXT_MAIN, 1);
    }

    fn render_logs(&self, buffer: &mut [u32], logs: &Arc<Mutex<VecDeque<String>>>) {
        let w = WINDOW_WIDTH;
        let h = WINDOW_HEIGHT;

        fill_rounded_rect(buffer, w, h, 30, 125, 760, 430, 6, COLOR_SURFACE);
        draw_rect_outline(buffer, w, h, 30, 125, 760, 430, COLOR_BORDER);

        draw_text(buffer, w, h, 50, 140, "LIVE SYSTEM & STREAM LOGS", COLOR_TEXT_MAIN, 1);

        // Terminal Log Viewer
        fill_rounded_rect(buffer, w, h, 50, 165, 720, 370, 4, COLOR_INPUT_BG);
        draw_rect_outline(buffer, w, h, 50, 165, 720, 370, COLOR_BORDER);

        if let Ok(guard) = logs.lock() {
            let max_lines = 21;
            let start = if guard.len() > max_lines { guard.len() - max_lines } else { 0 };

            for (idx, line) in guard.iter().skip(start).enumerate() {
                let y = 175 + idx * 16;
                let col = if line.contains("[SERVER]") {
                    COLOR_ACCENT_CYAN
                } else if line.contains("[AUTH]") || line.contains("[SESSION]") {
                    COLOR_SUCCESS
                } else if line.contains("[H264") || line.contains("[STREAM]") {
                    0x38BDF8 // Light Sky Blue
                } else if line.contains("ERROR") || line.contains("FAILED") {
                    COLOR_DANGER
                } else {
                    COLOR_TEXT_MUTED
                };
                let clean = if line.len() > 85 { &line[0..85] } else { line.as_str() };
                draw_text(buffer, w, h, 65, y, clean, col, 1);
            }
        }
    }
}

fn is_in_rect(mx: usize, my: usize, rx: usize, ry: usize, rw: usize, rh: usize) -> bool {
    mx >= rx && mx <= rx + rw && my >= ry && my <= ry + rh
}

fn fill_rect(buffer: &mut [u32], buf_w: usize, buf_h: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for row in 0..h {
        let py = y + row;
        if py >= buf_h { break; }
        for col in 0..w {
            let px = x + col;
            if px < buf_w {
                buffer[py * buf_w + px] = color;
            }
        }
    }
}

fn draw_rect_outline(buffer: &mut [u32], buf_w: usize, buf_h: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    if w == 0 || h == 0 { return; }
    for col in 0..w {
        if x + col < buf_w {
            if y < buf_h { buffer[y * buf_w + (x + col)] = color; }
            if y + h - 1 < buf_h { buffer[(y + h - 1) * buf_w + (x + col)] = color; }
        }
    }
    for row in 0..h {
        if y + row < buf_h {
            if x < buf_w { buffer[(y + row) * buf_w + x] = color; }
            if x + w - 1 < buf_w { buffer[(y + row) * buf_w + (x + w - 1)] = color; }
        }
    }
}

fn fill_rounded_rect(buffer: &mut [u32], buf_w: usize, buf_h: usize, x: usize, y: usize, w: usize, h: usize, radius: usize, color: u32) {
    let r = radius.min(w / 2).min(h / 2);
    fill_rect(buffer, buf_w, buf_h, x + r, y, w - 2 * r, h, color);
    fill_rect(buffer, buf_w, buf_h, x, y + r, r, h - 2 * r, color);
    fill_rect(buffer, buf_w, buf_h, x + w - r, y + r, r, h - 2 * r, color);
}

fn key_to_char(key: Key) -> Option<char> {
    match key {
        Key::Key0 | Key::NumPad0 => Some('0'),
        Key::Key1 | Key::NumPad1 => Some('1'),
        Key::Key2 | Key::NumPad2 => Some('2'),
        Key::Key3 | Key::NumPad3 => Some('3'),
        Key::Key4 | Key::NumPad4 => Some('4'),
        Key::Key5 | Key::NumPad5 => Some('5'),
        Key::Key6 | Key::NumPad6 => Some('6'),
        Key::Key7 | Key::NumPad7 => Some('7'),
        Key::Key8 | Key::NumPad8 => Some('8'),
        Key::Key9 | Key::NumPad9 => Some('9'),
        Key::A => Some('a'),
        Key::B => Some('b'),
        Key::C => Some('c'),
        Key::D => Some('d'),
        Key::E => Some('e'),
        Key::F => Some('f'),
        Key::G => Some('g'),
        Key::H => Some('h'),
        Key::I => Some('i'),
        Key::J => Some('j'),
        Key::K => Some('k'),
        Key::L => Some('l'),
        Key::M => Some('m'),
        Key::N => Some('n'),
        Key::O => Some('o'),
        Key::P => Some('p'),
        Key::Q => Some('q'),
        Key::R => Some('r'),
        Key::S => Some('s'),
        Key::T => Some('t'),
        Key::U => Some('u'),
        Key::V => Some('v'),
        Key::W => Some('w'),
        Key::X => Some('x'),
        Key::Y => Some('y'),
        Key::Z => Some('z'),
        Key::Period | Key::NumPadDot => Some('.'),
        Key::Minus | Key::NumPadMinus => Some('-'),
        Key::Space => Some(' '),
        _ => None,
    }
}
