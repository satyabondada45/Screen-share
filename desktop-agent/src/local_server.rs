use std::thread;
use std::io::Read;
use tiny_http::{Server, Response, Header};
use reqwest::blocking::Client;
use std::sync::Arc;
use serde_json::json;

fn upstream_api_url(local_path: &str) -> Option<String> {
    let api_path = local_path.strip_prefix("/api")?;
    if !api_path.starts_with('/') {
        return None;
    }
    Some(format!(
        "{}{}",
        crate::identity::device_id::DEFAULT_BACKEND_URL,
        api_path
    ))
}

#[cfg(test)]
mod api_url_tests {
    use super::upstream_api_url;

    #[test]
    fn api_proxy_uses_the_configured_production_api_base_once() {
        assert_eq!(
            upstream_api_url("/api/auth/login.php"),
            Some(
                "https://friendssoftwaresolutions.in/DeskStream/api/auth/login.php".to_string()
            )
        );
    }

    #[test]
    fn api_proxy_rejects_paths_outside_the_api_prefix() {
        assert_eq!(upstream_api_url("/apiary/auth/login.php"), None);
    }
}

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
                let upstream_url = match upstream_api_url(&path) {
                    Some(url) => url,
                    None => {
                        let _ = request.respond(Response::from_string("Invalid API path").with_status_code(400));
                        continue;
                    }
                };
                let method = match reqwest::Method::from_bytes(request.method().as_str().as_bytes()) {
                    Ok(method) => method,
                    Err(error) => {
                        eprintln!("[API PROXY] unsupported request method: {}", error);
                        let _ = request.respond(
                            Response::from_string("Unsupported API request method")
                                .with_status_code(405),
                        );
                        continue;
                    }
                };
                
                let mut req_builder = client.request(method, &upstream_url);
                
                let mut forwarded_cookie = false;
                let mut session_token: Option<String> = None;
                for header in request.headers() {
                    let name = header.field.to_string().to_lowercase();
                    if name == "content-type" 
                        || name == "accept" 
                        || name == "authorization" 
                        || name == "x-deskstream-session" 
                        || name == "x-auth-token" 
                    {
                        req_builder = req_builder.header(header.field.to_string(), header.value.to_string());
                        if name == "x-deskstream-session" {
                            session_token = Some(header.value.to_string());
                        }
                    } else if name == "cookie" {
                        forwarded_cookie = true;
                        req_builder = req_builder.header("Cookie", header.value.to_string());
                    }
                }
                if !forwarded_cookie {
                    if let Some(token) = session_token {
                        req_builder = req_builder.header("Cookie", format!("PHPSESSID={}", token));
                    }
                }
                
                // Read body
                let mut body = Vec::new();
                if let Err(error) = request.as_reader().read_to_end(&mut body) {
                    eprintln!(
                        "[API PROXY] failed to read request body for {}: {}",
                        path.split('?').next().unwrap_or("/api"),
                        error
                    );
                    let _ = request.respond(
                        Response::from_string("Could not read API request").with_status_code(400),
                    );
                    continue;
                }
                if !body.is_empty() {
                    req_builder = req_builder.body(body);
                }
                
                match req_builder.send() {
                    Ok(mut upstream_resp) => {
                        let status_code = upstream_resp.status().as_u16();
                        if status_code >= 400 {
                            eprintln!(
                                "[API PROXY] upstream returned HTTP {} for {}",
                                status_code,
                                path.split('?').next().unwrap_or("/api")
                            );
                        }
                        let mut resp_headers = Vec::new();
                        
                        // Forward Content-Type, Set-Cookie, Cache-Control
                        for (k, v) in upstream_resp.headers().iter() {
                            let name = k.as_str().to_lowercase();
                            if name == "content-type" || name == "set-cookie" || name == "cache-control" {
                                if let Ok(val) = v.to_str() {
                                    if let Ok(header) = Header::from_bytes(k.as_str().as_bytes(), val.as_bytes()) {
                                        resp_headers.push(header);
                                    }
                                }
                            }
                        }
                        
                        let mut resp_body = Vec::new();
                        if let Err(error) = upstream_resp.read_to_end(&mut resp_body) {
                            eprintln!(
                                "[API PROXY] failed to read upstream response for {}: {}",
                                path.split('?').next().unwrap_or("/api"),
                                error
                            );
                            let _ = request.respond(
                                Response::from_string("Could not read API response")
                                    .with_status_code(502),
                            );
                            continue;
                        }
                        
                        let mut response = Response::from_data(resp_body)
                            .with_status_code(status_code);
                            
                        for h in resp_headers {
                            response.add_header(h);
                        }
                        let _ = request.respond(response);
                    },
                    Err(e) => {
                        eprintln!(
                            "[API PROXY] upstream request failed for {}: {}",
                            path.split('?').next().unwrap_or("/api"),
                            e
                        );
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
                let (in_session, relay_health, session_health, reconnect_state, reconnect_attempt, reconnect_max_attempts, disconnect_reason) = match crate::status::LIVE.get() {
                    Some(status) => match status.lock() {
                        Ok(status) => (
                            status.in_session,
                            status.relay_health.clone(),
                            status.session_health.clone(),
                            status.reconnect_state.clone(),
                            status.reconnect_attempt,
                            status.reconnect_max_attempts,
                            status.disconnect_reason.clone(),
                        ),
                        Err(_) => {
                            let _ = request.respond(
                                Response::from_string("agent status lock poisoned").with_status_code(500),
                            );
                            continue;
                        }
                    },
                    None => (
                        false,
                        crate::status::HeartbeatHealth::default(),
                        crate::status::HeartbeatHealth::default(),
                        "Disconnected".to_string(),
                        0,
                        0,
                        "unknown_termination".to_string(),
                    ),
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
                    "in_session": in_session,
                    "connection_state": if in_session { session_health.state.clone() } else { reconnect_state.clone() },
                    "relay_health": {
                        "state": relay_health.state,
                        "rtt_ms": relay_health.rtt_ms,
                        "last_success_ms": relay_health.last_success_ms,
                        "missed_count": relay_health.missed_count,
                        "last_failure_reason": relay_health.last_failure_reason
                    },
                    "session_health": {
                        "state": session_health.state,
                        "rtt_ms": session_health.rtt_ms,
                        "last_success_ms": session_health.last_success_ms,
                        "missed_count": session_health.missed_count,
                        "last_failure_reason": session_health.last_failure_reason
                    },
                    "reconnect_state": reconnect_state,
                    "reconnect_attempt": reconnect_attempt,
                    "reconnect_max_attempts": reconnect_max_attempts,
                    "disconnect_reason": disconnect_reason
                }).to_string())
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/agent/reconnect" && request.method() == &tiny_http::Method::Post {
                if crate::status::request_manual_reconnect() {
                    let response = Response::from_string("{\"success\":true}")
                        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    let _ = request.respond(response);
                } else {
                    let _ = request.respond(
                        Response::from_string("Agent is not waiting for a manual retry").with_status_code(409),
                    );
                }
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

            // ── In-App Updater Endpoints ─────────────────────────────────────
            if path == "/desktop-api/updater/check" {
                let check_res = crate::updater::check_for_updates_sync();
                let json_body = match check_res {
                    Ok(result) => {
                        let manager = crate::updater::UPDATER.get();
                        let manifest = manager.cached_manifest.lock().ok().and_then(|m| m.clone());
                        json!({
                            "success": true,
                            "update_available": result.update_available,
                            "current_version": result.current_version,
                            "new_version": result.new_version,
                            "mandatory": result.mandatory,
                            "release_notes": result.release_notes,
                            "release_date": result.release_date,
                            "package_size": result.package_size,
                            "package_url": result.package_url,
                            "manifest": manifest,
                            "data": result
                        }).to_string()
                    },
                    Err(err) => json!({ "success": false, "error": err }).to_string(),
                };
                let response = Response::from_string(json_body)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/updater/download" && request.method() == &tiny_http::Method::Post {
                let dl_res = crate::updater::start_download_update();
                let json_body = match dl_res {
                    Ok(()) => json!({ "success": true }).to_string(),
                    Err(err) => json!({ "success": false, "error": err }).to_string(),
                };
                let response = Response::from_string(json_body)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/updater/status" {
                let manager = crate::updater::UPDATER.get();
                let current_state = manager.state.lock().map(|st| st.clone()).unwrap_or(crate::updater::UpdateState::Idle);
                let json_body = json!({
                    "success": true,
                    "state": current_state,
                    "current_version": crate::updater::CURRENT_VERSION
                }).to_string();
                let response = Response::from_string(json_body)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/updater/install" && request.method() == &tiny_http::Method::Post {
                let install_res = crate::updater::apply_update_and_restart();
                let json_body = match install_res {
                    Ok(()) => json!({ "success": true, "message": "Restarting application..." }).to_string(),
                    Err(err) => json!({ "success": false, "error": err }).to_string(),
                };
                let response = Response::from_string(json_body)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/disconnect"
                && request.method() == &tiny_http::Method::Post
            {
                let caller_source = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("X-DeskStream-Diagnostic-Source"))
                    .map(|header| header.value.as_str())
                    .unwrap_or("HTTP_REQUEST")
                    .chars()
                    .filter(|character| {
                        character.is_ascii_alphanumeric() || *character == '_' || *character == '-'
                    })
                    .take(64)
                    .collect::<String>();
                let in_session = crate::status::LIVE
                    .get()
                    .and_then(|status| status.lock().ok())
                    .map(|status| status.in_session)
                    .unwrap_or(false);
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[DISCONNECT ROUTE TRACE] component=LOCAL_HTTP direction=RECV endpoint=/desktop-api/session/disconnect source={} in_session={} timestamp_ms={}",
                        caller_source, in_session, crate::status::now_ms()
                    ),
                );
                let disconnect_reason = match caller_source.as_str() {
                    "END_SESSION_BUTTON" | "REMOTE_SESSION_END" | "REVERSE_HANDOFF" => {
                        "explicit_session_end"
                    }
                    _ => "explicit_user_disconnect",
                };
                crate::status::set_disconnect_reason(disconnect_reason);
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[TYPE99 TRACE] component=HOST direction=SEND device_id={} session_id={} source={} reason={} in_session={}",
                        system_id, system_id, caller_source, disconnect_reason, in_session
                    ),
                );
                match crate::status::send_session_packet(vec![99]) {
                    Ok(()) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[TYPE99 TRACE] component=HOST direction=SEND_COMPLETE device_id={} session_id={} source={} reason=packet_queued in_session={}",
                                system_id, system_id, caller_source, in_session
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
                                "[TYPE99 TRACE] component=HOST direction=SEND_FAILED device_id={} session_id={} source={} reason={} in_session={}",
                                system_id, system_id, caller_source, error, in_session
                            ),
                        );
                        crate::status::set_session_active(false);
                        let response = Response::from_string("{\"success\":true,\"session_active\":false}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
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
                let (Some(view_screen), Some(control_input), Some(file_transfer)) = (
                    payload.get("view_screen").and_then(serde_json::Value::as_bool),
                    payload.get("control_input").and_then(serde_json::Value::as_bool),
                    payload.get("file_transfer").and_then(serde_json::Value::as_bool),
                ) else {
                    let _ = request.respond(
                        Response::from_string(
                            "view_screen, control_input, and file_transfer must be booleans",
                        )
                        .with_status_code(400),
                    );
                    continue;
                };

                crate::status::set_session_perm_view_screen(view_screen);
                crate::status::set_session_perm_control_input(control_input);
                crate::status::set_session_perm_file_transfer(file_transfer);

                let response = Response::from_string("{\"success\":true}")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/authorize_request" && request.method() == &tiny_http::Method::Post {
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
                let token = payload.get("request_token").and_then(|v| v.as_str()).unwrap_or("");
                let requester_id = payload.get("requester_system_id").and_then(|v| v.as_str()).unwrap_or("");

                if token.trim().is_empty()
                    || requester_id.len() != 9
                    || !requester_id.bytes().all(|byte| byte.is_ascii_digit())
                {
                    let _ = request.respond(
                        Response::from_string("request_token and a valid requester_system_id are required")
                            .with_status_code(400),
                    );
                    continue;
                }
                match crate::status::record_approved_session(token, requester_id) {
                    Ok(()) => {
                        let response = Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/revoke_request" && request.method() == &tiny_http::Method::Post {
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
                let token = payload
                    .get("request_token")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if token.trim().is_empty() {
                    let _ = request.respond(
                        Response::from_string("request_token is required").with_status_code(400),
                    );
                    continue;
                }
                match crate::status::revoke_approved_session(token) {
                    Ok(_) => {
                        let response = Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                    }
                }
                continue;
            }

            if path == "/desktop-api/session/chat" && request.method() == &tiny_http::Method::Post {
                let trace_id = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("X-DeskStream-Trace-Id"))
                    .map(|header| header.value.as_str())
                    .filter(|value| {
                        value.len() <= 64
                            && value
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    })
                    .map(str::to_owned)
                    .unwrap_or_else(|| crate::session_debug::next_packet_trace_id(16));
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
                packet.extend_from_slice(&[16, 1, (message_bytes.len() >> 8) as u8, message_bytes.len() as u8]);
                packet.extend_from_slice(message_bytes);
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[CHAT TRACE] trace_id={} side=B direction=B_TO_A stage=NATIVE_PACKET_CREATED type=16 component=HOST payload_bytes={} packet_bytes={}",
                        trace_id,
                        message_bytes.len(),
                        packet.len()
                    ),
                );
                match crate::status::send_session_packet(packet) {
                    Ok(()) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[CHAT TRACE] trace_id={} side=B direction=B_TO_A stage=PACKET_QUEUED type=16 component=HOST payload_bytes={} packet_bytes={}",
                                trace_id,
                                message_bytes.len(),
                                4 + message_bytes.len()
                            ),
                        );
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
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[CHAT TRACE] trace_id={} side=B direction=B_TO_A stage=PACKET_QUEUE_ERROR type=16 component=HOST payload_bytes={} reason={}",
                                trace_id,
                                message_bytes.len(),
                                error
                            ),
                        );
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
                if enabled {
                    let in_session = crate::status::LIVE
                        .get()
                        .and_then(|status| status.lock().ok())
                        .map(|status| status.in_session)
                        .unwrap_or(false);
                    if !in_session {
                        let _ = request.respond(Response::from_string("No active session").with_status_code(409));
                        continue;
                    }
                    if !crate::status::session_audio_capture_available() {
                        let _ = request.respond(
                            Response::from_string("Microphone capture is unavailable; check that a microphone is connected and permitted")
                                .with_status_code(503),
                        );
                        continue;
                    }
                }
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
                let request_id = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("X-DeskStream-Reverse-Request-Id"))
                    .map(|header| header.value.as_str())
                    .unwrap_or("")
                    .to_string();
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
                match crate::status::begin_session_reverse_request_with_id(&request_id) {
                    Ok(true) => {}
                    Ok(false) => {
                        let _ = request.respond(Response::from_string("A reverse request is already pending").with_status_code(409));
                        continue;
                    }
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(400));
                        continue;
                    }
                }
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[REVERSE TRACE] role=HOST direction=SEND stage=REQUEST_QUEUED message_type=30 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} result=pending",
                        request_id, system_id, peer_system_id
                    ),
                );
                match crate::status::send_session_packet(vec![30]) {
                    Ok(()) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[REVERSE TRACE] role=HOST direction=SEND_COMPLETE message_type=30 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} result=queued",
                                request_id, system_id, peer_system_id
                            ),
                        );
                        let _ = request.respond(Response::from_string(json!({
                            "success": true,
                            "peer_system_id": peer_system_id,
                            "request_id": request_id
                        }).to_string())
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()));
                    }
                    Err(error) => {
                        crate::status::cancel_session_reverse_request();
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[REVERSE TRACE] role=HOST direction=SEND_FAILED message_type=30 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} result=failed reason={}",
                                request_id, system_id, peer_system_id, error
                            ),
                        );
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

            if path == "/desktop-api/session/reverse/cancel" && request.method() == &tiny_http::Method::Post {
                let (_, peer_system_id, _) =
                    match crate::status::peek_session_reverse_decision_with_peer() {
                        Ok(state) => state,
                        Err(error) => {
                            let _ = request.respond(
                                Response::from_string(error).with_status_code(500),
                            );
                            continue;
                        }
                    };
                let cancel_request_id = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("X-DeskStream-Reverse-Request-Id"))
                    .map(|header| header.value.as_str())
                    .unwrap_or("");
                if cancel_request_id.is_empty() {
                    let _ = request.respond(
                        Response::from_string("reverse request ID is required").with_status_code(400),
                    );
                    continue;
                }
                let cancelled = match crate::status::cancel_session_reverse_request_with_id(cancel_request_id) {
                    Ok(cancelled) => cancelled,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                        continue;
                    }
                };
                if !cancelled {
                    let _ = request.respond(
                        Response::from_string("reverse request ID does not match the outstanding request")
                            .with_status_code(409),
                    );
                    continue;
                }
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[REVERSE TRACE] role=HOST direction=LOCAL stage=CANCEL message_type=30 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} result=cleared",
                        cancel_request_id,
                        system_id,
                        peer_system_id.as_deref().unwrap_or("none")
                    ),
                );
                let response = Response::from_string("{\"success\":true}")
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/reverse/decision" && request.method() == &tiny_http::Method::Get {
                let (decision, peer_system_id, request_id) = match crate::status::peek_session_reverse_decision_with_peer() {
                    Ok((decision, peer_system_id, request_id)) => (
                        decision,
                        peer_system_id.unwrap_or_default(),
                        request_id.unwrap_or_default(),
                    ),
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                        continue;
                    }
                };
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[REVERSE TRACE] role=HOST direction=LOCAL_POLL stage=DECISION_READ message_type=31 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} decision={} result=retained_until_session_end_or_cancel",
                        request_id, system_id, peer_system_id, decision
                    ),
                );
                let response = Response::from_string(json!({
                    "decision": decision,
                    "peer_system_id": peer_system_id,
                    "request_id": request_id
                }).to_string())
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                let _ = request.respond(response);
                continue;
            }

            if path == "/desktop-api/session/reverse/decision/ack" && request.method() == &tiny_http::Method::Post {
                let request_id = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("X-DeskStream-Reverse-Request-Id"))
                    .map(|header| header.value.as_str())
                    .unwrap_or("");
                if request_id.is_empty() {
                    let _ = request.respond(
                        Response::from_string("reverse request ID is required").with_status_code(400),
                    );
                    continue;
                }
                let acknowledged = match crate::status::acknowledge_session_reverse_decision_with_id(request_id) {
                    Ok(acknowledged) => acknowledged,
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                        continue;
                    }
                };
                if !acknowledged {
                    let _ = request.respond(
                        Response::from_string("reverse decision is absent or belongs to another request")
                            .with_status_code(409),
                    );
                    continue;
                }
                let peer_system_id = match crate::status::session_peer_system_id() {
                    Ok(Some(peer_system_id)) => peer_system_id,
                    Ok(None) => "unknown".to_string(),
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                        continue;
                    }
                };
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[REVERSE TRACE] role=HOST direction=LOCAL stage=DECISION_ACK message_type=31 request_id={} active_session_id=unavailable active_session_host_id={} peer_system_id={} result=cleared_after_ui_validation",
                        request_id,
                        system_id,
                        peer_system_id
                    ),
                );
                let response = Response::from_string("{\"success\":true}")
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
                let peer_system_id = match crate::status::session_peer_system_id() {
                    Ok(Some(peer_system_id)) => peer_system_id,
                    Ok(None) => "unknown".to_string(),
                    Err(error) => {
                        let _ = request.respond(Response::from_string(error).with_status_code(500));
                        continue;
                    }
                };
                match crate::status::send_session_packet(vec![31, u8::from(accepted)]) {
                    Ok(()) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[REVERSE TRACE] role=HOST direction=SEND message_type=31 request_id=not_carried_by_wire active_session_id=unavailable active_session_host_id={} peer_system_id={} decision={} result=queued",
                                system_id,
                                peer_system_id,
                                u8::from(accepted)
                            ),
                        );
                        crate::status::clear_session_reverse_request_pending();
                        let _ = request.respond(Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()));
                    }
                    Err(error) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[REVERSE TRACE] role=HOST direction=SEND_FAILED message_type=31 request_id=not_carried_by_wire active_session_id=unavailable active_session_host_id={} peer_system_id={} decision={} result=failed reason={}",
                                system_id,
                                peer_system_id,
                                u8::from(accepted),
                                error
                            ),
                        );
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
                            "session_id": system_id,
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
                crate::session_debug::log(
                    &system_id,
                    &format!(
                        "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=A receiver=B stage=NATIVE_DECISION_RECEIVED decision={}",
                        system_id,
                        transfer_id,
                        if accepted { "ALLOW" } else { "REJECT" }
                    ),
                );
                match crate::status::decide_session_file(transfer_id, accepted) {
                    Ok(()) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=A receiver=B stage=NATIVE_DECISION_QUEUED packet_type={} decision={}",
                                system_id,
                                transfer_id,
                                if accepted { 26 } else { 25 },
                                if accepted { "ACCEPT" } else { "REJECT" }
                            ),
                        );
                        println!("[FILE RX DECISION] direction=A->B transfer_id={} decision={}", transfer_id, if accepted { "ACCEPT" } else { "REJECT" });
                        let response = Response::from_string("{\"success\":true}")
                            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        crate::session_debug::log(
                            &system_id,
                            &format!(
                                "[FILE APPROVAL TRACE] session_id={} transfer_id={} side=B sender=A receiver=B stage=NATIVE_DECISION_FAILED decision={} reason={}",
                                system_id,
                                transfer_id,
                                if accepted { "ALLOW" } else { "REJECT" },
                                error
                            ),
                        );
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
                "/controller_session.html" => (include_bytes!("../assets/controller_session.html").to_vec(), "text/html"),
                "/remote_session.html" => (include_bytes!("../assets/remote_session.html").to_vec(), "text/html"),
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
