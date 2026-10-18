// desktop-agent/src/updater.rs
// DeskStream Secure In-App Auto-Update System
// Handles manifest retrieval, Ed25519 digital signature verification,
// SHA-256 validation, background downloading with progress, and staging.

use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_ID: &str = "com.friendssoftwaresolutions.deskstream";
pub const UPDATE_MANIFEST_URL: &str =
    "https://friendssoftwaresolutions.in/DeskStream/updates/stable.json";
pub const UPDATE_MANIFEST_FALLBACK_URL: &str =
    "https://friendssoftwaresolutions.in/DeskStream/api/updates/check.php";

// Public key corresponding to scripts/keys/deskstream_update_private.key
// Used to cryptographically authenticate release manifests
pub const UPDATE_PUBLIC_KEY_HEX: &str =
    "3f6d4aef873a79c323dd91f2a0f72f898ec0ac6b381162d90ddc1947fb9b8b31";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub url: String,
    pub filename: String,
    pub sha256: String,
    pub size: u64,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureInfo {
    pub algorithm: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub app_id: String,
    pub channel: String,
    pub version: String,
    pub min_supported_version: Option<String>,
    pub release_date: String,
    pub mandatory: bool,
    pub release_notes: String,
    pub package: PackageInfo,
    pub signature: SignatureInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub update_available: bool,
    pub current_version: String,
    pub new_version: Option<String>,
    pub mandatory: bool,
    pub release_notes: Option<String>,
    pub release_date: Option<String>,
    pub package_size: Option<u64>,
    pub package_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    Available,
    Downloading { progress: u32, received_bytes: u64, total_bytes: u64 },
    ReadyToInstall,
    Installing,
    Error { message: String },
}

#[derive(Default)]
pub struct UpdaterManager {
    pub state: Mutex<UpdateState>,
    pub cached_manifest: Mutex<Option<ReleaseManifest>>,
    pub is_downloading: AtomicBool,
    pub download_progress: AtomicU32,
    pub staged_exe_path: Mutex<Option<PathBuf>>,
}

// Thread-safe global updater state container
pub struct GlobalUpdater {
    inner: std::sync::OnceLock<Arc<UpdaterManager>>,
}

impl GlobalUpdater {
    pub const fn new() -> Self {
        Self {
            inner: std::sync::OnceLock::new(),
        }
    }

    pub fn get(&self) -> Arc<UpdaterManager> {
        self.inner
            .get_or_init(|| Arc::new(UpdaterManager::default()))
            .clone()
    }
}

pub static UPDATER: GlobalUpdater = GlobalUpdater::new();

/// Decodes a hexadecimal string into bytes.
pub fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let clean = s.trim();
    if clean.len() % 2 != 0 {
        return Err("Hex string has odd length".to_string());
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|e| format!("Invalid hex byte: {}", e))
        })
        .collect()
}

/// Verifies an Ed25519 signature over a payload using a public key in hex format.
pub fn verify_signature(
    public_key_hex: &str,
    payload: &[u8],
    signature_hex: &str,
) -> Result<bool, String> {
    let pub_bytes = hex_decode(public_key_hex)?;
    let sig_bytes = hex_decode(signature_hex)?;

    if pub_bytes.len() != 32 {
        return Err(format!(
            "Invalid public key length: expected 32 bytes, got {}",
            pub_bytes.len()
        ));
    }
    if sig_bytes.len() != 64 {
        return Err(format!(
            "Invalid signature length: expected 64 bytes, got {}",
            sig_bytes.len()
        ));
    }

    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&pub_bytes);

    let mut sig_array = [0u8; 64];
    sig_array.copy_from_slice(&sig_bytes);

    let verifying_key = VerifyingKey::from_bytes(&key_array)
        .map_err(|e| format!("Invalid verifying key: {}", e))?;
    let signature = Signature::from_bytes(&sig_array);

    use ed25519_dalek::Verifier;
    verifying_key
        .verify(payload, &signature)
        .map(|_| true)
        .map_err(|e| format!("Ed25519 verification failed: {}", e))
}

/// Computes the standard payload used for signing release packages:
/// `format!("{}:{}:{}", app_id, version, package_sha256)`
pub fn get_manifest_signing_payload(manifest: &ReleaseManifest) -> Vec<u8> {
    format!(
        "{}:{}:{}",
        manifest.app_id.trim(),
        manifest.version.trim(),
        manifest.package.sha256.trim().to_lowercase()
    )
    .into_bytes()
}

