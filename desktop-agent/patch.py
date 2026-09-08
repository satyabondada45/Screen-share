import re

with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Fix input_handle read deadlock
# We want to change read_stream.set_read_timeout(None) to Some(Duration::from_millis(500))
# and handle the timeout error.

# First, fix the timeout setup.
old_setup = '''                            let mut read_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = read_stream.set_read_timeout(None);'''

new_setup = '''                            let mut read_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = read_stream.set_read_timeout(Some(Duration::from_millis(500)));'''

content = content.replace(old_setup, new_setup)

# Second, fix the read_exact loop in input_handle
old_loop = '''                                while is_conn_read.load(Ordering::SeqCst) {
                                    let mut pkt_type_buf = [0u8; 1];
                                    if read_stream.read_exact(&mut pkt_type_buf).is_err() {
                                        is_conn_read.store(false, Ordering::SeqCst);
                                        break;
                                    }'''

new_loop = '''                                while is_conn_read.load(Ordering::SeqCst) {
                                    let mut pkt_type_buf = [0u8; 1];
                                    if let Err(e) = read_stream.read_exact(&mut pkt_type_buf) {
                                        if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                                            continue;
                                        }
                                        is_conn_read.store(false, Ordering::SeqCst);
                                        break;
                                    }'''

content = content.replace(old_loop, new_loop)

# Third, fix the relay address configuration
old_relay_config = '''    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        env::var("SCREENSHARE_RELAY_URL").unwrap_or_else(|_| "192.168.29.229:9001".to_string())
    };'''

new_relay_config = '''    let relay_addr = if args.len() > 1 {
        args[1].clone()
    } else {
        env::var("SCREENSHARE_RELAY_URL").unwrap_or_else(|_| env::var("RELAY_HOST").unwrap_or_else(|_| "192.168.29.229:9001".to_string()))
    };'''
content = content.replace(old_relay_config, new_relay_config)


with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print("Patch applied")
