use std::thread;
use std::io::Read;
use tiny_http::{Server, Response, Header};
use reqwest::blocking::Client;
use std::sync::Arc;

const UPSTREAM: &str = "https://friendssoftwaresolutions.in/DeskStream";

pub fn start_local_server(system_id: String) -> u16 {
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
            
            // Health endpoint for UI
            if path == "/local-health" || path == "/local-health/" {
                let json = format!("{{\"running\": true, \"system_id\":\"{}\", \"status\": \"online\"}}", system_id);
                let response = Response::from_string(json)
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
