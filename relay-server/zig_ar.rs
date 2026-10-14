use std::env;
use std::process::{exit, Command};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let status = Command::new("C:\\cargo-target\\zig\\zig.exe")
        .arg("ar")
        .args(&args)
        .status()
        .expect("failed to execute zig ar");
    exit(status.code().unwrap_or(1));
}
