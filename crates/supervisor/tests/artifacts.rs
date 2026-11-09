//! Port of `supervisor/test/artifacts.test.ts` (R-04, R-13, R-14) plus the agent-tool routes from `extensions.ts`.
//! Open Design is B10: the case that needs its daemon skips with a printed reason; the built-in store covers the rest.

mod common;

use common::{serve, Served};
use serde_json::{json, Value};
use std::path::PathBuf;
use workspaceai_shared::util::descendants;

const DECK: &str = "<!doctype html><html><head><style>body{margin:0;font-family:sans-serif}</style></head><body><section class=\"slide\">Castform beta</section><section class=\"slide\">8 mm walls</section></body></html>";

fn contents() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Doc", "# Mold notes\n\nMinimum wall is 8 mm.\n\n- Mold Star 30\n- Degas twice"),
        ("Deck", DECK),
        ("Prototype", "<!doctype html><html><body><h1>Beta landing page</h1><button>Join</button></body></html>"),
        ("Sheet", "part,wall_mm\nbase,8\nlid,6\n"),
        ("Image", "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"40\"><circle cx=\"20\" cy=\"20\" r=\"18\" fill=\"#cc785c\"/></svg>"),
    ]
}

struct Ws {
    w: String,
    p: String,
    s: String,
    q: String,
}

fn workspace(f: &Served) -> Ws {
    let w = f.ok("POST", "/api/workspaces", Some(json!({ "name": "Art", "kind": "personal" })))["id"].as_str().unwrap().to_string();
    let project = |name: &str, parent: Value| f.ok("POST", &format!("/api/ws/{w}/projects"), Some(json!({ "name": name, "parent": parent })))["id"].as_str().unwrap().to_string();
    let p = project("Castform", Value::Null);
    let s = project("Simulation engine", json!(p));
    let q = project("Marketing", Value::Null);
    Ws { w, p, s, q }
}

/// A claude session: no opencode server needed.
fn session(f: &Served, w: &str, project: &str, title: &str) -> String {
    f.sup
        .with_core_mut(|c| {
            let rt = c.rt_mut(w).unwrap();
            if let Some(a) = rt.ws.agents.iter_mut().find(|a| a.id == "claude") {
                a.on = true;
            }
        })
        .unwrap();
    f.ok("POST", &format!("/api/ws/{w}/chats"), Some(json!({ "project": project, "title": title, "agent": "claude" })))["id"].as_str().unwrap().to_string()
}

fn art_dir(f: &Served, w: &str, project: &str, aid: &str) -> PathBuf {
    f.sup.with_core(|c| c.rt(w).unwrap().artifact_dir(aid, project).unwrap()).unwrap()
}

#[test]
fn artifacts_follow_their_chat_sub_project_placement_move_with_the_chat_roll_up_library_needs_a_project() {
    let f = serve();
    let Ws { w, p, s, q } = workspace(&f);
    let chat = session(&f, &w, &s, "Mold math");
    let a = f.ok("POST", &format!("/api/ws/{w}/artifacts"), Some(json!({ "chat": chat, "type": "Doc", "title": "Mold notes", "content": contents()[0].1 })));
    let aid = a["id"].as_str().unwrap().to_string();
    assert_eq!(a["project"], json!(s));
    assert!(art_dir(&f, &w, &s, &aid).join("v1.md").is_file());
    assert!(art_dir(&f, &w, &s, &aid).join("manifest.json").is_file());
    // Rolls up: Castform's Artifacts tab lists what its sub-projects made.
    let rolled = f
        .sup
        .with_core(|c| {
            let rt = c.rt(&w).unwrap();
            let mut scope = descendants(&rt.state.projects, &p);
            scope.push(p.clone());
            rt.state.artifacts.iter().filter(|x| scope.contains(&x.project)).map(|x| x.id.clone()).collect::<Vec<_>>()
        })
        .unwrap();
    assert!(rolled.contains(&aid));
    // Moves with the chat, files included.
    assert_eq!(f.ok("POST", &format!("/api/ws/{w}/chats/{chat}/move"), Some(json!({ "project": q })))["movedArtifacts"], 1);
    assert_eq!(f.sup.with_core(|c| c.rt(&w).unwrap().artifact(&aid).unwrap().project.clone()).unwrap(), q);
    assert!(art_dir(&f, &w, &q, &aid).join("v1.md").is_file());
    assert!(!art_dir(&f, &w, &s, &aid).exists());
    // The library must pick a project.
    let loose = f.sup.handle("POST", &format!("/api/ws/{w}/artifacts"), &Default::default(), Some(json!({ "type": "Doc", "title": "Loose", "content": "x" })));
    assert!(loose.unwrap_err().status >= 400);
}

