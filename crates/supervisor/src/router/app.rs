//! `/api/state` and `/api/settings`.

use crate::supervisor::{ApiError, Core};
use serde_json::Value;

fn json<T: serde::Serialize>(v: &T) -> Result<Value, ApiError> {
    serde_json::to_value(v).map_err(|e| ApiError::new(500, e.to_string()))
}

pub(super) fn state(core: &Core) -> Result<Value, ApiError> {
    json(&core.app_state())
}

/// Only the four app-level settings are writable here (`theme`, `accent`, `reduceMotion`, `followSystem`).
pub(super) fn patch_settings(core: &mut Core, body: &Value) -> Result<Value, ApiError> {
    let Some(obj) = body.as_object() else {
        return Err(ApiError::bad_request("Expected a JSON object"));
    };
    for key in ["theme", "accent", "reduceMotion", "followSystem"] {
        let Some(v) = obj.get(key) else { continue };
        match key {
            "theme" => {
                if let Some(s) = v.as_str() {
                    core.settings.theme = s.into();
                }
            }
            "accent" => {
                if let Some(s) = v.as_str() {
                    core.settings.accent = s.into();
                }
            }
            "reduceMotion" => {
                if let Some(b) = v.as_bool() {
                    core.settings.reduce_motion = b;
                }
            }
            _ => {
                if let Some(b) = v.as_bool() {
                    core.settings.follow_system = b;
                }
            }
        }
    }
    core.save_settings()?;
    state(core)
}
