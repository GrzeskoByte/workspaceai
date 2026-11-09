//! The models a workspace offers (`GET /api/ws/:id/models`), for the workspace and project default pickers.

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
    /// The default-model pickers list every connected provider's models for opencode; other agents bring their own.
    pub fn options_for(&self, agent: &str) -> Vec<ModelOption> {
        if agent == "opencode" {
            self.groups.iter().flat_map(|g| g.models.iter().cloned()).collect()
        } else {
            self.agents.get(agent).map(|a| a.models.clone()).unwrap_or_default()
        }
    }
}
