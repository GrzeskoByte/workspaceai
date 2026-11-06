//! App-level reads: workspace ordering, the rail note and `GET /api/state`.

use super::core::Core;
use crate::runtime::Runtime;
use workspaceai_shared::types::{AppState, WorkspaceKind, WorkspaceSummary};

impl Core {
    fn kind_rank(kind: WorkspaceKind) -> u8 {
        match kind {
            WorkspaceKind::Personal => 0,
            WorkspaceKind::Org => 1,
            WorkspaceKind::Team => 2,
        }
    }

    pub fn ordered(&self) -> Vec<&Runtime> {
        let mut out: Vec<&Runtime> = self.runtimes.values().collect();
        out.sort_by(|a, b| Self::kind_rank(a.ws.kind).cmp(&Self::kind_rank(b.ws.kind)).then(a.ws.created_at.cmp(&b.ws.created_at)));
        out
    }

    fn ws_note(&self, rt: &Runtime) -> String {
        let providers = match (&rt.policy, rt.ws.kind) {
            (Some(p), WorkspaceKind::Org) => &p.allowed_providers,
            _ => &rt.ws.providers,
        };
        let names: Vec<&str> = providers.iter().map(|p| p.name.as_str()).collect();
        let provs = if names.is_empty() { "no providers yet".to_string() } else { names.join(", ") };
        let mem = match rt.ws.memory {
            workspaceai_shared::types::MemoryBackend::Facts => "facts",
            workspaceai_shared::types::MemoryBackend::Graph => "Graphify",
            workspaceai_shared::types::MemoryBackend::Both => "Graphify + facts",
            workspaceai_shared::types::MemoryBackend::Mcp => "custom memory",
        };
        let connectors = rt.ws.mcp.iter().filter(|m| m.on).count();
        let residency = match (&rt.policy, rt.ws.kind) {
            (Some(p), WorkspaceKind::Org) => format!(" · {} data only", p.residency),
            _ => String::new(),
        };
        format!("{provs} · {connectors} connectors · {mem}{residency}")
    }

    pub fn app_state(&self) -> AppState {
        AppState {
            settings: self.settings.clone(),
            workspaces: self
                .ordered()
                .into_iter()
                .map(|r| WorkspaceSummary {
                    id: r.id.clone(),
                    name: r.ws.name.clone(),
                    kind: r.ws.kind,
                    initials: r.ws.initials.clone(),
                    accent: r.ws.accent.clone(),
                    note: self.ws_note(r),
                })
                .collect(),
            computers: self.computers.clone(),
        }
    }
}
