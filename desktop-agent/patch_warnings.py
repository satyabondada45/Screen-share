import re

with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Fix unreachable pattern warning
content = content.replace("0..=10 | 14 => {", "0..=10 => {")
content = content.replace("let is_first_key = frame_number == 1;", "let _is_first_key = frame_number == 1;")

# Ignore unused warnings for the two functions
content = content.replace("fn compute_sha256(input: &str) -> [u8; 32] {", "#[allow(dead_code)]\nfn compute_sha256(input: &str) -> [u8; 32] {")
content = content.replace("fn map_key_code(code: u32) -> Option<Key> {", "#[allow(dead_code)]\nfn map_key_code(code: u32) -> Option<Key> {")
content = content.replace("use enigo::{Axis, Direction, Enigo, Key, Keyboard, Mouse, Settings};", "use enigo::{Key};")

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print("Warnings patched")
