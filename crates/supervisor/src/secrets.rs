//! Secret names (R-34): the API stores values in the keychain and never echoes them back.
//! Port of the `/secrets` routes' normalisation and org gate in `routes.ts`.

use workspaceai_shared::types::{OrgPolicy, WorkspaceKind};

/// `name.toUpperCase().replace(/[^A-Z0-9_]/g, '_')`, as the PUT route does.
pub fn normalize_name(name: &str) -> String {
    name.to_uppercase().chars().map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' { c } else { '_' }).collect()
}

/// An org that blocks personal keys refuses a value that is not marked `org: true`.
pub fn blocked_in_org(kind: WorkspaceKind, policy: Option<&OrgPolicy>, org: bool) -> bool {
    kind == WorkspaceKind::Org && policy.is_some_and(|p| p.block_personal_keys) && !org
}

/// What `GET /secrets` answers: names only, never values.
pub fn listing(names: Vec<String>, backend: &str) -> serde_json::Value {
    serde_json::json!({ "names": names, "backend": backend })
}
