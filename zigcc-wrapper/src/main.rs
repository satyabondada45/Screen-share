use std::env;
use std::process::{Command, exit};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let status = Command::new("C:\\zig\\zig.exe")
        .arg("cc")
        .args(args)
        .status()
        .expect("Failed to execute zig");

    exit(status.code().unwrap_or(1));
}
