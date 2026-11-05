use std::io::{Read, Write};
use std::net::TcpListener;
use workspaceai_client::api::Api;
use workspaceai_client::events::{parse_sse_line, stream};
use workspaceai_client::supervisor::Info;
use workspaceai_client::types::UiEvent;

#[test]
fn delta_tool_done_and_toast_events_parse() {
    let d = parse_sse_line(r#"data: {"type":"chat.delta","workspace":"w","chat":"c","messageId":"m","delta":"Hel"}"#).unwrap();
    assert_eq!(d, UiEvent::ChatDelta { workspace: "w".into(), chat: "c".into(), message_id: "m".into(), delta: "Hel".into() });
    assert!(matches!(parse_sse_line(r#"data: {"type":"chat.done","workspace":"w","chat":"c","outcome":"ok"}"#), Some(UiEvent::ChatDone { .. })));
    assert!(matches!(parse_sse_line(r#"data: {"type":"chat.tool","workspace":"w","chat":"c","messageId":"m","tool":{"name":"bash","status":"running"}}"#), Some(UiEvent::ChatTool { .. })));
    assert_eq!(parse_sse_line(r#"data: {"type":"toast","text":"hi"}"#), Some(UiEvent::Toast { text: "hi".into() }));
    assert_eq!(parse_sse_line(r#"data: {"type":"state","workspace":null}"#), Some(UiEvent::State { workspace: None }));
    assert!(matches!(parse_sse_line(r#"data: {"type":"snapshot","workspace":"w"}"#), Some(UiEvent::Snapshot { .. })));
}

#[test]
fn unknown_event_types_are_other_not_errors() {
    assert_eq!(parse_sse_line(r#"data: {"type":"graph.run","workspace":"w","run":{}}"#), Some(UiEvent::Other));
}

#[test]
fn comments_blanks_and_junk_are_ignored() {
    for l in ["", ": keep-alive", "event: x", "data: not json", "data: {\"no_type\":1}"] {
        assert_eq!(parse_sse_line(l), None, "{l:?}");
    }
}

#[test]
fn stream_yields_events_until_the_server_closes() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = s.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n: hi\n\ndata: {\"type\":\"toast\",\"text\":\"a\"}\n\ndata: {\"type\":\"toast\",\"text\":\"b\"}\n\n").unwrap();
        req
    });
    let api = Api::new(&Info { port, token: "tok".into() });
    let mut seen = Vec::new();
    stream(&api, |e| seen.push(e)).unwrap();
    assert_eq!(seen, vec![UiEvent::Toast { text: "a".into() }, UiEvent::Toast { text: "b".into() }]);
    let req = h.join().unwrap().to_ascii_lowercase();
    assert!(req.starts_with("get /api/events "), "{req}");
    assert!(req.contains("x-harness-token: tok"), "{req}");
    assert!(!req.contains("token=tok"), "token must not travel in the URL: {req}");
}

#[test]
fn stream_reports_an_unreachable_supervisor() {
    let api = Api::new(&Info { port: 1, token: "s3cr3t-token".into() });
    let err = stream(&api, |_| {}).unwrap_err();
    assert!(!err.contains("s3cr3t-token"), "error text must not leak the token: {err}");
}

#[test]
fn permission_request_and_clear_events_parse() {
    let req = parse_sse_line(r#"data: {"type":"chat.permission","workspace":"w","chat":"c","request":{"id":"r1","sid":"s","title":"Run bash","detail":"ls"}}"#).unwrap();
    assert!(matches!(req, UiEvent::ChatPermission { ref request, .. } if request.id == "r1" && request.detail == "ls"));
    assert_eq!(
        parse_sse_line(r#"data: {"type":"chat.permission.clear","workspace":"w","chat":"c","requestId":"r1"}"#),
        Some(UiEvent::ChatPermissionClear { workspace: "w".into(), chat: "c".into(), request_id: Some("r1".into()) })
    );
    assert_eq!(
        parse_sse_line(r#"data: {"type":"chat.permission.clear","workspace":"w","chat":"c"}"#),
        Some(UiEvent::ChatPermissionClear { workspace: "w".into(), chat: "c".into(), request_id: None })
    );
}
