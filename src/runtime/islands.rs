//! Islands implementation

use std::hash::Hasher;
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
#[derive(Debug, Clone)]
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

    /// Register an island descriptor
    pub fn register(&mut self, desc: IslandDescriptor) {
        let entry = IslandManifestEntry {
            name: desc.name.clone(),
            hash: format!("{:x}", std::collections::hash_map::DefaultHasher::new().finish()),
            props: desc.props_type.split(',').map(|s| s.trim().to_string()).collect(),
            module_path: desc.import_path,
        };
        self.islands.insert(desc.name, entry);
    }

    /// Get an island by name
    pub fn get(&self, name: &str) -> Option<&IslandManifestEntry> {
        self.islands.get(name)
    }

    /// Get all registered island names
    #[allow(dead_code)]
    pub fn names(&self) -> Vec<&String> {
        self.islands.keys().collect()
    }

    /// Check if an island is registered
    #[allow(dead_code)]
    pub fn contains(&self, name: &str) -> bool {
        self.islands.contains_key(name)
    }
}

impl Default for IslandManifest {
    fn default() -> Self {
        Self::new()
    }
}

pub mod signal_integration {
    #[allow(dead_code)]
    pub struct SignalIslandState;
}
