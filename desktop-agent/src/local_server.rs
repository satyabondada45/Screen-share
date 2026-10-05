use std::thread;
use std::io::Read;
use tiny_http::{Server, Response, Header};
use reqwest::blocking::Client;
use std::sync::Arc;
use serde_json::json;

const UPSTREAM: &str = "https://friendssoftwaresolutions.in/DeskStream";

pub fn start_local_server(system_id: String, quit: std::sync::Arc<std::sync::atomic::AtomicBool>) -> u16 {
    let server = Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    
    // Enable reqwest cookie store to maintain upstream Hostinger session automatically
    // This avoids exposing the upstream PHPSESSID to the local browser context
    let client = Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();

    let client = Arc::new(client);

    thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let path = request.url().to_string();
            
            // Proxy API Requests
            if path.starts_with("/api/") {
                let upstream_url = format!("{}{}", UPSTREAM, path);
                let method = reqwest::Method::from_bytes(request.method().as_str().as_bytes()).unwrap();
                
                use std::io::Write;
                let mut log_file = std::fs::OpenOptions::new().create(true).append(true).open("proxy.log").unwrap();
                
                writeln!(log_file, "[PROXY] local request = {}", path).unwrap();
                writeln!(log_file, "[PROXY] upstream URL = {}", upstream_url).unwrap();
                writeln!(log_file, "[PROXY] method = {}", method).unwrap();
                
                let mut req_builder = client.request(method, &upstream_url);
                
                // Forward specific headers (do NOT forward Cookie)
                for header in request.headers() {
                    let name = header.field.to_string().to_lowercase();
                    if name == "content-type" || name == "accept" {
                        req_builder = req_builder.header(header.field.to_string(), header.value.to_string());
                    }
                }
                
                // Read body
                let mut body = Vec::new();
                let _ = request.as_reader().read_to_end(&mut body);
                if !body.is_empty() {
                    writeln!(log_file, "[PROXY] body length = {}", body.len()).unwrap();
                    if let Ok(body_str) = std::str::from_utf8(&body) {
                        writeln!(log_file, "[PROXY] request body = {}", body_str).unwrap();
                    }
                    req_builder = req_builder.body(body);
                }
                
                match req_builder.send() {
                    Ok(mut upstream_resp) => {
                        let status_code = upstream_resp.status().as_u16();
                        writeln!(log_file, "[PROXY] upstream status = {}", status_code).unwrap();
                        let mut resp_headers = Vec::new();
                        
                        // Forward Content-Type (do NOT forward Set-Cookie)
                        for (k, v) in upstream_resp.headers().iter() {
                            let name = k.as_str().to_lowercase();
                            if name == "content-type" {
                                if let Ok(val) = v.to_str() {
                                    if let Ok(header) = Header::from_bytes(k.as_str().as_bytes(), val.as_bytes()) {
                                        resp_headers.push(header);
                                    }
                                }
                            }
                        }
                        
                        let mut resp_body = Vec::new();
                        let _ = upstream_resp.read_to_end(&mut resp_body);
                        
                        if let Ok(body_str) = std::str::from_utf8(&resp_body) {
                            writeln!(log_file, "[PROXY] upstream response = {}", body_str).unwrap();
                        }
                        
                        let mut response = Response::from_data(resp_body)
                            .with_status_code(status_code);
                            
                        for h in resp_headers {
                            response.add_header(h);
                        }
                        let _ = request.respond(response);
                    },
                    Err(e) => {
                        println!("Proxy error: {}", e);
                        let _ = request.respond(Response::from_string("Proxy error").with_status_code(502));
                    }
                }
                continue;
            }
            
            // Health endpoint for UI and startup bootstrap.
            // The UI currently checks /local-health, while the startup bootstrap expects /health.
            // Keep both endpoints available on the existing local server so the dashboard can
            // come up without introducing a second backend or changing the startup flow.
            if matches!(path.as_str(), "/health" | "/health/" | "/local-health" | "/local-health/") {
                let in_session = match crate::status::LIVE.get() {
                    Some(status) => match status.lock() {
                        Ok(status) => status.in_session,
                        Err(_) => {
                            let _ = request.respond(
                                Response::from_string("agent status lock poisoned").with_status_code(500),
                            );
                            continue;
                        }
                    },
                    None => false,
                };
                let peer_system_id = match crate::status::session_peer_system_id() {
                    Ok(Some(peer_system_id)) if in_session => peer_system_id,
                    Ok(_) => String::new(),
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                        continue;
                    }
                };
                let response = Response::from_string(json!({
                    "running": true,
                    "system_id": system_id,
                    "status": "online",
                    "role": "REMOTE",
                    "state": if in_session { "CONNECTED" } else { "IDLE" },
                    "session_id": null,
                    "local_device": system_id,
                    "remote_device": peer_system_id,
                    "in_session": in_session
                }).to_string())
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/shutdown" && request.method() == &tiny_http::Method::Post {
                println!("[AGENT] Received graceful shutdown request from UI.");
                quit.store(true, std::sync::atomic::Ordering::Relaxed);
                let response = Response::from_string("{\"success\":true}")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/disconnect"
                && request.method() == &tiny_http::Method::Post
            {
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[DISCONNECT ROUTE TRACE] endpoint=/desktop-api/session/disconnect source=HTTP_REQUEST timestamp_ms={}",
                        crate::status::now_ms()
                    ),
                );
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[TYPE99 TRACE] component=HOST direction=SEND device_id={} session_id={} source=desktop_api_session_disconnect reason=explicit_local_disconnect",
                        system_id, system_id
                    ),
                );
                match crate::status::send_session_packet(vec![99]) {
                    Ok(()) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[TYPE99 TRACE] component=HOST direction=SEND_COMPLETE device_id={} session_id={} source=desktop_api_session_disconnect reason=packet_queued",
                                system_id, system_id
                            ),
                        );
                        println!("[SESSION DISCONNECT] TYPE 99 queued for active relay session.");
                        let response = Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[TYPE99 TRACE] component=HOST direction=SEND_FAILED device_id={} session_id={} source=desktop_api_session_disconnect reason={}",
                                system_id, system_id, error
                            ),
                        );
                        let _ = request.respond(Response::from_string(error).with_status_code(503));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/permissions" && request.method() == &tiny_http::Method::Post {
                let mut body = Vec::new();
                if let Err(error) = request.as_reader().read_to_end(&mut body) {
                    let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(payload) => payload,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                        continue;
                    }
                };
                let view_screen = payload.get("view_screen").and_then(|v| v.as_bool()).unwrap_or(true);
                let control_input = payload.get("control_input").and_then(|v| v.as_bool()).unwrap_or(true);
                let file_transfer = payload.get("file_transfer").and_then(|v| v.as_bool()).unwrap_or(true);

                crate::status::set_session_perm_view_screen(view_screen);
                crate::status::set_session_perm_control_input(control_input);
                crate::status::set_session_perm_file_transfer(file_transfer);

                let response = Response::from_string("{\"success\":true}")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/chat" && request.method() == &tiny_http::Method::Post {
                let mut body = Vec::new();
                if let Err(error) = request.as_reader().read_to_end(&mut body) {
                    let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(payload) => payload,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                        continue;
                    }
                };
                let message = match payload.get("message").and_then(serde_json::Value::as_str) {
                    Some(message) if !message.trim().is_empty() => message.trim(),
                    _ => {
                        let _ = request.respond(Response::from_string("message is required").with_status_code(400));
                        continue;
                    }
                };
                let message_bytes = message.as_bytes();
                if message_bytes.len() > u16::MAX as usize {
                    let _ = request.respond(Response::from_string("message is too long").with_status_code(413));
                    continue;
                }
                let mut packet = Vec::with_capacity(4 + message_bytes.len());
                packet.extend_from_slice(&[16, 0, (message_bytes.len() >> 8) as u8, message_bytes.len() as u8]);
                packet.extend_from_slice(message_bytes);
                match crate::status::send_session_packet(packet) {
                    Ok(()) => {
                        match crate::status::push_session_chat(message.to_string(), true) {
                            Ok(message) => {
                                println!(
                                    "[B CHAT] send type=16 payload_bytes={} packet_bytes={}",
                                    message_bytes.len(),
                                    4 + message_bytes.len()
                                );
                                let response = Response::from_string(json!({
                                    "success": true,
                                    "message": {
                                        "id": message.id.to_string(),
                                        "text": message.text,
                                        "from_local": message.from_local
                                    }
                                }).to_string())
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                                let _ = request.respond(response);
                            }
                            Err(error) => {
                                eprintln!("[CHAT UI] Sent message but failed to retain session history: {}", error);
                                let _ = request.respond(Response::from_string(error).with_status_code(500));
                            }
                        }
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(503));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/typing" && request.method() == &tiny_http::Method::Post {
                let mut body = Vec::new();
                if let Err(error) = request.as_reader().read_to_end(&mut body) {
                    let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(payload) => payload,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                        continue;
                    }
                };
                let typing = match payload.get("typing").and_then(serde_json::Value::as_bool) {
                    Some(typing) => typing,
                    None => {
                        let _ = request.respond(Response::from_string("typing must be a boolean").with_status_code(400));
                        continue;
                    }
                };
                let state = if typing {
                    crate::status::CHAT_TYPING_START
                } else {
                    crate::status::CHAT_TYPING_STOP
                };
                match crate::status::send_session_packet(vec![crate::status::CHAT_TYPING_PACKET_TYPE, state]) {
                    Ok(()) => {
                        let _ = request.respond(Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()));
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(503));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/typing" && request.method() == &tiny_http::Method::Get {
                let response = Response::from_string(json!({
                    "typing": crate::status::session_remote_typing()
                }).to_string())
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/audio" && request.method() == &tiny_http::Method::Post {
                let mut body = Vec::new();
                if let Err(error) = request.as_reader().read_to_end(&mut body) {
                    let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(payload) => payload,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                        continue;
                    }
                };
                let enabled = match payload.get("enabled").and_then(serde_json::Value::as_bool) {
                    Some(enabled) => enabled,
                    None => {
                        let _ = request.respond(Response::from_string("enabled must be a boolean").with_status_code(400));
                        continue;
                    }
                };
                crate::status::set_session_audio_enabled(enabled);
                let _ = request.respond(Response::from_string("{\"success\":true}")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()));
                continue;
            }

            if path == "/desktop-api/session/audio" && request.method() == &tiny_http::Method::Get {
                let response = Response::from_string(json!({
                    "enabled": crate::status::session_audio_enabled()
                }).to_string())
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/reverse" && request.method() == &tiny_http::Method::Post {
                let in_session = crate::status::LIVE
                    .get()
                    .and_then(|status| status.lock().ok())
                    .map(|status| status.in_session)
                    .unwrap_or(false);
                if !in_session {
                    let _ = request.respond(Response::from_string("No active session").with_status_code(409));
                    continue;
                }
                let peer_system_id = match crate::status::session_peer_system_id() {
                    Ok(Some(peer_system_id)) => peer_system_id,
                    Ok(None) => {
                        let _ = request.respond(Response::from_string("The active session has not registered its peer identity").with_status_code(409));
                        continue;
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                        continue;
                    }
                };
                if !crate::status::begin_session_reverse_request() {
                    let _ = request.respond(Response::from_string("A reverse request is already pending").with_status_code(409));
                    continue;
                }
                match crate::status::send_session_packet(vec![30]) {
                    Ok(()) => {
                        let _ = request.respond(Response::from_string(json!({
                            "success": true,
                            "peer_system_id": peer_system_id
                        }).to_string())
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()));
                    }
                    Err(error) => {
                        crate::status::cancel_session_reverse_request();
                        let _ = request.respond(Response::from_string(error).with_status_code(503));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/reverse/request" && request.method() == &tiny_http::Method::Get {
                let response = Response::from_string(json!({
                    "pending": crate::status::take_session_reverse_request()
                }).to_string())
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/reverse/decision" && request.method() == &tiny_http::Method::Get {
                let response = Response::from_string(json!({
                    "decision": crate::status::take_session_reverse_decision(),
                    "peer_system_id": crate::status::session_peer_system_id().unwrap_or_default()
                }).to_string())
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/reverse/decision" && request.method() == &tiny_http::Method::Post {
                let mut body = Vec::new();
                if let Err(error) = request.as_reader().read_to_end(&mut body) {
                    let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(payload) => payload,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                        continue;
                    }
                };
                let accepted = match payload.get("accepted").and_then(serde_json::Value::as_bool) {
                    Some(accepted) => accepted,
                    None => {
                        let _ = request.respond(Response::from_string("accepted must be a boolean").with_status_code(400));
                        continue;
                    }
                };
                match crate::status::send_session_packet(vec![31, u8::from(accepted)]) {
                    Ok(()) => {
                        crate::status::clear_session_reverse_request_pending();
                        let _ = request.respond(Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()));
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(503));
                    }
                }
                continue;
            }

            if path.split('?').next() == Some("/desktop-api/session/messages")
                && request.method() == &tiny_http::Method::Get
            {
                let after_cursor = path
                    .split_once('?')
                    .and_then(|(_, query)| query.split('&').find_map(|part| part.strip_prefix("after=")));
                let after_id = match after_cursor {
                    Some(value) => match value.parse::<u64>() {
                        Ok(value) => value,
                        Err(error) => {
                            let _ = request.respond(
                                Response::from_string(format!("invalid chat history cursor: {}", error))
                                    .with_status_code(400),
                            );
                            continue;
                        }
                    },
                    None => 0,
                };
                match crate::status::session_chat_since(after_id) {
                    Ok(messages) => {
                        let messages: Vec<_> = messages.into_iter().map(|message| json!({
                            "id": message.id.to_string(),
                            "text": message.text,
                            "from_local": message.from_local
                        })).collect();
                        let response = Response::from_string(json!(messages).to_string())
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/offers" && request.method() == &tiny_http::Method::Get {
                match crate::status::session_file_offers() {
                    Ok(offers) => {
                        let offers: Vec<_> = offers.into_iter().map(|offer| json!({
                            "transfer_id": offer.transfer_id.to_string(),
                            "filename": offer.filename,
                            "size": offer.size
                        })).collect();
                        let response = Response::from_string(json!(offers).to_string())
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/offer" && request.method() == &tiny_http::Method::Post {
                let mut body = Vec::new();
                if let Err(error) = request.as_reader().read_to_end(&mut body) {
                    let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(payload) => payload,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error.to_string()).with_status_code(400));
                        continue;
                    }
                };
                let transfer_id = payload.get("transfer_id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| value.parse::<u64>().ok());
                let accepted = payload.get("accept").and_then(serde_json::Value::as_bool);
                let (transfer_id, accepted) = match (transfer_id, accepted) {
                    (Some(id), Some(accepted)) => (id, accepted),
                    _ => {
                        let _ = request.respond(Response::from_string("transfer_id and accept are required").with_status_code(400));
                        continue;
                    }
                };
                match crate::status::decide_session_file(transfer_id, accepted) {
                    Ok(()) => {
                        println!("[FILE RX DECISION] direction=A->B transfer_id={} decision={}", transfer_id, if accepted { "ACCEPT" } else { "REJECT" });
                        let response = Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(409));
                    }
                }
                continue;
            }

            if path.starts_with("/desktop-api/transfers/pending") {
                // Return static empty for now just to not 404
                let json = "[]";
                let response = Response::from_string(json)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path.starts_with("/desktop-api/transfers/accept") {
                let response = Response::from_string("{\"success\":true}")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            // Extract the path without the query string for static file matching
            let static_path = match path.find('?') {
                Some(idx) => &path[..idx],
                None => &path,
            };

            // Static Files
            let (content, content_type) = match static_path {
                "/dashboard.html" | "/" | "" => (include_bytes!("../assets/dashboard.html").to_vec(), "text/html"),
                "/session.html" => (include_bytes!("../assets/session.html").to_vec(), "text/html"),
                "/deskstream-logo.png" => (include_bytes!("../assets/deskstream-logo.png").to_vec(), "image/png"),
                "/deskstream-icon.jpeg" => (include_bytes!("../assets/deskstream-icon.jpeg").to_vec(), "image/jpeg"),
                "/icon.ico" => (include_bytes!("../assets/icon.ico").to_vec(), "image/x-icon"),
                "/icon.png" => (include_bytes!("../assets/icon.png").to_vec(), "image/png"),
                _ => (vec![], "text/plain"),
            };
            
            if content.is_empty() {
                let _ = request.respond(Response::from_string("Not Found").with_status_code(404));
            } else {
                let response = Response::from_data(content)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap());
                let _ = request.respond(response);
            }
        }
    });
    
    port
}
