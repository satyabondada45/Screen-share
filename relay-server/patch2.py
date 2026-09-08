import re

with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Update ClientMap
content = content.replace(
    'type ClientMap = Arc<Mutex<HashMap<String, Sender<ViewerSessionRequest>>>>;',
    'type ClientMap = Arc<Mutex<HashMap<String, (u64, Sender<ViewerSessionRequest>)>>>;'
)

# 2. Update handle_host connection ID generation
content = content.replace(
    'fn handle_host(\n    mut stream: TcpStream,\n    hosts: ClientMap,\n) {',
    'fn handle_host(\n    mut stream: TcpStream,\n    hosts: ClientMap,\n) {\n    let conn_id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;'
)

# 3. Update map.insert
content = content.replace(
    'map.insert(session_id.clone(), session_tx);',
    'map.insert(session_id.clone(), (conn_id, session_tx));'
)

# 4. Update map.remove
old_remove = '''    // Clean up from activeAgents on exit
    if let Ok(mut map) = hosts.lock() {
        map.remove(&session_id);
    }'''
new_remove = '''    // Clean up from activeAgents on exit
    if let Ok(mut map) = hosts.lock() {
        if let Some((existing_id, _)) = map.get(&session_id) {
            if *existing_id == conn_id {
                map.remove(&session_id);
            }
        }
    }'''
content = content.replace(old_remove, new_remove)

# 5. Update map.get in handle_viewer
content = content.replace(
    'map.get(&session_id).cloned()',
    'map.get(&session_id).map(|(_, tx)| tx.clone())'
)

# 6. Update map.get in handle_websocket_viewer (it uses it twice in an or_else)
content = content.replace(
    'host_tx_opt = map.get(&session_id).cloned().or_else(|| map.get(&raw_clean).cloned());',
    'host_tx_opt = map.get(&session_id).map(|(_, tx)| tx.clone()).or_else(|| map.get(&raw_clean).map(|(_, tx)| tx.clone()));'
)

# 7. Update logging
content = content.replace(
    'println!("[RELAY][LOOKUP] NOT FOUND");\\n            println!("[RELAY][CLOSE] connection_type=viewer device={} reason=agent_not_found',
    'println!("[RELAY][ROUTE] Requested System ID {} not currently connected", session_id);\\n            println!("[RELAY][CLOSE] connection_type=viewer device={} reason=agent_not_found'
)
content = content.replace(
    'println!("[Relay] Target host not found: {}", session_id);',
    'println!("[RELAY][ROUTE] Requested System ID {} not currently connected", session_id);'
)

# 8. Graceful shutdown in main
old_main = '''fn main() {
    println!("========================================");'''
new_main = '''fn main() {
    println!("========================================");
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("[RELAY] Received SIGINT/SIGTERM, initiating graceful shutdown...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");'''
content = content.replace(old_main, new_main)

# And add non-blocking listener in main:
# Before:
#     for incoming in listener.incoming() {
#         let stream = match incoming {
#             Ok(stream) => stream,
#             Err(e) => {
old_loop = '''    for incoming in listener.incoming() {
        let stream = match incoming {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("[Relay] Incoming connection error: {:?}", e);
                continue;
            }
        };'''
new_loop = '''    let _ = listener.set_nonblocking(true);
    while running.load(Ordering::SeqCst) {
        let (stream, _addr) = match listener.accept() {
            Ok(res) => res,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                eprintln!("[Relay] Incoming connection error: {:?}", e);
                continue;
            }
        };'''
content = content.replace(old_loop, new_loop)

# Fix the end of the loop since we changed to while
# We need to find the end of the match connection_type
# Wait, it's easier to just replace or incoming in listener.incoming() { 
# with while running.load(Ordering::SeqCst) { ...

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print("Patch script completed.")
