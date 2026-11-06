//! Organization workspaces (R-07, R-08, R-36): SSO join, signed policy refresh, org locks.

use super::core::{Core, CreateOptions, PolicyOverrides, Sso};
use super::events::ApiError;
use crate::jsonfile::{read_json, write_json};
use crate::policy::{accept_policy, demo_issuer, sign_policy};
use serde_json::{json, Value};
use workspaceai_shared::defaults::demo_org_policy;
use workspaceai_shared::types::{McpServer, McpType, Member, MemberRole, OrgInfo, OrgPolicy, WorkspaceKind};
use workspaceai_shared::util::now_ms;

impl Core {
    pub fn join_organization(&mut self, name: &str, sso: Sso, overrides: PolicyOverrides) -> Result<String, ApiError> {
        let issuer = demo_issuer(&self.home).map_err(ApiError::bad_request)?;
        let mut policy: OrgPolicy = demo_org_policy();
        policy.org = name.into();
        if let Some(v) = overrides.audit_url {
            policy.audit_url = Some(v);
        }
        if let Some(v) = overrides.audit_every_minutes {
            policy.audit_every_minutes = Some(v);
        }
        if let Some(v) = overrides.log_content {
            policy.log_content = v;
        }
        if let Some(v) = overrides.gateway {
            policy.gateway = Some(v);
        }
        let policy_value = serde_json::to_value(&policy).map_err(|e| ApiError::new(500, e.to_string()))?;
        let signed = sign_policy(&policy_value, &issuer.private_key, &issuer.key_id).map_err(ApiError::bad_request)?;
        let default_model = policy.allowed_models.first().cloned().unwrap_or_default();
        let id = self.create_workspace(name, WorkspaceKind::Org, CreateOptions { providers: Some(vec![]), default_model: Some(default_model) })?;

        let check = accept_policy(Some(&signed), &issuer.public_key, None);
        let Some(accepted) = check.policy else {
            return Err(ApiError::bad_request(check.reason.unwrap_or_else(|| "Policy rejected".into())));
        };
        if !check.ok {
            return Err(ApiError::bad_request("Policy rejected"));
        }

        let rt = self.rt_mut(&id)?;
        rt.policy = Some(accepted);
        let mut stored = signed;
        stored["pinnedKey"] = json!(issuer.public_key);
        write_json(&rt.paths.policy_json, &stored)?;
        rt.ws.org = Some(OrgInfo {
            issuer: sso.issuer,
            idp: sso.idp.clone(),
            subject: sso.subject.clone(),
            policy_version: policy.version.clone(),
            policy_verified: true,
            last_policy_fetch: now_ms(),
            ..Default::default()
        });
        rt.ws.web.mode = policy.web.mode;
        rt.ws.web.searxng_url = policy.web.searxng_url.clone();
        rt.ws.web.locked = policy.web.locked;
        rt.ws.mcp = policy.locked_mcp.clone();
        rt.ws.mcp.push(McpServer {
            id: "gitlab".into(),
            name: "GitLab".into(),
            kind: "MCP · http".into(),
            kind_type: McpType::Remote,
            url: Some("https://gitlab.[ORG-HOST]/api/mcp".into()),
            on: true,
            ..Default::default()
        });
        rt.ws.keep_chats = policy.retention;
        rt.ws.public_links = !policy.block_external_sharing;
        rt.ws.agents.retain(|a| policy.allowed_agents.contains(&a.id));
        if let Some(version) = &policy.opencode_version {
            rt.ws.engine.version = version.clone();
        }
        rt.ws.members = vec![
            Member { name: "You".into(), email: Some(sso.subject.clone()), role: MemberRole::Admin, ..Default::default() },
            Member { name: "[Member]".into(), role: MemberRole::Member, ..Default::default() },
            Member { name: "[Member]".into(), role: MemberRole::Viewer, ..Default::default() },
        ];
        rt.audit("sso.signin", &format!("{} via {}", sso.subject, sso.idp), &sso.subject);
        rt.audit("policy.accept", &format!("v{} · key {} · verified", policy.version, issuer.key_id), "workspaceai");
        rt.save()?;
        rt.write_config()?;
        Ok(id)
    }

    /// Re-fetch and verify the org policy (on a timer and at sign-in).
    pub fn refresh_policy(&mut self, id: &str, tamper: bool) -> Result<Value, ApiError> {
        if self.rt(id)?.ws.kind != WorkspaceKind::Org {
            return Ok(json!({ "ok": true }));
        }
        let issuer = demo_issuer(&self.home).map_err(ApiError::bad_request)?;
        let rt = self.rt_mut(id)?;
        let Some(stored) = read_json::<Value>(&rt.paths.policy_json) else {
            return Ok(json!({ "ok": false, "reason": "No policy on file" }));
        };
        let Some(policy) = stored.get("policy").cloned() else {
            return Ok(json!({ "ok": false, "reason": "No policy on file" }));
        };
        let key_id = stored.get("keyId").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let pinned = stored.get("pinnedKey").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let mut fresh = sign_policy(&policy, &issuer.private_key, &key_id).map_err(ApiError::bad_request)?;
        if tamper {
            fresh["policy"]["blockPersonalKeys"] = json!(false);
        }
        let check = accept_policy(Some(&fresh), &pinned, rt.policy.clone());
        rt.policy = check.policy.clone();
        if let Some(org) = &mut rt.ws.org {
            org.policy_verified = check.ok;
            org.last_policy_fetch = now_ms();
        }
        rt.audit("policy.refresh", if check.ok { "verified" } else { check.reason.as_deref().unwrap_or("failed") }, "workspaceai");
        rt.changed()?;
        Ok(match check.reason {
            Some(reason) => json!({ "ok": false, "reason": reason }),
            None => json!({ "ok": true }),
        })
    }

    /// Org rows are locked; anything outside policy is refused (R-36).
    pub fn assert_allowed(&self, rt: &crate::runtime::Runtime, what: &str) -> Result<(), ApiError> {
        if rt.ws.kind != WorkspaceKind::Org {
            return Ok(());
        }
        if ["providers", "agents", "web", "engine", "name", "data"].contains(&what) {
            rt.audit("policy.block", &format!("attempt to change {what}"), "workspaceai");
            return Err(ApiError::forbidden("Locked by org policy"));
        }
        Ok(())
    }

    pub fn send_allowed(&self, rt: &crate::runtime::Runtime) -> Result<(), ApiError> {
        if rt.ws.kind == WorkspaceKind::Org && rt.ws.org.as_ref().is_some_and(|o| !o.policy_verified) {
            return Err(ApiError::new(423, "Organization policy could not be verified; sending is blocked until it is."));
        }
        Ok(())
    }
}
