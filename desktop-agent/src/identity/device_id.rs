use rand::Rng;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentConfig {
    pub device_uuid: String,
    pub system_id: String,
    pub name: String,
    pub relay_addr: String,
    #[serde(default = "default_app_mode")]
    pub app_mode: String, // "client" or "server"
    #[serde(default = "default_relay_port")]
    pub relay_port: u16,
    #[serde(default = "default_server_bind_addr")]
    pub server_bind_addr: String,
    pub pin: String,
}

fn default_app_mode() -> String {
    "client".to_string()
}

fn default_relay_port() -> u16 {
    9001
}

fn default_server_bind_addr() -> String {
    "0.0.0.0:9001".to_string()
}

impl AgentConfig {
    pub fn is_server_mode(&self) -> bool {
        self.app_mode.to_lowercase() == "server"
    }

    pub fn load_or_create(_unused_config_path: &str, default_relay: &str) -> Self {
        // 1. Read this machine's real hardware UUID from Windows MachineGuid registry key.
        //    This is the ONLY authoritative source of physical machine identity.
        let hardware_uuid = get_permanent_machine_uuid();
        let hostname = env::var("COMPUTERNAME")
            .or_else(|_| env::var("HOSTNAME"))
            .unwrap_or_else(|_| "Desktop-PC".to_string());

        // Deterministic 9-digit ID derived from hardware UUID + hostname
        let combined_hardware = format!("{}-{}", hardware_uuid, hostname);
        let deterministic_id = generate_deterministic_system_id(&combined_hardware);

        // Support forced identity reset via environment variable (repair second laptop)
        let force_reset = env::var("AGENT_RESET_IDENTITY").map(|v| v == "1").unwrap_or(false);

        // 2. Load existing config if present and not force-resetting
        if !force_reset {
            if let Some(canonical_path) = Self::canonical_config_path() {
                if let Some(mut cfg) = Self::try_read_config(canonical_path.to_str().unwrap_or_default()) {
                    let mut dirty = false;

                    // --- CLONED / CORRUPTED CONFIG DETECTION ---
                    // If the stored device_uuid does not match THIS machine's actual hardware UUID,
                    // the config was cloned from another machine, manually overwritten (e.g. by a
                    // repair script), or copied during development. In that case we MUST regenerate
                    // system_id so each physical laptop has its own unique identity.
                    let uuid_mismatch = !cfg.device_uuid.is_empty() && cfg.device_uuid != hardware_uuid;
                    if uuid_mismatch {
                        eprintln!("[IDENTITY] *** MISMATCH: stored device_uuid ({}) != hardware MachineGuid ({}).", cfg.device_uuid, hardware_uuid);
                        eprintln!("[IDENTITY] *** Config was cloned or manually overwritten. Regenerating identity for this hardware.");
                        cfg.device_uuid = hardware_uuid.clone();
                        cfg.system_id = deterministic_id.clone();
                        dirty = true;
                    }

                    if cfg.device_uuid.is_empty() {
                        cfg.device_uuid = hardware_uuid.clone();
                        dirty = true;
                    }
                    if cfg.system_id.is_empty() {
                        cfg.system_id = generate_deterministic_system_id(&format!("{}-{}", cfg.device_uuid, hostname));
                        dirty = true;
                    }
                    if cfg.relay_addr.is_empty() || cfg.relay_addr == "127.0.0.1:9001" {
                        cfg.relay_addr = default_relay.to_string();
                        dirty = true;
                    }
                    if cfg.app_mode.is_empty() {
                        cfg.app_mode = "client".to_string();
                        dirty = true;
                    }
                    if cfg.relay_port == 0 {
                        cfg.relay_port = 9001;
                        dirty = true;
                    }
                    if cfg.server_bind_addr.is_empty() {
                        cfg.server_bind_addr = "0.0.0.0:9001".to_string();
                        dirty = true;
                    }
                    if dirty {
                        let _ = cfg.save("");
                    }
                    println!("[IDENTITY] Loading persistent device identity...");
                    println!("[IDENTITY] Device ID:   {}", cfg.system_id);
                    println!("[IDENTITY] MachineGuid: {}", cfg.device_uuid);
                    println!("[IDENTITY] Hostname:    {}", hostname);
                    println!("[IDENTITY] Identity source: {}", if uuid_mismatch { "regenerated_hardware_mismatch" } else { "persisted" });
                    return cfg;
                }
            }
        } else {
            eprintln!("[IDENTITY] AGENT_RESET_IDENTITY=1 — forcing identity regeneration from hardware.");
        }

        // 3. First launch on this machine (or forced reset)
        println!("[IDENTITY] Generating new device identity from hardware...");
        println!("[IDENTITY] Generated Device ID: {}", deterministic_id);
        println!("[IDENTITY] MachineGuid: {}", hardware_uuid);
        println!("[IDENTITY] Hostname:    {}", hostname);
        println!("[IDENTITY] Identity source: generated_from_hardware");

        let config = AgentConfig {
            device_uuid: hardware_uuid,
            system_id: deterministic_id,
            name: hostname,
            relay_addr: default_relay.to_string(),
            app_mode: "client".to_string(),
            relay_port: 9001,
            server_bind_addr: "0.0.0.0:9001".to_string(),
            pin: String::new(),
        };

        let _ = config.save("");
        println!("[IDENTITY] Persisted Device ID: {}", config.system_id);
        config
    }

