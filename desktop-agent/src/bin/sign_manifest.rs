// desktop-agent/src/bin/sign_manifest.rs
// Signs update manifests with an Ed25519 private key

use ed25519_dalek::{Signer, SigningKey};
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: sign-manifest <private_key_hex_file> <message>");
        std::process::exit(1);
    }
    let key_file = &args[1];
    let message = &args[2];

    let key_hex = fs::read_to_string(key_file).expect("Failed to read private key file");
    let key_bytes = hex_decode(key_hex.trim()).expect("Invalid hex in private key");
    if key_bytes.len() != 32 {
        eprintln!("Expected 32-byte private key, got {}", key_bytes.len());
        std::process::exit(1);
    }
    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&key_bytes);
    let signing_key = SigningKey::from_bytes(&key_arr);
    let signature = signing_key.sign(message.as_bytes());
    let sig_hex = hex_encode(&signature.to_bytes());
    print!("{}", sig_hex);
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("Odd length hex string".to_string());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