/// Returns the local updates directory (%LOCALAPPDATA%\DeskStream\updates)
pub fn get_updates_dir() -> PathBuf {
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        PathBuf::from(local_app_data)
            .join("DeskStream")
            .join("updates")
    } else {
        env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("updates")
    }
}

/// Returns the staging directory for verified updates
pub fn get_staged_dir() -> PathBuf {
    get_updates_dir().join("staged")
}

/// Computes the SHA-256 hash of a file on disk.
pub fn compute_file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("Could not open file: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Checks for updates against the production manifest.
pub fn check_for_updates_sync() -> Result<UpdateCheckResult, String> {
    let manager = UPDATER.get();
    {
        let mut st = manager.state.lock().unwrap();
        *st = UpdateState::Checking;
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client initialization failed: {}", e))?;

    let urls = [UPDATE_MANIFEST_URL, UPDATE_MANIFEST_FALLBACK_URL];
    let mut attempt_errors: Vec<String> = Vec::new();
    let mut manifest_opt: Option<ReleaseManifest> = None;

    for url in urls {
        match client
            .get(url)
            .header("User-Agent", format!("DeskStream/{}", CURRENT_VERSION))
            .header("Accept", "application/json")
            .send()
        {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    match resp.json::<ReleaseManifest>() {
                        Ok(m) => {
                            manifest_opt = Some(m);
                            break;
                        }
                        Err(e) => {
                            attempt_errors.push(format!("{}: Malformed manifest ({})", url, e));
                        }
                    }
                } else {
                    attempt_errors.push(format!("{}: HTTP {}", url, status));
                }
            }
            Err(e) => {
                attempt_errors.push(format!("{}: Network error ({})", url, e));
            }
        }
    }

    let manifest = match manifest_opt {
        Some(m) => m,
        None => {
            let error_details = if attempt_errors.is_empty() {
                "Unable to connect to update endpoints.".to_string()
            } else {
                format!("Update check failed: {}", attempt_errors.join("; "))
            };
            let mut st = manager.state.lock().unwrap();
            *st = UpdateState::Error {
                message: error_details.clone(),
            };
            return Err(error_details);
        }
    };

    // 1. Validate application identifier
    if manifest.app_id != APP_ID {
        let err_msg = format!("Mismatched app_id: {}", manifest.app_id);
        let mut st = manager.state.lock().unwrap();
        *st = UpdateState::Error {
            message: err_msg.clone(),
        };
        return Err(err_msg);
    }

    // 2. Validate release channel
    if manifest.channel.to_lowercase() != "stable" {
        let err_msg = format!("Unsupported release channel: {}", manifest.channel);
        let mut st = manager.state.lock().unwrap();
        *st = UpdateState::Error {
            message: err_msg.clone(),
        };
        return Err(err_msg);
    }

    // 3. Cryptographically verify release manifest signature
    let payload = get_manifest_signing_payload(&manifest);
    if let Err(err) = verify_signature(UPDATE_PUBLIC_KEY_HEX, &payload, &manifest.signature.value)
    {
        let err_msg = format!("Untrusted release: signature verification failed ({})", err);
        let mut st = manager.state.lock().unwrap();
        *st = UpdateState::Error {
            message: err_msg.clone(),
        };
        return Err(err_msg);
    }

    // 4. Compare versions using strict SemVer
    let current_ver = Version::parse(CURRENT_VERSION)
        .map_err(|e| format!("Invalid current version format: {}", e))?;
    let remote_ver = Version::parse(&manifest.version)
        .map_err(|e| format!("Invalid remote version format: {}", e))?;

    let is_newer = remote_ver > current_ver;

    {
        let mut cached = manager.cached_manifest.lock().unwrap();
        *cached = Some(manifest.clone());

        let mut st = manager.state.lock().unwrap();
        if is_newer {
            *st = UpdateState::Available;
        } else {
            *st = UpdateState::Idle;
        }
    }

    Ok(UpdateCheckResult {
        update_available: is_newer,
        current_version: CURRENT_VERSION.to_string(),
        new_version: if is_newer {
            Some(manifest.version.clone())
        } else {
            None
        },
        mandatory: is_newer && manifest.mandatory,
        release_notes: if is_newer {
            Some(manifest.release_notes.clone())
        } else {
            None
        },
        release_date: if is_newer {
            Some(manifest.release_date.clone())
        } else {
            None
        },
        package_size: if is_newer {
            Some(manifest.package.size)
        } else {
            None
        },
        package_url: if is_newer {
            Some(manifest.package.url.clone())
        } else {
            None
        },
    })
}

