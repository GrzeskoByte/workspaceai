//! Port of the project, layers, archive/restore, delete-subtree and instructions cases of `core.test.ts`.
//! Chats are inserted directly: creating a real opencode chat is B5's session code.

mod common;

use common::{create, setup};
use serde_json::json;
use std::path::PathBuf;
use workspaceai_shared::types::Chat;
use workspaceai_shared::util::now_ms;

fn id(v: &serde_json::Value) -> String {
    v["id"].as_str().expect("id").to_string()
}

fn add_chat(f: &common::Fixture, ws: &str, project: &str, title: &str) -> String {
    let chat = Chat {
        id: format!("chat_{}", now_ms()),
        project: project.into(),
        title: title.into(),
        sid: "ses_test".into(),
        agent: "opencode".into(),
        created_at: now_ms(),
        updated_at: now_ms(),
        ..Default::default()
    };
    let chat_id = chat.id.clone();
    f.sup
        .with_core_mut(|core| {
            let rt = core.rt_mut(ws).unwrap();
            rt.state.chats.push(chat);
            rt.changed().unwrap();
        })
        .unwrap();
    chat_id
}

#[test]
fn nests_sub_projects_to_any_depth_and_refuses_a_move_inside_itself() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let root = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Castform", "parent": null }))));
    let sub = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Simulation engine", "parent": root }))));
    let subsub = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Solver", "parent": sub }))));
    let leaf = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Kernels", "parent": subsub }))));

    let snap = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    let folder = snap["projects"].as_array().unwrap().iter().find(|p| p["id"] == leaf).unwrap()["folder"].as_str().unwrap().to_string();
    assert!(folder.ends_with("castform/simulation-engine/solver/kernels"), "folder: {folder}");
    assert!(PathBuf::from(&folder).is_dir());

    let err = f.call("PATCH", &format!("/api/ws/{a}/projects/{root}"), Some(json!({ "parent": leaf }))).unwrap_err();
    assert_eq!(err.status, 400);
    assert_eq!(err.message, "A project cannot move inside itself");

    let err = f.call("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "  ", "parent": null }))).unwrap_err();
    assert_eq!(err.message, "Name the project");
    let err = f.call("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Orphan", "parent": "nope" }))).unwrap_err();
    assert_eq!(err.message, "Parent project not found");
}

