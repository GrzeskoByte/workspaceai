use serde_json::json;
use workspaceai_client::types::ChatMessage;
use workspaceai_client::web::*;

#[test]
fn addresses_become_urls() {
    assert_eq!(to_url("5173"), "http://localhost:5173/");
    assert_eq!(to_url(" localhost:3000/x "), "http://localhost:3000/x");
    assert_eq!(to_url("127.0.0.1:8080"), "http://127.0.0.1:8080");
    assert_eq!(to_url("app.localhost"), "http://app.localhost");
    assert_eq!(to_url("example.com"), "https://example.com");
    assert_eq!(to_url("HTTP://Example.com"), "HTTP://Example.com");
    assert_eq!(to_url("javascript:alert(1)"), "https://javascript:alert(1)", "only http(s) ever comes out");
    assert_eq!(to_url("  "), "");
}

#[test]
fn finds_the_first_local_address() {
    assert_eq!(local_url_in("Running at http://localhost:5173/ now."), Some("http://localhost:5173/".into()));
    assert_eq!(local_url_in("ready on http://0.0.0.0:3000."), Some("http://localhost:3000".into()));
    assert_eq!(local_url_in("see (http://127.0.0.1:8000/docs)"), Some("http://127.0.0.1:8000/docs".into()));
    assert_eq!(local_url_in("https://example.com then http://localhost:1"), Some("http://localhost:1".into()));
    assert_eq!(local_url_in("https://example.com only"), None);
    assert_eq!(short_url("http://localhost:5173/"), "localhost:5173");
}

#[test]
fn native_formats_stay_native() {
    assert_eq!(web_doc("markdown", "Doc", "# hi", "<p>page</p>"), None);
    assert_eq!(web_doc("csv", "Sheet", "a,b", "<table>"), None);
    assert_eq!(web_doc("markdown", "Deck", "# s", "<deck>"), Some("<deck>".into()), "decks show Open Design's page");
    assert_eq!(web_doc("html", "Prototype", "<h1>src</h1>", ""), Some("<h1>src</h1>".into()), "no page: the HTML source itself");
    assert_eq!(web_doc("json", "Doc", "{}", ""), None);
    assert_eq!(page_of(&json!({"content":"x","html":"<b>p</b>"})), "<b>p</b>");
    assert_eq!(page_of(&json!({"content":"x"})), "");
}

#[test]
fn host_page_sandboxes_the_artifact() {
    let h = host_page(r#"<body><a href="x">"hi" & bye</a></body>"#, false, true);
    assert!(h.contains(r#"<iframe sandbox="allow-scripts" srcdoc=""#));
    assert!(!h.contains("allow-same-origin"), "the page must get an opaque origin");
    assert!(h.contains("&lt;a href=&quot;x&quot;&gt;&quot;hi&quot; &amp; bye"), "the document is escaped into the attribute: {h}");
    assert!(h.contains("leave-fullscreen"), "Esc is relayed out of the frame");
    assert!(h.contains("width:390px"), "mobile narrows the frame");
    assert!(host_page("x", true, true).contains("aspect-ratio:16/9"), "decks keep 16:9");
}

#[test]
fn dev_server_reply_and_chips() {
    let d = parse_dev(&json!({"command":"npm run dev","running":true,"url":"http://localhost:5173/","log":["ok"],
        "servers":[{"port":5173,"url":"http://localhost:5173/","process":"node","pid":1},{"port":8080,"url":"http://localhost:8080/","process":"py","pid":2}]})).unwrap();
    assert!(d.running);
    assert_eq!(server_chips(&d), vec![("http://localhost:5173/".into(), "localhost:5173".into()), ("http://localhost:8080/".into(), "localhost:8080".into())]);
    assert_eq!(start_body("  "), None);
    assert_eq!(start_body(" npm run dev ").unwrap()["command"], "npm run dev");
    assert_eq!(stop_body()["action"], "stop");
    assert_eq!(dev_path("w 1", "p"), "/api/ws/w%201/projects/p/dev");
}

#[test]
fn messages_carry_their_artifact_and_tool_output() {
    let m: ChatMessage = serde_json::from_value(json!({"id":"m","role":"assistant","artifact":"a1","tools":[{"name":"bash","status":"ok","output":"listening on http://localhost:4000"}]})).unwrap();
    assert_eq!(m.artifact.as_deref(), Some("a1"));
    assert_eq!(m.tools[0].output.as_deref(), Some("listening on http://localhost:4000"));
}
