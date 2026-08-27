import re

with open(r'desktop-agent\src\main.rs', 'r') as f:
    lines = f.readlines()

new_lines = []
in_viewer_loop = False

i = 0
while i < len(lines):
    line = lines[i]

    if '// Authentication request' in line and not in_viewer_loop:
        new_lines.append('        let is_connected = Arc::new(AtomicBool::new(true));\n')
        new_lines.append('        println!("[SESSION STATE] ONLINE");\n')
        new_lines.append('        \'viewer_loop: loop {\n')
        new_lines.append('            println!("[STREAM] waiting for viewer");\n')
        new_lines.append('            let is_streaming = Arc::new(AtomicBool::new(false));\n')
        new_lines.append('            let is_conn_read = Arc::clone(&is_connected);\n')
        new_lines.append('            let is_conn_write = Arc::clone(&is_streaming);\n')
        new_lines.append('            let is_conn_capture = Arc::clone(&is_streaming);\n')
        new_lines.append('            let is_conn_clip = Arc::clone(&is_streaming);\n')
        new_lines.append('            let is_conn_audio = Arc::clone(&is_streaming);\n')
        new_lines.append('            let is_conn_ping = Arc::clone(&is_streaming);\n')
        in_viewer_loop = True

    if in_viewer_loop:
        if 'if let Ok(mut s) = conn_status.lock() {' in line and '*s = ui::ConnectionStatus::Disconnected;' in lines[i+1]:
            # Before we hit break, we might have these, change them to break 'viewer_loop or continue if we are inside match
            pass
            
    new_lines.append(line)
    i += 1

# This is getting too complex to do via Python regex safely. 