    fn try_read_config(path_str: &str) -> Option<Self> {
        if path_str.is_empty() || !Path::new(path_str).exists() {
            return None;
        }
        let mut file = File::open(path_str).ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;
        serde_json::from_str::<AgentConfig>(&contents).ok()
    }

    fn canonical_config_path() -> Option<PathBuf> {
        let base_dir = env::var("LOCALAPPDATA")
            .or_else(|_| env::var("APPDATA"))
            .or_else(|_| env::var("USERPROFILE"))
            .ok()?;
        let deskstream_dir = Path::new(&base_dir).join("DeskStream");
        let _ = fs::create_dir_all(&deskstream_dir);
        Some(deskstream_dir.join("agent_config.json"))
    }

    /// Saves config strictly to canonical appdata directory.
    pub fn save(&self, _path: &str) -> bool {
        let mut success = false;
        if let Ok(json_str) = serde_json::to_string_pretty(self) {
            if let Some(canonical_path) = Self::canonical_config_path() {
                if fs::write(canonical_path, &json_str).is_ok() {
                    success = true;
                }
            }
        }
        success
    }

    /// Formats a 9-digit System ID as "847 293 615"
    pub fn formatted_id(&self) -> String {
        let clean: String = self.system_id.chars().filter(|c| c.is_ascii_digit()).collect();
        if clean.len() == 9 {
            format!("{} {} {}", &clean[0..3], &clean[3..6], &clean[6..9])
        } else if clean.len() == 6 {
            format!("{} {}", &clean[0..3], &clean[3..6])
        } else if clean.is_empty() {
            "Connecting...".to_string()
        } else {
            clean
        }
    }
}

/// Retrieves permanent hardware Machine UUID from OS or generates RFC4122 UUID v4
fn get_permanent_machine_uuid() -> String {
    #[cfg(windows)]
    {
        if let Some(guid) = get_windows_machine_guid() {
            return guid;
        }
    }
    generate_uuid_v4()
}

#[cfg(windows)]
fn get_windows_machine_guid() -> Option<String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY,
    };

    let subkey: Vec<u16> = "SOFTWARE\\Microsoft\\Cryptography\0".encode_utf16().collect();
    let val_name: Vec<u16> = "MachineGuid\0".encode_utf16().collect();

    unsafe {
        let mut key = 0;
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut key,
        ) != 0
        {
            return None;
        }

        let mut buf_type = 0;
        let mut buf_size: u32 = 256;
        let mut buf = vec![0u8; 256];

        let res = RegQueryValueExW(
            key,
            val_name.as_ptr(),
            std::ptr::null_mut(),
            &mut buf_type,
            buf.as_mut_ptr(),
            &mut buf_size,
        );
        RegCloseKey(key);

        if res == 0 && buf_size > 0 {
            let u16_slice =
                std::slice::from_raw_parts(buf.as_ptr() as *const u16, (buf_size as usize) / 2);
            let guid_str = String::from_utf16_lossy(u16_slice)
                .trim_matches('\0')
                .trim()
                .to_string();
            if !guid_str.is_empty() {
                return Some(guid_str);
            }
        }
    }
    None
}

/// Generates a standard random UUID v4 string
fn generate_uuid_v4() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);

    // Set UUID version 4
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    // Set variant to RFC4122
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

pub fn generate_deterministic_system_id(device_uuid: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(device_uuid.as_bytes());
    let hash = hasher.finalize();
    let num = (u32::from(hash[0]) << 24)
        | (u32::from(hash[1]) << 16)
        | (u32::from(hash[2]) << 8)
        | u32::from(hash[3]);
    let id_9digit = (num % 900_000_000) + 100_000_000;
    id_9digit.to_string()
}