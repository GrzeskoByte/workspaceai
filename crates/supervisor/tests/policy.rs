//! Policy canonicalisation and cross-language signatures (B2 acceptance): a policy signed by the TS
//! `signPolicy` verifies in Rust, and one signed by Rust verifies in TS `verifyPolicy`.

use serde_json::{json, Number, Value};
use workspaceai_supervisor::policy::{canonical, generate_issuer, js_number, sign_policy, verify_policy};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[test]
fn js_numbers_match_json_stringify() {
    // (input literal, what ECMAScript Number::toString writes)
    let cases = [
        ("0", "0"),
        ("-0", "0"),
        ("1", "1"),
        ("1.5", "1.5"),
        ("1e21", "1e+21"),
        ("1e-7", "1e-7"),
        ("1e-6", "0.000001"),
        ("1e300", "1e+300"),
        ("123456789012345678901", "123456789012345680000"),
        ("2.5e-10", "2.5e-10"),
    ];
    for (input, want) in cases {
        let n: Number = serde_json::from_str(input).unwrap_or_else(|e| panic!("parse {input}: {e}"));
        assert_eq!(js_number(&n), want, "input {input}");
    }
}

#[test]
fn canonical_sorts_keys_like_js_and_keeps_strings_escaped() {
    let v = json!({ "b": 1, "a": 2, "ä": 3, "Z": 4, "🙂": 5, "0": 6 });
    assert_eq!(canonical(&v), r#"{"0":6,"Z":4,"a":2,"b":1,"ä":3,"🙂":5}"#);
    assert_eq!(canonical(&json!({ "x": "a\"b\\c\n" })), r#"{"x":"a\"b\\c\n"}"#);
    assert_eq!(canonical(&json!([3, 2, { "z": 1, "a": 0 }])), r#"[3,2,{"a":0,"z":1}]"#);
    assert_eq!(canonical(&json!(null)), "null");
    assert_eq!(canonical(&json!(true)), "true");
    // Non-ASCII keys are ordered by UTF-16 code units, so the surrogate pair sorts last.
    let keys = json!({ "🙂": 1, "Ω": 2, "中": 3, "a": 4 });
    assert_eq!(canonical(&keys), r#"{"a":4,"Ω":2,"中":3,"🙂":1}"#);
}

#[test]
fn ts_signed_policy_verifies_in_rust() {
    let signed: Value = serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/ts-signed-policy.json")).expect("fixture")).expect("json");
    let pem = std::fs::read_to_string(format!("{FIXTURES}/public.pem")).expect("public key");
    assert!(verify_policy(&signed, &pem), "TS signature must verify in Rust");

    let mut tampered = signed;
    tampered["policy"]["logContent"] = json!(true);
    assert!(!verify_policy(&tampered, &pem), "a changed policy must not verify");
    assert!(!verify_policy(&tampered, "not a pem"));
}

#[test]
fn rust_signed_policy_verifies_in_ts() {
    let issuer = generate_issuer("rust-fixture-key").expect("issuer");
    let policy = json!({
        "version": "2026.10.1",
        "org": "Rust · Ünïcode",
        "ключ": "значение",
        "emoji🙂": "ok",
        "numbers": [0.0, -0.0, 1.0, 1.5, 1e21, 1e-7, 1e-6, 1e300, 123456789012345678901.0, 2.5e-10],
        "nested": { "Ω": { "中": true, "🙂": null }, "control": "tab\tnewline\nquote\"backslash\\" },
        "logContent": false,
    });
    let signed = sign_policy(&policy, &issuer.private_key, &issuer.key_id).expect("sign");
    assert!(verify_policy(&signed, &issuer.public_key));

    let dir = tempfile::tempdir().expect("temp");
    let signed_path = dir.path().join("policy.json");
    let public_path = dir.path().join("public.pem");
    std::fs::write(&signed_path, serde_json::to_string_pretty(&signed).unwrap()).unwrap();
    std::fs::write(&public_path, &issuer.public_key).unwrap();

    let script = format!("{FIXTURES}/verify-ts-policy.ts");
    let output = std::process::Command::new("node").arg(&script).arg(&signed_path).arg(&public_path).output();
    match output {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping Rust→TS policy check: node is not on PATH");
        }
        Err(e) => panic!("running node: {e}"),
        Ok(out) => assert!(
            out.status.success(),
            "TS verifyPolicy rejected the Rust signature\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}
