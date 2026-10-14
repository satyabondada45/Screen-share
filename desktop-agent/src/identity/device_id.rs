use rand::Rng;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ServerConfig {
    pub backend_url: String,
    pub relay_url: String,
    pub server_addr: String,
}

/// Reads /Program Files/Screen Share/server-config.json (written by the installer)
/// to discover the production server address. Returns None if the file is not found
/// or is malformed.
pub fn read_server_config() -> Option<ServerConfig> {
    let pf = env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
    let path = Path::new(&pf).join("Screen Share").join("server-config.json");
    let contents = fs::read_to_string(&path).ok()?;
    serde_json::from_str::<ServerConfig>(&contents).ok()
}

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
    #[serde(default)]
    pub backend_url: String,
    #[serde(default)]
    pub environment: String,
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

const LEGACY_RELAY_ADDR: &str = "34.229.20.54:9001";
const CURRENT_RELAY_ADDR: &str = "34.207.168.24:9001";
pub const DEFAULT_BACKEND_URL: &str = "https://friendssoftwaresolutions.in/DeskStream/api";
pub const DEFAULT_PIN: &str = "123456";

pub fn resolve_backend_url(
    saved_backend: Option<&str>,
    env_backend_url: Option<&str>,
) -> String {
    if let Some(env_url) = env_backend_url {
        let trimmed = env_url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    match saved_backend {
        Some(url)
            if url.trim().is_empty()
                || url.contains("/Screen%20Share/backend/api")
                || url.contains("34.207.168.24")
                || url.contains("34.229.20.54") =>
        {
            DEFAULT_BACKEND_URL.to_string()
        }
        Some(url) => url.trim().to_string(),
        None => DEFAULT_BACKEND_URL.to_string(),
    }
}

fn resolve_relay_addr(
    saved_relay: Option<&str>,
    env_relay_addr: Option<&str>,
    env_relay_url: Option<&str>,
    default_relay: &str,
) -> String {
    if let Some(relay_addr) = env_relay_addr {
        return relay_addr.to_string();
    }
    if let Some(relay_url) = env_relay_url {
        return relay_url.to_string();
    }

    // A GUI/installer may pass relay_url as the default; migrate the known stale
    // installer value without overriding explicit environment or saved settings.
    let default_relay = if default_relay == LEGACY_RELAY_ADDR {
        CURRENT_RELAY_ADDR
    } else {
        default_relay
    };

    match saved_relay {
        Some(relay_addr) if relay_addr == LEGACY_RELAY_ADDR || relay_addr.is_empty() => {
            default_relay.to_string()
        }
        Some(relay_addr) => relay_addr.to_string(),
        None => default_relay.to_string(),
    }
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

        // Read environment variables for production configuration
        let env_relay_url = env::var("SCREENSHARE_RELAY_URL").ok();
        let env_backend_url = env::var("SCREENSHARE_BACKEND_URL").ok();
        let env_relay_addr = env::var("SCREENSHARE_RELAY_ADDR").ok();

        let resolved_relay = resolve_relay_addr(
            None,
            env_relay_addr.as_deref(),
            env_relay_url.as_deref(),
            default_relay,
        );

        // Support forced identity reset via environment variable
        let force_reset = env::var("AGENT_RESET_IDENTITY").map(|v| v == "1").unwrap_or(false);

        // 2. Load existing config if present and not force-resetting
        if !force_reset {
            if let Some(canonical_path) = Self::canonical_config_path() {
                if let Some(mut cfg) = Self::try_read_config(canonical_path.to_str().unwrap_or_default()) {
                    let mut dirty = false;

                    // Report a copied configuration, but never silently replace its
                    // persisted identity. The configured system_id is part of the
                    // device identity used by the viewer and relay.
                    let uuid_mismatch = !cfg.device_uuid.is_empty() && cfg.device_uuid != hardware_uuid;
                    if uuid_mismatch {
                        eprintln!("[IDENTITY] *** MISMATCH: stored device_uuid ({}) != hardware MachineGuid ({}).", cfg.device_uuid, hardware_uuid);
                        eprintln!("[IDENTITY] *** Preserving the configured device identity; no automatic reset was performed.");
                    }

                    if cfg.device_uuid.is_empty() {
                        cfg.device_uuid = hardware_uuid.clone();
                        dirty = true;
                    }
                    if cfg.system_id.is_empty() {
                        cfg.system_id = generate_deterministic_system_id(&format!("{}-{}", cfg.device_uuid, hostname));
                        dirty = true;
                    }

                    // Apply overrides and migrate only the known obsolete production relay.
                    let configured_relay = resolve_relay_addr(
                        Some(&cfg.relay_addr),
                        env_relay_addr.as_deref(),
                        env_relay_url.as_deref(),
                        default_relay,
                    );
                    if cfg.relay_addr != configured_relay {
                        cfg.relay_addr = configured_relay;
                        dirty = true;
                    }

                    // Apply env override or resolve backend URL
                    let configured_backend = resolve_backend_url(
                        Some(&cfg.backend_url),
                        env_backend_url.as_deref(),
                    );
                    if cfg.backend_url != configured_backend {
                        cfg.backend_url = configured_backend;
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
                    if cfg.environment.is_empty() {
                        cfg.environment = if env_relay_url.is_some() && !env_relay_url.as_deref().unwrap_or("").contains("localhost") {
                            "Production".to_string()
                        } else {
                            "Development".to_string()
                        };
                        dirty = true;
                    }
                    if dirty {
                        let _ = cfg.save("");
                    }
                    println!("[IDENTITY] Loading persistent device identity...");
                    println!("[IDENTITY] Device ID:   {}", cfg.system_id);
                    println!("[IDENTITY] MachineGuid: {}", cfg.device_uuid);
                    println!("[IDENTITY] Hostname:    {}", hostname);
                    println!("[IDENTITY] Relay Addr:  {}", cfg.relay_addr);
                    println!("[IDENTITY] Backend URL: {}", cfg.backend_url);
                    println!("[IDENTITY] Environment:  {}", cfg.environment);
                    println!("[IDENTITY] Identity source: {}", if uuid_mismatch { "regenerated_hardware_mismatch" } else { "persisted" });
                    return cfg;
                }
            }
        } else {
            eprintln!("[IDENTITY] AGENT_RESET_IDENTITY=1 — forcing identity regeneration from hardware.");
        }

        // Resolve relay address for new config
        let config_relay = resolved_relay;

        // Resolve backend URL for new config
        let config_backend = resolve_backend_url(
            None,
            env_backend_url.as_deref(),
        );

        // Determine environment
        let config_env = if env_relay_url.is_some() && !env_relay_url.as_deref().unwrap_or("").contains("localhost") {
            "Production".to_string()
        } else {
            "Development".to_string()
        };

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
            relay_addr: config_relay,
            app_mode: "client".to_string(),
            relay_port: 9001,
            server_bind_addr: "0.0.0.0:9001".to_string(),
            backend_url: config_backend,
            environment: config_env,
            pin: String::new(),
        };

        let _ = config.save("");
        println!("[IDENTITY] Persisted Device ID: {}", config.system_id);
        config
    }

    pub fn try_read_config(path_str: &str) -> Option<Self> {
        if path_str.is_empty() || !Path::new(path_str).exists() {
            return None;
        }
        let mut file = File::open(path_str).ok()?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).ok()?;
        serde_json::from_str::<AgentConfig>(&contents).ok()
    }

    pub fn canonical_config_path() -> Option<PathBuf> {
        let base_dir = env::var("LOCALAPPDATA")
            .or_else(|_| env::var("APPDATA"))
            .or_else(|_| env::var("USERPROFILE"))
            .ok()?;
        let deskstream_dir = Path::new(&base_dir).join("DeskStream");
        let _ = fs::create_dir_all(&deskstream_dir);
        Some(deskstream_dir.join("agent_config.json"))
    }

    pub fn load_current() -> Option<Self> {
        Self::canonical_config_path().and_then(|p| Self::try_read_config(p.to_str().unwrap_or_default()))
    }

    /// Resolves the effective PIN according to precedence:
    /// 1. SCREENSHARE_DEVICE_PIN environment variable (if non-empty)
    /// 2. Latest saved PIN from canonical agent_config.json
    /// 3. In-memory self.pin
    pub fn get_effective_pin(&self) -> String {
        Self::resolve_pin(
            env::var("SCREENSHARE_DEVICE_PIN").ok().as_deref(),
            Self::load_current().as_ref().map(|c| c.pin.as_str()),
            &self.pin,
        )
    }

    /// Helper for resolving PIN precedence deterministically.
    pub fn resolve_pin(env_pin: Option<&str>, saved_pin: Option<&str>, memory_pin: &str) -> String {
        if let Some(env_p) = env_pin {
            let trimmed = env_p.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        if let Some(saved_p) = saved_pin {
            let trimmed = saved_p.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        let mem_trimmed = memory_pin.trim();
        if !mem_trimmed.is_empty() {
            return mem_trimmed.to_string();
        }
        DEFAULT_PIN.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_install_uses_source_default() {
        assert_eq!(
            resolve_relay_addr(None, None, None, CURRENT_RELAY_ADDR),
            CURRENT_RELAY_ADDR
        );
    }

    #[test]
    fn command_line_relay_is_used_as_the_default() {
        let command_line_relay = "relay.example.com:9001";
        assert_eq!(
            resolve_relay_addr(None, None, None, command_line_relay),
            command_line_relay
        );
    }

    #[test]
    fn known_legacy_saved_relay_is_migrated() {
        assert_eq!(
            resolve_relay_addr(
                Some("34.229.20.54:9001"),
                None,
                None,
                CURRENT_RELAY_ADDR
            ),
            CURRENT_RELAY_ADDR
        );
    }

    #[test]
    fn stale_installer_relay_default_is_migrated() {
        assert_eq!(
            resolve_relay_addr(None, None, None, LEGACY_RELAY_ADDR),
            CURRENT_RELAY_ADDR
        );
    }

    #[test]
    fn stale_installer_default_does_not_restore_legacy_over_saved_current_relay() {
        assert_eq!(
            resolve_relay_addr(
                Some(CURRENT_RELAY_ADDR),
                None,
                None,
                LEGACY_RELAY_ADDR
            ),
            CURRENT_RELAY_ADDR
        );
    }

    #[test]
    fn explicit_environment_override_is_preserved_even_if_it_is_legacy() {
        assert_eq!(
            resolve_relay_addr(
                None,
                Some(LEGACY_RELAY_ADDR),
                None,
                CURRENT_RELAY_ADDR
            ),
            LEGACY_RELAY_ADDR
        );
    }

    #[test]
    fn explicitly_saved_nonlegacy_relay_is_preserved() {
        let custom_relay = "relay.example.com:9001";
        assert_eq!(
            resolve_relay_addr(
                Some(custom_relay),
                None,
                None,
                CURRENT_RELAY_ADDR
            ),
            custom_relay
        );
    }

    #[test]
    fn environment_relay_addr_overrides_saved_relay() {
        let configured_relay = "relay.example.com:9001";
        assert_eq!(
            resolve_relay_addr(
                Some("34.229.20.54:9001"),
                Some(configured_relay),
                None,
                CURRENT_RELAY_ADDR
            ),
            configured_relay
        );
    }

    #[test]
    fn environment_relay_url_overrides_saved_relay_when_addr_is_absent() {
        let configured_relay = "relay.example.com:9001";
        assert_eq!(
            resolve_relay_addr(
                Some("34.229.20.54:9001"),
                None,
                Some(configured_relay),
                CURRENT_RELAY_ADDR
            ),
            configured_relay
        );
    }

    #[test]
    fn backend_url_defaults_to_production_hostinger_endpoint() {
        assert_eq!(
            resolve_backend_url(None, None),
            DEFAULT_BACKEND_URL
        );
        assert_eq!(
            resolve_backend_url(Some(""), None),
            DEFAULT_BACKEND_URL
        );
    }

    #[test]
    fn backend_url_migrates_legacy_and_relay_ip_paths() {
        assert_eq!(
            resolve_backend_url(Some("http://34.207.168.24/Screen%20Share/backend/api"), None),
            DEFAULT_BACKEND_URL
        );
        assert_eq!(
            resolve_backend_url(Some("http://127.0.0.1/Screen%20Share/backend/api"), None),
            DEFAULT_BACKEND_URL
        );
    }

    #[test]
    fn backend_url_preserves_custom_and_respects_env_override() {
        let custom = "https://custom.deskstream.io/api";
        assert_eq!(
            resolve_backend_url(Some(custom), None),
            custom
        );
        let env_override = "https://staging.deskstream.io/api";
        assert_eq!(
            resolve_backend_url(Some(custom), Some(env_override)),
            env_override
        );
    }

    #[test]
    fn pin_resolution_precedence() {
        // 1. Env var takes precedence over saved and in-memory
        assert_eq!(
            AgentConfig::resolve_pin(Some("123456"), Some("654321"), "999999"),
            "123456"
        );
        // 2. Whitespace env var falls back to saved
        assert_eq!(
            AgentConfig::resolve_pin(Some("   "), Some("654321"), "999999"),
            "654321"
        );
        // 3. Unset env var falls back to saved
        assert_eq!(
            AgentConfig::resolve_pin(None, Some("654321"), "999999"),
            "654321"
        );
        // 4. Whitespace saved falls back to in-memory
        assert_eq!(
            AgentConfig::resolve_pin(None, Some("   "), "999999"),
            "999999"
        );
        // 5. Unset env and unset saved falls back to in-memory
        assert_eq!(
            AgentConfig::resolve_pin(None, None, "999999"),
            "999999"
        );
        // 6. If all are empty, falls back to DEFAULT_PIN
        assert_eq!(
            AgentConfig::resolve_pin(None, None, ""),
            DEFAULT_PIN
        );
        assert_eq!(
            AgentConfig::resolve_pin(None, None, "   "),
            DEFAULT_PIN
        );
    }
}