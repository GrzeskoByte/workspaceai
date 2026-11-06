//! Port of `policy.ts`. Organization policy: signed by the org, verified before use, last good copy kept (R-36).
//! Ed25519 over `canonical()` of the policy object, which is byte-for-byte what the TS `canonical` builds
//! (sorted keys, `JSON.stringify` strings and numbers), so policies signed on either side verify on the other.
//! Signing and verification work on the raw JSON value: a policy file is never re-serialized through a struct
//! before it is checked, so fields this build does not know cannot break a signature.

use crate::jsonfile::{read_json, write_json};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::pkcs8::{spki::der::pem::LineEnding, DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Number, Value};
use std::path::Path;
use workspaceai_shared::types::OrgPolicy;

/// A JS number as `JSON.stringify` writes it (ECMAScript Number::toString): `1` not `1.0`, `1e+21`, `1e-7`, `0` for -0.
pub fn js_number(n: &Number) -> String {
    if n.is_i64() || n.is_u64() {
        return n.to_string();
    }
    let x = n.as_f64().unwrap_or(0.0);
    if !x.is_finite() {
        return "null".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // `{:e}` gives the shortest digits that round-trip, as `d.ddde<exp>`.
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap() + 1; // position of the decimal point
    let body = if k <= n && n <= 21 {
        digits + &"0".repeat((n - k) as usize)
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let es = if e >= 0 { format!("+{e}") } else { e.to_string() };
        if k == 1 { format!("{digits}e{es}") } else { format!("{}.{}e{es}", &digits[..1], &digits[1..]) }
    };
    format!("{sign}{body}")
}

/// JSON with object keys sorted (by UTF-16 code units, as JS `sort()` does), no whitespace.
pub fn canonical(v: &Value) -> String {
    match v {
        Value::Array(a) => format!("[{}]", a.iter().map(canonical).collect::<Vec<_>>().join(",")),
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            let parts: Vec<String> = keys.iter().map(|k| format!("{}:{}", Value::String((*k).clone()), canonical(&o[*k]))).collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Number(n) => js_number(n),
        // serde_json escapes strings exactly like JSON.stringify (", \, \b \f \n \r \t, other controls as \u00xx).
        other => other.to_string(),
    }
}

/// `{ policy, signature, keyId }`, signature = base64 Ed25519 over `canonical(policy)`.
pub fn sign_policy(policy: &Value, private_key_pem: &str, key_id: &str) -> Result<Value, String> {
    let key = SigningKey::from_pkcs8_pem(private_key_pem).map_err(|e| e.to_string())?;
    let sig = key.sign(canonical(policy).as_bytes());
    Ok(json!({ "policy": policy, "signature": B64.encode(sig.to_bytes()), "keyId": key_id }))
}

pub fn verify_policy(signed: &Value, public_key_pem: &str) -> bool {
    let check = || -> Option<bool> {
        let key = VerifyingKey::from_public_key_pem(public_key_pem).ok()?;
        let sig = Signature::from_slice(&B64.decode(signed.get("signature")?.as_str()?).ok()?).ok()?;
        Some(key.verify(canonical(signed.get("policy")?).as_bytes(), &sig).is_ok())
    };
    check().unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issuer {
    pub public_key: String,
    pub private_key: String,
    pub key_id: String,
}

fn base36(mut n: u64) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn generate_issuer(key_id: &str) -> Result<Issuer, String> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| e.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    Ok(Issuer {
        public_key: key.verifying_key().to_public_key_pem(LineEnding::LF).map_err(|e| e.to_string())?,
        private_key: key.to_pkcs8_pem(LineEnding::LF).map_err(|e| e.to_string())?.to_string(),
        key_id: key_id.into(),
    })
}

/// The demo organization ("BSC AI Factory") is simulated locally: its issuer key lives under `<home>/demo-org`.
/// A real org publishes its policy and public key through its identity provider; only the fetch differs.
pub fn demo_issuer(home: &Path) -> Result<Issuer, String> {
    let file = home.join("demo-org/issuer.json");
    if let Some(existing) = read_json::<Issuer>(&file) {
        return Ok(existing);
    }
    let out = generate_issuer(&format!("demo-org-{}", base36(workspaceai_shared::util::now_ms() as u64)))?;
    write_json(&file, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct PolicyCheck {
    pub ok: bool,
    pub policy: Option<OrgPolicy>,
    pub reason: Option<String>,
}

/// Verify a freshly fetched policy; if it fails, keep the last good one and block sending.
pub fn accept_policy(fetched: Option<&Value>, pinned_public_key: &str, last_good: Option<OrgPolicy>) -> PolicyCheck {
    if let Some(f) = fetched {
        if verify_policy(f, pinned_public_key) {
            if let Some(p) = f.get("policy").and_then(|p| serde_json::from_value(p.clone()).ok()) {
                return PolicyCheck { ok: true, policy: Some(p), reason: None };
            }
        }
    }
    let reason = if fetched.is_some() {
        "Policy signature did not verify; keeping the last good policy and blocking sends."
    } else {
        "Could not fetch policy; keeping the last good one."
    };
    PolicyCheck { ok: false, policy: last_good, reason: Some(reason.into()) }
}
