// src/bin/updater.rs
// DeskStream Standalone Updater
// Safely replaces DeskStream.exe after the main process exits and relaunches it.
// Includes automated rollback if the updated executable fails to start.

#![windows_subsystem = "windows"]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{CloseHandle, FALSE, WAIT_TIMEOUT};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

fn log_updater(msg: &str) {
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let log_dir = PathBuf::from(local_app_data).join("DeskStream").join("updates");
        let _ = fs::create_dir_all(&log_dir);
        let log_file = log_dir.join("updater.log");
        let timestamp = chrono_or_simple_time();
        let entry = format!("[{}] {}\r\n", timestamp, msg);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file);
        if let Ok(ref mut f) = file {
            use std::io::Write;
            let _ = f.write_all(entry.as_bytes());
        }
    }
}

fn chrono_or_simple_time() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now();
    format!("{:?}", now)
}

fn wait_for_pid(pid: u32, timeout_secs: u32) {
    #[cfg(target_os = "windows")]
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, FALSE, pid);
        if handle != 0 {
            log_updater(&format!("Waiting for parent PID {} to terminate...", pid));
            let wait_res = WaitForSingleObject(handle, timeout_secs * 1000);
            if wait_res == WAIT_TIMEOUT {
                log_updater(&format!("Parent PID {} did not exit within {}s; terminating.", pid, timeout_secs));
                let _ = TerminateProcess(handle, 1);
                WaitForSingleObject(handle, 5000);
            }
            CloseHandle(handle);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        thread::sleep(Duration::from_secs(2));
    }
    // Allow extra grace period for file handles to be released
    thread::sleep(Duration::from_millis(500));
}

fn main() {
    log_updater("=== DeskStream Updater Started ===");

    let args: Vec<String> = env::args().collect();
    let mut target_str: Option<String> = None;
    let mut staged_str: Option<String> = None;
    let mut backup_str: Option<String> = None;
    let mut parent_pid: Option<u32> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pid" => {
                if i + 1 < args.len() {
                    parent_pid = args[i + 1].parse::<u32>().ok();
                    i += 1;
                }
            }
            "--target" => {
                if i + 1 < args.len() {
                    target_str = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--staged" => {
                if i + 1 < args.len() {
                    staged_str = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--backup" => {
                if i + 1 < args.len() {
                    backup_str = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let target_path = match target_str {
        Some(t) => PathBuf::from(t),
        None => {
            log_updater("Error: --target parameter is missing");
            return;
        }
    };

    let staged_path = match staged_str {
        Some(s) => PathBuf::from(s),
        None => {
            log_updater("Error: --staged parameter is missing");
            return;
        }
    };

    let backup_path = backup_str
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{}.bak", target_path.display())));

    if let Some(pid) = parent_pid {
        wait_for_pid(pid, 20);
    }

    if !staged_path.exists() {
        log_updater(&format!("Error: Staged package does not exist: {}", staged_path.display()));
        return;
    }

    log_updater(&format!("Backing up {} -> {}", target_path.display(), backup_path.display()));
    if target_path.exists() {
        // Try multiple times in case antivirus holds handle
        let mut moved = false;
        for attempt in 1..=10 {
            if backup_path.exists() {
                let _ = fs::remove_file(&backup_path);
            }
            match fs::rename(&target_path, &backup_path) {
                Ok(_) => {
                    moved = true;
                    break;
                }
                Err(err) => {
                    log_updater(&format!("Move attempt {} failed: {}", attempt, err));
                    thread::sleep(Duration::from_millis(300));
                }
            }
        }

        if !moved {
            log_updater("Fatal: Could not back up target executable. Aborting update.");
            return;
        }
    }

    log_updater(&format!("Applying staged update {} -> {}", staged_path.display(), target_path.display()));
    let mut copied = false;
    for attempt in 1..=10 {
        match fs::copy(&staged_path, &target_path) {
            Ok(_) => {
                copied = true;
                break;
            }
            Err(err) => {
                log_updater(&format!("Copy attempt {} failed: {}", attempt, err));
                thread::sleep(Duration::from_millis(300));
            }
        }
    }

    if !copied {
        log_updater("Fatal: Failed to install new executable! Initiating rollback...");
        if backup_path.exists() {
            let _ = fs::rename(&backup_path, &target_path);
            let _ = Command::new(&target_path).spawn();
        }
        return;
    }

    log_updater(&format!("Launching updated DeskStream: {}", target_path.display()));
    match Command::new(&target_path).spawn() {
        Ok(mut child) => {
            // Verify child process stays alive
            thread::sleep(Duration::from_millis(2500));
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process exited immediately! Failure!
                    log_updater(&format!("Error: Updated binary exited immediately with status: {:?}. Rolling back!", status));
                    let _ = fs::remove_file(&target_path);
                    if backup_path.exists() {
                        let _ = fs::rename(&backup_path, &target_path);
                        let _ = Command::new(&target_path).spawn();
                    }
                    return;
                }
                _ => {
                    // Process is running healthy!
                    log_updater("Update successfully applied and verified running!");
                    // Clean up backup and staged files
                    if backup_path.exists() {
                        let _ = fs::remove_file(&backup_path);
                    }
                    let _ = fs::remove_file(&staged_path);
                }
            }
        }
        Err(err) => {
            log_updater(&format!("Fatal: Failed to spawn updated binary: {}. Rolling back...", err));
            if backup_path.exists() {
                let _ = fs::rename(&backup_path, &target_path);
                let _ = Command::new(&target_path).spawn();
            }
        }
    }

    log_updater("=== DeskStream Updater Finished ===");
}
