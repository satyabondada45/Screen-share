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

start_marker = 'let is_connected = Arc::new(AtomicBool::new(true));'
end_marker = 'is_in_session.store(false, Ordering::SeqCst);'

start_idx = code.find(start_marker)
end_idx = code.find(end_marker, start_idx) + len(end_marker)

pipeline_body = code[start_idx:end_idx]

# Clean up pipeline body to remove TCP-specific setup
pipeline_body = pipeline_body.replace('''                            let mut read_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = read_stream.set_read_timeout(None);''', '')

pipeline_body = pipeline_body.replace('''                            let mut write_tcp = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    is_connected.store(false, Ordering::Release);
                                    break 'viewer_loop;
                                }
                            };
                            let _ = write_tcp.set_write_timeout(None);''', '')

pipeline_body = pipeline_body.replace('''                            #[cfg(windows)]
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
                            }''', '')

# Remove backend.log_session_end as it depends on backend
pipeline_body = pipeline_body.replace('backend.log_session_end(&system_id, 0.0);', '// backend.log_session_end(&system_id, 0.0);')

# Replace 'break 'viewer_loop;' with 'return;' inside the pipeline block
pipeline_body = pipeline_body.replace("break 'viewer_loop;", "return;")


func_def = f'''
pub fn start_session_pipeline(
    mut read_stream: SessionReader,
    mut write_tcp: SessionWriter,
    system_id: String,
    id_str: String,
    is_in_session: Arc<AtomicBool>,
) {{
{pipeline_body}
}}
'''

func_call = '''
                            // TCP INPUT
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
                            break 'viewer_loop;
'''

code = code[:start_idx] + func_call + code[end_idx:]
code += func_def

with open('main.rs', 'w', encoding='utf-8') as f:
    f.write(code)

print("Done refactoring main.rs")
