use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;
use workspaceai_client::api::Api;
use workspaceai_client::supervisor::Info;

/// Serves one request with `response`, returns the raw request text it saw.
fn serve_once(response: &'static str) -> (u16, JoinHandle<String>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = s.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(head_end) = text.find("\r\n\r\n") {
                let len = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                if buf.len() >= head_end + 4 + len {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        s.write_all(response.as_bytes()).unwrap();
        String::from_utf8_lossy(&buf).to_string()
    });
    (port, h)
}

fn api(port: u16) -> Api {
    Api::new(&Info { port, token: "secret".into() })
}

#[test]
fn get_sends_the_token_and_returns_json() {
    let (port, h) = serve_once("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 18\r\n\r\n{\"workspaces\":[1]}");
    let v = api(port).get("/api/state").unwrap();
    assert_eq!(v["workspaces"][0], 1);
    let req = h.join().unwrap().to_ascii_lowercase();
    assert!(req.starts_with("get /api/state "), "{req}");
    assert!(req.contains("x-harness-token: secret"), "{req}");
}

#[test]
fn post_sends_a_json_body() {
    let (port, h) = serve_once("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}");
    api(port).post("/api/ws/w/chats/c/send", Some(&serde_json::json!({"text": "héllo"}))).unwrap();
    let req = h.join().unwrap();
    assert!(req.starts_with("POST /api/ws/w/chats/c/send "), "{req}");
    assert!(req.contains(r#"{"text":"héllo"}"#), "{req}");
}

#[test]
fn error_message_comes_from_the_err_field() {
    let (port, _h) = serve_once("HTTP/1.1 400 Bad Request\r\ncontent-length: 14\r\n\r\n{\"err\":\"nope\"}");
    let e = api(port).get("/x").unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (400, "nope"));
}

#[test]
fn non_json_error_body_becomes_the_message() {
    let (port, _h) = serve_once("HTTP/1.1 500 Internal Server Error\r\ncontent-length: 4\r\n\r\nboom");
    let e = api(port).get("/x").unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (500, "boom"));
}

#[test]
fn unreachable_server_is_status_zero() {
    let e = Api::new(&Info { port: 1, token: "t".into() }).get("/x").unwrap_err();
    assert_eq!(e.status, 0);
}