#[test]
fn each_type_is_made_from_a_chat_and_the_library_versioned_by_open_design_and_exported() {
    eprintln!("skipped: needs the Open Design daemon (B10 ports opendesign.rs); the built-in store case below covers create/version/export");
}

#[test]
fn each_type_is_made_from_a_chat_and_the_library_versioned_and_exported_by_the_builtin_store() {
    let f = serve();
    let Ws { w, p, s, .. } = workspace(&f);
    let chat = session(&f, &w, &s, "Types");
    for (kind, content) in contents() {
        for from in ["chat", "library"] {
            let place = if from == "chat" { json!({ "chat": chat }) } else { json!({ "project": p }) };
            let mut body = json!({ "type": kind, "title": format!("{kind} from {from}"), "content": content });
            body.as_object_mut().unwrap().extend(place.as_object().unwrap().clone());
            let a = f.ok("POST", &format!("/api/ws/{w}/artifacts"), Some(body));
            assert_eq!(a["type"], kind);
            assert_eq!(a["versions"][0]["od"], Value::Null, "{kind}: built-in store until B10");
            let aid = a["id"].as_str().unwrap();
            let v2 = f.ok("POST", &format!("/api/ws/{w}/artifacts/{aid}/versions"), Some(json!({ "content": content.replace('8', "9"), "note": "edit" })));
            assert_eq!(v2["version"], 2);
            let formats = f.ok("GET", &format!("/api/ws/{w}/artifacts/{aid}/exports"), None)["formats"].as_array().unwrap().clone();
            for fm in formats {
                let fm = fm["format"].as_str().unwrap();
                let out = f.sup.export_artifact(&w, aid, fm, None);
                if fm == "pptx" {
                    assert!(out.unwrap_err().message.contains("Open Design"), "pptx needs Open Design");
                    continue;
                }
                let out = out.unwrap_or_else(|e| panic!("{kind} → {fm}: {}", e.message));
                assert!(out.body.len() > 20, "{kind} → {fm}");
                assert!(out.filename.contains("-v2."), "{kind} → {fm}: {}", out.filename);
                if fm == "pdf" {
                    assert!(String::from_utf8_lossy(&out.body).contains("window.print()"), "{kind}: PDF is the print page");
                }
            }
            // The content route serves the latest version and its page.
            let view = f.ok("GET", &format!("/api/ws/{w}/artifacts/{aid}/content"), None);
            assert_eq!(view["version"], 2);
            assert!(view["html"].as_str().unwrap().len() > 20);
            // Restoring v1 makes v3 with v1's content (R-14).
            let r = f.ok("POST", &format!("/api/ws/{w}/artifacts/{aid}/restore"), Some(json!({ "v": 1 })));
            assert_eq!(r["version"], 3);
            assert_eq!(f.ok("GET", &format!("/api/ws/{w}/artifacts/{aid}/content"), None)["content"], content);
        }
    }
    let audit = f.ok("GET", &format!("/api/ws/{w}/audit"), None).to_string();
    assert!(audit.contains("artifact.export"));
    assert!(audit.contains("via harness"));
}