#[test]
fn layers_go_broad_to_narrow_and_inheritance_can_be_turned_off() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let root = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Castform" }))));
    let sub = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Sim", "parent": root }))));
    let leaf = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Solver", "parent": sub }))));
    f.ok("PUT", &format!("/api/ws/{a}/projects/{root}/agents"), Some(json!({ "text": "# Castform\n- millimetres" })));
    f.ok("PUT", &format!("/api/ws/{a}/projects/{sub}/agents"), Some(json!({ "text": "# Sim\n- seed randomness" })));
    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "instructions": "Answer in English." })));

    let layers = f.ok("GET", &format!("/api/ws/{a}/projects/{leaf}/layers"), None)["layers"].clone();
    let kinds: Vec<&str> = layers.as_array().unwrap().iter().map(|l| l["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["workspace", "parent", "parent", "project"]);
    assert_eq!(layers[0]["text"], "Answer in English.");
    assert!(layers[1]["text"].as_str().unwrap().contains("millimetres"));
    assert!(layers[1]["file"].as_str().unwrap().ends_with("AGENTS.md"));

    f.ok("PATCH", &format!("/api/ws/{a}/projects/{leaf}"), Some(json!({ "inherit": false })));
    let off = f.ok("GET", &format!("/api/ws/{a}/projects/{leaf}/layers"), None)["layers"].clone();
    let kinds: Vec<&str> = off.as_array().unwrap().iter().map(|l| l["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["workspace", "project"]);
    f.ok("PATCH", &format!("/api/ws/{a}/projects/{leaf}"), Some(json!({ "inherit": true })));

    // The agents file route answers the path and text that layers read.
    let agents = f.ok("GET", &format!("/api/ws/{a}/projects/{root}/agents"), None);
    assert!(agents["file"].as_str().unwrap().ends_with("AGENTS.md"));
    assert!(agents["text"].as_str().unwrap().contains("millimetres"));
}

#[test]
fn the_general_project_cannot_be_archived_deleted_or_moved() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let general = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None)["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["isGeneral"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let err = f.call("POST", &format!("/api/ws/{a}/projects/{general}/archive"), None).unwrap_err();
    assert_eq!(err.message, "General can’t be archived — quick chats live here.");
    let err = f.call("DELETE", &format!("/api/ws/{a}/projects/{general}"), None).unwrap_err();
    assert_eq!(err.message, "General can’t be deleted");
    let err = f.call("PATCH", &format!("/api/ws/{a}/projects/{general}"), Some(json!({ "parent": "x" }))).unwrap_err();
    assert_eq!(err.message, "General stays at the top level");
    let err = f.call("PATCH", &format!("/api/ws/{a}/projects/{general}"), Some(json!({ "name": "" }))).unwrap_err();
    assert_eq!(err.message, "Name the project");
}

#[test]
fn archive_hides_the_subtree_and_its_chats_and_restore_brings_them_back() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let root = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Castform" }))));
    let sub = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Sim", "parent": root }))));
    let subsub = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Solver", "parent": sub }))));
    let _leaf = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Kernels", "parent": subsub }))));
    let chat = add_chat(&f, &a, &subsub, "Solver questions");

    let res = f.ok("POST", &format!("/api/ws/{a}/projects/{sub}/archive"), None);
    assert_eq!(res["hiddenSubprojects"], 2);
    let after = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    assert!(!after["projects"].as_array().unwrap().iter().any(|p| p["id"] == subsub));
    assert!(!after["chats"].as_array().unwrap().iter().any(|c| c["id"] == chat.as_str()));

    // The archived list holds the top of the archived tree only.
    let archived = f.ok("GET", &format!("/api/ws/{a}/archived"), None);
    let listed: Vec<&str> = archived["projects"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(listed, [sub.as_str()]);

    f.ok("POST", &format!("/api/ws/{a}/projects/{sub}/restore"), None);
    let back = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    assert!(back["chats"].as_array().unwrap().iter().any(|c| c["id"] == chat.as_str()));
    assert!(back["projects"].as_array().unwrap().iter().any(|p| p["id"] == subsub));
    let archived = f.ok("GET", &format!("/api/ws/{a}/archived"), None);
    assert_eq!(archived["projects"].as_array().unwrap().len(), 0);
}

#[test]
fn deleting_a_project_removes_the_whole_subtree_but_keeps_the_folders() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let top = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Doomed" }))));
    let kid = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Doomed kid", "parent": top }))));
    let grandkid = id(&f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Doomed grandkid", "parent": kid }))));
    let live = add_chat(&f, &a, &kid, "Live chat");
    let old = add_chat(&f, &a, &grandkid, "Archived chat");

    let (folder, facts_file, art_dir, graph_file, general) = f
        .sup
        .with_core_mut(|core| {
            let rt = core.rt_mut(&a).unwrap();
            let art = workspaceai_shared::types::Artifact { id: "art_doomed".into(), project: top.clone(), title: "Doomed doc".into(), ..Default::default() };
            rt.state.artifacts.push(art);
            rt.state.graphs.push(json!({ "id": "g_doomed", "project": kid, "name": "Doomed graph", "file": format!("{}/graphs/doomed.bpmn", rt.project(&kid).unwrap().folder) }));
            rt.add_fact("Doomed fact", &kid, workspaceai_shared::types::FactStatus::Approved, None).unwrap();
            rt.state.projects.iter_mut().find(|p| p.id == grandkid).unwrap().archived = true;
            rt.ws.default_project = kid.clone();
            rt.changed().unwrap();
            let folder = PathBuf::from(&rt.project(&grandkid).unwrap().folder);
            let facts = rt.facts_file(&kid).unwrap();
            let art_dir = PathBuf::from(&rt.project(&top).unwrap().folder).join(".harness/artifacts/art_doomed");
            std::fs::create_dir_all(&art_dir).unwrap();
            let graph_file = PathBuf::from(&rt.project(&kid).unwrap().folder).join("graphs/doomed.bpmn");
            std::fs::create_dir_all(graph_file.parent().unwrap()).unwrap();
            std::fs::write(&graph_file, "<bpmn/>").unwrap();
            (folder, facts, art_dir, graph_file, rt.general().unwrap().id.clone())
        })
        .unwrap();
    assert!(facts_file.exists() && art_dir.exists() && graph_file.exists());

    f.ok("DELETE", &format!("/api/ws/{a}/projects/{top}"), None);

    let gone = |needle: &str| f.ok("GET", &format!("/api/ws/{a}/snapshot"), None)["projects"].as_array().unwrap().iter().all(|p| p["id"] != needle);
    assert!(gone(&top) && gone(&kid) && gone(&grandkid));
    let snap = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    assert!(!snap["chats"].as_array().unwrap().iter().any(|c| c["id"] == live.as_str() || c["id"] == old.as_str()));
    assert!(!snap["graphs"].as_array().unwrap().iter().any(|g| g["id"] == "g_doomed"));
    assert!(!snap["artifacts"].as_array().unwrap().iter().any(|x| x["id"] == "art_doomed"));
    assert!(!facts_file.exists());
    assert!(!art_dir.exists());
    assert!(folder.is_dir(), "the project folder stays");
    f.sup
        .with_core(|core| {
            let rt = core.rt(&a).unwrap();
            assert_eq!(rt.ws.default_project, general);
        })
        .unwrap();
}

#[test]
fn instructions_persist_and_do_not_rewrite_opencode_json() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    f.sup.with_core(|core| core.rt(&a).unwrap().write_config()).unwrap().unwrap();

    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "instructions": "Never paste secrets." })));
    let snap = f.ok("GET", &format!("/api/ws/{a}/snapshot"), None);
    assert_eq!(snap["workspace"]["instructions"], "Never paste secrets.");
    // Instructions are not part of opencode.json, so changing them does not regenerate it.
    let config = f.path().join("ws").join(&a).join("config/opencode.json");
    let before = std::fs::read_to_string(&config).unwrap();
    f.ok("PATCH", &format!("/api/ws/{a}"), Some(json!({ "instructions": "Answer briefly." })));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), before);

    // A second supervisor over the same home reads them back.
    let sup = workspaceai_supervisor::Supervisor::new(f.path().to_path_buf()).unwrap();
    let snap = sup.handle("GET", &format!("/api/ws/{a}/snapshot"), &Default::default(), None).unwrap();
    assert_eq!(snap["workspace"]["instructions"], "Answer briefly.");
}
