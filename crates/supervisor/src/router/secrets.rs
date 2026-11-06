//! `/api/ws/:id/secrets` — values live in the keychain and are never echoed (R-34).

use crate::secrets::{blocked_in_org, listing, normalize_name};
use crate::supervisor::{ApiError, Core};
use serde_json::{json, Value};

pub(super) fn list(core: &Core, id: &str) -> Result<Value, ApiError> {
    let rt = core.rt(id)?;
    Ok(listing(rt.keychain.list(id), rt.keychain.backend_name()))
}

pub(super) fn put(core: &mut Core, id: &str, name: &str, body: &Value) -> Result<Value, ApiError> {
    {
        let rt = core.rt(id)?;
        let org = body["org"].as_bool() == Some(true);
        if blocked_in_org(rt.ws.kind, rt.policy.as_ref(), org) {
            return Err(ApiError::forbidden("Personal keys can’t be used in this organization"));
        }
    }
    let rt = core.rt_mut(id)?;
    rt.keychain.set(id, &normalize_name(name), body["value"].as_str().unwrap_or_default()).map_err(|e| ApiError::new(500, e))?;
    rt.audit("secret.set", name, "you");
    Ok(json!({ "ok": true }))
}

pub(super) fn delete(core: &mut Core, id: &str, name: &str) -> Result<Value, ApiError> {
    let rt = core.rt_mut(id)?;
    // Same as the TS: the delete route uses the name as given, no normalisation.
    let _ = rt.keychain.delete(id, name);
    Ok(json!({ "ok": true }))
}
