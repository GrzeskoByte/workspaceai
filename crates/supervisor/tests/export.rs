//! Port of the export case in `core.test.ts`, including the secrets rule: nothing from the keychain,
//! no engine credentials and no private keys may appear in the archive.

mod common;

use common::{create, setup};
use flate2::read::GzDecoder;
use serde_json::json;
use std::io::Read;

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut tar = tar::Archive::new(GzDecoder::new(bytes));
    let mut out = Vec::new();
    for entry in tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().into_owned();
        let mut body = Vec::new();
        entry.read_to_end(&mut body).unwrap();
        out.push((path, body));
    }
    out
}

#[test]
fn exports_config_projects_and_memory_but_never_secrets() {
    let f = setup();
    let a = create(&f, "Castform Labs", "team");
    let root = f.ok("POST", &format!("/api/ws/{a}/projects"), Some(json!({ "name": "Castform" })))["id"].as_str().unwrap().to_string();
    f.ok("PUT", &format!("/api/ws/{a}/projects/{root}/agents"), Some(json!({ "text": "# Castform\n- millimetres" })));
    f.ok("POST", &format!("/api/ws/{a}/facts"), Some(json!({ "text": "Wall is 8 mm.", "scope": "ws" })));

    let secret = "sk-test-aaaaaaaaaaaaaaaa";
    let leak = "sk-live-supersecretvalue123";
    f.sup
        .with_core(|core| {
            let rt = core.rt(&a).unwrap();
            rt.write_config().unwrap();
            core.keychain.set(&a, "MY_KEY", secret).unwrap();
            std::fs::write(rt.paths.config.join("auth.json"), json!({ "token": leak }).to_string()).unwrap();
            std::fs::write(rt.paths.config.join("leaky.txt"), format!("key={secret}")).unwrap();
            std::fs::write(rt.paths.config.join("issuer.json"), "{}").unwrap();
        })
        .unwrap();

    let archive = f.sup.export_workspace(&a).unwrap();
    assert_eq!(archive.content_type, "application/gzip");
    assert_eq!(archive.filename, "castform-labs.tar.gz");

    let all = entries(&archive.bytes);
    let names: Vec<String> = all.iter().map(|(n, _)| n.clone()).collect();
    assert!(names.iter().any(|n| n.ends_with("config/opencode.json")), "names: {names:?}");
    assert!(names.iter().any(|n| n == &format!("castform-labs/projects/{root}/AGENTS.md")));
    assert!(names.iter().any(|n| n == "castform-labs/memory/facts.md"));
    assert!(names.iter().any(|n| n == "castform-labs/data/state.json"));
    let agents = all.iter().find(|(n, _)| n.ends_with("AGENTS.md")).unwrap();
    assert!(String::from_utf8_lossy(&agents.1).contains("millimetres"));

    // Credential files and anything carrying a secret value are left out entirely.
    assert!(!names.iter().any(|n| n.ends_with("auth.json") || n.ends_with("issuer.json") || n.ends_with("leaky.txt")), "names: {names:?}");
    let text: String = all.iter().map(|(_, b)| String::from_utf8_lossy(b).into_owned()).collect();
    assert!(!text.contains(secret));
    assert!(!text.contains(leak));
    assert!(!text.contains("BEGIN PRIVATE KEY"));

    // The export itself is audited.
    let audit = f.ok("GET", &format!("/api/ws/{a}/audit"), None);
    assert!(audit["events"].as_array().unwrap().iter().any(|e| e["action"] == "workspace.export"));
}

#[test]
fn exporting_a_workspace_without_projects_still_works() {
    let f = setup();
    let a = create(&f, "Personal", "personal");
    let archive = f.sup.export_workspace(&a).unwrap();
    let names: Vec<String> = entries(&archive.bytes).into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().any(|n| n.ends_with("data/state.json")), "names: {names:?}");
    let err = f.sup.export_workspace("missing").unwrap_err();
    assert_eq!(err.status, 404);
    assert_eq!(err.message, "Workspace not found");
}