#[test]
fn sharing_publishes_a_static_link_that_needs_no_token_and_off_means_no_link() {
    let f = serve();
    let Ws { w, p, .. } = workspace(&f);
    let a = f.ok("POST", &format!("/api/ws/{w}/artifacts"), Some(json!({ "project": p, "type": "Prototype", "title": "Landing", "content": contents()[2].1 })));
    let aid = a["id"].as_str().unwrap();
    f.ok("PATCH", &format!("/api/ws/{w}"), Some(json!({ "publicLinks": true })));
    let shared = f.ok("POST", &format!("/api/ws/{w}/artifacts/{aid}/share"), None);
    assert_eq!(shared["scope"], "link");
    let url = shared["url"].as_str().unwrap();
    assert!(url.starts_with(&format!("{}/api/shared/{w}/", f.server.url())));
    let res = ureq::get(url).call().expect("shared page without a token");
    assert_eq!(res.status(), 200);
    assert!(res.header("content-security-policy").unwrap().contains("sandbox"));
    assert_eq!(res.header("x-content-type-options"), Some("nosniff"));
    assert!(res.into_string().unwrap().contains("Beta landing page"));
    let nope = format!("{}/nope", &url[..url.rfind('/').unwrap()]);
    match ureq::get(&nope).call() {
        Err(ureq::Error::Status(404, _)) => {}
        other => panic!("removed link should be 404: {other:?}"),
    }
    f.ok("PATCH", &format!("/api/ws/{w}"), Some(json!({ "publicLinks": false })));
    let off = f.ok("POST", &format!("/api/ws/{w}/artifacts/{aid}/share"), None);
    assert_eq!((off["scope"].as_str(), off.get("url")), (Some("private"), None));
}

#[test]
fn agent_tools_need_the_guide_first_and_land_in_the_calling_sessions_project() {
    let f = serve();
    let Ws { w, s, .. } = workspace(&f);
    let chat = session(&f, &w, &s, "Agent");
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    f.sup.with_core(|c| c.on(std::sync::Arc::new(move |e| sink.lock().unwrap().push(e)))).unwrap();
    let create = json!({ "chat": chat, "type": "doc", "title": "Plan", "content": "# Plan", "format": "markdown" });
    let first = f.sup.handle("POST", &format!("/api/ws/{w}/agent/artifacts"), &Default::default(), Some(create.clone()));
    assert!(first.unwrap_err().message.contains("artifact_guide"), "guide first");
    let guide = f.ok("POST", &format!("/api/ws/{w}/agent/guide"), Some(json!({ "chat": chat, "type": "doc" })));
    assert!(guide["guide"].as_str().unwrap().contains("## Format"));
    let a = f.ok("POST", &format!("/api/ws/{w}/agent/artifacts"), Some(create));
    assert_eq!(a["project"], json!(s));
    assert_eq!(a["chat"], json!(chat));
    assert_eq!(a["projectPath"], "Castform › Simulation engine");
    let aid = a["id"].as_str().unwrap();
    let v2 = f.ok("POST", &format!("/api/ws/{w}/agent/artifacts/{aid}"), Some(json!({ "chat": chat, "content": "# Plan v2", "title": "Plan B" })));
    assert_eq!((v2["version"].as_i64(), v2["title"].as_str()), (Some(2), Some("Plan B")));
    let seen: Vec<_> = events.lock().unwrap().iter().filter_map(|e| match e {
        workspaceai_supervisor::UiEvent::Artifact { artifact, version, .. } => Some((artifact.clone(), *version)),
        _ => None,
    }).collect();
    assert_eq!(seen, vec![(aid.to_string(), 1), (aid.to_string(), 2)]);
    // Without a session the artifact goes to the workspace's default project.
    f.ok("POST", &format!("/api/ws/{w}/agent/guide"), Some(json!({ "type": "sheet" })));
    let loose = f.ok("POST", &format!("/api/ws/{w}/agent/artifacts"), Some(json!({ "type": "sheet", "title": "Parts", "content": "a,b\n1,2" })));
    let default_project = f.sup.with_core(|c| c.rt(&w).unwrap().ws.default_project.clone()).unwrap();
    assert_eq!(loose["project"], json!(default_project));
    assert_eq!(loose["chat"], Value::Null);
}