/// Spawns background update download with progress reporting.
pub fn start_download_update() -> Result<(), String> {
    let manager = UPDATER.get();
    let manifest = {
        let cached = manager.cached_manifest.lock().unwrap();
        cached
            .clone()
            .ok_or_else(|| "No update manifest cached. Check for updates first.".to_string())?
    };

    if manager.is_downloading.swap(true, Ordering::SeqCst) {
        return Ok(()); // Already in progress
    }

    manager.download_progress.store(0, Ordering::SeqCst);

    let manager_clone = manager.clone();
    thread::spawn(move || {
        let result = perform_download_and_verify(&manifest, &manager_clone);
        manager_clone.is_downloading.store(false, Ordering::SeqCst);

        let mut st = manager_clone.state.lock().unwrap();
        match result {
            Ok(staged_path) => {
                *st = UpdateState::ReadyToInstall;
                let mut path_lock = manager_clone.staged_exe_path.lock().unwrap();
                *path_lock = Some(staged_path);
            }
            Err(err) => {
                *st = UpdateState::Error {
                    message: err.clone(),
                };
                eprintln!("[UPDATER] Download failed: {}", err);
            }
        }
    });

    Ok(())
}

fn perform_download_and_verify(
    manifest: &ReleaseManifest,
    manager: &UpdaterManager,
) -> Result<PathBuf, String> {
    let staged_dir = get_staged_dir();
    fs::create_dir_all(&staged_dir)
        .map_err(|e| format!("Failed to create staging directory: {}", e))?;

    let filename = format!("DeskStream-v{}.exe", manifest.version);
    let staged_file = staged_dir.join(&filename);
    let temp_file = staged_dir.join(format!("{}.download", &filename));

    // Remove any previous partial downloads
    if temp_file.exists() {
        let _ = fs::remove_file(&temp_file);
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let mut resp = client
        .get(&manifest.package.url)
        .header("User-Agent", format!("DeskStream/{}", CURRENT_VERSION))
        .send()
        .map_err(|e| format!("Failed to start package download: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download returned HTTP {}", resp.status()));
    }

    let total_bytes = resp.content_length().unwrap_or(manifest.package.size);
    let mut dest_file =
        File::create(&temp_file).map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut buffer = [0u8; 32768];
    let mut downloaded: u64 = 0;

    loop {
        let bytes_read = resp
            .read(&mut buffer)
            .map_err(|e| format!("Download stream error: {}", e))?;
        if bytes_read == 0 {
            break;
        }

        dest_file
            .write_all(&buffer[..bytes_read])
            .map_err(|e| format!("Failed to write to file: {}", e))?;
        downloaded += bytes_read as u64;

        let pct = if total_bytes > 0 {
            ((downloaded as f64 / total_bytes as f64) * 100.0).min(99.0) as u32
        } else {
            50
        };
        manager.download_progress.store(pct, Ordering::SeqCst);

        let mut st = manager.state.lock().unwrap();
        *st = UpdateState::Downloading {
            progress: pct,
            received_bytes: downloaded,
            total_bytes,
        };
    }

    dest_file
        .flush()
        .map_err(|e| format!("Failed to flush file: {}", e))?;
    drop(dest_file);

    // 1. Verify SHA-256
    let computed_hash = compute_file_sha256(&temp_file)?;
    let expected_hash = manifest.package.sha256.trim().to_lowercase();
    if computed_hash.to_lowercase() != expected_hash {
        let _ = fs::remove_file(&temp_file);
        return Err(format!(
            "SHA-256 integrity check failed! Expected {}, got {}",
            expected_hash, computed_hash
        ));
    }

    // 2. Atomically rename temp file to staged file
    if staged_file.exists() {
        let _ = fs::remove_file(&staged_file);
    }
    fs::rename(&temp_file, &staged_file)
        .map_err(|e| format!("Failed to finalize staged update file: {}", e))?;

    manager.download_progress.store(100, Ordering::SeqCst);
    println!(
        "[UPDATER] Verified update staged successfully at: {}",
        staged_file.display()
    );
    Ok(staged_file)
}

