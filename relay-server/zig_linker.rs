use std::env;
use std::fs;
use std::process::{exit, Command};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut new_args = Vec::new();

    for a in args {
        if a.starts_with('@') {
            let path = &a[1..];
            if let Ok(content) = fs::read_to_string(path) {
                let normalized = content
                    .replace(r"C:\\xampp\\htdocs\\Screen\ Share", r"C:\\deskstream")
                    .replace(r"C:\xampp\htdocs\Screen\ Share", r"C:\deskstream")
                    .replace("C:/xampp/htdocs/Screen/ Share", "C:/deskstream")
                    .replace("C:/xampp/htdocs/Screen Share", "C:/deskstream")
                    .replace(r"C:\\Users\\X1\ CORBON", r"C:\\cargo-target\\user")
                    .replace(r"C:\Users\X1\ CORBON", r"C:\cargo-target\user")
                    .replace("C:/Users/X1/ CORBON", "C:/cargo-target/user")
                    .replace("C:/Users/X1 CORBON", "C:/cargo-target/user")
                    .replace("--target=x86_64-unknown-linux-gnu", "--target=x86_64-linux-gnu");
                let new_path = "C:/cargo-target/tools/fixed-args.txt";
                fs::write(new_path, normalized).expect("write fixed-args.txt");
                new_args.push(format!("@{}", new_path));
                continue;
            }
        }
        let mut fixed = a
            .replace(r"C:\\xampp\\htdocs\\Screen\ Share", r"C:\\deskstream")
            .replace(r"C:\xampp\htdocs\Screen\ Share", r"C:\deskstream")
            .replace("C:/xampp/htdocs/Screen/ Share", "C:/deskstream")
            .replace("C:/xampp/htdocs/Screen Share", "C:/deskstream")
            .replace(r"C:\\Users\\X1\ CORBON", r"C:\\cargo-target\\user")
            .replace(r"C:\Users\X1\ CORBON", r"C:\cargo-target\user")
            .replace("C:/Users/X1/ CORBON", "C:/cargo-target/user")
            .replace("C:/Users/X1 CORBON", "C:/cargo-target/user");
        if fixed.starts_with("--target=x86_64-unknown-linux-gnu") {
            fixed = "--target=x86_64-linux-gnu".to_string();
        }
        new_args.push(fixed);
    }

    let status = Command::new("C:/cargo-target/zig/zig.exe")
        .arg("cc")
        .arg("-target")
        .arg("x86_64-linux-gnu")
        .args(&new_args)
        .status()
        .expect("failed to execute zig cc");
    exit(status.code().unwrap_or(1));
}
