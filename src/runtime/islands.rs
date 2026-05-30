//! Islands implementation

use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HydrationStrategy {
    Eager,
    Visible,
    Idle,
    Manual,
    Static,
}
impl Default for HydrationStrategy {
    fn default() -> Self {
        Self::Visible
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct IslandDescriptor {
    pub name: String,
    pub props_type: String,
    pub strategy: HydrationStrategy,
    pub import_path: String,
    pub ssr_capable: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct IslandInstance {
    pub name: String,
    pub props: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct IslandManifest {
    pub islands: std::collections::HashMap<String, IslandManifestEntry>,
}
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IslandManifestEntry {
    pub name: String,
    pub hash: String,
    pub props: Vec<String>,
    pub module_path: String,
}

#[allow(dead_code)]
impl IslandManifest {
    pub fn new() -> Self {
        Self {
            islands: std::collections::HashMap::new(),
        }
    }
    pub fn register(&mut self, _desc: IslandDescriptor) {}
    pub fn get(&self, _name: &str) -> Option<&IslandManifestEntry> {
        None
    }
}

pub mod signal_integration {
    #[allow(dead_code)]
    pub struct SignalIslandState;
}