/// Applies the staged update by launching the standalone deskstream-updater.exe
/// and exiting the current process.
pub fn apply_update_and_restart() -> Result<(), String> {
    let manager = UPDATER.get();

    // Check if an active session is in progress
    if let Some(live_status) = crate::status::LIVE.get() {
        if let Ok(st) = live_status.lock() {
            if st.in_session {
                return Err(
                    "Cannot restart while a remote desktop session is active. Please end the session first."
                        .to_string(),
                );
            }
        }
    }

    let staged_path = {
        let path_lock = manager.staged_exe_path.lock().unwrap();
        path_lock
            .clone()
            .ok_or_else(|| "No staged update file found.".to_string())?
    };

    if !staged_path.exists() {
        return Err(format!(
            "Staged update file missing at {}",
            staged_path.display()
        ));
    }

    let current_exe =
        env::current_exe().map_err(|e| format!("Failed to get current executable path: {}", e))?;
    let current_dir = current_exe
        .parent()
        .ok_or_else(|| "Failed to get exe directory".to_string())?;

    let updater_exe = current_dir.join("deskstream-updater.exe");

    // Ensure updater binary is present
    if !updater_exe.exists() {
        // Fallback: look in staging or project directory
        let alt_updater = get_updates_dir().join("deskstream-updater.exe");
        if alt_updater.exists() {
            let _ = fs::copy(&alt_updater, &updater_exe);
        }
    }

    let pid = std::process::id();

    {
        let mut st = manager.state.lock().unwrap();
        *st = UpdateState::Installing;
    }

    // Launch updater helper:
    // deskstream-updater.exe --pid <PID> --target "<target_exe>" --staged "<staged_exe>"
    let launch_result = if updater_exe.exists() {
        Command::new(&updater_exe)
            .arg("--pid")
            .arg(pid.to_string())
            .arg("--target")
            .arg(&current_exe)
            .arg("--staged")
            .arg(&staged_path)
            .spawn()
    } else {
        // Safe PowerShell fallback script if deskstream-updater.exe is not yet present on disk
        let ps_script = format!(
            "$pidToWait = {}; $target = '{}'; $staged = '{}'; $bak = \"$target.bak\"; \
             while (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 300 }}; \
             Start-Sleep -Milliseconds 500; \
             if (Test-Path $target) {{ Move-Item -LiteralPath $target -Destination $bak -Force }}; \
             Move-Item -LiteralPath $staged -Destination $target -Force; \
             Start-Process -FilePath $target; \
             Start-Sleep -Seconds 3; \
             if (Test-Path $bak) {{ Remove-Item -LiteralPath $bak -Force -ErrorAction SilentlyContinue }};",
            pid,
            current_exe.display(),
            staged_path.display()
        );

        Command::new("powershell.exe")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-WindowStyle")
            .arg("Hidden")
            .arg("-Command")
            .arg(ps_script)
            .spawn()
    };

    match launch_result {
        Ok(_) => {
            println!("[UPDATER] Updater launched successfully. Exiting DeskStream to complete update.");
            std::process::exit(0);
        }
        Err(e) => {
            let err_msg = format!("Failed to launch updater process: {}", e);
            let mut st = manager.state.lock().unwrap();
            *st = UpdateState::Error {
                message: err_msg.clone(),
            };
            Err(err_msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_verification() {
        let pubkey = UPDATE_PUBLIC_KEY_HEX;
        let message = b"test_message";
        let sig = "6f4a05627620e29328eee6a6195573dc9117ceeb229ae7e217b7aa87980b47c3f96b42e7177243e566ecc62916196469e0051ddc68ee69f344a5510752ba970b";
        let res = verify_signature(pubkey, message, sig);
        assert!(res.is_ok(), "Verification failed: {:?}", res);
        assert!(res.unwrap());
    }

    #[test]
    fn test_tampered_signature_rejected() {
        let pubkey = UPDATE_PUBLIC_KEY_HEX;
        let message = b"tampered_message";
        let sig = "6f4a05627620e29328eee6a6195573dc9117ceeb229ae7e217b7aa87980b47c3f96b42e7177243e566ecc62916196469e0051ddc68ee69f344a5510752ba970b";
        let res = verify_signature(pubkey, message, sig);
        assert!(res.is_err(), "Tampered message should fail verification");
    }

    #[test]
    fn test_semver_comparisons() {
        let current = Version::parse("1.1.3").unwrap();
        let newer = Version::parse("1.1.4").unwrap();
        let older = Version::parse("1.1.2").unwrap();
        let same = Version::parse("1.1.3").unwrap();
        assert!(newer > current);
        assert!(older < current);
        assert!(same == current);
    }

    #[test]
    fn test_sha256_hash_check() {
        let data = b"DeskStream update package payload";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());
        assert_eq!(hash.len(), 64);
    }
}

