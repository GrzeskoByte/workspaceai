use serde_json::json;
use workspaceai_client::onboarding::*;

fn s(id: &str, dir: &str, updated: i64) -> Session {
    Session { id: id.into(), title: id.into(), dir: dir.into(), updated }
}

#[test]
fn text_helpers_match_the_web_app() {
    assert_eq!(slugify("My  Project!", "x"), "my-project");
    assert_eq!(slugify("!!", "project"), "project");
    assert_eq!(initials_of("  "), "?");
    assert_eq!(initials_of("alpha"), "A");
    assert_eq!(initials_of("castform labs inc"), "CL");
    assert_eq!(title_case_segment("/home/me/code/"), "Code");
    assert_eq!(time_ago(0, 30_000), "just now");
    assert_eq!(time_ago(0, 5 * 60_000), "5 min ago");
    assert_eq!(time_ago(0, 3 * 3_600_000), "3 h ago");
    assert_eq!(time_ago(0, 24 * 3_600_000), "yesterday");
    assert_eq!(time_ago(0, 3 * 86_400_000), "3 days ago");
    assert_eq!(time_ago(0, 8 * 86_400_000), "last week");
    assert_eq!(time_ago(0, 40 * 86_400_000), "last month");
}

#[test]
fn folders_nest_under_their_closest_parent() {
    let dirs: Vec<String> = ["/a/b/c", "/a", "/a/b", "/x", "/a"].iter().map(|d| d.to_string()).collect();
    let n = nest_folders(&dirs);
    assert_eq!(n.len(), 4);
    let parent = |d: &str| n.iter().find(|(x, _)| x == d).unwrap().1.clone();
    assert_eq!(parent("/a"), None);
    assert_eq!(parent("/a/b"), Some("/a".into()));
    assert_eq!(parent("/a/b/c"), Some("/a/b".into()));
    assert_eq!(parent("/x"), None);
    // "/ab" is not inside "/a"
    assert_eq!(nest_folders(&["/a".into(), "/ab".into()])[1].1, None);
}

#[test]
fn selection_counts_folders_and_sessions() {
    let all = vec![s("1", "/a", 3), s("2", "/a", 9), s("3", "/b", 1)];
    let g = group_sessions(&all);
    assert_eq!(g[0].items[0].id, "2"); // newest first
    let mut sel = Selection::default();
    assert_eq!(sel.count(&all), 3);
    sel.toggle_session("1");
    sel.toggle_dir("/b");
    assert_eq!(sel.count(&all), 1);
    assert_eq!(sel.ids(&all, false), vec!["1", "3"]);
    sel.select_all();
    assert_eq!(sel.count(&all), 3);
    sel.start_fresh(&all);
    assert_eq!(sel.count(&all), 0);
    assert_eq!(import_count_text(1), "1 chat will be imported");
    assert_eq!(target_text("/a/b", Some("/a"), true), "sub-project “B” inside A");
    assert_eq!(target_text("/a/b", None, false), "skipped");
}

#[test]
fn scan_and_check_replies_read_like_the_web_app() {
    let (list, err) = parse_scan(&json!({"sessions": [{"id": "x", "title": "T", "dir": "/d", "updated": 5}], "error": "partial"}));
    assert_eq!((list.len(), err.as_str()), (1, "partial"));
    assert_eq!(parse_scan(&json!({})).0.len(), 0);
    assert_eq!(remote_result(&json!({"ok": true, "version": "1.2", "sessions": 4, "folders": 2, "providers": 1})), (true, "Connected · opencode 1.2 · 4 sessions in 2 folders · 1 providers configured".into()));
    assert_eq!(remote_result(&json!({"ok": false})), (false, "Can’t reach that server".into()));
    assert_eq!(provider_result(&json!({"ok": true, "ms": 80}), "anthropic", 0).1, "Connected · 80 ms");
    assert_eq!(provider_result(&json!({"ok": true}), "ollama", 2).1, "Connected · 2 models ready");
    assert_eq!(provider_result(&json!({"ok": false}), "custom", 0).0, false);
    assert_eq!(web_result(&json!({"ok": true, "note": "SearXNG answered"})), (true, "SearXNG answered".into()));
    assert_eq!(detect_path("my bin"), "/api/onboarding/detect?binary=my%20bin");
    assert_eq!(scan_body(false, "oc", "", "", "")["mode"], "managed");
    assert_eq!(scan_body(true, "", "http://h", "u", "p")["url"], "http://h");
}

#[test]
fn provider_defaults_and_templates() {
    assert_eq!(provider_kind(None, true, 3), "existing");
    assert_eq!(provider_kind(None, true, 0), "opencode");
    assert_eq!(provider_kind(Some("ollama"), true, 3), "ollama");
    assert_eq!(provider_note("claude", false, true), "run “claude auth login” first");
    assert_eq!(agents_template("Blank", "P"), "# P\n");
    assert!(agents_template("Software", "P").starts_with("# P\n- Explain the plan"));
}

