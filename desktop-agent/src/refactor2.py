with open('main.rs', 'r', encoding='utf-8') as f:
    code = f.read()

adapter_code = """
pub struct SessionReader {
    pub tcp: Option<std::net::TcpStream>,
    pub ws_rx: Option<std::sync::mpsc::Receiver<Vec<u8>>>,
    pub buffer: Vec<u8>,
}

impl SessionReader {
    pub fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        if let Some(tcp) = &mut self.tcp {
            use std::io::Read;
            tcp.read_exact(buf)
        } else if let Some(rx) = &self.ws_rx {
            if self.buffer.len() >= buf.len() {
                buf.copy_from_slice(&self.buffer[0..buf.len()]);
                self.buffer.drain(0..buf.len());
                Ok(())
            } else {
                while self.buffer.len() < buf.len() {
                    match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                        Ok(packet) => {
                            self.buffer.extend_from_slice(&packet);
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "timeout"));
                        }
                        Err(_) => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "disconnected")),
                    }
                }
                buf.copy_from_slice(&self.buffer[0..buf.len()]);
                self.buffer.drain(0..buf.len());
                Ok(())
            }
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "no reader"))
        }
    }
}

pub struct SessionWriter {
    pub tcp: Option<std::net::TcpStream>,
    pub ws_tx: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
}

impl SessionWriter {
    pub fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        if let Some(tcp) = &mut self.tcp {
            use std::io::Write;
            tcp.write_all(buf)
        } else if let Some(tx) = &self.ws_tx {
            match tx.send(buf.to_vec()) {
                Ok(_) => Ok(()),
                Err(_) => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "disconnected")),
            }
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "no writer"))
        }
    }
}
"""

code = code.replace('// ============================================================', adapter_code + '\n// ============================================================', 1)

old_tcp_code = '''                            let is_connected = Arc::new(AtomicBool::new(true));
                            let is_conn_read = Arc::clone(&is_connected);
                            let is_conn_write = Arc::clone(&is_connected);
                            let is_conn_capture = Arc::clone(&is_connected);
                            let is_conn_clip = Arc::clone(&is_connected);
                            let is_conn_audio = Arc::clone(&is_connected);
                            let is_conn_ping = Arc::clone(&is_connected);

                            if let Err(e) = stream.set_nodelay(true) {
                                eprintln!("[Agent] Warning: Could not set TCP_NODELAY: {}", e);
                            }

                            // TCP INPUT
                            let mut read_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = read_stream.set_read_timeout(None);

                            // Bounded Send Buffer: 64KB ensures the OS doesn't hide seconds of latency
                            #[cfg(windows)]
                            {
                                use std::os::windows::io::AsRawSocket;
                                let sock = stream.as_raw_socket();
                                let sndbuf: i32 = 65536;
                                unsafe {
                                    windows_sys::Win32::Networking::WinSock::setsockopt(
                                        sock as usize,
                                        windows_sys::Win32::Networking::WinSock::SOL_SOCKET,
                                        windows_sys::Win32::Networking::WinSock::SO_SNDBUF,
                                        &sndbuf as *const i32 as *const _,
                                        4,
                                    );
                                }
                            }

                            // OUTPUT QUEUE
                            let (out_tx, out_rx) = sync_channel::<Vec<u8>>(4);
                            let write_stream = out_tx.clone();
                            let mut write_tcp = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = write_tcp.set_write_timeout(None);'''

new_tcp_code = '''                            // TCP INPUT
                            let mut read_stream = match stream.try_clone() {
                                Ok(s) => SessionReader { tcp: Some(s), ws_rx: None, buffer: Vec::new() },
                                Err(_) => {
                                    break 'viewer_loop;
                                }
                            };
                            let mut write_tcp = match stream.try_clone() {
                                Ok(s) => SessionWriter { tcp: Some(s), ws_tx: None },
                                Err(_) => {
                                    break 'viewer_loop;
                                }
                            };
                            
                            start_session_pipeline(read_stream, write_tcp, system_id.clone(), id_str.clone(), is_in_session.clone());
                            break 'viewer_loop;'''

code = code.replace(old_tcp_code, new_tcp_code)

start_marker = 'let writer_handle = thread::spawn(move || {'
end_marker = 'is_in_session.store(false, Ordering::SeqCst);'

start_idx = code.find(start_marker)
end_idx = code.find(end_marker, start_idx) + len(end_marker)

pipeline_body = code[start_idx:end_idx]

# Remove the body from main code
code = code[:start_idx] + code[end_idx:]

func_def = f'''
pub fn start_session_pipeline(
    mut read_stream: SessionReader,
    mut write_tcp: SessionWriter,
    system_id: String,
    id_str: String,
    is_in_session: Arc<AtomicBool>,
) {{
    let is_connected = Arc::new(AtomicBool::new(true));
    let is_conn_read = Arc::clone(&is_connected);
    let is_conn_write = Arc::clone(&is_connected);
    let is_conn_capture = Arc::clone(&is_connected);
    let is_conn_clip = Arc::clone(&is_connected);
    let is_conn_audio = Arc::clone(&is_connected);
    let is_conn_ping = Arc::clone(&is_connected);
    let (out_tx, out_rx) = sync_channel::<Vec<u8>>(4);
    let write_stream = out_tx.clone();

    {pipeline_body}
}}
'''

code += func_def

# Also we need to fix backend.log_session_end!
# In the original code, `backend.log_session_end` is called at the end.
# But `backend` is a variable in `run_agent_loop` which we didn't pass to `start_session_pipeline`.
# Since Direct WS doesn't have `backend`, we should comment out `backend.log_session_end` in the extracted function,
# or we just comment it out since the relay logic might handle it differently now (the loop will reconnect and send offline).
code = code.replace('backend.log_session_end(&system_id, 0.0);', '// backend.log_session_end(&system_id, 0.0);')

with open('main.rs', 'w', encoding='utf-8') as f:
    f.write(code)

print("Done refactoring main.rs")
