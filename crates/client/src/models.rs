//! The models a workspace offers (`GET /api/ws/:id/models`) and how a chat's model and effort read.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelOption {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelGroup {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub models: Vec<ModelOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct AgentModels {
    #[serde(default)]
    pub models: Vec<ModelOption>,
    #[serde(default)]
    pub current: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct ModelList {
    #[serde(default)]
    pub groups: Vec<ModelGroup>,
    #[serde(default)]
    pub agents: HashMap<String, AgentModels>,
}

impl ModelList {
    /// opencode chats choose among every connected provider's models; other agents bring their own.
    pub fn options_for(&self, agent: &str) -> Vec<ModelOption> {
        if agent == "opencode" {
            self.groups.iter().flat_map(|g| g.models.iter().cloned()).collect()
        } else {
            self.agents.get(agent).map(|a| a.models.clone()).unwrap_or_default()
        }
    }

    pub fn efforts_for(&self, agent: &str, model: &str) -> Vec<String> {
        self.options_for(agent).into_iter().find(|m| m.id == model).map(|m| m.efforts).unwrap_or_default()
    }

    /// "Opus · high", the model's id when it is not listed, or "Default model".
    pub fn label(&self, agent: &str, model: &str, effort: Option<&str>) -> String {
        let model = if model.is_empty() && agent != "opencode" { self.agents.get(agent).map_or("", |a| a.current.as_str()) } else { model };
        if model.is_empty() {
            return "Default model".into();
        }
        let name = self.options_for(agent).into_iter().find(|m| m.id == model).map(|m| m.name).filter(|n| !n.is_empty()).unwrap_or_else(|| model.to_string());
        match effort.filter(|e| !e.is_empty()) {
            Some(e) => format!("{name} · {e}"),
            None => name,
        }
    }
}