#[test]
fn continue_is_gated_per_step() {
    let mut g = Gate { ws_name: "Personal".into(), proj_name: "P".into(), proj_folder: "~/p".into(), ..Gate::default() };
    assert_eq!(blocked("engine", &g), Some("Install opencode, or connect to a server"));
    g.oc_has = true;
    assert_eq!(blocked("engine", &g), Some("Install Open Design, or choose Install later"));
    g.od_ready = true;
    assert_eq!(blocked("engine", &g), None);
    g.remote = true;
    g.oc_has = false;
    assert_eq!(blocked("engine", &g), Some("Connect to the server first"));
    g.ws_name = " ".into();
    assert_eq!(blocked("workspace", &g), Some("Give the workspace a name"));
    g.proj_folder.clear();
    assert!(blocked("project", &g).is_some());
    assert_eq!(blocked("model", &g), None);
    assert_eq!(next_label(0, false, 0, 0, false), "Get started");
    assert_eq!(next_label(5, false, 0, 0, false), "Set up later");
    assert_eq!(next_label(3, true, 2, 0, false), "Start fresh");
    assert_eq!(next_label(6, true, 0, 0, true), "Skip");
    assert_eq!(next_label(9, true, 0, 0, false), "Open WorkspaceAI");
    assert_eq!(next_label(1, true, 0, 0, false), "Continue");
}

fn draft() -> Draft {
    Draft { remote: false, binary: "opencode".into(), url: "http://127.0.0.1:4096".into(), user: "opencode".into(), pass: String::new(), ws_name: " Personal ".into(), accent: "default".into(), provider: "anthropic".into(), key: "k".into(), endpoint: "u".into(), model: "m".into(), ollama: vec!["qwen3-coder".into()], web_mode: "exa".into(), web_provider: "Tavily".into(), web_key: String::new(), searx: "http://localhost:8888".into(), web_fetch: true, web_ask: false, memory: "facts".into(), folders: vec!["~/code".into()], watch: true, proj_name: "P".into(), proj_folder: "~/code/p".into(), agents: "# P\n".into(), import_ids: vec!["1".into()], skipped_ids: vec!["2".into()], sync: true, theme: "claude".into(), reduce_motion: false, follow_system: false, subscriptions: vec!["grok".into()] }
}

#[test]
fn finish_body_has_the_shape_the_supervisor_reads() {
    let b = finish_body(&draft());
    assert_eq!(b["workspaceName"], "Personal");
    assert_eq!(b["engine"]["mode"], "managed");
    assert_eq!(b["provider"]["kind"], "anthropic");
    assert_eq!(b["provider"]["ollamaModels"], json!(["qwen3-coder"]));
    assert_eq!(b["graphifyFolders"], json!([])); // facts only: no folders
    assert_eq!(b["importIds"], json!(["1"]));
    assert_eq!(b["subscriptions"], json!(["grok"]));
    assert!(b.get("defaultAgent").is_none());
    let c = finish_body(&Draft { provider: "claude".into(), memory: "both".into(), subscriptions: vec!["claude".into()], ..draft() });
    assert_eq!(c["provider"], json!({"kind": "opencode"}));
    assert_eq!(c["defaultAgent"], "claude");
    assert_eq!(c["subscriptions"], json!(["claude"])); // no duplicate
    assert_eq!(c["graphifyFolders"], json!(["~/code"]));
}

#[test]
fn finish_toasts_and_summary() {
    assert_eq!(finish_toast(&json!({"imported": {"chats": 3, "failed": ["a", "b"]}})), "Setup done — 3 chats imported");
    assert_eq!(failed_toast(&json!({"imported": {"chats": 3, "failed": ["a", "b"]}})).unwrap(), "2 sessions could not be copied");
    assert_eq!(finish_toast(&json!({"imported": {"chats": 0}})), "Setup done — welcome to WorkspaceAI");
    assert!(failed_toast(&json!({})).is_none());
    let rows = summary(&draft(), "", false, 1, true);
    assert_eq!(rows[5].1, "Anthropic · tested");
    assert_eq!(rows[3].1, "install later");
    assert_eq!(rows[7].1, "Facts");
}

#[test]
fn open_design_status_text() {
    let st: OdStatus = serde_json::from_value(json!({"installing": true, "progress": 42.7})).unwrap();
    assert_eq!(st.text(false), "installing… 42%");
    assert_eq!(OdStatus::default().text(true), "will install later — agents can’t save artifacts until it is installed");
    let done = OdStatus { installed: true, version: Some("0.4".into()), ..OdStatus::default() };
    assert_eq!((done.pct(), done.text(false).as_str()), (100, "installed · 0.4, one daemon per workspace"));
    assert_eq!(detect_text(None), "looking for opencode…");
    assert_eq!(detect_text(Some(&Detect { found: true, path: Some("/bin/oc".into()), version: Some("1".into()) })), "found at /bin/oc · 1");
}
